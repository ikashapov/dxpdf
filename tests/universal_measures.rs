//! §22.9.2.15 / §22.9.2.9 end-to-end: a document whose every measurement is
//! spelled with an explicit unit (`"595.30pt"`, `"2cm"`) or as a percentage
//! (`"50%"`) parses to the same model a bare-number spelling would.
//!
//! `test-files/universal-measures.docx` (built by
//! `scripts/make_universal_measures_fixture.py`) is the committed stand-in
//! for a Word "Strict Open XML" export, which writes these spellings for the
//! page geometry — the reported failure was a service rejecting every Strict
//! upload over its `<w:pgSz>`. The assertions pin the *converted* values in
//! each target's native unit, so a wrong per-unit ratio fails here even if
//! parsing succeeds.

use dxpdf::model::*;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/test-files/universal-measures.docx"
);

fn load() -> Document {
    let bytes = std::fs::read(FIXTURE).expect("fixture is committed");
    dxpdf::docx::parse(&bytes).expect("universal-measure spellings must parse")
}

/// The A4 page declared as `595.30pt × 841.90pt` with mixed-unit margins
/// lands in twips: pt × 20, cm × 567 (rounded), in × 1440.
#[test]
fn page_geometry_converts_to_twips() {
    let doc = load();
    let sect = &doc.final_section;
    let size = sect.page_size.get().expect("fixture declares a page size");
    assert_eq!(
        size.width.map(|w| w.raw()),
        Some(11906),
        "595.30pt in twips"
    );
    assert_eq!(
        size.height.map(|h| h.raw()),
        Some(16838),
        "841.90pt in twips"
    );

    let margins = sect.page_margins.get().expect("fixture declares margins");
    assert_eq!(margins.top.map(|v| v.raw()), Some(1134), "2cm in twips");
    assert_eq!(margins.right.map(|v| v.raw()), Some(850), "1.5cm in twips");
    assert_eq!(margins.left.map(|v| v.raw()), Some(1440), "1in in twips");
    assert_eq!(
        margins.header.map(|v| v.raw()),
        Some(708),
        "35.40pt in twips"
    );
}

/// `w:sz w:val="14pt"` is a half-point target: 28, not 14 — the case that
/// separates converting to the attribute's own unit from echoing the number.
#[test]
fn run_size_converts_to_half_points() {
    let doc = load();
    let sizes: Vec<i64> = doc
        .body
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .flat_map(|p| &p.content)
        .filter_map(|inline| match inline {
            Inline::TextRun(run) => run.properties.font_size.get().map(|s| s.raw()),
            _ => None,
        })
        .collect();
    assert_eq!(sizes, vec![28], "14pt is 28 half-points");
}

/// §17.18.87's two non-bare spellings, one table each: a `dxa` width in
/// points and a `pct` width as a percentage.
#[test]
fn table_widths_take_both_spellings() {
    let doc = load();
    let widths: Vec<TableMeasure> = doc
        .body
        .iter()
        .filter_map(|block| match block {
            Block::Table(t) => t.properties.width.get().copied(),
            _ => None,
        })
        .collect();
    assert_eq!(widths.len(), 2, "fixture holds two tables");
    match widths[0] {
        TableMeasure::Twips(d) => assert_eq!(d.raw(), 5953, "297.65pt in twips"),
        other => panic!("expected Twips, got {other:?}"),
    }
    match widths[1] {
        TableMeasure::Pct(d) => assert_eq!(d.raw(), 2500, "50% in fiftieths"),
        other => panic!("expected Pct, got {other:?}"),
    }
}

/// The whole document renders: the parse must not merely produce a model but
/// one layout accepts, since the reported symptom was a converter refusing
/// the file outright.
#[test]
fn fixture_converts_end_to_end() {
    let bytes = std::fs::read(FIXTURE).expect("fixture is committed");
    let pdf = dxpdf::convert(&bytes).expect("fixture must convert");
    assert!(pdf.starts_with(b"%PDF"), "output is a PDF");
}
