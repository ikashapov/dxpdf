use crate::render::dimension::Pt;
use crate::render::geometry::PtRect;

use super::types::{
    CellBorderOverride, TableBorderConfig, TableBorderLine, TableBorderStyle, TableCellInput,
};
use crate::render::layout::draw_command::DrawCommand;

/// One cell edge during and after §17.4.38 resolution.
///
/// Three states rather than `Option<TableBorderLine>`, because [MS-OI29500]
/// §17.4.66 distinguishes "nothing said about this edge" from "declared
/// `val="nil"`". The difference is entirely about **inheritance**: an omitted
/// or `none` edge falls back to the table style, then `tblPrEx`, then
/// `tblBorders`; `nil` declines that fallback and stays empty.
///
/// It is *not* about outranking the facing cell. `nil` removes this cell's
/// border and nothing else — see [`resolve_border_conflict`].
///
/// The distinction survives resolution for one downstream reader: the page-split
/// top-border restore in `emit.rs` may revive an `Absent` top but must not
/// revive a `Suppressed` one. For painting they are identical, which is what
/// [`CellEdge::line`] expresses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CellEdge {
    /// Nothing said about this edge — or it was declared `val="none"`, which
    /// §17.4.66 treats identically. Inherits, then yields.
    Absent,
    /// Declared `val="nil"`: no border here, and no inheritance either.
    Suppressed,
    /// A border to resolve against the opposing edge, and paint if it wins.
    Line(TableBorderLine),
}

impl CellEdge {
    /// The line to paint, if any. Both `Absent` and `Suppressed` paint nothing.
    pub(super) fn line(self) -> Option<TableBorderLine> {
        match self {
            Self::Line(l) => Some(l),
            Self::Absent | Self::Suppressed => None,
        }
    }

    /// Whether two *resolved* edges paint the same thing.
    ///
    /// Not `==`: by this point the `Absent`/`Suppressed` distinction is not
    /// observable to the painter, and letting it in would split a run of columns
    /// that paints one continuous line. Callers asking "can one cell draw this
    /// whole span in a single stroke?" mean *this* question.
    pub(super) fn paints_same(self, other: Self) -> bool {
        self.line() == other.line()
    }
}

impl From<Option<TableBorderLine>> for CellEdge {
    /// Table-level borders have no way to express `nil` — an edge is either
    /// configured or not — so an absent one is `Absent`, never `Suppressed`.
    fn from(b: Option<TableBorderLine>) -> Self {
        match b {
            Some(l) => Self::Line(l),
            None => Self::Absent,
        }
    }
}

/// Resolved borders for one cell.
#[derive(Clone)]
pub(super) struct CellBorders {
    pub(super) top: CellEdge,
    pub(super) bottom: CellEdge,
    pub(super) left: CellEdge,
    pub(super) right: CellEdge,
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
#[allow(clippy::too_many_arguments)] // one cell's grid position; cohesive
pub(super) fn resolve_cell_effective_borders(
    cell: &TableCellInput,
    table_borders: Option<&TableBorderConfig>,
    row_idx: usize,
    cell_grid_col: usize,
    cell_grid_span: usize,
    num_rows: usize,
    num_grid_cols: usize,
    // §17.4.44: whether this table has a non-zero `w:tblCellSpacing`. See the
    // `outer` closure below — it is the whole reason this parameter exists.
    spaced: bool,
) -> (CellEdge, CellEdge, CellEdge, CellEdge) {
    // Start with table-level borders mapped to cell edges.
    let tb = table_borders;
    let is_first_row = row_idx == 0;
    // `row_idx + 1 == num_rows`, not `row_idx == num_rows - 1`: the latter
    // underflows on an empty table. No caller passes `num_rows == 0` today, but
    // the parameter is free and the guard would live entirely in the callers.
    let is_last_row = row_idx + 1 == num_rows;
    let is_first_col = cell_grid_col == 0;
    let is_last_col = cell_grid_col + cell_grid_span >= num_grid_cols;

    // §17.4.44 / issue #168: with a non-zero cell spacing the outer edges are
    // **not** seeded from the table's own borders. A spaced cell is inset from
    // the table's boundary, so a table border painted on it lands in the wrong
    // place — and once `emit_table_outline` draws that border where it belongs,
    // seeding it here as well would paint it twice.
    //
    // The interior seeding is deliberately left alone. With a gap there is no
    // shared edge for `insideH`/`insideV` to sit on either, so what they mean
    // for a spaced table is a real question — but [MS-OI29500] §17.4.66 names
    // only "cell borders and outer table borders", and answering a second
    // unsettled question inside this one is how a fix stops being reviewable.
    //
    // **Word reference render needed** (issue #165 has the batch): a spaced
    // table with `insideV` set whose cells carry no `w:tcBorders`. If Word
    // draws one line per cell edge, today's behaviour is right; if it draws one
    // line in the gap, or none, this seeding has to change too.
    let outer = |line: Option<TableBorderLine>| -> CellEdge {
        if spaced {
            CellEdge::Absent
        } else {
            line.into()
        }
    };
    let mut top: CellEdge = if is_first_row {
        outer(tb.and_then(|b| b.top))
    } else {
        tb.and_then(|b| b.inside_h).into()
    };
    let mut bottom: CellEdge = if is_last_row {
        outer(tb.and_then(|b| b.bottom))
    } else {
        tb.and_then(|b| b.inside_h).into()
    };
    let mut left: CellEdge = if is_first_col {
        outer(tb.and_then(|b| b.left))
    } else {
        tb.and_then(|b| b.inside_v).into()
    };
    let mut right: CellEdge = if is_last_col {
        outer(tb.and_then(|b| b.right))
    } else {
        tb.and_then(|b| b.inside_v).into()
    };

    // Per-cell overrides. Only `nil` and a real border reach here — an explicit
    // `none` was mapped to "no override" upstream (§17.4.66: it inherits
    // exactly like an omitted edge), so it correctly leaves the table-level
    // border above untouched instead of erasing it.
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

/// Resolve a border conflict between two competing borders on a shared edge.
/// Returns the winning border (or `None` if both are `None`).
///
/// The algorithm is **not in ISO/IEC 29500-1** — the standard only says a method
/// exists. It is spelled out in [MS-OI29500] §17.4.66 (`tcBorders`, note a),
/// which is the authority for every step below:
///   1. An edge with no border yields to one that has it. `none` counts as
///      no border, per *"If the conflicting table cell border is `none` (no
///      border), then the opposing border shall be displayed."*
///   2. Weight = width in eighths of a point × style number. Higher wins.
///   3. Equal weight: the style **earlier in the spec's precedence list** wins —
///      `Single` over `Double`. See `style_precedence_index`.
///   4. Equal style: darker colour wins (`R+B+2G`, then `B+2G`, then `G`).
///
/// **What `nil` does, and what it does not.** The note adds *"If the conflicting
/// table cell border is `nil`, then no border shall be displayed"*, which reads
/// as `nil` beating everything on the far side of the edge. It does not, and
/// implementing it that way deleted borders Word draws. `nil` acts on **its own
/// cell only**: it is how a cell declines the inheritance the note describes one
/// step earlier (style → `tblPrEx` → `tblBorders`), which is the whole of its
/// difference from `none`. The facing cell's border is untouched, so
/// `Suppressed` yields here exactly like `Absent`.
///
/// Three independent facts in `IP 05 Trenches` fix the reading, and no evidence
/// contradicts it:
///
/// * a cell declaring `<w:bottom w:val="single"/>` above one declaring
///   `<w:top w:val="nil"/>` — Word draws the line, as does macOS's own DOCX
///   renderer on the same markup;
/// * a cell that *inherits* its bottom from `insideH`, faced by a `gridSpan=2`
///   spacer cell whose `nil` was aimed at the neighbouring column — Word draws
///   that line too, and it could not do otherwise: a cell paints one border
///   across its whole width, so a wide cell's `nil` cannot punch a hole in the
///   cell above it;
/// * down the document's spacer columns the generator writes `nil` on **both**
///   sides of every shared edge. Writing both is only necessary because one
///   alone does not suppress.
///
/// `nil` is still not a no-op: with nothing facing it — a table's outer edge, or
/// a facing cell that is also `nil` — declining inheritance is exactly what
/// removes the border. Both halves are pinned by
/// `tests/table_border_conflict.rs`.
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
/// Suppression is still a *third* state, which is why the argument type is
/// [`CellEdge`] and not `Option<TableBorderLine>`: when neither side paints,
/// returning `Suppressed` rather than `Absent` keeps the two distinguishable for
/// the caller — a suppressed edge must not be revived by the page-split
/// top-border restore in `emit.rs`, whereas an absent one should be.
pub(super) fn resolve_border_conflict(a: CellEdge, b: CellEdge) -> CellEdge {
    match (a, b) {
        (CellEdge::Line(la), CellEdge::Line(lb)) => {
            match border_precedence(&la).cmp(&border_precedence(&lb)) {
                std::cmp::Ordering::Less => b,
                _ => a,
            }
        }
        // One side paints: it does so regardless of what the other side says.
        // A facing `nil` removed *its* border, not this one.
        (CellEdge::Line(_), _) => a,
        (_, CellEdge::Line(_)) => b,
        // Neither side paints. Carry suppression forward so the page-split
        // restore cannot revive an edge the author explicitly emptied.
        (CellEdge::Suppressed, _) | (_, CellEdge::Suppressed) => CellEdge::Suppressed,
        (CellEdge::Absent, CellEdge::Absent) => CellEdge::Absent,
    }
}

/// Sort key for [MS-OI29500] §17.4.66 conflict resolution — greater wins.
///
/// Returns integers so the key is `Ord`: comparing `f32` weights directly would
/// need `partial_cmp`, and a `NaN` width (unreachable, but the type permits it)
/// would silently make the comparison non-transitive and reintroduce the
/// order-dependence this exists to remove.
///
/// **Two fields are inverted, and for the same reason.** The spec states both
/// style and colour as "lower value wins" rankings — earliest in the precedence
/// list, and smallest brightness. This key is "greater wins", so each is
/// subtracted from its type's maximum. Inverting one and not the other is the
/// defect this layout is meant to make obvious.
fn border_precedence(b: &TableBorderLine) -> (u32, u8, u32, u32, u32) {
    let (l0, l1, l2) = colour_luminance(b);
    (
        // Weight in eighths of a point, rounded — the spec's `sz` unit.
        (border_weight(b) * 8.0).round().max(0.0) as u32,
        u8::MAX - style_precedence_index(b.style),
        u32::MAX - l0,
        u32::MAX - l1,
        u32::MAX - l2,
    )
}

/// [MS-OI29500] §17.4.66 style precedence: at equal weight, *"the higher of the
/// two on this precedence list shall be displayed"*, the list being
///
/// > single, thick, double, dotted, dashed, dotDash, dotDotDash, triple,
/// > thinThickSmallGap, … outset, inset
///
/// "Higher on the list" means **earlier**, so this returns the 0-based index
/// into it and **lower wins** — `border_precedence` inverts it.
///
/// So `Single` beats `Double` at equal weight, which is worth stating plainly
/// because the intuition runs the other way: a double border has the greater
/// *style number* (3 vs 1) and therefore the greater weight at equal width, and
/// it is easy to carry that ordering into the tie-break, where the spec
/// reverses it. Equal weight means the single is three times wider — a 3pt
/// solid line against two 0.33pt hairlines — and the spec prefers the single.
///
/// Only `Single` and `Double` reach layout (the other 24 §17.4.38 styles are
/// approximated as `Single` — see `convert_model_border`), so only their two
/// positions are modelled: single is first, double is third.
fn style_precedence_index(style: TableBorderStyle) -> u8 {
    match style {
        TableBorderStyle::Single => 0,
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
    // Resolution is over by now, so `Suppressed` and `Absent` are the same
    // thing here: nothing to paint.
    let (top, bottom, left, right) = (b.top.line(), b.bottom.line(), b.left.line(), b.right.line());
    let top_w = top.map(|b| b.width).unwrap_or(Pt::ZERO);
    let bot_w = bottom.map(|b| b.width).unwrap_or(Pt::ZERO);
    let left_w = left.map(|b| b.width).unwrap_or(Pt::ZERO);
    let right_w = right.map(|b| b.width).unwrap_or(Pt::ZERO);

    // Horizontal borders: full cell width, covering corner squares.
    if let Some(ref border) = top {
        emit_border_rect(
            commands,
            border,
            PtRect::from_xywh(cell_x, row_y, cell_w, top_w),
            true,
        );
    }
    if let Some(ref border) = bottom {
        emit_border_rect(
            commands,
            border,
            PtRect::from_xywh(cell_x, row_y + row_h - bot_w, cell_w, bot_w),
            true,
        );
    }

    // Vertical borders: between horizontal borders (no corner overlap).
    let top_inset = if top.is_some() { top_w } else { Pt::ZERO };
    let bot_inset = if bottom.is_some() { bot_w } else { Pt::ZERO };
    let v_height = row_h - top_inset - bot_inset;
    if v_height > Pt::ZERO {
        if let Some(ref border) = left {
            emit_border_rect(
                commands,
                border,
                PtRect::from_xywh(cell_x, row_y + top_inset, left_w, v_height),
                false,
            );
        }
        if let Some(ref border) = right {
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

/// §17.4.44 / issue #168: draw the table's own outer border, for a table whose
/// `w:tblCellSpacing` is non-zero.
///
/// [MS-OI29500] §17.4.66: *"If the cell spacing is nonzero ... then all cell
/// borders and outer table borders display."* Everywhere else in this engine a
/// table border exists only as a **cell** edge, which is exactly right while the
/// spacing is zero — the outer cells' edges are then the table's edges. Once
/// there is a gap they are not, and nothing else in the pipeline draws the
/// table's own rectangle.
///
/// `rect` is the slice's box in table-local coordinates. `draw_top` and
/// `draw_bottom` are false on the sides where a paginated table continues:
/// an intermediate slice ends at a page cut, not at the table's edge, so it
/// gets left and right only.
///
/// Geometry mirrors [`emit_cell_borders`] exactly — horizontals span the full
/// width and own the corners, verticals are inset between them — so an outline
/// and a cell edge of the same width meet the same way a cell edge meets its
/// neighbour.
pub(super) fn emit_table_outline(
    commands: &mut Vec<DrawCommand>,
    borders: Option<&TableBorderConfig>,
    rect: PtRect,
    draw_top: bool,
    draw_bottom: bool,
) {
    let Some(cfg) = borders else {
        return;
    };
    let top = if draw_top { cfg.top } else { None };
    let bottom = if draw_bottom { cfg.bottom } else { None };
    let (x, y) = (rect.origin.x, rect.origin.y);
    let (w, h) = (rect.size.width, rect.size.height);

    let top_w = top.map(|b| b.width).unwrap_or(Pt::ZERO);
    let bot_w = bottom.map(|b| b.width).unwrap_or(Pt::ZERO);

    if let Some(ref border) = top {
        emit_border_rect(commands, border, PtRect::from_xywh(x, y, w, top_w), true);
    }
    if let Some(ref border) = bottom {
        emit_border_rect(
            commands,
            border,
            PtRect::from_xywh(x, y + h - bot_w, w, bot_w),
            true,
        );
    }

    let v_height = h - top_w - bot_w;
    if v_height > Pt::ZERO {
        if let Some(ref border) = cfg.left {
            emit_border_rect(
                commands,
                border,
                PtRect::from_xywh(x, y + top_w, border.width, v_height),
                false,
            );
        }
        if let Some(ref border) = cfg.right {
            emit_border_rect(
                commands,
                border,
                PtRect::from_xywh(x + w - border.width, y + top_w, border.width, v_height),
                false,
            );
        }
    }
}

/// [MS-OI29500] §17.4.66: border weight = width × style number, in points.
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

/// Width of the line this edge paints, or zero when it paints none — which
/// includes a suppressed edge, since suppression reserves no space.
pub(super) fn border_width(b: CellEdge) -> Pt {
    b.line().map(|b| b.width).unwrap_or(Pt::ZERO)
}

fn resolve_override(ovr: &CellBorderOverride) -> CellEdge {
    match ovr {
        CellBorderOverride::Suppress => CellEdge::Suppressed,
        // The cell's own `<w:tcBorders>` — the provenance that beats a facing
        // `nil` in `resolve_border_conflict`.
        CellBorderOverride::Border(line) => CellEdge::Line(*line),
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
    use crate::render::fonts::Toggle;
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
            shaped: None,
            level: crate::i18n::bidi::BidiLevel::LTR,
            text: text.into(),
            break_after: crate::render::layout::fragment::fixture_break_after(text),
            font: Rc::new(FontProps {
                rtl: crate::render::fonts::Toggle::Absent,
                family: Rc::from("Test"),
                size: Pt::new(12.0),
                bold: Toggle::Absent,
                italic: Toggle::Absent,
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
            Pt::ZERO,
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
        // [MS-OI29500] §17.4.66: shared edges drawn once after conflict resolution.
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
            Pt::ZERO,
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

    // ── issue #168: the outer table border of a spaced table ────────────────

    fn all_borders(width: f32) -> TableBorderConfig {
        let line = TableBorderLine {
            width: Pt::new(width),
            color: RgbColor::BLACK,
            style: TableBorderStyle::Single,
        };
        TableBorderConfig {
            top: Some(line),
            bottom: Some(line),
            left: Some(line),
            right: Some(line),
            inside_h: Some(line),
            inside_v: Some(line),
        }
    }

    /// One `Rect` command as `(x, y, w, h)`.
    type R = (f32, f32, f32, f32);

    /// Every `Rect` command, flattened to plain numbers so a failing assertion
    /// prints geometry rather than a wall of `Pt` wrappers.
    fn rects(cmds: &[DrawCommand]) -> Vec<R> {
        cmds.iter()
            .filter_map(|c| match c {
                DrawCommand::Rect { rect, .. } => Some((
                    rect.origin.x.raw(),
                    rect.origin.y.raw(),
                    rect.size.width.raw(),
                    rect.size.height.raw(),
                )),
                _ => None,
            })
            .collect()
    }

    fn two_rows() -> Vec<TableRowInput> {
        (0..2)
            .map(|_| TableRowInput {
                cells: vec![simple_cell("a"), simple_cell("b")],
                height_rule: None,
                is_header: None,
                cant_split: None,
                grid_before: 0,
                border_overrides: None,
            })
            .collect()
    }

    const NEAR: f32 = 0.01;

    /// The defect. [MS-OI29500] §17.4.66: *"If the cell spacing is nonzero ...
    /// then all cell borders and outer table borders display."* Before this
    /// fix nothing was ever drawn at the table's own bounds — table borders
    /// existed only as cell edges, which are inset by the spacing.
    #[test]
    fn a_spaced_table_draws_its_outer_border_at_the_table_bounds() {
        let cfg = all_borders(1.0);
        let result = layout_table(
            &two_rows(),
            &[Pt::new(100.0), Pt::new(100.0)],
            Pt::new(20.0),
            Pt::new(14.0),
            Some(&cfg),
            None,
            false,
        );
        let (w, h) = (result.size.width.raw(), result.size.height.raw());
        let r = rects(&result.commands);

        assert!(
            r.iter().any(|t| t.1.abs() < NEAR && (t.2 - w).abs() < NEAR),
            "no top outline spanning the table width at y=0; rects={r:?} (w={w}, h={h})"
        );
        assert!(
            r.iter()
                .any(|t| (t.1 + t.3 - h).abs() < NEAR && (t.2 - w).abs() < NEAR),
            "no bottom outline at the table's bottom edge; rects={r:?} (w={w}, h={h})"
        );
        assert!(
            r.iter().any(|t| t.0.abs() < NEAR && t.3 > h * 0.5),
            "no left outline down the table's left edge; rects={r:?} (w={w}, h={h})"
        );
        assert!(
            r.iter()
                .any(|t| (t.0 + t.2 - w).abs() < NEAR && t.3 > h * 0.5),
            "no right outline down the table's right edge; rects={r:?} (w={w}, h={h})"
        );
    }

    /// The guarantee the whole corpus rests on. With no spacing the outer
    /// cells' edges *are* the table's edges, the existing mapping is correct,
    /// and this fix must not add a single rect.
    ///
    /// Two rows deliberately: in a one-row table the row's own height equals
    /// the table's, so a full-height vertical rect would be ambiguous. With two
    /// rows only an outline can span the whole table.
    #[test]
    fn a_zero_spacing_table_draws_no_outline() {
        let cfg = all_borders(1.0);
        let result = layout_table(
            &two_rows(),
            &[Pt::new(100.0), Pt::new(100.0)],
            Pt::ZERO,
            Pt::new(14.0),
            Some(&cfg),
            None,
            false,
        );
        let h = result.size.height.raw();
        let spanning: Vec<_> = rects(&result.commands)
            .into_iter()
            .filter(|t| t.3 > h * 0.9 && t.2 < 5.0)
            .collect();
        assert!(
            spanning.is_empty(),
            "a zero-spacing table must draw no table-height outline, got {spanning:?}"
        );
    }

    /// A spaced table split across pages: left and right bound every slice,
    /// but the table's top edge exists only where the table starts and its
    /// bottom edge only where it ends. An intermediate slice stops at a page
    /// cut, not at the table's boundary, and drawing a horizontal rule there
    /// would draw a table edge that does not exist.
    #[test]
    fn a_paginated_spaced_table_splits_its_outline_across_slices() {
        use crate::render::layout::table::{layout_table_paginated, TablePaginationConfig};

        let cfg = all_borders(1.0);
        // Six rows against a short page, so the table needs at least three
        // slices and therefore has a middle one with neither horizontal edge.
        let rows: Vec<TableRowInput> = (0..6)
            .map(|_| TableRowInput {
                cells: vec![simple_cell("a")],
                height_rule: None,
                is_header: None,
                cant_split: None,
                grid_before: 0,
                border_overrides: None,
            })
            .collect();
        let slices = layout_table_paginated(
            &rows,
            &[Pt::new(100.0)],
            Pt::new(20.0),
            Pt::new(14.0),
            Some(&cfg),
            None,
            &TablePaginationConfig {
                available_height: Pt::new(80.0),
                page_height: Pt::new(80.0),
                suppress_first_row_top: false,
            },
        );
        assert!(
            slices.len() >= 3,
            "need a middle slice to test; got {}",
            slices.len()
        );

        let last = slices.len() - 1;
        for (i, slice) in slices.iter().enumerate() {
            let (w, h) = (slice.size.width.raw(), slice.size.height.raw());
            let r = rects(&slice.commands);
            let spans_width = |y: f32| {
                r.iter()
                    .any(|t| (t.1 - y).abs() < NEAR && (t.2 - w).abs() < NEAR)
            };

            assert_eq!(
                spans_width(0.0),
                i == 0,
                "slice {i}: top edge should be present only on the first slice"
            );
            let bottom_present = r
                .iter()
                .any(|t| (t.1 + t.3 - h).abs() < NEAR && (t.2 - w).abs() < NEAR);
            assert_eq!(
                bottom_present,
                i == last,
                "slice {i}: bottom edge should be present only on the last slice"
            );
            assert!(
                r.iter().any(|t| t.0.abs() < NEAR && t.3 > h * 0.4),
                "slice {i}: left edge must bound every slice; rects={r:?}"
            );
            assert!(
                r.iter()
                    .any(|t| (t.0 + t.2 - w).abs() < NEAR && t.3 > h * 0.4),
                "slice {i}: right edge must bound every slice; rects={r:?}"
            );
        }
    }

    /// The other half of §17.4.66's sentence: the cells' own borders keep
    /// drawing, at the cells' rectangles, alongside the outline. A fix that
    /// moved the border out to the table bounds and dropped the cell edges
    /// would satisfy the first test and still be wrong.
    #[test]
    fn a_spaced_table_draws_cell_borders_as_well_as_the_outline() {
        let cfg = all_borders(1.0);
        let result = layout_table(
            &two_rows(),
            &[Pt::new(100.0), Pt::new(100.0)],
            Pt::new(20.0),
            Pt::new(14.0),
            Some(&cfg),
            None,
            false,
        );
        let w = result.size.width.raw();
        // A rect that touches neither the left nor the right table edge can
        // only belong to a cell.
        let interior = rects(&result.commands)
            .into_iter()
            .filter(|t| t.0 > NEAR && (t.0 + t.2) < w - NEAR)
            .count();
        assert!(
            interior > 0,
            "the cells' own borders vanished; only the outline is left"
        );
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
                let ab = resolve_border_conflict(CellEdge::Line(*a), CellEdge::Line(*b));
                let ba = resolve_border_conflict(CellEdge::Line(*b), CellEdge::Line(*a));
                assert_eq!(
                    (ab.line().map(|x| (x.width, x.style, x.color))),
                    (ba.line().map(|x| (x.width, x.style, x.color))),
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
            resolve_border_conflict(CellEdge::Line(thin), CellEdge::Line(thick))
                .line()
                .map(|b| b.width),
            Some(Pt::new(2.0))
        );
        assert_eq!(
            resolve_border_conflict(CellEdge::Line(thick), CellEdge::Line(thin))
                .line()
                .map(|b| b.width),
            Some(Pt::new(2.0))
        );
    }

    /// Step 3 — equal weight, so position in the spec's precedence list decides,
    /// and **`Single` wins**. 3pt single and 1pt double both weigh 3
    /// (width × style number), which is exactly the tie the pre-E5b#2 code
    /// resolved by argument position.
    ///
    /// This test previously asserted the opposite, and was mutation-checked in
    /// that state — the code and the assertion shared one error, so no mutation
    /// could expose it. [MS-OI29500] §17.4.66 orders the list
    /// `single, thick, double, …` and displays *"the higher of the two on this
    /// precedence list"*, i.e. the earlier one.
    #[test]
    fn equal_weight_prefers_the_earlier_style_in_the_precedence_list() {
        let single = line(3.0, TableBorderStyle::Single, BLACK);
        let double = line(1.0, TableBorderStyle::Double, BLACK);
        assert_eq!(
            border_weight(&single),
            border_weight(&double),
            "same weight"
        );

        for (a, b) in [(single, double), (double, single)] {
            assert_eq!(
                resolve_border_conflict(CellEdge::Line(a), CellEdge::Line(b))
                    .line()
                    .map(|x| x.style),
                Some(TableBorderStyle::Single),
                "Single is earlier in the precedence list, so it wins at equal weight"
            );
        }
    }

    /// The tie-break must not leak into the *weight* comparison: a double
    /// border of equal width still outweighs a single (style number 3 vs 1) and
    /// wins at step 2, before precedence is consulted.
    ///
    /// Pins the two steps apart. Ranking `Single` above `Double` is only correct
    /// as a tie-break; applied one step earlier it would invert every ordinary
    /// single-vs-double edge in a table.
    #[test]
    fn precedence_does_not_override_weight() {
        let single = line(1.0, TableBorderStyle::Single, BLACK);
        let double = line(1.0, TableBorderStyle::Double, BLACK);
        assert!(
            border_weight(&double) > border_weight(&single),
            "equal width, double is heavier"
        );

        for (a, b) in [(single, double), (double, single)] {
            assert_eq!(
                resolve_border_conflict(CellEdge::Line(a), CellEdge::Line(b))
                    .line()
                    .map(|x| x.style),
                Some(TableBorderStyle::Double),
                "the heavier border wins outright, regardless of precedence"
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
                resolve_border_conflict(CellEdge::Line(a), CellEdge::Line(b))
                    .line()
                    .map(|x| x.color),
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
        let winner = resolve_border_conflict(CellEdge::Line(a), CellEdge::Line(b))
            .line()
            .expect("some");
        assert_eq!(winner.color, RgbColor { r: 100, g: 0, b: 0 });
        // And symmetric.
        assert_eq!(
            resolve_border_conflict(CellEdge::Line(b), CellEdge::Line(a))
                .line()
                .map(|x| x.color),
            Some(RgbColor { r: 100, g: 0, b: 0 })
        );
    }

    /// Step 1 — an absent border yields to a present one, in both directions,
    /// and two absent borders stay absent.
    #[test]
    fn absent_yields_to_present() {
        let some = line(1.0, TableBorderStyle::Single, BLACK);
        assert_eq!(
            resolve_border_conflict(CellEdge::Absent, CellEdge::Line(some))
                .line()
                .map(|b| b.width),
            Some(Pt::new(1.0))
        );
        assert_eq!(
            resolve_border_conflict(CellEdge::Line(some), CellEdge::Absent)
                .line()
                .map(|b| b.width),
            Some(Pt::new(1.0))
        );
        assert_eq!(
            resolve_border_conflict(CellEdge::Absent, CellEdge::Absent),
            CellEdge::Absent
        );
    }

    /// **`nil` does not reach across the edge.** [MS-OI29500] §17.4.66 says
    /// *"If the conflicting table cell border is nil, then no border shall be
    /// displayed"*, and read literally that is wrong: `nil` empties its own
    /// cell's edge and leaves the facing cell's border alone. It loses from
    /// either side and at any weight — even a hairline survives it.
    ///
    /// `IP 05 Trenches` is the reference. `<w:bottom w:val="single"/>` above
    /// `<w:top w:val="nil"/>` draws in Word and in macOS's DOCX renderer; so
    /// does an *inherited* bottom faced by a `gridSpan` spacer cell's `nil`, and
    /// it must — a cell paints one border across its whole width, so a wide
    /// cell's `nil` cannot punch a hole in the cell above it.
    #[test]
    fn nil_yields_to_the_facing_border() {
        let hair = line(0.25, TableBorderStyle::Single, BLACK);
        for (a, b) in [
            (CellEdge::Suppressed, CellEdge::Line(hair)),
            (CellEdge::Line(hair), CellEdge::Suppressed),
        ] {
            assert_eq!(
                resolve_border_conflict(a, b).line(),
                Some(hair),
                "the facing border must survive the nil: {a:?} vs {b:?}"
            );
        }
    }

    /// …and yet `nil` is not a no-op, because it declined **inheritance**
    /// upstream in `resolve_cell_effective_borders`. With nothing facing it —
    /// another `nil`, or an edge nobody spoke for — nothing is painted, and the
    /// result stays `Suppressed` rather than collapsing to `Absent`.
    ///
    /// That last part is load-bearing: `emit.rs` may revive an `Absent` top when
    /// a row starts a page slice, and must not revive an emptied one.
    #[test]
    fn nil_stays_suppressed_when_nothing_faces_it() {
        for (a, b) in [
            (CellEdge::Suppressed, CellEdge::Absent),
            (CellEdge::Absent, CellEdge::Suppressed),
            (CellEdge::Suppressed, CellEdge::Suppressed),
        ] {
            assert_eq!(
                resolve_border_conflict(a, b),
                CellEdge::Suppressed,
                "suppression must survive where nothing paints: {a:?} vs {b:?}"
            );
        }
        assert_eq!(
            resolve_border_conflict(CellEdge::Absent, CellEdge::Absent),
            CellEdge::Absent,
            "…but two silent edges stay restorable"
        );
    }

    /// The counterpart, and the half that is easy to get wrong when fixing the
    /// other: an edge declared `none` is **not** suppression. §17.4.66 puts it
    /// with the omitted case — *"If the conflicting table cell border is none
    /// (no border), then the opposing border shall be displayed."*
    ///
    /// `none` never reaches the resolver as its own state; it arrives as
    /// `Absent` because `convert_cell_border_override` maps it to "no override".
    /// This test pins the consequence at the level the resolver sees.
    #[test]
    fn an_absent_edge_never_suppresses() {
        let border = line(1.0, TableBorderStyle::Single, BLACK);
        assert_eq!(
            resolve_border_conflict(CellEdge::Absent, CellEdge::Line(border)),
            CellEdge::Line(border),
            "absent (which is what `none` becomes) must yield, not suppress"
        );
    }

    /// Identical borders resolve to themselves — the reflexive case, which a
    /// comparison built on `partial_cmp` of `f32` could get wrong.
    #[test]
    fn identical_borders_resolve_to_themselves() {
        for b in sample_borders() {
            let r = resolve_border_conflict(CellEdge::Line(b), CellEdge::Line(b))
                .line()
                .expect("some");
            assert_eq!((r.width, r.style, r.color), (b.width, b.style, b.color));
        }
    }
}

/// §17.4.38 edge mapping: which of the six table-level borders each cell edge
/// draws from, given the cell's position in the grid.
#[cfg(test)]
mod edge_mapping_tests {
    use super::*;
    use crate::render::geometry::PtEdgeInsets;
    use crate::render::layout::table::CellVAlign;
    use crate::render::resolve::color::RgbColor;

    /// Every edge gets its own width, so a resolved border names the config
    /// field it came from.
    const TOP: f32 = 1.0;
    const BOTTOM: f32 = 2.0;
    const LEFT: f32 = 3.0;
    const RIGHT: f32 = 4.0;
    const INSIDE_H: f32 = 5.0;
    const INSIDE_V: f32 = 6.0;

    fn edge(width: f32) -> Option<TableBorderLine> {
        Some(TableBorderLine {
            width: Pt::new(width),
            color: RgbColor::BLACK,
            style: TableBorderStyle::Single,
        })
    }

    fn config() -> TableBorderConfig {
        TableBorderConfig {
            top: edge(TOP),
            bottom: edge(BOTTOM),
            left: edge(LEFT),
            right: edge(RIGHT),
            inside_h: edge(INSIDE_H),
            inside_v: edge(INSIDE_V),
        }
    }

    fn plain_cell() -> TableCellInput {
        TableCellInput {
            blocks: vec![],
            margins: PtEdgeInsets::ZERO,
            grid_span: 1,
            shading: None,
            cell_borders: None,
            vertical_merge: None,
            vertical_align: CellVAlign::Top,
        }
    }

    /// `(top, bottom, left, right)` widths, so a failure reads as which edges
    /// were mis-mapped rather than as four separate assertions.
    fn widths(
        row_idx: usize,
        grid_col: usize,
        num_rows: usize,
        num_grid_cols: usize,
    ) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
        let (t, b, l, r) = resolve_cell_effective_borders(
            &plain_cell(),
            Some(&config()),
            row_idx,
            grid_col,
            1,
            num_rows,
            num_grid_cols,
            false,
        );
        let w = |e: CellEdge| e.line().map(|e| e.width.raw());
        (w(t), w(b), w(l), w(r))
    }

    /// The same corner cell, in a table whose `w:tblCellSpacing` is non-zero.
    fn spaced_edges(
        row_idx: usize,
        grid_col: usize,
        num_rows: usize,
        num_grid_cols: usize,
    ) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
        let (t, b, l, r) = resolve_cell_effective_borders(
            &plain_cell(),
            Some(&config()),
            row_idx,
            grid_col,
            1,
            num_rows,
            num_grid_cols,
            true,
        );
        let w = |e: CellEdge| e.line().map(|e| e.width.raw());
        (w(t), w(b), w(l), w(r))
    }

    /// Issue #168. A spaced cell is inset from the table's boundary, so the
    /// table's own borders must not be painted on it — `emit_table_outline`
    /// draws them at the table's bounds instead, and seeding them here too
    /// would paint each one twice.
    #[test]
    fn a_spaced_cell_takes_no_outer_border_from_the_table() {
        // Top-left corner of a 2x2 table: outer on top and left, interior on
        // bottom and right.
        let (t, b, l, r) = spaced_edges(0, 0, 2, 2);
        assert_eq!(
            t, None,
            "top is the table's own edge and belongs to the outline"
        );
        assert_eq!(
            l, None,
            "left is the table's own edge and belongs to the outline"
        );
        assert_eq!(
            (b, r),
            (Some(INSIDE_H), Some(INSIDE_V)),
            "interior edges are untouched by spacing — see the comment in \
             resolve_cell_effective_borders for why that question is left open"
        );
    }

    /// And the unspaced mapping is exactly as it was, which is what every
    /// document in the corpus depends on.
    #[test]
    fn an_unspaced_cell_still_takes_the_tables_outer_borders() {
        assert_eq!(
            widths(0, 0, 2, 2),
            (Some(TOP), Some(INSIDE_H), Some(LEFT), Some(INSIDE_V))
        );
    }

    /// A 3×3 grid: the corners take the outer borders, the middle takes
    /// `insideH`/`insideV` on all four sides.
    #[test]
    fn outer_edges_use_outer_borders_and_interior_edges_use_inside() {
        assert_eq!(
            widths(0, 0, 3, 3),
            (Some(TOP), Some(INSIDE_H), Some(LEFT), Some(INSIDE_V)),
            "top-left cell"
        );
        assert_eq!(
            widths(1, 1, 3, 3),
            (
                Some(INSIDE_H),
                Some(INSIDE_H),
                Some(INSIDE_V),
                Some(INSIDE_V)
            ),
            "centre cell"
        );
        assert_eq!(
            widths(2, 2, 3, 3),
            (Some(INSIDE_H), Some(BOTTOM), Some(INSIDE_V), Some(RIGHT)),
            "bottom-right cell"
        );
    }

    /// A single-row, single-column table is both first and last on both axes,
    /// so it takes all four outer borders and neither inside border.
    #[test]
    fn a_one_cell_table_takes_all_four_outer_borders() {
        assert_eq!(
            widths(0, 0, 1, 1),
            (Some(TOP), Some(BOTTOM), Some(LEFT), Some(RIGHT))
        );
    }

    /// E5b#7. `num_rows == 0` is unreachable through `layout_table` — it returns
    /// early on empty input, and every other caller is inside a row loop — but
    /// `num_rows` is a free parameter of a `pub(super)` function, so the
    /// last-row test must not depend on a caller having checked it.
    /// `row_idx == num_rows - 1` underflows here; `row_idx + 1 == num_rows`
    /// answers "no row is the last row of an empty table".
    #[test]
    fn an_empty_table_does_not_underflow_the_last_row_check() {
        assert_eq!(
            widths(0, 0, 0, 3),
            (Some(TOP), Some(INSIDE_H), Some(LEFT), Some(INSIDE_V))
        );
    }
}
