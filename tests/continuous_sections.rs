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
  <Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/>
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
  <Relationship Id="rIdFn" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>
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

    // §17.11.23: two notes, one referenced on each side of the break.
    zip.start_file("word/footnotes.xml", o).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:type="separator" w:id="0"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
  <w:footnote w:type="continuationSeparator" w:id="1"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>
  <w:footnote w:id="2"><w:p><w:r><w:t>NOTEBEFORE</w:t></w:r></w:p></w:footnote>
  <w:footnote w:id="3"><w:p><w:r><w:t>NOTEAFTER</w:t></w:r></w:p></w:footnote>
</w:footnotes>"#,
    )
    .unwrap();

    zip.start_file("word/document.xml", o).unwrap();
    zip.write_all(
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
            xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
            xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
            xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">
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

// ── §17.6.22 continuation fidelity (issue #83, plan §3) ─────────────────────

/// Content width for [`PG`]: 11906 − 1134 − 1134 twips.
const CONTENT_WIDTH_PT: f32 = (11906.0 - 1134.0 - 1134.0) / 20.0;

/// §17.11.23: the separator is drawn at one third of the content width, which
/// is distinctive enough to count in a fixture with no borders or tables.
fn footnote_separator_count(pages: &[dxpdf::render::layout::draw_command::LayoutedPage]) -> usize {
    use dxpdf::render::layout::draw_command::DrawCommand;
    let want = CONTENT_WIDTH_PT * 0.33;
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter(|c| match c {
            DrawCommand::Line { line, .. } => {
                ((line.end.x - line.start.x).raw() - want).abs() < 0.5
            }
            _ => false,
        })
        .count()
}

/// The y of the footnote separator on `page`, if one was drawn.
fn footnote_separator_y(page: &dxpdf::render::layout::draw_command::LayoutedPage) -> Option<f32> {
    use dxpdf::render::layout::draw_command::DrawCommand;
    let want = CONTENT_WIDTH_PT * 0.33;
    page.commands.iter().find_map(|c| match c {
        DrawCommand::Line { line, .. }
            if ((line.end.x - line.start.x).raw() - want).abs() < 0.5 =>
        {
            Some(line.start.y.raw())
        }
        _ => None,
    })
}

/// A paragraph carrying a footnote reference.
fn para_with_note(text: &str, id: u32) -> String {
    format!(
        r#"<w:p><w:r><w:t>{text}</w:t></w:r><w:r><w:footnoteReference w:id="{id}"/></w:r></w:p>"#
    )
}

/// §17.11.23: a page carries **one** separator, above all the notes on it. Two
/// sections contributing notes to the same physical page is still one page.
///
/// Each section flushing its own notes draws a second separator, and positions
/// it from the page's unreduced bottom — so the second block is laid over the
/// first rather than above it.
#[test]
fn footnotes_from_both_sections_share_one_separator() {
    let pages = layout(&format!(
        "{}<w:p><w:pPr>{}</w:pPr></w:p>{}{}",
        para_with_note("BODYBEFORE", 2),
        sect_pr("rIdH1", false),
        para_with_note("BODYAFTER", 3),
        sect_pr("rIdH1", true),
    ));

    assert_eq!(pages.len(), 1, "one shared physical page");
    let texts = drawn_text(&pages);
    assert!(
        texts.iter().any(|t| t.contains("NOTEBEFORE")),
        "the note referenced before the break must render: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("NOTEAFTER")),
        "the note referenced after the break must render: {texts:?}"
    );
    assert_eq!(
        footnote_separator_count(&pages),
        1,
        "§17.11.23: one separator per page, not one per section"
    );
}

/// §17.11.23: notes reserve space at the page bottom, and content after the
/// break must respect that reservation — the succeeding section inherits a
/// bottom already reduced by what the preceding one reserved, not the page's
/// unreduced bottom.
#[test]
fn content_after_the_break_clears_footnotes_reserved_before_it() {
    // The succeeding section must fill past the page bottom for the
    // reservation to bite: a couple of short paragraphs never reach it, and the
    // test would pass without anything being carried.
    let filler: String = (0..120).map(|i| para(&format!("FILL{i}"))).collect();
    let pages = layout(&format!(
        "{}<w:p><w:pPr>{}</w:pPr></w:p>{}{}",
        para_with_note("BODYBEFORE", 2),
        sect_pr("rIdH1", false),
        filler,
        sect_pr("rIdH1", true),
    ));

    // The separator is the top of the reserved block — comparing against the
    // note's own baseline instead would call a line drawn *between* separator
    // and note a pass.
    let separator_y = footnote_separator_y(&pages[0]).expect("a separator on the shared page");
    let last_body_y = {
        use dxpdf::render::layout::draw_command::DrawCommand;
        pages[0]
            .commands
            .iter()
            .filter_map(|c| match c {
                DrawCommand::Text { text, position, .. } if text.starts_with("FILL") => {
                    Some(position.y.raw())
                }
                _ => None,
            })
            .fold(f32::MIN, f32::max)
    };
    assert!(
        last_body_y > f32::MIN,
        "the succeeding section must actually reach the shared page"
    );
    assert!(
        last_body_y < separator_y,
        "body after the break (lowest line {last_body_y}) must stay above the \
         footnote separator ({separator_y}) reserved before it"
    );
}

/// §17.3.1.9: `space_before` and `space_after` collapse between adjacent
/// paragraphs. A continuous break is not a paragraph boundary of its own, so
/// the collapse must survive it — the succeeding section must know what the
/// preceding one's last paragraph left behind.
#[test]
fn paragraph_spacing_collapses_across_the_break() {
    let spaced = |text: &str, before: u32, after: u32| {
        format!(
            r#"<w:p><w:pPr><w:spacing w:before="{before}" w:after="{after}"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
        )
    };
    // 400tw = 20pt after, 400tw = 20pt before: collapsed to 20pt, not 40pt.
    //
    // The break rides on BODYBEFORE's own `w:pPr` rather than a following empty
    // paragraph: an empty paragraph is a real paragraph mark that renders a
    // blank line *and* takes part in the collapse, so it would make the two
    // documents incomparable.
    let split = layout(&format!(
        r#"<w:p><w:pPr><w:spacing w:before="0" w:after="400"/>{}</w:pPr><w:r><w:t>BODYBEFORE</w:t></w:r></w:p>{}"#,
        sect_pr("rIdH1", false),
        spaced("BODYAFTER", 400, 0) + &sect_pr("rIdH1", true),
    ));
    let joined = layout(&format!(
        "{}{}{}",
        spaced("BODYBEFORE", 0, 400),
        spaced("BODYAFTER", 400, 0),
        sect_pr("rIdH1", false),
    ));

    let gap = |p: &[dxpdf::render::layout::draw_command::LayoutedPage]| {
        y_of(p, "BODYAFTER") - y_of(p, "BODYBEFORE")
    };
    assert!(
        (gap(&split) - gap(&joined)).abs() < 0.01,
        "collapse must survive the break: split gap {}, unbroken gap {}",
        gap(&split),
        gap(&joined),
    );
}

/// §17.3.1.33: `space_before` is suppressed at the top of a page. The first
/// paragraph *after* a continuous break is mid-page, so its `space_before`
/// applies — treating it as a page top swallows the space.
#[test]
fn space_before_is_not_suppressed_after_a_mid_page_break() {
    let with_space = |before: u32| {
        let after_para = format!(
            r#"<w:p><w:pPr><w:spacing w:before="{before}"/></w:pPr><w:r><w:t>BODYAFTER</w:t></w:r></w:p>"#
        );
        format!(
            "{}<w:p><w:pPr>{}</w:pPr></w:p>{after_para}{}",
            para("BODYBEFORE"),
            sect_pr("rIdH1", false),
            sect_pr("rIdH1", true),
        )
    };
    let none = layout(&with_space(0));
    let spaced = layout(&with_space(600)); // 30pt

    let gap = |p: &[dxpdf::render::layout::draw_command::LayoutedPage]| {
        y_of(p, "BODYAFTER") - y_of(p, "BODYBEFORE")
    };
    assert!(
        gap(&spaced) - gap(&none) > 29.0,
        "30pt of space_before must apply mid-page: {} vs {}",
        gap(&spaced),
        gap(&none),
    );
}

/// §17.3.3.1: an explicit page break in the paragraph *before* a continuous
/// break still moves the following content to a new page. The break is deferred
/// to the start of the next block, so it has to survive the section boundary.
#[test]
fn an_inline_page_break_before_the_continuous_break_is_honoured() {
    let pages = layout(&format!(
        r#"<w:p><w:r><w:t>BODYBEFORE</w:t></w:r><w:r><w:br w:type="page"/></w:r></w:p>
           <w:p><w:pPr>{}</w:pPr></w:p>{}{}"#,
        sect_pr("rIdH1", false),
        para("BODYAFTER"),
        sect_pr("rIdH1", true),
    ));

    assert_eq!(
        pages.len(),
        2,
        "the page break before the section break must still start a new page"
    );
}

/// §20.4.2.17 `wrapSquare` — a left-hand floating shape 200pt wide, anchored to
/// its paragraph, that text must flow to the right of.
const FLOAT_PARA: &str = r#"<w:p><w:r><w:drawing>
  <wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0"
             relativeHeight="1" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">
    <wp:simplePos x="0" y="0"/>
    <wp:positionH relativeFrom="margin"><wp:posOffset>0</wp:posOffset></wp:positionH>
    <wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>
    <wp:extent cx="2540000" cy="2540000"/>
    <wp:wrapSquare wrapText="bothSides"/>
    <wp:docPr id="1" name="Box"/>
    <a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">
      <wps:wsp>
        <wps:cNvSpPr/>
        <wps:spPr>
          <a:xfrm><a:off x="0" y="0"/><a:ext cx="2540000" cy="2540000"/></a:xfrm>
          <a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
          <a:solidFill><a:srgbClr val="DDDDDD"/></a:solidFill>
        </wps:spPr>
      </wps:wsp>
    </a:graphicData></a:graphic>
  </wp:anchor>
</w:drawing></w:r></w:p>"#;

/// §20.4.2: a float is a property of the *page*, not of the section that
/// registered it. Text after a continuous break shares the page, so it must
/// wrap around a float placed before the break — dropping the float set at the
/// boundary lets that text run straight through the shape.
#[test]
fn text_after_the_break_wraps_around_a_float_registered_before_it() {
    let x_of = |pages: &[dxpdf::render::layout::draw_command::LayoutedPage], needle: &str| {
        use dxpdf::render::layout::draw_command::DrawCommand;
        pages
            .iter()
            .flat_map(|p| &p.commands)
            .find_map(|c| match c {
                DrawCommand::Text { text, position, .. } if text.contains(needle) => {
                    Some(position.x.raw())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} was never drawn"))
    };

    let pages = layout(&format!(
        "{FLOAT_PARA}<w:p><w:pPr>{}</w:pPr></w:p>{}{}",
        sect_pr("rIdH1", false),
        para("BODYAFTER"),
        sect_pr("rIdH1", true),
    ));

    // 2540000 EMU = 200pt, anchored at the left margin (56.7pt).
    let left_margin = 1134.0 / 20.0;
    let x = x_of(&pages, "BODYAFTER");
    assert!(
        x > left_margin + 100.0,
        "text after the break must clear the 200pt float registered before it, \
         but starts at x={x} (left margin {left_margin})"
    );
}
