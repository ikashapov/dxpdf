#!/usr/bin/env python3
"""Build test-files/hidden-text.docx — §17.3.2 `w:vanish` in every position that
resolves differently.

Word hides a run marked `w:vanish` and closes the surrounding text up around it.
This engine painted it, which is the worst direction for the error to run in:
content an author marked invisible appeared in the PDF.

Every paragraph pairs a marker that must survive with one that must not, so a
test can assert on the rendered text without depending on measurement:

  1. two adjacent visible runs, nothing hidden        → the control
  2. a hidden run between two visible ones            → same geometry as 1
  3. a character style that hides                     → no SECRET
  4. the same style, overridden by `w:val="0"`        → VISIBLE
  5. every run hidden                                 → an empty line, not a
                                                        vanished paragraph
  6. a hidden run carrying a tab and a break          → neither survives it
  7. a hidden `w:sym`                                 → **still drawn**, the
                                                        known limit

Paragraph 1 is what makes paragraph 2 checkable without measuring anything: the
two carry the same visible runs and differ only in the hidden one between them,
so their second run must land at the same x. Hiding that reserved the run's
width instead of removing it would move one and not the other.

Case 7 is the boundary rather than a bug in the filter: the parser flushes a
run's `w:sym` / `w:drawing` / `w:pict` children into sibling inlines of their
own, and those model types carry no run properties, so the `w:vanish` that
governed them is gone before layout sees them. Closing it means carrying the
run's `w:rPr` onto those inlines — a model change, tracked separately.

`SECRET` is the only string that must never reach the page, so a test can assert
its absence globally rather than per paragraph.

Regenerate and commit the result if the content changes; the build is
deterministic and needs no third-party packages.

    scripts/make_hidden_text_fixture.py
"""

import pathlib
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "test-files"

CONTENT_TYPES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>
"""

ROOT_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>
"""

DOC_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>
"""

#: A character style whose whole content is `w:vanish` — case 3's mechanism, and
#: case 4's, which turns it back off run-side.
STYLES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="character" w:styleId="SecretChar">
    <w:name w:val="Secret Char"/>
    <w:rPr><w:vanish/></w:rPr>
  </w:style>
</w:styles>
"""


def run(text: str, rpr: str = "") -> str:
    return f'<w:r>{rpr}<w:t xml:space="preserve">{text}</w:t></w:r>'


def para(*runs: str) -> str:
    return "<w:p>" + "".join(runs) + "</w:p>"


VANISH = "<w:rPr><w:vanish/></w:rPr>"
UNVANISH = '<w:rPr><w:rStyle w:val="SecretChar"/><w:vanish w:val="0"/></w:rPr>'
STYLED = '<w:rPr><w:rStyle w:val="SecretChar"/></w:rPr>'

BODY = "".join(
    [
        # 1 — the control: two adjacent visible runs and nothing hidden.
        para(run("VISIBLE"), run("VISIBLE")),
        # 2 — the same, with a hidden run between them. The two VISIBLE markers
        #     must land exactly where paragraph 1's do, which is the difference
        #     between hiding a run and drawing nothing at its reserved width.
        para(run("VISIBLE"), run("SECRET", VANISH), run("VISIBLE")),
        # 3 — hidden by the character style alone.
        para(run("SECRET", STYLED)),
        # 4 — same style, un-hidden run-side. `w:val="0"` has to outrank it.
        para(run("VISIBLE", UNVANISH)),
        # 5 — every run hidden. The paragraph mark is not, so the paragraph
        #     still occupies a line; it does not disappear.
        para(run("SECRET", VANISH), run("SECRET", VANISH)),
        # 6 — a hidden run's tab and break go with it.
        para(
            run("VISIBLE"),
            f"<w:r>{VANISH}<w:tab/><w:t>SECRET</w:t><w:br/></w:r>",
            run("VISIBLE"),
        ),
        # 7 — the known limit: a `w:sym` in a hidden run keeps drawing, because
        #     the model drops the run's properties on the way to `Inline::Symbol`.
        #     Wingdings F0FC is the check mark; any host face may substitute, so
        #     tests assert that *a* fragment survives, never which glyph.
        para(
            run("VISIBLE"),
            f'<w:r>{VANISH}<w:sym w:font="Wingdings" w:char="F0FC"/></w:r>',
        ),
    ]
)

DOCUMENT = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {BODY}
    <w:sectPr>
      <w:pgSz w:w="12240" w:h="15840"/>
      <w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"
               w:header="720" w:footer="720" w:gutter="0"/>
    </w:sectPr>
  </w:body>
</w:document>
"""


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    target = OUT / "hidden-text.docx"
    # Fixed timestamps so regenerating an unchanged fixture produces identical
    # bytes and does not show up as a diff.
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in (
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", DOC_RELS),
            ("word/document.xml", DOCUMENT),
            ("word/styles.xml", STYLES),
        ):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    print(f"wrote {target.relative_to(ROOT)} ({target.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
