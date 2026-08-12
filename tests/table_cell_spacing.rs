//! §17.4.44 `w:tblCellSpacing` geometry — issue #165.
//!
//! `test-files/issue-165-cellspacing-scale.docx` is four otherwise-identical
//! tables — same `w:tblW`, same three columns, same fixed layout — at spacings
//! 0, 200, 400 twips, and 400 with a row-level 800. Everything asserted here is
//! a *difference between two of those tables*, so no glyph metric, cell margin
//! or border width has to be known: whatever those contribute, they contribute
//! equally to all four and cancel.
//!
//! # Why this file exists
//!
//! Word draws `issue-165-cellspacing.docx` with gaps about twice this engine's,
//! and the obvious conclusion — that the declared value is half the gap — is
//! wrong. ONLYOFFICE, an independent implementation that both renders and
//! targets Word compatibility, applies **no factor**: `sdkjs`'s
//! `TableRecalculate.js` insets a cell by `CellSpacing` on the table's outer
//! edges and by `CellSpacing / 2` on every interior side, so an interior gap is
//! two halves and an edge gap one whole — every gap equals the declared value.
//! Neither ECMA-376 §17.4.44 nor [MS-OI29500] states any factor either. See
//! `build::table::resolve_cell_spacing` for the evidence in full.
//!
//! The doubling is far likelier to come from the probe than from a factor:
//! `issue-165-cellspacing.docx` declares the spacing in `tblPr` *and* again in
//! `trPr`, so a Word that sums the two lands on exactly twice. This fixture
//! declares it at table level only in tables 2 and 3, which is what separates
//! the two explanations, and its table 4 carries 400 with a row-level 800, which
//! measures the sum directly.
//!
//! The tests below pin current behaviour on four independent axes, each failing
//! separately, so a Word render can refute one without disturbing the rest. Two
//! are corroborated by ONLYOFFICE — the absent factor, and the carve. Two are
//! measured against nothing:
//!
//! * that the gap scales *linearly* with the declared value, rather than the
//!   spec's "including the width of the table borders" hiding a constant offset;
//! * that a row-level `w:tblCellSpacing` is ignored. §17.4.44 says it should
//!   *supersede* the table-level value, and this engine warns that it does not.
//!   The last table exists so that gap has a test to fail when it is closed.

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/test-files/issue-165-cellspacing-scale.docx"
);

/// One table's measurements, in page coordinates.
struct Table {
    /// x of the first cell's text — the left cell edge plus a cell margin that
    /// is identical in every table here.
    first_cell_text_x: f32,
    /// x of the table's rightmost border rect.
    right_edge: f32,
}

/// The four tables, in document order, identified by the tag their cells carry
/// (`T1C1`, `T2C1`, …). The readable description of each is the heading above
/// it in the fixture.
fn tables() -> Vec<Table> {
    let bytes = std::fs::read(FIXTURE).expect("fixture is committed");
    let doc = dxpdf::docx::parse(&bytes).expect("fixture parses");
    let pages: Vec<LayoutedPage> = dxpdf::render::resolve_and_layout(doc).1;
    let commands: Vec<&DrawCommand> = pages.iter().flat_map(|p| &p.commands).collect();

    ["T1", "T2", "T3", "T4"]
        .iter()
        .map(|tag| {
            // One unbroken token per cell, so it survives line fitting as a
            // single draw command and no other cell's text is a prefix of it —
            // see the fixture builder for why the labels are spelled this way.
            let cell1 = format!("{tag}C1");
            let (text_x, text_y) = commands
                .iter()
                .find_map(|c| match c {
                    DrawCommand::Text { position, text, .. } if **text == *cell1 => {
                        Some((position.x.raw(), position.y.raw()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no cell text {cell1:?} on the page"));

            // A *vertical* border rect spans its whole cell or its whole table,
            // so it contains this table's text baseline; a horizontal one sits
            // entirely above or below it, and another table's sits elsewhere on
            // the page. Selecting by containment therefore picks out exactly
            // this table's left/right edges without needing to know how tall it
            // grew — which is the thing the spacing changes.
            let right_edge = commands
                .iter()
                .filter_map(|c| match c {
                    DrawCommand::Rect { rect, .. }
                        if rect.origin.y.raw() <= text_y
                            && text_y <= rect.origin.y.raw() + rect.size.height.raw() =>
                    {
                        Some(rect.origin.x.raw() + rect.size.width.raw())
                    }
                    _ => None,
                })
                .fold(f32::MIN, f32::max);
            assert!(
                right_edge > text_x,
                "{tag}: found no vertical border rects straddling this table's baseline"
            );

            Table {
                first_cell_text_x: text_x,
                right_edge,
            }
        })
        .collect()
}

/// §17.4.44: the rendered gap *is* the declared value — no factor.
///
/// Corroborated by ONLYOFFICE's renderer; see the module docs. Read as a
/// difference against the zero-spacing table, which cancels the cell margin and
/// the border width — whatever they are, the first cell's text sits the same
/// distance inside its cell in all four tables, so the shift between two tables
/// *is* the difference in their spacing.
#[test]
fn the_rendered_gap_is_the_declared_spacing() {
    let t = tables();
    // 200 twips = 10pt and 400 twips = 20pt, at the table's own edge.
    for (idx, declared_pt, label) in [(1usize, 10.0_f32, "T2"), (2, 20.0, "T3")] {
        let shift = t[idx].first_cell_text_x - t[0].first_cell_text_x;
        assert!(
            (shift - declared_pt).abs() < 0.01,
            "{label}: the first cell should start {declared_pt}pt inside the table, \
             got {shift}pt"
        );
    }
}

/// And it scales linearly, which is what tells a factor from a constant offset.
/// §17.4.44 folds "the width of the table borders" into the spacing, so an
/// implementation that subtracted a border width would still look roughly
/// doubled at one value and wrong at another.
#[test]
fn doubling_the_declared_spacing_doubles_the_gap() {
    let t = tables();
    let at_200 = t[1].first_cell_text_x - t[0].first_cell_text_x;
    let at_400 = t[2].first_cell_text_x - t[0].first_cell_text_x;
    assert!(
        (at_400 - at_200 * 2.0).abs() < 0.01,
        "400 twips must give exactly twice the gap of 200: {at_200}pt vs {at_400}pt"
    );
}

/// The spacing is carved out of `w:tblW`, not added to it: all four tables
/// declare the same width and must render the same width, with the cells
/// shrinking as the gaps grow.
///
/// Corroborated by ONLYOFFICE, which insets cells within their grid slots and so
/// leaves the table's own width alone. Not confirmed against Word itself, and
/// pinned so that a render contradicting it fails loudly instead of being
/// absorbed into the numbers above.
#[test]
fn spacing_is_carved_out_of_the_declared_table_width() {
    let t = tables();
    for (idx, label) in [(1usize, "T2"), (2, "T3"), (3, "T4")] {
        assert!(
            (t[idx].right_edge - t[0].right_edge).abs() < 0.01,
            "{label}: table width must not change with the spacing — {} vs {}",
            t[idx].right_edge,
            t[0].right_edge
        );
    }
}

/// §17.4.44: a row-level `w:tblCellSpacing` "shall supersede" the table-level
/// value. This engine ignores it and warns, so the last table renders exactly
/// like the plain table-level-400 one.
///
/// A characterization test for a known gap, not a claim about Word: when
/// row-level overrides are implemented this must fail, and the fixture's last
/// table is what will then measure them.
#[test]
fn a_row_level_spacing_is_ignored_and_renders_like_the_table_level_one() {
    let t = tables();
    assert!(
        (t[3].first_cell_text_x - t[2].first_cell_text_x).abs() < 0.01,
        "row-level 800 is skipped, so this table matches the table-level 400 one: \
         {} vs {}",
        t[3].first_cell_text_x,
        t[2].first_cell_text_x
    );
}
