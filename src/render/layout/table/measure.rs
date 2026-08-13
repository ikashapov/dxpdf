//! Table measurement phase — cell layout, row heights, cell geometry.
//!
//! Border *resolution* is not here. `borders.rs` answers what each cell's four
//! edges are (§17.4.66, including who owns each shared edge); this phase asks
//! for that answer once, then spends its length on the question it is named
//! for: how tall each row is and where each cell sits.

use crate::render::dimension::Pt;

use crate::render::layout::cell::{layout_cell, CellLayout};

use super::borders::{border_width, resolve_table_cell_borders};
use super::grid::{expand_rows_for_vmerge, is_vmerge_continue};
use super::types::{
    CellLayoutEntry, MeasuredRow, MeasuredTable, RowHeightRule, TableBorderConfig, TableRowInput,
    VerticalMergeState,
};

/// Measure all table rows: lay out cell content, compute row heights and cell
/// positions, carrying each cell's resolved borders (from `borders.rs`) along.
/// This is the shared measurement phase used by both `layout_table` (monolithic)
/// and `layout_table_paginated` (page-splitting).
///
/// §17.4.38: `suppress_first_row_top` — when `true`, the top border of the first
/// row is suppressed. Used for adjacent table border collapse: consecutive tables
/// with the same style are treated as a single merged table, so the second table's
/// top border would duplicate the first table's bottom border.
pub(super) fn measure_table_rows(
    rows: &[TableRowInput],
    col_widths: &[Pt],
    // §17.4.44 `tblCellSpacing`, already resolved to points. Zero for every
    // table that does not set it, which is the overwhelming majority.
    cell_spacing: Pt,
    default_line_height: Pt,
    borders: Option<&TableBorderConfig>,
    measure_text: crate::render::layout::paragraph::MeasureTextFn<'_>,
    suppress_first_row_top: bool,
) -> MeasuredTable {
    // §17.4.44: the slots were shrunk by one `cell_spacing` before they got
    // here, so adding it back recovers the table's own outer width — the
    // spacing is carved out of the table, not added to it.
    //
    // §17.4.44 says the spacing sits "between adjacent cells and the edges of
    // the table" without saying whether an edge gets the same amount as the gap
    // between two cells. **Settled** (issue #165) by a Word render of
    // `test-files/issue-165-cellspacing.docx`: the two are equal — one full
    // spacing everywhere, which is what is implemented here and what HTML's
    // `cellspacing` does. The half-gap reading is out.
    //
    // The same render showed the gaps at twice this engine's width, which is a
    // separate and still-open question — see
    // `build::table::resolve_cell_spacing`, which explains why the answer is
    // probably not a factor (ONLYOFFICE's renderer applies none) and probably is
    // that probe declaring `tblCellSpacing` at both table and row level. The
    // equality above is unaffected either way, being a ratio.
    //
    // Note this is *carved out of* the table rather than added to it, so the
    // cells shrink as the spacing grows and the table keeps its declared
    // `w:tblW`. ONLYOFFICE does the same, insetting each cell within its grid
    // slot. `test-files/issue-165-cellspacing-scale.docx` shows both at once.
    let table_width: Pt = col_widths.iter().copied().sum::<Pt>() + cell_spacing;
    let num_rows = rows.len();
    let mut row_heights = Vec::with_capacity(num_rows);

    // Pass 2a: §17.4.66 border resolution — what each cell declares, then who
    // owns each shared edge. Both live in `borders.rs`; this phase only needs
    // the answer.
    let resolved_borders = resolve_table_cell_borders(
        rows,
        col_widths.len(),
        borders,
        cell_spacing,
        suppress_first_row_top,
    );

    // Pass 2b: lay out each cell.
    let mut row_cell_layouts: Vec<Vec<CellLayoutEntry>> = Vec::new();

    for (row_idx, row) in rows.iter().enumerate() {
        let mut entries = Vec::new();
        let mut max_height = Pt::ZERO;
        // §17.4.17: gridBefore — first cell offset.
        let mut grid_idx = row.grid_before as usize;

        for (cell_ci, cell) in row.cells.iter().enumerate() {
            let span = cell.grid_span.max(1) as usize;
            // Defensive clamp: malformed DOCX where gridBefore + spans + gridAfter
            // exceed the grid would otherwise panic in the slice index below.
            // Both ends need clamping — clamping only `grid_end` inverts the
            // range (`start > end`), which panics just as an out-of-bounds end
            // would. Mirrors the same clamp in `build/table.rs`.
            let grid_start = grid_idx.min(col_widths.len());
            let grid_end = (grid_start + span).min(col_widths.len());
            // §17.4.44: the grid slots were already shrunk so they sum to
            // `table_width - cell_spacing`; offsetting every cell by one
            // spacing and taking one off its width then leaves exactly
            // `cell_spacing` between adjacent cells *and* at both table edges,
            // without changing the table's own width. A `gridSpan` cell
            // absorbs the interior gaps it covers, which is what a merged cell
            // should do.
            let slots: Pt = col_widths[grid_start..grid_end].iter().copied().sum();
            let cell_w: Pt = (slots - cell_spacing).max(Pt::ZERO);
            let cell_x: Pt = col_widths[..grid_start].iter().copied().sum::<Pt>() + cell_spacing;

            let b = &resolved_borders[row_idx][cell_ci];
            let extra_left = (border_width(b.left) - cell.margins.left).max(Pt::ZERO);
            let extra_right = (border_width(b.right) - cell.margins.right).max(Pt::ZERO);
            let layout_w = (cell_w - extra_left - extra_right).max(Pt::ZERO);

            let is_continue = cell.vertical_merge == Some(VerticalMergeState::Continue);
            let layout = if is_continue {
                CellLayout {
                    commands: Vec::new(),
                    content_height: Pt::ZERO,
                    lines: Vec::new(),
                }
            } else {
                layout_cell(
                    &cell.blocks,
                    layout_w,
                    &cell.margins,
                    default_line_height,
                    measure_text,
                )
            };

            // §17.4.85: a merged cell's height is normally decided by
            // `expand_rows_for_vmerge` over the whole span, not here — folding a
            // `Restart` cell's full content into its *first* row would double-count
            // it against the rows below.
            //
            // Unless the span is a span of one. A `Restart` with no `Continue`
            // under it is an ordinary cell, and `expand_rows_for_vmerge` skips it
            // (it returns early when the group is a single row), so if this branch
            // skipped it too the row would get **no** height from any path while
            // still emitting its content — following blocks then draw on top of
            // the table. Word treats a restart with nothing continuing as a plain
            // cell, which is what this reproduces.
            let continues_below =
                row_idx + 1 < num_rows && is_vmerge_continue(&rows[row_idx + 1], grid_idx);
            let is_lone_restart =
                cell.vertical_merge == Some(VerticalMergeState::Restart) && !continues_below;
            if cell.vertical_merge.is_none() || is_lone_restart {
                max_height = max_height.max(layout.content_height + cell.margins.vertical());
            }

            entries.push(CellLayoutEntry {
                layout,
                cell_x,
                cell_w,
                grid_col: grid_idx,
            });
            grid_idx += span;
        }

        match row.height_rule {
            Some(RowHeightRule::AtLeast(min_h)) => max_height = max_height.max(min_h),
            Some(RowHeightRule::Exact(h)) => max_height = h,
            None => {}
        }

        // §17.4.44: the row's box reserves its own leading gap, mirroring the
        // horizontal inset above — `emit_one_row` places content one spacing
        // below the cursor, so consecutive rows end up exactly `cell_spacing`
        // apart. `RowHeightRule` is applied to the *content* height first, so a
        // `trHeight` still means the height of the row's content, not of the
        // content plus a gap the author never asked for.
        row_heights.push(max_height + cell_spacing);
        row_cell_layouts.push(entries);
    }

    // §17.4.85: distribute vMerge overflow.
    expand_rows_for_vmerge(rows, &row_cell_layouts, &mut row_heights);

    // Compute border gaps and assemble measured rows.
    let measured_rows: Vec<MeasuredRow> = row_cell_layouts
        .into_iter()
        .zip(resolved_borders)
        .zip(row_heights.iter())
        .enumerate()
        .map(|(row_idx, ((entries, borders), &height))| {
            // With cell spacing there is no shared edge to reserve room for:
            // every cell draws its own bottom border inside its own box, and
            // the gap between rows is the spacing itself.
            let border_gap_below = if row_idx + 1 < num_rows && cell_spacing <= Pt::ZERO {
                borders
                    .iter()
                    .map(|b| border_width(b.bottom))
                    .fold(Pt::ZERO, Pt::max)
            } else {
                Pt::ZERO
            };
            MeasuredRow {
                entries,
                borders,
                height,
                leading_gap: cell_spacing,
                border_gap_below,
            }
        })
        .collect();

    MeasuredTable {
        rows: measured_rows,
        table_width,
    }
}

#[cfg(test)]
mod tests {
    use super::super::borders::CellEdge;
    use super::super::types::{
        CellBorderConfig, CellBorderOverride, CellVAlign, TableBorderConfig, TableBorderLine,
        TableBorderStyle, TableCellInput, TableRowInput,
    };
    use super::measure_table_rows;
    use crate::render::dimension::Pt;
    use crate::render::geometry::PtEdgeInsets;
    use crate::render::resolve::color::RgbColor;

    fn single(w: f32) -> TableBorderLine {
        TableBorderLine {
            width: Pt::new(w),
            color: RgbColor::BLACK,
            style: TableBorderStyle::Single,
        }
    }

    /// A table style like `Tabellenraster`: every side plus insideH/insideV.
    fn all_single() -> TableBorderConfig {
        let s = single(0.5);
        TableBorderConfig {
            top: Some(s),
            bottom: Some(s),
            left: Some(s),
            right: Some(s),
            inside_h: Some(s),
            inside_v: Some(s),
        }
    }

    fn cb(top: Option<CellBorderOverride>, bottom: Option<CellBorderOverride>) -> CellBorderConfig {
        CellBorderConfig {
            top,
            bottom,
            left: None,
            right: None,
        }
    }

    fn cell(span: u32, borders: Option<CellBorderConfig>) -> TableCellInput {
        TableCellInput {
            blocks: vec![],
            margins: PtEdgeInsets::ZERO,
            grid_span: span,
            shading: None,
            cell_borders: borders,
            vertical_merge: None,
            vertical_align: CellVAlign::Top,
        }
    }

    fn row(cells: Vec<TableCellInput>) -> TableRowInput {
        TableRowInput {
            cells,
            height_rule: None,
            is_header: None,
            cant_split: None,
            grid_before: 0,
            border_overrides: None,
        }
    }

    /// [MS-OI29500] §17.4.66 regression: a `gridSpan` upper cell facing several
    /// lower cells must not drop the later cells' top borders (previously only
    /// the first lower cell was resolved and the rest nulled), and the whole
    /// shared edge must be drawn from a single side so the line does not split
    /// across two y positions. Mirrors the real doc's
    /// `[spacer | Function: | Qualitätssicherung]` row under a `gridSpan` header.
    ///
    /// Non-uniformity comes from a **heavier border on one column**, not from a
    /// `nil`. It used to come from a nil, which worked only while nil resolved
    /// to "absent": now that nil suppresses (§17.4.66), a nil under the wide
    /// cell makes its run uniformly suppressed and the upper row owns the edge —
    /// the opposite branch, so the configuration no longer reaches the bug this
    /// test exists for. Suppression is covered separately by
    /// `nil_suppresses_across_a_gridspan_mismatch`.
    #[test]
    fn wide_upper_cell_draws_whole_edge_from_lower_row() {
        let s = single(0.5);
        let heavy = single(2.0);
        let rows = vec![
            // Row 0: gridSpan=2 header over spacer+Function, then two single
            // cells over the Qualitätssicherung span. All bottoms inherit.
            row(vec![cell(2, None), cell(1, None), cell(1, None)]),
            // Row 1: [spacer | Function (heavy top) | Q (gridSpan=2)]. The heavy
            // top on column 1 alone makes the wide upper cell's run non-uniform.
            row(vec![
                cell(1, None),
                cell(1, Some(cb(Some(CellBorderOverride::Border(heavy)), None))),
                cell(2, None),
            ]),
        ];
        let cols = vec![Pt::new(100.0); 4];
        let m = measure_table_rows(
            &rows,
            &cols,
            Pt::ZERO,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );

        // Whole edge drawn from the lower row → every upper bottom cleared,
        // so Function and Qualitätssicherung tops share one y position.
        for b in &m.rows[0].borders {
            assert_eq!(
                b.bottom,
                CellEdge::Absent,
                "upper bottoms cleared (edge owned below)"
            );
        }
        assert_eq!(
            m.rows[1].borders[0].top.line(),
            Some(s),
            "spacer column keeps the inherited insideH"
        );
        assert_eq!(
            m.rows[1].borders[1].top.line(),
            Some(heavy),
            "Function keeps its heavier top border across the gridSpan mismatch"
        );
        assert_eq!(
            m.rows[1].borders[2].top.line(),
            Some(s),
            "Qualitätssicherung top drawn from the same (lower) side as Function"
        );
    }

    /// §17.4.66 step 0 across a `gridSpan` mismatch: a `nil` bottom on a wide
    /// upper cell suppresses the columns it spans that fall back to the table —
    /// but **not** the column where the lower cell has a border of its own,
    /// which survives and is drawn from below. One nil bottom therefore resolves to two different
    /// answers along its own width, which is exactly the case that forces the
    /// per-column resolution this pass does.
    ///
    /// This is the configuration `wide_upper_cell_draws_whole_edge_from_lower_row`
    /// used to carry, kept here for what it now demonstrates.
    #[test]
    fn nil_suppresses_across_a_gridspan_mismatch() {
        let s = single(0.5);
        let rows = vec![
            row(vec![
                cell(2, Some(cb(None, Some(CellBorderOverride::Suppress)))),
                cell(1, None),
                cell(1, None),
            ]),
            row(vec![
                cell(1, Some(cb(Some(CellBorderOverride::Suppress), None))),
                cell(1, Some(cb(Some(CellBorderOverride::Border(s)), None))),
                cell(2, None),
            ]),
        ];
        let cols = vec![Pt::new(100.0); 4];
        let m = measure_table_rows(
            &rows,
            &cols,
            Pt::ZERO,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );

        // The nil bottom spans columns 0-1 and resolves differently on each:
        // column 0 faces another nil and stays suppressed, column 1 faces
        // Function's *declared* top and loses to it. A cell paints one border
        // across its width, so that split hands the whole edge to the lower row.
        for b in &m.rows[0].borders {
            assert_eq!(
                b.bottom,
                CellEdge::Absent,
                "upper bottoms cleared — the nil span is not uniform, so the edge is owned below"
            );
        }
        assert_eq!(
            m.rows[1].borders[0].top,
            CellEdge::Suppressed,
            "column 0: nil against nil stays suppressed"
        );
        assert_eq!(
            m.rows[1].borders[1].top.line(),
            Some(s),
            "column 1: Function has a top of its own, so the nil above does not erase it"
        );
        // Columns outside the nil span keep the inherited insideH, drawn from
        // the same (lower) side so the line sits at one y.
        assert_eq!(m.rows[1].borders[2].top.line(), Some(s));
    }

    /// A cell can paint one border across its whole span when every column under
    /// it *paints the same thing* — not when every column resolved to the same
    /// `CellEdge`. `Absent` and `Suppressed` both paint nothing, so a run
    /// mixing them is uniform; comparing with `==` would split it and hand the
    /// edge to the other row for no visible reason.
    ///
    /// Built so the two columns differ **only** in that: with no `insideH` to
    /// inherit, column 0 resolves to `Suppressed` (the lower cell wrote `nil`)
    /// and column 1 to `Absent` (nobody said anything). Neither paints.
    ///
    /// The upper cell must end up owning the edge, and carrying `Suppressed`
    /// while doing so — `emit.rs` restores an `Absent` top at a page split and
    /// must not restore an emptied one, so which of the two lands there is a
    /// real distinction, not bookkeeping.
    #[test]
    fn a_uniform_run_is_not_split_by_absent_versus_suppressed() {
        let hair = single(0.5);
        let borders = TableBorderConfig {
            top: Some(hair),
            bottom: Some(hair),
            left: Some(hair),
            right: Some(hair),
            // Nothing to inherit on the inter-row edge — so the only states in
            // play there are `Absent` and `Suppressed`.
            inside_h: None,
            inside_v: Some(hair),
        };
        let rows = vec![
            // One wide cell saying nothing about its bottom.
            row(vec![cell(2, None)]),
            // Column 0 writes `nil`; column 1 says nothing.
            row(vec![
                cell(1, Some(cb(Some(CellBorderOverride::Suppress), None))),
                cell(1, None),
            ]),
        ];
        let cols = vec![Pt::new(100.0), Pt::new(100.0)];
        let m = measure_table_rows(
            &rows,
            &cols,
            Pt::ZERO,
            Pt::new(10.0),
            Some(&borders),
            None,
            false,
        );

        assert_eq!(
            m.rows[0].borders[0].bottom,
            CellEdge::Suppressed,
            "the wide cell owns the whole edge, carrying the suppression"
        );
        assert_eq!(
            m.rows[1].borders[0].top,
            CellEdge::Absent,
            "so the lower row's tops are cleared"
        );
        assert_eq!(m.rows[1].borders[1].top, CellEdge::Absent);
    }

    /// §17.4.66: a `nil` among the cells on one side of an edge **cannot** punch
    /// a gap through a wide `gridSpan` cell facing it. The wide cell inherits
    /// `insideH` for its whole width and paints one border across it; a
    /// neighbour's `nil` empties only that neighbour's own edge.
    ///
    /// This is `IP 05 Trenches`' `Date/Time:` cell with the sides swapped — the
    /// real document has the wide spacer cell *below*, its `nil` aimed at the
    /// narrow spacer column, and the label cell above still draws the bottom it
    /// inherited. The assertion here was inverted twice: once when `nil`
    /// wrongly collapsed to "absent" (so it also wrongly *inherited*), and once
    /// when `nil` was made to win the conflict outright. Declining inheritance
    /// and overruling the neighbour are different powers; `nil` has only the
    /// first.
    #[test]
    fn nil_spacer_cannot_punch_a_gap_through_a_wide_facing_cell() {
        let s = single(0.5);
        let rows = vec![
            // Row 0: [inherits single | nil spacer | inherits single].
            row(vec![
                cell(1, None),
                cell(1, Some(cb(None, Some(CellBorderOverride::Suppress)))),
                cell(1, None),
            ]),
            // Row 1: one gridSpan=3 cell inheriting insideH as its top.
            row(vec![cell(3, None)]),
        ];
        let cols = vec![Pt::new(100.0), Pt::new(100.0), Pt::new(100.0)];
        let m = measure_table_rows(
            &rows,
            &cols,
            Pt::ZERO,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );

        // Every column resolves to the same line, so the edge is uniform and one
        // side paints it whole. The nil column is not a hole in it.
        assert_eq!(
            m.rows[0].borders[1].bottom.line(),
            Some(s),
            "the wide cell below still supplies this column's border"
        );
        assert_eq!(m.rows[0].borders[0].bottom.line(), Some(s));
        assert_eq!(m.rows[0].borders[2].bottom.line(), Some(s));
        // …and it is drawn exactly once, from the upper row.
        assert_eq!(m.rows[1].borders[0].top.line(), None);
    }

    /// [MS-OI29500] §17.4.66 regression: an upper `gridSpan` cell that leaves the last
    /// column uncovered (its gridAfter gap) must not "own" the edge, or a
    /// lower cell straddling that boundary would draw its own top over the
    /// upper bottom → a doubled line. Mirrors the real doc's `gridSpan=9`
    /// section row above the `Observations` (`gridSpan=2`) header.
    #[test]
    fn upper_grid_after_gap_yields_edge_to_lower_row() {
        let s = single(0.5);
        let rows = vec![
            // Row 0: one gridSpan=2 cell over cols 0-1; col 2 is its gridAfter.
            row(vec![cell(2, None)]),
            // Row 1: [cell | gridSpan=2 cell straddling covered col 1 + col 2].
            row(vec![cell(1, None), cell(2, None)]),
        ];
        let cols = vec![Pt::new(100.0); 3];
        let m = measure_table_rows(
            &rows,
            &cols,
            Pt::ZERO,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );

        // Upper can't cover col 2, so the lower row owns the whole edge:
        // its bottom is cleared (no doubling), the lower tops carry the line.
        assert_eq!(
            m.rows[0].borders[0].bottom.line(),
            None,
            "upper bottom cleared so the straddling lower cell isn't doubled"
        );
        assert_eq!(m.rows[1].borders[0].top.line(), Some(s));
        assert_eq!(m.rows[1].borders[1].top.line(), Some(s));
    }

    /// Aligned grids keep the pre-existing "upper cell owns the shared edge"
    /// behaviour: the lower cell's top is cleared, the upper bottom carries it.
    #[test]
    fn aligned_grid_upper_cell_owns_horizontal_edge() {
        let s = single(0.5);
        let rows = vec![
            row(vec![cell(1, None), cell(1, None)]),
            row(vec![cell(1, None), cell(1, None)]),
        ];
        let cols = vec![Pt::new(100.0), Pt::new(100.0)];
        let m = measure_table_rows(
            &rows,
            &cols,
            Pt::ZERO,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );
        for ci in 0..2 {
            assert_eq!(m.rows[0].borders[ci].bottom.line(), Some(s));
            assert_eq!(m.rows[1].borders[ci].top.line(), None);
        }
    }

    /// §17.4.44 geometry. The invariant that matters is a *uniform* gap: exactly
    /// one `cell_spacing` between adjacent cells **and** at both table edges,
    /// with the table's own width unchanged.
    ///
    /// Slots are pre-shrunk by `reserve_cell_spacing` (build side), so this
    /// feeds slots summing to `width - spacing` and checks the resulting edges.
    #[test]
    fn cell_spacing_leaves_a_uniform_gap_and_keeps_the_table_width() {
        let spacing = Pt::new(10.0);
        // Table is 100pt wide; slots therefore sum to 90.
        let slots = vec![Pt::new(45.0), Pt::new(45.0)];
        let rows = vec![row(vec![cell(1, None), cell(1, None)])];
        let m = measure_table_rows(
            &rows,
            &slots,
            spacing,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );

        assert_eq!(
            m.table_width,
            Pt::new(100.0),
            "spacing comes out of the table"
        );

        let e = &m.rows[0].entries;
        let left_edge = e[0].cell_x;
        let gap_between = e[1].cell_x - (e[0].cell_x + e[0].cell_w);
        let right_edge = m.table_width - (e[1].cell_x + e[1].cell_w);
        assert_eq!(left_edge, spacing, "gap at the table's left edge");
        assert_eq!(gap_between, spacing, "gap between adjacent cells");
        assert_eq!(right_edge, spacing, "gap at the table's right edge");
    }

    /// A `gridSpan` cell absorbs the interior gaps it covers — it is one cell,
    /// so the spacing between the columns it spans belongs to it.
    #[test]
    fn a_gridspan_cell_absorbs_the_gaps_it_covers() {
        let spacing = Pt::new(10.0);
        let slots = vec![Pt::new(30.0); 3]; // 90 + 10 spacing = 100 wide
        let rows = vec![
            row(vec![cell(3, None)]),
            row(vec![cell(1, None), cell(1, None), cell(1, None)]),
        ];
        let m = measure_table_rows(
            &rows,
            &slots,
            spacing,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );

        let wide = &m.rows[0].entries[0];
        let narrow = &m.rows[1].entries;
        assert_eq!(wide.cell_x, spacing);
        assert_eq!(wide.cell_w, Pt::new(80.0), "spans 90 of slot minus one gap");
        // The wide cell covers both interior gaps plus the two narrow cells
        // between them, so its right edge matches the last narrow cell's.
        assert_eq!(
            wide.cell_x + wide.cell_w,
            narrow[2].cell_x + narrow[2].cell_w
        );
    }

    /// Vertically the rule is the same: one spacing between row boxes, and one
    /// above the first row. The row's own height carries its leading gap.
    #[test]
    fn cell_spacing_separates_rows_vertically() {
        let spacing = Pt::new(6.0);
        let slots = vec![Pt::new(94.0)];
        let rows = vec![row(vec![cell(1, None)]), row(vec![cell(1, None)])];
        let m = measure_table_rows(
            &rows,
            &slots,
            spacing,
            Pt::new(10.0),
            Some(&all_single()),
            None,
            false,
        );

        for r in &m.rows {
            assert_eq!(r.leading_gap, spacing);
            assert_eq!(
                r.border_gap_below,
                Pt::ZERO,
                "no shared edge to reserve for once cells are separated"
            );
        }
        // These cells are empty, so the whole of each row's height is the
        // reserved gap — added exactly once per row, not once per cell.
        for r in &m.rows {
            assert_eq!(r.height, spacing);
        }
    }

    /// [MS-OI29500] §17.4.66: *"If the cell spacing is nonzero ... then all cell
    /// borders and outer table borders display."* Separated cells share no edge,
    /// so conflict resolution is skipped and **both** sides keep their border —
    /// where collapsing would have kept one and cleared the other.
    #[test]
    fn nonzero_cell_spacing_disables_border_collapsing() {
        let slots = vec![Pt::new(45.0), Pt::new(45.0)];
        let rows = vec![
            row(vec![cell(1, None), cell(1, None)]),
            row(vec![cell(1, None), cell(1, None)]),
        ];
        let measure = |spacing: Pt| {
            measure_table_rows(
                &rows,
                &slots,
                spacing,
                Pt::new(10.0),
                Some(&all_single()),
                None,
                false,
            )
        };

        // Collapsed (no spacing): the right/left pair resolves to one border on
        // the left cell, and the shared horizontal edge is owned by one row.
        let collapsed = measure(Pt::ZERO);
        assert!(collapsed.rows[0].borders[1].left.line().is_none());
        assert!(collapsed.rows[1].borders[0].top.line().is_none());

        // Spaced: no edge is resolved away. This is what the test is about —
        // where collapsing kept one side of a shared edge and cleared the
        // other, both sides now survive.
        let spaced = measure(Pt::new(8.0));
        assert!(spaced.rows[0].borders[1].left.line().is_some(), "left kept");
        assert!(
            spaced.rows[0].borders[0].right.line().is_some(),
            "right kept"
        );
        assert!(spaced.rows[1].borders[0].top.line().is_some(), "top kept");
        assert!(
            spaced.rows[0].borders[0].bottom.line().is_some(),
            "bottom kept"
        );

        // Issue #168: the *outer* edges of a spaced table are no longer drawn
        // on the cells. They are the table's own border, and a cell inset by
        // the spacing is the wrong place for it — `emit_table_outline` paints
        // it at the table's bounds instead. This assertion used to read "every
        // cell keeps all four of its own borders", which was true of the
        // implementation but said more than the §17.4.66 sentence above does:
        // the sentence distinguishes *cell* borders from *outer table* borders
        // precisely so both can display, each in its own place.
        assert!(spaced.rows[0].borders[0].top.line().is_none());
        assert!(spaced.rows[0].borders[0].left.line().is_none());
        assert!(spaced.rows[1].borders[1].bottom.line().is_none());
        assert!(spaced.rows[1].borders[1].right.line().is_none());
    }
}
