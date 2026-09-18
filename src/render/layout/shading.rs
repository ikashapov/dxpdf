//! §17.18.78 geometric shading patterns as draw commands (issue #149).
//!
//! One `ResolvedShading` reaches the page the same way at every level it can
//! appear — cell (§17.4.33, `table::emit`), paragraph (§17.3.1.31,
//! `paragraph::borders`) and run (§17.3.2.32, `paragraph::line_emit`): a
//! patterned box is its fill rect (when it has one) plus stripe **lines** in
//! the pattern colour, clipped to the box here rather than by a clip state —
//! every stripe reaches the page as an ordinary [`DrawCommand::Line`], so the
//! painter needs no shader machinery and a test can assert the geometry
//! command-by-command — the same reasoning that keeps borders as lines. This
//! module owns only the geometry; each caller supplies its own box and
//! decides where the two halves land in its own command stream.
//!
//! # Tile geometry — Word 97's 8×8 tiles, in points
//!
//! Word's patterns are its legacy 8×8 1-bit tiles, drawn at screen
//! resolution; the 5th-edition spec's own per-value swatches (Word-rendered
//! bitmaps, corroborated tile-for-tile by the legacy MS Office pattern set
//! preserved in GNOME goffice's `go-pattern.c`) measure out to a period of
//! **4 px in every family**, a thick ("dark") stripe covering 2 px of it
//! and a thin ("light") one 1 px. At 96 dpi one pixel is 0.75 pt, so the
//! constants below: 3 pt pitch, 1.5 / 0.75 pt stripes. A diagonal stripe's
//! 2 px are measured *along a pixel row*, so its stroked (perpendicular)
//! width is that over √2, and its 4-px period likewise lies along the edge
//! — which is what makes the diagonals visually denser than the
//! orthogonals, exactly as the tiles have it.
//!
//! The crosses are emitted as the union of their two constituent stripe
//! families. For three of the four the spec's tiles *decompose* into
//! exactly that union (phase aside, which a repeating fill cannot show);
//! the dark `horzCross` swatch is the recorded anomaly — see
//! [`PatternFamily::HorzCross`].
//!
//! Stripes are emitted only where they fit whole: a stripe whose width
//! would cross the box edge is dropped rather than half-painted, since a
//! `Line` is stroked symmetrically about its spine and cannot be clipped
//! lengthwise. At most one stripe is lost at each edge, well under a tile.

use crate::render::dimension::Pt;
use crate::render::geometry::{PtLineSegment, PtOffset, PtRect};
use crate::render::layout::draw_command::DrawCommand;
use crate::render::resolve::shading::{PatternFamily, PatternGeometry, ResolvedShading};

/// One Word pattern period: 4 px at 96 dpi.
const TILE: f32 = 3.0;
/// A thick ("dark") stripe is 2 of the period's 4 px.
const THICK: f32 = 1.5;
/// A thin ("light") stripe is 1 px.
const THIN: f32 = 0.75;
/// A diagonal stripe's width is measured along a pixel row, so its stroked
/// width is the orthogonal one over √2.
const SQRT2: f32 = std::f32::consts::SQRT_2;
/// Hard cap on stripes emitted per pattern axis, independent of `rect`'s
/// magnitude. `horizontal`/`vertical`/`diagonal` step a [`TILE`]-sized f32
/// accumulator with no other bound: past roughly 2^25 pt the accumulator's
/// own ULP exceeds the step, so `+= TILE` silently stalls and the loop never
/// reaches its exit test; a `rect` dimension of `+Infinity` never satisfies
/// that test in the first place. Both turn a single crafted
/// `w:trHeight`/`w:tblW` into an unbounded loop. A 300,000 pt cell — over
/// three thousand pages tall — still fits under this cap, so no real
/// document's stripes are truncated by it.
const MAX_STRIPES: u32 = 100_000;

/// The background half of a shaded box: a flat colour's whole rect, or a
/// pattern's background rect (absent for an `auto` fill, which shades like
/// `clear` — stripes over nothing).
///
/// Split from the stripes deliberately: [`coalesce_abutting_rects`]
/// (`draw_command.rs`) only fuses *consecutive* same-colour `Rect`s, which is
/// what keeps a run of same-coloured boxes — adjacent table cells
/// (§17.4.33), adjacent runs sharing one paragraph line (§17.3.2.32) — from
/// leaving a CoreGraphics-visible seam at each shared edge
/// (`tests/table_shading_seams.rs`). A patterned box's stripe `Line`s sit
/// between its own background and the next box's, which would block that
/// fusion for every same-coloured neighbour — so every caller emits a whole
/// run's backgrounds in one pass before any box's stripes, keeping
/// same-colour neighbours adjacent in the stream regardless of which boxes
/// between them are patterned. `table::emit` does this per row,
/// `paragraph::line_emit` per line.
///
/// [`coalesce_abutting_rects`]: crate::render::layout::draw_command::coalesce_abutting_rects
pub(super) fn emit_shading_background(
    commands: &mut Vec<DrawCommand>,
    rect: PtRect,
    shading: &ResolvedShading,
) {
    match *shading {
        ResolvedShading::Flat(color) => commands.push(DrawCommand::Rect { rect, color }),
        ResolvedShading::Pattern {
            background: Some(color),
            ..
        } => commands.push(DrawCommand::Rect { rect, color }),
        ResolvedShading::Pattern {
            background: None, ..
        } => {}
    }
}

/// The stripe half of a shaded box's shading — a flat colour has none. See
/// [`emit_shading_background`] for why the two are emitted in separate
/// passes over a run of boxes rather than together per box.
pub(super) fn emit_shading_stripes(
    commands: &mut Vec<DrawCommand>,
    rect: PtRect,
    shading: &ResolvedShading,
) {
    if let ResolvedShading::Pattern {
        geometry,
        foreground,
        ..
    } = *shading
    {
        for (line, width) in stripe_lines(rect, geometry) {
            commands.push(DrawCommand::Line {
                line,
                color: foreground,
                width,
            });
        }
    }
}

/// The stripes of one pattern over one box, as `(segment, stroke width)` —
/// every segment lies inside `rect`, pre-clipped.
pub(super) fn stripe_lines(rect: PtRect, geometry: PatternGeometry) -> Vec<(PtLineSegment, Pt)> {
    let width = if geometry.thin { THIN } else { THICK };
    let mut out = Vec::new();
    match geometry.family {
        PatternFamily::Horz => horizontal(rect, width, &mut out),
        PatternFamily::Vert => vertical(rect, width, &mut out),
        PatternFamily::Diag => diagonal(rect, width / SQRT2, false, &mut out),
        PatternFamily::ReverseDiag => diagonal(rect, width / SQRT2, true, &mut out),
        PatternFamily::HorzCross => {
            horizontal(rect, width, &mut out);
            vertical(rect, width, &mut out);
        }
        PatternFamily::DiagCross => {
            diagonal(rect, width / SQRT2, false, &mut out);
            diagonal(rect, width / SQRT2, true, &mut out);
        }
    }
    out
}

fn push(out: &mut Vec<(PtLineSegment, Pt)>, x0: f32, y0: f32, x1: f32, y1: f32, width: f32) {
    out.push((
        PtLineSegment {
            start: PtOffset {
                x: Pt::new(x0),
                y: Pt::new(y0),
            },
            end: PtOffset {
                x: Pt::new(x1),
                y: Pt::new(y1),
            },
        },
        Pt::new(width),
    ));
}

/// Horizontal stripes: spines every [`TILE`], on a lattice anchored at
/// `y = 0` in `rect`'s own coordinate space rather than at this box's own
/// top — `y = width/2 + k·TILE` for integer `k`, so two boxes sharing that
/// space still land their spines on the same grid instead of each
/// restarting flush with its own top edge (PR #180 review finding #4: two
/// adjacent same-pattern cells otherwise tile independently and visibly
/// mismatch at the shared edge — confirmed against a Word render, see
/// `test-files/shading-phase-probe.docx`).
///
/// "`rect`'s own coordinate space" is `layout::table::emit`'s table-local
/// one, not the page's: cells in one row share it, which is what the fix
/// above needs, but a page slice's own first row does not share it with the
/// row above the page break — `table::emit::SliceCursor` starts every page
/// slice fresh at table-local `y = 0`, split continuations included. The
/// net effect, confirmed against the same Word render for finding #4's
/// other half (`test-files/shading-phase-probe.docx`'s Table C, a row split
/// across a page by a `cantSplit`-free overlong cell): a split row's
/// continuation *restarts* its pattern flush with its own top rather than
/// continuing the phase the first page's fragment was at, which is what
/// that Word render showed. Pinned at `tests/table_shading_page_split.rs`.
fn horizontal(rect: PtRect, width: f32, out: &mut Vec<(PtLineSegment, Pt)>) {
    let (left, top) = (rect.origin.x.raw(), rect.origin.y.raw());
    let (w, h) = (rect.size.width.raw(), rect.size.height.raw());
    let bottom = top + h;
    let half = width / 2.0;
    // Smallest lattice spine at or after `top`.
    let mut y = half + TILE * ((top - half) / TILE).ceil();
    let mut n = 0;
    while y + half <= bottom && n < MAX_STRIPES {
        push(out, left, y, left + w, y, width);
        y += TILE;
        n += 1;
    }
}

/// Vertical stripes: [`horizontal`] with the axes swapped, lattice anchored
/// at `x = 0` in the same coordinate space — see [`horizontal`]'s doc for
/// what that space is and what it means at a page break.
fn vertical(rect: PtRect, width: f32, out: &mut Vec<(PtLineSegment, Pt)>) {
    let (left, top) = (rect.origin.x.raw(), rect.origin.y.raw());
    let (w, h) = (rect.size.width.raw(), rect.size.height.raw());
    let right = left + w;
    let half = width / 2.0;
    let mut x = half + TILE * ((left - half) / TILE).ceil();
    let mut n = 0;
    while x + half <= right && n < MAX_STRIPES {
        push(out, x, top, x, top + h, width);
        x += TILE;
        n += 1;
    }
}

/// 45° stripes, one spine per [`TILE`] measured along the top edge.
/// `falling` selects `reverseDiagStripe`'s `\` (y grows with x); otherwise
/// the stripes rise to the right — `diagStripe`'s `/`, emitted as runs from
/// the upper right toward the lower left. See [`PatternFamily`] for how
/// the two names map onto the slopes.
///
/// The spine walks the family of lines `y_local = ±x_local + c` (c stepping
/// one tile), same as [`horizontal`]/[`vertical`]: which `c` values are
/// visited is anchored to a lattice fixed in `rect`'s own coordinate space
/// (`c ≡ phase (mod TILE)`, `phase = top - left` for falling, `top + left`
/// for rising — see the test module for the derivation) rather than
/// restarting fresh at this box's own top-left, so two boxes sharing that
/// space land the bulk of their spines on one shared grid — see
/// [`horizontal`]'s doc for what that space is and what it means at a page
/// break.
///
/// "The bulk of" rather than "every": the `.max(inset)` clamps below keep a
/// spine's drawn segment inside the box by sliding its start point along
/// whichever edge it would otherwise cross, and exactly at the one point
/// where a box's own diagonal crosses its corner (`c` near zero) that slide
/// can drift the drawn segment by up to one `width` short of where the
/// lattice would otherwise place it. That is a limit of this per-box clamp
/// algorithm, not something the lattice anchoring can close — it is also far
/// smaller than the multi-point misalignment this anchoring fixes, and
/// occurs at most once per box.
fn diagonal(rect: PtRect, width: f32, falling: bool, out: &mut Vec<(PtLineSegment, Pt)>) {
    let (left, top) = (rect.origin.x.raw(), rect.origin.y.raw());
    let (w, h) = (rect.size.width.raw(), rect.size.height.raw());
    // Inset so a stripe's stroked width stays inside the box (a 45° stroke
    // reaches width/(2√2) into each axis; width/2 is the safe bound).
    let inset = width / 2.0;
    let phase = if falling { top - left } else { top + left };
    // Smallest lattice-aligned c (c ≡ phase mod TILE) at or above the same
    // lower bound the box-local form used.
    let c_floor = -(h - inset);
    let mut c = phase + TILE * ((c_floor - phase) / TILE).ceil();
    let mut n = 0;
    while c < w - inset && n < MAX_STRIPES {
        let x0 = c.max(inset);
        let y0 = (-c).max(inset);
        let run = (w - inset - x0).min(h - inset - y0);
        if run > 0.0 {
            if falling {
                push(
                    out,
                    left + x0,
                    top + y0,
                    left + x0 + run,
                    top + y0 + run,
                    width,
                );
            } else {
                push(
                    out,
                    left + w - x0,
                    top + y0,
                    left + w - x0 - run,
                    top + y0 + run,
                    width,
                );
            }
        }
        c += TILE;
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::geometry::PtSize;
    use crate::render::resolve::color::RgbColor;

    fn geometry(family: PatternFamily, thin: bool) -> PatternGeometry {
        PatternGeometry { family, thin }
    }

    fn rect(w: f32, h: f32) -> PtRect {
        PtRect {
            origin: PtOffset {
                x: Pt::new(100.0),
                y: Pt::new(50.0),
            },
            size: PtSize {
                width: Pt::new(w),
                height: Pt::new(h),
            },
        }
    }

    /// Every stripe of every family stays inside the box it shades,
    /// **including its stroked width** — the pre-clipping this module
    /// exists for, over a box that cuts both diagonal runs short. A stroke
    /// extends half its width to each side of the spine, perpendicular to
    /// the run.
    #[test]
    fn every_stripe_lies_inside_the_rect() {
        use PatternFamily::*;
        let r = rect(40.0, 13.0);
        for family in [Horz, Vert, Diag, ReverseDiag, HorzCross, DiagCross] {
            for thin in [false, true] {
                for (line, width) in stripe_lines(r, geometry(family, thin)) {
                    // A diagonal stroke reaches width/(2√2) into each axis;
                    // an orthogonal one width/2 along its own. Half in both
                    // axes is the safe bound for every family at once.
                    let half = width.raw() / 2.0;
                    let horizontal = line.start.y == line.end.y;
                    let vertical = line.start.x == line.end.x;
                    let (hx, hy) = match (horizontal, vertical) {
                        (true, false) => (0.0, half),
                        (false, true) => (half, 0.0),
                        _ => (half, half),
                    };
                    for p in [line.start, line.end] {
                        assert!(
                            p.x.raw() - hx >= 100.0 - 1e-4 && p.x.raw() + hx <= 140.0 + 1e-4,
                            "{family:?} thin={thin}: x of {p:?} (width {width:?}) leaves the box"
                        );
                        assert!(
                            p.y.raw() - hy >= 50.0 - 1e-4 && p.y.raw() + hy <= 63.0 + 1e-4,
                            "{family:?} thin={thin}: y of {p:?} (width {width:?}) leaves the box"
                        );
                    }
                }
            }
        }
    }

    /// Horizontal spines sit one [`TILE`] apart, on a lattice anchored at
    /// global `y = 0` rather than flush with this box's own top — for a box
    /// at `y = 50`, thick (1.5 pt) spines land at the smallest
    /// `0.75 + k·3 >= 50`, i.e. 51.75, then step by 3 while the stripe still
    /// fits within the box's 14 pt height — four of them — and the 0.75 pt
    /// thin stripe's smaller half-width lets a fifth fit, which is the
    /// thick/thin distinction doing observable work beyond the widths
    /// themselves.
    #[test]
    fn horizontal_stripes_step_one_tile() {
        let thick = stripe_lines(rect(40.0, 14.0), geometry(PatternFamily::Horz, false));
        let ys: Vec<f32> = thick.iter().map(|(l, _)| l.start.y.raw()).collect();
        assert_eq!(
            ys,
            [51.75, 54.75, 57.75, 60.75],
            "smallest lattice y >= top, then + n·TILE"
        );
        assert!(thick.iter().all(|(l, _)| l.start.y == l.end.y));
        assert!(
            thick
                .iter()
                .all(|(l, _)| (l.end.x - l.start.x).raw() == 40.0),
            "full-width runs"
        );
        assert!(thick.iter().all(|(_, w)| w.raw() == 1.5), "2 px stripes");

        let thin = stripe_lines(rect(40.0, 14.0), geometry(PatternFamily::Horz, true));
        assert_eq!(thin.len(), 5, "a thinner stripe fits once more");
        assert!(thin.iter().all(|(_, w)| w.raw() == 0.75), "1 px stripes");
    }

    /// The property [`horizontal_stripes_step_one_tile`] pins with one box:
    /// two boxes offset from each other by a **non-multiple** of [`TILE`]
    /// still land their spines on one shared lattice, rather than each
    /// restarting flush with its own top edge — PR #180 review finding #4,
    /// confirmed against a Word render (`test-files/shading-phase-probe.docx`:
    /// four identically-shaded cells in one row show one continuous grid,
    /// not four independently-phased patches).
    #[test]
    fn two_boxes_offset_by_a_non_multiple_of_tile_share_one_lattice() {
        let a = rect(40.0, 40.0);
        // Offset by 5pt — not a multiple of TILE (3pt) — the way two real
        // adjacent cells' heights or a page's own margins generically are.
        let b = PtRect {
            origin: PtOffset {
                x: a.origin.x,
                y: a.origin.y + Pt::new(5.0),
            },
            size: a.size,
        };
        let geo = geometry(PatternFamily::Horz, false);
        let ys_a: Vec<f32> = stripe_lines(a, geo)
            .iter()
            .map(|(l, _)| l.start.y.raw())
            .collect();
        let ys_b: Vec<f32> = stripe_lines(b, geo)
            .iter()
            .map(|(l, _)| l.start.y.raw())
            .collect();
        // Every spine of `b` is a spine `a`'s own (unbounded) lattice would
        // also produce — i.e. congruent to `a`'s spines modulo TILE — which
        // is what "one shared grid" means and what box-local phase (always
        // restarting at `width/2` from its own top) would not satisfy.
        for y in ys_b {
            let nearest_multiple = ((y - ys_a[0]) / TILE).round() * TILE + ys_a[0];
            assert!(
                (y - nearest_multiple).abs() < 1e-3,
                "box b's spine at {y} is not on box a's lattice {ys_a:?}"
            );
        }
    }

    /// The literal shape of the confirmed case, `vertStripe`: two side-by-side
    /// table cells, `b` starting exactly where `a` ends, at a width that is
    /// not a multiple of [`TILE`] — the way real column widths generically
    /// aren't. Real Word render: `test-files/shading-phase-probe.docx`'s
    /// Table A, four identically-shaded cells in one row showing one
    /// continuous vertical grid, not four independently-phased patches.
    #[test]
    fn adjacent_cells_share_one_vertical_lattice() {
        let a = rect(37.0, 20.0);
        let b = PtRect {
            origin: PtOffset {
                x: a.origin.x + a.size.width,
                y: a.origin.y,
            },
            size: a.size,
        };
        let geo = geometry(PatternFamily::Vert, false);
        let xs_a: Vec<f32> = stripe_lines(a, geo)
            .iter()
            .map(|(l, _)| l.start.x.raw())
            .collect();
        let xs_b: Vec<f32> = stripe_lines(b, geo)
            .iter()
            .map(|(l, _)| l.start.x.raw())
            .collect();
        assert!(!xs_a.is_empty() && !xs_b.is_empty());
        for x in xs_b {
            let nearest_multiple = ((x - xs_a[0]) / TILE).round() * TILE + xs_a[0];
            assert!(
                (x - nearest_multiple).abs() < 1e-3,
                "cell b's spine at {x} is not on cell a's lattice {xs_a:?}"
            );
        }
    }

    /// The diagonal-family analogue, both directions — Table B of the same
    /// probe (`diagCross`). A falling (`\`) spine's global invariant is
    /// `y - x`, a rising (`/`) spine's is `y + x`; held constant between the
    /// two cells' same-index spines is what "one continuous lattice" means
    /// for a 45° family.
    ///
    /// Compared index-for-index rather than against a single extrapolated
    /// reference, and tolerating **one** mismatch: [`diagonal`]'s own doc
    /// explains why its corner clamp can drift a single spine, at the one
    /// point where a box's own diagonal crosses its corner, by up to one
    /// stroke `width` — both cells hit that at the same index here, since
    /// they share a height, and it is the only pair this test allows to
    /// disagree.
    #[test]
    fn adjacent_cells_share_one_diagonal_lattice() {
        let a = rect(37.0, 20.0);
        let b = PtRect {
            origin: PtOffset {
                x: a.origin.x + a.size.width,
                y: a.origin.y,
            },
            size: a.size,
        };
        for family in [PatternFamily::Diag, PatternFamily::ReverseDiag] {
            let geo = geometry(family, false);
            let invariant = |l: &PtLineSegment| match family {
                PatternFamily::Diag => l.start.y.raw() + l.start.x.raw(),
                PatternFamily::ReverseDiag => l.start.y.raw() - l.start.x.raw(),
                _ => unreachable!(),
            };
            let a_vals: Vec<f32> = stripe_lines(a, geo)
                .iter()
                .map(|(l, _)| invariant(l))
                .collect();
            let b_vals: Vec<f32> = stripe_lines(b, geo)
                .iter()
                .map(|(l, _)| invariant(l))
                .collect();
            assert!(
                !a_vals.is_empty() && !b_vals.is_empty(),
                "{family:?}: expected spines in both cells"
            );
            let mismatches: Vec<(usize, f32, f32)> = a_vals
                .iter()
                .zip(b_vals.iter())
                .enumerate()
                .filter_map(|(i, (&av, &bv))| {
                    let diff_mod_tile = (av - bv).rem_euclid(TILE);
                    let on_lattice = diff_mod_tile < 1e-3 || TILE - diff_mod_tile < 1e-3;
                    (!on_lattice).then_some((i, av, bv))
                })
                .collect();
            assert!(
                mismatches.len() <= 1,
                "{family:?}: more than the one corner-clamp mismatch this test \
                 tolerates: {mismatches:?}"
            );
        }
    }

    /// The two diagonal families mirror each other — comparable spine count,
    /// 45° both, opposite slopes: `diagStripe` rises to the right (emitted
    /// upper-right → lower-left, so Δx < 0 with Δy > 0),
    /// `reverseDiagStripe` falls (Δx and Δy both positive). Their stroke is
    /// the orthogonal width over √2, the tile's 2 px measured along a row.
    ///
    /// Not an exact count match: each family is now phase-locked to its own
    /// global lattice (`c + top - left` for falling, `c + top + left` for
    /// rising — see [`diagonal`]'s doc), so at a box whose position makes the
    /// two lattices land differently relative to the box edges, one family
    /// can fit one more spine than the other. `rect`'s box (100, 50) is
    /// exactly such a case.
    #[test]
    fn diagonals_mirror() {
        let diag = stripe_lines(rect(40.0, 13.0), geometry(PatternFamily::Diag, false));
        let reverse = stripe_lines(
            rect(40.0, 13.0),
            geometry(PatternFamily::ReverseDiag, false),
        );
        assert!(
            diag.len().abs_diff(reverse.len()) <= 1,
            "independently phase-locked families may differ by at most one \
             boundary spine: diag={} reverse={}",
            diag.len(),
            reverse.len()
        );
        assert!(!diag.is_empty());
        for (l, w) in &diag {
            let (dx, dy) = ((l.end.x - l.start.x).raw(), (l.end.y - l.start.y).raw());
            assert!(dx < 0.0 && dy > 0.0, "diagStripe rises to the right: {l:?}");
            assert!((dx + dy).abs() < 1e-4, "…at 45°");
            assert!((w.raw() - 1.5 / SQRT2).abs() < 1e-5);
        }
        for (l, _) in &reverse {
            let (dx, dy) = ((l.end.x - l.start.x).raw(), (l.end.y - l.start.y).raw());
            assert!(
                dx > 0.0 && dy > 0.0,
                "reverseDiagStripe falls to the right: {l:?}"
            );
            assert!((dx - dy).abs() < 1e-4, "…at 45°");
        }
    }

    /// The crosses are exactly their two constituent families' unions —
    /// the reading the spec's own tiles prove for three of the four and
    /// [`PatternFamily::HorzCross`] records for the fourth.
    #[test]
    fn crosses_are_the_union_of_their_families() {
        let r = rect(40.0, 13.0);
        let cross = stripe_lines(r, geometry(PatternFamily::HorzCross, false));
        let horz = stripe_lines(r, geometry(PatternFamily::Horz, false));
        let vert = stripe_lines(r, geometry(PatternFamily::Vert, false));
        assert_eq!(cross.len(), horz.len() + vert.len());

        // `diag`/`reverse` rather than `2 * diag.len()`: the two diagonal
        // families are independently phase-locked ([`diagonals_mirror`]) and
        // need not have equal counts, but `DiagCross` is still exactly their
        // union — it calls the same two `diagonal()` invocations these do.
        let dcross = stripe_lines(r, geometry(PatternFamily::DiagCross, false));
        let diag = stripe_lines(r, geometry(PatternFamily::Diag, false));
        let reverse = stripe_lines(r, geometry(PatternFamily::ReverseDiag, false));
        assert_eq!(dcross.len(), diag.len() + reverse.len());
    }

    /// Calling `emit_shading_background` then `emit_shading_stripes` — what
    /// every caller does for one box across its two run-wide passes — paints
    /// the background rect first and only stripes after, and no rect at all
    /// over an `auto` fill, which shades like `clear`: stripes over nothing.
    #[test]
    fn background_then_stripes_matches_caller_per_box_order() {
        let fg = RgbColor { r: 1, g: 2, b: 3 };
        let bg = RgbColor {
            r: 250,
            g: 250,
            b: 250,
        };
        let shading = ResolvedShading::Pattern {
            geometry: geometry(PatternFamily::Horz, false),
            foreground: fg,
            background: Some(bg),
        };
        let mut commands = Vec::new();
        emit_shading_background(&mut commands, rect(40.0, 13.0), &shading);
        emit_shading_stripes(&mut commands, rect(40.0, 13.0), &shading);
        assert!(
            matches!(&commands[0], DrawCommand::Rect { color, .. } if *color == bg),
            "background rect first"
        );
        assert!(commands.len() > 1, "then stripes");
        assert!(commands[1..]
            .iter()
            .all(|c| matches!(c, DrawCommand::Line { color, .. } if *color == fg)));

        let transparent_shading = ResolvedShading::Pattern {
            geometry: geometry(PatternFamily::Horz, false),
            foreground: fg,
            background: None,
        };
        let mut transparent = Vec::new();
        emit_shading_background(&mut transparent, rect(40.0, 13.0), &transparent_shading);
        emit_shading_stripes(&mut transparent, rect(40.0, 13.0), &transparent_shading);
        assert!(
            transparent
                .iter()
                .all(|c| matches!(c, DrawCommand::Line { .. })),
            "an auto fill draws stripes over nothing"
        );
    }

    /// A cell dimension pathological enough to defeat f32 `TILE`
    /// accumulation — either far past the ~2^25 pt point where its ULP
    /// exceeds the step, or literal `+Infinity`, which never satisfies the
    /// loop's own exit test at all — must still return in bounded time with
    /// a bounded number of stripes, not hang the render thread. This is the
    /// DoS `MAX_STRIPES` exists to close; each family and axis is checked
    /// since `horizontal`/`vertical`/`diagonal` each accumulate separately.
    #[test]
    fn stripe_count_is_capped_for_pathological_rect_dimensions() {
        use PatternFamily::*;
        let with_height = |h: f32| PtRect {
            origin: PtOffset {
                x: Pt::new(0.0),
                y: Pt::new(0.0),
            },
            size: PtSize {
                width: Pt::new(40.0),
                height: Pt::new(h),
            },
        };
        let with_width = |w: f32| PtRect {
            origin: PtOffset {
                x: Pt::new(0.0),
                y: Pt::new(0.0),
            },
            size: PtSize {
                width: Pt::new(w),
                height: Pt::new(40.0),
            },
        };
        // The crosses union two independently capped loops, so their own
        // bound is twice a single axis's.
        let limit = |family: PatternFamily| match family {
            HorzCross | DiagCross => 2 * MAX_STRIPES,
            _ => MAX_STRIPES,
        };
        for h in [1e9_f32, f32::INFINITY] {
            let r = with_height(h);
            for family in [Horz, Vert, Diag, ReverseDiag, HorzCross, DiagCross] {
                let n = stripe_lines(r, geometry(family, false)).len() as u32;
                assert!(n <= limit(family), "{family:?} h={h}: {n} stripes");
            }
        }
        for w in [1e9_f32, f32::INFINITY] {
            let r = with_width(w);
            for family in [Horz, Vert, Diag, ReverseDiag, HorzCross, DiagCross] {
                let n = stripe_lines(r, geometry(family, false)).len() as u32;
                assert!(n <= limit(family), "{family:?} w={w}: {n} stripes");
            }
        }
    }
}
