//! M2e `search` unit tests (PRD §8.6): PyMuPDF `search_for` semantics over a
//! `TextPage`. Self-built `TextPage`s constructed directly from the model
//! structs with explicit char bboxes so geometry assertions are deterministic.
//! No PyMuPDF files. Catalog IDs: `SEARCH-001` … `SEARCH-011`.

use pdf_core::geom::{Point, Rect};
use pdf_text::model::{Block, BlockKind, Char, Line, Span, TextPage};
use pdf_text::{search, SearchOptions};

const EPS: f64 = 1e-6;

/// A device-space char cell `[x .. x + w] × [y0 .. y1]` carrying `c`.
fn ch(c: char, x: f64, w: f64, y0: f64, y1: f64) -> Char {
    Char {
        origin: Point::new(x, y1),
        bbox: Rect::new(x, y0, x + w, y1),
        c,
    }
}

/// Builds a span from a run of equal-width chars starting at `x0`, baseline
/// band `[y0 .. y1]`, advancing `w` per char.
fn span_of(text: &str, x0: f64, w: f64, y0: f64, y1: f64) -> Span {
    let mut x = x0;
    let mut chars = Vec::new();
    for c in text.chars() {
        chars.push(ch(c, x, w, y0, y1));
        x += w;
    }
    let bbox = chars
        .iter()
        .fold(Rect::default(), |acc, c| acc.union(&c.bbox));
    Span {
        bbox,
        font: "Helvetica".into(),
        size: y1 - y0,
        flags: 0,
        color: 0,
        ascender: 0.8,
        descender: -0.2,
        origin: Point::new(x0, y1),
        text: text.to_string(),
        chars,
    }
}

/// A line wrapping the given spans (bbox = union of span bboxes).
fn line_of(spans: Vec<Span>) -> Line {
    let bbox = spans
        .iter()
        .fold(Rect::default(), |acc, s| acc.union(&s.bbox));
    Line {
        bbox,
        wmode: 0,
        dir: (1.0, 0.0),
        spans,
    }
}

/// A text block wrapping the given lines (bbox = union of line bboxes).
fn block_of(number: usize, lines: Vec<Line>) -> Block {
    let bbox = lines
        .iter()
        .fold(Rect::default(), |acc, l| acc.union(&l.bbox));
    Block {
        bbox,
        kind: BlockKind::Text,
        lines,
        image: None,
        number,
    }
}

/// A single-block page from a list of lines.
fn page(lines: Vec<Line>) -> TextPage {
    let block = block_of(0, lines);
    TextPage {
        width: 612.0,
        height: 792.0,
        blocks: vec![block],
    }
}

/// A one-line, one-span page from `text`.
fn simple_page(text: &str, x0: f64, w: f64, y0: f64, y1: f64) -> TextPage {
    page(vec![line_of(vec![span_of(text, x0, w, y0, y1)])])
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() <= EPS, "expected {a} ≈ {b}");
}

// === SEARCH-001 single hit → one quad overlapping the word ================

#[test]
fn search_001_single_hit_one_quad() {
    // "Hello world" — search "world" → one quad over the second word.
    let tp = simple_page("Hello world", 100.0, 10.0, 700.0, 712.0);
    let quads = search(&tp, "world", SearchOptions::default());
    assert_eq!(quads.len(), 1, "one quad for a single hit");
    let r = quads[0].rect();
    // "world" starts at char index 6 → x in [160 .. 210].
    approx(r.x0, 160.0);
    approx(r.x1, 210.0);
    approx(r.y0, 700.0);
    approx(r.y1, 712.0);
}

// === SEARCH-002 multiple hits → one quad each in reading order ============

#[test]
fn search_002_multiple_hits_reading_order() {
    // Two lines each containing "cat"; expect two quads, top line first.
    let tp = page(vec![
        line_of(vec![span_of("a cat", 100.0, 10.0, 100.0, 112.0)]),
        line_of(vec![span_of("the cat", 100.0, 10.0, 200.0, 212.0)]),
    ]);
    let quads = search(&tp, "cat", SearchOptions::default());
    assert_eq!(quads.len(), 2, "one quad per hit");
    // Reading order: first hit on the upper line (smaller y).
    assert!(
        quads[0].rect().y0 < quads[1].rect().y0,
        "hits returned top-to-bottom"
    );
    // First "cat" begins at index 2 on line 1: x in [120 .. 150].
    approx(quads[0].rect().x0, 120.0);
    approx(quads[0].rect().x1, 150.0);
    // Second "cat" begins at index 4 on line 2: x in [140 .. 170].
    approx(quads[1].rect().x0, 140.0);
    approx(quads[1].rect().x1, 170.0);
}

// === SEARCH-003 case-insensitive default ==================================

#[test]
fn search_003_case_insensitive() {
    let tp = simple_page("Hello", 100.0, 10.0, 700.0, 712.0);
    let quads = search(&tp, "HELLO", SearchOptions::default());
    assert_eq!(quads.len(), 1, "uppercase needle finds mixed-case text");
    let lower = search(&tp, "hello", SearchOptions::default());
    assert_eq!(lower.len(), 1, "lowercase needle finds mixed-case text");
}

// === SEARCH-004 Unicode-normalized compare (NFC vs NFD) ===================

#[test]
fn search_004_unicode_normalized_compare() {
    // Page has the precomposed (NFC) 'é' as a single Char; search with the
    // decomposed (NFD) needle "e\u{0301}" must still match.
    let tp = simple_page("café", 100.0, 10.0, 700.0, 712.0);
    let nfd_needle = "cafe\u{0301}";
    let quads = search(&tp, nfd_needle, SearchOptions::default());
    assert_eq!(quads.len(), 1, "NFD needle matches NFC page text");
    let r = quads[0].rect();
    // Whole word: chars 0..4 → x in [100 .. 140].
    approx(r.x0, 100.0);
    approx(r.x1, 140.0);
}

// === SEARCH-005 across spans within a line → one quad =====================

#[test]
fn search_005_across_spans_one_quad() {
    // One line, two spans "He" + "llo"; "Hello" must match as one quad.
    let line = line_of(vec![
        span_of("He", 100.0, 10.0, 700.0, 712.0),
        span_of("llo", 120.0, 10.0, 700.0, 712.0),
    ]);
    let tp = page(vec![line]);
    let quads = search(&tp, "Hello", SearchOptions::default());
    assert_eq!(quads.len(), 1, "cross-span hit on one line is one quad");
    let r = quads[0].rect();
    approx(r.x0, 100.0);
    approx(r.x1, 150.0); // "He"(2)+"llo"(3) = 5 chars × 10 = 50 wide.
}

// === SEARCH-006 spanning a line break → one quad per line =================

#[test]
fn search_006_across_line_break_two_quads() {
    // Two lines "Hel" / "lo"; "Hello" matches across the visual line break
    // (PyMuPDF matches across the break, no separator inserted), yielding one
    // quad per line segment.
    let tp = page(vec![
        line_of(vec![span_of("Hel", 100.0, 10.0, 100.0, 112.0)]),
        line_of(vec![span_of("lo", 100.0, 10.0, 200.0, 212.0)]),
    ]);
    let quads = search(&tp, "Hello", SearchOptions::default());
    assert_eq!(quads.len(), 2, "wrapped hit yields one quad per line");
    // First quad: "Hel" on the upper line.
    let r0 = quads[0].rect();
    approx(r0.x0, 100.0);
    approx(r0.x1, 130.0);
    approx(r0.y0, 100.0);
    approx(r0.y1, 112.0);
    // Second quad: "lo" on the lower line.
    let r1 = quads[1].rect();
    approx(r1.x0, 100.0);
    approx(r1.x1, 120.0);
    approx(r1.y0, 200.0);
    approx(r1.y1, 212.0);
}

// === SEARCH-007 hit_max caps ==============================================

#[test]
fn search_007_hit_max_caps() {
    // Three "ab" occurrences across three lines.
    let tp = page(vec![
        line_of(vec![span_of("ab", 100.0, 10.0, 100.0, 112.0)]),
        line_of(vec![span_of("ab", 100.0, 10.0, 200.0, 212.0)]),
        line_of(vec![span_of("ab", 100.0, 10.0, 300.0, 312.0)]),
    ]);
    let all = search(&tp, "ab", SearchOptions::default());
    assert_eq!(all.len(), 3, "unlimited finds all three");
    let capped = search(
        &tp,
        "ab",
        SearchOptions {
            hit_max: 2,
            ..SearchOptions::default()
        },
    );
    assert_eq!(capped.len(), 2, "hit_max caps to 2 hits");
}

// === SEARCH-008 clip restricts ============================================

#[test]
fn search_008_clip_restricts() {
    // Two "x" hits on two lines; clip keeps only the lower line.
    let tp = page(vec![
        line_of(vec![span_of("x", 100.0, 10.0, 100.0, 112.0)]),
        line_of(vec![span_of("x", 100.0, 10.0, 200.0, 212.0)]),
    ]);
    let unclipped = search(&tp, "x", SearchOptions::default());
    assert_eq!(unclipped.len(), 2);
    let clip = Rect::new(0.0, 150.0, 612.0, 300.0); // covers only the 2nd line
    let clipped = search(
        &tp,
        "x",
        SearchOptions {
            clip: Some(clip),
            ..SearchOptions::default()
        },
    );
    assert_eq!(clipped.len(), 1, "clip drops the non-intersecting hit");
    approx(clipped[0].rect().y0, 200.0);
}

// === SEARCH-009 not found → empty =========================================

#[test]
fn search_009_not_found_empty() {
    let tp = simple_page("Hello world", 100.0, 10.0, 700.0, 712.0);
    let quads = search(&tp, "zzz", SearchOptions::default());
    assert!(quads.is_empty(), "no match → empty Vec");
}

// === SEARCH-010 quads=false vs quads=true (same Rust geometry) ============

#[test]
fn search_010_quads_modes_same_geometry() {
    let tp = simple_page("Hello world", 100.0, 10.0, 700.0, 712.0);
    let q_false = search(
        &tp,
        "world",
        SearchOptions {
            quads: false,
            ..SearchOptions::default()
        },
    );
    let q_true = search(
        &tp,
        "world",
        SearchOptions {
            quads: true,
            ..SearchOptions::default()
        },
    );
    assert_eq!(q_false.len(), 1);
    assert_eq!(q_true.len(), 1);
    // Both modes return the same Quad geometry at the Rust level (the
    // Rect-vs-Quad distinction is the PyO3 layer's job).
    assert_eq!(q_false[0], q_true[0]);
    // The quad is axis-aligned and round-trips through Quad::from_rect(&rect).
    let r = q_false[0].rect();
    assert_eq!(pdf_core::geom::Quad::from_rect(&r), q_false[0]);
}

// === SEARCH-011 empty needle → empty ======================================

#[test]
fn search_011_empty_needle_empty() {
    let tp = simple_page("Hello world", 100.0, 10.0, 700.0, 712.0);
    let quads = search(&tp, "", SearchOptions::default());
    assert!(quads.is_empty(), "empty needle → empty Vec, no panic");
}
