use crate::render::dimension::Pt;
use crate::render::geometry::PtRect;

use super::types::{
    CellBorderOverride, TableBorderConfig, TableBorderLine, TableBorderStyle, TableCellInput,
};
use crate::render::layout::draw_command::DrawCommand;

/// Resolved borders for one cell edge.
#[derive(Clone)]
pub(super) struct CellBorders {
    pub(super) top: Option<TableBorderLine>,
    pub(super) bottom: Option<TableBorderLine>,
    pub(super) left: Option<TableBorderLine>,
    pub(super) right: Option<TableBorderLine>,
}

/// §17.4.38 / §17.7.6: resolve effective borders for a cell.
/// Per-cell borders (from conditional formatting) override table-level borders.
/// Table-level insideH/insideV are mapped to cell edges based on position.
///
/// `cell_grid_col` is the cell's absolute starting grid column (accounting
/// for the row's `gridBefore`); `cell_grid_span` is its `gridSpan` (≥1);
/// `num_grid_cols` is the table-wide grid column count. Together these
/// determine whether the cell is at the table's left or right edge — which
/// matters because §17.4.17/§17.4.16 (`gridBefore`/`gridAfter`) can leave
/// the row's first/last cell *not* at the table edge.
pub(super) fn resolve_cell_effective_borders(
    cell: &TableCellInput,
    table_borders: Option<&TableBorderConfig>,
    row_idx: usize,
    cell_grid_col: usize,
    cell_grid_span: usize,
    num_rows: usize,
    num_grid_cols: usize,
) -> (
    Option<TableBorderLine>,
    Option<TableBorderLine>,
    Option<TableBorderLine>,
    Option<TableBorderLine>,
) {
    // Start with table-level borders mapped to cell edges.
    let tb = table_borders;
    let is_first_row = row_idx == 0;
    let is_last_row = row_idx == num_rows - 1;
    let is_first_col = cell_grid_col == 0;
    let is_last_col = cell_grid_col + cell_grid_span >= num_grid_cols;

    let mut top = if is_first_row {
        tb.and_then(|b| b.top)
    } else {
        tb.and_then(|b| b.inside_h)
    };
    let mut bottom = if is_last_row {
        tb.and_then(|b| b.bottom)
    } else {
        tb.and_then(|b| b.inside_h)
    };
    let mut left = if is_first_col {
        tb.and_then(|b| b.left)
    } else {
        tb.and_then(|b| b.inside_v)
    };
    let mut right = if is_last_col {
        tb.and_then(|b| b.right)
    } else {
        tb.and_then(|b| b.inside_v)
    };

    // Per-cell borders from conditional formatting override.
    if let Some(ref cb) = cell.cell_borders {
        if let Some(v) = &cb.top {
            top = resolve_override(v);
        }
        if let Some(v) = &cb.bottom {
            bottom = resolve_override(v);
        }
        if let Some(v) = &cb.left {
            left = resolve_override(v);
        }
        if let Some(v) = &cb.right {
            right = resolve_override(v);
        }
    }

    (top, bottom, left, right)
}

/// §17.4.43: resolve a border conflict between two competing borders on
/// a shared edge.  Returns the winning border (or `None` if both are `None`).
///
/// Tie-breaking per [MS-OI29500] §17.4.66, in order:
///   1. Absent yields to present.
///   2. Weight = width × style number.  Higher wins.
///   3. Equal weight: heavier style wins (`Double` over `Single`).
///   4. Equal style: darker colour wins (`R+B+2G`, then `B+2G`, then `G`).
///
/// **The comparison is a total order, and that is the point.** The caller feeds
/// this (upper row's bottom, lower row's top) and (left cell's right, right
/// cell's left), so a rule that stops at step 2 leaves the winner decided by
/// *which side of the edge a border came from* — an implementation detail. Ties
/// used to fall through to whichever argument came first, which meant an
/// equal-weight 3pt single beat a 1pt double or lost to it depending on
/// argument order, and of two equal borders differing only in colour the paler
/// one won half the time. `resolve_border_conflict(a, b)` now always equals
/// `resolve_border_conflict(b, a)`.
///
/// Step 1 is narrower than the spec's: an explicit `<w:top w:val="nil"/>` and
/// "no border specified" both arrive here as `None`, because
/// `resolve_override` collapses them upstream. So an explicit nil *yields* to
/// the opposing border instead of suppressing it. `emit.rs` keeps the
/// distinction for its own top-border restore (`user_suppressed_top`), so the
/// information exists — it is just not threaded this far. Tracked as E5b#8.
pub(super) fn resolve_border_conflict(
    a: Option<TableBorderLine>,
    b: Option<TableBorderLine>,
) -> Option<TableBorderLine> {
    match (a, b) {
        (None, b) => b,
        (a, None) => a,
        (Some(a), Some(b)) => Some(match border_precedence(&a).cmp(&border_precedence(&b)) {
            std::cmp::Ordering::Less => b,
            _ => a,
        }),
    }
}

/// Sort key for §17.4.43 conflict resolution — greater wins.
///
/// Returns integers so the key is `Ord`: comparing `f32` weights directly would
/// need `partial_cmp`, and a `NaN` width (unreachable, but the type permits it)
/// would silently make the comparison non-transitive and reintroduce the
/// order-dependence this exists to remove.
///
/// Colour is inverted (`u32::MAX - luminance`) so that *darker* sorts greater,
/// matching "darker colour wins".
fn border_precedence(b: &TableBorderLine) -> (u32, u8, u32, u32, u32) {
    let (l0, l1, l2) = colour_luminance(b);
    (
        // Weight in eighths of a point, rounded — the spec's `sz` unit.
        (border_weight(b) * 8.0).round().max(0.0) as u32,
        style_rank(b.style),
        u32::MAX - l0,
        u32::MAX - l1,
        u32::MAX - l2,
    )
}

/// §17.4.43 style precedence. Only `Single` and `Double` reach layout (the
/// other 24 §17.4.38 styles are approximated as `Single` — see
/// `convert_model_border`), so the full precedence list is not modelled; of the
/// two that exist, `Double` is the heavier.
fn style_rank(style: TableBorderStyle) -> u8 {
    match style {
        TableBorderStyle::Single => 1,
        TableBorderStyle::Double => 2,
    }
}

/// [MS-OI29500] §17.4.66 darkness keys, compared in order: `R+B+2G`, then
/// `B+2G`, then `G`. Lower is darker.
fn colour_luminance(b: &TableBorderLine) -> (u32, u32, u32) {
    let (r, g, bl) = (b.color.r as u32, b.color.g as u32, b.color.b as u32);
    (r + bl + 2 * g, bl + 2 * g, g)
}

/// Emit all four borders for a cell as filled rectangles.
/// Borders are drawn INWARD from the cell edge per OOXML.
///
/// Horizontal borders (top/bottom) own the corner squares — they span the
/// full cell width. Vertical borders (left/right) fill only the space
/// between the horizontals. This eliminates anti-aliasing gaps at corners
/// that plagued the previous stroke-based approach.
pub(super) fn emit_cell_borders(
    commands: &mut Vec<DrawCommand>,
    b: CellBorders,
    cell_x: Pt,
    cell_w: Pt,
    row_y: Pt,
    row_h: Pt,
) {
    let top_w = b.top.map(|b| b.width).unwrap_or(Pt::ZERO);
    let bot_w = b.bottom.map(|b| b.width).unwrap_or(Pt::ZERO);
    let left_w = b.left.map(|b| b.width).unwrap_or(Pt::ZERO);
    let right_w = b.right.map(|b| b.width).unwrap_or(Pt::ZERO);

    // Horizontal borders: full cell width, covering corner squares.
    if let Some(ref border) = b.top {
        emit_border_rect(
            commands,
            border,
            PtRect::from_xywh(cell_x, row_y, cell_w, top_w),
            true,
        );
    }
    if let Some(ref border) = b.bottom {
        emit_border_rect(
            commands,
            border,
            PtRect::from_xywh(cell_x, row_y + row_h - bot_w, cell_w, bot_w),
            true,
        );
    }

    // Vertical borders: between horizontal borders (no corner overlap).
    let top_inset = if b.top.is_some() { top_w } else { Pt::ZERO };
    let bot_inset = if b.bottom.is_some() { bot_w } else { Pt::ZERO };
    let v_height = row_h - top_inset - bot_inset;
    if v_height > Pt::ZERO {
        if let Some(ref border) = b.left {
            emit_border_rect(
                commands,
                border,
                PtRect::from_xywh(cell_x, row_y + top_inset, left_w, v_height),
                false,
            );
        }
        if let Some(ref border) = b.right {
            emit_border_rect(
                commands,
                border,
                PtRect::from_xywh(
                    cell_x + cell_w - right_w,
                    row_y + top_inset,
                    right_w,
                    v_height,
                ),
                false,
            );
        }
    }
}

/// §17.4.43: border weight = width × style number, in points.
///
/// The spec states the rule in eighths of a point (`w:sz`), but every use is a
/// *comparison* between two weights, and converting both to eighths scales both
/// by the same 8 — so the factor cancels. Keeping it in points avoids implying
/// that a unit conversion is load-bearing here. `border_precedence` scales to
/// eighths once, where rounding to an integer sort key does depend on the unit.
fn border_weight(b: &TableBorderLine) -> f32 {
    let style_number = match b.style {
        TableBorderStyle::Single => 1.0,
        TableBorderStyle::Double => 3.0,
    };
    b.width.raw() * style_number
}

/// Extract border width or zero if absent.
pub(super) fn border_width(b: Option<TableBorderLine>) -> Pt {
    b.map(|b| b.width).unwrap_or(Pt::ZERO)
}

fn resolve_override(ovr: &CellBorderOverride) -> Option<TableBorderLine> {
    match ovr {
        CellBorderOverride::Nil => None,
        CellBorderOverride::Border(line) => Some(*line),
    }
}

/// Emit a border as filled rectangle(s).
/// `is_horizontal` controls double-border sub-rect orientation.
fn emit_border_rect(
    commands: &mut Vec<DrawCommand>,
    b: &TableBorderLine,
    rect: PtRect,
    is_horizontal: bool,
) {
    match b.style {
        TableBorderStyle::Single => {
            commands.push(DrawCommand::Rect {
                rect,
                color: b.color,
            });
        }
        TableBorderStyle::Double => {
            // §17.4.38: total = w:sz, each sub-line = sz/3, gap = sz/3.
            let sub = b.width * (1.0 / 3.0);
            if is_horizontal {
                // Two horizontal sub-rects: top and bottom of the border area.
                commands.push(DrawCommand::Rect {
                    rect: PtRect::from_xywh(rect.origin.x, rect.origin.y, rect.size.width, sub),
                    color: b.color,
                });
                commands.push(DrawCommand::Rect {
                    rect: PtRect::from_xywh(
                        rect.origin.x,
                        rect.origin.y + rect.size.height - sub,
                        rect.size.width,
                        sub,
                    ),
                    color: b.color,
                });
            } else {
                // Two vertical sub-rects: left and right of the border area.
                commands.push(DrawCommand::Rect {
                    rect: PtRect::from_xywh(rect.origin.x, rect.origin.y, sub, rect.size.height),
                    color: b.color,
                });
                commands.push(DrawCommand::Rect {
                    rect: PtRect::from_xywh(
                        rect.origin.x + rect.size.width - sub,
                        rect.origin.y,
                        sub,
                        rect.size.height,
                    ),
                    color: b.color,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::render::dimension::Pt;
    use crate::render::geometry::PtEdgeInsets;
    use crate::render::layout::draw_command::DrawCommand;
    use crate::render::layout::fragment::{FontProps, Fragment, TextMetrics};
    use crate::render::layout::paragraph::ParagraphStyle;
    use crate::render::layout::section::LayoutBlock;
    use crate::render::layout::table::{
        layout_table, CellVAlign, TableBorderConfig, TableBorderLine, TableBorderStyle,
        TableCellInput, TableRowInput,
    };
    use crate::render::resolve::color::RgbColor;
    use std::rc::Rc;

    fn text_frag(text: &str, width: f32) -> Fragment {
        Fragment::Text {
            text: text.into(),
            font: Rc::new(FontProps {
                family: Rc::from("Test"),
                size: Pt::new(12.0),
                bold: false,
                italic: false,
                underline: false,
                char_spacing: Pt::ZERO,
                text_scale: 1.0,
                underline_position: Pt::ZERO,
                underline_thickness: Pt::ZERO,
            }),
            color: RgbColor::BLACK,
            width: Pt::new(width),
            trimmed_width: Pt::new(width),
            metrics: TextMetrics {
                ascent: Pt::new(10.0),
                descent: Pt::new(4.0),
                leading: Pt::ZERO,
            },
            hyperlink_url: None,
            shading: None,
            border: None,
            baseline_offset: Pt::ZERO,
            text_offset: Pt::ZERO,
            is_footnote_ref: false,
        }
    }

    fn simple_cell(text: &str) -> TableCellInput {
        TableCellInput {
            blocks: vec![LayoutBlock::Paragraph {
                fragments: vec![text_frag(text, 30.0)],
                style: ParagraphStyle::default(),
                page_break_before: false,
                footnotes: vec![],
                floating_images: vec![],
                floating_shapes: vec![],
            }],
            margins: PtEdgeInsets::ZERO,
            grid_span: 1,
            shading: None,
            cell_borders: None,
            vertical_merge: None,
            vertical_align: CellVAlign::Top,
        }
    }

    #[test]
    fn borders_emit_lines() {
        let rows = vec![TableRowInput {
            cells: vec![simple_cell("a"), simple_cell("b")],
            height_rule: None,
            is_header: None,
            cant_split: None,
            grid_before: 0,
            border_overrides: None,
        }];
        let col_widths = vec![Pt::new(100.0), Pt::new(100.0)];
        let result = layout_table(
            &rows,
            &col_widths,
            Pt::new(14.0),
            Some(&TableBorderConfig {
                top: Some(TableBorderLine {
                    width: Pt::new(0.5),
                    color: RgbColor::BLACK,
                    style: TableBorderStyle::Single,
                }),
                bottom: Some(TableBorderLine {
                    width: Pt::new(0.5),
                    color: RgbColor::BLACK,
                    style: TableBorderStyle::Single,
                }),
                left: Some(TableBorderLine {
                    width: Pt::new(0.5),
                    color: RgbColor::BLACK,
                    style: TableBorderStyle::Single,
                }),
                right: Some(TableBorderLine {
                    width: Pt::new(0.5),
                    color: RgbColor::BLACK,
                    style: TableBorderStyle::Single,
                }),
                inside_h: Some(TableBorderLine {
                    width: Pt::new(0.5),
                    color: RgbColor::BLACK,
                    style: TableBorderStyle::Single,
                }),
                inside_v: Some(TableBorderLine {
                    width: Pt::new(0.5),
                    color: RgbColor::BLACK,
                    style: TableBorderStyle::Single,
                }),
            }),
            None,
            false,
        );

        // Borders are emitted as filled rects. Count border rects by
        // excluding cell shading rects (which use non-BLACK colors or
        // appear before borders in the command list).
        let border_rect_count = result
            .commands
            .iter()
            .filter(|c| matches!(c, DrawCommand::Rect { color, .. } if *color == RgbColor::BLACK))
            .count();
        // §17.4.43: shared edges drawn once after conflict resolution.
        // Top(2) + bottom(2) + left(1) + insideV(1) + right(1) = 7 border rects.
        assert_eq!(border_rect_count, 7);
    }

    /// §17.4.61 tblPrEx — when a row carries a `tblBorders` override,
    /// it fully replaces the table's tblBorders for *that row only*.
    /// Here row 0 sets every side to "no border", row 1 doesn't.
    /// The table-wide config has all sides set to single. Expectation:
    /// row 0's cell contributes zero border rects, while row 1's cell
    /// produces the usual top/left/right/bottom set.
    #[test]
    fn row_border_override_replaces_table_borders_for_that_row() {
        let single = TableBorderLine {
            width: Pt::new(0.5),
            color: RgbColor::BLACK,
            style: TableBorderStyle::Single,
        };
        let all_single = TableBorderConfig {
            top: Some(single),
            bottom: Some(single),
            left: Some(single),
            right: Some(single),
            inside_h: Some(single),
            inside_v: Some(single),
        };
        let no_borders = TableBorderConfig {
            top: None,
            bottom: None,
            left: None,
            right: None,
            inside_h: None,
            inside_v: None,
        };
        let rows = vec![
            TableRowInput {
                cells: vec![simple_cell("opt-out")],
                height_rule: None,
                is_header: None,
                cant_split: None,
                grid_before: 0,
                border_overrides: Some(no_borders),
            },
            TableRowInput {
                cells: vec![simple_cell("normal")],
                height_rule: None,
                is_header: None,
                cant_split: None,
                grid_before: 0,
                border_overrides: None,
            },
        ];
        let col_widths = vec![Pt::new(100.0)];
        let result = layout_table(
            &rows,
            &col_widths,
            Pt::new(14.0),
            Some(&all_single),
            None,
            false,
        );

        // Group border rects by their y position. The opt-out row is
        // first (lower y), the normal row second. We know the order
        // because layout_table walks rows top-down.
        let border_rects: Vec<_> = result
            .commands
            .iter()
            .filter_map(|c| match c {
                DrawCommand::Rect { rect, color } if *color == RgbColor::BLACK => Some(*rect),
                _ => None,
            })
            .collect();

        // No rect should sit entirely within row 0's vertical span —
        // not the cell's top, not its sides, not its bottom (with row 1
        // having a top border, conflict resolution gives row 0 a
        // bottom from row 1's top, but that's drawn at the boundary,
        // not inside row 0).
        // We exercise this by asserting that no rect's *vertical*
        // extent falls within (epsilon, row_0_height - epsilon) — the
        // strict interior of row 0.
        let row_0_height = Pt::new(14.0);
        let interior_eps = Pt::new(0.1);
        let interior_top = interior_eps;
        let interior_bottom = row_0_height - interior_eps;
        for rect in &border_rects {
            let r_top = rect.origin.y;
            let r_bottom = rect.origin.y + rect.size.height;
            let entirely_inside = r_top >= interior_top && r_bottom <= interior_bottom;
            assert!(
                !entirely_inside,
                "row 0 (border-override = all None) must not host a \
                 black border rect strictly inside its content area; got rect \
                 y=[{:.2}..{:.2}] (interior was ({:.2}..{:.2}))",
                r_top.raw(),
                r_bottom.raw(),
                interior_top.raw(),
                interior_bottom.raw(),
            );
        }
    }
}

#[cfg(test)]
mod conflict_tests {
    use super::*;
    use crate::render::resolve::color::RgbColor;

    const BLACK: RgbColor = RgbColor { r: 0, g: 0, b: 0 };
    const PALE: RgbColor = RgbColor {
        r: 220,
        g: 220,
        b: 220,
    };

    fn line(width: f32, style: TableBorderStyle, color: RgbColor) -> TableBorderLine {
        TableBorderLine {
            width: Pt::new(width),
            color,
            style,
        }
    }

    /// A representative spread: both styles, several widths, both colours.
    fn sample_borders() -> Vec<TableBorderLine> {
        let mut v = Vec::new();
        for &w in &[0.5f32, 1.0, 2.0, 3.0, 6.0] {
            for &s in &[TableBorderStyle::Single, TableBorderStyle::Double] {
                for &c in &[BLACK, PALE] {
                    v.push(line(w, s, c));
                }
            }
        }
        v
    }

    /// **The property that matters.** The caller passes (upper row's bottom,
    /// lower row's top) and (left cell's right, right cell's left), so a
    /// resolution that depends on argument order makes the rendered border
    /// depend on which *side of the edge* it was declared on.
    ///
    /// Before this was a total order, ties fell through to whichever argument
    /// came first: an equal-weight 3pt single beat a 1pt double or lost to it
    /// depending on the call, and of two borders differing only in colour the
    /// paler one won half the time.
    #[test]
    fn resolution_is_independent_of_argument_order() {
        let borders = sample_borders();
        for a in &borders {
            for b in &borders {
                let ab = resolve_border_conflict(Some(*a), Some(*b));
                let ba = resolve_border_conflict(Some(*b), Some(*a));
                assert_eq!(
                    (ab.map(|x| (x.width, x.style, x.color))),
                    (ba.map(|x| (x.width, x.style, x.color))),
                    "order-dependent for {a:?} vs {b:?}"
                );
            }
        }
    }

    /// Step 2 — the heavier border wins outright.
    #[test]
    fn heavier_weight_wins() {
        let thin = line(0.5, TableBorderStyle::Single, BLACK);
        let thick = line(2.0, TableBorderStyle::Single, BLACK);
        assert_eq!(
            resolve_border_conflict(Some(thin), Some(thick)).map(|b| b.width),
            Some(Pt::new(2.0))
        );
        assert_eq!(
            resolve_border_conflict(Some(thick), Some(thin)).map(|b| b.width),
            Some(Pt::new(2.0))
        );
    }

    /// Step 3 — equal weight, so the heavier *style* decides. 3pt single and
    /// 1pt double both weigh 3 (width × style number), which is exactly the tie
    /// the old code resolved by argument position.
    #[test]
    fn equal_weight_prefers_the_heavier_style() {
        let single = line(3.0, TableBorderStyle::Single, BLACK);
        let double = line(1.0, TableBorderStyle::Double, BLACK);
        assert_eq!(
            border_weight(&single),
            border_weight(&double),
            "same weight"
        );

        for (a, b) in [(single, double), (double, single)] {
            assert_eq!(
                resolve_border_conflict(Some(a), Some(b)).map(|x| x.style),
                Some(TableBorderStyle::Double),
                "Double outranks Single at equal weight"
            );
        }
    }

    /// Step 4 — equal weight and style, so the darker colour decides.
    #[test]
    fn equal_weight_and_style_prefers_the_darker_colour() {
        let dark = line(1.0, TableBorderStyle::Single, BLACK);
        let pale = line(1.0, TableBorderStyle::Single, PALE);
        for (a, b) in [(dark, pale), (pale, dark)] {
            assert_eq!(
                resolve_border_conflict(Some(a), Some(b)).map(|x| x.color),
                Some(BLACK),
                "darker colour wins regardless of argument order"
            );
        }
    }

    /// The §17.4.66 darkness keys are compared in order `R+B+2G`, then `B+2G`,
    /// then `G` — so two colours with the same total brightness are separated by
    /// the later keys rather than by argument order.
    #[test]
    fn darkness_tie_breaks_on_the_secondary_keys() {
        // R+B+2G equal (both 255*2 = 510... constructed to match), differing in
        // the B+2G term.
        let a = line(
            1.0,
            TableBorderStyle::Single,
            RgbColor { r: 100, g: 0, b: 0 },
        );
        let b = line(
            1.0,
            TableBorderStyle::Single,
            RgbColor { r: 0, g: 0, b: 100 },
        );
        assert_eq!(
            colour_luminance(&a).0,
            colour_luminance(&b).0,
            "primary key ties"
        );
        // a has B+2G = 0, b has B+2G = 100 → a is "darker" by the second key.
        let winner = resolve_border_conflict(Some(a), Some(b)).expect("some");
        assert_eq!(winner.color, RgbColor { r: 100, g: 0, b: 0 });
        // And symmetric.
        assert_eq!(
            resolve_border_conflict(Some(b), Some(a)).map(|x| x.color),
            Some(RgbColor { r: 100, g: 0, b: 0 })
        );
    }

    /// Step 1 — an absent border yields to a present one, in both directions,
    /// and two absent borders stay absent.
    #[test]
    fn absent_yields_to_present() {
        let some = line(1.0, TableBorderStyle::Single, BLACK);
        assert_eq!(
            resolve_border_conflict(None, Some(some)).map(|b| b.width),
            Some(Pt::new(1.0))
        );
        assert_eq!(
            resolve_border_conflict(Some(some), None).map(|b| b.width),
            Some(Pt::new(1.0))
        );
        assert!(resolve_border_conflict(None, None).is_none());
    }

    /// Identical borders resolve to themselves — the reflexive case, which a
    /// comparison built on `partial_cmp` of `f32` could get wrong.
    #[test]
    fn identical_borders_resolve_to_themselves() {
        for b in sample_borders() {
            let r = resolve_border_conflict(Some(b), Some(b)).expect("some");
            assert_eq!((r.width, r.style, r.color), (b.width, b.style, b.color));
        }
    }
}
