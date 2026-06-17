//! M7 table-detection regression tests guarding the "find_tables over-detection"
//! fix (differential vs fitz). The default `Lines`/`LinesStrict` strategies must
//! detect tables from REAL vector ruling evidence only — never from
//! prose/whitespace column structure — and must not over-segment / over-merge
//! the ruled grid. Catalog IDs: `TABLES-REGR-*`.
//!
//! Concretely these pin:
//!  - borderless multi-column prose ⇒ **no** tables under the default strategy
//!    (fitz `lines`/`lines_strict` likewise find nothing on unruled text);
//!  - a single ruled grid ⇒ exactly one table with the correct row/col count;
//!  - two vertically-separated ruled blocks ⇒ **two** distinct tables (no
//!    gap-bridging into one giant grid);
//!  - near-coincident / fragmented rulings ⇒ merged grid lines (no spurious
//!    extra rows/cols).

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

/// A borderless multi-column "prose" page: several columns of left-aligned text
/// laid out in a grid-like fashion but with **no ruling lines at all** (this is
/// the structure that previously triggered hundreds of spurious tables on the
/// GovInfo / NASA corpus). Three text columns, many rows.
fn borderless_prose_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("BT /F1 9 Tf\n");
    let words_per_col = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
    for (row, w) in words_per_col.iter().enumerate() {
        let y = 720 - (row as i32) * 18;
        for (col, x) in [80, 240, 400].into_iter().enumerate() {
            c.push_str(&format!("1 0 0 1 {x} {y} Tm ({w}{col}) Tj\n"));
        }
    }
    c.push_str("ET\n");
    c.into_bytes()
}

#[test]
fn tables_regr_001_borderless_prose_yields_no_tables() {
    // The crux of the over-detection bug: columnar prose without rulings must NOT
    // be detected as a table by the default ruling-based strategies. Only the
    // explicit `Text` strategy clusters whitespace into a grid.
    let (tp, ws, dev) = build(&borderless_prose_content());
    for strat in [Strategy::Lines, Strategy::LinesStrict] {
        let finder = find_tables(&tp, &ws, &dev, &TableOptions::with_strategy(strat));
        assert!(
            finder.is_empty(),
            "borderless prose must yield no tables under {strat:?} (got {})",
            finder.len()
        );
    }
    // The opt-in `Text` strategy may still infer a grid (that is its job); this
    // just documents that the default differs from `Text`.
    let text = find_tables(&tp, &ws, &dev, &TableOptions::with_strategy(Strategy::Text));
    assert!(
        !text.is_empty(),
        "the explicit Text strategy still clusters the columns"
    );
}

/// One fully-ruled 3-row × 4-col grid (user space, y-up): x ∈ {100,160,220,280,340},
/// y ∈ {700,670,640,610}.
fn ruled_grid_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 w\n");
    for y in [700, 670, 640, 610] {
        c.push_str(&format!("100 {y} m 340 {y} l S\n"));
    }
    for x in [100, 160, 220, 280, 340] {
        c.push_str(&format!("{x} 610 m {x} 700 l S\n"));
    }
    c.into_bytes()
}

#[test]
fn tables_regr_002_ruled_grid_one_table_correct_shape() {
    let (tp, ws, dev) = build(&ruled_grid_content());
    for strat in [Strategy::Lines, Strategy::LinesStrict] {
        let finder = find_tables(&tp, &ws, &dev, &TableOptions::with_strategy(strat));
        assert_eq!(finder.len(), 1, "exactly one ruled table ({strat:?})");
        let t = &finder.tables[0];
        assert_eq!(t.row_count, 3, "3 rows ({strat:?})");
        assert_eq!(t.col_count, 4, "4 cols ({strat:?})");
        // Grid-line counts: row_count+1 horizontal, col_count+1 vertical.
        assert_eq!(t.rows.len(), 4);
        assert_eq!(t.cols.len(), 5);
    }
}

/// Two ruled 2×2 grids separated by a vertical gap (no rules in the gap). Block A
/// at y 700..650, block B at y 600..550. The two blocks share NO rules.
fn two_separated_grids_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 w\n");
    // Block A (top): y in {700,675,650}, x in {100,200,300}.
    for y in [700, 675, 650] {
        c.push_str(&format!("100 {y} m 300 {y} l S\n"));
    }
    for x in [100, 200, 300] {
        c.push_str(&format!("{x} 650 m {x} 700 l S\n"));
    }
    // Block B (bottom): y in {600,575,550}, x in {100,200,300}.
    for y in [600, 575, 550] {
        c.push_str(&format!("100 {y} m 300 {y} l S\n"));
    }
    for x in [100, 200, 300] {
        c.push_str(&format!("{x} 550 m {x} 600 l S\n"));
    }
    c.into_bytes()
}

#[test]
fn tables_regr_003_separated_blocks_are_distinct_tables() {
    // Two vertically-separated ruled blocks must yield TWO tables, not one giant
    // grid bridging the gap (the over-merge half of the bug).
    let (tp, ws, dev) = build(&two_separated_grids_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    assert_eq!(finder.len(), 2, "two separate ruled blocks → two tables");
    for t in &finder.tables {
        assert_eq!(t.row_count, 2, "each block is 2×2");
        assert_eq!(t.col_count, 2);
    }
}

/// A 2×2 ruled grid where each ruling is drawn in two collinear pieces with a
/// tiny (< snap) positional jitter, plus a near-duplicate line 1pt away. A
/// correct detector snaps these to a single 2×2 grid rather than over-segmenting.
fn jittered_grid_content() -> Vec<u8> {
    let mut c = String::new();
    c.push_str("1 w\n");
    // Horizontal rules near y = 700, 660, 620, each split into two touching halves
    // and with a duplicate ~1pt away (within snap tolerance).
    for y in [700.0_f64, 660.0, 620.0] {
        c.push_str(&format!("100 {y} m 200 {y} l S\n"));
        c.push_str(&format!("200 {} m 300 {} l S\n", y - 0.4, y - 0.4));
        c.push_str(&format!("100 {} m 300 {} l S\n", y + 0.8, y + 0.8));
    }
    // Vertical rules near x = 100, 200, 300 (full height 620..700).
    for x in [100.0_f64, 200.0, 300.0] {
        c.push_str(&format!("{x} 620 m {x} 700 l S\n"));
        c.push_str(&format!("{} 620 m {} 700 l S\n", x + 0.7, x + 0.7));
    }
    c.into_bytes()
}

#[test]
fn tables_regr_004_jittered_rulings_do_not_over_segment() {
    let (tp, ws, dev) = build(&jittered_grid_content());
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    assert_eq!(finder.len(), 1, "one table from the jittered grid");
    let t = &finder.tables[0];
    // Near-coincident / fragmented rulings collapse to a clean 2×2 grid; they must
    // not invent extra rows/cols.
    assert_eq!(t.row_count, 2, "snapped to 2 rows, not over-segmented");
    assert_eq!(t.col_count, 2, "snapped to 2 cols, not over-segmented");
}
