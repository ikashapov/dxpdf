//! Row splitting for page pagination.
//!
//! §17.4.1: a row without `cantSplit` may have its content broken across
//! page boundaries. This module derives safe cut points from a row's
//! already-laid-out cell commands and partitions those commands into a
//! first slice (stays on the current page) and a second slice (flows to
//! the next page). Both halves preserve the cell's top/bottom margins so
//! the text doesn't collide with the cut-edge border.

use crate::render::dimension::Pt;
use crate::render::layout::draw_command::DrawCommand;
use crate::render::layout::section::CellLine;

use super::borders::CellBorders;
use super::types::{CellLayoutEntry, MeasuredRow, TableRowInput};

/// Options for finding a row cut.
pub(super) struct RowCutInput<'a> {
    pub(super) mr: &'a MeasuredRow,
    pub(super) row: &'a TableRowInput,
    /// Space available for the row on the current page, measured from the
    /// row's top edge.
    pub(super) available: Pt,
}

/// Per-cell split decision: where to partition the commands and how far
/// to shift the tail commands so the continuation half has the cell's
/// natural top margin.
struct CellCut {
    /// Partition threshold: commands with primary-Y strictly less than
    /// this value stay on the first half (cell-box coordinates).
    content_cut_y: Pt,
    /// Partition threshold for the [`CellLine`] cut model, in cell-content
    /// coordinates (i.e. `content_cut_y - margin_top`): lines whose box top is
    /// strictly less stay on the first half.
    line_cut_y: Pt,
    /// Amount by which second-half commands (and continuation line tops) are
    /// shifted up so that the first surviving line lands at the continuation
    /// cell's natural top margin.
    shift: Pt,
}

impl CellCut {
    /// "Don't split" sentinel — all commands and lines stay on the first half.
    fn keep_all() -> Self {
        Self {
            content_cut_y: Pt::new(f32::INFINITY),
            line_cut_y: Pt::new(f32::INFINITY),
            shift: Pt::ZERO,
        }
    }
}

/// Aggregate split decision for a whole row.
pub(super) struct SplitCut {
    /// Row-level first-half height (max across cells of per-cell first
    /// half heights). Each cell's visible box in the first half extends
    /// from the row top to this Y.
    first_half_height: Pt,
    /// One per cell, in the same order as `row.cells`.
    cells: Vec<CellCut>,
}

/// Pick a row cut that fits in `available`, honoring each cell's top and
/// bottom margins so the cut-edge border gets the natural padding Word
/// preserves on split cells.
///
/// Returns `None` when no cell has at least one line that fits within
/// `available - margins.vertical()`.
pub(super) fn find_row_cut(input: &RowCutInput<'_>) -> Option<SplitCut> {
    let mut cells: Vec<CellCut> = Vec::with_capacity(input.row.cells.len());
    let mut first_half_height = Pt::ZERO;
    let mut any_fits = false;
    // Cells that can't be split text-wise (too few baselines or
    // image/shape-only cells) force the row to leave room for their full
    // visible height on the first half; otherwise their content (image,
    // path, etc.) would overflow the cut-edge border.
    let mut non_splittable_heights: Vec<Pt> = Vec::with_capacity(input.row.cells.len());

    for (entry, cell) in input.mr.entries.iter().zip(&input.row.cells) {
        match cut_for_cell(
            entry,
            cell.margins.top,
            cell.margins.bottom,
            input.available,
        ) {
            Some((cut, half_h)) => {
                any_fits = true;
                if half_h > first_half_height {
                    first_half_height = half_h;
                }
                cells.push(cut);
                non_splittable_heights.push(Pt::ZERO);
            }
            None => {
                cells.push(CellCut::keep_all());
                // Cell's natural visible height (content + vertical margins)
                // must fit on the first half — we can't cut through an
                // image or shape.
                let required = entry.layout.content_height + cell.margins.top + cell.margins.bottom;
                non_splittable_heights.push(required);
            }
        }
    }

    if !any_fits {
        return None;
    }

    // Raise first_half_height so every non-splittable cell fits. If that
    // exceeds `available`, splitting isn't safe — caller should spill the
    // whole row to the next page.
    let non_splittable_max = non_splittable_heights
        .iter()
        .copied()
        .fold(Pt::ZERO, Pt::max);
    if non_splittable_max > first_half_height {
        first_half_height = non_splittable_max;
    }
    if first_half_height > input.available {
        return None;
    }
    Some(SplitCut {
        first_half_height,
        cells,
    })
}

/// For a single cell, choose the largest legal prefix of lines that fits in
/// `available`, honoring the cell paragraphs' §17.3.1.14 keepLines,
/// §17.3.1.44 widow/orphan control, and §17.3.1.15 keepNext — the same policy
/// body across-page splitting applies. Returns the `CellCut` and the first-half
/// height this cell needs: the retained content plus the cell's top and bottom
/// margins, so the cut edge gets the natural padding Word preserves (variant 2
/// of the OOXML split-edge options; §17.4.40 re-applied at the cut edge).
///
/// Returns `None` when the cell exposes no legal cut point within `available`
/// (empty/one-line, image/shape-only, keepLines, or every fitting cut would
/// strand a widow) — the caller then keeps the cell whole on the first half.
fn cut_for_cell(
    entry: &CellLayoutEntry,
    margin_top: Pt,
    margin_bottom: Pt,
    available: Pt,
) -> Option<(CellCut, Pt)> {
    // Space available for the content between both margins, since the first
    // half reproduces the cell's natural layout (top margin, retained lines,
    // bottom margin) — same as a non-split cell rendering those lines.
    let budget = available - margin_top - margin_bottom;
    if budget <= Pt::ZERO {
        return None;
    }

    let cont_top = largest_legal_cut(&entry.layout.lines, budget)?;
    if cont_top <= Pt::ZERO {
        return None;
    }

    // `cont_top` is the box top of the first continuation line, in cell-content
    // coordinates — equivalently the content height the first half retains.
    // Shifting the continuation up by it lands its first line at the cell's top
    // margin; the partition threshold sits at `margin_top + cont_top` (draw
    // commands are margin-shifted, the cut model is not).
    let shift = cont_top;
    let half_h = cont_top + margin_top + margin_bottom;
    Some((
        CellCut {
            content_cut_y: margin_top + cont_top,
            line_cut_y: cont_top,
            shift,
        },
        half_h,
    ))
}

/// The largest legal cut offset (`cont_top`, cell-content coords) not exceeding
/// `budget`: the most content the first half may retain while leaving a
/// spec-legal split point. A cut "after line L" keeps `lines[0..=L]` and flows
/// the rest to the continuation.
///
/// Legality (§17.3.1.14 / §17.3.1.44 / §17.3.1.15):
/// - **interior** (L and L+1 share a paragraph): illegal if the paragraph is
///   internally atomic (keepLines / bordered / shaded / drop cap); otherwise,
///   under widow control, both sides must keep `>= 2` of the paragraph's lines.
/// - **boundary** (L and L+1 are different paragraphs): legal unless the earlier
///   paragraph is keepNext (bound to the block that follows).
///
/// Line box tops increase monotonically, so the scan stops once a candidate
/// exceeds `budget`.
fn largest_legal_cut(lines: &[CellLine], budget: Pt) -> Option<Pt> {
    let n = lines.len();
    if n < 2 {
        return None;
    }
    let mut best: Option<Pt> = None;
    for l in 0..n - 1 {
        let a = &lines[l];
        let b = &lines[l + 1];
        let cont_top = b.top_y;
        if cont_top > budget {
            break;
        }
        let legal = if a.para == b.para {
            if a.interior_atomic {
                false
            } else if a.widow_control {
                let first = lines.iter().position(|x| x.para == a.para).unwrap_or(l);
                let last = lines.iter().rposition(|x| x.para == a.para).unwrap_or(l);
                let head = l + 1 - first; // this paragraph's lines kept
                let tail = last - l; // this paragraph's lines continued
                head >= 2 && tail >= 2
            } else {
                true
            }
        } else {
            !a.keep_next
        };
        if legal {
            best = Some(best.map_or(cont_top, |x: Pt| x.max(cont_top)));
        }
    }
    best
}

/// A row cut into two halves. Each half is a full `MeasuredRow` ready to
/// pass to `emit_one_row`.
pub(super) struct SplitRow {
    pub(super) first: MeasuredRow,
    pub(super) second: MeasuredRow,
}

/// Split a row using `cut`. Each cell's commands are partitioned at the
/// cell's own `content_cut_y`; the tail is shifted so its first line
/// lands at the cell's natural `margin_top + ascent` in the
/// continuation.
///
/// Borders: Word preserves the full border box on each half. First half
/// keeps all original borders. Second half inherits a top border from
/// the original top if set, otherwise falls back to the original bottom
/// (same inside-horizontal style used by conflict resolution on
/// mid-table rows).
pub(super) fn split_row_at(mr: &MeasuredRow, cut: &SplitCut) -> SplitRow {
    let first_h = cut.first_half_height;
    // The largest shift across cells is the amount of content consumed by
    // the first half. Whatever's left in the original row height is what
    // the continuation needs to render (top/bottom margins are already
    // included in `mr.height`, so no additional padding is needed).
    let max_shift = cut.cells.iter().map(|c| c.shift).fold(Pt::ZERO, Pt::max);
    let second_h = (mr.height - max_shift).max(Pt::ZERO);

    let mut first_entries: Vec<CellLayoutEntry> = Vec::with_capacity(mr.entries.len());
    let mut second_entries: Vec<CellLayoutEntry> = Vec::with_capacity(mr.entries.len());

    for (entry, cc) in mr.entries.iter().zip(cut.cells.iter()) {
        let (first_cmds, second_cmds) =
            partition_commands(&entry.layout.commands, cc.content_cut_y, cc.shift);
        // Partition the cut model too, rebasing the continuation's line tops by
        // the same shift, so a further split of the continuation (mod.rs's
        // iterative loop) re-evaluates §17.3.1.44 widow control against the
        // lines that actually remain.
        let (first_lines, second_lines) =
            partition_lines(&entry.layout.lines, cc.line_cut_y, cc.shift);
        first_entries.push(CellLayoutEntry {
            layout: crate::render::layout::cell::CellLayout {
                commands: first_cmds,
                content_height: entry.layout.content_height.min(first_h),
                lines: first_lines,
            },
            cell_x: entry.cell_x,
            cell_w: entry.cell_w,
            grid_col: entry.grid_col,
        });
        second_entries.push(CellLayoutEntry {
            layout: crate::render::layout::cell::CellLayout {
                commands: second_cmds,
                content_height: (entry.layout.content_height - cc.shift).max(Pt::ZERO),
                lines: second_lines,
            },
            cell_x: entry.cell_x,
            cell_w: entry.cell_w,
            grid_col: entry.grid_col,
        });
    }

    let first_borders: Vec<CellBorders> = mr.borders.to_vec();
    let second_borders: Vec<CellBorders> = mr
        .borders
        .iter()
        .map(|b| CellBorders {
            top: b.top.or(b.bottom),
            bottom: b.bottom,
            left: b.left,
            right: b.right,
        })
        .collect();

    SplitRow {
        first: MeasuredRow {
            entries: first_entries,
            borders: first_borders,
            height: first_h,
            border_gap_below: Pt::ZERO,
        },
        second: MeasuredRow {
            entries: second_entries,
            borders: second_borders,
            height: second_h,
            border_gap_below: mr.border_gap_below,
        },
    }
}

/// Split a command list at `cut_y`. Commands whose primary Y < `cut_y`
/// go to the first half; the rest land in the second half shifted up by
/// `shift` so they start at the continuation cell's natural top margin.
fn partition_commands(
    commands: &[DrawCommand],
    cut_y: Pt,
    shift: Pt,
) -> (Vec<DrawCommand>, Vec<DrawCommand>) {
    let mut first = Vec::new();
    let mut second = Vec::new();
    for cmd in commands {
        if command_primary_y(cmd) < cut_y {
            first.push(cmd.clone());
        } else {
            let mut c = cmd.clone();
            c.shift_y(-shift);
            second.push(c);
        }
    }
    (first, second)
}

/// Partition the [`CellLine`] cut model at `cut_y` (cell-content coords).
/// Lines whose box top is `< cut_y` stay on the first half; the rest form the
/// continuation, with their tops rebased up by `shift` so the model tracks the
/// continuation's own coordinates for any further split.
fn partition_lines(lines: &[CellLine], cut_y: Pt, shift: Pt) -> (Vec<CellLine>, Vec<CellLine>) {
    let mut first = Vec::new();
    let mut second = Vec::new();
    for line in lines {
        if line.top_y < cut_y {
            first.push(line.clone());
        } else {
            let mut l = line.clone();
            l.top_y -= shift;
            second.push(l);
        }
    }
    (first, second)
}

/// The Y used to decide which side of the cut a command belongs to. For
/// `Text` we use the baseline; for rect/line/image we use the top edge.
fn command_primary_y(cmd: &DrawCommand) -> Pt {
    match cmd {
        DrawCommand::Text { position, .. } | DrawCommand::NamedDestination { position, .. } => {
            position.y
        }
        DrawCommand::Underline { line, .. } | DrawCommand::Line { line, .. } => line.start.y,
        DrawCommand::Image { rect, .. }
        | DrawCommand::EmojiCluster { rect, .. }
        | DrawCommand::Rect { rect, .. }
        | DrawCommand::LinkAnnotation { rect, .. }
        | DrawCommand::InternalLink { rect, .. } => rect.origin.y,
        DrawCommand::Path { origin, .. } => origin.y,
    }
}
