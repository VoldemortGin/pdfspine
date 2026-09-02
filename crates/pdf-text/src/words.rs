//! Word segmentation (M2c, PRD §8.6.2, §10.7).
//!
//! Splits each [`Line`] of a [`TextPage`] into [`Word`]s on whitespace
//! characters **only**. Word breaks that the PDF renders as a bare spatial gap
//! (`TJ`-kerned words with no space glyph) are already materialised by
//! [`crate::layout`], which synthesizes one space char at the shared
//! [`WORD_GAP_FRAC`] threshold and suppresses it inside letter-spaced (tracked)
//! runs. Segmenting on whitespace alone therefore keeps `get_text("words")`
//! boundaries identical to the text/dict/blocks output — a second, mask-less
//! spatial split here would re-shatter tracked headings that layout kept whole.
//! Produces the `(bbox, text, block_no, line_no, word_no)` tuples that drive
//! `get_text("words")` in M2d.

use pdf_core::geom::Rect;

use crate::model::{Char, Line, TextPage, Word};

/// Spatial-gap threshold as a fraction of font size. A gap between the right
/// edge of one glyph and the left edge of the next that exceeds `size *
/// WORD_GAP_FRAC` is a word break even without a literal space (PRD §8.6.2;
/// PyMuPDF uses ≈ 0.2–0.3× space width — we key off the font size, which is a
/// stable proxy across fonts).
///
/// Consumed by [`crate::layout`], whose line assembly synthesizes the inter-word
/// space at this threshold; this module then splits on that space like any
/// other, so text/dict/blocks word boundaries agree with `get_text("words")`.
pub(crate) const WORD_GAP_FRAC: f64 = 0.2;

/// A stable device-space font-size estimate from a line's glyph-cell heights —
/// the **median positive height**.
///
/// The word-gap threshold must be measured in the same space as the gaps it is
/// compared against. Glyph gaps live in device space (the bboxes are already
/// transformed by the page CTM), but the raw `Tf` operand carried on each
/// span/glyph is in *text* space: on PDFs that emit `Tf 1` and bake the real
/// scale into the text/CTM matrix (common in PMC/LaTeX output) the two diverge
/// by the CTM scale, collapsing the threshold and shattering words/URLs.
///
/// Keying off the median cell *height* makes the threshold invariant to where
/// the scale lives. The median (not the mean or a per-glyph height) ignores the
/// jitter of short glyphs like `.`/`,` whose cells are smaller than the body
/// text. Returns `0.0` when no positive height is available; callers fall back
/// to the raw operand size in that case.
pub(crate) fn effective_size_from_heights(heights: impl Iterator<Item = f64>) -> f64 {
    let mut hs: Vec<f64> = heights.filter(|h| *h > 0.0).collect();
    if hs.is_empty() {
        return 0.0;
    }
    hs.sort_by(f64::total_cmp);
    hs[hs.len() / 2]
}

/// Extracts every word of a [`TextPage`] in reading order (PRD §10.7).
#[must_use]
pub fn words(tp: &TextPage) -> Vec<Word> {
    let mut out = Vec::new();
    for block in &tp.blocks {
        for (line_no, line) in block.lines.iter().enumerate() {
            segment_line(line, block.number, line_no, &mut out);
        }
    }
    out
}

/// Segments one line into words on whitespace chars, appending to `out`.
///
/// Whitespace (literal space glyphs and the spaces [`crate::layout`] synthesized
/// from spatial gaps) is the *only* word boundary: no geometric re-splitting
/// happens here, so `words` can never disagree with the line text.
fn segment_line(line: &Line, block_no: usize, line_no: usize, out: &mut Vec<Word>) {
    let mut word_no = 0usize;
    let mut cur: Vec<&Char> = Vec::new();

    // Iterate the line's chars in advance order (spans are already ordered).
    for span in &line.spans {
        for ch in &span.chars {
            // A whitespace char terminates the current word and is not itself
            // part of any word.
            if is_word_separator(ch.c) {
                flush(&mut cur, block_no, line_no, &mut word_no, out);
                continue;
            }
            cur.push(ch);
        }
    }
    flush(&mut cur, block_no, line_no, &mut word_no, out);
}

/// Emits the accumulated chars as one [`Word`] (no-op when empty), advancing
/// `word_no`.
fn flush(
    cur: &mut Vec<&Char>,
    block_no: usize,
    line_no: usize,
    word_no: &mut usize,
    out: &mut Vec<Word>,
) {
    if cur.is_empty() {
        return;
    }
    let mut bbox = Rect::default();
    let mut text = String::with_capacity(cur.len());
    for ch in cur.iter() {
        bbox = bbox.union(&ch.bbox);
        text.push(ch.c);
    }
    out.push(Word {
        bbox,
        text,
        block_no,
        line_no,
        word_no: *word_no,
    });
    *word_no += 1;
    cur.clear();
}

/// Whether a char is a word separator: ASCII whitespace or NBSP (`0xA0`).
fn is_word_separator(c: char) -> bool {
    c.is_whitespace() || c == '\u{00A0}'
}
