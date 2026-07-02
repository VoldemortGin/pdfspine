//! TS-4 flow-layout acceptance: read-back fidelity, justify geometry, shared
//! baselines, spacing / indents / lists / decorations, pagination — all on the
//! deterministic bundled-face resolver (no system-font dependence).

mod common;

use common::*;
use pdf_typeset::{
    Align, Block, ExportWarning, LineSpacing, ListLabel, Op, PageGeom, ParaProps, Rgb, Run,
};

/// The op-IR baselines of every text op on one page, in emission order.
fn text_baselines(ops: &[Op]) -> Vec<f64> {
    ops.iter()
        .filter_map(|op| match op {
            Op::Text { baseline, .. } => Some(*baseline),
            _ => None,
        })
        .collect()
}

/// Distinct line baselines (words grouped by cell bottom = baseline + descent
/// × size; single-size fixtures only), ascending.
fn word_lines(ws: &[pdf_api::WordTuple]) -> Vec<(f64, Vec<pdf_api::WordTuple>)> {
    let mut lines: Vec<(f64, Vec<pdf_api::WordTuple>)> = Vec::new();
    for w in ws {
        match lines.iter_mut().find(|(y, _)| (w.3 - *y).abs() < 0.5) {
            Some((_, line)) => line.push(w.clone()),
            None => lines.push((w.3, vec![w.clone()])),
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

#[test]
fn paragraph_reads_back_exactly() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let (_, result) = export(&[para("Alpha beta gamma delta", 12.0)], geom);
    assert_eq!(tokens(&result.pdf), ["Alpha", "beta", "gamma", "delta"]);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

#[test]
fn consecutive_spaces_are_preserved() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let (_, result) = export(&[para("a  b", 12.0)], geom);
    let ws = words(&result.pdf, 0);
    assert_eq!(ws.len(), 2);
    let gap = ws[1].0 - ws[0].2;
    let single = {
        let (_, r2) = export(&[para("a b", 12.0)], geom);
        let ws2 = words(&r2.pdf, 0);
        ws2[1].0 - ws2[0].2
    };
    assert_near(gap, 2.0 * single, 0.01, "double space gap");
}

#[test]
fn long_text_wraps_and_paginates_without_loss() {
    let geom = PageGeom::new(300.0, 160.0, 40.0);
    let text: Vec<String> = (0..120).map(|i| format!("w{i:03}")).collect();
    let (_, result) = export(&[para(&text.join(" "), 12.0)], geom);
    let doc = open(&result.pdf);
    assert!(doc.page_count() > 1, "should paginate");
    assert_eq!(tokens(&result.pdf), text, "no token lost across pages");
}

#[test]
fn justify_interior_lines_reach_the_right_edge() {
    // Content column: 50 .. 350 (width 300).
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let text = "The quick brown fox jumps over a lazy dog while newly minted \
                glyph runs keep flowing toward both margins evenly end";
    let mut props = ParaProps::new();
    props.align = Align::Justify;
    let blocks = vec![Block::Paragraph(props, vec![Run::new(text, style(12.0))])];
    let (_, result) = export(&blocks, geom);
    let ws = words(&result.pdf, 0);
    let lines = word_lines(&ws);
    assert!(lines.len() >= 3, "fixture should wrap to ≥ 3 lines");
    for (i, (_, line)) in lines.iter().enumerate() {
        let right = line.iter().map(|w| w.2).fold(f64::MIN, f64::max);
        if i + 1 < lines.len() {
            assert_near(right, 350.0, 0.5, &format!("line {i} right edge"));
        } else {
            assert!(right < 349.5, "last line stays left (right = {right})");
        }
    }
    assert_eq!(
        tokens(&result.pdf).len(),
        text.split_whitespace().count(),
        "justify must not lose words"
    );
}

#[test]
fn justify_hard_broken_line_stays_left() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut props = ParaProps::new();
    props.align = Align::Justify;
    let text = "short line\nthe following words fill and wrap across the column \
                width to make at least two more lines of justified output here";
    let blocks = vec![Block::Paragraph(props, vec![Run::new(text, style(12.0))])];
    let (_, result) = export(&blocks, geom);
    let lines = word_lines(&words(&result.pdf, 0));
    let first_right = lines[0].1.iter().map(|w| w.2).fold(f64::MIN, f64::max);
    assert!(
        first_right < 200.0,
        "hard-broken first line must not justify (right = {first_right})"
    );
}

#[test]
fn mixed_sizes_share_one_baseline() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let blocks = vec![Block::Paragraph(
        ParaProps::new(),
        vec![
            Run::new("small ", style(10.0)),
            Run::new("BIG", style(24.0)),
        ],
    )];
    let (pages, result) = export(&blocks, geom);
    let baselines = text_baselines(&pages[0].ops);
    assert_eq!(baselines.len(), 2, "two sizes ⇒ two text ops");
    assert_near(baselines[0], baselines[1], 1e-9, "op-IR shared baseline");

    // PRD acceptance phrasing: assert on get_text_words coordinates too.
    let (_, desc, _) = liberation_metrics();
    let ws = words(&result.pdf, 0);
    assert_eq!(ws.len(), 2);
    let small = ws.iter().find(|w| w.4 == "small").expect("small word");
    let big = ws.iter().find(|w| w.4 == "BIG").expect("big word");
    let b_small = small.3 - desc * 10.0;
    let b_big = big.3 - desc * 24.0;
    assert_near(b_small, b_big, 0.5, "words-derived shared baseline");
}

#[test]
fn space_before_and_after_accumulate_between_paragraphs() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut a = ParaProps::new();
    a.space_after = 10.0;
    let mut b = ParaProps::new();
    b.space_before = 5.0;
    let blocks = vec![
        Block::Paragraph(a, vec![Run::new("one", style(12.0))]),
        Block::Paragraph(b, vec![Run::new("two", style(12.0))]),
    ];
    let (pages, _) = export(&blocks, geom);
    let baselines = text_baselines(&pages[0].ops);
    let pitch = baselines[1] - baselines[0];
    assert_near(
        pitch,
        natural_line_height(12.0) + 15.0,
        1e-6,
        "space_after + space_before add",
    );
}

#[test]
fn space_before_collapses_at_page_top() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut props = ParaProps::new();
    props.space_before = 100.0;
    let blocks = vec![Block::Paragraph(props, vec![Run::new("top", style(12.0))])];
    let (pages, _) = export(&blocks, geom);
    let (_, desc, _) = liberation_metrics();
    let expected = 50.0 + natural_line_height(12.0) - desc * 12.0;
    assert_near(
        text_baselines(&pages[0].ops)[0],
        expected,
        1e-6,
        "collapsed space_before",
    );
}

#[test]
fn line_spacing_multiple_and_exact() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let (_, desc, _) = liberation_metrics();

    let mut double = ParaProps::new();
    double.spacing = LineSpacing::Multiple(2.0);
    let blocks = vec![Block::Paragraph(
        double,
        vec![Run::new("one\ntwo", style(12.0))],
    )];
    let (pages, _) = export(&blocks, geom);
    let b = text_baselines(&pages[0].ops);
    assert_near(
        b[1] - b[0],
        2.0 * natural_line_height(12.0),
        1e-6,
        "double-spaced pitch",
    );

    let mut exact = ParaProps::new();
    exact.spacing = LineSpacing::Exact(20.0);
    let blocks = vec![Block::Paragraph(
        exact,
        vec![Run::new("one\ntwo", style(12.0))],
    )];
    let (pages, _) = export(&blocks, geom);
    let b = text_baselines(&pages[0].ops);
    assert_near(b[1] - b[0], 20.0, 1e-6, "exact pitch");
    assert_near(
        b[0],
        50.0 + 20.0 - desc * 12.0,
        1e-6,
        "exact first baseline",
    );
}

#[test]
fn first_line_and_hanging_indents_shift_line_starts() {
    let geom = PageGeom::new(300.0, 500.0, 50.0);
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa \
                lambda mu nu xi omicron pi rho sigma tau upsilon";

    let mut first = ParaProps::new();
    first.indent_left = 40.0;
    first.first_line_indent = 20.0;
    let (_, result) = export(
        &[Block::Paragraph(first, vec![Run::new(text, style(12.0))])],
        geom,
    );
    let lines = word_lines(&words(&result.pdf, 0));
    assert!(lines.len() >= 2);
    assert_near(lines[0].1[0].0, 110.0, 0.5, "first-line indent start");
    assert_near(lines[1].1[0].0, 90.0, 0.5, "continuation start");

    let mut hanging = ParaProps::new();
    hanging.indent_left = 40.0;
    hanging.first_line_indent = -20.0;
    let (_, result) = export(
        &[Block::Paragraph(hanging, vec![Run::new(text, style(12.0))])],
        geom,
    );
    let lines = word_lines(&words(&result.pdf, 0));
    assert!(lines.len() >= 2);
    assert_near(lines[0].1[0].0, 70.0, 0.5, "hanging first line start");
    assert_near(lines[1].1[0].0, 90.0, 0.5, "hanging continuation start");
}

#[test]
fn list_label_right_aligned_at_gutter_on_first_baseline() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut props = ParaProps::new();
    props.indent_left = 30.0;
    props.list = Some(ListLabel {
        text: "1.".to_string(),
        gutter: 6.0,
    });
    let (_, result) = export(
        &[Block::Paragraph(
            props,
            vec![Run::new("item text", style(12.0))],
        )],
        geom,
    );
    let ws = words(&result.pdf, 0);
    let label = ws.iter().find(|w| w.4 == "1.").expect("label word");
    let item = ws.iter().find(|w| w.4 == "item").expect("item word");
    assert_near(
        label.2,
        80.0 - 6.0,
        0.5,
        "label right edge = text start - gutter",
    );
    assert_near(item.0, 80.0, 0.5, "text starts at indent_left");
    assert_near(label.3, item.3, 0.5, "label sits on the first baseline");
}

#[test]
fn decorations_materialize_as_ops() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut deco = style(12.0);
    deco.underline = true;
    deco.strike = true;
    deco.highlight = Some(Rgb::new(1.0, 1.0, 0.0));
    deco.color = Rgb::new(0.8, 0.1, 0.1);
    let blocks = vec![Block::Paragraph(
        ParaProps::new(),
        vec![Run::new("marked", deco)],
    )];
    let (pages, _) = export(&blocks, geom);
    let ops = &pages[0].ops;

    let text_at = ops
        .iter()
        .position(|op| matches!(op, Op::Text { .. }))
        .expect("text op");
    let baseline = match &ops[text_at] {
        Op::Text { baseline, .. } => *baseline,
        _ => unreachable!(),
    };
    let hl_at = ops
        .iter()
        .position(
            |op| matches!(op, Op::FillRect { color, .. } if *color == Rgb::new(1.0, 1.0, 0.0)),
        )
        .expect("highlight rect");
    assert!(hl_at < text_at, "highlight paints behind the text");

    let lines: Vec<(f64, f64)> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Line { y1, y2, .. } => Some((*y1, *y2)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2, "underline + strike");
    assert!(
        lines.iter().any(|(y, _)| *y > baseline),
        "underline below the baseline (top-left coords)"
    );
    assert!(
        lines.iter().any(|(y, _)| *y < baseline),
        "strike above the baseline"
    );
    for (y1, y2) in lines {
        assert_near(y1, y2, 1e-9, "decoration lines are horizontal");
    }
}

#[test]
fn empty_paragraph_advances_one_line() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let blocks = vec![
        para("one", 12.0),
        Block::Paragraph(ParaProps::new(), vec![Run::new("", style(12.0))]),
        para("two", 12.0),
    ];
    let (pages, _) = export(&blocks, geom);
    let b = text_baselines(&pages[0].ops);
    assert_eq!(b.len(), 2);
    assert_near(
        b[1] - b[0],
        2.0 * natural_line_height(12.0),
        1e-6,
        "blank paragraph consumes exactly one line box",
    );
}

#[test]
fn page_break_starts_a_new_page() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let blocks = vec![para("first", 12.0), Block::PageBreak, para("second", 12.0)];
    let (pages, result) = export(&blocks, geom);
    assert_eq!(pages.len(), 2);
    let doc = open(&result.pdf);
    assert_eq!(doc.page_count(), 2);
    assert_eq!(words(&result.pdf, 0)[0].4, "first");
    assert_eq!(words(&result.pdf, 1)[0].4, "second");
}

#[test]
fn per_section_page_geometry_via_provider() {
    struct TwoSizes(usize);
    impl pdf_typeset::PageProvider for TwoSizes {
        fn next_page(&mut self) -> PageGeom {
            self.0 += 1;
            if self.0 == 1 {
                PageGeom::new(400.0, 500.0, 50.0)
            } else {
                PageGeom::new(600.0, 300.0, 30.0)
            }
        }
    }
    let mut engine = ts();
    let blocks = vec![para("first", 12.0), Block::PageBreak, para("second", 12.0)];
    let pages = engine.layout_flow(&blocks, &mut TwoSizes(0));
    assert_eq!(pages.len(), 2);
    assert_eq!((pages[0].width, pages[0].height), (400.0, 500.0));
    assert_eq!((pages[1].width, pages[1].height), (600.0, 300.0));
    let result = engine.emit(&pages).expect("emit");
    assert_eq!(words(&result.pdf, 1)[0].0, 30.0, "second-section margin");
}

#[test]
fn glyph_fallback_warned_once_per_family_char() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    // 𝔸 lives in bundled Noto Sans Math, not Liberation; repeated across runs.
    let blocks = vec![para("x 𝔸 y", 12.0), para("𝔸 again 𝔸", 12.0)];
    let (_, result) = export(&blocks, geom);
    let fallbacks: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ExportWarning::GlyphFallback { ch: '𝔸', .. }))
        .collect();
    assert_eq!(fallbacks.len(), 1, "{:?}", result.warnings);
}

#[test]
fn font_substitution_warned_once_per_style() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut unknown = style(12.0);
    unknown.family = "No Such Family 42".to_string();
    let blocks = vec![
        Block::Paragraph(ParaProps::new(), vec![Run::new("one", unknown.clone())]),
        Block::Paragraph(ParaProps::new(), vec![Run::new("two", unknown)]),
    ];
    let (_, result) = export(&blocks, geom);
    let subs: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ExportWarning::FontSubstituted { .. }))
        .collect();
    assert_eq!(subs.len(), 1, "{:?}", result.warnings);
    assert_eq!(tokens(&result.pdf), ["one", "two"]);
}

#[test]
fn exhausted_fallback_degrades_to_notdef_never_panics() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let (_, result) = export(&[para("中", 12.0)], geom); // bundled faces: no CJK
    assert!(result
        .warnings
        .iter()
        .any(|w| matches!(w, ExportWarning::GlyphFallback { ch: '中', .. })));
    assert!(open(&result.pdf).page_count() == 1);
}
