//! §17.3.1.31 paragraph shading and §17.3.2.32 run shading end to end.
//!
//! Both used to resolve through `resolve_color(s.fill, Background)` —
//! the fill alone, ignoring `s.pattern` entirely — while §17.4.33 cell
//! shading (issue #149) was moved onto `resolve_shading`, which reads the
//! whole `w:shd` element: `solid` is the *pattern* colour, not the fill;
//! `nil` is no shading at all; and a geometric pattern draws stripes, not a
//! flat blend. The gap meant the same `w:shd` element resolved three
//! different ways depending on whether it sat on a cell, a paragraph or a
//! run. These pin paragraph and run shading to the same resolution cell
//! shading already gets — see `tests/shading_patterns.rs` for the cell-level
//! equivalents these mirror.

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};
use std::io::Write;

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

fn layout(body: &str) -> Vec<LayoutedPage> {
    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {body}
    <w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>
  </w:body>
</w:document>"#
    );
    let doc = dxpdf::docx::parse(&make_docx(&document_xml)).expect("parse");
    dxpdf::render::resolve_and_layout(doc).1
}

/// Every filled rect's colour, as `(r, g, b)`.
fn rect_colors(pages: &[LayoutedPage]) -> Vec<(u8, u8, u8)> {
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            DrawCommand::Rect { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect()
}

/// Every line's colour, as `(r, g, b)`.
fn line_colors(pages: &[LayoutedPage]) -> Vec<(u8, u8, u8)> {
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            DrawCommand::Line { color, .. } => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect()
}

const PATTERN_COLOR: (u8, u8, u8) = (0xCC, 0x00, 0x00);
const FILL_COLOR: (u8, u8, u8) = (0x00, 0xCC, 0x00);

/// §17.3.1.31: a paragraph-level `solid` shading paints its **pattern**
/// colour (`w:color`) — the fill sits fully covered behind a 100% pattern —
/// not the fill (`w:fill`). This is the exact bug the cell-shading fix
/// (issue #149) closed at the table level without touching this call site.
#[test]
fn paragraph_solid_shading_paints_the_pattern_colour_not_the_fill() {
    let body = r#"<w:p>
        <w:pPr><w:shd w:val="solid" w:color="CC0000" w:fill="00CC00"/></w:pPr>
        <w:r><w:t>text</w:t></w:r>
      </w:p>"#;
    let colors = rect_colors(&layout(body));
    assert!(
        colors.contains(&PATTERN_COLOR),
        "expected the pattern colour {PATTERN_COLOR:?} somewhere; got {colors:?}"
    );
    assert!(
        !colors.contains(&FILL_COLOR),
        "the fill must not appear at all for a solid pattern; got {colors:?}"
    );
}

/// The run-level counterpart of the test above (§17.3.2.32).
#[test]
fn run_solid_shading_paints_the_pattern_colour_not_the_fill() {
    let body = r#"<w:p>
        <w:r>
          <w:rPr><w:shd w:val="solid" w:color="CC0000" w:fill="00CC00"/></w:rPr>
          <w:t>text</w:t>
        </w:r>
      </w:p>"#;
    let colors = rect_colors(&layout(body));
    assert!(
        colors.contains(&PATTERN_COLOR),
        "expected the pattern colour {PATTERN_COLOR:?} somewhere; got {colors:?}"
    );
    assert!(
        !colors.contains(&FILL_COLOR),
        "the fill must not appear at all for a solid pattern; got {colors:?}"
    );
}

/// §17.3.1.31: an explicit `nil` is no shading at all — not the fill colour
/// painted opaque. [MS-OI29500] §2.1.550 records this as the product
/// behaviour; the old `resolve_color(s.fill, ..)` call site read the fill
/// unconditionally and never consulted `s.pattern`, so a `nil` painted
/// whatever the (often `auto` → white) fill resolved to.
#[test]
fn paragraph_nil_shading_paints_nothing() {
    let body = r#"<w:p>
        <w:pPr><w:shd w:val="nil" w:color="auto" w:fill="auto"/></w:pPr>
        <w:r><w:t>text</w:t></w:r>
      </w:p>"#;
    let colors = rect_colors(&layout(body));
    assert!(
        colors.is_empty(),
        "a nil shading must paint no rect at all; got {colors:?}"
    );
}

/// The run-level counterpart (§17.3.2.32).
#[test]
fn run_nil_shading_paints_nothing() {
    let body = r#"<w:p>
        <w:r>
          <w:rPr><w:shd w:val="nil" w:color="auto" w:fill="auto"/></w:rPr>
          <w:t>text</w:t>
        </w:r>
      </w:p>"#;
    let colors = rect_colors(&layout(body));
    assert!(
        colors.is_empty(),
        "a nil shading must paint no rect at all; got {colors:?}"
    );
}

/// §17.3.1.31 / §17.18.78: a geometric pattern at paragraph level draws its
/// background rect *plus* stripe lines in the pattern colour — it must not
/// silently flatten to the fill colour the way the pre-fix
/// `resolve_color(s.fill, ..)` call site did (which had no notion of a
/// pattern at all).
#[test]
fn paragraph_pattern_shading_draws_background_and_stripes() {
    let body = r#"<w:p>
        <w:pPr><w:shd w:val="horzStripe" w:color="CC0000" w:fill="00CC00"/></w:pPr>
        <w:r><w:t>text</w:t></w:r>
      </w:p>"#;
    let pages = layout(body);
    let colors = rect_colors(&pages);
    let lines = line_colors(&pages);
    assert!(
        colors.contains(&FILL_COLOR),
        "the pattern's background is the fill colour; got {colors:?}"
    );
    assert!(
        lines.contains(&PATTERN_COLOR),
        "the stripes are drawn in the pattern colour; got {lines:?}"
    );
}

/// The run-level counterpart (§17.3.2.32).
#[test]
fn run_pattern_shading_draws_background_and_stripes() {
    let body = r#"<w:p>
        <w:r>
          <w:rPr><w:shd w:val="horzStripe" w:color="CC0000" w:fill="00CC00"/></w:rPr>
          <w:t>text</w:t>
        </w:r>
      </w:p>"#;
    let pages = layout(body);
    let colors = rect_colors(&pages);
    let lines = line_colors(&pages);
    assert!(
        colors.contains(&FILL_COLOR),
        "the pattern's background is the fill colour; got {colors:?}"
    );
    assert!(
        lines.contains(&PATTERN_COLOR),
        "the stripes are drawn in the pattern colour; got {lines:?}"
    );
}

/// §17.3.2.15: a `w:highlight` has no pattern of its own — the fixed
/// ST_HighlightColor palette — and must still resolve to a flat fill now
/// that run shading is folded through `ResolvedShading` rather than a bare
/// `RgbColor`. The control for the run-shading change: highlighting must
/// keep working exactly as before.
#[test]
fn run_highlight_still_resolves_to_a_flat_fill() {
    let body = r#"<w:p>
        <w:r>
          <w:rPr><w:highlight w:val="yellow"/></w:rPr>
          <w:t>text</w:t>
        </w:r>
      </w:p>"#;
    let colors = rect_colors(&layout(body));
    assert!(
        colors.contains(&(0xFF, 0xFF, 0x00)),
        "expected yellow highlight; got {colors:?}"
    );
}
