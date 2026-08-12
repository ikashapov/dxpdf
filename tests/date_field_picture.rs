//! Issue #159 end-to-end: a `DATE` field's `\@` picture must not leak the
//! backslash that escapes a character.
//!
//! `test-files/issue-159-minimal.docx` is the reporter's own document. Its four
//! `w:fldSimple` fields cover an escaped space (`MMM\ d, yyyy`), the same
//! picture without an escape, an escaped letter (`MMM d \a yyyy`), and no
//! picture at all. Each carries `CACHED` as its cached result — a deliberately
//! wrong value, so a renderer that echoes the cache instead of evaluating the
//! field is obvious rather than plausible.
//!
//! # What is asserted, and what cannot be
//!
//! §17.16.5.13 evaluates `DATE` against the moment of the render, seeded once
//! per document in `render::layout_document` from `field::now::now`. There is
//! no seam to inject a fixed instant, so the rendered date *string* is whatever
//! day the suite runs and cannot be asserted here. The two properties below are
//! the ones that hold on every day, and between them they pin the bug and the
//! wiring:
//!
//! 1. no rendered text contains a backslash — which is the defect itself;
//! 2. no rendered text is `CACHED` — so the fields were evaluated at all.
//!
//! The values each picture produces are pinned against a fixed `Date` by the
//! unit tests in `field::format`, which need no clock.

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};
use dxpdf::render::resolve_and_layout;

fn fixture() -> dxpdf::model::Document {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test-files/issue-159-minimal.docx"
    ))
    .expect("test-files/issue-159-minimal.docx — the reporter's document from issue #159");
    dxpdf::docx::parse(&bytes).expect("the fixture must parse")
}

fn page_text(page: &LayoutedPage) -> String {
    let mut out = String::new();
    for cmd in &page.commands {
        if let DrawCommand::Text { text, .. } = cmd {
            out.push_str(text);
            out.push(' ');
        }
    }
    out
}

fn rendered_text() -> String {
    let (_, pages) = resolve_and_layout(fixture());
    pages.iter().map(page_text).collect()
}

/// Rendered text with runs of whitespace collapsed. `page_text` joins each
/// draw command with a space, so a label the document writes as
/// `A  escaped space` arrives with the fragment joins folded in; the assertion
/// below is about which words appear, not about spacing.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The defect. §17.16.4.2: a backslash escapes the character that follows it,
/// so the escape is consumed and never reaches the output. 0.5.0 rendered
/// `Aug\ 11, 2026` for case A and `Aug 11 \a 2026` for case C.
#[test]
fn an_escaped_picture_does_not_render_its_backslash() {
    let text = rendered_text();
    assert!(
        !text.contains('\\'),
        "issue #159: a picture escape leaked into the output: {text:?}"
    );
}

/// The reporter's control. A renderer that never evaluates the fields would
/// also pass the assertion above — 0.4.0 does exactly that, echoing `CACHED`
/// for all four cases — so the cached result has to be gone too.
#[test]
fn the_date_fields_are_evaluated_not_echoed_from_the_cache() {
    let text = rendered_text();
    assert!(
        !text.contains("CACHED"),
        "the cached field result was rendered instead of being evaluated: {text:?}"
    );
}

/// The fixture's own labels must survive, so the two assertions above are
/// reading real output rather than passing on an empty page.
#[test]
fn the_fixture_renders_its_labels() {
    let text = words(&rendered_text());
    for label in [
        "escaped space",
        "plain space",
        "escaped letter",
        "no picture",
    ] {
        assert!(text.contains(label), "missing label {label:?} in {text:?}");
    }
}
