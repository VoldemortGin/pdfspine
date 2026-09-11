//! Explicit signed cluster gaps and typed, text-preserving failure handling.
mod common;
use common::*;
use pdf_typeset::{
    Block, CharacterSpacing, ExportWarning, FixedPages, FontResolver, Op, PageGeom, PageProvider,
    ParaProps, Rect, Run, TextBoxSpec, Typesetter,
};
fn signed(text: &str, gap: f64) -> Run {
    let mut s = style(12.0);
    s.character_spacing = CharacterSpacing::resolved_signed(gap).unwrap();
    Run::new(text, s)
}
fn blocks(gap: f64) -> Vec<Block> {
    vec![Block::Paragraph(
        ParaProps::new(),
        vec![signed("iiiiMMMM", gap)],
    )]
}
fn ts() -> Typesetter {
    Typesetter::new(FontResolver::without_system_fonts())
}
fn text(ops: &[Op]) -> String {
    ops.iter()
        .filter_map(|op| {
            if let Op::Text { text, .. } = op {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect()
}
#[test]
fn constructors_preserve_old_values() {
    assert!(CharacterSpacing::new(-1.0).is_err());
    for x in [0.0, 1.0, 3.0] {
        assert_eq!(
            CharacterSpacing::new(x),
            CharacterSpacing::resolved_signed(x)
        );
    }
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(CharacterSpacing::resolved_signed(x).is_err());
    }
}
#[test]
fn moderate_condensation_keeps_text_and_reduces_width() {
    let mut t = ts();
    let plain = t.measure_blocks(&blocks(0.0), 200.0, false);
    let condensed = t.try_measure_blocks(&blocks(-1.0), 200.0, false).unwrap();
    assert!((plain.max_width - condensed.max_width - 7.0).abs() < 1e-6);
    let pages = t
        .try_layout_flow(
            &blocks(-1.0),
            &mut FixedPages::new(PageGeom::new(300.0, 300.0, 20.0)),
        )
        .unwrap();
    assert_eq!(text(&pages[0].ops), "iiiiMMMM");
}
struct CountingPages(usize);
impl PageProvider for CountingPages {
    fn next_page(&mut self) -> PageGeom {
        self.0 += 1;
        PageGeom::new(300.0, 300.0, 20.0)
    }
}
#[test]
fn checked_failure_precedes_provider_and_all_four_facades_report_it() {
    let mut t = ts();
    let b = blocks(-100.0);
    let mut p = CountingPages(0);
    let err = t.try_layout_flow(&b, &mut p).unwrap_err();
    assert_eq!(p.0, 0);
    assert_eq!(err.run_index, 0);
    assert_eq!(err.byte_offset, 0);
    assert!(t.try_measure_blocks(&b, 200.0, true).is_err());
    let spec = TextBoxSpec::new(Rect::new(0.0, 0.0, 200.0, 200.0), b);
    assert!(t.try_layout_text_box(&spec).is_err());
    assert!(t.try_measure_text_box(&spec).is_err());
}
#[test]
fn legacy_reports_every_bad_paragraph_and_preserves_input_and_text() {
    let mut t = ts();
    let b = vec![
        Block::Paragraph(ParaProps::new(), vec![signed("first", -100.0)]),
        Block::Paragraph(ParaProps::new(), vec![signed("second", -200.0)]),
    ];
    let pages = t.layout_flow(&b, &mut FixedPages::new(PageGeom::new(300.0, 300.0, 20.0)));
    assert_eq!(text(&pages[0].ops), "firstsecond");
    assert_eq!(
        t.warnings()
            .iter()
            .filter(|w| matches!(w, ExportWarning::SignedSpacingFallback { .. }))
            .count(),
        2
    );
    if let Block::Paragraph(_, runs) = &b[0] {
        assert_eq!(runs[0].style.character_spacing.points(), -100.0);
    }
}

#[test]
fn condensed_pen_does_not_shrink_the_advance_cell_envelope() {
    let mut t = ts();
    let mut large = signed("M", -2.0);
    large.style.highlight = Some(pdf_typeset::Rgb::new(1.0, 1.0, 0.0));
    large.style.underline = true;
    large.style.link = Some("https://example.test".into());
    let small = Run::new("i", style(0.1));
    let b = vec![Block::Paragraph(ParaProps::new(), vec![large, small])];
    let nominal = t
        .measure_blocks(
            &[Block::Paragraph(
                ParaProps::new(),
                vec![Run::new("M", style(12.0))],
            )],
            200.0,
            false,
        )
        .max_width;
    let measured = t.try_measure_blocks(&b, 200.0, false).unwrap();
    assert!(measured.max_width >= nominal - 1e-6);
    let ops = t
        .try_layout_text_box(&TextBoxSpec::new(Rect::new(0.0, 0.0, 200.0, 200.0), b))
        .unwrap();
    assert!(ops
        .iter()
        .any(|op| matches!(op,Op::FillRect{w,..} if *w>=nominal-1e-6)));
    assert!(ops
        .iter()
        .any(|op| matches!(op,Op::Link{w,..} if *w>=nominal-1e-6)));
}

#[test]
fn split_runs_combining_and_cjk_retain_every_source_scalar() {
    let mut t = ts();
    let b = vec![Block::Paragraph(
        ParaProps::new(),
        vec![signed("中", -1.0), signed("\u{301}B", -0.5)],
    )];
    let ops = t
        .try_layout_text_box(&TextBoxSpec::new(Rect::new(0.0, 0.0, 14.0, 100.0), b))
        .unwrap();
    assert_eq!(text(&ops), "中\u{301}B");
    let whole = blocks(-1.0);
    let split = vec![Block::Paragraph(
        ParaProps::new(),
        vec![signed("iiii", -1.0), signed("MMMM", -1.0)],
    )];
    let a = t.try_measure_blocks(&whole, 200.0, false).unwrap();
    let b = t.try_measure_blocks(&split, 200.0, false).unwrap();
    assert!((a.max_width - b.max_width).abs() < 1e-9);
}
#[test]
fn nested_bad_paragraph_has_a_typed_path_and_single_fallback() {
    use pdf_typeset::{ColumnWidth, SpacingPathStep, TableCell, TableRow, TableSpec};
    let table = TableSpec::new(
        vec![ColumnWidth::Fixed(100.0)],
        vec![TableRow::new(vec![TableCell::new(blocks(-100.0))])],
    );
    let mut t = ts();
    let b = vec![Block::Table(table)];
    let e = t.try_measure_blocks(&b, 150.0, true).unwrap_err();
    assert_eq!(
        e.path,
        vec![
            SpacingPathStep::Block(0),
            SpacingPathStep::Row(0),
            SpacingPathStep::Cell(0),
            SpacingPathStep::Block(0)
        ]
    );
    t.measure_blocks(&b, 150.0, true);
    assert_eq!(
        t.warnings()
            .iter()
            .filter(|w| matches!(w, ExportWarning::SignedSpacingFallback { .. }))
            .count(),
        1
    );
}
#[test]
fn autoshrink_underflow_is_reported_before_returning_ops() {
    let mut spec = TextBoxSpec::new(Rect::new(0.0, 0.0, 100.0, 100.0), blocks(-1.0));
    spec.font_scale = Some(f64::from_bits(1));
    assert!(ts().try_layout_text_box(&spec).is_err());
}
#[test]
fn new_nonnegative_constructor_keeps_pdf_bytes() {
    for gap in [0.0, 1.5] {
        let make = |resolved: bool| {
            let mut r = Run::new("old positive path", style(12.0));
            r.style.character_spacing = if resolved {
                CharacterSpacing::resolved_signed(gap).unwrap()
            } else {
                CharacterSpacing::new(gap).unwrap()
            };
            let mut t = ts();
            let p = t.layout_flow(
                &[Block::Paragraph(ParaProps::new(), vec![r])],
                &mut FixedPages::new(PageGeom::new(300.0, 300.0, 20.0)),
            );
            t.emit(&p).unwrap().pdf
        };
        assert_eq!(make(false), make(true));
    }
}

#[test]
fn soft_wrap_uses_the_final_advance_cell_edge() {
    let mut t = ts();
    let one = t
        .measure_blocks(
            &[Block::Paragraph(
                ParaProps::new(),
                vec![Run::new("M", style(12.0))],
            )],
            100.0,
            false,
        )
        .max_width;
    let b = vec![Block::Paragraph(ParaProps::new(), vec![signed("MM", -1.0)])];
    let fit = t
        .try_measure_blocks(&b, 2.0 * one - 1.0 + 1e-5, true)
        .unwrap();
    let split = t
        .try_measure_blocks(&b, 2.0 * one - 1.0 - 1e-5, true)
        .unwrap();
    assert_eq!(fit.lines.len(), 1);
    assert_eq!(split.lines.len(), 2);
    assert!((split.max_width - one).abs() < 1e-6);
}
#[test]
fn script_tracking_is_independent_and_box_scaling_happens_once() {
    let mut r = signed("MM", -1.0);
    r.style.script_placement = Some(pdf_typeset::ResolvedScriptPlacement::new(0.5, 3.0).unwrap());
    let b = vec![Block::Paragraph(ParaProps::new(), vec![r])];
    let mut t = ts();
    let base = t.try_measure_blocks(&b, 200.0, false).unwrap().max_width;
    let mut spec = TextBoxSpec::new(Rect::new(0.0, 0.0, 200.0, 200.0), b);
    spec.font_scale = Some(0.5);
    let ops = t.try_layout_text_box(&spec).unwrap();
    let positions: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Text { x, size, text, .. } => Some((*x, *size, text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(positions.len(), 2);
    assert!((positions[0].1 - 3.0).abs() < 1e-9);
    let one = base / 2.0 + 0.5;
    assert!((positions[1].0 - positions[0].0 - (one - 1.0) * 0.5).abs() < 1e-6);
}
#[test]
fn explicit_break_and_spaces_preserve_text_without_nonfinite_ops() {
    let mut t = ts();
    let b = vec![Block::Paragraph(
        ParaProps::new(),
        vec![signed("M M\nM\tM", -1.0)],
    )];
    let ops = t
        .try_layout_text_box(&TextBoxSpec::new(Rect::new(0.0, 0.0, 200.0, 200.0), b))
        .unwrap();
    assert_eq!(text(&ops).replace(' ', ""), "MMMM");
    assert!(ops.iter().all(|op| match op {
        Op::Text { x, baseline, .. } => x.is_finite() && baseline.is_finite(),
        _ => true,
    }));
}

#[test]
fn paragraph_final_gap_is_not_consumed_or_checked_as_a_move() {
    let mut t = ts();
    let b = vec![Block::Paragraph(
        ParaProps::new(),
        vec![signed("M", -100.0)],
    )];
    let actual = t.try_measure_blocks(&b, 100.0, false).unwrap();
    let normal = t.measure_blocks(
        &[Block::Paragraph(
            ParaProps::new(),
            vec![Run::new("M", style(12.0))],
        )],
        100.0,
        false,
    );
    assert!((actual.max_width - normal.max_width).abs() < 1e-9);
}
