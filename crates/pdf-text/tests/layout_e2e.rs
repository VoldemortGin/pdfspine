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

/// Helvetica AFM advances for codes 32..=126 (1000-unit glyph space), so a
/// full-width `Tj` string lays its glyph cells edge to edge exactly as a real
/// Core-14 page does.
const HELV: [i64; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32..47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 48..63
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // 64..79
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // 80..95
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 96..111
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // 112..126
];

/// Every line's text (spans concatenated), in emitted block/line order.
fn line_texts(tp: &pdf_text::TextPage) -> Vec<String> {
    tp.blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .map(|l| l.spans.iter().flat_map(|s| s.text.chars()).collect())
        .collect()
}

/// Builds a page from `content` with Helvetica (full AFM widths) as `/F1`.
fn helvetica_page(content: &[u8]) -> pdf_text::TextPage {
    let font = winansi_type1("Helvetica", 32, &HELV);
    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = page_handle(doc);
    build_textpage(page.document(), &page, &Limits::unbounded_decode())
}

/// LAYOUT-E2E-003: a single `Tj` title whose glyph cells touch edge to edge is
/// never broken into several lines by the page's column-gutter detector.
///
/// Reproduces the fintabnet `SLB_2015_page_72` header: a financial statement
/// whose label column (x 40..150) and five numeric columns (x ≥ 300) leave a
/// wide glyph-free band in every table row, so `detect_page_gutters` finds a
/// gutter there; the one-string title `(SCHLUMBERGER LIMITED AND SUBSIDIARIES) Tj`
/// runs straight across that band with zero gap between consecutive cells. A
/// gutter is a property of the *other* lines; it must not cut a run whose own
/// glyphs are contiguous (PyMuPDF keeps the title as one line).
#[test]
fn layout_e2e_003_touching_glyphs_cross_gutter_stay_one_line() {
    let mut content = String::from("BT /F1 10 Tf ");
    content.push_str("1 0 0 1 60 740 Tm (SCHLUMBERGER LIMITED AND SUBSIDIARIES) Tj ");
    for row in 0..12 {
        let y = 700.0 - 14.0 * row as f64;
        content.push_str(&format!("1 0 0 1 40 {y} Tm (Revenue and other income) Tj "));
        for x in [300, 360, 420, 480, 540] {
            content.push_str(&format!("1 0 0 1 {x} {y} Tm (12,345) Tj "));
        }
    }
    content.push_str("ET");
    let tp = helvetica_page(content.as_bytes());

    let lines = line_texts(&tp);
    assert!(
        lines
            .iter()
            .any(|l| l == "SCHLUMBERGER LIMITED AND SUBSIDIARIES"),
        "title was cut at the table gutter: {lines:?}"
    );
    let ws: Vec<String> = words(&tp).iter().map(|w| w.text.clone()).collect();
    for w in ["SCHLUMBERGER", "LIMITED", "AND", "SUBSIDIARIES"] {
        assert!(ws.iter().any(|x| x == w), "word {w:?} missing from {ws:?}");
    }
    // The table rows themselves still split at their genuine inter-cell gaps.
    assert!(
        lines.iter().any(|l| l == "Revenue and other income"),
        "table label row lost: {lines:?}"
    );
}

/// LAYOUT-E2E-004: a `Tj` whose *space glyph* sits inside a detected gutter band
/// stays one line — the space cell fills the gap edge to edge, so there is no
/// along-axis whitespace at the gutter to cut on.
///
/// Reproduces the IRS `f1120` p1 pattern (`(Schedule C) Tj` → `Sche\ndule C` /
/// `Schedule \nC`): a two-column form whose label column (x 36..74) and value
/// columns (x ≥ 110) leave a band at x 71..110 (midpoint ≈ 90.5), and a section
/// title painted across that band as one string, its space cell spanning
/// x ≈ 89.4..91.6 — right over the gutter midpoint.
#[test]
fn layout_e2e_004_space_glyph_inside_gutter_stays_one_line() {
    let mut content = String::from("BT /F1 8 Tf ");
    content.push_str("1 0 0 1 56 740 Tm (Schedule C) Tj ");
    for row in 0..12 {
        let y = 700.0 - 12.0 * row as f64;
        content.push_str(&format!("1 0 0 1 36 {y} Tm (Dividends) Tj "));
        content.push_str(&format!("1 0 0 1 110 {y} Tm (1,234) Tj "));
        content.push_str(&format!("1 0 0 1 160 {y} Tm (5,678) Tj "));
    }
    content.push_str("ET");
    let tp = helvetica_page(content.as_bytes());

    let lines = line_texts(&tp);
    assert!(
        lines.iter().any(|l| l == "Schedule C"),
        "form title was cut at the gutter: {lines:?}"
    );
}

/// LAYOUT-E2E-005 (reverse invariant of 003/004): a genuine two-column body —
/// each column its own `Tj` per baseline, the two columns sharing baselines and
/// separated by a real glyph-free gutter — still splits into per-column lines
/// and reads column-major (same expectation as LAYOUT-COLUMN-REGRESSION-001).
#[test]
fn layout_e2e_005_true_two_column_still_splits_at_gutter() {
    let left = ["Lone", "Ltwo", "Lthree", "Lfour", "Lfive", "Lsix"];
    let right = ["Rone", "Rtwo", "Rthree", "Rfour", "Rfive", "Rsix"];
    let mut content = String::from("BT /F1 10 Tf ");
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        let y = 740.0 - 20.0 * i as f64;
        content.push_str(&format!(
            "1 0 0 1 40 {y} Tm ({l} column body text runs to here) Tj "
        ));
        content.push_str(&format!(
            "1 0 0 1 320 {y} Tm ({r} column body text runs to here) Tj "
        ));
    }
    content.push_str("ET");
    let tp = helvetica_page(content.as_bytes());

    let lines = line_texts(&tp);
    assert_eq!(
        lines.len(),
        12,
        "each column line must stay its own line: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|l| !(l.contains("Lone") && l.contains("Rone"))),
        "columns merged into one line: {lines:?}"
    );
    let pos = |w: &str| {
        lines
            .iter()
            .position(|l| l.starts_with(w))
            .unwrap_or(usize::MAX)
    };
    let last_left = left.iter().map(|w| pos(w)).max().unwrap();
    let first_right = right.iter().map(|w| pos(w)).min().unwrap();
    assert!(
        last_left < first_right,
        "expected column-major order (all L before all R): {lines:?}"
    );
}
