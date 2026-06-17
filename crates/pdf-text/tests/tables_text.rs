//! M7 table tests — `Text` strategy: detect a borderless table by clustering
//! word columns/rows from spacing alone. Catalog IDs: `TABLES-TEXT-*`.

mod common;

use std::sync::Arc;

use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::Limits;
use pdf_text::tables::{find_tables, Strategy, TableOptions};
use pdf_text::{build_textpage, words};

use common::{winansi_type1, PageDoc};

fn page_handle(doc: pdf_core::DocumentStore) -> Page {
    Page::new(Arc::new(doc), 0, ObjRef::new(3, 0))
}

fn wide_font() -> pdf_core::Object {
    let widths: Vec<i64> = std::iter::once(250)
        .chain(std::iter::repeat_n(500, 122 - 32))
        .collect();
    winansi_type1("Helvetica", 32, &widths)
}

/// Borderless 2-row x 3-col table: three columns left-aligned at x = 100, 250,
/// 400 (wide x-gaps), two rows 30pt apart. No ruling lines at all.
fn borderless_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("BT /F1 10 Tf\n");
    let cells = [
        (100, 700, "Name"),
        (250, 700, "Age"),
        (400, 700, "City"),
        (100, 670, "Alice"),
        (250, 670, "30"),
        (400, 670, "Paris"),
    ];
    for (x, y, t) in cells {
        c.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj\n"));
    }
    c.push_str("ET\n");
    c.into_bytes()
}

fn build_text_page() -> (pdf_text::TextPage, Vec<pdf_text::Word>) {
    let (doc, _page_dict) = PageDoc::new()
        .font("F1", wide_font())
        .content(&borderless_content())
        .open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());
    let ws = words(&tp);
    (tp, ws)
}

#[test]
fn tables_text_001_detects_grid_from_spacing() {
    let (tp, ws) = build_text_page();
    let finder = find_tables(&tp, &ws, &[], &TableOptions::with_strategy(Strategy::Text));
    assert_eq!(finder.len(), 1, "expected one text-strategy table");
    let table = &finder.tables[0];
    assert_eq!(table.col_count, 3, "three columns from x-gaps");
    assert_eq!(table.row_count, 2, "two rows from y-gaps");
}

#[test]
fn tables_text_002_extract_cell_text() {
    let (tp, ws) = build_text_page();
    let finder = find_tables(&tp, &ws, &[], &TableOptions::with_strategy(Strategy::Text));
    let grid = finder.tables[0].extract(&ws);
    let texts: Vec<Vec<Option<&str>>> = grid
        .iter()
        .map(|r| r.iter().map(|c| c.as_deref()).collect())
        .collect();
    assert_eq!(
        texts,
        vec![
            vec![Some("Name"), Some("Age"), Some("City")],
            vec![Some("Alice"), Some("30"), Some("Paris")],
        ]
    );
}

#[test]
fn tables_text_003_to_markdown_uses_first_row_as_header() {
    let (tp, ws) = build_text_page();
    let finder = find_tables(&tp, &ws, &[], &TableOptions::with_strategy(Strategy::Text));
    let md = finder.tables[0].to_markdown(&ws);
    let lines: Vec<&str> = md.lines().collect();
    assert_eq!(lines.len(), 3, "header+sep+1 row: {md}");
    assert_eq!(lines[0], "| Name | Age | City |");
    assert_eq!(lines[1], "| --- | --- | --- |");
    assert_eq!(lines[2], "| Alice | 30 | Paris |");
}

#[test]
fn tables_text_004_lines_strategy_ignores_borderless_grid() {
    // A borderless (no-ruling) word grid is a `Text`-strategy table, but the
    // default `Lines` strategy detects from vector ruling evidence ONLY and must
    // NOT cluster prose/whitespace into a table — matching PyMuPDF's default,
    // where borderless multi-column text yields no tables. (Previously this
    // strategy fell back to text clustering, which over-detected tables on
    // borderless multi-column prose; that fallback was removed.)
    let (tp, ws) = build_text_page();
    for strat in [Strategy::Lines, Strategy::LinesStrict] {
        let finder = find_tables(&tp, &ws, &[], &TableOptions::with_strategy(strat));
        assert!(
            finder.is_empty(),
            "no rulings → {strat:?} must find no table (got {})",
            finder.len()
        );
    }
}
