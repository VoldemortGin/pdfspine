//! M2c property / containment / never-panic tests. Catalog IDs: `LAYOUT-PROP-*`.

use pdf_core::geom::{Point, Rect};
use pdf_text::model::WritingDir;
use pdf_text::{textpage_from_glyphs, words, PositionedGlyph, TextPage};
use smol_str::SmolStr;

fn letter() -> Rect {
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

fn glyph(c: &str, ox: f64, oy: f64, size: f64) -> PositionedGlyph {
    let w = 0.5 * size;
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

/// Builds a small multi-line, multi-word page for the containment / equivalence
/// properties.
fn sample_page() -> TextPage {
    let mut gs = Vec::new();
    let mut x = 100.0;
    for (i, c) in "Hello world".chars().enumerate() {
        gs.push(glyph(&c.to_string(), x, 700.0, 12.0));
        x += if c == ' ' { 4.0 } else { 7.0 };
        let _ = i;
    }
    x = 100.0;
    for c in "second line".chars() {
        gs.push(glyph(&c.to_string(), x, 686.0, 12.0));
        x += if c == ' ' { 4.0 } else { 7.0 };
    }
    textpage_from_glyphs(&gs, &[], letter(), 0)
}

#[test]
fn layout_prop_001_containment_char_span_line_block() {
    let tp = sample_page();
    for block in &tp.blocks {
        for line in &block.lines {
            assert!(
                block.bbox.contains_rect(&line.bbox),
                "line ⊄ block: {:?} ⊄ {:?}",
                line.bbox,
                block.bbox
            );
            for span in &line.spans {
                assert!(
                    line.bbox.contains_rect(&span.bbox),
                    "span ⊄ line: {:?} ⊄ {:?}",
                    span.bbox,
                    line.bbox
                );
                for ch in &span.chars {
                    assert!(
                        span.bbox.contains_rect(&ch.bbox),
                        "char ⊄ span: {:?} ⊄ {:?}",
                        ch.bbox,
                        span.bbox
                    );
                }
            }
        }
    }
}

#[test]
fn layout_prop_002_words_concat_matches_text_normalized() {
    let tp = sample_page();

    // Text-mode rendering: spans joined within a line, lines joined by '\n'.
    let mut text_lines: Vec<String> = Vec::new();
    for block in &tp.blocks {
        for line in &block.lines {
            let s: String = line.spans.iter().flat_map(|sp| sp.text.chars()).collect();
            text_lines.push(s);
        }
    }
    let text_mode = text_lines.join("\n");
    let text_norm: String = text_mode.split_whitespace().collect::<Vec<_>>().join(" ");

    // Words concat (space-joined).
    let ws = words(&tp);
    let words_concat = ws
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    assert_eq!(words_concat, text_norm);
}

#[test]
fn layout_prop_003_arbitrary_never_panics_finite() {
    // A spray of glyphs at extreme / degenerate coordinates must not panic and
    // must yield finite bboxes.
    let coords = [
        (0.0, 0.0),
        (1e6, -1e6),
        (-50.0, 900.0),
        (300.0, 300.0),
        (612.0, 792.0),
        (f64::from(i32::MAX), 0.0),
    ];
    let gs: Vec<PositionedGlyph> = coords
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| glyph(&((b'a' + (i as u8)) as char).to_string(), x, y, 12.0))
        .collect();
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    for block in &tp.blocks {
        assert!(block.bbox.x0.is_finite() && block.bbox.y1.is_finite());
        for line in &block.lines {
            for span in &line.spans {
                assert!(span.bbox.x0.is_finite() && span.bbox.x1.is_finite());
            }
        }
    }
    // Words on the same page also never panic.
    let _ = words(&tp);
}

#[test]
fn layout_prop_003_empty_and_single_glyph() {
    // Degenerate sizes.
    let _ = textpage_from_glyphs(&[], &[], letter(), 0);
    let one = textpage_from_glyphs(&[glyph("A", 10.0, 10.0, 0.0)], &[], letter(), 0);
    assert_eq!(one.blocks.len(), 1);
    // A zero MediaBox must not panic.
    let _ = textpage_from_glyphs(
        &[glyph("A", 0.0, 0.0, 12.0)],
        &[],
        Rect::new(0.0, 0.0, 0.0, 0.0),
        0,
    );
}
