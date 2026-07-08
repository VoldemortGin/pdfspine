//! Table layout primitives (TS-4, PRD §10 scope (e)): grid measure (fixed +
//! auto columns with the proven fair-share shrink), cell block layout through
//! the shared box core, and per-edge border painting (4 [`Op::Line`]s per
//! cell instead of a stroked rect).
//!
//! Rows never split across pages; a row taller than a whole page overflows
//! its page (the documented pdf-markdown limitation, kept).

use crate::flow::{layout_box_content, natural_width, tokens, Ctx};
use crate::model::{Block, CellBorders, ColumnWidth, TableRow, TableSpec};
use crate::ops::{translate_ops, Op};
use crate::Typesetter;

/// Narrowest a column may shrink during fair-share distribution, in points
/// (a fixed column narrower than this keeps its requested width).
const MIN_COL_WIDTH: f64 = 12.0;

/// Lays out a table at the context cursor: measured grid, per-row cell
/// layout, fills → content → borders paint order.
pub(crate) fn layout_table(ctx: &mut Ctx, spec: &TableSpec) {
    if spec.columns.is_empty() || spec.rows.is_empty() {
        return;
    }
    ctx.flush_gap();
    let avail = (ctx.right() - ctx.left()).max(1.0);
    let widths = column_widths(ctx.ts, spec, avail);
    let left = ctx.left();
    for row in &spec.rows {
        layout_row(ctx, row, &widths, left);
    }
}

/// Resolves the column grid: fixed widths as requested, auto widths measured
/// from the widest natural cell line; when the preferred total overflows the
/// available width, columns whose preference fits their fair share keep it and
/// oversized columns split the remainder (deterministic, index-stable).
pub(crate) fn column_widths(ts: &mut Typesetter, spec: &TableSpec, avail: f64) -> Vec<f64> {
    let ncols = spec.columns.len();
    let mut pref: Vec<f64> = spec
        .columns
        .iter()
        .map(|c| match c {
            ColumnWidth::Fixed(w) => {
                if w.is_finite() && *w > 0.0 {
                    *w
                } else {
                    MIN_COL_WIDTH
                }
            }
            ColumnWidth::Auto => MIN_COL_WIDTH,
        })
        .collect();
    for row in &spec.rows {
        for (c, cell) in row.cells.iter().enumerate().take(ncols) {
            if matches!(spec.columns[c], ColumnWidth::Auto) {
                let w = natural_blocks_width(ts, &cell.blocks) + 2.0 * cell.padding.max(0.0);
                pref[c] = pref[c].max(w);
            }
        }
    }
    let total: f64 = pref.iter().sum();
    if !(total.is_finite() && total > avail) {
        return pref;
    }
    // Fair-share shrink (pdf-markdown `layout.rs` policy, generalized): process
    // ascending by preference so under-share columns release their slack.
    let mut order: Vec<usize> = (0..ncols).collect();
    order.sort_by(|&a, &b| {
        pref[a]
            .partial_cmp(&pref[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut widths = vec![0.0; ncols];
    let mut remaining = avail;
    let mut cols_left = ncols;
    for &c in &order {
        let share = remaining / cols_left as f64;
        let floor = pref[c].min(MIN_COL_WIDTH);
        let w = pref[c].min(share).max(floor);
        widths[c] = w;
        remaining = (remaining - w).max(0.0);
        cols_left -= 1;
    }
    widths
}

/// The widest natural (soft-unbreakable) line of a block list — auto-column
/// measurement. Nested tables measure recursively at their own preference.
fn natural_blocks_width(ts: &mut Typesetter, blocks: &[Block]) -> f64 {
    let mut w = 0.0f64;
    for block in blocks {
        let bw = match block {
            Block::Paragraph(props, runs) => {
                let toks = tokens(ts, runs);
                let tab_interval = ts.tab_interval();
                natural_width(&toks, tab_interval)
                    + props.indent_left.max(0.0)
                    + props.indent_right.max(0.0)
                    + props.first_line_indent.max(0.0)
            }
            Block::Table(nested) => column_widths(ts, nested, f64::INFINITY).iter().sum(),
            Block::Image(img) => {
                if img.width.is_finite() {
                    img.width.max(0.0)
                } else {
                    0.0
                }
            }
            Block::PageBreak => 0.0,
        };
        w = w.max(bw);
    }
    w
}

/// Lays out one row: every cell's blocks through the shared box core, row
/// height = tallest cell (≥ `min_height`), then fills, offset content and
/// per-edge borders.
fn layout_row(ctx: &mut Ctx, row: &TableRow, widths: &[f64], left: f64) {
    let ncols = widths.len();
    let mut laid: Vec<(Vec<Op>, f64)> = Vec::with_capacity(ncols);
    for (c, width) in widths.iter().enumerate() {
        match row.cells.get(c) {
            Some(cell) => {
                let pad = cell.padding.max(0.0);
                let inner = (width - 2.0 * pad).max(1.0);
                let (ops, h, _) = layout_box_content(ctx.ts, &cell.blocks, inner, true);
                laid.push((ops, h + 2.0 * pad));
            }
            None => laid.push((Vec::new(), 0.0)),
        }
    }
    let mut row_h = row.min_height.unwrap_or(0.0).max(0.0);
    for (_, h) in &laid {
        row_h = row_h.max(*h);
    }
    ctx.ensure(row_h);
    let y0 = ctx.y;
    let mut x = left;
    for (c, (mut ops, _)) in laid.into_iter().enumerate() {
        let w = widths[c];
        if let Some(cell) = row.cells.get(c) {
            let pad = cell.padding.max(0.0);
            if let Some(fill) = cell.fill {
                ctx.op(Op::FillRect {
                    x,
                    y: y0,
                    w,
                    h: row_h,
                    color: fill,
                });
            }
            translate_ops(&mut ops, x + pad, y0 + pad);
            ctx.extend_ops(ops);
            draw_borders(ctx, &cell.borders, x, y0, w, row_h);
        }
        x += w;
    }
    ctx.y += row_h;
    ctx.max_x = ctx.max_x.max(x);
}

/// Paints a cell's per-edge borders as 4 independent line ops (`None` edges
/// are not painted — the PRD §10 per-edge requirement).
fn draw_borders(ctx: &mut Ctx, borders: &CellBorders, x: f64, y: f64, w: f64, h: f64) {
    let edges = [
        (borders.top, x, y, x + w, y),
        (borders.right, x + w, y, x + w, y + h),
        (borders.bottom, x, y + h, x + w, y + h),
        (borders.left, x, y, x, y + h),
    ];
    for (edge, x1, y1, x2, y2) in edges {
        if let Some(e) = edge {
            if e.width > 0.0 && e.width.is_finite() {
                ctx.op(Op::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: e.color,
                    width: e.width,
                });
            }
        }
    }
}
