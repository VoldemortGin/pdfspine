//! M7 table tests — `Lines` strategy: detect a ruled table from vector rulings,
//! extract its cell text, and render Markdown. Catalog IDs: `TABLES-LINES-*`.

mod common;

use std::sync::Arc;

use pdf_core::geom::Rect;
use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::Limits;
use pdf_text::tables::{drawings_to_device, find_tables, Strategy, TableOptions};
use pdf_text::{build_textpage, interpret_page, page_transform, words};

use common::{winansi_type1, PageDoc};

fn page_handle(doc: pdf_core::DocumentStore) -> Page {
    Page::new(Arc::new(doc), 0, ObjRef::new(3, 0))
}

/// A WinAnsi font with width 500 for every printable code from space (32) up to
/// 'z' (122) — deterministic advances for the cell text.
fn wide_font() -> pdf_core::Object {
    let widths: Vec<i64> = std::iter::once(250)
        .chain(std::iter::repeat_n(500, 122 - 32))
        .collect();
    winansi_type1("Helvetica", 32, &widths)
}

/// Builds a 2-row x 3-col ruled table in PDF user space (y-up) on a 612x792 page.
///
/// Grid lines (user space): x in {100, 200, 300, 400}, y in {700, 670, 640}.
/// So rows top→bottom are y 700..670 and 670..640; cols are 3 spans of 100 wide.
/// Cell text is placed near each cell's baseline.
fn ruled_table_content() -> Vec<u8> {
    let mut c = String::new();
    // --- ruling lines (stroke) ---
    c.push_str("1 w\n");
    // Horizontal lines at y = 700, 670, 640 from x=100 to x=400.
    for y in [700, 670, 640] {
        c.push_str(&format!("100 {y} m 400 {y} l S\n"));
    }
    // Vertical lines at x = 100, 200, 300, 400 from y=640 to y=700.
    for x in [100, 200, 300, 400] {
        c.push_str(&format!("{x} 640 m {x} 700 l S\n"));
    }
    // --- cell text ---
    // Row 0 baseline ~685, row 1 baseline ~655. Column left edges at 110/210/310.
    c.push_str("BT /F1 10 Tf\n");
    let cells = [
        // (x, y, text)
        (110, 685, "A1"),
        (210, 685, "B1"),
        (310, 685, "C1"),
        (110, 655, "A2"),
        (210, 655, "B2"),
        (310, 655, "C2"),
    ];
    for (x, y, t) in cells {
        c.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj\n"));
    }
    c.push_str("ET\n");
    c.into_bytes()
}

/// Builds the table page and returns (textpage, words, device-space drawings).
fn build_table_page() -> (
    pdf_text::TextPage,
    Vec<pdf_text::Word>,
    Vec<pdf_text::DrawPath>,
) {
    let (doc, page_dict) = PageDoc::new()
        .font("F1", wide_font())
        .content(&ruled_table_content())
        .open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());
    let ws = words(&tp);
    let res = interpret_page(page.document(), &page_dict);
    let pt = page_transform(page.mediabox(), page.rotation());
    let dev = drawings_to_device(&res.drawings, &pt);
    (tp, ws, dev)
}

#[test]
fn tables_lines_001_detects_single_2x3_table() {
    let (tp, ws, dev) = build_table_page();
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    assert_eq!(finder.len(), 1, "expected exactly one table");
    let table = &finder.tables[0];
    assert_eq!(table.row_count, 2, "expected 2 rows");
    assert_eq!(table.col_count, 3, "expected 3 cols");
    // bbox should span the outer grid lines (device space; y flipped).
    assert!(table.bbox.width() > 290.0 && table.bbox.width() < 310.0);
    assert!(table.bbox.height() > 55.0 && table.bbox.height() < 65.0);
}

#[test]
fn tables_lines_002_extract_returns_cell_strings_in_order() {
    let (tp, ws, dev) = build_table_page();
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    let table = &finder.tables[0];
    let grid = table.extract(&ws);
    assert_eq!(grid.len(), 2);
    assert_eq!(grid[0].len(), 3);
    let texts: Vec<Vec<Option<&str>>> = grid
        .iter()
        .map(|r| r.iter().map(|c| c.as_deref()).collect())
        .collect();
    assert_eq!(
        texts,
        vec![
            vec![Some("A1"), Some("B1"), Some("C1")],
            vec![Some("A2"), Some("B2"), Some("C2")],
        ]
    );
}

#[test]
fn tables_lines_003_to_markdown_shape() {
    let (tp, ws, dev) = build_table_page();
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    let md = finder.tables[0].to_markdown(&ws);
    let lines: Vec<&str> = md.lines().collect();
    // header + separator + 1 body row = 3 lines (first grid row is header).
    assert_eq!(lines.len(), 3, "markdown has header+sep+1 body line: {md}");
    assert_eq!(lines[0], "| A1 | B1 | C1 |");
    assert_eq!(lines[1], "| --- | --- | --- |");
    assert_eq!(lines[2], "| A2 | B2 | C2 |");
}

#[test]
fn tables_lines_004_cells_are_within_bbox() {
    let (tp, ws, dev) = build_table_page();
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    let table = &finder.tables[0];
    for row in &table.cells {
        for cell in row {
            let r: Rect = cell.expect("every cell of a full grid is present");
            assert!(table.bbox.contains_rect(&r), "cell {r:?} outside bbox");
        }
    }
    // cols/rows line counts: col_count+1 vertical lines, row_count+1 horizontal.
    assert_eq!(table.cols.len(), 4);
    assert_eq!(table.rows.len(), 3);
}

#[test]
fn tables_lines_006_header_is_first_row() {
    let (tp, ws, dev) = build_table_page();
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    let header: Vec<Option<&str>> = finder.tables[0]
        .header
        .iter()
        .map(|c| c.as_deref())
        .collect();
    assert_eq!(header, vec![Some("A1"), Some("B1"), Some("C1")]);
}

#[test]
fn tables_lines_005_strict_also_detects() {
    let (tp, ws, dev) = build_table_page();
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    assert_eq!(finder.len(), 1);
    assert_eq!(finder.tables[0].row_count, 2);
    assert_eq!(finder.tables[0].col_count, 3);
}
