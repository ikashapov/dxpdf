//! §17.18.78 `ST_Shd` cell shading patterns end to end (issue #149): the
//! percentage tints and `solid` blend into one flat colour, and the geometric
//! stripe/cross families draw their fill *plus* stripe lines in the pattern
//! colour — where before this every cell painted its `w:fill` alone.
//!
//! The fixture (`scripts/make_issue149_fixture.py`) gives every cell unique
//! colours, so each assertion identifies its cell's output by colour with no
//! coordinates pinned. Stripe geometry (pitch, width) is pinned by the unit
//! tests beside the generator; here only the *shape* of each family is
//! asserted — orientation, both crosses' two directions, thin thinner than
//! thick, and the two diagonals mirroring each other.

use std::path::Path;

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};

const TEST_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test-files");

fn pages() -> Vec<LayoutedPage> {
    let path = Path::new(TEST_DIR).join("shading-patterns.docx");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let doc = dxpdf::docx::parse(&bytes)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
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

/// Every line of the given colour, as `(dx, dy, width)`.
fn lines_of(pages: &[LayoutedPage], rgb: (u8, u8, u8)) -> Vec<(f32, f32, f32)> {
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            DrawCommand::Line { line, color, width } if (color.r, color.g, color.b) == rgb => {
                Some((
                    (line.end.x - line.start.x).raw(),
                    (line.end.y - line.start.y).raw(),
                    width.raw(),
                ))
            }
            _ => None,
        })
        .collect()
}

#[track_caller]
fn assert_rect(colors: &[(u8, u8, u8)], rgb: (u8, u8, u8), what: &str) {
    assert!(
        colors.contains(&rgb),
        "{what}: expected a rect of {rgb:?}; rect colors drawn: {colors:?}"
    );
}

/// The flat family: each value resolves to exactly one colour.
///
/// - `pct25` of blue over white: 25% of each channel toward the pattern
///   colour — (191, 191, 255).
/// - `pct50` with both colours `auto`: black into white, mid grey.
/// - `solid` paints its **pattern** colour whole; its fill must appear
///   nowhere, and is one of the two colours the whole fixture forbids.
/// - `clear` keeps today's behaviour: the fill alone.
/// - `nil` is no shading whatsoever — its colour is the other forbidden one.
#[test]
fn tints_blend_solid_takes_the_pattern_color_and_nil_paints_nothing() {
    let pages = pages();
    let colors = rect_colors(&pages);

    assert_rect(&colors, (191, 191, 255), "pct25 blue over white");
    assert_rect(&colors, (127, 127, 127), "pct50 auto over auto");
    assert_rect(&colors, (204, 0, 0), "solid paints w:color");
    assert_rect(&colors, (255, 204, 0), "clear paints w:fill");
    assert!(
        !colors.contains(&(0, 204, 0)),
        "solid must not paint its w:fill: {colors:?}"
    );
    assert!(
        !colors.contains(&(170, 0, 0)),
        "nil must paint nothing: {colors:?}"
    );
}

/// Each geometric cell paints its fill behind its stripes, so the pattern
/// stays legible over the page — the fill colours are the proof the rect
/// half of the pair was emitted.
#[test]
fn geometric_patterns_paint_their_fill_behind_the_stripes() {
    let pages = pages();
    let colors = rect_colors(&pages);
    for (fill, what) in [
        ((0xDD, 0xFF, 0xDD), "horzStripe"),
        ((0xEE, 0xFF, 0xDD), "vertStripe"),
        ((0xEE, 0xFF, 0xEE), "diagStripe"),
        ((0xFF, 0xEE, 0xDD), "reverseDiagStripe"),
        ((0xFF, 0xEE, 0xEE), "horzCross"),
        ((0xFF, 0xDD, 0xEE), "diagCross"),
    ] {
        assert_rect(&colors, fill, what);
    }
}

#[test]
fn horz_stripes_are_horizontal_and_thin_is_thinner() {
    let pages = pages();
    let thick = lines_of(&pages, (0x22, 0x00, 0x22));
    assert!(thick.len() >= 2, "horzStripe draws stripes: {thick:?}");
    assert!(
        thick.iter().all(|&(_, dy, _)| dy == 0.0),
        "…all horizontal: {thick:?}"
    );

    let thin = lines_of(&pages, (0x33, 0x00, 0x33));
    assert!(thin.len() >= 2, "thinHorzStripe draws stripes: {thin:?}");
    assert!(
        thin.iter().all(|&(_, dy, _)| dy == 0.0),
        "…all horizontal: {thin:?}"
    );
    let thick_w = thick.iter().map(|&(_, _, w)| w).fold(f32::MAX, f32::min);
    let thin_w = thin.iter().map(|&(_, _, w)| w).fold(0.0, f32::max);
    assert!(
        thin_w < thick_w,
        "a thin stripe is thinner than a thick one: thin {thin_w} vs thick {thick_w}"
    );
}

#[test]
fn vert_stripes_are_vertical() {
    let pages = pages();
    let lines = lines_of(&pages, (0x44, 0x00, 0x44));
    assert!(lines.len() >= 2, "vertStripe draws stripes: {lines:?}");
    assert!(
        lines.iter().all(|&(dx, _, _)| dx == 0.0),
        "…all vertical: {lines:?}"
    );
}

/// The two diagonal families slope, and mirror each other.
///
/// Which specific slope `diagStripe` gets was, for a while, settled only by
/// [MS-DOC]'s Ipat naming and the spec's own swatches — no independent
/// second source, unlike this module's other choices (see
/// `PatternFamily`'s doc). **Confirmed against a real Word render** (PR
/// #180 review finding #6): `diagStripe` renders `/` (rising to the
/// right), `reverseDiagStripe` renders `\` (falling to the right) — a line
/// segment's `(dx, dy)` in this coordinate system (`y` grows downward) has
/// opposite-signed components for `/` and same-signed for `\`, so `/` is
/// `dx * dy < 0`.
#[test]
fn diagonal_stripes_slope_and_mirror_each_other() {
    let pages = pages();
    let diag = lines_of(&pages, (0x55, 0x00, 0x55));
    let reverse = lines_of(&pages, (0x66, 0x00, 0x66));
    assert!(!diag.is_empty() && !reverse.is_empty());
    assert!(
        diag.iter().all(|&(dx, dy, _)| dx != 0.0 && dy != 0.0),
        "diagStripe slopes: {diag:?}"
    );
    assert!(
        reverse.iter().all(|&(dx, dy, _)| dx != 0.0 && dy != 0.0),
        "reverseDiagStripe slopes: {reverse:?}"
    );
    let sign = |v: f32| v.signum();
    assert!(
        diag.iter().all(|&(dx, dy, _)| sign(dx * dy) < 0.0),
        "diagStripe should slope '/' (dx * dy < 0): {diag:?}"
    );
    assert!(
        reverse.iter().all(|&(dx, dy, _)| sign(dx * dy) > 0.0),
        "reverseDiagStripe should slope '\\' (dx * dy > 0): {reverse:?}"
    );
}

/// `horzCross` draws both horizontals and verticals; `diagCross` both
/// diagonal directions — each in its own colour.
///
/// `horzCross`'s axis-aligned (not diagonal) reading was, for a while, a
/// judgment call against conflicting spec evidence — see
/// `PatternFamily::HorzCross`'s doc for the full story. **Confirmed against
/// a real Word render** (PR #180 review finding #5): this cell renders as
/// straight crossing lines, not a tilted diamond lattice, so this
/// assertion is no longer just pinning an implementation choice.
#[test]
fn crosses_draw_both_directions() {
    let pages = pages();
    let horz_cross = lines_of(&pages, (0x77, 0x00, 0x77));
    assert!(
        horz_cross.iter().any(|&(_, dy, _)| dy == 0.0)
            && horz_cross.iter().any(|&(dx, _, _)| dx == 0.0),
        "horzCross draws horizontals and verticals: {horz_cross:?}"
    );
    let diag_cross = lines_of(&pages, (0x88, 0x00, 0x88));
    assert!(
        diag_cross.iter().any(|&(dx, dy, _)| dx * dy > 0.0)
            && diag_cross.iter().any(|&(dx, dy, _)| dx * dy < 0.0),
        "diagCross draws both diagonal directions: {diag_cross:?}"
    );
}

/// The 5 `thin*` variants no earlier version of this fixture carried (PR
/// #180 review, minor finding): `thinVertStripe`, `thinDiagStripe`,
/// `thinReverseDiagStripe`, `thinHorzCross`, `thinDiagCross` used to be
/// exercised only against a synthetic `PatternGeometry` in
/// `layout::shading`'s own unit tests, never through the full `w:shd` XML →
/// cascade → `resolve_shading` → `emit_cell_shading` path a real document
/// takes. Each is shaped like its already-pinned thick sibling, plus
/// thinner — the same "thin thinner than thick" property
/// `horz_stripes_are_horizontal_and_thin_is_thinner` pins for the one thin
/// variant (`thinHorzStripe`) the fixture already had.
#[test]
fn the_five_previously_untested_thin_variants_render_end_to_end() {
    let pages = pages();
    let colors = rect_colors(&pages);

    assert_rect(&colors, (0xFF, 0xEE, 0xFF), "thinVertStripe fill");
    let thin_vert = lines_of(&pages, (0x99, 0x00, 0x99));
    assert!(!thin_vert.is_empty(), "thinVertStripe draws stripes");
    assert!(
        thin_vert.iter().all(|&(dx, _, _)| dx == 0.0),
        "…all vertical: {thin_vert:?}"
    );

    assert_rect(&colors, (0xEE, 0xDD, 0xFF), "thinDiagStripe fill");
    let thin_diag = lines_of(&pages, (0xAA, 0x00, 0xAA));
    assert!(!thin_diag.is_empty(), "thinDiagStripe draws stripes");
    assert!(
        thin_diag.iter().all(|&(dx, dy, _)| dx != 0.0 && dy != 0.0),
        "…all diagonal: {thin_diag:?}"
    );

    assert_rect(&colors, (0xDD, 0xEE, 0xFF), "thinReverseDiagStripe fill");
    let thin_reverse = lines_of(&pages, (0xBB, 0x00, 0xBB));
    assert!(
        !thin_reverse.is_empty(),
        "thinReverseDiagStripe draws stripes"
    );
    assert!(
        thin_reverse
            .iter()
            .all(|&(dx, dy, _)| dx != 0.0 && dy != 0.0),
        "…all diagonal: {thin_reverse:?}"
    );

    assert_rect(&colors, (0xEE, 0xFF, 0xFF), "thinHorzCross fill");
    let thin_horz_cross = lines_of(&pages, (0xCC, 0x00, 0xCC));
    assert!(
        thin_horz_cross.iter().any(|&(_, dy, _)| dy == 0.0)
            && thin_horz_cross.iter().any(|&(dx, _, _)| dx == 0.0),
        "thinHorzCross draws horizontals and verticals: {thin_horz_cross:?}"
    );

    assert_rect(&colors, (0xFF, 0xFF, 0xEE), "thinDiagCross fill");
    let thin_diag_cross = lines_of(&pages, (0xDD, 0x00, 0xDD));
    assert!(
        thin_diag_cross.iter().any(|&(dx, dy, _)| dx * dy > 0.0)
            && thin_diag_cross.iter().any(|&(dx, dy, _)| dx * dy < 0.0),
        "thinDiagCross draws both diagonal directions: {thin_diag_cross:?}"
    );

    // Every one thinner than its already-pinned thick sibling.
    let max_width = |rgb| -> f32 {
        lines_of(&pages, rgb)
            .iter()
            .map(|&(_, _, w)| w)
            .fold(0.0_f32, f32::max)
    };
    let min_width = |rgb| -> f32 {
        lines_of(&pages, rgb)
            .iter()
            .map(|&(_, _, w)| w)
            .fold(f32::MAX, f32::min)
    };
    for (thick_rgb, thin_rgb, what) in [
        ((0x44, 0x00, 0x44), (0x99, 0x00, 0x99), "vertStripe"),
        ((0x55, 0x00, 0x55), (0xAA, 0x00, 0xAA), "diagStripe"),
        ((0x66, 0x00, 0x66), (0xBB, 0x00, 0xBB), "reverseDiagStripe"),
        ((0x77, 0x00, 0x77), (0xCC, 0x00, 0xCC), "horzCross"),
        ((0x88, 0x00, 0x88), (0xDD, 0x00, 0xDD), "diagCross"),
    ] {
        let thick_w = min_width(thick_rgb);
        let thin_w = max_width(thin_rgb);
        assert!(
            thin_w < thick_w,
            "{what}: thin should be thinner than thick: thin {thin_w} vs thick {thick_w}"
        );
    }
}
