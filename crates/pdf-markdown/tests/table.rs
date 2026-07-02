//! `MD-TABLE-*` — GFM tables: borders, cell order, wrapping, alignment.

mod common;

use common::{assert_in_order, full_text, page_count, raw, render};

const SIMPLE: &str = "\
| Name | Role |
| ---- | ---- |
| Ada | Engineer |
| Grace | Admiral |
";

#[test]
fn table_cells_extract_row_major() {
    let bytes = render(SIMPLE);
    assert_eq!(page_count(&bytes), 1);
    assert_in_order(
        &full_text(&bytes),
        &["Name", "Role", "Ada", "Engineer", "Grace", "Admiral"],
    );
}

#[test]
fn table_draws_cell_borders_and_header_background() {
    let bytes = render(SIMPLE);
    let raw = raw(&bytes);
    // 3 rows × 2 cells = 6 stroked cell rects.
    assert!(
        raw.matches("re S").count() >= 6,
        "expected 6 cell borders:\n{raw}"
    );
    assert!(
        raw.contains("0.92 0.92 0.92 rg"),
        "header background missing"
    );
}

#[test]
fn long_cell_content_wraps_within_the_cell() {
    let long = "verylongtoken ".repeat(20);
    let md = format!("| A | B |\n| - | - |\n| {long} | tiny |\n");
    let bytes = render(&md);
    let text = full_text(&bytes);
    assert_eq!(
        text.matches("verylongtoken").count(),
        20,
        "cell wrapping must not drop content"
    );
    assert!(text.contains("tiny"));
}

#[test]
fn header_cells_render_bold() {
    let bytes = render(SIMPLE);
    assert!(
        raw(&bytes).contains("/Helvetica-Bold"),
        "table header must use the bold face"
    );
}

#[test]
fn many_rows_paginate_without_losing_cells() {
    let mut md = String::from("| N | Square |\n| - | - |\n");
    for i in 0..80 {
        md.push_str(&format!("| n{i} | {} |\n", i * i));
    }
    let bytes = render(&md);
    assert!(page_count(&bytes) > 1, "80 rows must span pages");
    let text = full_text(&bytes);
    assert!(text.contains("n0"));
    assert!(text.contains("n79"), "last row lost across pages");
}

#[test]
fn ragged_rows_pad_to_the_header_width() {
    // Body row with fewer cells than the header must not panic or drop cells.
    let md = "| A | B | C |\n| - | - | - |\n| only |\n";
    let bytes = render(md);
    assert_in_order(&full_text(&bytes), &["A", "B", "C", "only"]);
}
