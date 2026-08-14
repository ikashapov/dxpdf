#!/usr/bin/env python3
"""Build test-files/bidi-visual-table.docx — §17.4.1 `w:bidiVisual`, the
right-to-left table, next to the same table without it.

`w:bidiVisual` says a table's columns run right to left: the first cell of a row
is the rightmost one. The engine implements it by rewriting the table's layout
input into visual order once, at the build seam, so everything downstream keeps
seeing an ordinary left-to-right table (`build::table::mirror_columns`).

The document holds **two tables that differ only in that element**, which is what
makes the fixture readable without measuring anything: whatever the host's fonts
do, they do it to both, so every claim is the second table being the first one
reflected about its own edges.

Both tables carry, in one grid, each feature that has to travel with the flip:

  * an **unequal** grid (1000/2000/3000 twips). Three equal columns cannot tell
    a correct mirror from one that reverses the cells and leaves the slot widths
    where they were — with these, cell A is 50pt wide wherever it lands.
  * `w:gridSpan` — row 2's first cell covers two columns, so the run of slots it
    covers has to mirror as a run.
  * `w:gridBefore` — row 3 skips its first column, and the gap has to come out on
    the right.
  * `w:vMerge` — rows 4 and 5 merge their first column, which has to end up
    merged in the mirrored column.
  * `w:tcBorders/w:left` on cell A — the logical *start* edge, which has to paint
    on the visual right. Transitional `w:left` is Strict's `w:start`, which is
    why this is the reading and not a guess.

Cell labels are stable and unique (A…) so a test or a reader can name a cell
rather than an index. Shading is per-cell and distinct so each cell's box can be
picked off the rendered page by colour.

The text is Hebrew because that is what an RTL table is for, and because a
document that mirrors its columns while its runs stay LTR would look right for
the wrong reason. The paragraphs carry `w:bidi` so the cell content is RTL too;
`w:bidiVisual` governs the columns and nothing else, and keeping the two
independent is the point of setting both here.

Run `scripts/verify_docx.py` on the result before committing — this parser is
tolerant enough to read packages Word refuses to open.

Regenerate and commit the result if the content changes; the build is
deterministic and needs no third-party packages.

    scripts/make_bidi_visual_fixture.py
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

#: The three declared columns, deliberately unequal — see the module docstring.
GRID = (
    '<w:gridCol w:w="1000"/><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/>'
)

#: A 3pt border on the cell's logical *start* edge, and nothing on the other
#: three, so which side it paints on is unambiguous.
START_BORDER = (
    "<w:tcBorders>"
    '<w:left w:val="single" w:sz="24" w:space="0" w:color="C00000"/>'
    '<w:top w:val="nil"/><w:bottom w:val="nil"/><w:right w:val="nil"/>'
    "</w:tcBorders>"
)


def cell(label: str, fill: str, extra: str = "") -> str:
    """One shaded, labelled cell. `w:bidi` makes the paragraph itself RTL."""
    return (
        "<w:tc>"
        f'<w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>{extra}</w:tcPr>'
        "<w:p><w:pPr><w:bidi/></w:pPr>"
        f'<w:r><w:rPr><w:rtl/></w:rPr><w:t xml:space="preserve">{label} שלום</w:t></w:r>'
        "</w:p></w:tc>"
    )


def rows() -> str:
    """The five rows, identical in both tables."""
    return "".join(
        [
            # 1 — three plain cells: the mirror itself, and the unequal widths.
            "<w:tr>"
            + cell("A", "F8CBAD", START_BORDER)
            + cell("B", "C6E0B4")
            + cell("C", "BDD7EE")
            + "</w:tr>",
            # 2 — a span across the first two columns.
            "<w:tr>"
            + cell("D", "FFE699", '<w:gridSpan w:val="2"/>')
            + cell("E", "D9D2E9")
            + "</w:tr>",
            # 3 — one column skipped at the row's logical start.
            "<w:tr>"
            + '<w:trPr><w:gridBefore w:val="1"/></w:trPr>'
            + cell("F", "F4CCCC", '<w:gridSpan w:val="2"/>')
            + "</w:tr>",
            # 4/5 — a vertical merge down the first column.
            "<w:tr>"
            + cell("G", "D0E0E3", '<w:vMerge w:val="restart"/>')
            + cell("H", "EAD1DC")
            + cell("I", "FFF2CC")
            + "</w:tr>",
            "<w:tr>"
            + cell("", "D0E0E3", "<w:vMerge/>")
            + cell("J", "D9EAD3")
            + cell("K", "CFE2F3")
            + "</w:tr>",
        ]
    )


def table(bidi: bool) -> str:
    flag = "<w:bidiVisual/>" if bidi else ""
    return (
        "<w:tbl><w:tblPr>"
        '<w:tblW w:w="6000" w:type="dxa"/>'
        '<w:tblLayout w:type="fixed"/>'
        '<w:tblBorders>'
        '<w:top w:val="single" w:sz="8" w:space="0" w:color="808080"/>'
        '<w:bottom w:val="single" w:sz="8" w:space="0" w:color="808080"/>'
        '<w:insideH w:val="single" w:sz="8" w:space="0" w:color="808080"/>'
        '<w:insideV w:val="single" w:sz="8" w:space="0" w:color="808080"/>'
        '<w:left w:val="nil"/><w:right w:val="nil"/>'
        "</w:tblBorders>"
        f"{flag}"
        "</w:tblPr>"
        f"<w:tblGrid>{GRID}</w:tblGrid>{rows()}</w:tbl>"
    )


def heading(text: str) -> str:
    return f'<w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p>'


DOCUMENT = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {heading("Control: no w:bidiVisual - column A is leftmost")}
    {table(False)}
    <w:p/>
    {heading("w:bidiVisual - the same table, column A is rightmost")}
    {table(True)}
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
    target = OUT / "bidi-visual-table.docx"
    # Fixed timestamps so regenerating an unchanged fixture produces identical
    # bytes and does not show up as a diff.
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in (
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("word/document.xml", DOCUMENT),
        ):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    print(f"wrote {target.relative_to(ROOT)} ({target.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
