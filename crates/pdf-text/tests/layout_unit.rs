//! M2c layout-reconstruction unit tests (PRD §8.6.1/§8.6.2): device/page
//! transform, line clustering, span splitting, block grouping, reading order,
//! flags and edge cases. Self-built glyph lists in PDF user space; no PyMuPDF
//! files. Catalog IDs: `LAYOUT-DEVICE-*`, `COORD-ROT-*-PAGE`, `LAYOUT-LINE-*`,
//! `LAYOUT-SPAN-*`, `LAYOUT-BLOCK-*`, `LAYOUT-ORDER-*`, `LAYOUT-FLAGS-*`,
//! `LAYOUT-EDGE-*`.

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_text::model::{flags, BlockKind, WritingDir};
use pdf_text::{page_size, page_transform, textpage_from_glyphs, words, ImageRef, PositionedGlyph};
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

/// A declared-12pt glyph whose actual rendering geometry is supplied directly.
/// Keeps origin, bbox and advance direction consistent with the matrix so the
/// tests exercise layout rather than malformed fixture artifacts.
fn glyph_with_matrix(c: &str, matrix: Matrix) -> PositionedGlyph {
    let mut g = glyph(c, matrix.e, matrix.f, 12.0);
    g.origin = Point::new(matrix.e, matrix.f);
    g.render_matrix = matrix;
    let norm = (matrix.a * matrix.a + matrix.b * matrix.b).sqrt();
    if norm > f64::EPSILON {
        g.advance_dir = (matrix.a / norm, matrix.b / norm);
    }
    g.bbox = g.cell.transform(&matrix).normalize();
    g
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

/// LAYOUT-SPAN-006: adjacent glyphs with one declared `Tf` but materially
/// different painted scale / Tz / shear form distinct visual spans. They remain
/// one line and their flattened text, word, bbox and order contracts stay intact.
#[test]
fn layout_span_006_render_matrix_changes_split_without_text_reordering() {
    let gs = vec![
        glyph_with_matrix("A", Matrix::new(12.0, 0.0, 0.0, 12.0, 100.0, 700.0)),
        glyph_with_matrix("B", Matrix::new(18.0, 0.0, 0.0, 12.0, 104.0, 700.0)),
        glyph_with_matrix("C", Matrix::new(18.0, 0.0, 6.0, 12.0, 108.0, 700.0)),
        glyph_with_matrix("D", Matrix::new(12.0, 0.0, 0.0, 12.0, 112.0, 700.0)),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let lines: Vec<_> = tp.blocks.iter().flat_map(|block| &block.lines).collect();
    assert_eq!(lines.len(), 1, "geometry-only changes must remain one line");
    let line = lines[0];
    assert_eq!(
        line.spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C", "D"]
    );
    assert_eq!(line.number, 0);
    assert_eq!(
        words(&tp)
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        vec!["ABCD"],
        "span boundaries must not become word boundaries"
    );
    let char_bbox = line
        .spans
        .iter()
        .flat_map(|span| &span.chars)
        .fold(Rect::default(), |bbox, ch| bbox.union(&ch.bbox));
    approx(line.bbox.x0, char_bbox.x0);
    approx(line.bbox.y0, char_bbox.y0);
    approx(line.bbox.x1, char_bbox.x1);
    approx(line.bbox.y1, char_bbox.y1);
}

/// LAYOUT-SPAN-007: a 4° change remains inside the existing 5° line-direction
/// tolerance, yet its linear matrix delta is large enough to split the span.
#[test]
fn layout_span_007_small_rotation_stays_one_line_but_splits_span() {
    let angle = 4.0_f64.to_radians();
    let (sin, cos) = angle.sin_cos();
    let first = glyph_with_matrix("A", Matrix::new(12.0, 0.0, 0.0, 12.0, 10.0, 782.0));
    let second = glyph_with_matrix(
        "B",
        Matrix::new(
            12.0 * cos,
            12.0 * sin,
            -12.0 * sin,
            12.0 * cos,
            10.0 + 6.0 * cos,
            782.0 + 6.0 * sin,
        ),
    );
    let tp = textpage_from_glyphs(&[first, second], &[], letter(), 0);
    let lines: Vec<_> = tp.blocks.iter().flat_map(|block| &block.lines).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].spans.len(), 2);
    assert_eq!(
        lines[0]
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B"]
    );
}

/// LAYOUT-SPAN-008: a cross-axis jump can stay inside the broad line cluster
/// while two same-flag glyphs on that line still split into visual spans.
#[test]
fn layout_span_008_adjacent_baseline_jump_splits_same_flag_glyphs() {
    let gs = vec![
        glyph("s", 94.0, 782.0, 12.0),
        glyph("a", 100.0, 780.0, 12.0),
        glyph("b", 106.0, 780.0, 12.0),
        glyph("c", 112.0, 778.0, 12.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let lines: Vec<_> = tp.blocks.iter().flat_map(|block| &block.lines).collect();
    assert_eq!(lines.len(), 1);
    let line = lines[0];
    assert_eq!(
        line.spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<Vec<_>>(),
        vec!["s", "ab", "c"]
    );
    assert_eq!(line.spans[1].flags, line.spans[2].flags);
}

/// LAYOUT-SPAN-009: an ordinary run and a normal superscript run each remain
/// cohesive. The pre-existing style flag makes the boundary between them.
#[test]
fn layout_span_009_normal_and_superscript_runs_are_not_over_split() {
    let gs = vec![
        glyph("a", 100.0, 780.0, 12.0),
        glyph("b", 106.0, 780.0, 12.0),
        glyph("c", 112.0, 780.0, 12.0),
        glyph("2", 118.0, 783.0, 8.0),
        glyph("3", 122.0, 783.0, 8.0),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let line = &tp.blocks[0].lines[0];
    assert_eq!(
        line.spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<Vec<_>>(),
        vec!["abc", "23"]
    );
    assert_eq!(line.spans[1].flags & flags::SUPERSCRIPT, flags::SUPERSCRIPT);
}

/// LAYOUT-SPAN-010: several Unicode scalars carried by one glyph never acquire
/// an artificial internal geometry seam.
#[test]
fn layout_span_010_ligature_mapping_stays_one_span() {
    let tp = textpage_from_glyphs(&[glyph("fi", 100.0, 700.0, 12.0)], &[], letter(), 0);
    let span = &tp.blocks[0].lines[0].spans[0];
    assert_eq!(span.text, "fi");
    assert_eq!(span.chars.len(), 2);
    assert_eq!(span.chars[0].bbox, span.chars[1].bbox);
    assert_eq!(span.chars[0].quad, span.chars[1].quad);
}

/// LAYOUT-SPAN-011: translating the same pair on the page cannot change the
/// geometry split; the baseline test is based on adjacent deltas.
#[test]
fn layout_span_011_split_is_translation_invariant() {
    let build = |dx: f64, dy: f64| {
        let gs = vec![
            glyph_with_matrix(
                "A",
                Matrix::new(12.0, 0.0, 0.0, 12.0, 20.0 + dx, 700.0 + dy),
            ),
            glyph_with_matrix(
                "B",
                Matrix::new(18.0, 0.0, 0.0, 18.0, 26.0 + dx, 700.0 + dy),
            ),
        ];
        textpage_from_glyphs(&gs, &[], letter(), 0)
    };
    let original = build(0.0, 0.0);
    let translated = build(250.0, -300.0);
    for tp in [&original, &translated] {
        let line = &tp.blocks[0].lines[0];
        assert_eq!(line.spans.len(), 2);
        assert_eq!(
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "AB"
        );
    }
}

/// LAYOUT-SPAN-012: singular matrices follow a finite conservative policy:
/// identical finite linear parts may merge on one baseline, a changed singular
/// transform splits, and no NaN or panic escapes into the model.
#[test]
fn layout_span_012_degenerate_matrix_policy_is_finite() {
    let gs = vec![
        glyph_with_matrix("A", Matrix::new(12.0, 0.0, 0.0, 0.0, 100.0, 700.0)),
        glyph_with_matrix("B", Matrix::new(12.0, 0.0, 0.0, 0.0, 106.0, 700.0)),
        glyph_with_matrix("C", Matrix::new(12.0, 0.0, 1.0, 0.0, 112.0, 700.0)),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let line = &tp.blocks[0].lines[0];
    assert_eq!(
        line.spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<Vec<_>>(),
        vec!["AB", "C"]
    );
    assert!(line.bbox.x0.is_finite() && line.bbox.x1.is_finite());
}

/// LAYOUT-SPAN-013: a synthesized word space remains exactly once at a geometry
/// span boundary and `words()` still sees the same two words.
#[test]
fn layout_span_013_synthetic_space_survives_geometry_boundary_once() {
    let gs = vec![
        glyph_with_matrix("A", Matrix::new(12.0, 0.0, 0.0, 12.0, 100.0, 700.0)),
        glyph_with_matrix("B", Matrix::new(18.0, 0.0, 0.0, 18.0, 110.0, 700.0)),
    ];
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let line = &tp.blocks[0].lines[0];
    assert_eq!(line.spans.len(), 2);
    let flattened: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(flattened, "A B");
    let spaces: Vec<_> = line
        .spans
        .iter()
        .flat_map(|span| &span.chars)
        .filter(|ch| ch.c == ' ')
        .collect();
    assert_eq!(spaces.len(), 1);
    assert!(spaces[0].synthetic);
    assert_eq!(
        words(&tp)
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B"]
    );
}

/// LAYOUT-SPAN-014: decimal representation noise at the named 5% boundary is
/// absorbed by the separate numerical epsilon, while a material excess splits.
#[test]
fn layout_span_014_linear_threshold_has_numeric_slack_only() {
    let span_count = |shear: f64| {
        let gs = vec![
            glyph_with_matrix("A", Matrix::new(10.0, 0.0, 0.0, 10.0, 100.0, 700.0)),
            glyph_with_matrix("B", Matrix::new(10.0, 0.0, shear, 10.0, 105.0, 700.0)),
        ];
        let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
        tp.blocks[0].lines[0].spans.len()
    };

    assert_eq!(span_count(0.5000000000000012), 1);
    assert_eq!(span_count(0.500001), 2);
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
    // Use substantial prose-like lines rather than four isolated table cells:
    // same-baseline cells intentionally stay in one block for PyMuPDF block
    // compatibility and are not enough evidence of a column. Paint row-major
    // so this also proves that the two column regions remain atomic.
    let mut gs = Vec::new();
    for (text, x, y) in [
        ("L1-left-column", 80.0, 700.0),
        ("R1-right-column", 400.0, 700.0),
        ("L2-left-column", 80.0, 660.0),
        ("R2-right-column", 400.0, 660.0),
    ] {
        for (index, ch) in text.chars().enumerate() {
            gs.push(glyph(&ch.to_string(), x + index as f64 * 6.0, y, 12.0));
        }
    }
    let tp = textpage_from_glyphs(&gs, &[], letter(), 0);
    let order: Vec<String> = tp
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Text)
        .map(|b| {
            b.lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.text.as_str())
                .collect()
        })
        .collect();
    assert_eq!(
        order,
        vec![
            "L1-left-column",
            "L2-left-column",
            "R1-right-column",
            "R2-right-column",
        ]
    );
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

// === two-column record grids (correlation tables) =========================

/// Lays one cell (`text` starting at `x`, baseline `y`) into `out`.
fn cell(out: &mut Vec<PositionedGlyph>, text: &str, x: f64, y: f64, size: f64) {
    for (index, ch) in text.chars().enumerate() {
        out.push(glyph(
            &ch.to_string(),
            x + index as f64 * 0.5 * size,
            y,
            size,
        ));
    }
}

fn ordered_block_texts(tp: &pdf_text::model::TextPage) -> Vec<String> {
    tp.blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Text)
        .map(|b| {
            b.lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.text.as_str())
                .collect()
        })
        .collect()
}

/// Builds a two-column grid painted row-major: `rows` rows at `pitch`×`size`
/// baseline spacing, left cells at x=68 and right cells at x=303. When
/// `unpaired` is set every fourth row has no right cell (the empty-cell form of
/// a real correlation table).
fn two_column_grid(rows: usize, pitch: f64, unpaired: bool) -> Vec<PositionedGlyph> {
    let size = 10.0;
    let mut gs = Vec::new();
    for row in 0..rows {
        let baseline = 720.0 - row as f64 * pitch * size;
        cell(
            &mut gs,
            &format!("Article-{row:02}-old"),
            68.0,
            baseline,
            size,
        );
        if !(unpaired && row % 4 == 3) {
            cell(
                &mut gs,
                &format!("Article-{row:02}-new"),
                303.0,
                baseline,
                size,
            );
        }
    }
    gs
}

/// LAYOUT-ORDER-004: a two-column correlation table (the EUR-Lex annex form —
/// cells far apart, rows spaced well beyond the prose leading, and rows whose
/// right cell is empty) must read **row-major**, the way the page is painted.
/// The XY-cut would otherwise emit the whole left column and then the whole
/// right one, which is where pdfspine diverged from the official text.
#[test]
fn layout_order_004_two_column_record_grid_reads_row_major() {
    let tp = textpage_from_glyphs(&two_column_grid(14, 2.1, true), &[], letter(), 0);
    let want: Vec<String> = (0..14)
        .map(|row| {
            if row % 4 == 3 {
                format!("Article-{row:02}-old")
            } else {
                format!("Article-{row:02}-oldArticle-{row:02}-new")
            }
        })
        .collect();
    assert_eq!(ordered_block_texts(&tp), want);
}

/// LAYOUT-ORDER-005: two-column prose at an ordinary leading keeps column-major
/// order even when painted row-major. The record-grid path must not swallow it.
#[test]
fn layout_order_005_two_column_prose_stays_column_major() {
    let tp = textpage_from_glyphs(&two_column_grid(14, 1.2, false), &[], letter(), 0);
    let joined = ordered_block_texts(&tp).join("|");
    assert!(
        joined.find("Article-13-old").unwrap() < joined.find("Article-00-new").unwrap(),
        "two-column prose read row-major: {joined}"
    );
}

/// LAYOUT-ORDER-006: a table-pitch two-column layout whose rows are *all*
/// paired stays column-major — with no empty cell anywhere the two columns are
/// indistinguishable from two parallel text flows.
#[test]
fn layout_order_006_fully_paired_columns_stay_column_major() {
    let tp = textpage_from_glyphs(&two_column_grid(14, 2.1, false), &[], letter(), 0);
    let joined = ordered_block_texts(&tp).join("|");
    assert!(
        joined.find("Article-13-old").unwrap() < joined.find("Article-00-new").unwrap(),
        "fully paired columns read row-major: {joined}"
    );
}
