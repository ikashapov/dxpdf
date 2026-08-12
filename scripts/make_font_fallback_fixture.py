#!/usr/bin/env python3
"""Build test-files/issue-139-minimal.docx — the reproduction from issue #139.

One paragraph, and deliberately **no font named anywhere**: no `w:rFonts` on the
run, no `w:docDefaults`, no styles part at all. That is the whole point of the
fixture. The run therefore resolves to the §17.7.2 spec fallback, and the
engine has nothing to go on but a family name that cannot draw most of the
text.

The text mixes four scripts against ASCII:

    ASCII ok / circled: ① / katakana: ア / hebrew: א / thai: ๑

`א` is in there as the control. Times New Roman happens to cover Hebrew, so it
rendered correctly even before per-glyph fallback existed — which is exactly
the point the issue makes about coverage being luck rather than resolution. A
fix that changed how `א` is drawn would be doing something wrong.

Deterministic: no timestamps, fixed ZIP metadata, so re-running produces a
byte-identical archive. Regenerate rather than hand-edit.

    python3 scripts/make_font_fallback_fixture.py
"""

import pathlib
import zipfile

OUT = pathlib.Path(__file__).resolve().parent.parent / "test-files" / "issue-139-minimal.docx"

CONTENT_TYPES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"""

RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"""

# The four non-ASCII scripts are written as numeric character references so this
# script stays pure ASCII and the bytes it emits cannot depend on the editor
# that saved it.
DOCUMENT = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
<w:p><w:r><w:t xml:space="preserve">ASCII ok / circled: &#x2460; / katakana: &#x30A2; / hebrew: &#x05D0; / thai: &#x0E51;</w:t></w:r></w:p>
<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>
</w:body>
</w:document>"""

PARTS = [
    ("[Content_Types].xml", CONTENT_TYPES),
    ("_rels/.rels", RELS),
    ("word/document.xml", DOCUMENT),
]


def main() -> None:
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as zf:
        for name, body in PARTS:
            # Fixed date_time so the archive is reproducible.
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            zf.writestr(info, body)
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
