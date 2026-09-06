//! `LineHeightRule` acceptance: the default keeps the real face-metric line
//! boxes byte-for-byte, while `FontIndependent` gives PowerPoint / Impress
//! font-independent spacing (line height 1.2 em, baseline 1.0 em below the line
//! top) engine-wide — flow, text boxes, table cells and the measure API.

mod common;

use common::*;
use pdf_typeset::{
    Block, BorderEdge, CellBorders, ColumnWidth, FixedPages, LineHeightRule, LineSpacing, Op,
    PageGeom, PageOps, ParaProps, Rect, Rgb, Run, TableCell, TableRow, TableSpec, TextBoxSpec,
    VAnchor,
};

/// The baselines of a flat op list's `Op::Text`, in emission order.
fn text_baselines(ops: &[Op]) -> Vec<f64> {
    ops.iter()
        .filter_map(|op| match op {
            Op::Text { baseline, .. } => Some(*baseline),
            _ => None,
        })
        .collect()
}

/// Sorted, near-coincident-deduped text baselines (mixed-size ops on one line
/// share a baseline, so they collapse to a single value).
fn distinct_baselines(ops: &[Op]) -> Vec<f64> {
    let mut bs = text_baselines(ops);
    bs.sort_by(f64::total_cmp);
    bs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    bs
}

/// A paragraph of one plain run under an explicit line-spacing rule.
fn para_spaced(text: &str, size: f64, spacing: LineSpacing) -> Block {
    let mut props = ParaProps::new();
    props.spacing = spacing;
    Block::Paragraph(props, vec![Run::new(text, style(size))])
}

/// A font-independent engine.
fn fi() -> pdf_typeset::Typesetter {
    let mut e = ts();
    e.set_line_height_rule(LineHeightRule::FontIndependent);
    e
}

// --- (a) default is FontMetrics and provably unchanged -----------------------

#[test]
fn default_rule_is_font_metrics_and_unchanged() {
    assert_eq!(ts().line_height_rule(), LineHeightRule::FontMetrics);

    let rect = Rect {
        x0: 72.0,
        y0: 180.0,
        x1: 648.0,
        y1: 460.0,
    };
    let mut e = ts();
    let ops = e.layout_text_box(&TextBoxSpec::new(rect, vec![para("Hello world", 18.0)]));
    let (_, desc, _) = liberation_metrics();
    let bs = text_baselines(&ops);
    assert_eq!(bs.len(), 1, "one merged line op: {bs:?}");
    assert_near(
        bs[0],
        180.0 + natural_line_height(18.0) - desc * 18.0,
        1e-6,
        "default (face-metric) first baseline",
    );
}

// --- (b) FontIndependent single line -----------------------------------------

#[test]
fn font_independent_single_line_baseline() {
    let rect = Rect {
        x0: 72.0,
        y0: 180.0,
        x1: 648.0,
        y1: 460.0,
    };
    let mut e = fi();
    let ops = e.layout_text_box(&TextBoxSpec::new(rect, vec![para("First body line", 18.0)]));
    let bs = text_baselines(&ops);
    assert_eq!(bs.len(), 1, "{bs:?}");
    // baseline = top + ascent(1.0 em) = 180 + 18.
    assert_near(bs[0], 180.0 + 18.0, 1e-6, "font-independent first baseline");
}

// --- (c) FontIndependent pitch mirrors the LO oracle -------------------------

#[test]
fn font_independent_pitch_matches_lo_body() {
    let rect = Rect {
        x0: 72.0,
        y0: 180.0,
        x1: 648.0,
        y1: 460.0,
    };
    let body: Vec<Block> = [
        "First body line rendered at eighteen points",
        "Second body line for the advisory comparison",
        "Third body line keeps the layout deliberately plain",
        "Fourth body line closes the sample slide",
    ]
    .iter()
    .map(|t| {
        let mut p = ParaProps::new();
        p.space_after = 8.0;
        Block::Paragraph(p, vec![Run::new(*t, style(18.0))])
    })
    .collect();

    let mut e = fi();
    let spec = TextBoxSpec::new(rect, body);
    let ops = e.layout_text_box(&spec);
    let bs = text_baselines(&ops);
    assert_eq!(bs.len(), 4, "one line per paragraph: {bs:?}");
    // pitch = line height (21.6) + space_after (8) = 29.6.
    for (got, want) in bs.iter().zip([198.0, 227.6, 257.2, 286.8]) {
        assert_near(*got, want, 1e-6, "body baseline");
    }

    // Read back through the emitted PDF: the first word's bottom minus the real
    // glyph descent recovers the baseline (~198).
    let page = PageOps {
        width: 720.0,
        height: 540.0,
        ops,
    };
    let result = e.emit(&[page]).expect("emit");
    let ws = words(&result.pdf, 0);
    let (_, desc, _) = liberation_metrics();
    let top = ws
        .iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("at least one word");
    assert_near(top.3 - desc * 18.0, 198.0, 0.5, "read-back first baseline");
}

// --- (d) mixed sizes share one baseline --------------------------------------

#[test]
fn font_independent_mixed_sizes_share_one_baseline() {
    let rect = Rect {
        x0: 72.0,
        y0: 100.0,
        x1: 648.0,
        y1: 460.0,
    };
    let mut e = fi();
    // Line 0 mixes 12 pt and 24 pt; a hard break opens line 1 (24 pt).
    let mixed = Block::Paragraph(
        ParaProps::new(),
        vec![
            Run::new("small ", style(12.0)),
            Run::new("BIG\nnext", style(24.0)),
        ],
    );
    let ops = e.layout_text_box(&TextBoxSpec::new(rect, vec![mixed]));
    let bs = distinct_baselines(&ops);
    assert_eq!(bs.len(), 2, "two lines: {bs:?}");
    // baseline = top + largest size on the line (24).
    assert_near(bs[0], 100.0 + 24.0, 1e-6, "shared baseline at top + 24");
    // next line is 1.2 * 24 = 28.8 lower.
    assert_near(
        bs[1] - bs[0],
        28.8,
        1e-6,
        "next line 1.2 em of the max size",
    );
}

// --- (e) empty paragraph advances one 1.2-em line ----------------------------

#[test]
fn font_independent_empty_paragraph_advances_one_line() {
    let mut e = fi();
    let empty = Block::Paragraph(ParaProps::new(), vec![Run::new("", style(18.0))]);
    let m = e.measure_blocks(&[empty], 500.0, true);
    assert_eq!(
        m.lines.len(),
        1,
        "the paragraph-mark run still sizes one line"
    );
    assert_near(m.lines[0].height, 21.6, 1e-6, "empty line height 1.2 em");
    assert_near(m.height, 21.6, 1e-6, "advances 21.6");
}

// --- (f) measure API under the rule ------------------------------------------

#[test]
fn font_independent_measure_line_metrics_and_spacing() {
    let mut e = fi();

    let base = e.measure_blocks(&[para("Hello", 18.0)], 500.0, true);
    assert_eq!(base.lines.len(), 1);
    assert_near(base.lines[0].ascent, 18.0, 1e-6, "ascent 1.0 em");
    assert_near(base.lines[0].descent, 3.6, 1e-6, "descent 0.2 em");
    assert_near(base.lines[0].height, 21.6, 1e-6, "height 1.2 em");

    let mult = e.measure_blocks(
        &[para_spaced("Hello", 18.0, LineSpacing::Multiple(1.5))],
        500.0,
        true,
    );
    assert_near(mult.lines[0].height, 32.4, 1e-6, "1.5 * 21.6");
    assert_near(
        mult.lines[0].ascent,
        18.0,
        1e-6,
        "ascent unchanged by spacing",
    );
    assert_near(
        mult.lines[0].descent,
        3.6,
        1e-6,
        "descent unchanged by spacing",
    );

    let exact = e.measure_blocks(
        &[para_spaced("Hello", 18.0, LineSpacing::Exact(30.0))],
        500.0,
        true,
    );
    assert_near(exact.lines[0].height, 30.0, 1e-6, "exact 30");

    let least_lo = e.measure_blocks(
        &[para_spaced("Hello", 18.0, LineSpacing::AtLeast(10.0))],
        500.0,
        true,
    );
    assert_near(
        least_lo.lines[0].height,
        21.6,
        1e-6,
        "atLeast below natural",
    );

    let least_hi = e.measure_blocks(
        &[para_spaced("Hello", 18.0, LineSpacing::AtLeast(40.0))],
        500.0,
        true,
    );
    assert_near(
        least_hi.lines[0].height,
        40.0,
        1e-6,
        "atLeast above natural",
    );
}

// --- (g) the rule is engine-wide ---------------------------------------------

#[test]
fn font_independent_applies_to_paged_flow() {
    let mut e = fi();
    let blocks = vec![
        para("First flow line", 18.0),
        para("Second flow line", 18.0),
    ];
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let pages = e.layout_flow(&blocks, &mut FixedPages::new(geom));
    let bs = text_baselines(&pages[0].ops);
    assert_eq!(bs.len(), 2, "{bs:?}");
    assert_near(bs[0], 50.0 + 18.0, 1e-6, "flow first baseline = top + 18");
    assert_near(bs[1] - bs[0], 21.6, 1e-6, "flow pitch = 1.2 em");
}

#[test]
fn font_independent_applies_to_table_rows() {
    let mut e = fi();
    let edge = BorderEdge {
        width: 1.0,
        color: Rgb::new(0.0, 0.0, 0.0),
    };
    let mut cell = TableCell::new(vec![para("Cell", 18.0)]);
    cell.padding = 4.0;
    cell.borders = CellBorders {
        top: Some(edge),
        right: None,
        bottom: Some(edge),
        left: None,
    };
    let table = TableSpec::new(
        vec![ColumnWidth::Fixed(200.0)],
        vec![TableRow::new(vec![cell])],
    );
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let pages = e.layout_flow(&[Block::Table(table)], &mut FixedPages::new(geom));

    // The top and bottom borders are the two horizontal (y1 == y2) line ops;
    // their gap is the row height = content (1.2 em = 21.6) + 2 * padding.
    let ys: Vec<f64> = pages[0]
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Line { y1, y2, .. } if (y1 - y2).abs() < 1e-6 => Some(*y1),
            _ => None,
        })
        .collect();
    assert_eq!(ys.len(), 2, "top + bottom horizontal borders: {ys:?}");
    assert_near(
        (ys[0] - ys[1]).abs(),
        21.6 + 8.0,
        1e-6,
        "row height = 1.2-em content + 2 * padding",
    );
}

#[test]
fn font_independent_measure_and_layout_agree_for_middle_anchor() {
    let rect = Rect {
        x0: 72.0,
        y0: 100.0,
        x1: 400.0,
        y1: 300.0,
    };
    let mut e = fi();
    let mut spec = TextBoxSpec::new(rect, vec![para("Anchored middle", 18.0)]);
    spec.v_anchor = VAnchor::Middle;

    let m = e.measure_text_box(&spec);
    let ops = e.layout_text_box(&spec);
    let bs = text_baselines(&ops);
    assert!(!bs.is_empty());
    let box_h = rect.y1 - rect.y0;
    // ascent (1.0 em) = 18 for the first line under the rule.
    let predicted = rect.y0 + (box_h - m.height) / 2.0 + 18.0;
    assert_near(bs[0], predicted, 1e-6, "measure height drives the anchor");
}

// --- (h) the rule only affects layouts run after it is set -------------------

#[test]
fn setting_rule_after_layout_only_affects_later_layouts() {
    let rect = Rect {
        x0: 72.0,
        y0: 100.0,
        x1: 400.0,
        y1: 300.0,
    };
    let mut e = ts();
    let spec = TextBoxSpec::new(rect, vec![para("Some text here", 18.0)]);
    let first = e.layout_text_box(&spec);
    e.set_line_height_rule(LineHeightRule::FontIndependent);
    let second = e.layout_text_box(&spec);
    assert_ne!(
        text_baselines(&first),
        text_baselines(&second),
        "the second layout uses the new rule; the first stays as produced",
    );
}
