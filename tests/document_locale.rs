//! §17.3.2.20 `w:lang` — the document's language, and the two things layout
//! does with it.
//!
//! The tag is parsed and cascades like any other run property, but nothing
//! downstream read it, so two decisions were made in English regardless of what
//! the document said:
//!
//! * §17.18.85 — a `decimal` tab aligned on `.`, so a German column of figures
//!   found no separator and fell through to the right-aligning degrade;
//! * §17.9.27 — `ordinal` spelled English suffixes onto a German list, and
//!   `cardinalText` / `ordinalText` were not rendered as words at all.
//!
//! Neither symptom appears in `test-files/` or `test-cases/`: the corpus's one
//! decimal tab is in an `en-US` document and no numbering definition in it uses
//! a text format (counted, not assumed). The fixtures are therefore built here.

use std::io::Write;

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};

fn make_docx(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let overrides: String = parts
            .iter()
            .map(|(name, _)| {
                let content_type = match *name {
                    "word/document.xml" => "wordprocessingml.document.main",
                    "word/styles.xml" => "wordprocessingml.styles",
                    "word/numbering.xml" => "wordprocessingml.numbering",
                    other => panic!("no content type registered for {other}"),
                };
                format!(
                    r#"<Override PartName="/{name}" ContentType="application/vnd.openxmlformats-officedocument.{content_type}+xml"/>"#
                )
            })
            .collect();

        zip.start_file("[Content_Types].xml", o).unwrap();
        zip.write_all(
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  {overrides}
</Types>"#
            )
            .as_bytes(),
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

        zip.start_file("word/_rels/document.xml.rels", o).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
</Relationships>"#,
        )
        .unwrap();

        for (name, body) in parts {
            zip.start_file(*name, o).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    buf
}

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

/// `styles.xml` whose `docDefaults` declare `lang` — the document-wide default
/// every other layer falls back to.
fn styles(default_lang: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles {W}>
  <w:docDefaults><w:rPrDefault><w:rPr>
    <w:lang w:val="{default_lang}"/>
  </w:rPr></w:rPrDefault></w:docDefaults>
</w:styles>"#
    )
}

fn layout(parts: &[(&str, &str)]) -> Vec<LayoutedPage> {
    let doc = dxpdf::docx::parse(&make_docx(parts)).expect("fixture parses");
    dxpdf::render::resolve_and_layout(doc).1
}

/// Every drawn string with its x, in draw order.
fn texts(pages: &[LayoutedPage]) -> Vec<(String, f32)> {
    pages
        .iter()
        .flat_map(|p| p.commands.iter())
        .filter_map(|c| match c {
            DrawCommand::Text { text, position, .. } => Some((text.to_string(), position.x.raw())),
            _ => None,
        })
        .collect()
}

// ── §17.18.85 `decimal` tabs ────────────────────────────────────────────────

/// Two entries against one decimal stop. `mark_lang`, if non-empty, is spliced
/// into `<w:pPr><w:rPr>` — the paragraph mark, which is what governs the stop.
fn decimal_tab_document(entries: &[&str], mark_lang: &str) -> String {
    let mark_rpr = if mark_lang.is_empty() {
        String::new()
    } else {
        format!(r#"<w:rPr><w:lang w:val="{mark_lang}"/></w:rPr>"#)
    };
    let paragraphs: String = entries
        .iter()
        .map(|entry| {
            format!(
                r#"<w:p>
  <w:pPr><w:tabs><w:tab w:val="decimal" w:pos="4320"/></w:tabs>{mark_rpr}</w:pPr>
  <w:r><w:tab/><w:t>{entry}</w:t></w:r>
</w:p>"#
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document {W}><w:body>{paragraphs}</w:body></w:document>"#
    )
}

/// Whether the separators of two entries were aligned on each other.
///
/// Coordinate-free, and the only property that matters: two numbers whose
/// integer parts differ in width start at *different* x exactly when the stop
/// found a separator in each. A stop that found none right-aligns both, which
/// puts the wider one further left by its own extra digits — so the sign of the
/// difference cannot distinguish them, but this can: with separators found,
/// `"1,5"` and `"333,125"` start 2 digits apart; right-aligned, 3.
fn separator_offsets(pages: &[LayoutedPage]) -> Vec<f32> {
    texts(pages).into_iter().map(|(_, x)| x).collect()
}

/// The baseline: an English document aligns on the point it always has.
#[test]
fn an_english_document_aligns_a_decimal_tab_on_the_point() {
    let xs = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1.5", "333.125"], "en-US"),
        ),
        ("word/styles.xml", &styles("en-US")),
    ]));

    // Both put their `.` on the stop, so the wider integer part starts further
    // left by exactly its two extra digits.
    assert!(xs[0] > xs[1], "aligned on the separator: {xs:?}");
}

/// §17.18.85 in a comma-decimal document. `1,5` and `333,125` must align on
/// their commas — which means starting at *different* x, by the width of the
/// wider integer part alone.
#[test]
fn a_german_document_aligns_a_decimal_tab_on_the_comma() {
    let german = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], "de-DE"),
        ),
        ("word/styles.xml", &styles("de-DE")),
    ]));
    let english_dots = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1.5", "333.125"], "en-US"),
        ),
        ("word/styles.xml", &styles("en-US")),
    ]));

    // The two documents are the same shape with the separator swapped, so the
    // German commas must land the same way the English points do.
    let german_gap = german[0] - german[1];
    let english_gap = english_dots[0] - english_dots[1];
    assert!(
        (german_gap - english_gap).abs() < 1.5,
        "a comma must anchor a German zone the way a point anchors an English \
         one: German gap {german_gap}, English gap {english_gap}",
    );
}

/// …and the same German document written with points has no separator the
/// locale recognises, so it takes §17.18.85's settled degrade: right-aligned,
/// keeping the column flush rather than anchoring on a thousands separator.
#[test]
fn a_german_document_right_aligns_a_zone_written_with_points() {
    let xs = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1.5", "333.125"], "de-DE"),
        ),
        ("word/styles.xml", &styles("de-DE")),
    ]));

    // Right-aligned: every zone *ends* at the stop, so the wider one starts
    // further left by its full extra width — three characters, not two.
    let aligned = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], "de-DE"),
        ),
        ("word/styles.xml", &styles("de-DE")),
    ]));
    assert!(
        (xs[0] - xs[1]) > (aligned[0] - aligned[1]) + 1.0,
        "a point in a German document anchors nothing: degraded {:?} vs \
         anchored {:?}",
        xs[0] - xs[1],
        aligned[0] - aligned[1],
    );
}

/// The document default reaches a paragraph that declares nothing of its own.
#[test]
fn the_document_default_language_governs_a_paragraph_that_declares_none() {
    let xs = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], ""),
        ),
        ("word/styles.xml", &styles("de-DE")),
    ]));
    let english = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], ""),
        ),
        ("word/styles.xml", &styles("en-US")),
    ]));

    assert!(
        (xs[0] - xs[1]) < (english[0] - english[1]) - 1.0,
        "docDefaults de-DE anchors the comma; en-US does not: {xs:?} vs {english:?}",
    );
}

/// …and the paragraph mark **overrides** it. §17.7.2 resolves `w:lang` like any
/// other run property, so the nearer layer wins — which is also the only test
/// here that can tell the two layers apart, every other fixture setting both to
/// the same tag.
#[test]
fn the_paragraph_mark_overrides_the_document_default_language() {
    let german_mark = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], "de-DE"),
        ),
        ("word/styles.xml", &styles("en-US")),
    ]));
    let default_only = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], ""),
        ),
        ("word/styles.xml", &styles("en-US")),
    ]));

    assert!(
        (german_mark[0] - german_mark[1]) < (default_only[0] - default_only[1]) - 1.0,
        "a de-DE paragraph mark beats an en-US docDefault: {german_mark:?} vs \
         {default_only:?}",
    );
}

/// The converse, so the test above cannot pass by the mark being ignored in
/// both directions: an English mark in a German document keeps the point.
#[test]
fn an_english_paragraph_mark_overrides_a_german_document_default() {
    let english_mark = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], "en-US"),
        ),
        ("word/styles.xml", &styles("de-DE")),
    ]));
    let default_only = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1,5", "333,125"], ""),
        ),
        ("word/styles.xml", &styles("de-DE")),
    ]));

    assert!(
        (english_mark[0] - english_mark[1]) > (default_only[0] - default_only[1]) + 1.0,
        "an en-US paragraph mark beats a de-DE docDefault: {english_mark:?} vs \
         {default_only:?}",
    );
}

/// A tag this engine does not know changes nothing — the point stays the
/// separator, so an unfamiliar document renders exactly as it did before
/// `w:lang` was ever read.
#[test]
fn an_unrecognised_language_tag_leaves_the_separator_alone() {
    let unknown = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1.5", "333.125"], "zz-ZZ"),
        ),
        ("word/styles.xml", &styles("zz-ZZ")),
    ]));
    let english = separator_offsets(&layout(&[
        (
            "word/document.xml",
            &decimal_tab_document(&["1.5", "333.125"], "en-US"),
        ),
        ("word/styles.xml", &styles("en-US")),
    ]));

    assert_eq!(unknown, english, "an unknown tag is a no-op, not a degrade");
}

// ── §17.9.27 numbering text formats ─────────────────────────────────────────

fn numbering(format: &str) -> String {
    numbering_with_level_lang(format, "")
}

/// `level_lang`, if non-empty, is spliced into `<w:lvl><w:rPr>` — the top of
/// the §17.9.23 label cascade.
fn numbering_with_level_lang(format: &str, level_lang: &str) -> String {
    let level_rpr = if level_lang.is_empty() {
        String::new()
    } else {
        format!(r#"<w:rPr><w:lang w:val="{level_lang}"/></w:rPr>"#)
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:numbering {W}>
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="{format}"/>
      <w:lvlText w:val="%1"/>
      <w:suff w:val="space"/>
      {level_rpr}
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
</w:numbering>"#
    )
}

/// Three numbered paragraphs, so the labels are 1, 2, 3.
fn numbered_document(mark_lang: &str) -> String {
    let mark_rpr = if mark_lang.is_empty() {
        String::new()
    } else {
        format!(r#"<w:rPr><w:lang w:val="{mark_lang}"/></w:rPr>"#)
    };
    let paragraphs: String = (1..=3)
        .map(|i| {
            format!(
                r#"<w:p>
  <w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>{mark_rpr}</w:pPr>
  <w:r><w:t>item{i}</w:t></w:r>
</w:p>"#
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document {W}><w:body>{paragraphs}</w:body></w:document>"#
    )
}

/// The three list labels, in order.
fn labels(format: &str, lang: &str) -> Vec<String> {
    let pages = layout(&[
        ("word/document.xml", &numbered_document(lang)),
        ("word/styles.xml", &styles(lang)),
        ("word/numbering.xml", &numbering(format)),
    ]);
    texts(&pages)
        .into_iter()
        .map(|(t, _)| t.trim().to_string())
        .filter(|t| !t.starts_with("item"))
        .filter(|t| !t.is_empty())
        .collect()
}

#[test]
fn english_ordinals_are_spelled_with_english_suffixes() {
    assert_eq!(labels("ordinal", "en-US"), ["1st", "2nd", "3rd"]);
}

/// §17.9.27 `ordinal` is language-dependent, and this engine knows the suffixes
/// of exactly one language. Emitting `1st` onto a German list is not a degrade,
/// it is a different language's text — so a language whose ordinals we cannot
/// spell gets the digits, which is what Word writes when it has no rule either.
#[test]
fn a_german_document_does_not_get_english_ordinal_suffixes() {
    assert_eq!(labels("ordinal", "de-DE"), ["1", "2", "3"]);
}

#[test]
fn english_cardinal_text_is_spelled_out() {
    assert_eq!(labels("cardinalText", "en-US"), ["One", "Two", "Three"]);
}

#[test]
fn english_ordinal_text_is_spelled_out() {
    assert_eq!(labels("ordinalText", "en-US"), ["First", "Second", "Third"]);
}

#[test]
fn a_german_document_gets_digits_rather_than_english_number_words() {
    assert_eq!(labels("cardinalText", "de-DE"), ["1", "2", "3"]);
    assert_eq!(labels("ordinalText", "de-DE"), ["1", "2", "3"]);
}

/// An unrecognised tag keeps today's behaviour here too: English words, because
/// that is what every document got before `w:lang` was read. Degrading it to
/// digits would make an unfamiliar tag *lose* labels it renders today.
#[test]
fn an_unrecognised_language_tag_still_gets_english_number_words() {
    assert_eq!(labels("ordinal", "zz-ZZ"), ["1st", "2nd", "3rd"]);
    assert_eq!(labels("cardinalText", "zz-ZZ"), ["One", "Two", "Three"]);
}

/// §17.9.23: a label reads its language from its *own* cascade, whose top layer
/// is the level's `<w:rPr>` — the same layer its font and colour come from. A
/// German document whose numbering level declares English gets English words.
#[test]
fn a_numbering_levels_own_language_beats_the_paragraphs() {
    let pages = layout(&[
        ("word/document.xml", &numbered_document("de-DE")),
        ("word/styles.xml", &styles("de-DE")),
        (
            "word/numbering.xml",
            &numbering_with_level_lang("cardinalText", "en-US"),
        ),
    ]);
    let got: Vec<String> = texts(&pages)
        .into_iter()
        .map(|(t, _)| t.trim().to_string())
        .filter(|t| !t.starts_with("item") && !t.is_empty())
        .collect();

    assert_eq!(got, ["One", "Two", "Three"]);
}

/// Formats that are not language-dependent are untouched by any of this.
#[test]
fn language_independent_number_formats_are_unchanged_by_locale() {
    for lang in ["en-US", "de-DE", "ja-JP", "zz-ZZ"] {
        assert_eq!(labels("decimal", lang), ["1", "2", "3"], "decimal/{lang}");
        assert_eq!(
            labels("lowerRoman", lang),
            ["i", "ii", "iii"],
            "lowerRoman/{lang}"
        );
        assert_eq!(
            labels("upperLetter", lang),
            ["A", "B", "C"],
            "upperLetter/{lang}"
        );
    }
}
