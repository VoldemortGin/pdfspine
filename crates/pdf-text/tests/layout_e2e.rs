//! M2c end-to-end tests: build a `TextPage` from a real self-built PDF page via
//! `build_textpage`, asserting the full block/line/span/word structure + text in
//! device space. Catalog IDs: `LAYOUT-E2E-*`.

mod common;

use std::sync::Arc;

use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::Limits;
use pdf_text::{build_textpage, words};

use common::{winansi_type1, PageDoc};

/// Wraps a fixture `(DocumentStore, _)` into a `Page` handle. The fixture always
/// emits the single page as object 3, generation 0 (see `tests/common`).
fn page_handle(doc: pdf_core::DocumentStore) -> Page {
    Page::new(Arc::new(doc), 0, ObjRef::new(3, 0))
}

#[test]
fn layout_e2e_001_two_lines_two_words_structure_and_text() {
    // A WinAnsi font with explicit widths so advances are deterministic. Codes
    // 'A'..='Z' and space; width 500 for letters, 250 for space (1000-unit).
    // FirstChar 32 (space). Widths cover 32..=90 ('Z').
    let mut widths = vec![250i64]; // space (32)
    for code in 33..=90 {
        // give space (would be 32 only) — punctuation/digits 33..=64 width 500
        let _ = code;
        widths.push(500);
    }
    let font = winansi_type1("Helvetica", 32, &widths);

    // Content: "AB CD" on line 1 (y=700), "EF GH" on line 2 (y=686, 14pt down).
    // 12pt text. Each letter advances 500/1000*12 = 6pt; space 250/1000*12=3pt.
    let content = b"BT /F1 12 Tf \
        1 0 0 1 100 700 Tm (AB CD) Tj \
        1 0 0 1 100 686 Tm (EF GH) Tj \
        ET";

    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());

    // One text block with two lines (lines 14pt apart → same paragraph block).
    let text_blocks: Vec<_> = tp
        .blocks
        .iter()
        .filter(|b| b.kind == pdf_text::BlockKind::Text)
        .collect();
    assert_eq!(text_blocks.len(), 1, "expected one paragraph block");
    let block = text_blocks[0];
    assert_eq!(block.lines.len(), 2, "expected two lines");

    // Line texts.
    let l0: String = block.lines[0]
        .spans
        .iter()
        .flat_map(|s| s.text.chars())
        .collect();
    let l1: String = block.lines[1]
        .spans
        .iter()
        .flat_map(|s| s.text.chars())
        .collect();
    assert_eq!(l0, "AB CD");
    assert_eq!(l1, "EF GH");

    // Device-space y-flip: line 1 (user y 700) is above line 2 (user y 686), so
    // its device y0 is smaller.
    assert!(block.lines[0].bbox.y0 < block.lines[1].bbox.y0);

    // Words: 2 per line, 4 total, with correct numbering.
    let ws = words(&tp);
    let triples: Vec<(usize, usize, usize, &str)> = ws
        .iter()
        .map(|w| (w.block_no, w.line_no, w.word_no, w.text.as_str()))
        .collect();
    assert_eq!(
        triples,
        vec![
            (0, 0, 0, "AB"),
            (0, 0, 1, "CD"),
            (0, 1, 0, "EF"),
            (0, 1, 1, "GH"),
        ]
    );
}

#[test]
fn layout_e2e_002_device_space_top_left_origin() {
    let widths: Vec<i64> = std::iter::once(250)
        .chain(std::iter::repeat_n(500, 58))
        .collect();
    let font = winansi_type1("Helvetica", 32, &widths);
    // A single word near the top of the page (user y 760 on a 792-high page).
    let content = b"BT /F1 12 Tf 1 0 0 1 72 760 Tm (Top) Tj ET";
    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = page_handle(doc);
    let tp = build_textpage(page.document(), &page, &Limits::unbounded_decode());

    assert_eq!((tp.width, tp.height), (612.0, 792.0));
    let line = &tp.blocks[0].lines[0];
    // Near the top → small device y. Baseline device y ≈ 792 - 760 = 32.
    assert!(line.bbox.y0 < 100.0, "text near top should have small y");
    // x is preserved (x0 = 72).
    assert!((line.bbox.x0 - 72.0).abs() < 1.0);
    let text: String = line.spans.iter().flat_map(|s| s.text.chars()).collect();
    assert_eq!(text, "Top");
}
