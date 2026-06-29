//! M2d `get_text` serializer unit tests (PRD §8.6.2, §10.7): TEXTFLAGS values
//! and per-method defaults, plain text, get_textbox, blocks, words, dict and
//! rawdict tree, json and rawjson. Self-built glyph lists in PDF user space via
//! `textpage_from_glyphs` (no PyMuPDF files). Catalog IDs: TEXTFLAGS-*,
//! SERIAL-TEXT-*, SERIAL-TEXTBOX-*, SERIAL-BLOCKS-*, SERIAL-WORDS-*, DICT-*,
//! RAWDICT-*, JSON-*.

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_text::model::{Block, BlockKind, ImageBlock, WritingDir};
use pdf_text::serialize::{
    defaults, get_textbox, textflags, to_blocks, to_dict, to_json, to_text, to_words, DictBlock,
};
use pdf_text::{textpage_from_glyphs, ImageRef, PositionedGlyph, TextPage};
use smol_str::SmolStr;

const EPS: f64 = 1e-6;

fn letter() -> Rect {
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

/// A horizontal-writing glyph in PDF user space (origin bottom-left).
fn glyph(c: &str, ox: f64, oy: f64, size: f64, w: f64) -> PositionedGlyph {
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

/// Lays out a run of chars on one baseline at user-y `oy`, advancing `w` per
/// glyph (literal spaces become separator glyphs).
fn line_glyphs(text: &str, x0: f64, oy: f64, size: f64, w: f64) -> Vec<PositionedGlyph> {
    let mut gs = Vec::new();
    let mut x = x0;
    for c in text.chars() {
        gs.push(glyph(&c.to_string(), x, oy, size, w));
        x += w;
    }
    gs
}

// === TEXTFLAGS values + defaults =========================================

#[test]
fn textflags_value_001_bit_values() {
    assert_eq!(textflags::PRESERVE_LIGATURES, 1);
    assert_eq!(textflags::PRESERVE_WHITESPACE, 2);
    assert_eq!(textflags::PRESERVE_IMAGES, 4);
    assert_eq!(textflags::INHIBIT_SPACES, 8);
    assert_eq!(textflags::DEHYPHENATE, 16);
    assert_eq!(textflags::PRESERVE_SPANS, 32);
    assert_eq!(textflags::MEDIABOX_CLIP, 64);
    assert_eq!(textflags::CID_FOR_UNKNOWN, 128);
}

#[test]
fn textflags_default_001_text_blocks_words() {
    assert_eq!(defaults::TEXT, 1 | 2 | 64);
    assert_eq!(defaults::TEXT, 67);
    assert_eq!(defaults::BLOCKS, 67);
    assert_eq!(defaults::WORDS, 67);
}

#[test]
fn textflags_default_002_dict_family() {
    assert_eq!(defaults::DICT, 1 | 2 | 4 | 64);
    assert_eq!(defaults::DICT, 71);
    assert_eq!(defaults::RAWDICT, 71);
    assert_eq!(defaults::JSON, 71);
    assert_eq!(defaults::RAWJSON, 71);
}

#[test]
fn textflags_default_003_html_xhtml() {
    assert_eq!(defaults::HTML, 71);
    assert_eq!(defaults::XHTML, 71);
}

#[test]
fn textflags_default_004_xml() {
    assert_eq!(defaults::XML, 1 | 2 | 64);
    assert_eq!(defaults::XML, 67);
}

// === plain text ==========================================================

#[test]
fn serial_text_001_words_joined_line_newline() {
    let gs = line_glyphs("Hi there", 100.0, 700.0, 12.0, 7.0);
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let txt = to_text(&tp, defaults::TEXT);
    assert!(txt.starts_with("Hi there"), "got {txt:?}");
    assert!(txt.ends_with('\n'));
}

#[test]
fn serial_text_002_two_lines_in_block() {
    // Two lines, small vertical gap → one block.
    let mut gs = line_glyphs("aaa", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("bbb", 100.0, 688.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    assert_eq!(
        tp.blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Text)
            .count(),
        1
    );
    let txt = to_text(&tp, defaults::TEXT);
    assert_eq!(txt, "aaa\nbbb\n");
}

#[test]
fn serial_text_003_two_blocks_blank_line() {
    // Big vertical gap → two blocks → blank line between.
    let mut gs = line_glyphs("aaa", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("bbb", 100.0, 500.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    assert_eq!(tp.blocks.len(), 2);
    let txt = to_text(&tp, defaults::TEXT);
    assert_eq!(txt, "aaa\nbbb\n");
    // Each block ends with its own `\n` → the two blocks are "aaa\n" + "bbb\n".
    assert_eq!(txt.matches('\n').count(), 2);
}

#[test]
fn serial_text_004_empty_page() {
    let tp = textpage_from_glyphs(&[], &[], letter(), 0);
    assert_eq!(to_text(&tp, defaults::TEXT), "");
}

#[test]
fn serial_text_005_hyphen_kept_by_default() {
    let mut gs = line_glyphs("co-", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("op", 100.0, 688.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let txt = to_text(&tp, defaults::TEXT);
    assert!(txt.contains("co-"), "default keeps hyphen: {txt:?}");
    assert!(txt.contains('\n'));
}

#[test]
fn serial_text_006_dehyphenate_joins() {
    let mut gs = line_glyphs("co-", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("op", 100.0, 688.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let txt = to_text(&tp, defaults::TEXT | textflags::DEHYPHENATE);
    assert!(txt.contains("coop"), "dehyphenate joins: {txt:?}");
    assert!(!txt.contains("co-"));
}

#[test]
fn serial_text_007_image_block_no_text() {
    let img = ImageRef {
        name: Some(SmolStr::new("Im0")),
        inline: false,
        ctm: Matrix::new(100.0, 0.0, 0.0, 100.0, 50.0, 50.0),
        width: Some(8),
        height: Some(8),
    };
    let gs = line_glyphs("hi", 100.0, 700.0, 12.0, 7.0);
    let tp = textpage_from_glyphs(&gs, &[img], letter(), 0);
    let txt = to_text(&tp, defaults::TEXT);
    assert_eq!(txt, "hi\n");
}

// === get_textbox =========================================================

#[test]
fn serial_textbox_001_clip_selects_lines() {
    let mut gs = line_glyphs("top", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("bot", 100.0, 500.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    // Device y: top line near y≈92, bottom near y≈292. Clip the upper band.
    let clip = Rect::new(0.0, 0.0, 612.0, 150.0);
    let txt = get_textbox(&tp, clip);
    assert_eq!(txt, "top");
}

#[test]
fn serial_textbox_002_clip_outside() {
    let gs = line_glyphs("hi", 100.0, 700.0, 12.0, 7.0);
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let clip = Rect::new(0.0, 400.0, 50.0, 450.0);
    assert_eq!(get_textbox(&tp, clip), "");
}

// === blocks ==============================================================

#[test]
fn serial_blocks_001_arity_and_types() {
    let gs = line_glyphs("hi", 100.0, 700.0, 12.0, 7.0);
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let bs = to_blocks(&tp, defaults::BLOCKS);
    assert_eq!(bs.len(), 1);
    let (x0, y0, x1, y1, text, no, ty) = &bs[0];
    assert!(x0 <= x1 && y0 <= y1);
    assert!(text.starts_with("hi"));
    assert_eq!(*no, 0);
    assert_eq!(*ty, 0);
}

#[test]
fn serial_blocks_002_text_type_and_numbering() {
    let mut gs = line_glyphs("aaa", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("bbb", 100.0, 500.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let bs = to_blocks(&tp, defaults::BLOCKS);
    assert_eq!(bs.len(), 2);
    assert_eq!(bs[0].5, 0);
    assert_eq!(bs[1].5, 1);
    assert!(bs.iter().all(|b| b.6 == 0));
}

#[test]
fn serial_blocks_003_block_text_trailing_newline() {
    let mut gs = line_glyphs("aaa", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("bbb", 100.0, 688.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let bs = to_blocks(&tp, defaults::BLOCKS);
    assert_eq!(bs.len(), 1);
    assert_eq!(bs[0].4, "aaa\nbbb\n");
}

#[test]
fn serial_blocks_004_image_type1_when_preserved() {
    let img = ImageRef {
        name: Some(SmolStr::new("Im0")),
        inline: false,
        ctm: Matrix::new(80.0, 0.0, 0.0, 80.0, 40.0, 40.0),
        width: Some(8),
        height: Some(8),
    };
    let gs = line_glyphs("hi", 100.0, 700.0, 12.0, 7.0);
    let tp = textpage_from_glyphs(&gs, &[img], letter(), 0);
    let bs = to_blocks(&tp, defaults::DICT); // includes PRESERVE_IMAGES
    assert!(
        bs.iter().any(|b| b.6 == 1),
        "expected an image tuple: {bs:?}"
    );
}

#[test]
fn serial_blocks_005_image_omitted_without_preserve() {
    let img = ImageRef {
        name: Some(SmolStr::new("Im0")),
        inline: false,
        ctm: Matrix::new(80.0, 0.0, 0.0, 80.0, 40.0, 40.0),
        width: Some(8),
        height: Some(8),
    };
    let gs = line_glyphs("hi", 100.0, 700.0, 12.0, 7.0);
    let tp = textpage_from_glyphs(&gs, &[img], letter(), 0);
    let bs = to_blocks(&tp, defaults::BLOCKS); // no PRESERVE_IMAGES
    assert!(bs.iter().all(|b| b.6 == 0));
}

// === words ===============================================================

#[test]
fn serial_words_001_arity() {
    let gs = line_glyphs("Hi there", 100.0, 700.0, 12.0, 7.0);
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = to_words(&tp, defaults::WORDS);
    assert_eq!(ws.len(), 2);
    let (x0, y0, x1, y1, word, b, l, w) = &ws[0];
    assert!(x0 <= x1 && y0 <= y1);
    assert_eq!(word, "Hi");
    assert_eq!((*b, *l, *w), (0, 0, 0));
    assert_eq!(ws[1].4, "there");
    assert_eq!((ws[1].5, ws[1].6, ws[1].7), (0, 0, 1));
}

#[test]
fn serial_words_002_numbering_two_lines() {
    let mut gs = line_glyphs("a b", 100.0, 700.0, 12.0, 7.0);
    gs.extend(line_glyphs("c d", 100.0, 688.0, 12.0, 7.0));
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let ws = to_words(&tp, defaults::WORDS);
    let labels: Vec<(i32, i32, i32, &str)> =
        ws.iter().map(|w| (w.5, w.6, w.7, w.4.as_str())).collect();
    assert_eq!(
        labels,
        vec![
            (0, 0, 0, "a"),
            (0, 0, 1, "b"),
            (0, 1, 0, "c"),
            (0, 1, 1, "d"),
        ]
    );
}

#[test]
fn serial_words_003_image_no_words() {
    let img = ImageRef {
        name: None,
        inline: true,
        ctm: Matrix::new(80.0, 0.0, 0.0, 80.0, 40.0, 40.0),
        width: Some(8),
        height: Some(8),
    };
    let tp = textpage_from_glyphs(&[], &[img], letter(), 0);
    assert!(to_words(&tp, defaults::WORDS).is_empty());
}

// === dict / rawdict ======================================================

fn sample_tp() -> TextPage {
    let gs = line_glyphs("Hi", 100.0, 700.0, 12.0, 7.0);
    textpage_from_glyphs(&gs, &[], letter(), 0)
}

#[test]
fn dict_001_top_keys() {
    let tp = sample_tp();
    let d = to_dict(&tp, false, defaults::DICT);
    assert!((d.width - 612.0).abs() < EPS);
    assert!((d.height - 792.0).abs() < EPS);
    assert_eq!(d.blocks.len(), 1);
}

#[test]
fn dict_002_text_block_keys() {
    let tp = sample_tp();
    let d = to_dict(&tp, false, defaults::DICT);
    match &d.blocks[0] {
        DictBlock::Text(b) => {
            assert_eq!(b.number, 0);
            assert!(b.bbox.0 <= b.bbox.2 && b.bbox.1 <= b.bbox.3);
            assert_eq!(b.lines.len(), 1);
        }
        DictBlock::Image(_) => panic!("expected text block"),
    }
}

#[test]
fn dict_003_line_keys() {
    let tp = sample_tp();
    let d = to_dict(&tp, false, defaults::DICT);
    let DictBlock::Text(b) = &d.blocks[0] else {
        panic!()
    };
    let line = &b.lines[0];
    assert_eq!(line.wmode, 0);
    assert!((line.dir.0 - 1.0).abs() < EPS && line.dir.1.abs() < EPS);
    assert_eq!(line.spans.len(), 1);
}

#[test]
fn dict_004_span_keys() {
    let tp = sample_tp();
    let d = to_dict(&tp, false, defaults::DICT);
    let DictBlock::Text(b) = &d.blocks[0] else {
        panic!()
    };
    let span = &b.lines[0].spans[0];
    assert!((span.size - 12.0).abs() < EPS);
    assert_eq!(span.font, "Helvetica");
    assert!((span.ascender - 0.7).abs() < EPS);
    assert!((span.descender + 0.2).abs() < EPS);
    // origin at baseline-left of first char (device space).
    assert!(span.origin.0 >= 0.0);
    assert!(span.bbox.0 <= span.bbox.2);
    assert_eq!(span.text, "Hi");
}

#[test]
fn dict_005_color_is_int() {
    let tp = sample_tp();
    let d = to_dict(&tp, false, defaults::DICT);
    let DictBlock::Text(b) = &d.blocks[0] else {
        panic!()
    };
    // i32 color field; black = 0.
    assert_eq!(b.lines[0].spans[0].color, 0);
}

#[test]
fn dict_006_dict_span_has_text_no_chars() {
    let tp = sample_tp();
    let d = to_dict(&tp, false, defaults::DICT);
    let DictBlock::Text(b) = &d.blocks[0] else {
        panic!()
    };
    let span = &b.lines[0].spans[0];
    assert_eq!(span.text, "Hi");
    assert!(span.chars.is_empty());
}

#[test]
fn dict_007_image_block_keys() {
    let img = ImageRef {
        name: Some(SmolStr::new("Im0")),
        inline: false,
        ctm: Matrix::new(80.0, 0.0, 0.0, 80.0, 40.0, 40.0),
        width: Some(8),
        height: Some(9),
    };
    let tp = textpage_from_glyphs(&[], &[img], letter(), 0);
    let d = to_dict(&tp, false, defaults::DICT);
    let DictBlock::Image(b) = &d.blocks[0] else {
        panic!("expected image block")
    };
    assert_eq!(b.width, 8);
    assert_eq!(b.height, 9);
    assert_eq!(b.ext, "");
    assert_eq!(b.colorspace, 0);
    assert_eq!(b.bpc, 0);
    assert_eq!(b.size, 0);
    assert!(b.image.is_empty());
    assert!(b.image_stubbed);
    // transform present (6-tuple); bbox present (4-tuple).
    let _ = b.transform;
    assert!(b.bbox.0 <= b.bbox.2);
}

#[test]
fn dict_008_empty_page() {
    let tp = textpage_from_glyphs(&[], &[], letter(), 0);
    let d = to_dict(&tp, false, defaults::DICT);
    assert!(d.blocks.is_empty());
    assert!((d.width - 612.0).abs() < EPS);
    assert!((d.height - 792.0).abs() < EPS);
}

#[test]
fn rawdict_001_span_has_chars_not_text() {
    let tp = sample_tp();
    let d = to_dict(&tp, true, defaults::RAWDICT);
    let DictBlock::Text(b) = &d.blocks[0] else {
        panic!()
    };
    let span = &b.lines[0].spans[0];
    assert!(span.text.is_empty());
    assert_eq!(span.chars.len(), 2);
}

#[test]
fn rawdict_002_char_keys() {
    let tp = sample_tp();
    let d = to_dict(&tp, true, defaults::RAWDICT);
    let DictBlock::Text(b) = &d.blocks[0] else {
        panic!()
    };
    let ch = &b.lines[0].spans[0].chars[0];
    assert!(ch.origin.0 >= 0.0);
    assert!(ch.bbox.0 <= ch.bbox.2);
    assert_eq!(ch.c, "H");
}

#[test]
fn rawdict_003_char_c_single_scalar() {
    let tp = sample_tp();
    let d = to_dict(&tp, true, defaults::RAWDICT);
    let DictBlock::Text(b) = &d.blocks[0] else {
        panic!()
    };
    for ch in &b.lines[0].spans[0].chars {
        assert_eq!(ch.c.chars().count(), 1);
    }
}

// === json / rawjson ======================================================

/// A tiny recursive-descent JSON validity check (no external dep): returns true
/// iff `s` is well-formed JSON. Used to assert serializer output parses.
fn is_valid_json(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut pos = 0usize;
    skip_ws(bytes, &mut pos);
    if !parse_value(bytes, &mut pos) {
        return false;
    }
    skip_ws(bytes, &mut pos);
    pos == bytes.len()
}

fn skip_ws(b: &[u8], pos: &mut usize) {
    while *pos < b.len() && matches!(b[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

fn parse_value(b: &[u8], pos: &mut usize) -> bool {
    skip_ws(b, pos);
    if *pos >= b.len() {
        return false;
    }
    match b[*pos] {
        b'{' => parse_object(b, pos),
        b'[' => parse_array(b, pos),
        b'"' => parse_string(b, pos),
        b't' => parse_lit(b, pos, "true"),
        b'f' => parse_lit(b, pos, "false"),
        b'n' => parse_lit(b, pos, "null"),
        _ => parse_number(b, pos),
    }
}

fn parse_lit(b: &[u8], pos: &mut usize, lit: &str) -> bool {
    if b[*pos..].starts_with(lit.as_bytes()) {
        *pos += lit.len();
        true
    } else {
        false
    }
}

fn parse_object(b: &[u8], pos: &mut usize) -> bool {
    *pos += 1; // '{'
    skip_ws(b, pos);
    if *pos < b.len() && b[*pos] == b'}' {
        *pos += 1;
        return true;
    }
    loop {
        skip_ws(b, pos);
        if !parse_string(b, pos) {
            return false;
        }
        skip_ws(b, pos);
        if *pos >= b.len() || b[*pos] != b':' {
            return false;
        }
        *pos += 1;
        if !parse_value(b, pos) {
            return false;
        }
        skip_ws(b, pos);
        if *pos >= b.len() {
            return false;
        }
        match b[*pos] {
            b',' => *pos += 1,
            b'}' => {
                *pos += 1;
                return true;
            }
            _ => return false,
        }
    }
}

fn parse_array(b: &[u8], pos: &mut usize) -> bool {
    *pos += 1; // '['
    skip_ws(b, pos);
    if *pos < b.len() && b[*pos] == b']' {
        *pos += 1;
        return true;
    }
    loop {
        if !parse_value(b, pos) {
            return false;
        }
        skip_ws(b, pos);
        if *pos >= b.len() {
            return false;
        }
        match b[*pos] {
            b',' => *pos += 1,
            b']' => {
                *pos += 1;
                return true;
            }
            _ => return false,
        }
    }
}

fn parse_string(b: &[u8], pos: &mut usize) -> bool {
    if *pos >= b.len() || b[*pos] != b'"' {
        return false;
    }
    *pos += 1;
    while *pos < b.len() {
        match b[*pos] {
            b'"' => {
                *pos += 1;
                return true;
            }
            b'\\' => *pos += 2,
            _ => *pos += 1,
        }
    }
    false
}

fn parse_number(b: &[u8], pos: &mut usize) -> bool {
    let start = *pos;
    if *pos < b.len() && b[*pos] == b'-' {
        *pos += 1;
    }
    while *pos < b.len() && b[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos < b.len() && b[*pos] == b'.' {
        *pos += 1;
        while *pos < b.len() && b[*pos].is_ascii_digit() {
            *pos += 1;
        }
    }
    if *pos < b.len() && matches!(b[*pos], b'e' | b'E') {
        *pos += 1;
        if *pos < b.len() && matches!(b[*pos], b'+' | b'-') {
            *pos += 1;
        }
        while *pos < b.len() && b[*pos].is_ascii_digit() {
            *pos += 1;
        }
    }
    *pos > start
}

#[test]
fn json_001_valid_json() {
    let tp = sample_tp();
    let j = to_json(&tp, false, defaults::JSON);
    assert!(is_valid_json(&j), "invalid json: {j}");
}

#[test]
fn json_002_bbox_is_array() {
    let tp = sample_tp();
    let j = to_json(&tp, false, defaults::JSON);
    assert!(j.contains("\"bbox\":["), "bbox should be an array: {j}");
}

#[test]
fn json_003_text_vs_chars() {
    let tp = sample_tp();
    let j = to_json(&tp, false, defaults::JSON);
    assert!(j.contains("\"text\":\"Hi\""));
    assert!(!j.contains("\"chars\""));
    let rj = to_json(&tp, true, defaults::RAWJSON);
    assert!(rj.contains("\"chars\":["));
    assert!(rj.contains("\"c\":\"H\""));
    assert!(is_valid_json(&rj), "invalid rawjson: {rj}");
}

#[test]
fn json_004_image_base64_placeholder() {
    let img = ImageRef {
        name: Some(SmolStr::new("Im0")),
        inline: false,
        ctm: Matrix::new(80.0, 0.0, 0.0, 80.0, 40.0, 40.0),
        width: Some(8),
        height: Some(8),
    };
    let tp = textpage_from_glyphs(&[], &[img], letter(), 0);
    let j = to_json(&tp, false, defaults::JSON);
    assert!(is_valid_json(&j), "invalid json: {j}");
    assert!(j.contains("\"type\":1"));
    // image is a (possibly empty) JSON string.
    assert!(j.contains("\"image\":\""));
}

#[test]
fn json_005_top_keys_order() {
    let tp = sample_tp();
    let j = to_json(&tp, false, defaults::JSON);
    assert!(j.starts_with("{\"width\":"));
    assert!(j.contains("\"height\":"));
    assert!(j.contains("\"blocks\":["));
}

// Direct model construction for an image block via build (sanity that
// BlockKind::Image is reachable through textpage_from_glyphs).
#[test]
fn image_block_kind_reachable() {
    let img = ImageRef {
        name: None,
        inline: false,
        ctm: Matrix::new(10.0, 0.0, 0.0, 10.0, 0.0, 0.0),
        width: Some(2),
        height: Some(2),
    };
    let tp = textpage_from_glyphs(&[], &[img], letter(), 0);
    assert!(tp
        .blocks
        .iter()
        .any(|b: &Block| matches!(b.image, Some(ImageBlock { .. }))));
}
