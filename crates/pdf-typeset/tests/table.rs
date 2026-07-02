//! TS-4 table primitives: fixed/auto grid measure, fair-share shrink,
//! per-edge borders (4 line ops, never a stroked rect), cell fills, row
//! pagination (rows never split).

mod common;

use common::*;
use pdf_typeset::{
    Block, BorderEdge, CellBorders, ColumnWidth, Op, PageGeom, ParaProps, Rgb, Run, TableCell,
    TableRow, TableSpec,
};

fn cell(text: &str, size: f64) -> TableCell {
    let mut c = TableCell::new(vec![para(text, size)]);
    c.padding = 4.0;
    c
}

#[test]
fn fixed_columns_place_cell_text_at_grid_positions() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let spec = TableSpec::new(
        vec![ColumnWidth::Fixed(100.0), ColumnWidth::Fixed(150.0)],
        vec![TableRow::new(vec![cell("aa", 12.0), cell("bb", 12.0)])],
    );
    let (_, result) = export(&[Block::Table(spec)], geom);
    let ws = words(&result.pdf, 0);
    let aa = ws.iter().find(|w| w.4 == "aa").expect("aa");
    let bb = ws.iter().find(|w| w.4 == "bb").expect("bb");
    assert_near(aa.0, 50.0 + 4.0, 0.5, "cell 0 content x");
    assert_near(bb.0, 50.0 + 100.0 + 4.0, 0.5, "cell 1 content x");
}

#[test]
fn auto_column_width_follows_measured_content() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let spec = TableSpec::new(
        vec![ColumnWidth::Auto, ColumnWidth::Auto],
        vec![TableRow::new(vec![
            cell("wide content", 12.0),
            cell("x", 12.0),
        ])],
    );
    let (_, result) = export(&[Block::Table(spec)], geom);
    let ws = words(&result.pdf, 0);
    let wide = ws.iter().find(|w| w.4 == "wide").expect("wide");
    let content = ws.iter().find(|w| w.4 == "content").expect("content");
    let x = ws.iter().find(|w| w.4 == "x").expect("x");
    // Column 0 preference = natural text width + 2 × padding.
    let text_w = content.2 - wide.0;
    assert_near(
        x.0,
        50.0 + (text_w + 8.0) + 4.0,
        0.6,
        "auto column 1 starts after measured column 0",
    );
}

#[test]
fn fair_share_shrink_keeps_the_grid_inside_the_column() {
    let geom = PageGeom::new(300.0, 500.0, 50.0); // content width 200
    let long = "unbreakable_word_that_wants_lots_of_space and more and more";
    let spec = TableSpec::new(
        vec![ColumnWidth::Auto, ColumnWidth::Auto, ColumnWidth::Auto],
        vec![TableRow::new(vec![
            cell(long, 12.0),
            cell(long, 12.0),
            cell("tiny", 12.0),
        ])],
    );
    let (_, result) = export(&[Block::Table(spec)], geom);
    for w in words(&result.pdf, 0) {
        assert!(
            w.2 <= 250.0 + 0.5,
            "word {:?} leaks past the content column (x1 = {})",
            w.4,
            w.2
        );
    }
}

#[test]
fn per_edge_borders_paint_as_individual_lines() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let edge = BorderEdge {
        width: 1.0,
        color: Rgb::BLACK,
    };
    let mut c = cell("b", 12.0);
    c.borders = CellBorders {
        top: Some(edge),
        left: Some(edge),
        right: None,
        bottom: None,
    };
    let spec = TableSpec::new(
        vec![ColumnWidth::Fixed(120.0)],
        vec![TableRow::new(vec![c])],
    );
    let (pages, _) = export(&[Block::Table(spec)], geom);
    let ops = &pages[0].ops;
    assert!(
        !ops.iter().any(|op| matches!(op, Op::StrokeRect { .. })),
        "borders must not use StrokeRect"
    );
    let lines: Vec<(f64, f64, f64, f64)> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Line { x1, y1, x2, y2, .. } => Some((*x1, *y1, *x2, *y2)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2, "exactly the two requested edges");
    assert!(
        lines
            .iter()
            .any(|(x1, y1, x2, y2)| y1 == y2 && (*x2 - *x1 - 120.0).abs() < 1e-6),
        "horizontal top edge spans the cell: {lines:?}"
    );
    assert!(
        lines.iter().any(|(x1, _, x2, _)| x1 == x2),
        "vertical left edge: {lines:?}"
    );
}

#[test]
fn cell_fill_paints_behind_content() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut c = cell("filled", 12.0);
    c.fill = Some(Rgb::new(0.9, 0.9, 0.2));
    let spec = TableSpec::new(
        vec![ColumnWidth::Fixed(120.0)],
        vec![TableRow::new(vec![c])],
    );
    let (pages, _) = export(&[Block::Table(spec)], geom);
    let ops = &pages[0].ops;
    let fill_at = ops
        .iter()
        .position(
            |op| matches!(op, Op::FillRect { color, .. } if *color == Rgb::new(0.9, 0.9, 0.2)),
        )
        .expect("fill rect");
    let text_at = ops
        .iter()
        .position(|op| matches!(op, Op::Text { .. }))
        .expect("text");
    assert!(fill_at < text_at, "fill paints before the cell content");
    match &ops[fill_at] {
        Op::FillRect { x, w, .. } => {
            assert_near(*x, 50.0, 1e-6, "fill spans the cell");
            assert_near(*w, 120.0, 1e-6, "fill width");
        }
        _ => unreachable!(),
    }
}

#[test]
fn rows_paginate_without_splitting() {
    let geom = PageGeom::new(300.0, 200.0, 40.0);
    let rows: Vec<TableRow> = (0..12)
        .map(|i| {
            TableRow::new(vec![cell(&format!("left{i} more words here"), 12.0), {
                cell(&format!("right{i}"), 12.0)
            }])
        })
        .collect();
    let spec = TableSpec::new(vec![ColumnWidth::Auto, ColumnWidth::Auto], rows);
    let (_, result) = export(&[Block::Table(spec)], geom);
    let doc = open(&result.pdf);
    assert!(doc.page_count() > 1, "rows must flow to further pages");
    for i in 0..12 {
        let (mut left_page, mut right_page) = (None, None);
        for p in 0..doc.page_count() {
            for w in words(&result.pdf, p) {
                if w.4 == format!("left{i}") {
                    left_page = Some(p);
                }
                if w.4 == format!("right{i}") {
                    right_page = Some(p);
                }
            }
        }
        assert_eq!(
            left_page.expect("left cell present"),
            right_page.expect("right cell present"),
            "row {i} must not split across pages"
        );
    }
}

#[test]
fn min_row_height_grows_the_row() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut row1 = TableRow::new(vec![cell("one", 12.0)]);
    row1.min_height = Some(100.0);
    let row2 = TableRow::new(vec![cell("two", 12.0)]);
    let spec = TableSpec::new(vec![ColumnWidth::Fixed(120.0)], vec![row1, row2]);
    let (_, result) = export(&[Block::Table(spec)], geom);
    let ws = words(&result.pdf, 0);
    let one = ws.iter().find(|w| w.4 == "one").expect("one");
    let two = ws.iter().find(|w| w.4 == "two").expect("two");
    assert!(
        two.1 >= one.1 - (one.3 - one.1) + 100.0 - 1.0,
        "second row starts below the 100 pt first row (y0 {} vs {})",
        two.1,
        one.1
    );
}

#[test]
fn nested_tables_lay_out_inside_cells() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let inner = TableSpec::new(
        vec![ColumnWidth::Fixed(60.0)],
        vec![TableRow::new(vec![cell("inner", 10.0)])],
    );
    let outer_cell = TableCell::new(vec![
        Block::Paragraph(ParaProps::new(), vec![Run::new("outer", style(12.0))]),
        Block::Table(inner),
    ]);
    let spec = TableSpec::new(
        vec![ColumnWidth::Fixed(160.0)],
        vec![TableRow::new(vec![outer_cell])],
    );
    let (_, result) = export(&[Block::Table(spec)], geom);
    let toks = tokens(&result.pdf);
    assert!(toks.contains(&"outer".to_string()));
    assert!(toks.contains(&"inner".to_string()));
}
