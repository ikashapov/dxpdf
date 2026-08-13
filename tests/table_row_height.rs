//! §17.4.85 / §17.4.1 — table row height, page-advance, and the two malformed
//! `w:vMerge` shapes.
//!
//! These defects are invisible at the `layout_table` level and only show up
//! once the section layer places the result: a zero-height row lets the *next*
//! block draw over the table, an abandoned empty leading slice becomes a blank
//! page, and an unpaired `w:vMerge` drops a cell's text out of the document
//! altogether. The unit tests in `table/mod.rs` and `build/table.rs` pin the
//! arithmetic; these pin what a reader of the PDF would actually see.

use std::io::Write;

use dxpdf::render::layout::draw_command::DrawCommand;

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

fn doc(body: &str) -> Vec<u8> {
    make_docx(&format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{body}</w:body>
</w:document>"#
    ))
}

fn layout(bytes: &[u8]) -> Vec<dxpdf::render::layout::draw_command::LayoutedPage> {
    let parsed = dxpdf::docx::parse(bytes).expect("parse");
    dxpdf::render::resolve_and_layout(parsed).1
}

/// y of the first text command whose content equals `needle`.
fn y_of(pages: &[dxpdf::render::layout::draw_command::LayoutedPage], needle: &str) -> f32 {
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .find_map(|c| match c {
            DrawCommand::Text { text, position, .. } if &**text == needle => Some(position.y.raw()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text command {needle:?}"))
}

/// A single-cell table whose only cell carries `vmerge` verbatim in `tcPr`.
fn lone_cell_table(vmerge: &str) -> Vec<u8> {
    doc(&format!(
        r#"<w:p><w:r><w:t>before</w:t></w:r></w:p>
    <w:tbl>
      <w:tblPr><w:tblW w:w="8000" w:type="dxa"/></w:tblPr>
      <w:tblGrid><w:gridCol w:w="8000"/></w:tblGrid>
      <w:tr><w:tc><w:tcPr><w:tcW w:w="8000" w:type="dxa"/>{vmerge}</w:tcPr>
        <w:p><w:r><w:t>AAA</w:t></w:r></w:p>
        <w:p><w:r><w:t>BBB</w:t></w:r></w:p>
        <w:p><w:r><w:t>CCC</w:t></w:r></w:p>
      </w:tc></w:tr>
    </w:tbl>
    <w:p><w:r><w:t>after</w:t></w:r></w:p>"#
    ))
}

/// §17.4.85: a `vMerge="restart"` with no continuation is an ordinary cell.
///
/// The row used to collapse to zero height while still drawing its content,
/// so `after` was emitted *above* the table's own text instead of below it.
#[test]
fn lone_vmerge_restart_does_not_let_following_content_overlap_the_table() {
    let pages = layout(&lone_cell_table(r#"<w:vMerge w:val="restart"/>"#));

    let last_row_text = y_of(&pages, "CCC");
    let after = y_of(&pages, "after");

    assert!(
        after > last_row_text,
        "\"after\" must follow the table's last line (y={last_row_text:.1}), \
         but was drawn at y={after:.1} — on top of it"
    );
}

/// Calibrated against the unmerged control: a lone restart must lay out
/// identically to no merge at all, not merely "somewhere below".
#[test]
fn lone_vmerge_restart_matches_the_unmerged_layout() {
    let restart = layout(&lone_cell_table(r#"<w:vMerge w:val="restart"/>"#));
    let control = layout(&lone_cell_table(""));

    assert!(
        (y_of(&restart, "after") - y_of(&control, "after")).abs() < 0.01,
        "lone restart moved the following paragraph: {:.1} vs control {:.1}",
        y_of(&restart, "after"),
        y_of(&control, "after"),
    );
}

/// Every string this document draws, in page order.
fn all_text(pages: &[dxpdf::render::layout::draw_command::LayoutedPage]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            DrawCommand::Text { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

/// A two-row, two-column table whose **first** row's left cell carries
/// `vmerge` verbatim. `w:val="continue"` there has no `restart` above it —
/// there is no row above it at all — which is the orphan case.
fn orphan_continue_table(vmerge: &str) -> Vec<u8> {
    doc(&format!(
        r#"<w:p><w:r><w:t>before</w:t></w:r></w:p>
    <w:tbl>
      <w:tblPr><w:tblW w:w="8000" w:type="dxa"/></w:tblPr>
      <w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>
      <w:tr>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/>{vmerge}</w:tcPr>
          <w:p><w:r><w:t>ORPHAN</w:t></w:r></w:p></w:tc>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>
          <w:p><w:r><w:t>PEER</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>
          <w:p><w:r><w:t>NEXT</w:t></w:r></w:p></w:tc>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>
          <w:p><w:r><w:t>NEXT2</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
    <w:p><w:r><w:t>after</w:t></w:r></w:p>"#
    ))
}

/// §17.4.85: a `<w:vMerge w:val="continue"/>` with **no `restart` above it**
/// must not cost the cell its content.
///
/// §17.4.85 describes `continue` only as continuing the merge begun by a
/// `restart`, and says nothing about a `continue` that begins one — so the
/// engine is free to choose what an unpaired one means. It is *not* free to
/// choose that the text disappears: no reading of §17.4.85 deletes content,
/// and a merged cell shows the *restart* cell's content precisely because
/// there is one to show.
///
/// See `build::table::promote_orphan_vmerge_continues` for the choice made and
/// the corroborating LibreOffice behaviour.
#[test]
fn orphan_vmerge_continue_keeps_its_cell_content() {
    let pages = layout(&orphan_continue_table(r#"<w:vMerge w:val="continue"/>"#));
    let drawn = all_text(&pages);

    assert!(
        drawn.iter().any(|t| t == "ORPHAN"),
        "the orphaned cell's text was dropped from the document — drawn: {drawn:?}"
    );
}

/// …and the cell is an ordinary one, not merely a visible one: it lays out
/// exactly as the same table with no `w:vMerge` at all, which is what
/// "promote it to an ordinary cell" has to mean if it is to mean anything
/// measurable. Calibrated against that control rather than against a literal
/// y, so no glyph metric has to be known.
#[test]
fn orphan_vmerge_continue_matches_the_unmerged_layout() {
    let orphan = layout(&orphan_continue_table(r#"<w:vMerge w:val="continue"/>"#));
    let control = layout(&orphan_continue_table(""));

    for needle in ["ORPHAN", "PEER", "NEXT", "NEXT2", "after"] {
        assert!(
            (y_of(&orphan, needle) - y_of(&control, needle)).abs() < 0.01,
            "{needle} moved: y={:.2} vs control y={:.2}",
            y_of(&orphan, needle),
            y_of(&control, needle),
        );
    }
}

/// The control that must not move: a `continue` that *does* have a `restart`
/// above it is a real merge, and §17.4.85 makes the merged region show the
/// restart cell's content — so the continue cell's own content stays hidden.
/// A fix that simply stopped honouring `continue` would pass both tests above
/// and fail this one.
#[test]
fn a_paired_vmerge_continue_still_hides_its_own_content() {
    let pages = layout(&doc(r#"<w:tbl>
      <w:tblPr><w:tblW w:w="8000" w:type="dxa"/></w:tblPr>
      <w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>
      <w:tr>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/><w:vMerge w:val="restart"/></w:tcPr>
          <w:p><w:r><w:t>MERGED</w:t></w:r></w:p></w:tc>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>
          <w:p><w:r><w:t>PEER</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/><w:vMerge w:val="continue"/></w:tcPr>
          <w:p><w:r><w:t>HIDDEN</w:t></w:r></w:p></w:tc>
        <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>
          <w:p><w:r><w:t>PEER2</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>"#));
    let drawn = all_text(&pages);

    assert!(drawn.iter().any(|t| t == "MERGED"), "drawn: {drawn:?}");
    assert!(
        !drawn.iter().any(|t| t == "HIDDEN"),
        "a continued merge shows the restart cell's content, not the \
         continue cell's — drawn: {drawn:?}"
    );
}

/// §17.4.1: a `cantSplit` row taller than a whole page fits nowhere, so the
/// paginator must not advance to a fresh page it cannot use. It used to
/// abandon an empty leading slice, which the section layer turned into a
/// blank first page.
#[test]
fn oversized_cant_split_row_does_not_emit_a_blank_leading_page() {
    let paras: String = (0..70)
        .map(|i| format!("<w:p><w:r><w:t>line {i}</w:t></w:r></w:p>"))
        .collect();
    let pages = layout(&doc(&format!(
        r#"<w:tbl>
      <w:tblPr><w:tblW w:w="8000" w:type="dxa"/></w:tblPr>
      <w:tblGrid><w:gridCol w:w="8000"/></w:tblGrid>
      <w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc>
        <w:tcPr><w:tcW w:w="8000" w:type="dxa"/></w:tcPr>
        {paras}
      </w:tc></w:tr>
    </w:tbl>
    <w:p><w:r><w:t>after</w:t></w:r></w:p>"#
    )));

    assert!(
        !pages[0].commands.is_empty(),
        "page 0 is blank — the table advanced to a page that gave it no more room"
    );
}

/// The converse: a table that genuinely doesn't fit in the *remaining* space
/// but does fit on a fresh page must still move. Guards against a fix that
/// simply stops advancing.
#[test]
fn table_after_body_text_still_moves_to_the_next_page_when_it_does_not_fit() {
    let filler: String = (0..60)
        .map(|i| format!("<w:p><w:r><w:t>filler {i}</w:t></w:r></w:p>"))
        .collect();
    let rows: String = (0..12)
        .map(|i| {
            format!(
                r#"<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc>
                     <w:tcPr><w:tcW w:w="8000" w:type="dxa"/></w:tcPr>
                     <w:p><w:r><w:t>row {i}</w:t></w:r></w:p></w:tc></w:tr>"#
            )
        })
        .collect();
    let pages = layout(&doc(&format!(
        r#"{filler}
    <w:tbl>
      <w:tblPr><w:tblW w:w="8000" w:type="dxa"/></w:tblPr>
      <w:tblGrid><w:gridCol w:w="8000"/></w:tblGrid>
      {rows}
    </w:tbl>"#
    )));

    assert!(
        pages.len() > 1,
        "filler plus a 12-row table should not fit on one page"
    );
    for (i, page) in pages.iter().enumerate() {
        assert!(!page.commands.is_empty(), "page {i} is blank");
    }
}
