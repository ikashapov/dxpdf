#!/usr/bin/env python3
"""Build test-files/shading-phase-probe.docx — a probe, not a fixture.

PR #180's review flagged §17.18.78 pattern geometry as phase-*local*: every
stripe/cross family anchors its tile to the shaded box's own top-left corner
(`layout::shading::horizontal`/`vertical`/`diagonal`), never to anything wider.
Two open questions follow, and this file asks both without answering either:

- **Table A** (`vertStripe`) and **Table B** (`diagCross`), four identically
  shaded cells in one row each — does Word tile the pattern as one continuous
  grid across the row, or does each cell restart its own tile at its own left
  edge?
- **Table C** (`horzStripe`, one cell stuffed with 70 filler paragraphs) —
  forces a §17.4.6 row split across a page boundary. At the cut, does the
  tile's phase continue from where the first page left off, or restart flush
  with the continuation's own top?

**Not yet measured.** No Word render has answered either question, so nothing
in `layout::shading` assumes a fixed origin — it stays box-local until one
does. Answering this needs a real Word render, not reasoning from the spec:
ECMA-376 says nothing about pattern-fill phase at all, and this codebase's own
convention (see the border-geometry and diagonal-slope entries in AGENTS.md)
is to pin tile/stroke geometry only from measured Word output. Once measured,
fold the answer into `layout::shading`, add the assertions this file is
missing, and update its entry in AGENTS.md's fixture table to say what was
found instead of "not yet measured".
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

COLW = 2000
N_COLS = 4
FILLER_LINES = 70


def cell(width, shd=None, text="X"):
    shd_xml = ""
    if shd:
        val, color, fill = shd
        shd_xml = f'<w:shd w:val="{val}" w:color="{color}" w:fill="{fill}"/>'
    return f"""<w:tc>
  <w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>{shd_xml}</w:tcPr>
  <w:p><w:r><w:t>{text}</w:t></w:r></w:p>
</w:tc>"""


def four_cell_row(shd):
    cols = "".join(f'<w:gridCol w:w="{COLW}"/>' for _ in range(N_COLS))
    cells = "".join(cell(COLW, shd, f"Cell {i + 1}") for i in range(N_COLS))
    borders = """<w:tblBorders>
      <w:top w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:bottom w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:left w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:right w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:insideH w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:insideV w:val="single" w:sz="4" w:space="0" w:color="999999"/>
    </w:tblBorders>"""
    return f"""<w:tbl>
  <w:tblPr>
    <w:tblW w:w="{COLW * N_COLS}" w:type="dxa"/>
    <w:tblLayout w:type="fixed"/>
    {borders}
  </w:tblPr>
  <w:tblGrid>{cols}</w:tblGrid>
  <w:tr><w:trPr><w:trHeight w:val="1600" w:hRule="atLeast"/></w:trPr>{cells}</w:tr>
</w:tbl>"""


def row_split_table():
    para = (
        "<w:p><w:r><w:t>Row-split phase probe line {}. "
        "The quick brown fox jumps over the lazy dog.</w:t></w:r></w:p>"
    )
    filler = "".join(para.format(i) for i in range(FILLER_LINES))
    return f"""<w:tbl>
  <w:tblPr>
    <w:tblW w:w="8000" w:type="dxa"/>
    <w:tblLayout w:type="fixed"/>
    <w:tblBorders>
      <w:top w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:bottom w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:left w:val="single" w:sz="4" w:space="0" w:color="999999"/>
      <w:right w:val="single" w:sz="4" w:space="0" w:color="999999"/>
    </w:tblBorders>
  </w:tblPr>
  <w:tblGrid><w:gridCol w:w="8000"/></w:tblGrid>
  <w:tr>
    <w:tc>
      <w:tcPr>
        <w:tcW w:w="8000" w:type="dxa"/>
        <w:shd w:val="horzStripe" w:color="000000" w:fill="FFFF00"/>
      </w:tcPr>
      {filler}
    </w:tc>
  </w:tr>
</w:tbl>"""


def build():
    table_a = four_cell_row(("vertStripe", "000000", "FFFFFF"))
    table_b = four_cell_row(("diagCross", "990000", "FFFFCC"))
    table_c = row_split_table()

    body = f"""
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Table A -- vertStripe, four cells, one row (same colour/pattern).</w:t></w:r></w:p>
<w:p><w:r><w:t>Question: do the vertical stripes form ONE continuous grid across all four cells, or does each cell restart its own stripe pattern at its own left edge?</w:t></w:r></w:p>
{table_a}
<w:p><w:r><w:t xml:space="preserve"> </w:t></w:r></w:p>
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Table B -- diagCross, four cells, one row (same colour/pattern).</w:t></w:r></w:p>
<w:p><w:r><w:t>Question: same as above, on both axes at once -- one continuous diamond lattice, or four separate patches?</w:t></w:r></w:p>
{table_b}
<w:p><w:r><w:t xml:space="preserve"> </w:t></w:r></w:p>
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Table C -- horzStripe, one cell, stuffed with text to force a page-split row.</w:t></w:r></w:p>
<w:p><w:r><w:t>Question: at the page boundary where this row's single cell splits, does the horizontal stripe pattern continue its phase from the bottom of the previous page, or restart flush with the top of the continuation?</w:t></w:r></w:p>
{table_c}
"""

    document_xml = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {body}
    <w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>
  </w:body>
</w:document>
"""

    OUT.mkdir(parents=True, exist_ok=True)
    out_path = OUT / "shading-phase-probe.docx"
    with zipfile.ZipFile(out_path, "w") as z:
        z.writestr("[Content_Types].xml", CONTENT_TYPES)
        z.writestr("_rels/.rels", ROOT_RELS)
        z.writestr("word/document.xml", document_xml)
    print(f"wrote {out_path}")


if __name__ == "__main__":
    build()
