//! TS-10 public measure-API acceptance (PRD §10): `measure_blocks` /
//! `measure_text_box` report per-line ascent/descent/height and totals that
//! agree **point-for-point** with what the emitters actually lay out (same
//! measure → wrap → line-box path), so consumers (pptspine autofit, docspine
//! cell vertical alignment, tables grown to content) can size before emitting.

mod common;

use common::*;
use pdf_typeset::{Block, LineSpacing, PageOps, ParaProps, Rect, Run, TextBoxSpec, VAnchor};

/// A paragraph with an explicit line-spacing rule.
fn para_spaced(text: &str, size: f64, spacing: LineSpacing) -> Block {
    let mut props = ParaProps::new();
    props.spacing = spacing;
    Block::Paragraph(props, vec![Run::new(text, style(size))])
}

#[test]
fn single_line_reports_face_metrics() {
    let mut e = ts();
    let m = e.measure_blocks(&[para("Hello", 20.0)], 500.0, true);

    assert_eq!(m.lines.len(), 1, "one wrapped line");
    let (asc, desc, _) = liberation_metrics();
    assert_near(m.lines[0].ascent, asc * 20.0, 1e-6, "line ascent");
    assert_near(m.lines[0].descent, desc * 20.0, 1e-6, "line descent");
    assert_near(m.lines[0].height, natural_line_height(20.0), 1e-6, "line height");
    assert_near(m.height, natural_line_height(20.0), 1e-6, "total height");
    assert!(
        m.max_width > 0.0 && m.max_width < 500.0,
        "natural width is the inked extent ({})",
        m.max_width
    );
}

#[test]
fn total_height_equals_sum_of_line_heights() {
    let mut e = ts();
    let text = "one two three four five six seven eight nine ten eleven twelve";
    let m = e.measure_blocks(&[para(text, 12.0)], 80.0, true);

    assert!(m.lines.len() >= 2, "narrow width forces wrapping");
    let sum: f64 = m.lines.iter().map(|l| l.height).sum();
    assert_near(m.height, sum, 1e-6, "height == Σ line heights");
    for l in &m.lines {
        assert_near(l.height, natural_line_height(12.0), 1e-6, "single-spaced line");
    }
}

#[test]
fn line_spacing_multiple_scales_line_height() {
    let mut e = ts();
    let single = e.measure_blocks(&[para("line one\nline two", 12.0)], 500.0, true);
    let double = e.measure_blocks(
        &[para_spaced("line one\nline two", 12.0, LineSpacing::Multiple(2.0))],
        500.0,
        true,
    );

    assert_eq!(single.lines.len(), 2);
    assert_eq!(double.lines.len(), 2);
    for l in &double.lines {
        assert_near(l.height, natural_line_height(12.0) * 2.0, 1e-6, "double-spaced");
        // ascent/descent are the raw font metrics regardless of spacing.
        let (asc, desc, _) = liberation_metrics();
        assert_near(l.ascent, asc * 12.0, 1e-6, "ascent unchanged by spacing");
        assert_near(l.descent, desc * 12.0, 1e-6, "descent unchanged by spacing");
    }
    assert_near(double.height, 2.0 * single.height, 1e-6, "twice the total");
}

#[test]
fn wrap_off_keeps_one_line() {
    let mut e = ts();
    let m = e.measure_blocks(&[para("these words would surely wrap here", 12.0)], 40.0, false);
    assert_eq!(m.lines.len(), 1, "wrap-off breaks only at hard \\n");
    assert!(m.max_width > 40.0, "the single line overflows the width");
}

#[test]
fn empty_paragraph_measures_nothing() {
    let mut e = ts();
    let m = e.measure_blocks(&[Block::Paragraph(ParaProps::new(), Vec::<Run>::new())], 200.0, true);
    assert!(m.lines.is_empty(), "no runs ⇒ no line box");
    assert_near(m.height, 0.0, 1e-6, "no height");
    assert_near(m.max_width, 0.0, 1e-6, "no width");
}

#[test]
fn measure_text_box_matches_measure_blocks_at_box_width() {
    let mut e = ts();
    let blocks = vec![para("some wrapping text here to fill several lines nicely", 12.0)];
    let rect = Rect { x0: 100.0, y0: 100.0, x1: 300.0, y1: 400.0 };
    let spec = TextBoxSpec::new(rect, blocks.clone());

    let via_box = e.measure_text_box(&spec);
    let via_blocks = e.measure_blocks(&blocks, 200.0, true);
    assert_eq!(via_box, via_blocks, "text box measures its content at rect width");
    assert!(via_box.lines.len() >= 2);
}

/// The pin: the measured height is exactly the content height a `VAnchor`
/// text box anchors within its rect, so a middle-anchored box's first baseline
/// falls where `measure_text_box` predicts (read back through the real stack).
#[test]
fn measured_height_matches_text_box_layout() {
    let rect = Rect { x0: 100.0, y0: 100.0, x1: 300.0, y1: 400.0 };
    let blocks = vec![para("point consistency between measure and real layout here", 12.0)];

    let mut e = ts();
    let mut spec = TextBoxSpec::new(rect, blocks);
    spec.v_anchor = VAnchor::Middle;
    let m = e.measure_text_box(&spec);
    assert!(m.lines.len() >= 2, "content wraps to several lines");

    let ops = e.layout_text_box(&spec);
    let page = PageOps { width: 400.0, height: 500.0, ops };
    let result = e.emit(&[page]).expect("emit");
    let ws = words(&result.pdf, 0);
    assert!(!ws.is_empty());

    // The first laid-out line is the topmost; its baseline sits `descent`
    // below the words' bottom edge. Middle anchor offsets content by
    // (box_h − content_h) / 2, so the predicted baseline is a pure function of
    // the measured height and first-line metrics.
    let top = ws.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    let read_baseline = top.3 - m.lines[0].descent;
    let box_h = rect.y1 - rect.y0;
    let predicted = rect.y0 + (box_h - m.height) / 2.0 + m.lines[0].height - m.lines[0].descent;
    assert_near(read_baseline, predicted, 1.0, "measure height drives the anchor offset");
}
