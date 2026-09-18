//! §17.4.6 row split × §17.18.78 pattern phase — PR #180 review finding #4,
//! part (a): the cross-page half `tests/paragraph_run_shading.rs` and
//! `layout::shading`'s own unit tests don't cover (those pin part (b), two
//! adjacent *cells in one row* sharing one lattice).
//!
//! `test-files/shading-phase-probe.docx`'s Table C is one cell, stuffed with
//! filler paragraphs so its row splits across a page. The reporter's own
//! Word screenshots of the split (see AGENTS.md's fixture table) read as a
//! **restart**: the pattern sits flush with the top of the continuation
//! fragment rather than picking up where the first page's fragment left off.
//!
//! `layout::table::emit::SliceCursor` already produces exactly that: every
//! page slice of a table — the table's genuine first page and every
//! continuation after a split alike — starts its own vertical cursor at
//! table-local `y = 0` (`SliceCursor::new()`), and a row's cell rects are
//! built directly from that local cursor. So a row's *own* first page and a
//! *continuation* fragment after a split are both, from `layout::shading`'s
//! point of view, "the first row of a fresh slice" — indistinguishable from
//! two unrelated boxes that happen to start at the same local origin. These
//! pin that this is what actually happens, so a change that threads a
//! carried-over phase across the page break (making the continuation
//! *continue* instead of *restart*) does not slip in unnoticed.

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};

const TABLE_C_FILL: (u8, u8, u8) = (0xFF, 0xFF, 0x00);
const TABLE_C_STRIPE: (u8, u8, u8) = (0x00, 0x00, 0x00);
/// `layout::shading::THICK` — `horzStripe` with no `w:val` modifier (as
/// Table C declares it) is the thick variant.
const STRIPE_WIDTH: f32 = 1.5;
const EPSILON: f32 = 0.01;

fn table_c_pages() -> Vec<LayoutedPage> {
    let bytes = std::fs::read("test-files/shading-phase-probe.docx")
        .expect("read shading-phase-probe.docx");
    let doc = dxpdf::docx::parse(&bytes).expect("parse shading-phase-probe.docx");
    dxpdf::render::resolve_and_layout(doc).1
}

/// Table C's fill rect on one page, as `(top, bottom)` — `FFFF00` is unique
/// to Table C in this fixture (Tables A/B are white/`FFFFCC`), so this needs
/// no dependency on layout internals to find it.
fn table_c_fill_band(page: &LayoutedPage) -> Option<(f32, f32)> {
    page.commands.iter().find_map(|c| match c {
        DrawCommand::Rect { rect, color, .. } if (color.r, color.g, color.b) == TABLE_C_FILL => {
            Some((
                rect.origin.y.raw(),
                rect.origin.y.raw() + rect.size.height.raw(),
            ))
        }
        _ => None,
    })
}

/// The topmost `horzStripe` line within `[top, bottom]` on `page`.
fn first_stripe_y(page: &LayoutedPage, top: f32, bottom: f32) -> f32 {
    page.commands
        .iter()
        .filter_map(|c| match c {
            DrawCommand::Line { line, color, .. }
                if (color.r, color.g, color.b) == TABLE_C_STRIPE =>
            {
                let y = line.start.y.raw();
                (y >= top - EPSILON && y <= bottom + EPSILON).then_some(y)
            }
            _ => None,
        })
        .fold(f32::INFINITY, f32::min)
}

/// The fixture is built to split Table C's one row across exactly two pages
/// (70 filler paragraphs in an A4 page). If a future layout change makes it
/// fit on one page, or split further, the fixture no longer asks the
/// question it was built for.
#[test]
fn the_fixture_still_splits_table_c_across_exactly_two_pages() {
    let pages = table_c_pages();
    assert_eq!(
        pages.len(),
        2,
        "expected shading-phase-probe.docx to lay out as 2 pages"
    );
    assert!(
        table_c_fill_band(&pages[0]).is_some(),
        "page 1 should hold Table C's first fragment"
    );
    assert!(
        table_c_fill_band(&pages[1]).is_some(),
        "page 2 should hold Table C's continuation fragment"
    );
}

/// The measurement this file exists to pin: both the pre-split fragment and
/// the post-split continuation land their first stripe exactly `width / 2`
/// below *their own* fill rect's top — flush with their own top edge, same
/// as an ordinary unsplit box would. A phase carried over from the first
/// page (true continuation) would instead put the continuation's first
/// stripe at an offset that depends on how far the pattern had progressed
/// before the cut, which is not, in general, `width / 2`.
#[test]
fn a_split_rows_continuation_restarts_its_pattern_flush_with_its_own_top() {
    let pages = table_c_pages();
    assert_eq!(pages.len(), 2);

    for (page, label) in [
        (&pages[0], "page 1 (pre-split)"),
        (&pages[1], "page 2 (continuation)"),
    ] {
        let (top, bottom) = table_c_fill_band(page)
            .unwrap_or_else(|| panic!("{label}: no Table C fill rect found"));
        let first_y = first_stripe_y(page, top, bottom);
        assert!(
            first_y.is_finite(),
            "{label}: no horzStripe line found inside the fill band [{top}, {bottom}]"
        );
        let offset = first_y - top;
        assert!(
            (offset - STRIPE_WIDTH / 2.0).abs() < EPSILON,
            "{label}: expected the first stripe flush with the fragment's own top \
             (offset {} == width/2 == {}), got offset {offset}",
            STRIPE_WIDTH / 2.0,
            STRIPE_WIDTH / 2.0
        );
    }
}
