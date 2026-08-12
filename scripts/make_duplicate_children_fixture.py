#!/usr/bin/env python3
"""Rebuild `test-files/duplicate-children.docx`.

Every OOXML property bag is an `xsd:sequence` whose children carry
`maxOccurs="1"`, so repeating one makes the document schema-invalid. Real
producers do it anyway — LibreOffice/AOO emit redundant toggles like
`<w:b/><w:b/>`, and a duplicated `<w:tcMar>` inside one `<w:tcPr>` is what
motivated PR #146. Word opens all of it without complaint.

This fixture repeats one child in each bag the converter models, so a
regression that makes any of them fatal again shows up in
`tests/parse_test_files.rs` without anyone writing a new test.

The `<v:roundrect>` covers the same ground for VML, which was the last place
in the parser where a repeated child could fail a document — its common
children reached the model behind a `#[serde(flatten)]`, and serde cannot
collect repeated keys into a sequence across that boundary. See
`docx::parse::vml::schema::CommonAttrsXml`.

**Every duplicate here disagrees with itself**, deliberately: the two
occurrences carry different values, so the fixture pins *which* one wins
(the last) rather than merely proving the parse survived. `tests/
duplicate_children.rs` asserts the resolved values.

Deterministic — no timestamps, no rsids, fixed ZIP metadata — so rebuilding
produces byte-identical output and the committed file never churns.

Usage: python3 scripts/make_duplicate_children_fixture.py
"""

import pathlib
import zipfile

OUT = pathlib.Path(__file__).resolve().parent.parent / "test-files" / "duplicate-children.docx"

CONTENT_TYPES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>"""

ROOT_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"""

DOC_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"""

# A localized style name whose byte 4 falls inside a codepoint: `É`(2) `l`(1)
# `é`(2), so slicing at 4 splits `é`. This used to panic the whole library
# through `is_toc_entry_name`; it belongs in the same fixture because it is the
# other half of what PR #146 fixed.
STYLES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="ElementsDeStyle">
<w:name w:val="Éléments de style"/>
</w:style>
<w:style w:type="paragraph" w:styleId="Toc1"><w:name w:val="toc 1"/></w:style>
</w:styles>"""

# Each bag repeats one child, and the two occurrences disagree.
BODY = """
<w:p><w:pPr>
  <w:jc w:val="left"/><w:jc w:val="center"/>
  <w:ind w:left="100"/><w:ind w:left="1440"/>
</w:pPr><w:r><w:t>pPr: jc and ind repeated</w:t></w:r></w:p>

<w:p><w:pPr>
  <w:pBdr><w:top w:val="single" w:sz="4" w:space="1" w:color="auto"/></w:pBdr>
  <w:pBdr><w:bottom w:val="single" w:sz="4" w:space="1" w:color="auto"/></w:pBdr>
</w:pPr><w:r><w:t>pPr: pBdr repeated</w:t></w:r></w:p>

<w:p><w:r><w:rPr>
  <w:sz w:val="20"/><w:sz w:val="48"/>
  <w:color w:val="FF0000"/><w:color w:val="0000FF"/>
</w:rPr><w:t>rPr: sz and color repeated</w:t></w:r></w:p>

<w:tbl>
  <w:tblPr>
    <w:tblW w:w="5000" w:type="dxa"/>
    <w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders>
    <w:tblBorders><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders>
    <w:jc w:val="left"/><w:jc w:val="center"/>
  </w:tblPr>
  <w:tblGrid><w:gridCol w:w="5000"/></w:tblGrid>
  <w:tr>
    <w:trPr><w:trHeight w:val="200"/><w:trHeight w:val="900"/></w:trPr>
    <w:tc>
      <w:tcPr>
        <w:tcMar><w:top w:w="100" w:type="dxa"/></w:tcMar>
        <w:tcMar><w:bottom w:w="200" w:type="dxa"/></w:tcMar>
      </w:tcPr>
      <w:p><w:r><w:t>tblPr/trPr/tcPr repeated</w:t></w:r></w:p>
    </w:tc>
  </w:tr>
</w:tbl>

<w:p><w:pPr><w:pStyle w:val="ElementsDeStyle"/></w:pPr>
<w:r><w:t>a style name whose byte 4 splits a codepoint</w:t></w:r></w:p>

<w:p><w:r><w:pict>
  <v:roundrect id="rr" style="width:60pt;height:30pt" arcsize="0.2">
    <v:stroke dashstyle="dot"/><v:stroke dashstyle="dash"/>
    <v:fill type="solid" color="#ff0000"/><v:fill type="solid" color="#0000ff"/>
  </v:roundrect>
</w:pict></w:r></w:p>

<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>
<w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134"
         w:header="709" w:footer="709" w:gutter="0"/></w:sectPr>
"""

DOCUMENT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
    ' xmlns:v="urn:schemas-microsoft-com:vml">'
    f"<w:body>{BODY}</w:body></w:document>"
)

PARTS = {
    "[Content_Types].xml": CONTENT_TYPES,
    "_rels/.rels": ROOT_RELS,
    "word/_rels/document.xml.rels": DOC_RELS,
    "word/document.xml": DOCUMENT,
    "word/styles.xml": STYLES,
}


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
        for name, text in PARTS.items():
            # Fixed date_time so the archive is reproducible byte-for-byte.
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            z.writestr(info, text)
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
