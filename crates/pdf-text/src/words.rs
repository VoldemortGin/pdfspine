//! Word segmentation (M2c, PRD §8.6.2, §10.7).
//!
//! Splits each [`Line`] of a [`TextPage`] into [`Word`]s on whitespace
//! characters **only**. Word breaks that the PDF renders as a bare spatial gap
//! (`TJ`-kerned words with no space glyph) are already materialised by
//! [`crate::layout`], which synthesizes one space char at its word-gap
//! threshold (a fraction of the device-space font size) and suppresses it
//! inside letter-spaced (tracked) runs. Segmenting on whitespace alone
//! therefore keeps `get_text("words")` boundaries identical to the
//! text/dict/blocks output — a second, mask-less spatial split here would
//! re-shatter tracked headings that layout kept whole.
//! Produces the `(bbox, text, block_no, line_no, word_no)` tuples that drive
//! `get_text("words")` in M2d.

use pdf_core::geom::Rect;

use crate::model::{Char, Line, TextPage, Word};

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
