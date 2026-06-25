//! M2c layout-reconstruction unit tests (PRD §8.6.1/§8.6.2): device/page
//! transform, line clustering, span splitting, block grouping, reading order,
//! flags and edge cases. Self-built glyph lists in PDF user space; no PyMuPDF
//! files. Catalog IDs: `LAYOUT-DEVICE-*`, `COORD-ROT-*-PAGE`, `LAYOUT-LINE-*`,
//! `LAYOUT-SPAN-*`, `LAYOUT-BLOCK-*`, `LAYOUT-ORDER-*`, `LAYOUT-FLAGS-*`,
//! `LAYOUT-EDGE-*`.

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_text::model::{flags, BlockKind, WritingDir};
use pdf_text::{page_size, page_transform, textpage_from_glyphs, ImageRef, PositionedGlyph};
use smol_str::SmolStr;

const EPS: f64 = 1e-6;

/// A US-Letter MediaBox at the origin.
fn letter() -> Rect {
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

/// Builds a horizontal-writing glyph in PDF user space (origin bottom-left).
fn glyph(c: &str, ox: f64, oy: f64, size: f64) -> PositionedGlyph {
    glyph_styled(c, ox, oy, size, "Helvetica", 0)
}

fn glyph_styled(c: &str, ox: f64, oy: f64, size: f64, font: &str, color: u32) -> PositionedGlyph {
    // A simple cell: advance ≈ 0.5·size wide, ascent 0.7·size, descent -0.2·size.
    let w = 0.5 * size;
    let asc = 0.7 * size;
    let desc = -0.2 * size;
    PositionedGlyph {
        unicode: SmolStr::new(c),
        code: c.chars().next().map_or(0, |ch| ch as u32),
        origin: Point::new(ox, oy),
        bbox: Rect::new(ox, oy + desc, ox + w, oy + asc),
        font_name: SmolStr::new(font),
        size,
        color,
        render_mode: 0,
        writing_dir: WritingDir::Horizontal,
        ascender: 0.7,
        descender: -0.2,
    }
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() <= EPS, "expected {a} ≈ {b}");
}

// === device / page transform =============================================

#[test]
fn layout_device_001_y_flip_top_has_small_y() {
    // A glyph near the page top (large user y) → small device y.
    let g = glyph("A", 100.0, 700.0, 12.0); // origin user y = 700 (near top of 792)
    let tp = textpage_from_glyphs(&[g], &[], letter(), 0);
    let span = &tp.blocks[0].lines[0].spans[0];
    // device origin y = y1 - user_y = 792 - 700 = 92 (small → near top).
    approx(span.chars[0].origin.y, 92.0);
    assert!(span.chars[0].origin.y < tp.height / 2.0);
}

#[test]
fn layout_device_002_transform_r0_and_size() {
    let m = page_transform(letter(), 0);
    assert_eq!(m, Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, 792.0));
    assert_eq!(page_size(letter(), 0), (612.0, 792.0));
}

#[test]
fn coord_rot_0_page_inside_bounds() {
    let g = glyph("A", 10.0, 10.0, 12.0);
    let tp = textpage_from_glyphs(&[g], &[], letter(), 0);
    let bb = tp.blocks[0].bbox;
    assert!(bb.x0 >= 0.0 && bb.x1 <= tp.width);
    assert!(bb.y0 >= 0.0 && bb.y1 <= tp.height);
}

#[test]
fn coord_rot_90_page_matrix_and_size() {
    // P_90 = [0, 1, 1, 0, -y0, -x0]; size h×w.
    let m = page_transform(letter(), 90);
    assert_eq!(m, Matrix::new(0.0, 1.0, 1.0, 0.0, 0.0, 0.0));
    assert_eq!(page_size(letter(), 90), (792.0, 612.0));
}

#[test]
fn coord_rot_180_page_matrix_and_size() {
    // P_180 = [-1, 0, 0, 1, x1, -y0]; size w×h.
    let m = page_transform(letter(), 180);
    assert_eq!(m, Matrix::new(-1.0, 0.0, 0.0, 1.0, 612.0, 0.0));
    assert_eq!(page_size(letter(), 180), (612.0, 792.0));
}

#[test]
fn coord_rot_270_page_matrix_and_size() {
    // P_270 = [0, -1, -1, 0, y1, x1]; size h×w.
    let m = page_transform(letter(), 270);
    assert_eq!(m, Matrix::new(0.0, -1.0, -1.0, 0.0, 792.0, 612.0));
    assert_eq!(page_size(letter(), 270), (792.0, 612.0));
}

#[test]
fn coord_rot_cropbox_origin_baked_in() {
    // The page transform bakes out the **CropBox** origin (the coordinate basis):
    // a glyph at the CropBox top-left maps to device (0,0), independent of where
    // the MediaBox origin sits. `page_transform` is basis-agnostic and unchanged;
    // `build_textpage` now feeds it the CropBox, so all extraction channels share
    // one origin on CropBox ≠ MediaBox pages.
    let cropbox = Rect::new(50.0, 100.0, 662.0, 892.0); // non-zero-origin CropBox
    let m = page_transform(cropbox, 0);
    // P_0 = [1,0,0,-1,-x0,y1] = [1,0,0,-1,-50,892].
    assert_eq!(m, Matrix::new(1.0, 0.0, 0.0, -1.0, -50.0, 892.0));
    // A glyph at the CropBox top-left corner user (50,892) → device (0,0).
    let g = glyph("A", 50.0, 892.0, 12.0);
    let tp = textpage_from_glyphs(&[g], &[], cropbox, 0);
    approx(tp.blocks[0].lines[0].spans[0].chars[0].origin.x, 0.0);
    approx(tp.blocks[0].lines[0].spans[0].chars[0].origin.y, 0.0);
}

#[test]
fn layout_device_003_textpage_size_matches_rotation() {
    let g = glyph("A", 10.0, 10.0, 12.0);
    let one = std::slice::from_ref(&g);
    let tp0 = textpage_from_glyphs(one, &[], letter(), 0);
    assert_eq!((tp0.width, tp0.height), (612.0, 792.0));
    let tp90 = textpage_from_glyphs(one, &[], letter(), 90);
    assert_eq!((tp90.width, tp90.height), (792.0, 612.0));
}

// === line grouping =======================================================

#[test]
fn layout_line_001_same_baseline_one_line() {
    let gs = vec![
        glyph("H", 100.0, 700.0, 12.0),
        glyph("i", 106.0, 700.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    assert_eq!(tp.blocks.iter().flat_map(|b| &b.lines).count(), 1);
}

#[test]
fn layout_line_002_two_baselines_two_lines() {
    let gs = vec![
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 100.0, 680.0, 12.0), // 20pt lower → distinct baseline
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let n_lines: usize = tp.blocks.iter().map(|b| b.lines.len()).sum();
    assert_eq!(n_lines, 2);
}

#[test]
fn layout_line_003_small_rise_same_line() {
    // A superscript raised by 3pt on a 12pt baseline stays on the line.
    let gs = vec![
        glyph("x", 100.0, 700.0, 12.0),
        glyph("2", 106.0, 703.0, 8.0), // small rise
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let n_lines: usize = tp.blocks.iter().map(|b| b.lines.len()).sum();
    assert_eq!(n_lines, 1);
}

#[test]
fn layout_line_004_large_gap_new_line() {
    let gs = vec![
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 100.0, 600.0, 12.0), // 100pt lower
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let n_lines: usize = tp.blocks.iter().map(|b| b.lines.len()).sum();
    assert_eq!(n_lines, 2);
}

#[test]
fn layout_line_005_sorted_by_advance() {
    // Provide glyphs out of advance order; expect text in left-to-right order.
    // The cells are 6pt wide at 10pt pitch, so each ~4pt gap exceeds the word-gap
    // threshold (0.2·12 = 2.4) and the layout synthesizes an inter-word space —
    // hence "A B C" (the contract is the left-to-right ordering, not adjacency).
    let gs = vec![
        glyph("C", 120.0, 700.0, 12.0),
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 110.0, 700.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let line = &tp.blocks[0].lines[0];
    let text: String = line.spans.iter().flat_map(|s| s.text.chars()).collect();
    assert_eq!(text, "A B C");
    // Order is preserved regardless of spacing.
    assert_eq!(text.replace(' ', ""), "ABC");
}

// === span splitting ======================================================

#[test]
fn layout_span_001_same_style_merges() {
    let gs = vec![
        glyph("a", 100.0, 700.0, 12.0),
        glyph("b", 106.0, 700.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    assert_eq!(tp.blocks[0].lines[0].spans.len(), 1);
    assert_eq!(tp.blocks[0].lines[0].spans[0].text, "ab");
}

#[test]
fn layout_span_002_font_change_splits() {
    let gs = vec![
        glyph_styled("a", 100.0, 700.0, 12.0, "Helvetica", 0),
        glyph_styled("b", 106.0, 700.0, 12.0, "Times", 0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    assert_eq!(tp.blocks[0].lines[0].spans.len(), 2);
}

#[test]
fn layout_span_003_size_change_splits() {
    let gs = vec![
        glyph("a", 100.0, 700.0, 12.0),
        glyph("b", 106.0, 700.0, 18.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    assert_eq!(tp.blocks[0].lines[0].spans.len(), 2);
}

#[test]
fn layout_span_004_color_change_splits() {
    let gs = vec![
        glyph_styled("a", 100.0, 700.0, 12.0, "Helvetica", 0x000000),
        glyph_styled("b", 106.0, 700.0, 12.0, "Helvetica", 0xFF0000),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    assert_eq!(tp.blocks[0].lines[0].spans.len(), 2);
}

#[test]
fn layout_span_005_text_is_char_concat() {
    let gs = vec![
        glyph("H", 100.0, 700.0, 12.0),
        glyph("i", 106.0, 700.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let span = &tp.blocks[0].lines[0].spans[0];
    let from_chars: String = span.chars.iter().map(|c| c.c).collect();
    assert_eq!(span.text, from_chars);
}

// === block grouping + reading order ======================================

#[test]
fn layout_block_001_small_gap_one_block() {
    // Two lines 14pt apart (single-spaced 12pt text) → one block.
    let gs = vec![
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 100.0, 686.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let text_blocks = tp
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Text)
        .count();
    assert_eq!(text_blocks, 1);
    assert_eq!(tp.blocks[0].lines.len(), 2);
}

#[test]
fn layout_block_002_large_gap_two_blocks() {
    // Two lines 60pt apart → distinct paragraph blocks.
    let gs = vec![
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 100.0, 640.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let text_blocks = tp
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Text)
        .count();
    assert_eq!(text_blocks, 2);
}

#[test]
fn layout_block_003_image_block_present() {
    let g = glyph("A", 100.0, 700.0, 12.0);
    // An image placed at user (200,200)-(300,300) via a scale+translate CTM.
    let ctm = Matrix::concat(
        &Matrix::scale(100.0, 100.0),
        &Matrix::translate(200.0, 200.0),
    );
    let img = ImageRef {
        name: Some(SmolStr::new("Im0")),
        inline: false,
        ctm,
        width: Some(640),
        height: Some(480),
    };
    let tp = textpage_from_glyphs(&[g], &[img], letter(), 0);
    let imgs: Vec<_> = tp
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Image)
        .collect();
    assert_eq!(imgs.len(), 1);
    let ib = imgs[0].image.as_ref().unwrap();
    assert_eq!(ib.name.as_deref(), Some("Im0"));
    assert_eq!(ib.width, Some(640));
    // Image device bbox: user (200,200)-(300,300) → y-flip on 792-high page.
    let bb = imgs[0].bbox;
    approx(bb.x0, 200.0);
    approx(bb.x1, 300.0);
    approx(bb.y0, 792.0 - 300.0);
    approx(bb.y1, 792.0 - 200.0);
}

#[test]
fn layout_order_001_single_column_top_to_bottom() {
    // Three paragraphs stacked vertically → block numbers increase downward.
    let gs = vec![
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 100.0, 600.0, 12.0),
        glyph("C", 100.0, 500.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let order: Vec<char> = tp
        .blocks
        .iter()
        .filter_map(|b| b.lines.first())
        .filter_map(|l| l.spans.first())
        .filter_map(|s| s.text.chars().next())
        .collect();
    assert_eq!(order, vec!['A', 'B', 'C']);
}

#[test]
fn layout_order_002_two_column_column_by_column() {
    // Left column (x≈80): L1 top, L2 bottom. Right column (x≈400): R1, R2.
    // XY-cut must read the whole left column before the right one: L1,L2,R1,R2.
    let gs = vec![
        glyph("1", 80.0, 700.0, 12.0),  // L1
        glyph("2", 80.0, 500.0, 12.0),  // L2
        glyph("3", 400.0, 700.0, 12.0), // R1
        glyph("4", 400.0, 500.0, 12.0), // R2
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let order: Vec<char> = tp
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Text)
        .filter_map(|b| b.lines.first())
        .filter_map(|l| l.spans.first())
        .filter_map(|s| s.text.chars().next())
        .collect();
    assert_eq!(order, vec!['1', '2', '3', '4']);
}

#[test]
fn layout_order_003_block_numbers_monotonic() {
    let gs = vec![
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 100.0, 600.0, 12.0),
        glyph("C", 100.0, 500.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    for (i, b) in tp.blocks.iter().enumerate() {
        assert_eq!(b.number, i);
    }
}

// === span flags ==========================================================

#[test]
fn layout_flags_001_bold_name() {
    let gs = vec![glyph_styled("A", 100.0, 700.0, 12.0, "Helvetica-Bold", 0)];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let f = tp.blocks[0].lines[0].spans[0].flags;
    assert_eq!(f & flags::BOLD, flags::BOLD);
}

#[test]
fn layout_flags_002_italic_name() {
    let gs = vec![glyph_styled(
        "A",
        100.0,
        700.0,
        12.0,
        "Helvetica-Oblique",
        0,
    )];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let f = tp.blocks[0].lines[0].spans[0].flags;
    assert_eq!(f & flags::ITALIC, flags::ITALIC);
}

#[test]
fn layout_flags_003_serif_and_mono() {
    let serif = textpage_from_glyphs(
        &[glyph_styled("A", 100.0, 700.0, 12.0, "Times-Roman", 0)],
        &[],
        letter(),
        0,
    );
    assert_eq!(
        serif.blocks[0].lines[0].spans[0].flags & flags::SERIF,
        flags::SERIF
    );
    let mono = textpage_from_glyphs(
        &[glyph_styled("A", 100.0, 700.0, 12.0, "Courier", 0)],
        &[],
        letter(),
        0,
    );
    assert_eq!(
        mono.blocks[0].lines[0].spans[0].flags & flags::MONO,
        flags::MONO
    );
}

#[test]
fn layout_flags_004_superscript_rise() {
    // Baseline glyph at y=700, a higher (raised) glyph → superscript bit.
    let gs = vec![
        glyph("x", 100.0, 700.0, 12.0),
        glyph("2", 106.0, 706.0, 8.0), // raised 6pt: above baseline
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    // Find the span whose text is "2".
    let sup = tp.blocks[0].lines[0]
        .spans
        .iter()
        .find(|s| s.text == "2")
        .expect("superscript span");
    assert_eq!(sup.flags & flags::SUPERSCRIPT, flags::SUPERSCRIPT);
}

// === edge cases ==========================================================

#[test]
fn layout_edge_001_rotated_text_grouped() {
    // 90°-rotated page: horizontal user text becomes vertical device text but
    // must still group as one line (one block, one line).
    let gs = vec![
        glyph("A", 100.0, 700.0, 12.0),
        glyph("B", 106.0, 700.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 90);
    let n_lines: usize = tp.blocks.iter().map(|b| b.lines.len()).sum();
    assert_eq!(n_lines, 1);
    // dir on a 90° page is (0,1): horizontal user advance reads downward.
    let dir = tp.blocks[0].lines[0].dir;
    approx(dir.0, 0.0);
    approx(dir.1, 1.0);
}

#[test]
fn layout_edge_002_vertical_writing_wmode() {
    // Vertical-writing glyphs stacked downward in user space.
    let mut a = glyph("\u{4E00}", 300.0, 700.0, 20.0);
    a.writing_dir = WritingDir::Vertical;
    let mut b = glyph("\u{4E8C}", 300.0, 670.0, 20.0);
    b.writing_dir = WritingDir::Vertical;
    let tp = textpage_from_glyphs(&[a, b], &[], letter(), 0);
    let line = tp
        .blocks
        .iter()
        .flat_map(|bl| &bl.lines)
        .next()
        .expect("a line");
    assert_eq!(line.wmode, 1);
}

#[test]
fn layout_edge_003_rtl_visual_order() {
    // Hebrew alef, bet, gimel laid out logically left-to-right at increasing x;
    // a predominantly-RTL run is reversed to visual right-to-left order.
    let gs = vec![
        glyph("\u{05D0}", 100.0, 700.0, 12.0), // alef
        glyph("\u{05D1}", 110.0, 700.0, 12.0), // bet
        glyph("\u{05D2}", 120.0, 700.0, 12.0), // gimel
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let text: String = tp.blocks[0].lines[0]
        .spans
        .iter()
        .flat_map(|s| s.text.chars())
        .collect();
    // Visual order: rightmost (gimel) first.
    assert_eq!(text, "\u{05D2}\u{05D1}\u{05D0}");
}

#[test]
fn layout_edge_004_empty_input_no_panic() {
    let tp = textpage_from_glyphs(&[], &[], letter(), 0);
    assert!(tp.blocks.is_empty());
    assert_eq!((tp.width, tp.height), (612.0, 792.0));
}
