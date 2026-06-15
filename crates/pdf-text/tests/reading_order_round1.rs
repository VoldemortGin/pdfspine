//! Round-1 accuracy regressions: column-aware reading order + CropBox clipping.
//! Catalog IDs: `READORDER-*`, `CROPCLIP-*`.
//!
//! These encode the *rules* fixed in round 1 against self-built fixtures (no
//! PyMuPDF files):
//!   - a two-column page reads the left column fully, then the right column
//!     (fitz-matching), not interleaved line-by-line;
//!   - a full-width header above a two-column body is emitted before the body
//!     and does not collapse the columns;
//!   - a glyph string outside the CropBox is excluded from `get_text("text")`;
//!   - no line is double-emitted and whitespace stays sane.

use pdf_core::geom::{Point, Rect};
use pdf_text::model::WritingDir;
use pdf_text::{
    defaults, textpage_from_glyphs, textpage_from_glyphs_clipped, to_text, PositionedGlyph,
};
use smol_str::SmolStr;

fn letter() -> Rect {
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

/// One glyph at user-space origin `(ox, oy)` (y-up), advance ≈ 0.5·size.
fn glyph(c: char, ox: f64, oy: f64, size: f64) -> PositionedGlyph {
    let w = 0.5 * size;
    PositionedGlyph {
        unicode: SmolStr::new(c.to_string()),
        code: c as u32,
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

/// Lays a word out as a run of glyphs starting at `(x, y)` (user space, y-up),
/// appending to `out`. Returns the x-advance end.
fn lay_word(out: &mut Vec<PositionedGlyph>, word: &str, x: f64, y: f64, size: f64) -> f64 {
    let mut cx = x;
    for ch in word.chars() {
        out.push(glyph(ch, cx, y, size));
        cx += 0.5 * size + 0.5; // glyph advance + tracking
    }
    cx
}

/// Lays a whole sentence (words separated by single spaces) on one baseline
/// starting at `(x, y)`, appending one glyph per character — including an
/// explicit space glyph for ' ' so the serialized line text preserves spaces.
fn lay_word_line(out: &mut Vec<PositionedGlyph>, sentence: &str, x: f64, y: f64, size: f64) {
    let mut cx = x;
    for ch in sentence.chars() {
        out.push(glyph(ch, cx, y, size));
        cx += 0.5 * size + 0.5;
    }
}

/// Plain-text extraction of a glyph set (default `get_text("text")` flags),
/// trimmed, with blank lines removed for stable assertions.
fn extract(glyphs: &[PositionedGlyph]) -> Vec<String> {
    let tp = textpage_from_glyphs(glyphs, &[], letter(), 0);
    to_text(&tp, defaults::TEXT)
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

// === READORDER: two-column reading order ==================================

/// A clean two-column page (left col x≈60, right col x≈360, wide gutter) must
/// read the *entire* left column before the right column — not interleaved.
#[test]
fn readorder_001_two_columns_left_then_right() {
    // Two substantial columns (each a wide multi-word line, left x≈60, right
    // x≈340, wide gutter near x≈300). Read the entire left column, then the
    // right — not interleaved line-by-line.
    let size = 10.0;
    let left = [
        "L1 left column first wide line of text",
        "L2 left column second wide line of text",
        "L3 left column third wide line of text",
    ];
    let right = [
        "R1 right column first wide line of text",
        "R2 right column second wide line of text",
        "R3 right column third wide line of text",
    ];
    let mut gs = Vec::new();
    // Y positions top→bottom (user space): 700, 686, 672.
    for (i, w) in left.iter().enumerate() {
        lay_word_line(&mut gs, w, 60.0, 700.0 - 14.0 * i as f64, size);
    }
    for (i, w) in right.iter().enumerate() {
        lay_word_line(&mut gs, w, 340.0, 700.0 - 14.0 * i as f64, size);
    }

    let lines = extract(&gs);
    // Expect left column (top→bottom) entirely, then right column.
    let joined = lines.join("|");
    let l_idx = joined.find("L1 left").unwrap();
    let r_idx = joined.find("R1 right").unwrap();
    assert!(
        l_idx < r_idx,
        "left column must precede right column; got {lines:?}"
    );
    // No interleave: L2 must come before R1.
    let l2 = joined.find("L2 left").unwrap();
    assert!(l2 < r_idx, "columns interleaved: {lines:?}");
    // Within each column, order is top-to-bottom.
    assert!(joined.find("L1 left").unwrap() < joined.find("L3 left").unwrap());
    assert!(joined.find("R1 right").unwrap() < joined.find("R3 right").unwrap());
}

/// A full-width header above a two-column body: the header reads first, then the
/// two columns stay un-interleaved (the body does not collapse into one block).
#[test]
fn readorder_002_full_width_header_then_columns() {
    let size = 10.0;
    let mut gs = Vec::new();
    // Full-width header at the top (spans the gutter, x 60..520).
    lay_word_line(
        &mut gs,
        "HEADER spans the full page width here",
        60.0,
        740.0,
        14.0,
    );
    // Two-column body lower down, substantial columns, wide gutter near x≈300.
    let left = [
        "alpha left column wide first line text",
        "beta left column wide second line text",
        "gamma left column wide third line text",
    ];
    let right = [
        "delta right column wide first line text",
        "epsilon right column wide second line text",
        "zeta right column wide third line text",
    ];
    for (i, w) in left.iter().enumerate() {
        lay_word_line(&mut gs, w, 60.0, 700.0 - 14.0 * i as f64, size);
    }
    for (i, w) in right.iter().enumerate() {
        lay_word_line(&mut gs, w, 340.0, 700.0 - 14.0 * i as f64, size);
    }

    let lines = extract(&gs);
    let joined = lines.join("|");
    let h = joined.find("HEADER spans").unwrap();
    let a = joined.find("alpha").unwrap();
    let d = joined.find("delta").unwrap();
    assert!(h < a && h < d, "header must precede the body: {lines:?}");
    // Left column (alpha..gamma) before right column (delta..).
    assert!(a < d, "columns interleaved under header: {lines:?}");
    assert!(
        joined.find("gamma").unwrap() < d,
        "left col split: {lines:?}"
    );
}

/// A single column of plain prose must NOT be split into columns by the
/// column-gutter heuristic (no false positives).
#[test]
fn readorder_003_single_column_not_split() {
    let size = 11.0;
    let mut gs = Vec::new();
    let words = [
        "The", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog",
    ];
    // One justified line: words across the full width, single baseline.
    let mut x = 50.0;
    for w in words {
        x = lay_word(&mut gs, w, x, 700.0, size) + 6.0;
    }
    // A second line below.
    let mut x2 = 50.0;
    for w in ["second", "line", "of", "text", "here"] {
        x2 = lay_word(&mut gs, w, x2, 686.0, size) + 6.0;
    }

    let lines = extract(&gs);
    // Two physical lines, each containing its words in order (no column split).
    assert_eq!(lines.len(), 2, "single column over-split: {lines:?}");
    assert!(lines[0].contains("The") && lines[0].contains("dog"));
    assert!(lines[1].contains("second") && lines[1].contains("here"));
}

// === de-dup / whitespace ==================================================

/// Two columns that happen to start with the *same* text must each appear once
/// (no double-emit) and in column order — the artifact that previously looked
/// like a duplicated line.
#[test]
fn readorder_004_identical_column_starts_not_duplicated() {
    // Two *substantial* columns (each a wide multi-word line) whose first line is
    // the identical phrase — the cdc-mmwr case where both the Abstract and
    // Introduction columns open with the same sentence. With line-by-line column
    // interleave they would print as two adjacent duplicate lines; column-aware
    // ordering keeps each column contiguous, so the phrase appears once per
    // column and never as consecutive duplicates.
    let size = 10.0;
    let mut gs = Vec::new();
    // Left column at x≈60, right column at x≈340; each line spans ~230pt.
    let phrase = "the same opening sentence of this column";
    for (i, line) in [phrase, "left column body continues here onward"]
        .iter()
        .enumerate()
    {
        lay_word_line(&mut gs, line, 60.0, 700.0 - 14.0 * i as f64, size);
    }
    for (i, line) in [phrase, "right column body continues here onward"]
        .iter()
        .enumerate()
    {
        lay_word_line(&mut gs, line, 340.0, 700.0 - 14.0 * i as f64, size);
    }

    let lines = extract(&gs);
    // The phrase appears exactly twice (once per column).
    let phrase_lines: Vec<&String> = lines.iter().filter(|l| l.trim() == phrase).collect();
    assert_eq!(phrase_lines.len(), 2, "unexpected phrase count: {lines:?}");
    // Never as two consecutive identical non-empty lines (the old artifact).
    for w in lines.windows(2) {
        assert!(
            w[0] != w[1] || w[0].trim().is_empty(),
            "consecutive duplicate line: {:?}",
            w[0]
        );
    }
    // Column order: left body precedes right body.
    let joined = lines.join("|");
    assert!(joined.find("left column body").unwrap() < joined.find("right column body").unwrap());
}

/// Whitespace sanity: a simple two-line single column yields exactly two
/// non-empty text lines (no spurious extra blank lines between them).
#[test]
fn readorder_005_no_spurious_blank_lines() {
    let size = 11.0;
    let mut gs = Vec::new();
    lay_word(&mut gs, "first", 50.0, 700.0, size);
    lay_word(&mut gs, "second", 50.0, 686.0, size);
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let text = to_text(&tp, defaults::TEXT);
    // Exactly the two lines + a trailing block newline; no internal blank line.
    let nonblank: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(nonblank, vec!["first", "second"], "got: {text:?}");
    assert!(
        !text.contains("\n\n"),
        "spurious blank line in single block: {text:?}"
    );
}

// === CROPCLIP: CropBox clipping ===========================================

/// A glyph string whose origin is outside the CropBox is excluded from
/// `get_text("text")` (the `TEXT_MEDIABOX_CLIP` default behaviour); on-page text
/// is retained.
#[test]
fn cropclip_001_off_cropbox_string_excluded() {
    let size = 10.0;
    let mut gs = Vec::new();
    // On-page body text (well inside the crop region).
    lay_word(&mut gs, "VISIBLE", 200.0, 600.0, size);
    // Off-page print-control string to the left of the crop region (x ≈ -40).
    lay_word(&mut gs, "OFFPAGEMARK", -40.0, 600.0, size);

    // CropBox = a centered sub-rectangle of the media box.
    let crop = Rect::new(50.0, 50.0, 562.0, 742.0);
    let tp = textpage_from_glyphs_clipped(&gs, &[], letter(), 0, Some(crop));
    let text = to_text(&tp, defaults::TEXT);
    assert!(text.contains("VISIBLE"), "on-page text dropped: {text:?}");
    assert!(
        !text.contains("OFFPAGEMARK"),
        "off-cropbox text leaked: {text:?}"
    );
}

/// Without a clip rect, the off-page string is retained (clipping is opt-in via
/// the CropBox; the no-clip entry point must not drop anything).
#[test]
fn cropclip_002_no_clip_keeps_everything() {
    let size = 10.0;
    let mut gs = Vec::new();
    lay_word(&mut gs, "VISIBLE", 200.0, 600.0, size);
    lay_word(&mut gs, "OFFPAGEMARK", -40.0, 600.0, size);
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let text = to_text(&tp, defaults::TEXT);
    assert!(
        text.contains("VISIBLE") && text.contains("OFFPAGEMARK"),
        "{text:?}"
    );
}

/// A glyph sitting exactly on the CropBox edge is kept (1pt slack), matching
/// fitz keeping marginal glyphs.
#[test]
fn cropclip_003_edge_glyph_kept() {
    let size = 10.0;
    let mut gs = Vec::new();
    // Origin exactly on the left crop edge.
    lay_word(&mut gs, "EDGE", 50.0, 400.0, size);
    let crop = Rect::new(50.0, 50.0, 562.0, 742.0);
    let tp = textpage_from_glyphs_clipped(&gs, &[], letter(), 0, Some(crop));
    let text = to_text(&tp, defaults::TEXT);
    assert!(text.contains("EDGE"), "edge glyph dropped: {text:?}");
}
