//! §17.4.1 `w:bidiVisual` — a table whose columns run right to left.
//!
//! The element says the first cell of a row is the **rightmost** one. Everything
//! that follows from that is geometry: a `w:gridSpan` covers the mirrored run of
//! slots, a `w:gridBefore` leaves its gap at the visual right, a `w:vMerge`
//! spans the mirrored column, and a cell's logical start border paints on its
//! visual right.
//!
//! That last one is the only reading here ECMA-376 does not spell out in one
//! place, and it is settled from inside this repo rather than guessed: the
//! Transitional `w:left`/`w:right` this parser reads *are* Strict's
//! `w:start`/`w:end`, which is why `docx::parse::properties::schema::border` and
//! `::insets` already declare them as serde aliases of one another. They are
//! logical edges, so a `w:left` belongs at the cell's logical start — which
//! under `bidiVisual` is on the right.
//!
//! # How these tests are written
//!
//! Every assertion is a **relation between the same table with and without
//! `<w:bidiVisual/>`**, not a list of coordinates. A cell's box must satisfy
//!
//! ```text
//! x_rtl == table_left + table_right − (x_ltr + w_ltr)
//! ```
//!
//! with its width unchanged — a reflection about the table's own edges. Written
//! that way, no page origin, cell margin or glyph metric has to be known, and
//! the assertions survive any later refinement of the geometry they reflect.
//!
//! The declared grid is deliberately **unequal** (1000/2000/3000 twips). With
//! three equal columns, reversing the cells while leaving the slot widths in
//! place produces exactly the same page as reversing both, so an equal grid
//! cannot tell a correct mirror from half of one.

use std::collections::HashMap;
use std::io::Write;

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};

fn make_docx(document_xml: &str, styles_xml: Option<&str>) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", o).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>"#,
        )
        .unwrap();

        zip.start_file("_rels/.rels", o).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
        )
        .unwrap();

        if let Some(styles) = styles_xml {
            zip.start_file("word/_rels/document.xml.rels", o).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#,
            )
            .unwrap();
            zip.start_file("word/styles.xml", o).unwrap();
            zip.write_all(styles.as_bytes()).unwrap();
        }

        zip.start_file("word/document.xml", o).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buf
}

/// The fixture's grid: 1000 + 2000 + 3000 twips = 50 + 100 + 150 pt.
const GRID: &str = r#"<w:gridCol w:w="1000"/><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/>"#;

/// One table of `rows`, with `<w:bidiVisual/>` when `bidi`, plus any extra
/// `tblPr` children. `w:tblLayout="fixed"` and an explicit `w:tblW` keep the
/// declared grid from being rescaled, so the slot widths under test are the
/// ones written here.
fn table(bidi: bool, extra_tbl_pr: &str, rows: &str) -> String {
    let flag = if bidi { "<w:bidiVisual/>" } else { "" };
    format!(
        r#"<w:tbl>
  <w:tblPr>
    <w:tblW w:w="6000" w:type="dxa"/>
    <w:tblLayout w:type="fixed"/>
    {flag}{extra_tbl_pr}
  </w:tblPr>
  <w:tblGrid>{GRID}</w:tblGrid>
  {rows}
</w:tbl>"#
    )
}

fn layout(body: &str, styles: Option<&str>) -> Vec<LayoutedPage> {
    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {body}
    <w:sectPr><w:pgSz w:w="12240" w:h="15840"/>
      <w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"
               w:header="720" w:footer="720" w:gutter="0"/>
    </w:sectPr>
  </w:body>
</w:document>"#
    );
    let doc = dxpdf::docx::parse(&make_docx(&document_xml, styles)).expect("parse");
    dxpdf::render::resolve_and_layout(doc).1
}

/// A shaded cell. The fill is how these tests read a cell's box off the page:
/// §17.4.33 shading is emitted at the cell box exactly, and a distinct colour
/// per cell keeps `coalesce_abutting_rects` from fusing neighbours.
fn cell(fill: &str, extra_tc_pr: &str) -> String {
    format!(
        r#"<w:tc>
  <w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>{extra_tc_pr}</w:tcPr>
  <w:p><w:r><w:t>x</w:t></w:r></w:p>
</w:tc>"#
    )
}

type Rect = (f32, f32, f32, f32);

/// Every shaded box on the page, by fill colour.
fn boxes(pages: &[LayoutedPage]) -> HashMap<(u8, u8, u8), Rect> {
    let mut out = HashMap::new();
    for c in pages.iter().flat_map(|p| &p.commands) {
        if let DrawCommand::Rect { rect, color } = c {
            out.entry((color.r, color.g, color.b)).or_insert((
                rect.origin.x.raw(),
                rect.origin.y.raw(),
                rect.size.width.raw(),
                rect.size.height.raw(),
            ));
        }
    }
    out
}

const RED: (u8, u8, u8) = (0xFF, 0x00, 0x00);
const GREEN: (u8, u8, u8) = (0x00, 0xFF, 0x00);
const BLUE: (u8, u8, u8) = (0x00, 0x00, 0xFF);

/// The table's own span, as `(left, right)`, taken from the shaded boxes rather
/// than from the page margin — so the reflection is about the table's edges and
/// not about anything the section happens to set.
fn span(rects: &HashMap<(u8, u8, u8), Rect>) -> (f32, f32) {
    let left = rects.values().map(|r| r.0).fold(f32::INFINITY, f32::min);
    let right = rects
        .values()
        .map(|r| r.0 + r.2)
        .fold(f32::NEG_INFINITY, f32::max);
    (left, right)
}

/// Assert that every box in `rtl` is the reflection of its `ltr` twin about the
/// table's own edges, with its width unchanged.
fn assert_mirrored(ltr: &HashMap<(u8, u8, u8), Rect>, rtl: &HashMap<(u8, u8, u8), Rect>) {
    assert_eq!(ltr.len(), rtl.len(), "same cells either way");
    let (l, r) = span(ltr);
    assert_eq!((l, r), span(rtl), "the table itself must not move");
    for (colour, &(x, _, w, _)) in ltr {
        let &(rx, _, rw, _) = rtl
            .get(colour)
            .unwrap_or_else(|| panic!("no {colour:?} cell in the mirrored table"));
        assert_eq!(rw, w, "{colour:?}: a mirrored cell keeps its width");
        assert_eq!(
            rx,
            l + r - (x + w),
            "{colour:?}: expected the reflection of {x}..{} about {l}..{r}",
            x + w
        );
    }
}

// ── the mirror itself ───────────────────────────────────────────────────────

/// The first cell of a row is the rightmost one, and the slot widths travel with
/// the columns.
///
/// The non-vacuity assertions are what make the reflection mean something: the
/// three cells have three *different* widths and are in ascending order without
/// the flag, so a renderer that ignored `bidiVisual` — or that reversed the
/// cells while leaving the slot widths where they were — fails.
#[test]
fn the_first_cell_of_a_row_becomes_the_rightmost() {
    let row = format!(
        "<w:tr>{}{}{}</w:tr>",
        cell("FF0000", ""),
        cell("00FF00", ""),
        cell("0000FF", "")
    );
    let ltr = boxes(&layout(&table(false, "", &row), None));
    let rtl = boxes(&layout(&table(true, "", &row), None));

    // Non-vacuity: the declared grid really is unequal and really is in source
    // order without the flag.
    assert_eq!(ltr[&RED].2, 50.0, "1000 twips");
    assert_eq!(ltr[&GREEN].2, 100.0, "2000 twips");
    assert_eq!(ltr[&BLUE].2, 150.0, "3000 twips");
    assert!(
        ltr[&RED].0 < ltr[&GREEN].0 && ltr[&GREEN].0 < ltr[&BLUE].0,
        "without the flag the cells run left to right"
    );

    assert_mirrored(&ltr, &rtl);

    // …and the headline claim, stated directly rather than as a difference.
    assert!(
        rtl[&RED].0 > rtl[&GREEN].0 && rtl[&GREEN].0 > rtl[&BLUE].0,
        "with it they run right to left: {rtl:?}"
    );
}

// ── §17.4.17 `w:gridSpan` ───────────────────────────────────────────────────

/// A span covers the mirrored *run* of slots — not one slot, and not the run it
/// covered before the flip.
///
/// The grid is unequal, so this is a real constraint: the red cell spans
/// 1000 + 2000 twips on the left and must come out spanning the same two
/// columns once they are the rightmost two, keeping its 150 pt.
#[test]
fn a_grid_span_covers_the_mirrored_run_of_slots() {
    let row = format!(
        "<w:tr>{}{}</w:tr>",
        cell("FF0000", r#"<w:gridSpan w:val="2"/>"#),
        cell("00FF00", "")
    );
    let ltr = boxes(&layout(&table(false, "", &row), None));
    let rtl = boxes(&layout(&table(true, "", &row), None));

    assert_eq!(ltr[&RED].2, 150.0, "1000 + 2000 twips");
    assert_eq!(ltr[&GREEN].2, 150.0, "3000 twips");
    assert_mirrored(&ltr, &rtl);
}

// ── §17.4.15 `w:gridBefore` ─────────────────────────────────────────────────

/// A row's skipped columns are skipped at its *logical* start, so under the flag
/// the gap is at the visual right.
///
/// The second row spans the whole grid and is what makes the first row's gap
/// measurable: a row with a gap cannot say where the table's edges are, so on
/// its own every claim below would be about the cell relative to itself and
/// would hold however the gap fell.
#[test]
fn a_grid_before_gap_moves_to_the_visual_right() {
    let rows = format!(
        "<w:tr><w:trPr><w:gridBefore w:val=\"1\"/></w:trPr>{}</w:tr><w:tr>{}</w:tr>",
        cell("FF0000", r#"<w:gridSpan w:val="2"/>"#),
        cell("00FF00", r#"<w:gridSpan w:val="3"/>"#),
    );
    let ltr = boxes(&layout(&table(false, "", &rows), None));
    let rtl = boxes(&layout(&table(true, "", &rows), None));

    // 2000 + 3000 twips of cell, one 1000-twip column skipped ahead of it; the
    // full-width row below is the table's own 6000.
    assert_eq!(ltr[&RED].2, 250.0);
    assert_eq!(ltr[&GREEN].2, 300.0, "the reference row spans the grid");
    let (l, r) = span(&ltr);
    assert_eq!((l, r), (ltr[&GREEN].0, ltr[&GREEN].0 + 300.0));

    assert_eq!(
        ltr[&RED].0 - l,
        50.0,
        "without the flag the skipped 1000-twip column is on the left"
    );
    assert_eq!(
        r - (rtl[&RED].0 + rtl[&RED].2),
        50.0,
        "with it the same column is skipped on the right"
    );
    assert_mirrored(&ltr, &rtl);
}

// ── §17.4.84 `w:vMerge` ─────────────────────────────────────────────────────

/// A merge spans the mirrored column, and still spans both rows.
///
/// The height is asserted as well as the box, because a merge that lost its
/// span would still mirror correctly as a one-row cell.
#[test]
fn a_vertical_merge_mirrors_with_its_column() {
    let rows = format!(
        "<w:tr>{}{}{}</w:tr><w:tr>{}{}{}</w:tr>",
        cell("FF0000", r#"<w:vMerge w:val="restart"/>"#),
        cell("00FF00", ""),
        cell("0000FF", ""),
        cell("FF0000", "<w:vMerge/>"),
        cell("00FFFF", ""),
        cell("FF00FF", ""),
    );
    let ltr = boxes(&layout(&table(false, "", &rows), None));
    let rtl = boxes(&layout(&table(true, "", &rows), None));

    assert_mirrored(&ltr, &rtl);
    assert_eq!(
        rtl[&RED].3, ltr[&RED].3,
        "the merged cell keeps the height of its span"
    );
    assert!(
        rtl[&RED].3 > rtl[&GREEN].3,
        "and that height really is more than one row: {rtl:?}"
    );
}

// ── §17.4.39 the logical start edge ─────────────────────────────────────────

/// A cell that declares only `w:left` paints that border on its **visual right**
/// under the flag, because `w:left` is Strict's `w:start` — the cell's logical
/// leading edge.
///
/// Read as the border's offset within its own cell box, so the assertion is
/// about which side of the cell the line is on and not about where the cell is.
#[test]
fn a_cells_start_border_paints_on_its_visual_right() {
    let row = format!(
        "<w:tr>{}</w:tr>",
        cell(
            "FF0000",
            r#"<w:tcBorders>
                 <w:left w:val="single" w:sz="24" w:space="0" w:color="0000FF"/>
                 <w:top w:val="nil"/><w:bottom w:val="nil"/><w:right w:val="nil"/>
               </w:tcBorders>"#
        )
    );

    // `(gap left of the border, gap right of it)` within the cell's own box, so
    // the claim is which side of the cell the line sits on and needs neither the
    // cell's width nor its position spelled out.
    let gaps = |bidi: bool| -> (f32, f32) {
        let pages = layout(&table(bidi, "", &row), None);
        let rects = boxes(&pages);
        let (cx, _, cw, _) = rects[&RED];
        let (bx, _, bw, _) = rects[&BLUE];
        assert_eq!(bw, 3.0, "w:sz=24 is 3pt");
        (bx - cx, (cx + cw) - (bx + bw))
    };

    let (before, after) = gaps(false);
    assert_eq!(before, 0.0, "without the flag, flush with the left edge");
    assert!(after > 0.0, "…and the rest of the cell is to its right");
    // With the flag the two gaps swap: the border is flush with the *right*
    // edge, and the same amount of cell is now to its left.
    assert_eq!(
        gaps(true),
        (after, before),
        "the logical start border moves to the visual right of its cell"
    );
}

// ── §17.7.6 conditional formatting stays logical ────────────────────────────

/// `firstColumn` keeps meaning the **logical** first column, which under the
/// flag is the rightmost one.
///
/// This is the test that pins the ordering the whole design rests on: the
/// conditional region is resolved on logical grid columns before the mirror is
/// applied, so moving the mirror any earlier would silently shade the wrong
/// column. Without the flag the same style shades the leftmost cell, which is
/// the control.
#[test]
fn the_first_column_region_stays_the_logical_first_column() {
    let styles = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="table" w:styleId="TestTbl">
    <w:name w:val="Test Table"/>
    <w:tblStylePr w:type="firstCol">
      <w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="00FF00"/></w:tcPr>
    </w:tblStylePr>
  </w:style>
</w:styles>"#;
    // No per-cell shading: the only fill on the page is the one the firstCol
    // layer paints, so its box *is* the answer.
    let plain = r#"<w:tc><w:tcPr/><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc>"#;
    let row = format!("<w:tr>{plain}{plain}{plain}</w:tr>");
    let extra = r#"<w:tblStyle w:val="TestTbl"/>
                   <w:tblLook w:firstRow="0" w:lastRow="0" w:firstColumn="1"
                              w:lastColumn="0" w:noHBand="1" w:noVBand="1"/>"#;

    let ltr = boxes(&layout(&table(false, extra, &row), Some(styles)));
    let rtl = boxes(&layout(&table(true, extra, &row), Some(styles)));

    let (l, r) = (ltr[&GREEN].0, ltr[&GREEN].0 + ltr[&GREEN].2);
    assert_eq!(
        ltr[&GREEN].2, 50.0,
        "the logical first column is 1000 twips"
    );
    assert_eq!(rtl[&GREEN].2, 50.0, "…and still is, on the other side");
    assert!(
        rtl[&GREEN].0 > l,
        "the shaded column must move right, from {l}..{r} to {:?}",
        rtl[&GREEN]
    );
}
