//! M2c word-segmentation tests (PRD §8.6.2, §10.7). Catalog IDs: `WORDS-*`.

mod common;

use std::sync::Arc;

use pdf_core::geom::{Point, Rect};
use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::{Limits, Object};
use pdf_text::model::WritingDir;
use pdf_text::serialize::to_text;
use pdf_text::{build_textpage, textpage_from_glyphs, words, PositionedGlyph, TextPage};
use smol_str::SmolStr;

use common::{winansi_type1, winansi_type1_with_metrics, PageDoc};

fn letter() -> Rect {
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

/// A glyph with an explicit advance width (so we control inter-char gaps).
fn g(c: &str, ox: f64, oy: f64, size: f64, w: f64) -> PositionedGlyph {
    PositionedGlyph {
        unicode: SmolStr::new(c),
        code: c.chars().next().map_or(0, |ch| ch as u32),
        origin: Point::new(ox, oy),
        bbox: Rect::new(ox, oy - 0.2 * size, ox + w, oy + 0.7 * size),
        font_name: SmolStr::new("Helvetica"),
        size,
        color: 0,
        render_mode: 0,
        writing_dir: WritingDir::Horizontal,
        advance_dir: (1.0, 0.0),
        ascender: 0.7,
        descender: -0.2,
    }
}

#[test]
fn words_001_split_on_space() {
    // "Hi there" with a literal space char.
    let gs = vec![
        g("H", 100.0, 700.0, 12.0, 6.0),
        g("i", 106.0, 700.0, 12.0, 4.0),
        g(" ", 110.0, 700.0, 12.0, 4.0),
        g("t", 114.0, 700.0, 12.0, 4.0),
        g("o", 118.0, 700.0, 12.0, 6.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = words(&tp);
    let texts: Vec<&str> = ws.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, vec!["Hi", "to"]);
}

#[test]
fn words_002_kerned_gap_no_space_still_splits() {
    // The hard PyMuPDF case: "AB" then a large TJ-kerned gap then "CD", with NO
    // space character — must still split into two words.
    let gs = vec![
        g("A", 100.0, 700.0, 12.0, 6.0),
        g("B", 106.0, 700.0, 12.0, 6.0), // right edge at 112
        // big gap: next char starts at 130 → gap = 18 > 0.2*12 = 2.4
        g("C", 130.0, 700.0, 12.0, 6.0),
        g("D", 136.0, 700.0, 12.0, 6.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = words(&tp);
    let texts: Vec<&str> = ws.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, vec!["AB", "CD"]);
}

#[test]
fn words_003_small_gap_does_not_split() {
    // Normal inter-glyph spacing must keep one word.
    let gs = vec![
        g("w", 100.0, 700.0, 12.0, 6.0),  // right edge 106
        g("o", 106.5, 700.0, 12.0, 6.0),  // gap 0.5 < 2.4
        g("r", 112.75, 700.0, 12.0, 6.0), // gap 0.25
        g("d", 119.0, 700.0, 12.0, 6.0),  // gap 0.25
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = words(&tp);
    assert_eq!(ws.len(), 1);
    assert_eq!(ws[0].text, "word");
}

#[test]
fn words_004_word_bbox_is_char_union() {
    let gs = vec![
        g("A", 100.0, 700.0, 12.0, 6.0),
        g("B", 106.0, 700.0, 12.0, 6.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = words(&tp);
    assert_eq!(ws.len(), 1);
    // Char bboxes are in device space; union spans x [100,112].
    let bb = ws[0].bbox;
    assert!((bb.x0 - 100.0).abs() < 1e-6);
    assert!((bb.x1 - 112.0).abs() < 1e-6);
    // Every char bbox is contained in the word bbox.
    for span in &tp.blocks[0].lines[0].spans {
        for ch in &span.chars {
            assert!(bb.contains_rect(&ch.bbox));
        }
    }
}

#[test]
fn words_005_block_line_word_numbering_monotonic() {
    // Two lines, two words each → (block,line,word) triples well-formed.
    let gs = vec![
        // line 1
        g("a", 100.0, 700.0, 12.0, 6.0),
        g(" ", 106.0, 700.0, 12.0, 4.0),
        g("b", 110.0, 700.0, 12.0, 6.0),
        // line 2 (14pt lower → same block)
        g("c", 100.0, 686.0, 12.0, 6.0),
        g(" ", 106.0, 686.0, 12.0, 4.0),
        g("d", 110.0, 686.0, 12.0, 6.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = words(&tp);
    assert_eq!(ws.len(), 4);
    // line 0: words a,b ; line 1: words c,d. word_no resets per line.
    assert_eq!(
        (ws[0].line_no, ws[0].word_no, ws[0].text.as_str()),
        (0, 0, "a")
    );
    assert_eq!(
        (ws[1].line_no, ws[1].word_no, ws[1].text.as_str()),
        (0, 1, "b")
    );
    assert_eq!(
        (ws[2].line_no, ws[2].word_no, ws[2].text.as_str()),
        (1, 0, "c")
    );
    assert_eq!(
        (ws[3].line_no, ws[3].word_no, ws[3].text.as_str()),
        (1, 1, "d")
    );
    // All in the same block.
    assert!(ws.iter().all(|w| w.block_no == ws[0].block_no));
}

#[test]
fn words_006_nbsp_is_separator() {
    // A non-breaking space (U+00A0) splits like a normal space.
    let gs = vec![
        g("a", 100.0, 700.0, 12.0, 6.0),
        g("\u{00A0}", 106.0, 700.0, 12.0, 4.0),
        g("b", 110.0, 700.0, 12.0, 6.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = words(&tp);
    let texts: Vec<&str> = ws.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, vec!["a", "b"]);
}

// === end-to-end: words vs. layout's synthesized spaces =====================

/// Helvetica AFM advances for WinAnsi codes 32..=126 (1000-unit glyph space).
const HELV: [i64; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32..47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 48..63
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // 64..79
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // 80..95
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 96..111
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // 112..126
];

/// Builds a one-page PDF with `font` as `/F1` and `content`, then runs the full
/// `build_textpage` pipeline (interpreter → layout). The fixture always emits
/// the page as object 3 (see `tests/common`).
fn textpage_e2e(font: Object, content: &[u8]) -> TextPage {
    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = Page::new(Arc::new(doc), 0, ObjRef::new(3, 0));
    build_textpage(page.document(), &page, &Limits::unbounded_decode())
}

fn word_texts(tp: &TextPage) -> Vec<String> {
    words(tp).iter().map(|w| w.text.clone()).collect()
}

#[test]
fn words_007_tc_tracking_keeps_word_whole() {
    // `3 Tc` at 12pt opens a 3pt gap after every glyph — wider than the word-gap
    // threshold. Layout recognises the uniform tracking and emits "extraction"
    // unbroken; `words` must agree instead of shattering it into letters.
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 72 700 Td 3 Tc (extraction) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "extraction");
    assert_eq!(word_texts(&tp), vec!["extraction"]);
}

#[test]
fn words_008_positive_descent_tracked_text_keeps_words_whole() {
    // eurlex pattern: /Descent written POSITIVE (+250) shrinks the glyph cell,
    // `Tf 1` with the 9.59 scale in `Tm`, and `0.15 Tc` tracking (1.44pt per
    // letter). The literal space is the only word boundary.
    let tp = textpage_e2e(
        winansi_type1_with_metrics("Helvetica", 32, &HELV, 750, 250),
        b"BT /F1 1 Tf 9.59 0 0 9.59 72 700 Tm 0.15 Tc (Particular provisions) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "Particular provisions");
    assert_eq!(word_texts(&tp), vec!["Particular", "provisions"]);
}

#[test]
fn words_009_word_boundaries_always_match_text() {
    // Invariant: a real `TJ` word gap (-300 → 3.6pt at 12pt) with no space glyph
    // is split by layout's synthesized space, and `words` sees exactly the same
    // boundaries as `to_text` split on whitespace — never more, never fewer.
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 72 700 Td [(extr) -300 (action)] TJ ET",
    );
    let text = to_text(&tp, 0);
    let from_text: Vec<&str> = text.split_whitespace().collect();
    assert_eq!(from_text, vec!["extr", "action"]);
    assert_eq!(word_texts(&tp), from_text);
}
