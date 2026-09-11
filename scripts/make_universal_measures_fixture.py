#!/usr/bin/env python3
"""Build test-files/universal-measures.docx — every measurement spelled as a
§22.9.2.15 universal measure or a §22.9.2.9 percentage instead of a bare
number.

Word writes these spellings when a document is saved as Strict Open XML
(`<w:pgSz w:w="595.30pt"/>`), and Transitional's measurement types admit them
equally — ST_TwipsMeasure, ST_SignedTwipsMeasure, ST_HpsMeasure and
ST_MeasurementOrPercent each union a universal measure in. A converter that
reads only the bare spelling rejects the whole document over its page size,
which is how the gap was found: a service fed a Strict export failed every
conversion with "expected an integer or decimal measurement".

One of each target unit, so a wrong per-unit ratio fails its own assertion
rather than everything at once:

  - `w:pgSz` / `w:pgMar` — twips targets, spelled in pt / cm / in
  - `w:sz w:val="14pt"`  — a half-point target (28 half-points)
  - `w:tblW w:w="297.65pt" w:type="dxa"` — twips through ST_MeasurementOrPercent
  - `w:tblW w:w="50%" w:type="pct"` — the percentage spelling (2500 fiftieths)

The A4 geometry is exact on purpose: 595.3pt × 20 = 11906 twips even, so the
assertion pins the conversion with no rounding tolerance hiding in it.

Regenerate and commit the result if the content changes; the build is
deterministic and needs no third-party packages.

    scripts/make_universal_measures_fixture.py
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
</Types>
"""

ROOT_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>
"""

DOCUMENT = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r>
        <w:rPr><w:sz w:val="14pt"/></w:rPr>
        <w:t>Fourteen points, spelled with its unit.</w:t>
      </w:r>
    </w:p>
    <w:tbl>
      <w:tblPr>
        <w:tblW w:w="297.65pt" w:type="dxa"/>
        <w:tblLayout w:type="fixed"/>
      </w:tblPr>
      <w:tblGrid><w:gridCol w:w="5953"/></w:tblGrid>
      <w:tr><w:tc>
        <w:tcPr><w:tcW w:w="297.65pt" w:type="dxa"/></w:tcPr>
        <w:p><w:r><w:t>dxa spelled in points</w:t></w:r></w:p>
      </w:tc></w:tr>
    </w:tbl>
    <w:p/>
    <w:tbl>
      <w:tblPr>
        <w:tblW w:w="50%" w:type="pct"/>
      </w:tblPr>
      <w:tblGrid><w:gridCol w:w="4820"/></w:tblGrid>
      <w:tr><w:tc>
        <w:tcPr><w:tcW w:w="50%" w:type="pct"/></w:tcPr>
        <w:p><w:r><w:t>pct spelled as a percentage</w:t></w:r></w:p>
      </w:tc></w:tr>
    </w:tbl>
    <w:p/>
    <w:sectPr>
      <w:pgSz w:w="595.30pt" w:h="841.90pt"/>
      <w:pgMar w:top="2cm" w:right="1.5cm" w:bottom="2cm" w:left="1in"
               w:header="35.40pt" w:footer="35.40pt" w:gutter="0pt"/>
    </w:sectPr>
  </w:body>
</w:document>
"""


def build() -> None:
    path = OUT / "universal-measures.docx"
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CONTENT_TYPES)
        z.writestr("_rels/.rels", ROOT_RELS)
        z.writestr("word/document.xml", DOCUMENT)
    print(f"wrote {path}")


if __name__ == "__main__":
    build()
