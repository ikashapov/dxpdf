//! A property bag that repeats a child the schema allows once must not fail the
//! parse, and must resolve to the last occurrence.
//!
//! Every OOXML property bag is an `xsd:sequence` whose children carry
//! `maxOccurs="1"`, so a repeated child is schema-invalid. Real producers emit
//! them anyway and Word opens the result; before this was handled, one
//! redundant element failed the **whole conversion**, because
//! `docx::parse` returns a `Result`.
//!
//! Last-wins is this parser's choice, not the spec's — §17.7.2 defines it for
//! toggle properties only. The reasoning and the Word reference render that
//! would overturn it are in `docx::parse::primitives::duplicates`. These tests
//! pin the choice so it cannot drift silently.
//!
//! Each case builds its own minimal document rather than sharing one, so a
//! failure names the bag that broke. `test-files/duplicate-children.docx`
//! covers the same ground end-to-end via `tests/parse_test_files.rs`.

use std::io::Write;

use dxpdf::model::{Alignment, Block, Color, TableMeasure};

/// Minimal in-memory DOCX around the given body XML. Local rather than shared
/// with `tests/integration.rs` so this file stands alone.
fn make_docx(body: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("[Content_Types].xml", opts).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    )
    .unwrap();
    zip.start_file("_rels/.rels", opts).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
    )
    .unwrap();
    zip.start_file("word/document.xml", opts).unwrap();
    zip.write_all(
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>{body}</w:body></w:document>"#
        )
        .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap().into_inner()
}

/// Wrap body XML in a document and parse it.
fn parse_body(body: &str) -> dxpdf::model::Document {
    let docx = make_docx(body);
    dxpdf::docx::parse(&docx).expect("a repeated child must not fail the parse")
}

fn first_paragraph(doc: &dxpdf::model::Document) -> &dxpdf::model::Paragraph {
    doc.body
        .iter()
        .find_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .expect("expected a paragraph")
}

fn first_table(doc: &dxpdf::model::Document) -> &dxpdf::model::Table {
    doc.body
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("expected a table")
}

#[test]
fn ppr_repeated_jc_takes_the_last() {
    let doc = parse_body(
        r#"<w:p><w:pPr><w:jc w:val="left"/><w:jc w:val="center"/></w:pPr>
           <w:r><w:t>x</w:t></w:r></w:p>"#,
    );
    assert_eq!(
        first_paragraph(&doc).properties.alignment,
        Some(Alignment::Center),
        "§17.3.1.13"
    );
}

#[test]
fn ppr_repeated_ind_takes_the_last() {
    let doc = parse_body(
        r#"<w:p><w:pPr><w:ind w:left="100"/><w:ind w:left="1440"/></w:pPr>
           <w:r><w:t>x</w:t></w:r></w:p>"#,
    );
    let ind = first_paragraph(&doc)
        .properties
        .indentation
        .expect("indentation present");
    assert_eq!(ind.start.map(|d| d.raw()), Some(1440), "§17.3.1.12");
}

/// `<w:pBdr>` is a *child of the bag*, so the whole border set is carried and
/// resolved at the read — unlike the sides inside it, which collapse at the
/// seam so `ParagraphBorders` stays `Copy`. See `model::dup`.
#[test]
fn ppr_repeated_pbdr_takes_the_last_and_keeps_both() {
    let doc = parse_body(
        r#"<w:p><w:pPr>
             <w:pBdr><w:top w:val="single" w:sz="4" w:space="1" w:color="auto"/></w:pBdr>
             <w:pBdr><w:bottom w:val="single" w:sz="4" w:space="1" w:color="auto"/></w:pBdr>
           </w:pPr><w:r><w:t>x</w:t></w:r></w:p>"#,
    );
    let borders = &first_paragraph(&doc).properties.borders;
    assert!(borders.is_duplicated(), "the document repeated <w:pBdr>");
    assert_eq!(borders.all().len(), 2, "neither occurrence was dropped");
    let effective = borders.get().expect("a border set is present");
    assert!(effective.bottom.is_some(), "last occurrence wins");
    assert!(
        effective.top.is_none(),
        "and the earlier one does not leak in"
    );
    // first-wins is still reachable downstream without touching the parser
    assert!(borders.all()[0].top.is_some());
}

#[test]
fn rpr_repeated_sz_and_color_take_the_last() {
    let doc = parse_body(
        r#"<w:p><w:r><w:rPr>
             <w:sz w:val="20"/><w:sz w:val="48"/>
             <w:color w:val="FF0000"/><w:color w:val="0000FF"/>
           </w:rPr><w:t>x</w:t></w:r></w:p>"#,
    );
    let para = first_paragraph(&doc);
    let run = para
        .content
        .iter()
        .find_map(|i| match i {
            dxpdf::model::Inline::TextRun(r) => Some(r),
            _ => None,
        })
        .expect("expected a run");
    assert_eq!(
        run.properties.font_size.map(|d| d.raw()),
        Some(48),
        "§17.3.2.38"
    );
    assert_eq!(
        run.properties.color,
        Some(Color::Rgb(0x0000FF)),
        "§17.3.2.6"
    );
}

#[test]
fn tblpr_repeated_children_take_the_last() {
    let doc = parse_body(
        r#"<w:tbl><w:tblPr>
             <w:tblW w:w="1000" w:type="dxa"/><w:tblW w:w="5000" w:type="dxa"/>
             <w:jc w:val="left"/><w:jc w:val="center"/>
           </w:tblPr><w:tblGrid><w:gridCol w:w="5000"/></w:tblGrid>
           <w:tr><w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let props = &first_table(&doc).properties;
    assert!(
        matches!(props.width, Some(TableMeasure::Twips(d)) if d.raw() == 5000),
        "§17.4.63, got {:?}",
        props.width
    );
    assert_eq!(props.alignment, Some(Alignment::Center), "§17.4.29");
}

#[test]
fn trpr_repeated_trheight_takes_the_last() {
    let doc = parse_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="5000"/></w:tblGrid>
           <w:tr><w:trPr><w:trHeight w:val="200"/><w:trHeight w:val="900"/></w:trPr>
           <w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let row = &first_table(&doc).rows[0];
    assert_eq!(
        row.properties.height.map(|h| h.value.raw()),
        Some(900),
        "§17.4.81"
    );
}

#[test]
fn tcpr_repeated_tcmar_takes_the_last_and_keeps_both() {
    let doc = parse_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="5000"/></w:tblGrid>
           <w:tr><w:tc><w:tcPr>
             <w:tcMar><w:top w:w="100" w:type="dxa"/></w:tcMar>
             <w:tcMar><w:bottom w:w="200" w:type="dxa"/></w:tcMar>
           </w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let margins = &first_table(&doc).rows[0].cells[0].properties.margins;
    assert!(margins.is_duplicated());
    assert_eq!(margins.all().len(), 2);
    assert_eq!(
        margins.get().unwrap().bottom.map(|d| d.raw()),
        Some(200),
        "§17.4.42, last occurrence wins"
    );
}

/// The committed fixture exercises every bag at once through the real
/// end-to-end path, so a regression shows up without a new test being written.
#[test]
fn the_committed_fixture_parses_and_resolves() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test-files/duplicate-children.docx"
    ))
    .expect("fixture present — rebuild with scripts/make_duplicate_children_fixture.py");
    let doc = dxpdf::docx::parse(&bytes).expect("every duplicated bag must parse");
    assert_eq!(
        first_paragraph(&doc).properties.alignment,
        Some(Alignment::Center),
        "the fixture's first paragraph repeats <w:jc>; the last must win"
    );
    // And it must render, not merely parse.
    dxpdf::convert(&bytes).expect("fixture must convert");
}
