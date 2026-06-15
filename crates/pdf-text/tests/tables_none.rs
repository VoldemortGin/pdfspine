//! M7 table tests — negative + robustness cases: a no-table page yields no
//! tables, and arbitrary content never panics. Catalog IDs: `TABLES-NONE-*`,
//! `TABLES-PROP-*`.

mod common;

use std::sync::Arc;

use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::Limits;
use pdf_text::tables::{drawings_to_device, find_tables, Strategy, TableOptions};
use pdf_text::{build_textpage, interpret_page, page_transform, words, TextPage};

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

fn build(content: &[u8]) -> (TextPage, Vec<pdf_text::Word>, Vec<pdf_text::DrawPath>) {
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

#[test]
fn tables_none_001_empty_page_no_tables() {
    let tp = TextPage::default();
    for strat in [Strategy::Lines, Strategy::LinesStrict, Strategy::Text] {
        let finder = find_tables(&tp, &[], &[], &TableOptions::with_strategy(strat));
        assert!(
            finder.is_empty(),
            "empty page must yield no tables ({strat:?})"
        );
    }
}

#[test]
fn tables_none_002_prose_no_table() {
    // A single paragraph of running prose — no aligned columns, no rulings.
    let content = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm \
        (The quick brown fox jumps over the lazy dog) Tj ET";
    let (tp, ws, dev) = build(content);
    // Lines strategy: no rulings, falls back to text; prose is a single row so
    // it should not form a >= 2-row grid.
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::Lines),
    );
    assert!(finder.is_empty(), "prose is not a table");
    // Strict lines: definitely empty (no rulings).
    let strict = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    assert!(strict.is_empty());
}

#[test]
fn tables_none_003_single_stroke_no_grid() {
    // A lone horizontal rule (e.g. an underline) is not a table grid.
    let content = b"1 w 100 700 m 400 700 l S \
        BT /F1 12 Tf 1 0 0 1 100 705 Tm (Underlined) Tj ET";
    let (tp, ws, dev) = build(content);
    let finder = find_tables(
        &tp,
        &ws,
        &dev,
        &TableOptions::with_strategy(Strategy::LinesStrict),
    );
    assert!(finder.is_empty(), "one rule does not make a grid");
}

#[test]
fn tables_prop_001_arbitrary_content_never_panics() {
    // A grab-bag of operators: text, partial rulings, curves, filled boxes.
    let contents: &[&[u8]] = &[
        b"",
        b"BT ET",
        b"1 w 50 50 m 60 60 l S",
        b"100 100 200 50 re f",
        b"BT /F1 8 Tf 1 0 0 1 10 10 Tm (x) Tj ET 0 0 m 5 5 l S",
        b"10 10 m 20 20 c 30 30 40 40 S",
        b"q 1 0 0 1 0 0 cm 0 0 100 100 re S Q",
    ];
    for c in contents {
        let (tp, ws, dev) = build(c);
        for strat in [Strategy::Lines, Strategy::LinesStrict, Strategy::Text] {
            // Must not panic; result validity is enough.
            let finder = find_tables(&tp, &ws, &dev, &TableOptions::with_strategy(strat));
            for t in &finder.tables {
                assert_eq!(t.cells.len(), t.row_count);
                for row in &t.cells {
                    assert_eq!(row.len(), t.col_count);
                }
            }
        }
    }
}
