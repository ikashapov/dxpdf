//! §17.4.14 / §17.4.71 — the `<w:tblGrid>` must have a column for every cell a
//! row declares, and what happens when it does not.
//!
//! # The invariant, and the one place the spec leaves open
//!
//! §17.4.63 (`tblW`) and §17.4.71 (`tcW`) carry the same paragraph, word for
//! word: *"All widths in a table are considered preferred because: The table
//! **shall** satisfy the shared columns as specified by the `tblGrid` element …
//! Two or more widths can have conflicting values for the width of the same
//! grid column … The table layout algorithm can require a preference to be
//! overridden."* So the grid is the invariant, `tblW` and `tcW` are preferences
//! that may be overridden, and the spec names the conflict without resolving
//! it.
//!
//! What that leaves open is the **widths**, which is why nothing in this file
//! asserts that a `tcW` beats the grid slice it disagrees with. What it does
//! not leave open is the **seating**: a grid with fewer columns than a row has
//! cells cannot "satisfy the shared columns" for that row under any reading,
//! because there is no column for the last cell to sit in. Such a file is
//! self-contradictory and a renderer has to decide what gives.
//!
//! This file pins the half that must never move — a grid that *can* seat every
//! cell is scaled proportionally and nothing else happens to it. These are the
//! trap-detector for the repair in `build/table.rs::seat_every_cell`, which is
//! gated strictly on a grid too short to seat some row: if that gate ever
//! widens, these fail first. Seating counts exactly what the grid walk counts
//! (§17.4.17 `gridBefore`, §17.4.18 `gridSpan`, §17.4.16 `gridAfter`), so each
//! of those is pinned here as seating a grid rather than needing repair.

use std::io::Write;

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};

fn make_docx(document_xml: &str) -> Vec<u8> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", o).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml"
    ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    )
    .unwrap();

    zip.start_file("_rels/.rels", o).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1"
    Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"
    Target="word/document.xml"/>
</Relationships>"#,
    )
    .unwrap();

    zip.start_file("word/document.xml", o).unwrap();
    zip.write_all(document_xml.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

/// A bordered table with the given `<w:tblW>`, `<w:gridCol>` list and rows.
///
/// No `<w:sectPr>`, so the page is the §17.6.13 default Letter with 1-inch
/// margins: 612 pt wide, text column 72…540. No styles part either, so no
/// `TableNormal` cell margin — a cell's drawn extent is its column.
pub fn table_doc(tbl_w: &str, grid_cols: &str, rows: &str) -> Vec<u8> {
    make_docx(&format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:tbl>
      <w:tblPr>
        {tbl_w}
        <w:tblLayout w:type="fixed"/>
        <w:tblBorders>
          <w:top w:val="single" w:sz="4" w:color="000000"/>
          <w:left w:val="single" w:sz="4" w:color="000000"/>
          <w:bottom w:val="single" w:sz="4" w:color="000000"/>
          <w:right w:val="single" w:sz="4" w:color="000000"/>
          <w:insideH w:val="single" w:sz="4" w:color="000000"/>
          <w:insideV w:val="single" w:sz="4" w:color="000000"/>
        </w:tblBorders>
      </w:tblPr>
      <w:tblGrid>{grid_cols}</w:tblGrid>
      {rows}
    </w:tbl>
  </w:body>
</w:document>"#
    ))
}

/// `<w:gridCol>`s from a list of twip widths.
pub fn grid(widths: &[i32]) -> String {
    widths
        .iter()
        .map(|w| format!(r#"<w:gridCol w:w="{w}"/>"#))
        .collect()
}

/// One `<w:tc>` labelled `text`, optionally carrying `w:tcW` and `w:gridSpan`.
pub fn cell(text: &str, tcw: Option<(i32, &str)>, span: Option<i32>) -> String {
    let w = match tcw {
        Some((v, t)) => format!(r#"<w:tcW w:w="{v}" w:type="{t}"/>"#),
        None => String::new(),
    };
    let s = match span {
        Some(n) => format!(r#"<w:gridSpan w:val="{n}"/>"#),
        None => String::new(),
    };
    format!(r#"<w:tc><w:tcPr>{w}{s}</w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"#)
}

pub fn row(cells: &str) -> String {
    format!("<w:tr>{cells}</w:tr>")
}

pub fn row_with(tr_pr: &str, cells: &str) -> String {
    format!("<w:tr><w:trPr>{tr_pr}</w:trPr>{cells}</w:tr>")
}

pub fn layout(bytes: &[u8]) -> Vec<LayoutedPage> {
    let parsed = dxpdf::docx::parse(bytes).expect("parse");
    dxpdf::render::resolve_and_layout(parsed).1
}

/// Every cell's drawn `(x, width)` in the table's first row, in cell order.
///
/// Read off the horizontal border rects at the table's top edge: each cell
/// draws exactly one, spanning that cell's column slice. That makes a cell the
/// grid could not seat directly visible as a `width` of 0 rather than something
/// inferred from where its text landed.
pub fn first_row_cells(pages: &[LayoutedPage]) -> Vec<(f32, f32)> {
    let rects: Vec<(f32, f32, f32)> = pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            // Horizontal edges only: a border rect is 0.5 pt thick one way.
            DrawCommand::Rect { rect, .. } if rect.size.height.raw() <= 1.0 => Some((
                rect.origin.y.raw(),
                rect.origin.x.raw(),
                rect.size.width.raw(),
            )),
            _ => None,
        })
        .collect();
    let top = rects
        .iter()
        .map(|(y, _, _)| *y)
        .fold(f32::INFINITY, f32::min);
    rects
        .iter()
        .filter(|(y, _, _)| (*y - top).abs() < 0.01)
        .map(|(_, x, w)| (*x, *w))
        .collect()
}

/// Assert `got` matches `want` as `(x, width)` pairs, to 0.01 pt.
pub fn assert_cells(got: &[(f32, f32)], want: &[(f32, f32)], what: &str) {
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: expected {} cells, drew {}: {got:?}",
        want.len(),
        got.len()
    );
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g.0 - w.0).abs() < 0.01 && (g.1 - w.1).abs() < 0.01,
            "{what}: cell {i} drawn at x={:.2} w={:.2}, expected x={:.2} w={:.2} — all: {got:?}",
            g.0,
            g.1,
            w.0,
            w.1
        );
    }
}

// ── The half that must never move ────────────────────────────────────────────

/// §17.4.14: a grid with one column per cell is scaled to the declared `tblW`
/// and otherwise used as declared. 500/1000/1500 twips are one, two and three
/// sixths of the grid, and the grid sums to 3000 against a declared `tblW` of
/// 6000 — so the scale factor is 2 and the 300 pt table's columns come out at
/// 50, 100 and 150 pt from the 72 pt left margin.
///
/// The grid deliberately sums to *half* the declared width rather than to it.
/// With `sum(grid) == tblW` the scale factor is 1, and a mutation deleting the
/// scaling entirely still passes — which is exactly what the mutation check
/// caught when this test was first written that way.
#[test]
fn a_grid_that_seats_every_cell_is_scaled_proportionally() {
    let pages = layout(&table_doc(
        r#"<w:tblW w:w="6000" w:type="dxa"/>"#,
        &grid(&[500, 1000, 1500]),
        &row(&format!(
            "{}{}{}",
            cell("a", None, None),
            cell("b", None, None),
            cell("c", None, None)
        )),
    ));

    assert_cells(
        &first_row_cells(&pages),
        &[(72.0, 50.0), (122.0, 100.0), (222.0, 150.0)],
        "declared grid scaled to tblW",
    );
}

/// §17.4.18: a `gridSpan` cell occupies that many grid columns, so two cells
/// spanning 1 and 2 seat a three-column grid exactly and nothing is appended.
#[test]
fn grid_span_counts_toward_seating() {
    let pages = layout(&table_doc(
        r#"<w:tblW w:w="6000" w:type="dxa"/>"#,
        &grid(&[2000, 2000, 2000]),
        &row(&format!(
            "{}{}",
            cell("a", None, None),
            cell("b", None, Some(2))
        )),
    ));

    // 300 pt over three equal columns: 100 pt each, and the span-2 cell is 200.
    assert_cells(
        &first_row_cells(&pages),
        &[(72.0, 100.0), (172.0, 200.0)],
        "gridSpan seats the grid",
    );
}

/// §17.4.17 / §17.4.16: `gridBefore` and `gridAfter` occupy grid columns that
/// hold no cell, and both count toward seating — one cell plus one leading and
/// two trailing skips seat a four-column grid.
#[test]
fn grid_before_and_after_count_toward_seating() {
    let pages = layout(&table_doc(
        r#"<w:tblW w:w="8000" w:type="dxa"/>"#,
        &grid(&[2000, 2000, 2000, 2000]),
        &row_with(
            r#"<w:gridBefore w:val="1"/><w:gridAfter w:val="2"/>"#,
            &cell("a", None, None),
        ),
    ));

    // 400 pt over four equal columns: 100 pt each, and gridBefore=1 puts the
    // only cell in column 1, so it is drawn from x = 72 + 100.
    assert_cells(
        &first_row_cells(&pages),
        &[(172.0, 100.0)],
        "gridBefore offsets the cell and seats the grid",
    );
}

/// A grid with *more* columns than a row uses still seats every cell in that
/// row — the row simply ends early, which is what `gridAfter` says explicitly
/// and what a producer omitting it leaves implicit.
///
/// Pinned because it is the shape most easily confused with the one the repair
/// exists for, and it is deliberately **not** repaired: every cell has a
/// column, so the seating invariant holds and there is nothing
/// self-contradictory to fix. Note the 4000-twip `tcW` on each cell disagrees
/// with its 2000-twip grid column and is ignored — that is §17.4.71's
/// unresolved conflict, and resolving it needs a **Word reference render**, not
/// this repair.
#[test]
fn a_row_shorter_than_the_grid_is_left_short() {
    let pages = layout(&table_doc(
        r#"<w:tblW w:w="8000" w:type="dxa"/>"#,
        &grid(&[2000, 2000, 2000, 2000]),
        &row(&format!(
            "{}{}",
            cell("a", Some((4000, "dxa")), None),
            cell("b", Some((4000, "dxa")), None)
        )),
    ));

    assert_cells(
        &first_row_cells(&pages),
        &[(72.0, 100.0), (172.0, 100.0)],
        "a short row keeps its grid columns and stops",
    );
}
