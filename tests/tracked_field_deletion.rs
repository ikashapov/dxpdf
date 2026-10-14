//! §17.16.13 — a field deleted under change tracking parses and converts
//! (issue #253).
//!
//! Word writes `w:delInstrText` for the instruction of a field deleted while
//! changes are tracked, exactly as it writes `w:delText` for deleted text. The
//! element was missing from the run-content variants, so the whole document was
//! rejected — "unknown variant `delInstrText`" — while a deleted word was
//! fine. This is the reporter's reproducer, verbatim.

use std::io::Write;

fn make_docx(document_xml: &str) -> Vec<u8> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("[Content_Types].xml", o).unwrap();
    zip.write_all(
        br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
    )
    .unwrap();
    zip.start_file("_rels/.rels", o).unwrap();
    zip.write_all(
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
    )
    .unwrap();
    zip.start_file("word/document.xml", o).unwrap();
    zip.write_all(document_xml.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

const TRACKED: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Kept</w:t></w:r><w:del w:id="1"><w:r><w:delInstrText xml:space="preserve"> SEQ exhibit \* ARABIC </w:delInstrText><w:delText>gone</w:delText></w:r></w:del></w:p></w:body></w:document>"#;

/// The document parses, and the deleted field's instruction is never drawn —
/// whether deletions are dropped (upstream) or rendered as tracked changes
/// (the fork's #154): a field code is not text either way.
#[test]
fn a_deleted_field_instruction_parses_and_is_never_drawn() {
    use dxpdf::render::layout::draw_command::DrawCommand;
    let doc = dxpdf::docx::parse(&make_docx(TRACKED)).expect("delInstrText is run content");
    let pages = dxpdf::render::resolve_and_layout(doc).1;
    let drawn: Vec<String> = pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            DrawCommand::Text { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        drawn.iter().any(|t| t == "Kept"),
        "the kept word is drawn: {drawn:?}"
    );
    assert!(
        !drawn
            .iter()
            .any(|t| t.contains("SEQ") || t.contains("ARABIC")),
        "a deleted field code is never drawn: {drawn:?}"
    );
}

/// End to end: the reporter's file converts to a PDF.
#[test]
fn a_deleted_field_converts_to_pdf() {
    let pdf = dxpdf::convert(&make_docx(TRACKED)).expect("converts");
    assert_eq!(&pdf[..5], b"%PDF-");
}
