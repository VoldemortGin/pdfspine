//! Word segmentation (M2c, PRD §8.6.2, §10.7).
//!
//! Splits each [`Line`] of a [`TextPage`] into [`Word`]s on (a) literal
//! whitespace characters and (b) spatial gaps wider than a size-relative
//! threshold — the latter catches `TJ`-kerned words rendered without any space
//! character. Produces the `(bbox, text, block_no, line_no, word_no)` tuples
//! that drive `get_text("words")` in M2d.

use pdf_core::geom::Rect;

use crate::model::{Char, Line, TextPage, Word};

/// Spatial-gap threshold as a fraction of font size. A gap between the right
/// edge of one char and the left edge of the next that exceeds `size *
/// WORD_GAP_FRAC` starts a new word even without a literal space (PRD §8.6.2;
/// PyMuPDF uses ≈ 0.2–0.3× space width — we key off the font size, which is a
/// stable proxy across fonts).
///
/// Shared with [`crate::layout`], whose line assembly synthesizes an inter-word
/// space at the very same threshold — so text/dict/blocks word boundaries agree
/// with `get_text("words")`.
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

/// Segments one line into words, appending to `out`.
fn segment_line(line: &Line, block_no: usize, line_no: usize, out: &mut Vec<Word>) {
    let mut word_no = 0usize;
    let mut cur: Vec<&Char> = Vec::new();
    let mut prev_right: Option<f64> = None;

    // The gap threshold is keyed off a device-space size measure (the median
    // glyph-cell height) so it is invariant to whether the scale lives in `Tf`
    // or the CTM — see [`effective_size_from_heights`]. The fallback to the
    // first span's raw operand size only fires for degenerate lines with no
    // positive cell height, where the two spaces coincide anyway.
    let eff_size = {
        let h = effective_size_from_heights(
            line.spans
                .iter()
                .flat_map(|s| s.chars.iter())
                .map(|ch| ch.bbox.height()),
        );
        if h > 0.0 {
            h
        } else {
            line.spans.first().map_or(0.0, |s| s.size.abs())
        }
    };
    let thresh = eff_size * WORD_GAP_FRAC;

    // Iterate the line's chars in advance order (spans are already ordered).
    for span in &line.spans {
        for ch in &span.chars {
            // A literal whitespace char terminates the current word and is not
            // itself part of any word.
            if is_word_separator(ch.c) {
                flush(&mut cur, block_no, line_no, &mut word_no, out);
                prev_right = None;
                continue;
            }
            // A spatial gap larger than the threshold also splits.
            if let Some(pr) = prev_right {
                let gap = ch.bbox.x0 - pr;
                if gap > thresh {
                    flush(&mut cur, block_no, line_no, &mut word_no, out);
                }
            }
            prev_right = Some(ch.bbox.x1);
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
