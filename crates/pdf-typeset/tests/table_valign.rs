//! TS-11 table cell vertical anchoring acceptance (PRD §10): a cell's content
//! is offset within the (content-driven) row height per `TableCell::v_align`,
//! after the row height is fixed. Asserts the first-baseline position of a
//! short cell inside a tall (min-height) row for Top / Middle / Bottom.

mod common;

use common::*;
use pdf_typeset::{
    Block, ColumnWidth, FixedPages, PageGeom, TableCell, TableRow, TableSpec, VAnchor,
};

/// One-row table, one `Fixed(300)` column, `min_height` = 120 pt forcing a tall
/// row around a single-line cell anchored `anchor`. Returns the read-back first
/// baseline (top-left y) of the cell text.
fn cell_baseline(anchor: VAnchor) -> f64 {
    let mut cell = TableCell::new(vec![para("Anchored", 12.0)]);
    cell.v_align = anchor;
    let mut row = TableRow::new(vec![cell]);
    row.min_height = Some(120.0);
    let table = TableSpec::new(vec![ColumnWidth::Fixed(300.0)], vec![row]);

    let mut engine = ts();
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let pages = engine.layout_flow(&[Block::Table(table)], &mut FixedPages::new(geom));
    let result = engine.emit(&pages).expect("emit");
    let ws = words(&result.pdf, 0);
    assert_eq!(ws.len(), 1, "one word in the cell; got {ws:?}");
    let (_, desc, _) = liberation_metrics();
    ws[0].3 - desc * 12.0 // word bottom minus glyph descent = baseline
}

#[test]
fn cell_v_align_places_content_within_the_row() {
    let y0 = 50.0; // row top = page top margin (table is the first block)
    let row_h = 120.0;
    let content_h = natural_line_height(12.0);
    let (_, desc, _) = liberation_metrics();
    let top_baseline = y0 + content_h - desc * 12.0;

    assert_near(cell_baseline(VAnchor::Top), top_baseline, 1.0, "top anchor");
    assert_near(
        cell_baseline(VAnchor::Middle),
        top_baseline + (row_h - content_h) / 2.0,
        1.0,
        "middle anchor",
    );
    assert_near(
        cell_baseline(VAnchor::Bottom),
        top_baseline + (row_h - content_h),
        1.0,
        "bottom anchor",
    );
}

#[test]
fn top_anchor_is_the_default() {
    // A default cell (no v_align set) anchors like an explicit Top.
    let default = {
        let mut row = TableRow::new(vec![TableCell::new(vec![para("Anchored", 12.0)])]);
        row.min_height = Some(120.0);
        let table = TableSpec::new(vec![ColumnWidth::Fixed(300.0)], vec![row]);
        let mut engine = ts();
        let geom = PageGeom::new(400.0, 500.0, 50.0);
        let pages = engine.layout_flow(&[Block::Table(table)], &mut FixedPages::new(geom));
        let result = engine.emit(&pages).expect("emit");
        let ws = words(&result.pdf, 0);
        let (_, desc, _) = liberation_metrics();
        ws[0].3 - desc * 12.0
    };
    assert_near(default, cell_baseline(VAnchor::Top), 0.01, "default == Top");
}
