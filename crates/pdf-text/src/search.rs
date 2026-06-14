//! Searching a [`TextPage`] for a needle string (PyMuPDF `search_for`; PRD
//! §8.6).
//!
//! The page is flattened to a sequence of "search chars" (block → line → span →
//! char, in reading order, skipping image blocks). Comparison is
//! case-insensitive and Unicode-normalized (NFC + lowercasing), but the
//! returned geometry always comes from the original source [`Char`] bboxes — the
//! fold only affects *matching*, never the quads.
//!
//! A normalized fold can change the char count relative to the source (NFD
//! decomposition, `ß` → `ss`, case maps that grow), so we fold **per source
//! `Char`**: each source char contributes 0..n folded chars, and every folded
//! char remembers which source char (hence bbox + line identity) it came from.
//! A plain substring search of the folded needle over the folded page string
//! then maps matched folded ranges back to source chars for geometry.

use pdf_core::geom::{Quad, Rect};
use unicode_normalization::UnicodeNormalization;

use crate::model::{BlockKind, TextPage};

/// Options for [`search`] (PyMuPDF `search_for`).
///
/// The default is `hit_max: 0` (unlimited), `clip: None`, `quads: false`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOptions {
    /// Max number of hits to return; `0` = unlimited.
    pub hit_max: usize,
    /// Optional clip rect (device space); only hits intersecting it are kept.
    pub clip: Option<Rect>,
    /// When `false`, callers want the enclosing `Rect` of each hit (still
    /// returned as a `Quad` via `Quad::from_rect` — the caller flattens to
    /// `Rect`). When `true`, the per-line `Quad` is returned. We always return
    /// `Quad`s; the PyO3 layer converts to `Rect` when `quads == false`.
    pub quads: bool,
}

/// A flattened page char carrying its source geometry + line identity.
struct SearchChar {
    /// The source `Char` bbox (device space) — used verbatim for quad geometry.
    bbox: Rect,
    /// A running `(block_idx, line_idx)` identity, so multi-line matches can be
    /// split into per-line segments.
    line_id: (usize, usize),
}

/// Searches `tp` for `needle` (PyMuPDF `search_for` semantics).
///
/// Case-insensitive, Unicode-normalized (NFC) compare. A hit that spans
/// multiple lines yields one [`Quad`] per line segment (PyMuPDF behavior).
/// Returns hits in reading order. An empty `needle` returns an empty `Vec`.
#[must_use]
pub fn search(tp: &TextPage, needle: &str, opts: SearchOptions) -> Vec<Quad> {
    if needle.is_empty() {
        return Vec::new();
    }

    // Fold the needle the same way as the page (NFC + lowercase). An empty
    // folded needle (e.g. needle made only of chars that fold away) can never
    // be located, so bail out.
    let folded_needle = fold(needle);
    if folded_needle.is_empty() {
        return Vec::new();
    }

    // Flatten the page. `folded` is the concatenated comparison string; `owner`
    // maps each folded char (by char index) to the index of its source char in
    // `chars`. We intentionally insert NO separator between lines so a match can
    // cross a visual line break (PyMuPDF matches across the break; SEARCH-006).
    let mut chars: Vec<SearchChar> = Vec::new();
    let mut folded = String::new();
    let mut owner: Vec<usize> = Vec::new();

    for (b_idx, block) in tp.blocks.iter().enumerate() {
        if block.kind == BlockKind::Image {
            continue;
        }
        for (l_idx, line) in block.lines.iter().enumerate() {
            for span in &line.spans {
                for c in &span.chars {
                    let src_idx = chars.len();
                    chars.push(SearchChar {
                        bbox: c.bbox,
                        line_id: (b_idx, l_idx),
                    });
                    for fc in fold_char(c.c) {
                        folded.push(fc);
                        owner.push(src_idx);
                    }
                }
            }
        }
    }

    if folded.is_empty() {
        return Vec::new();
    }

    // Char-index views over the folded strings for a simple substring scan.
    let hay: Vec<char> = folded.chars().collect();
    let pat: Vec<char> = folded_needle.chars().collect();
    let pat_len = pat.len();

    let mut out: Vec<Quad> = Vec::new();
    let mut hits = 0usize;
    let mut start = 0usize;

    while start + pat_len <= hay.len() {
        let window = hay.get(start..start + pat_len);
        if window == Some(pat.as_slice()) {
            // Map the matched folded range back to its source chars, grouped by
            // line, producing one quad per distinct line in reading order.
            let match_quads = quads_for_match(&chars, &owner, start, pat_len, opts.clip);

            // A match counts toward `hit_max` only if it contributes geometry
            // after clip filtering.
            if !match_quads.is_empty() {
                out.extend(match_quads);
                hits += 1;
                if opts.hit_max != 0 && hits >= opts.hit_max {
                    break;
                }
            }
            // Non-overlapping search: advance past this match.
            start += pat_len;
        } else {
            start += 1;
        }
    }

    out
}

/// Builds the per-line quads for one folded match `[start .. start + len)`.
///
/// Matched folded chars are mapped to their owning source chars; consecutive
/// source chars sharing a `line_id` are unioned into one rect → one quad.
/// Quads whose enclosing rect does not intersect `clip` (when set) are dropped.
fn quads_for_match(
    chars: &[SearchChar],
    owner: &[usize],
    start: usize,
    len: usize,
    clip: Option<Rect>,
) -> Vec<Quad> {
    let mut quads: Vec<Quad> = Vec::new();
    let mut seg_line: Option<(usize, usize)> = None;
    let mut seg_rect = Rect::default();

    for k in start..start + len {
        let Some(&src_idx) = owner.get(k) else {
            continue;
        };
        let Some(sc) = chars.get(src_idx) else {
            continue;
        };
        match seg_line {
            Some(line) if line == sc.line_id => {
                seg_rect = seg_rect.union(&sc.bbox);
            }
            _ => {
                if seg_line.is_some() {
                    push_quad(&mut quads, seg_rect, clip);
                }
                seg_line = Some(sc.line_id);
                seg_rect = sc.bbox;
            }
        }
    }
    if seg_line.is_some() {
        push_quad(&mut quads, seg_rect, clip);
    }
    quads
}

/// Pushes `Quad::from_rect(&rect)` unless `clip` is set and the rect misses it.
fn push_quad(quads: &mut Vec<Quad>, rect: Rect, clip: Option<Rect>) {
    if let Some(c) = clip {
        if !rect.intersects(&c) {
            return;
        }
    }
    quads.push(Quad::from_rect(&rect));
}

/// Folds a whole string for comparison: NFC normalization + lowercasing.
fn fold(s: &str) -> String {
    s.nfc().flat_map(char::to_lowercase).nfc().collect()
}

/// Folds a single source char for comparison, preserving the source-char →
/// folded-char ownership. Lowercase first (may expand, e.g. `İ`), then NFC the
/// result so it matches [`fold`] applied to the needle.
fn fold_char(c: char) -> Vec<char> {
    c.to_lowercase().collect::<String>().nfc().collect()
}
