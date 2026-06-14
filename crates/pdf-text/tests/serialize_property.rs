//! M2d serializer property tests (PRD §8.6.2, §10.7, §8.1): cross-mode
//! consistency + never-panic. Catalog IDs: `SERIAL-PROP-*`.

use pdf_core::geom::{Point, Rect};
use pdf_text::model::{BlockKind, WritingDir};
use pdf_text::serialize::{
    defaults, get_textbox, to_blocks, to_dict, to_html, to_json, to_text, to_words, to_xhtml,
    to_xml, DictBlock,
};
use pdf_text::{textpage_from_glyphs, PositionedGlyph};
use proptest::prelude::*;
use smol_str::SmolStr;

fn letter() -> Rect {
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

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

/// Whitespace-normalize: collapse runs of whitespace to one space, trim.
fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn serial_prop_001_words_concat_approx_text() {
    // "Hello world" on one line; words space-joined ≈ text whitespace-normalized.
    let mut gs = Vec::new();
    let mut x = 100.0;
    for c in "Hello world".chars() {
        gs.push(glyph(c, x, 700.0, 12.0));
        x += if c == ' ' { 4.0 } else { 7.0 };
    }
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let words_joined = to_words(&tp, defaults::WORDS)
        .iter()
        .map(|w| w.4.clone())
        .collect::<Vec<_>>()
        .join(" ");
    let text_norm = norm_ws(&to_text(&tp, defaults::TEXT));
    assert_eq!(norm_ws(&words_joined), text_norm);
}

#[test]
fn serial_prop_002_dict_counts_match_model() {
    let mut gs = Vec::new();
    let mut x = 100.0;
    for c in "abc".chars() {
        gs.push(glyph(c, x, 700.0, 12.0));
        x += 7.0;
    }
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let d = to_dict(&tp, false, defaults::DICT);

    let model_text_blocks = tp
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Text)
        .count();
    let dict_text_blocks = d
        .blocks
        .iter()
        .filter(|b| matches!(b, DictBlock::Text(_)))
        .count();
    assert_eq!(model_text_blocks, dict_text_blocks);

    for (mb, db) in
        tp.blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Text)
            .zip(d.blocks.iter().filter_map(|b| match b {
                DictBlock::Text(t) => Some(t),
                DictBlock::Image(_) => None,
            }))
    {
        assert_eq!(mb.lines.len(), db.lines.len());
        for (ml, dl) in mb.lines.iter().zip(&db.lines) {
            assert_eq!(ml.spans.len(), dl.spans.len());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// `SERIAL-PROP-003` — arbitrary small glyph lists never panic across every
    /// serializer, and `SERIAL-PROP-004` — json always parses.
    #[test]
    fn serial_prop_003_004_never_panic_json_parses(
        glyphs in proptest::collection::vec(
            (
                proptest::char::range('!', '~'),
                0.0f64..612.0,
                0.0f64..792.0,
                1.0f64..40.0,
            ),
            0..30,
        )
    ) {
        let gs: Vec<PositionedGlyph> = glyphs
            .into_iter()
            .map(|(c, x, y, size)| glyph(c, x, y, size))
            .collect();
        let tp = textpage_from_glyphs(&gs, &[], letter(), 0);

        // None of these may panic.
        let _ = to_text(&tp, defaults::TEXT);
        let _ = to_blocks(&tp, defaults::BLOCKS);
        let _ = to_words(&tp, defaults::WORDS);
        let _ = to_dict(&tp, false, defaults::DICT);
        let _ = to_dict(&tp, true, defaults::RAWDICT);
        let _ = to_html(&tp, defaults::HTML);
        let _ = to_xhtml(&tp, defaults::XHTML);
        let _ = to_xml(&tp, defaults::XML);
        let _ = get_textbox(&tp, Rect::new(0.0, 0.0, 300.0, 300.0));

        let j = to_json(&tp, false, defaults::JSON);
        prop_assert!(is_valid_json(&j), "json failed to parse: {j}");
        let rj = to_json(&tp, true, defaults::RAWJSON);
        prop_assert!(is_valid_json(&rj), "rawjson failed to parse: {rj}");
    }
}

// --- minimal JSON validity check (shared shape with serialize_unit) -------

fn is_valid_json(s: &str) -> bool {
    let b = s.as_bytes();
    let mut p = 0usize;
    skip_ws(b, &mut p);
    if !value(b, &mut p) {
        return false;
    }
    skip_ws(b, &mut p);
    p == b.len()
}
fn skip_ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && matches!(b[*p], b' ' | b'\t' | b'\n' | b'\r') {
        *p += 1;
    }
}
fn value(b: &[u8], p: &mut usize) -> bool {
    skip_ws(b, p);
    if *p >= b.len() {
        return false;
    }
    match b[*p] {
        b'{' => object(b, p),
        b'[' => array(b, p),
        b'"' => string(b, p),
        b't' => lit(b, p, "true"),
        b'f' => lit(b, p, "false"),
        b'n' => lit(b, p, "null"),
        _ => number(b, p),
    }
}
fn lit(b: &[u8], p: &mut usize, l: &str) -> bool {
    if b[*p..].starts_with(l.as_bytes()) {
        *p += l.len();
        true
    } else {
        false
    }
}
fn object(b: &[u8], p: &mut usize) -> bool {
    *p += 1;
    skip_ws(b, p);
    if *p < b.len() && b[*p] == b'}' {
        *p += 1;
        return true;
    }
    loop {
        skip_ws(b, p);
        if !string(b, p) {
            return false;
        }
        skip_ws(b, p);
        if *p >= b.len() || b[*p] != b':' {
            return false;
        }
        *p += 1;
        if !value(b, p) {
            return false;
        }
        skip_ws(b, p);
        if *p >= b.len() {
            return false;
        }
        match b[*p] {
            b',' => *p += 1,
            b'}' => {
                *p += 1;
                return true;
            }
            _ => return false,
        }
    }
}
fn array(b: &[u8], p: &mut usize) -> bool {
    *p += 1;
    skip_ws(b, p);
    if *p < b.len() && b[*p] == b']' {
        *p += 1;
        return true;
    }
    loop {
        if !value(b, p) {
            return false;
        }
        skip_ws(b, p);
        if *p >= b.len() {
            return false;
        }
        match b[*p] {
            b',' => *p += 1,
            b']' => {
                *p += 1;
                return true;
            }
            _ => return false,
        }
    }
}
fn string(b: &[u8], p: &mut usize) -> bool {
    if *p >= b.len() || b[*p] != b'"' {
        return false;
    }
    *p += 1;
    while *p < b.len() {
        match b[*p] {
            b'"' => {
                *p += 1;
                return true;
            }
            b'\\' => *p += 2,
            _ => *p += 1,
        }
    }
    false
}
fn number(b: &[u8], p: &mut usize) -> bool {
    let start = *p;
    if *p < b.len() && b[*p] == b'-' {
        *p += 1;
    }
    while *p < b.len() && b[*p].is_ascii_digit() {
        *p += 1;
    }
    if *p < b.len() && b[*p] == b'.' {
        *p += 1;
        while *p < b.len() && b[*p].is_ascii_digit() {
            *p += 1;
        }
    }
    if *p < b.len() && matches!(b[*p], b'e' | b'E') {
        *p += 1;
        if *p < b.len() && matches!(b[*p], b'+' | b'-') {
            *p += 1;
        }
        while *p < b.len() && b[*p].is_ascii_digit() {
            *p += 1;
        }
    }
    *p > start
}
