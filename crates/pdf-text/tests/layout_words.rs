//! M2c word-segmentation tests (PRD §8.6.2, §10.7). Catalog IDs: `WORDS-*`.

use pdf_core::geom::{Point, Rect};
use pdf_text::model::WritingDir;
use pdf_text::{textpage_from_glyphs, words, PositionedGlyph};
use smol_str::SmolStr;

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
