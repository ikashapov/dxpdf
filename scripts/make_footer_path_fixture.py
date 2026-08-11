#!/usr/bin/env python3
"""Build test-files/footer-path-wrap.docx — an unbreakable token in a narrow cell.

The shape comes from a real report (`VE_Anlagenfreigabe_…docx`): a three-column
table in the page footer whose right-hand cell carries the template's disk path,
right-aligned in 6 pt Arial. Reduced to what makes it a hard case, and nothing
of the original document survives here but the geometry.

Why a Windows path is the interesting token. UAX #14 gives it exactly *one*
break opportunity — after the `:` in `"Z:` — and none at all thereafter: `\\` is
class PR, and [LB24] (`PR × AL`) and [LB25] (`PR × NU`) both forbid breaking
after it, so the remaining ninety-odd characters are one token that no rule may
cut. That is the combination the fitter got wrong: an early opportunity, then a
run wider than the line with nothing legal behind it. On overflow it rewound to
the opportunity but resumed measuring at the fragment that overflowed, so
everything in between was painted onto the new line without being counted into
its width — one 295 pt line in a 167.80 pt cell, 91 pt past the edge of the
page.

The cell is deliberately narrower than the token by a wide margin, so the
assertion in tests/footer_path_wrap.rs does not depend on which font the host
substitutes for Arial: any face at 6 pt puts this path well past 167 pt.

[LB24]: https://www.unicode.org/reports/tr14/#LB24
[LB25]: https://www.unicode.org/reports/tr14/#LB25

Regenerate and commit the result if the text changes; the build is
deterministic and needs no third-party packages.

    scripts/make_footer_path_fixture.py
"""

import pathlib
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "test-files"

#: The token under test. Backslash-separated, quoted, and long enough that no
#: plausible 6 pt face fits it in the cell below.
PATH = r'"Z:\10_Vorlagen\20_Vorlagen_Auftragsmanagment_QM\10_Formtastic\WAM_Bericht_Anlagenfreigabe_V03.docx"'

#: Three equal columns that fit the A4 text area below (3 x 3000 twips = 450 pt
#: against 467.4 pt of content). The original's columns were 3572 twips inside
#: narrower margins; what matters to the test is only that the cell is far
#: narrower than the token, and 150 pt against ~295 pt is that with room to
#: spare — so the assertion does not depend on which face the host substitutes.
COLUMN_TWIPS = 3000

CONTENT_TYPES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>
</Types>
"""

ROOT_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>
"""

DOC_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rIdF" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>
</Relationships>
"""

RPR = (
    '<w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:cs="Arial"/>'
    '<w:sz w:val="12"/><w:szCs w:val="12"/></w:rPr>'
)


def cell(text: str, align: str) -> str:
    """One table cell holding a single right- or left-aligned run."""
    return (
        "<w:tc>"
        f'<w:tcPr><w:tcW w:w="{COLUMN_TWIPS}" w:type="dxa"/></w:tcPr>'
        f'<w:p><w:pPr><w:jc w:val="{align}"/>{RPR}</w:pPr>'
        f"<w:r>{RPR}<w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>"
        "</w:tc>"
    )


def escape(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


FOOTER = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:tbl>
    <w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblLayout w:type="fixed"/></w:tblPr>
    <w:tblGrid>
      <w:gridCol w:w="{COLUMN_TWIPS}"/><w:gridCol w:w="{COLUMN_TWIPS}"/><w:gridCol w:w="{COLUMN_TWIPS}"/>
    </w:tblGrid>
    <w:tr>
      {cell("Left column", "left")}
      {cell("Middle column", "left")}
      {cell(escape(PATH), "right")}
    </w:tr>
  </w:tbl>
  <w:p/>
</w:ftr>
"""

DOCUMENT = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    <w:p><w:r><w:t>Body text. The footer below carries the token under test.</w:t></w:r></w:p>
    <w:sectPr>
      <w:footerReference w:type="default" r:id="rIdF"/>
      <w:pgSz w:w="11907" w:h="16840"/>
      <w:pgMar w:top="1418" w:right="1134" w:bottom="1134" w:left="1418"
               w:header="0" w:footer="0" w:gutter="0"/>
    </w:sectPr>
  </w:body>
</w:document>
"""


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    target = OUT / "footer-path-wrap.docx"
    # Fixed timestamps so regenerating an unchanged fixture produces identical
    # bytes and does not show up as a diff.
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in (
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", DOC_RELS),
            ("word/document.xml", DOCUMENT),
            ("word/footer1.xml", FOOTER),
        ):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    print(f"wrote {target.relative_to(ROOT)} ({target.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
