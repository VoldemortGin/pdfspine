//! M7 table tests — `to_html` high-fidelity output + merged/spanning cell
//! detection (colspan/rowspan). Catalog IDs: `TABLES-HTML-*`, `TABLES-SPAN-*`,
//! `TABLES-HTML-WELLFORMED-*`.

mod common;

use std::sync::Arc;

use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::Limits;
use pdf_text::tables::{drawings_to_device, find_tables, Strategy, TableOptions};
use pdf_text::{build_textpage, interpret_page, page_transform, words};

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

fn build(
    content: &[u8],
) -> (
    pdf_text::TextPage,
    Vec<pdf_text::Word>,
    Vec<pdf_text::DrawPath>,
) {
    let (doc, page_dict) = PageDoc::new()
        .font("F1", wide_font())
        .content(content)
        .open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());
    let ws = words(&tp);
    let res = interpret_page(page.document(), &page_dict);
    let pt = page_transform(page.mediabox(), page.rotation());
    let dev = drawings_to_device(&res.drawings, &pt);
    (tp, ws, dev)
}

/// A simple structural check that every start tag has a matching end tag and
/// tags nest correctly (a minimal well-formedness / balance verifier for the
/// `<table>` subset we emit). Self-closing `<br>`/`<br/>` are ignored.
fn assert_well_formed(html: &str) {
    let mut stack: Vec<String> = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Find the end of the tag.
            let end = html[i..].find('>').map(|o| i + o).unwrap_or(bytes.len());
            let inner = &html[i + 1..end];
            let inner_trim = inner.trim();
            if let Some(rest) = inner_trim.strip_prefix('/') {
                // Closing tag.
                let name = rest.trim().to_ascii_lowercase();
                let top = stack
                    .pop()
                    .unwrap_or_else(|| panic!("closing </{name}> with empty stack in:\n{html}"));
                assert_eq!(
                    top, name,
                    "mismatched close </{name}> (open <{top}>)\n{html}"
                );
            } else if inner_trim.ends_with('/') {
                // Self-closing, e.g. <br/>. Nothing to push.
            } else {
                // Opening tag: take the tag name (up to first whitespace).
                let name = inner_trim
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if name == "br" {
                    // void element, no close expected
                } else {
                    stack.push(name);
                }
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    assert!(stack.is_empty(), "unclosed tags {stack:?} in:\n{html}");
}

// === TABLES-HTML-* — regular (no-span) tables =============================

/// Reuses the ruled 2x3 table from the lines suite.
fn ruled_table_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 w\n");
    for y in [700, 670, 640] {
        c.push_str(&format!("100 {y} m 400 {y} l S\n"));
    }
    for x in [100, 200, 300, 400] {
        c.push_str(&format!("{x} 640 m {x} 700 l S\n"));
    }
    c.push_str("BT /F1 10 Tf\n");
    let cells = [
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

#[test]
fn tables_html_001_regular_table_one_td_per_cell() {
    let (tp, ws, dev) = build(&ruled_table_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    let html = finder.tables[0].to_html(&ws);
    assert_well_formed(&html);
    // Header row uses <th>; body uses <td>.
    assert_eq!(html.matches("<th").count(), 3, "3 header cells: {html}");
    assert_eq!(html.matches("<td").count(), 3, "3 body cells: {html}");
    // Two <tr> rows total.
    assert_eq!(html.matches("<tr>").count(), 2, "2 rows: {html}");
    // Cell text present.
    for t in ["A1", "B1", "C1", "A2", "B2", "C2"] {
        assert!(html.contains(t), "missing cell {t}: {html}");
    }
    // No colspan/rowspan on a regular grid.
    assert!(
        !html.contains("colspan"),
        "no colspan on regular grid: {html}"
    );
    assert!(
        !html.contains("rowspan"),
        "no rowspan on regular grid: {html}"
    );
}

#[test]
fn tables_html_002_escapes_special_chars() {
    // A borderless text table whose cells carry HTML metacharacters.
    let mut c = String::new();
    c.push_str("BT /F1 10 Tf\n");
    let cells = [
        (100, 700, "a<b"),
        (250, 700, "x&y"),
        (100, 670, "p>q"),
        (250, 670, "1\"2"),
    ];
    for (x, y, t) in cells {
        // Escape the literal parens-free strings into the content stream.
        c.push_str(&format!("1 0 0 1 {x} {y} Tm ({t}) Tj\n"));
    }
    c.push_str("ET\n");
    let (tp, ws, _dev) = build(c.as_bytes());
    let finder = find_tables(&tp, &ws, &[], &TableOptions::with_strategy(Strategy::Text));
    let html = finder.tables[0].to_html(&ws);
    assert_well_formed(&html);
    assert!(html.contains("a&lt;b"), "< escaped: {html}");
    assert!(html.contains("x&amp;y"), "& escaped: {html}");
    assert!(html.contains("p&gt;q"), "> escaped: {html}");
    // Raw metacharacters must not survive in element content.
    assert!(!html.contains("a<b"), "raw < leaked: {html}");
    assert!(!html.contains("x&y"), "raw & leaked: {html}");
}

#[test]
fn tables_html_003_multiline_cell_uses_br() {
    // A 2-col, single visible row table where the left cell has two text lines
    // (two words stacked vertically inside one cell). Build a ruled 1x2 grid
    // tall enough to hold two lines in the left cell.
    let mut c = String::new();
    c.push_str("1 w\n");
    // Horizontal rules at y = 700 (top) and y = 640 (bottom).
    for y in [700, 640] {
        c.push_str(&format!("100 {y} m 300 {y} l S\n"));
    }
    // Vertical rules at x = 100, 200, 300.
    for x in [100, 200, 300] {
        c.push_str(&format!("{x} 640 m {x} 700 l S\n"));
    }
    c.push_str("BT /F1 10 Tf\n");
    // Left cell: two stacked lines "Line1" (y=688) and "Line2" (y=655).
    c.push_str("1 0 0 1 110 688 Tm (Line1) Tj\n");
    c.push_str("1 0 0 1 110 655 Tm (Line2) Tj\n");
    // Right cell: single line.
    c.push_str("1 0 0 1 210 670 Tm (Solo) Tj\n");
    c.push_str("ET\n");
    let (tp, ws, dev) = build(c.as_bytes());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    let table = &finder.tables[0];
    let html = table.to_html(&ws);
    assert_well_formed(&html);
    assert!(
        html.contains("Line1<br>Line2") || html.contains("Line1<br/>Line2"),
        "multi-line cell should join lines with <br>: {html}"
    );
}

#[test]
fn tables_html_004_wellformed_balanced_tags() {
    let (tp, ws, dev) = build(&ruled_table_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    let html = finder.tables[0].to_html(&ws);
    assert_well_formed(&html);
    assert!(html.starts_with("<table"), "starts with <table>: {html}");
    assert!(
        html.trim_end().ends_with("</table>"),
        "ends with </table>: {html}"
    );
}

// === TABLES-SPAN-LINES-* — merged header spanning all columns =============

/// A ruled table whose top row is one merged header cell: there are NO internal
/// vertical rules within row 0 (only the outer left/right verticals reach the
/// top). Rows 1+ have the full 3-column grid.
///
/// Grid lines (user space): x in {100,200,300,400}, y in {700,670,640}.
/// Row 0 = y 700..670 (single merged cell). Row 1 = y 670..640 (3 cells).
fn merged_header_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 w\n");
    // Horizontal rules: full width at all three y levels.
    for y in [700, 670, 640] {
        c.push_str(&format!("100 {y} m 400 {y} l S\n"));
    }
    // Outer verticals run the full height (640..700).
    for x in [100, 400] {
        c.push_str(&format!("{x} 640 m {x} 700 l S\n"));
    }
    // Interior verticals (200,300) ONLY exist in the body row (640..670), not
    // the header row → so row 0 is one merged cell spanning all 3 columns.
    for x in [200, 300] {
        c.push_str(&format!("{x} 640 m {x} 670 l S\n"));
    }
    c.push_str("BT /F1 10 Tf\n");
    // Header text centered-ish in the merged top cell.
    c.push_str("1 0 0 1 110 685 Tm (Header) Tj\n");
    // Body row cells.
    c.push_str("1 0 0 1 110 655 Tm (A2) Tj\n");
    c.push_str("1 0 0 1 210 655 Tm (B2) Tj\n");
    c.push_str("1 0 0 1 310 655 Tm (C2) Tj\n");
    c.push_str("ET\n");
    c.into_bytes()
}

#[test]
fn tables_span_lines_001_merged_header_colspan() {
    let (tp, ws, dev) = build(&merged_header_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    assert_eq!(finder.len(), 1, "one table");
    let table = &finder.tables[0];
    assert_eq!(table.col_count, 3, "3 columns");
    assert_eq!(table.row_count, 2, "2 rows");
    // The origin cell at (0,0) must span all 3 columns.
    let origin = table.span_at(0, 0).expect("origin span at (0,0)");
    assert_eq!(origin.col_span, 3, "header spans all columns");
    assert_eq!(origin.row_span, 1, "header is one row tall");
    // (0,1) and (0,2) are continuation slots, not origins.
    assert!(table.span_at(0, 1).is_none(), "(0,1) is a continuation");
    assert!(table.span_at(0, 2).is_none(), "(0,2) is a continuation");
}

#[test]
fn tables_span_lines_002_html_emits_colspan_once() {
    let (tp, ws, dev) = build(&merged_header_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    let html = finder.tables[0].to_html(&ws);
    assert_well_formed(&html);
    // The merged header is emitted exactly once with colspan=3.
    assert_eq!(
        html.matches("colspan=\"3\"").count(),
        1,
        "one colspan=3: {html}"
    );
    assert!(html.contains("Header"), "header text present: {html}");
    // Header row should have a single cell element (th), body row three (td).
    // Count <th in header (1) + <td in body (3) = total cell tags 4.
    let cells = html.matches("<th").count() + html.matches("<td").count();
    assert_eq!(cells, 4, "1 merged header + 3 body cells: {html}");
}

// === TABLES-SPAN-ROW-* — cell spanning two rows ===========================

/// A 3-row x 2-col ruled table where the left column's top two cells are merged
/// into one cell spanning rows 0 and 1 (the horizontal rule that would split
/// them is missing on the left column only).
///
/// Grid lines: x in {100,200,300}, y in {700,660,620,580}.
/// Left col: merged cell over y 700..620 (rows 0+1); a normal cell at row 2.
/// Right col: three normal cells.
fn rowspan_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 w\n");
    // Top and bottom full-width horizontals.
    for y in [700, 620, 580] {
        c.push_str(&format!("100 {y} m 300 {y} l S\n"));
    }
    // The y=660 horizontal exists ONLY on the right column (200..300), so the
    // left column's rows 0 and 1 are merged.
    c.push_str("200 660 m 300 660 l S\n");
    // Verticals full height.
    for x in [100, 200, 300] {
        c.push_str(&format!("{x} 580 m {x} 700 l S\n"));
    }
    c.push_str("BT /F1 10 Tf\n");
    // Left merged cell text.
    c.push_str("1 0 0 1 110 650 Tm (L) Tj\n");
    // Right column three cells.
    c.push_str("1 0 0 1 210 685 Tm (R0) Tj\n");
    c.push_str("1 0 0 1 210 645 Tm (R1) Tj\n");
    c.push_str("1 0 0 1 110 600 Tm (L2) Tj\n");
    c.push_str("1 0 0 1 210 600 Tm (R2) Tj\n");
    c.push_str("ET\n");
    c.into_bytes()
}

#[test]
fn tables_span_row_001_rowspan_detected() {
    let (tp, ws, dev) = build(&rowspan_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    assert_eq!(finder.len(), 1, "one table");
    let table = &finder.tables[0];
    assert_eq!(table.row_count, 3, "3 rows");
    assert_eq!(table.col_count, 2, "2 cols");
    let origin = table.span_at(0, 0).expect("origin at (0,0)");
    assert_eq!(origin.row_span, 2, "left cell spans rows 0+1");
    assert_eq!(origin.col_span, 1, "left cell is one column wide");
    // (1,0) is a continuation of the rowspan.
    assert!(table.span_at(1, 0).is_none(), "(1,0) is continuation");
    // Right column cells are all normal origins.
    assert_eq!(table.span_at(0, 1).expect("(0,1)").row_span, 1);
    assert_eq!(table.span_at(1, 1).expect("(1,1)").row_span, 1);
}

#[test]
fn tables_span_row_002_html_emits_rowspan_once() {
    let (tp, ws, dev) = build(&rowspan_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    let html = finder.tables[0].to_html(&ws);
    assert_well_formed(&html);
    assert_eq!(
        html.matches("rowspan=\"2\"").count(),
        1,
        "one rowspan=2: {html}"
    );
    assert!(
        html.contains(">L<") || html.contains(">L "),
        "left text present: {html}"
    );
}

// === degradation of extract()/to_markdown() on spanned tables =============

#[test]
fn tables_span_003_markdown_still_valid_gfm() {
    let (tp, ws, dev) = build(&merged_header_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    let md = finder.tables[0].to_markdown(&ws);
    let lines: Vec<&str> = md.lines().collect();
    // header + separator + 1 body row.
    assert_eq!(lines.len(), 3, "valid GFM shape: {md}");
    // Every row has the same pipe count (col_count + 1 = 4 pipes for 3 cols).
    for l in &lines {
        assert_eq!(l.matches('|').count(), 4, "consistent column count: {l}");
    }
    // Header text appears in the originating (first) cell.
    assert!(lines[0].contains("Header"), "header text in row: {md}");
}

#[test]
fn tables_span_004_extract_origin_holds_text_continuation_none() {
    let (tp, ws, dev) = build(&merged_header_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    let grid = finder.tables[0].extract(&ws);
    assert_eq!(grid.len(), 2);
    assert_eq!(grid[0].len(), 3);
    // Origin slot holds the header text; continuation slots are None.
    assert_eq!(grid[0][0].as_deref(), Some("Header"));
    assert_eq!(grid[0][1], None, "continuation slot None");
    assert_eq!(grid[0][2], None, "continuation slot None");
    // Body row intact.
    assert_eq!(grid[1][0].as_deref(), Some("A2"));
    assert_eq!(grid[1][1].as_deref(), Some("B2"));
    assert_eq!(grid[1][2].as_deref(), Some("C2"));
}
