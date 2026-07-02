//! TS-5 text-box acceptance (PRD §10): rect containment ± 1 pt, VAnchor
//! first-baseline positions ± 1 pt, wrap-off hard breaks, autofit with zero
//! words lost, rotation read-back + non-blank raster, clip + overflow warning.

mod common;

use common::*;
use pdf_typeset::{Block, ExportWarning, Op, PageOps, ParaProps, Rect, Run, TextBoxSpec, VAnchor};

/// Emits a single fixed page carrying one laid-out text box.
fn export_box(spec: &TextBoxSpec) -> (Vec<Op>, pdf_typeset::ExportResult) {
    export_box_on(spec, 400.0, 500.0)
}

fn export_box_on(spec: &TextBoxSpec, pw: f64, ph: f64) -> (Vec<Op>, pdf_typeset::ExportResult) {
    let mut engine = ts();
    let ops = engine.layout_text_box(spec);
    let page = PageOps {
        width: pw,
        height: ph,
        ops: ops.clone(),
    };
    let result = engine.emit(&[page]).expect("emit");
    (ops, result)
}

#[test]
fn all_words_land_inside_the_box_rect() {
    let rect = Rect {
        x0: 100.0,
        y0: 100.0,
        x1: 300.0,
        y1: 200.0,
    };
    let spec = TextBoxSpec::new(
        rect,
        vec![para("boxed words wrap neatly inside this rect", 12.0)],
    );
    let (_, result) = export_box(&spec);
    let ws = words(&result.pdf, 0);
    assert!(!ws.is_empty());
    for w in &ws {
        assert!(
            w.0 >= 99.0 && w.2 <= 301.0 && w.1 >= 99.0 && w.3 <= 201.0,
            "word {:?} escapes the box: ({}, {}, {}, {})",
            w.4,
            w.0,
            w.1,
            w.2,
            w.3
        );
    }
}

#[test]
fn middle_and_bottom_anchors_hit_expected_first_baselines() {
    let rect = Rect {
        x0: 100.0,
        y0: 100.0,
        x1: 300.0,
        y1: 200.0,
    };
    let (_, desc, _) = liberation_metrics();
    let natural = natural_line_height(12.0);

    for (anchor, expected_baseline) in [
        (VAnchor::Top, 100.0 + natural - desc * 12.0),
        (
            VAnchor::Middle,
            100.0 + (100.0 - natural) / 2.0 + natural - desc * 12.0,
        ),
        (VAnchor::Bottom, 200.0 - desc * 12.0),
    ] {
        let mut spec = TextBoxSpec::new(rect, vec![para("Anchored", 12.0)]);
        spec.v_anchor = anchor;
        let (_, result) = export_box(&spec);
        let ws = words(&result.pdf, 0);
        assert_eq!(ws.len(), 1);
        let baseline = ws[0].3 - desc * 12.0;
        assert_near(
            baseline,
            expected_baseline,
            1.0,
            &format!("{anchor:?} first baseline"),
        );
    }
}

#[test]
fn wrap_off_breaks_only_at_hard_newlines() {
    let rect = Rect {
        x0: 100.0,
        y0: 100.0,
        x1: 150.0,
        y1: 200.0,
    };
    let mut spec = TextBoxSpec::new(
        rect,
        vec![para("these words would surely wrap\nshort", 12.0)],
    );
    spec.wrap = false;
    let (_, result) = export_box(&spec);
    let ws = words(&result.pdf, 0);
    let mut bottoms: Vec<f64> = ws.iter().map(|w| w.3).collect();
    bottoms.sort_by(f64::total_cmp);
    bottoms.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    assert_eq!(bottoms.len(), 2, "exactly two lines (one hard break)");
    let max_x1 = ws.iter().map(|w| w.2).fold(f64::MIN, f64::max);
    assert!(
        max_x1 > 150.0,
        "wrap-off line overflows the 50 pt box (x1 = {max_x1})"
    );
}

#[test]
fn autofit_scales_down_until_zero_words_lost() {
    let rect = Rect {
        x0: 100.0,
        y0: 100.0,
        x1: 400.0,
        y1: 140.0,
    };
    let text = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8"; // ≈ 8 × 13.8 pt ≫ 40 pt box
    let mut spec = TextBoxSpec::new(rect, vec![para(text, 12.0)]);
    spec.font_scale = Some(1.0);
    let (_, result) = export_box(&spec);
    let ws = words(&result.pdf, 0);
    assert_eq!(ws.len(), 8, "zero words lost in read-back");
    for w in &ws {
        assert!(
            w.1 >= 99.0 && w.3 <= 141.0,
            "autofit word {:?} outside the box: y = ({}, {})",
            w.4,
            w.1,
            w.3
        );
    }
    let tallest = ws.iter().map(|w| w.3 - w.1).fold(f64::MIN, f64::max);
    assert!(
        tallest < 8.0,
        "autofit must have shrunk 12 pt text (cell height = {tallest})"
    );
}

#[test]
fn rotated_box_still_reads_back_and_rasters_ink() {
    let rect = Rect {
        x0: 100.0,
        y0: 100.0,
        x1: 300.0,
        y1: 160.0,
    };
    let mut spec = TextBoxSpec::new(rect, vec![para("Rotated words", 14.0)]);
    spec.rotation_deg = 90.0;
    let (ops, result) = export_box(&spec);
    assert!(
        matches!(
            ops.as_slice(),
            [Op::Group {
                transform: Some(_),
                ..
            }]
        ),
        "rotation wraps the box in one transformed group"
    );
    let toks = tokens(&result.pdf);
    assert!(toks.contains(&"Rotated".to_string()), "{toks:?}");
    assert!(toks.contains(&"words".to_string()));
    let pix = render(&result.pdf, 0);
    assert!(ink_pixels(&pix) > 50, "rotated text must raster non-blank");

    // The 90° rotation swaps the text's extent axes: the rotated word column
    // is taller than it is wide.
    let ws = words(&result.pdf, 0);
    let min_x = ws.iter().map(|w| w.0).fold(f64::MAX, f64::min);
    let max_x = ws.iter().map(|w| w.2).fold(f64::MIN, f64::max);
    let min_y = ws.iter().map(|w| w.1).fold(f64::MAX, f64::min);
    let max_y = ws.iter().map(|w| w.3).fold(f64::MIN, f64::max);
    assert!(
        (max_y - min_y) > (max_x - min_x),
        "rotated extent: dx = {}, dy = {}",
        max_x - min_x,
        max_y - min_y
    );
}

#[test]
fn clip_clips_overflow_and_warns_once() {
    let rect = Rect {
        x0: 50.0,
        y0: 50.0,
        x1: 150.0,
        y1: 90.0,
    };
    // Highlighted runs: the overflow leaves *fill* ops below the box, which
    // the repo rasterizer clips faithfully (its glyph path does not apply
    // soft clips yet, so the visual oracle keys off the red highlight rects).
    let mut hl = style(12.0);
    hl.highlight = Some(pdf_typeset::Rgb::new(1.0, 0.0, 0.0));
    let overflowing = Block::Paragraph(
        ParaProps::new(),
        vec![Run::new("one\ntwo\nthree\nfour\nfive\nsix", hl)],
    );

    let mut clipped = TextBoxSpec::new(rect, vec![overflowing.clone()]);
    clipped.clip = true;
    let (ops, result) = export_box_on(&clipped, 200.0, 200.0);
    assert!(
        matches!(ops.as_slice(), [Op::Group { clip: Some(_), .. }]),
        "clip wraps the box in one clipped group"
    );
    let overflow_warnings: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| match w {
            ExportWarning::BoxOverflowClipped { overflow_pt } => Some(*overflow_pt),
            _ => None,
        })
        .collect();
    assert_eq!(overflow_warnings.len(), 1, "{:?}", result.warnings);
    assert!(overflow_warnings[0] > 30.0, "≈ 6 lines in a 40 pt box");
    assert!(raw(&result.pdf).contains("W n"), "clip path in the content");

    let mut unclipped = TextBoxSpec::new(rect, vec![overflowing]);
    unclipped.clip = false;
    let (_, un_result) = export_box_on(&unclipped, 200.0, 200.0);
    assert!(un_result.warnings.is_empty(), "{:?}", un_result.warnings);

    // Below the box bottom (y > 90) the clipped page carries no highlight
    // fill; the unclipped page keeps painting it.
    let red_below = |pix: &pdf_api::Pixmap, from_row: usize| {
        let n = usize::from(pix.colorspace.components()) + usize::from(pix.alpha);
        let stride = pix.width as usize * n;
        pix.samples()[from_row * stride..]
            .chunks(n)
            .filter(|px| px[0] > 200 && px[1] < 100 && px[2] < 100)
            .count()
    };
    let clipped_pix = render(&result.pdf, 0);
    let unclipped_pix = render(&un_result.pdf, 0);
    assert_eq!(
        red_below(&clipped_pix, 95),
        0,
        "no highlight fill below the clipped box"
    );
    assert!(
        red_below(&unclipped_pix, 95) > 50,
        "unclipped overflow keeps painting"
    );
}

#[test]
fn page_break_is_ignored_inside_boxes() {
    let rect = Rect {
        x0: 100.0,
        y0: 100.0,
        x1: 300.0,
        y1: 200.0,
    };
    let spec = TextBoxSpec::new(
        rect,
        vec![para("before", 12.0), Block::PageBreak, para("after", 12.0)],
    );
    let (_, result) = export_box(&spec);
    assert_eq!(open(&result.pdf).page_count(), 1);
    let toks = tokens(&result.pdf);
    assert_eq!(toks, ["before", "after"]);
}

#[test]
fn empty_box_produces_no_ops_and_no_warnings() {
    let rect = Rect {
        x0: 10.0,
        y0: 10.0,
        x1: 50.0,
        y1: 50.0,
    };
    let mut engine = ts();
    let ops = engine.layout_text_box(&TextBoxSpec::new(
        rect,
        vec![Block::Paragraph(ParaProps::new(), Vec::<Run>::new())],
    ));
    assert!(ops.is_empty());
    assert!(engine.warnings().is_empty());
}
