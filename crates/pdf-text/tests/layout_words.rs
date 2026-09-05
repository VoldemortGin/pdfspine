//! M2c word-segmentation tests (PRD §8.6.2, §10.7). Catalog IDs: `WORDS-*`.

mod common;

use std::sync::Arc;

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_core::object::ObjRef;
use pdf_core::page::Page;
use pdf_core::{Limits, Object};
use pdf_text::model::WritingDir;
use pdf_text::serialize::textflags;
use pdf_text::serialize::to_text;
use pdf_text::{
    build_textpage, build_textpage_flagged, textpage_from_glyphs, words, PositionedGlyph, TextPage,
};
use smol_str::SmolStr;

use common::{
    dict, name_obj, raw_stream, rref, winansi_type1, winansi_type1_with_metrics, PageDoc,
};

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
        spacing_advance: (0.0, 0.0),
        ascender: 0.7,
        descender: -0.2,
        // Synthetic glyph: an upright Trm reproducing the origin + size, and a
        // cell whose quad through it is exactly `bbox`.
        text_matrix: Matrix::translate(ox, oy),
        ctm: Matrix::IDENTITY,
        render_matrix: Matrix::new(size, 0.0, 0.0, size, ox, oy),
        cell: Rect::new(0.0, -0.2, w / size, 0.7),
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

/// Like [`textpage_e2e`], but with a PyMuPDF `TEXT_*` flag set in force.
fn textpage_e2e_flagged(font: Object, content: &[u8], flags: u32) -> TextPage {
    let (doc, _page) = PageDoc::new().font("F1", font).content(content).open();
    let page = Page::new(Arc::new(doc), 0, ObjRef::new(3, 0));
    build_textpage_flagged(page.document(), &page, &Limits::unbounded_decode(), flags)
}

/// Every char cell on the page, in reading order.
fn chars(tp: &TextPage) -> Vec<(char, Rect)> {
    tp.blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .flat_map(|l| l.spans.iter())
        .flat_map(|s| s.chars.iter())
        .map(|c| (c.c, c.bbox))
        .collect()
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

#[test]
fn words_010_positive_descent_keeps_word_gap_threshold() {
    // eurlex pattern: `/Descent +250` once halved the glyph cell (0.5×size), so
    // the word-gap threshold (cell-height median × 0.2) fell to 1.2pt at 12pt
    // and a -120 kern (1.44pt) inside "extraction" was read as a word gap.
    // With the sign normalised the fixture must read exactly like its
    // well-formed (750, -250) twin: one word, no synthesized space.
    for descent in [250, -250] {
        let tp = textpage_e2e(
            winansi_type1_with_metrics("Helvetica", 32, &HELV, 750, descent),
            b"BT /F1 12 Tf 72 700 Td [(extr) -120 (action)] TJ ET",
        );
        assert_eq!(
            to_text(&tp, 0).trim_end(),
            "extraction",
            "Descent {descent}"
        );
        assert_eq!(word_texts(&tp), vec!["extraction"], "Descent {descent}");
    }
}

#[test]
fn words_011_short_cell_descriptor_keeps_kerned_word_whole() {
    // A well-formed but short glyph cell (`/Ascent 500 /Descent 0`, cell =
    // 0.5×size) is legal and must not shrink the word-gap threshold: keyed on
    // the cell height it collapsed to 0.1×size, so a -120 kern (1.44pt at
    // 12pt) inside "extraction" read as a word gap ("extr action"; PyMuPDF
    // keys on the device font size and reads "extraction").
    let tp = textpage_e2e(
        winansi_type1_with_metrics("Helvetica", 32, &HELV, 500, 0),
        b"BT /F1 12 Tf 72 700 Td [(extr) -120 (action)] TJ ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "extraction");
    assert_eq!(word_texts(&tp), vec!["extraction"]);
}

#[test]
fn words_012_word_gap_threshold_invariants() {
    // Re-keying the threshold on the device font size must not move the
    // boundaries of the plain cases: a literal space and a real `TJ` word gap
    // (-300 → 3.6pt) still split; ordinary kerning (-30) and a positive
    // adjustment that pulls the glyphs together (+250) never do.
    let cases: [(&[u8], &[&str]); 4] = [
        (
            b"BT /F1 12 Tf 72 700 Td (text extraction) Tj ET",
            &["text", "extraction"],
        ),
        (
            b"BT /F1 12 Tf 72 700 Td [(extr) -300 (action)] TJ ET",
            &["extr", "action"],
        ),
        (
            b"BT /F1 12 Tf 72 700 Td [(extr) -30 (action)] TJ ET",
            &["extraction"],
        ),
        (
            b"BT /F1 12 Tf 72 700 Td [(e) 250 (xtraction)] TJ ET",
            &["extraction"],
        ),
    ];
    for (content, expected) in cases {
        let tp = textpage_e2e(winansi_type1("Helvetica", 32, &HELV), content);
        let text = to_text(&tp, 0);
        let from_text: Vec<&str> = text.split_whitespace().collect();
        let src = String::from_utf8_lossy(content);
        assert_eq!(from_text, expected, "{src}");
        assert_eq!(word_texts(&tp), expected, "{src}");
    }
}

#[test]
fn words_013_threshold_is_device_space_tf1_tm_scale() {
    // The threshold is keyed on the *device* font size: `/F1 1 Tf` with the
    // 12pt scale baked into `Tm` must segment exactly like the plain `12 Tf`
    // form — a -300 kern splits, a -120 kern does not, in both spellings.
    let pairs: [(&[u8], &[u8], &[&str]); 2] = [
        (
            b"BT /F1 12 Tf 72 700 Td [(extr) -300 (action)] TJ ET",
            b"BT /F1 1 Tf 12 0 0 12 72 700 Tm [(extr) -300 (action)] TJ ET",
            &["extr", "action"],
        ),
        (
            b"BT /F1 12 Tf 72 700 Td [(extr) -120 (action)] TJ ET",
            b"BT /F1 1 Tf 12 0 0 12 72 700 Tm [(extr) -120 (action)] TJ ET",
            &["extraction"],
        ),
    ];
    for (plain, scaled, expected) in pairs {
        let a = textpage_e2e(winansi_type1("Helvetica", 32, &HELV), plain);
        let b = textpage_e2e(winansi_type1("Helvetica", 32, &HELV), scaled);
        let src = String::from_utf8_lossy(scaled);
        assert_eq!(word_texts(&a), expected, "{src}");
        assert_eq!(word_texts(&b), expected, "{src}");
        assert_eq!(to_text(&a, 0), to_text(&b, 0), "{src}");
    }
}

// === letter-spacing mask vs. genuine word gaps ==============================

/// Helvetica advance for a WinAnsi code (1000-unit glyph space).
fn helv_w(c: u8) -> i64 {
    HELV[(c - 32) as usize]
}

/// Every glyph its own `Tj`, positioned with a relative `Td` of exactly its
/// advance (letter gap 0); words separated by a `space_frac × size` advance.
fn per_glyph_words(words: &[&str], size: f64, space_frac: f64) -> String {
    let mut s = String::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            s.push_str(&format!("{:.4} 0 Td ", space_frac * size));
        }
        for c in w.bytes() {
            let adv = helv_w(c) as f64 * size / 1000.0;
            s.push_str(&format!("({}) Tj {adv:.4} 0 Td ", c as char));
        }
    }
    s
}

/// A `TJ` dot leader: `n` dots, each pulled `kern` (1/1000 em) apart.
fn dot_leader(n: usize, kern: i64) -> String {
    (0..n)
        .map(|i| {
            if i == 0 {
                "(.)".to_string()
            } else {
                format!(" {} (.)", -kern)
            }
        })
        .collect()
}

#[test]
fn words_014_positioned_toc_line_with_dot_leader_keeps_word_gaps() {
    // eurlex / govdocs TOC pattern: every word (or every glyph) positioned by
    // `Td`/`TJ` with no space glyph — letter gap 0, word gap ≈ one space width
    // (0.278×size) — followed by a long `-190` dot leader (dot gap 0.19×size)
    // and the page number. `Tc` is 0 throughout, so every gap here is pure
    // `TJ` kerning and nothing is deducted — the regression this pins is a
    // line-wide *median* heuristic that once read the leader as letter-spacing
    // and swallowed every real word gap:
    // "Originandscopeofrightofdeduction". PyMuPDF: the words split, the first
    // dot stays glued to "deduction" (gap 0), the remaining dots are separate
    // words, and the far-right page number starts a new line.
    let toc = ["Origin", "and", "scope", "of", "right", "of", "deduction"];
    let per_glyph = format!(
        "BT /F1 8.5 Tf 72 700 Td {} [{} -1500 (35)] TJ ET",
        per_glyph_words(&toc, 8.5, 0.278),
        dot_leader(40, 190)
    );
    let per_word_tj = format!(
        "BT /F1 1 Tf 8.5 0 0 8.5 72 700 Tm [(Or) 12 (igin) -337 (and) -337 (scope) -337 (of) -337 (r) 10 (ight) -337 (of) -337 (deduction)] TJ [{} -1500 (35)] TJ ET",
        dot_leader(40, 190)
    );
    for content in [per_glyph.as_bytes(), per_word_tj.as_bytes()] {
        let tp = textpage_e2e(winansi_type1("Helvetica", 32, &HELV), content);
        let src = String::from_utf8_lossy(content);
        let text = to_text(&tp, 0);
        let first_line = text.lines().next().unwrap_or("");
        assert!(
            first_line.starts_with("Origin and scope of right of deduction. . . ."),
            "{src}\n{text:?}"
        );
        let ws = word_texts(&tp);
        assert_eq!(
            &ws[..7],
            &["Origin", "and", "scope", "of", "right", "of", "deduction."],
            "{src}"
        );
        assert_eq!(ws.last().map(String::as_str), Some("35"), "{src}");
        assert_eq!(ws.len(), 7 + 39 + 1, "{src}: {ws:?}");
        assert!(ws[7..46].iter().all(|w| w == "."), "{src}: {ws:?}");
    }
}

#[test]
fn words_015_tc_dot_leader_is_not_letter_spacing() {
    // govdocs1-00000 pattern: `Tf 1` + `Tm` scale 8, `TJ` word gaps of -332.7
    // (2.66pt, no space glyph), then a `0.2219 Tc` dot leader (dot gap 1.78pt
    // = 0.22×size) and right-aligned figures. The leader's uniform gap is not
    // tracking that holds a word together — it joins no letters at all — so it
    // is never deducted: the label splits into words and each dot is its own
    // word (PyMuPDF); the figures start new lines (gap ≫ size).
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 1 Tf 8 0 0 8 38.9 700 Tm [(Under) -332.7 (5) -332.7 (years)] TJ 6.8494 0 TD 0.2219 Tc [(...............................) -5034.5 (8) 221.9 (8) -2888 (6) 221.9 (.) 221.9 (1)] TJ ET",
    );
    let mut expected: Vec<&str> = vec!["Under", "5", "years"];
    expected.extend(std::iter::repeat_n(".", 31));
    expected.extend(["88", "6.1"]);
    assert_eq!(word_texts(&tp), expected);
    let text = to_text(&tp, 0);
    assert!(text.starts_with("Under 5 years . . . ."), "{text:?}");
}

#[test]
fn words_016_tracked_heading_keeps_word_gap_before_number() {
    // Genuine letter-spacing: `2.5 Tc` at 12pt tracks every letter of
    // "Abschnitt" 2.5pt apart (0.21×size, uniform, narrower than a space);
    // the `-400` kern before "2" opens a clearly wider gap (7.3pt). `Tc`
    // explains the letter gaps down to nothing and leaves the kern's 4.8pt
    // standing. (Product choice: PyMuPDF, blind to `Tc`, shatters the heading.)
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 72 700 Td 2.5 Tc [(Abschnitt) -400 (2)] TJ ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "Abschnitt 2");
    assert_eq!(word_texts(&tp), vec!["Abschnitt", "2"]);
}

#[test]
fn words_017_tc_tracking_with_literal_space_stays_two_words() {
    // `3 Tc` tracking (0.25×size) around a literal space: the space glyph
    // breaks the tracked run, so both words collapse and the boundary holds.
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 72 700 Td 3 Tc (text extraction) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "text extraction");
    assert_eq!(word_texts(&tp), vec!["text", "extraction"]);
}

#[test]
fn words_018_tracking_at_threshold_keeps_kern_loosened_pair_whole() {
    // eurlex body text: `0.15 Tc` at `Tf 1`/`Tm 9.59` tracks every letter by
    // exactly the word-gap threshold, and a `-30` kern loosens one pair inside
    // "transport" a little further (0.18×size). Deducting the tracking leaves
    // 0.03×size — not a word break — so the word stays whole (PyMuPDF reads
    // "transpor t"; the goal is whole words, not oracle parity).
    let tp = textpage_e2e(
        winansi_type1_with_metrics("Helvetica", 32, &HELV, 750, -250),
        b"BT /F1 1 Tf 9.59 0 0 9.59 72 700 Tm 0.15 Tc [(transpor) -30 (t)] TJ ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "transport");
    assert_eq!(word_texts(&tp), vec!["transport"]);
}

#[test]
fn words_029_tc_heading_kern_is_not_a_word_break() {
    // EUR-Lex sub-heading: `0.1499 Tc` at `Tf 1`/`Tm 9.59` sets every letter
    // gap to 1.43750 against a 1.43850 threshold - one part in ten thousand
    // below it - so the smallest kern tips a pair over and cuts the word in
    // half. That is exactly how PyMuPDF reads these 33 pages (`proper ty`,
    // `ser vices`, `Def inition`). Judging the *residual* gap (1.19875 of pure
    // `TJ` kern, the tracking deducted) keeps the word whole...
    let tp = textpage_e2e(
        winansi_type1_with_metrics("Helvetica", 32, &HELV, 750, -250),
        b"BT /F1 1 Tf 9.59 0 0 9.59 72 700 Tm 0.1499 Tc [(proper) -125 (ty)] TJ ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "property");
    assert_eq!(word_texts(&tp), vec!["property"]);

    // ...while a genuine word gap on the very same line - `-337`, the TOC's
    // idiom, three times the threshold once the tracking is deducted - still
    // splits.
    let tp = textpage_e2e(
        winansi_type1_with_metrics("Helvetica", 32, &HELV, 750, -250),
        b"BT /F1 1 Tf 9.59 0 0 9.59 72 700 Tm 0.1499 Tc [(Origin) -337 (and) -337 (scope)] TJ ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "Origin and scope");
    assert_eq!(word_texts(&tp), vec!["Origin", "and", "scope"]);
}

#[test]
fn words_030_tracking_deduction_is_device_space_and_directional() {
    // The deducted share is a vector carried through `Tm`/`CTM`, not a scalar.
    //
    // Rotated 90 degrees: the same `0.1499 Tc` heading painted up the page. The
    // tracking now lies on device *y*; projecting it onto the line's own
    // reading axis is what keeps `property` whole (an x-only reading would
    // deduct nothing and cut it).
    let tp = textpage_e2e(
        winansi_type1_with_metrics("Helvetica", 32, &HELV, 750, -250),
        b"BT /F1 1 Tf 0 9.59 -9.59 0 300 300 Tm 0.1499 Tc [(proper) -125 (ty)] TJ ET",
    );
    assert_eq!(word_texts(&tp), vec!["property"]);

    // `50 Tz` halves both the advance and the `Tc` share: `5 Tc` opens 2.5pt
    // per letter at 12pt - past the 1.8pt threshold on its own - and a `-600`
    // kern opens 3.6pt more. Deducting the *scaled* 2.5 holds each word
    // together and still leaves the kern standing. Read `Tc` unscaled and 5.0
    // is past the tracking bound, so nothing is deducted and `text` shatters.
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 50 Tz 5 Tc 72 700 Td [(text) -600 (block)] TJ ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "text block");
    assert_eq!(word_texts(&tp), vec!["text", "block"]);
}

#[test]
fn words_031_tracking_deduction_is_bounded() {
    // Two bounds keep the deduction to tracking that is holding a word
    // together. A `Tc` wider than a word space is a row of separately-read
    // tokens - a map label set as `O R E G O N` (0.5 em) - and stays split,
    // as PyMuPDF reads it...
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 6 Tc 72 700 Td (OREGON) Tj ET",
    );
    assert_eq!(
        word_texts(&tp),
        vec!["O", "R", "E", "G", "O", "N"],
        "0.5 em tracking is a row of tokens"
    );
    // ...while 0.25 em, narrower than Helvetica's own space, is tracking and
    // collapses (the WORDS-007 product choice, now per glyph).
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 3 Tc 72 700 Td (OREGON) Tj ET",
    );
    assert_eq!(word_texts(&tp), vec!["OREGON"]);

    // And a run of punctuation at the same `Tc` joins no letters, so nothing
    // is deducted there: a `* * *` rule stays three tokens.
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 3 Tc 72 700 Td (***) Tj ET",
    );
    assert_eq!(word_texts(&tp), vec!["*", "*", "*"]);
}

#[test]
fn words_032_synthesized_space_cell_spans_the_seam() {
    // A synthesized word space stands for a real gap but carried a zero-width
    // cell at the next word's origin, so `rawdict` reported a seam of nothing
    // where MuPDF reports the seam it actually bridges (it hands its synthetic
    // space the pen position it left off at and the new glyph's origin). Here
    // the `-600` kern at 12pt opens 7.2pt between `B` and `C`.
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 12 Tf 72 700 Td [(AB) -600 (CD)] TJ ET",
    );
    let cs = chars(&tp);
    let texts: String = cs.iter().map(|(c, _)| *c).collect();
    assert_eq!(texts, "AB CD");
    let (_, b_cell) = cs[1];
    let (_, space) = cs[2];
    let (_, c_cell) = cs[3];
    assert!(
        (space.x1 - space.x0 - 7.2).abs() < 1e-6,
        "space cell {space:?} should be 7.2 wide"
    );
    assert!(
        (space.x0 - b_cell.x1).abs() < 1e-6,
        "{space:?} vs {b_cell:?}"
    );
    assert!(
        (space.x1 - c_cell.x0).abs() < 1e-6,
        "{space:?} vs {c_cell:?}"
    );
    // The cell is as tall as the glyph it precedes, and no bbox grew.
    assert!((space.y0 - c_cell.y0).abs() < 1e-6);
    assert!((space.y1 - c_cell.y1).abs() < 1e-6);
}

#[test]
fn words_033_inhibit_spaces_suppresses_gap_synthesis() {
    // `TEXT_INHIBIT_SPACES` (8) is MuPDF's "give me only the whitespace the
    // page actually paints" -- the flag was defined here but nothing consumed
    // it. The `-600` kern that reads as a word gap by default now synthesizes
    // nothing, so the two words run together...
    let kerned: &[u8] = b"BT /F1 12 Tf 72 700 Td [(AB) -600 (CD)] TJ ET";
    let tp = textpage_e2e_flagged(
        winansi_type1("Helvetica", 32, &HELV),
        kerned,
        textflags::INHIBIT_SPACES,
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "ABCD");
    assert_eq!(word_texts(&tp), vec!["ABCD"]);

    // ...while a *literal* space glyph is untouched: the flag inhibits
    // synthesis, it does not strip whitespace.
    let literal: &[u8] = b"BT /F1 12 Tf 72 700 Td (AB CD) Tj ET";
    let tp = textpage_e2e_flagged(
        winansi_type1("Helvetica", 32, &HELV),
        literal,
        textflags::INHIBIT_SPACES,
    );
    assert_eq!(word_texts(&tp), vec!["AB", "CD"]);

    // Without the flag the kerned pair splits, as WORDS-002 and friends show.
    let tp = textpage_e2e(winansi_type1("Helvetica", 32, &HELV), kerned);
    assert_eq!(word_texts(&tp), vec!["AB", "CD"]);
}

// === missing /Widths: the advance fallback chain feeds the layout ===========

/// A non-embedded, non-standard TrueType font (`ABCDEF+Calibri`) with **no**
/// `/Widths` and a non-serif descriptor (Flags 32, no `/MissingWidth`): the
/// mapper falls back to the Helvetica substitute metrics, as MuPDF does.
fn calibri_nowidths() -> Object {
    let descriptor = Object::Dictionary(dict([
        ("Type", name_obj("FontDescriptor")),
        ("FontName", name_obj("ABCDEF+Calibri")),
        ("Flags", Object::Integer(32)),
        ("ItalicAngle", Object::Integer(0)),
        ("Ascent", Object::Integer(750)),
        ("Descent", Object::Integer(-250)),
        ("StemV", Object::Integer(88)),
    ]));
    Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("ABCDEF+Calibri")),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FontDescriptor", descriptor),
    ]))
}

/// A Core-14 Times-Roman WinAnsi font without `/Widths` (the fintabnet /
/// govdocs pattern where `\222` quoteright once advanced 0).
fn times_nowidths() -> Object {
    Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Times-Roman")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ]))
}

fn line_count(tp: &TextPage) -> usize {
    tp.blocks.iter().map(|b| b.lines.len()).sum()
}

/// Every non-blank char cell on the page has a positive device width (a
/// synthesized word-gap space is a zero-width marker, so blanks are skipped).
fn assert_all_cells_positive(tp: &TextPage) {
    for line in tp.blocks.iter().flat_map(|b| b.lines.iter()) {
        for c in line.spans.iter().flat_map(|s| s.chars.iter()) {
            if c.c.is_whitespace() {
                continue;
            }
            assert!(
                c.bbox.x1 - c.bbox.x0 > 0.0,
                "char {:?} has a zero-width cell",
                c.c
            );
        }
    }
}

#[test]
fn words_019_nowidths_truetype_per_glyph_td_keeps_word_whole() {
    // Each letter its own `Tj` positioned by a `Td` of its Helvetica advance;
    // the font has no `/Widths`, so every cell once had width 0 and every `Td`
    // read as a word gap ("e x t r a c t i o n"). With the substitute metrics
    // the cells touch and the word stays whole (PyMuPDF: "extraction").
    for word in ["extraction", "minimum"] {
        let content = format!(
            "BT /F1 12 Tf 72 700 Td {} ET",
            per_glyph_words(&[word], 12.0, 0.278)
        );
        let tp = textpage_e2e(calibri_nowidths(), content.as_bytes());
        assert_eq!(to_text(&tp, 0).trim_end(), word);
        assert_eq!(word_texts(&tp), vec![word]);
        assert_eq!(line_count(&tp), 1, "{word}");
        assert_all_cells_positive(&tp);
    }
}

#[test]
fn words_020_nowidths_truetype_per_word_td_splits_words_once() {
    // Two words each their own `Tj`, the second positioned by a `Td` that lands
    // it one space past the first (`text` = 22.7pt at 12pt + a 4pt gap): with
    // zero-width cells the gap was a line break; now it is exactly one word gap.
    let tp = textpage_e2e(
        calibri_nowidths(),
        b"BT /F1 12 Tf 72 700 Td (text) Tj 26.7 0 Td (extraction) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "text extraction");
    assert_eq!(word_texts(&tp), vec!["text", "extraction"]);
    assert_eq!(line_count(&tp), 1);
    assert_all_cells_positive(&tp);
}

#[test]
fn words_021_std14_nowidths_winansi_quotes_advance() {
    // Times-Roman without `/Widths`: WinAnsi 0x92 (quoteright), 0x93/0x94
    // (double quotes) and 0x96 (endash) have no ASCII AFM entry and once
    // advanced 0, so the next glyph overprinted the quote and the word split
    // ("Company’ s"). The high-punctuation AFM row restores the advance.
    let tp = textpage_e2e(
        times_nowidths(),
        b"BT /F1 12 Tf 72 700 Td (Company\\222s report \\223quoted\\224 \\226 dash) Tj ET",
    );
    assert_eq!(
        to_text(&tp, 0).trim_end(),
        "Company\u{2019}s report \u{201C}quoted\u{201D} \u{2013} dash"
    );
    assert_eq!(
        word_texts(&tp),
        vec![
            "Company\u{2019}s",
            "report",
            "\u{201C}quoted\u{201D}",
            "\u{2013}",
            "dash"
        ]
    );
    assert_all_cells_positive(&tp);
}

#[test]
fn words_022_std14_nowidths_quoteright_per_word_td() {
    // The fintabnet AIZ pattern: `(Company\222s) Tj` then a `Td` to the next
    // word. A zero-width quoteright made `s` overlap the quote and the word
    // read as "Company’ s"; with the AFM advance it is one word.
    let tp = textpage_e2e(
        times_nowidths(),
        b"BT /F1 12 Tf 72 700 Td (Company\\222s) Tj 60 0 Td (report) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "Company\u{2019}s report");
    assert_eq!(word_texts(&tp), vec!["Company\u{2019}s", "report"]);
    assert_eq!(line_count(&tp), 1);
}

// === Type3 fonts: `/Widths` live in the `/FontMatrix` glyph space ==========

/// A pdfTeX-like Type3 font (no descriptor): `FontMatrix 0.01204`, `/Widths`
/// for `a..z` are the Helvetica advances ÷ 12.04 (i.e. in that glyph space),
/// real glyph names via `/Differences`, every CharProc the same `40 0 d0`
/// stream. Returns the builder (the stream is an indirect object) + the font.
fn type3_helv() -> (PageDoc, Object) {
    let mut d = PageDoc::new();
    let proc_num = d.add(raw_stream([], b"40 0 d0"));
    let mut charprocs = dict([]);
    let mut diffs = vec![Object::Integer(97)];
    let mut widths = Vec::new();
    for c in b'a'..=b'z' {
        let gname = (c as char).to_string();
        charprocs.insert(pdf_core::Name::new(&gname), rref(proc_num, 0));
        diffs.push(name_obj(&gname));
        widths.push(Object::Real(helv_w(c) as f64 / 12.04));
    }
    let font = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type3")),
        (
            "FontBBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(-20),
                Object::Integer(80),
                Object::Integer(70),
            ]),
        ),
        (
            "FontMatrix",
            Object::Array(vec![
                Object::Real(0.01204),
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(0.01204),
                Object::Integer(0),
                Object::Integer(0),
            ]),
        ),
        ("CharProcs", Object::Dictionary(charprocs)),
        (
            "Encoding",
            Object::Dictionary(dict([
                ("Type", name_obj("Encoding")),
                ("Differences", Object::Array(diffs)),
            ])),
        ),
        ("FirstChar", Object::Integer(97)),
        ("LastChar", Object::Integer(122)),
        ("Widths", Object::Array(widths)),
        ("Resources", Object::Dictionary(dict([]))),
    ]));
    (d, font)
}

fn textpage_type3(content: &[u8]) -> TextPage {
    let (d, font) = type3_helv();
    let (doc, _page) = d.font("F1", font).content(content).open();
    let page = Page::new(Arc::new(doc), 0, ObjRef::new(3, 0));
    build_textpage(page.document(), &page, &Limits::unbounded_decode())
}

/// Device width of the first char cell showing `c`.
fn first_cell_width(tp: &TextPage, c: char) -> f64 {
    tp.blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .flat_map(|l| l.spans.iter())
        .flat_map(|s| s.chars.iter())
        .find(|ch| ch.c == c)
        .map(|ch| ch.bbox.x1 - ch.bbox.x0)
        .unwrap_or_else(|| panic!("no char {c:?} on the page"))
}

#[test]
fn words_023_type3_fontmatrix_kerned_tj_keeps_word_whole() {
    // `[(extr) -30 (action)] TJ` @12pt: with `/Widths` read as 1000-unit
    // values the cells were 12× too narrow, every glyph sat in a gap and the
    // word shattered. Through the FontMatrix 'e' is 556/1000×12 ≈ 6.67 wide
    // and the −30 kern stays a kern (PyMuPDF: "extraction").
    let tp = textpage_type3(b"BT /F1 12 Tf 72 700 Td [(extr) -30 (action)] TJ ET");
    assert_eq!(to_text(&tp, 0).trim_end(), "extraction");
    assert_eq!(word_texts(&tp), vec!["extraction"]);
    assert_eq!(line_count(&tp), 1);
    let e = first_cell_width(&tp, 'e');
    assert!(
        (e - 6.672).abs() < 0.05,
        "'e' cell width {e}, expected ≈ 6.67"
    );
}

#[test]
fn words_024_type3_fontmatrix_per_word_td_one_line() {
    // `(text) Tj 26.7 0 Td (extraction) Tj`: `text` advances 19.3pt, so the
    // `Td` opens one ~7pt word gap. Narrow cells turned that into a line
    // break; now it is exactly one word gap on one line (PyMuPDF).
    let tp = textpage_type3(b"BT /F1 12 Tf 72 700 Td (text) Tj 26.7 0 Td (extraction) Tj ET");
    assert_eq!(to_text(&tp, 0).trim_end(), "text extraction");
    assert_eq!(word_texts(&tp), vec!["text", "extraction"]);
    assert_eq!(line_count(&tp), 1);
    assert_all_cells_positive(&tp);
}

#[test]
fn words_025_type3_fontmatrix_per_glyph_td_keeps_word_whole() {
    // Each letter its own `Tj` + a `Td` of its Helvetica advance: the mapped
    // cells touch, so the letters form one word (PyMuPDF: "extraction").
    let content = format!(
        "BT /F1 12 Tf 72 700 Td {} ET",
        per_glyph_words(&["extraction"], 12.0, 0.278)
    );
    let tp = textpage_type3(content.as_bytes());
    assert_eq!(to_text(&tp, 0).trim_end(), "extraction");
    assert_eq!(word_texts(&tp), vec!["extraction"]);
    assert_eq!(line_count(&tp), 1);
    assert_all_cells_positive(&tp);
}

// === whitespace glyphs from another run overlapping ink ======================

/// Two independent text objects (Helvetica 12pt) on one page.
fn textpage_two_runs(a: &str, b: &str) -> TextPage {
    let content = format!("BT /F1 12 Tf {a} ET BT /F1 12 Tf {b} ET");
    textpage_e2e(winansi_type1("Helvetica", 32, &HELV), content.as_bytes())
}

fn char_count(tp: &TextPage) -> usize {
    tp.blocks
        .iter()
        .flat_map(|b| b.lines.iter())
        .flat_map(|l| l.spans.iter())
        .map(|s| s.chars.len())
        .sum()
}

#[test]
fn words_026_overlapping_phantom_space_from_other_run_is_dropped() {
    // GAO letterhead pattern: Word emits an empty paragraph `( ) Tj` whose
    // baseline clusters with the heading and whose space cell lands *inside*
    // the heading's ink. "United States Government" at 12pt Helvetica puts
    // "Stat|es" at x = 131.4; a space run at x = 129 (cell [129, 132.3])
    // overlaps both `t` and `e`, and the advance sort used to insert it there
    // ("United Stat es"). PyMuPDF: "United States Government".
    let heading = "72 700 Td (United States Government) Tj";
    let tp = textpage_two_runs(heading, "129 700 Td ( ) Tj");
    assert_eq!(to_text(&tp, 0).trim_end(), "United States Government");
    assert_eq!(word_texts(&tp), vec!["United", "States", "Government"]);
    // The phantom is gone from the char array too (rawdict consistency).
    assert_eq!(char_count(&tp), "United States Government".len());

    // Two phantom spaces `(  ) Tj`: the second lands inside `e` ("Stat e s").
    let tp = textpage_two_runs(heading, "129 700 Td (  ) Tj");
    assert_eq!(to_text(&tp, 0).trim_end(), "United States Government");
    assert_eq!(word_texts(&tp), vec!["United", "States", "Government"]);

    // A phantom straddling the end of "States" (`s` ends at 144.0) and the real
    // word space [144.0, 147.4]: no double space, still three words.
    let tp = textpage_two_runs(heading, "142 700 Td ( ) Tj");
    assert_eq!(to_text(&tp, 0).trim_end(), "United States Government");
    assert_eq!(word_texts(&tp), vec!["United", "States", "Government"]);
    assert_eq!(char_count(&tp), "United States Government".len());

    // The real GAO geometry: 10pt, the empty paragraph 2.1pt *below* the
    // heading baseline (still within the line tolerance), its cell inside `s`.
    let tp = textpage_e2e(
        winansi_type1("Helvetica", 32, &HELV),
        b"BT /F1 10 Tf 56.16 690.3 Td (United States Government Accountability Office) Tj ET \
          BT /F1 10 Tf 111.6 688.2 Td ( ) Tj ET",
    );
    assert_eq!(
        to_text(&tp, 0).trim_end(),
        "United States Government Accountability Office"
    );
    assert_eq!(
        word_texts(&tp),
        vec!["United", "States", "Government", "Accountability", "Office"]
    );
}

#[test]
fn words_027_real_spaces_touching_their_neighbours_are_kept() {
    // Invariant: a literal space whose cell merely *touches* (or is kerned /
    // tracked slightly under) its neighbours is a real word gap.
    let helv = || winansi_type1("Helvetica", 32, &HELV);
    // Single show string, gap exactly 0 on both sides.
    let tp = textpage_e2e(helv(), b"BT /F1 12 Tf 72 700 Td (United States) Tj ET");
    assert_eq!(to_text(&tp, 0).trim_end(), "United States");
    assert_eq!(word_texts(&tp), vec!["United", "States"]);

    // Three runs on one baseline, the space run placed exactly at the end of
    // "United" (106.02) and "States" exactly after it (109.356): one space.
    let tp = textpage_e2e(
        helv(),
        b"BT /F1 12 Tf 72 700 Td (United) Tj ET \
          BT /F1 12 Tf 106.02 700 Td ( ) Tj ET \
          BT /F1 12 Tf 109.356 700 Td (States) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "United States");
    assert_eq!(word_texts(&tp), vec!["United", "States"]);
    assert_eq!(char_count(&tp), "United States".len());

    // Tight tracking (`-0.8 Tc` at 10pt = −0.08 em: every cell overlaps the
    // previous one) and negative word spacing (`-1.5 Tw`): PyMuPDF keeps the
    // literal space in both, so must we.
    let tp = textpage_e2e(
        helv(),
        b"BT /F1 10 Tf 72 700 Td -0.8 Tc (United States) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "United States");
    assert_eq!(word_texts(&tp), vec!["United", "States"]);
    let tp = textpage_e2e(
        helv(),
        b"BT /F1 10 Tf 72 700 Td -1.5 Tw (United States) Tj ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "United States");
    assert_eq!(word_texts(&tp), vec!["United", "States"]);

    // EUR-Lex footnote marker: `(` then, at the superscript size, a space with
    // a kern *back* over its full width and the digit painted on top of it; the
    // same again before `)`. Both spaces are completely covered by ink, but
    // they are painted in sequence with their neighbours, so they are real —
    // PyMuPDF reads "( 1 )" as three words and so did we before this rule.
    let tp = textpage_e2e(
        helv(),
        b"BT /F1 9.5 Tf 72 700 Td (\\() Tj /F1 6.2 Tf [( ) 278 (1)] TJ /F1 9.5 Tf [( ) 278 (\\))] TJ ET",
    );
    assert_eq!(to_text(&tp, 0).trim_end(), "( 1 )");
    assert_eq!(word_texts(&tp), vec!["(", "1", ")"]);
}

#[test]
fn words_028_overprinted_ink_glyphs_are_unchanged() {
    // Fake bold: `(Bold) Tj` painted twice, 0.3pt apart. Only *whitespace*
    // glyphs are subject to the overlap rule; ink-on-ink overlap keeps the
    // pre-existing advance-sorted merge (PyMuPDF reads it as two lines,
    // "Bold\nBold" — a separate, known divergence).
    let tp = textpage_two_runs("72 700 Td (Bold) Tj", "72.3 700 Td (Bold) Tj");
    assert_eq!(to_text(&tp, 0).trim_end(), "BBoolldd");
    assert_eq!(word_texts(&tp), vec!["BBoolldd"]);
    assert_eq!(char_count(&tp), 8);
    assert_eq!(line_count(&tp), 1);
}
