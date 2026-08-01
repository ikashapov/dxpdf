//! §17.18.85 `ST_TabJc` — decimal tab alignment, end to end.
//!
//! No document in `test-files/` uses `<w:tab w:val="decimal"/>`, so the whole
//! corpus renders byte-identically whether this works or not. The fixture is
//! built here rather than committed as a binary for the same reason the
//! `wholeTable` fixture is: the XML *is* the point of the test, and a `.docx`
//! would hide it.

use std::io::Write;

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};

fn make_docx(document_xml: &str) -> Vec<u8> {
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

        zip.start_file("word/document.xml", o).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buf
}

/// One paragraph per entry, each `<tab>` + the entry, against a single
/// decimal stop at 4320 twips (216pt = 3in).
fn decimal_tab_document(entries: &[&str]) -> String {
    let paragraphs: String = entries
        .iter()
        .map(|entry| {
            format!(
                r#"<w:p>
  <w:pPr>
    <w:tabs><w:tab w:val="decimal" w:pos="4320"/></w:tabs>
  </w:pPr>
  <w:r><w:tab/><w:t>{entry}</w:t></w:r>
</w:p>"#
            )
        })
        .collect();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{paragraphs}</w:body>
</w:document>"#
    )
}

/// The x of every emitted text command carrying `needle`, in order.
fn xs_of_texts(pages: &[LayoutedPage]) -> Vec<(String, f32)> {
    pages
        .iter()
        .flat_map(|p| p.commands.iter())
        .filter_map(|c| match c {
            DrawCommand::Text { text, position, .. } => Some((text.to_string(), position.x.raw())),
            _ => None,
        })
        .collect()
}

#[test]
fn decimal_stop_aligns_separators_across_paragraphs() {
    // §17.18.85: whatever the integer part's width, the separator lands on
    // the stop — so a column of figures aligns on its decimal points.
    let bytes = make_docx(&decimal_tab_document(&["1.5", "22.75", "333.125"]));
    let doc = dxpdf::docx::parse(&bytes).expect("fixture parses");
    let (_, pages) = dxpdf::render::resolve_and_layout(&doc);

    let texts = xs_of_texts(&pages);
    assert_eq!(texts.len(), 3, "one text command per entry: {texts:?}");

    // Each entry's separator x = start x + width of the part before it. The
    // three must coincide; the starts must not.
    let starts: Vec<f32> = texts.iter().map(|(_, x)| *x).collect();
    assert!(
        starts[0] > starts[1] && starts[1] > starts[2],
        "wider integer parts start further left: {starts:?}"
    );

    // A left-aligned stop would put all three at the same x — the exact bug
    // this guards. Assert they differ by the integer-part widths.
    assert!(
        (starts[0] - starts[2]).abs() > 1.0,
        "1.5 and 333.125 must not start at the same x: {starts:?}"
    );
}

#[test]
fn decimal_stop_right_aligns_an_entry_with_no_separator() {
    // Word right-aligns a separator-less decimal zone rather than
    // left-aligning it, so a whole number stays flush with the column.
    let bytes = make_docx(&decimal_tab_document(&["1234", "1.5"]));
    let doc = dxpdf::docx::parse(&bytes).expect("fixture parses");
    let (_, pages) = dxpdf::render::resolve_and_layout(&doc);

    let texts = xs_of_texts(&pages);
    assert_eq!(texts.len(), 2, "{texts:?}");

    // Coordinate-free: "1234" *ends* at the stop, "1.5" puts its separator
    // there, so the whole number starts further left by the width of its extra
    // digits. Under a left-aligned fallback both would start at the same x —
    // which is exactly the defect this guards.
    let (whole, whole_x) = &texts[0];
    let (fraction, fraction_x) = &texts[1];
    assert_eq!((whole.as_str(), fraction.as_str()), ("1234", "1.5"));
    assert!(
        whole_x < fraction_x,
        "a separator-less zone must right-align, so {whole:?} at {whole_x} \
         starts left of {fraction:?} at {fraction_x}; equal x means it was \
         left-aligned instead"
    );
}
