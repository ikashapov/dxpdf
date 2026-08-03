//! §17.6.22 `SectionType::Continuous` — a section break that starts the next
//! section on the page the previous one is still filling.
//!
//! Issue #83. The engine had no coverage for continuous breaks at all before
//! this file, so the cases here are written from the spec clause rather than
//! from the current behaviour.

use std::io::Write;

/// Minimal single-part DOCX around `body`.
fn docx(body: &str) -> Vec<u8> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", o).unwrap();
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
  <Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
  <Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
</Types>"#).unwrap();

    zip.start_file("_rels/.rels", o).unwrap();
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#).unwrap();

    zip.start_file("word/_rels/document.xml.rels", o).unwrap();
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rIdNum" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
  <Relationship Id="rIdH1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
  <Relationship Id="rIdH2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/>
</Relationships>"#).unwrap();

    // A single decimal list, so §17.9 label counters are observable in output.
    zip.start_file("word/numbering.xml", o).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
</w:numbering>"#,
    )
    .unwrap();

    // A header, so sections can differ in header height for the clearance
    // cases. Header content is built by `build_non_story_content`, which calls
    // `build_fragments` directly and never `inject_list_label` — so measuring a
    // header does *not* touch §17.9 list counters. The document-order side
    // effect it does have (§17.11.12 footnote display numbers) is guarded by a
    // unit test in `render::tests`, where the peek itself is reachable.
    zip.start_file("word/header1.xml", o).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:p><w:r><w:t>HDRSHORT</w:t></w:r></w:p>
</w:hdr>"#,
    )
    .unwrap();

    // §17.10.1: a deliberately *taller* header — four 20pt lines. Reserving it
    // pushes the body top far below where the short header would.
    zip.start_file("word/header2.xml", o).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:p><w:pPr><w:rPr><w:sz w:val="40"/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>HDRTALL1</w:t></w:r></w:p>
  <w:p><w:pPr><w:rPr><w:sz w:val="40"/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>HDRTALL2</w:t></w:r></w:p>
  <w:p><w:pPr><w:rPr><w:sz w:val="40"/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>HDRTALL3</w:t></w:r></w:p>
  <w:p><w:pPr><w:rPr><w:sz w:val="40"/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>HDRTALL4</w:t></w:r></w:p>
</w:hdr>"#,
    )
    .unwrap();

    zip.start_file("word/document.xml", o).unwrap();
    zip.write_all(
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{body}</w:body>
</w:document>"#
        )
        .as_bytes(),
    )
    .unwrap();

    zip.finish().unwrap().into_inner()
}

const PG: &str = r#"<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134" w:header="567" w:footer="567"/>"#;

/// A numbered list item — its rendered label is the §17.9 counter, so the
/// drawn text is a direct read-out of numbering state.
fn list_item(text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
    )
}

/// Every text string drawn across every page, in page then emission order.
fn drawn_text(pages: &[dxpdf::render::layout::draw_command::LayoutedPage]) -> Vec<String> {
    use dxpdf::render::layout::draw_command::DrawCommand;
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            DrawCommand::Text { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

fn layout(body: &str) -> Vec<dxpdf::render::layout::draw_command::LayoutedPage> {
    let doc = dxpdf::docx::parse(&docx(body)).expect("parse");
    let (_, pages) = dxpdf::render::resolve_and_layout(doc);
    pages
}

/// §17.9: list numbering runs in **document order**, so splitting a document
/// into two sections with a continuous break must not renumber it.
///
/// This guards the *relayout*, not the clearance peek: re-running a section's
/// layout must not re-run its block building, which is where list counters
/// advance (`inject_list_label`). Written first, while the relayout does not
/// exist yet, so it is green until that work lands and red the moment a
/// re-layout starts double-counting.
#[test]
fn continuous_break_does_not_renumber_lists() {
    let items = || format!("{}{}", list_item("Alpha"), list_item("Beta"));

    // One section, no break — the reference numbering.
    let single = layout(&format!(
        "{}{}<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdH1\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"/>{PG}</w:sectPr>",
        items(),
        list_item("Gamma"),
    ));

    // Same content, split by a continuous break.
    let split = layout(&format!(
        "<w:p><w:pPr><w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdH1\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"/>{PG}</w:sectPr></w:pPr></w:p>\
         {}{}<w:sectPr><w:type w:val=\"continuous\"/>{PG}</w:sectPr>",
        items(),
        list_item("Gamma"),
    ));

    let labels = |pages: &[dxpdf::render::layout::draw_command::LayoutedPage]| {
        drawn_text(pages)
            .into_iter()
            .filter(|t| {
                t.trim_end().ends_with('.') && t.trim_end_matches('.').parse::<u32>().is_ok()
            })
            .collect::<Vec<_>>()
    };

    let a = labels(&single);
    let b = labels(&split);
    assert!(
        !a.is_empty(),
        "fixture must actually render numbered labels; got {:?}",
        drawn_text(&single)
    );
    assert_eq!(
        a, b,
        "a continuous section break must not change §17.9 list numbering — \
         block building ran more than once for the same blocks"
    );
}

/// §17.6.22: content order across the break is document order. The break moves
/// the *page* boundary, never the sequence.
#[test]
fn continuous_break_preserves_content_order() {
    let pages = layout(&format!(
        "<w:p><w:r><w:t>Before</w:t></w:r></w:p>\
         <w:p><w:pPr><w:sectPr>{PG}</w:sectPr></w:pPr></w:p>\
         <w:p><w:r><w:t>After</w:t></w:r></w:p>\
         <w:sectPr><w:type w:val=\"continuous\"/>{PG}</w:sectPr>"
    ));

    let texts = drawn_text(&pages);
    let before = texts.iter().position(|t| t.contains("Before"));
    let after = texts.iter().position(|t| t.contains("After"));
    assert!(
        before.is_some() && after.is_some(),
        "both sections must render: {texts:?}"
    );
    assert!(
        before < after,
        "the section after a continuous break follows it in document order: {texts:?}"
    );
}

/// §17.6.22: "continuous" means the next section shares the current page. It
/// must not silently become a page break.
#[test]
fn continuous_break_shares_one_physical_page() {
    let pages = layout(&format!(
        "<w:p><w:r><w:t>Before</w:t></w:r></w:p>\
         <w:p><w:pPr><w:sectPr>{PG}</w:sectPr></w:pPr></w:p>\
         <w:p><w:r><w:t>After</w:t></w:r></w:p>\
         <w:sectPr><w:type w:val=\"continuous\"/>{PG}</w:sectPr>"
    ));

    assert_eq!(
        pages.len(),
        1,
        "two short sections joined by a continuous break occupy one page"
    );
}

// ── §17.10 header/footer clearance on a shared page (issue #83, plan §2) ─────

const R_NS: &str =
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

/// A `sectPr` referencing header `rel`, optionally as a continuous break.
fn sect_pr(rel: &str, continuous: bool) -> String {
    let ty = if continuous {
        r#"<w:type w:val="continuous"/>"#
    } else {
        ""
    };
    format!(
        r#"<w:sectPr>{ty}<w:headerReference w:type="default" r:id="{rel}" {R_NS}/>{PG}</w:sectPr>"#
    )
}

fn para(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

/// The y of the first draw command whose text is exactly `needle`.
fn y_of(pages: &[dxpdf::render::layout::draw_command::LayoutedPage], needle: &str) -> f32 {
    use dxpdf::render::layout::draw_command::DrawCommand;
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .find_map(|c| match c {
            DrawCommand::Text { text, position, .. } if text.contains(needle) => {
                Some(position.y.raw())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{needle:?} was never drawn"))
}

/// Two sections sharing one page: the first uses `first_hdr`, the second
/// continues on the same page under `second_hdr`.
fn shared_page(
    first_hdr: &str,
    second_hdr: &str,
) -> Vec<dxpdf::render::layout::draw_command::LayoutedPage> {
    layout(&format!(
        "{}<w:p><w:pPr>{}</w:pPr></w:p>{}{}",
        para("BODYBEFORE"),
        sect_pr(first_hdr, false),
        para("BODYAFTER"),
        sect_pr(second_hdr, true),
    ))
}

/// One section, so `hdr` governs the whole page — the reference layout the
/// shared page must reproduce.
///
/// The empty paragraph is not padding: in [`shared_page`] the section break
/// rides on a `w:p`'s `w:pPr`, and that paragraph mark is real content that
/// renders as a blank line. Without it here the two documents differ by a line
/// and the comparison measures the fixture rather than the engine.
fn single_section(hdr: &str) -> Vec<dxpdf::render::layout::draw_command::LayoutedPage> {
    layout(&format!(
        "{}<w:p/>{}{}",
        para("BODYBEFORE"),
        para("BODYAFTER"),
        sect_pr(hdr, false),
    ))
}

/// §17.6 / §17.10: ECMA-376 does not say which section owns the header of a
/// page holding a continuous break. This pins the engine's answer — the **last
/// section on the page** — so the clearance rule below has a stated premise
/// rather than an implicit one.
#[test]
fn shared_page_renders_the_succeeding_sections_header() {
    let pages = shared_page("rIdH1", "rIdH2");
    assert_eq!(pages.len(), 1, "the sections share one physical page");

    let texts = drawn_text(&pages);
    assert!(
        texts.iter().any(|t| t.contains("HDRTALL")),
        "the succeeding section's header is the one drawn: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("HDRSHORT")),
        "the preceding section's header must not also be drawn: {texts:?}"
    );
}

/// §17.10.1: the shared page is drawn with the succeeding section's header, so
/// **all** body content on it — including the preceding section's — must clear
/// that header. Laying the preceding section out against its own shorter
/// header leaves its content underneath the taller one that is actually drawn.
#[test]
fn taller_succeeding_header_does_not_overlap_preceding_content() {
    let shared = shared_page("rIdH1", "rIdH2");
    let reference = single_section("rIdH2");

    assert_eq!(
        y_of(&shared, "BODYBEFORE"),
        y_of(&reference, "BODYBEFORE"),
        "content preceding the break must sit where the tall header puts it, \
         not where the short header it was laid out against did"
    );
}

/// The converse, and the reason this is not simply "take the larger of the
/// two": a *shorter* succeeding header must reclaim the band the preceding
/// section's taller one reserved, rather than leaving it blank.
#[test]
fn shorter_succeeding_header_reclaims_the_band() {
    let shared = shared_page("rIdH2", "rIdH1");
    let reference = single_section("rIdH1");

    assert_eq!(
        y_of(&shared, "BODYBEFORE"),
        y_of(&reference, "BODYBEFORE"),
        "a shorter succeeding header must not retain the preceding section's \
         clearance"
    );
}

/// Equal slots are the no-op case: the relayout must not perturb a document
/// whose sections agree.
#[test]
fn equal_headers_leave_the_shared_page_unchanged() {
    let shared = shared_page("rIdH1", "rIdH1");
    let reference = single_section("rIdH1");

    assert_eq!(
        y_of(&shared, "BODYBEFORE"),
        y_of(&reference, "BODYBEFORE"),
        "the shared page's own bounds are unchanged"
    );
    assert_eq!(
        y_of(&shared, "BODYAFTER"),
        y_of(&reference, "BODYAFTER"),
        "§17.6.22: a continuous break inserts no vertical space — content after \
         it flows exactly where it would have without the break. This fails on \
         the *continuation cursor*, not the bounds: reconstructing it from draw \
         commands as `baseline + font_size` overshoots the true line bottom."
    );
}

/// §17.6.11: the resolved body bounds are a *floor* on the configured margin,
/// never a reduction of it. A header shorter than `w:pgMar/@top` leaves the
/// margin standing.
#[test]
fn shared_page_bounds_never_reduce_the_configured_margin() {
    // 1134tw = 56.7pt top margin; HDRSHORT is one 10pt line from 567tw
    // (28.35pt), so it ends well above the margin.
    const TOP_MARGIN_PT: f32 = 56.7;
    let shared = shared_page("rIdH1", "rIdH1");
    assert!(
        y_of(&shared, "BODYBEFORE") >= TOP_MARGIN_PT,
        "body must start at or below the configured top margin, got {}",
        y_of(&shared, "BODYBEFORE")
    );
}
