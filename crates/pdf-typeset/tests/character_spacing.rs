//! Nonnegative run tracking: measured advances and ordinary-layout compatibility.
mod common;
use common::*;
use pdf_typeset::{
    Align, Block, CharacterSpacing, CharacterSpacingError, Op, PageGeom, ParaProps, Run,
};

fn run(text: &str, spacing: f64) -> Run {
    let mut s = style(12.0);
    s.character_spacing = CharacterSpacing::new(spacing).unwrap();
    Run::new(text, s)
}
fn positions(runs: Vec<Run>, align: Align) -> Vec<(String, f64, f64)> {
    let mut p = ParaProps::new();
    p.align = align;
    let (pages, _) = export(
        &[Block::Paragraph(p, runs)],
        PageGeom::new(300.0, 300.0, 50.0),
    );
    pages[0]
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Text {
                text, x, baseline, ..
            } => Some((text.clone(), *x, *baseline)),
            _ => None,
        })
        .collect()
}
#[test]
fn rejects_unsupported_values_and_defaults_to_zero() {
    assert_eq!(
        CharacterSpacing::new(-1.0),
        Err(CharacterSpacingError::Negative)
    );
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            CharacterSpacing::new(v),
            Err(CharacterSpacingError::NonFinite)
        );
    }
    assert_eq!(style(12.0).character_spacing.points(), 0.0);
}
#[test]
fn positive_tracking_moves_glyphs_and_crosses_run_boundaries() {
    let zero = positions(vec![run("AB", 0.0), run("CD", 0.0)], Align::Left);
    let spaced = positions(vec![run("AB", 1.0), run("CD", 0.0)], Align::Left);
    assert_eq!(
        spaced.iter().map(|x| x.0.as_str()).collect::<String>(),
        "ABCD"
    );
    assert_ne!(zero, spaced);
    let first = spaced
        .iter()
        .find(|x| x.0.starts_with('C'))
        .expect("new run begins at C");
    let mut engine = ts();
    let natural = engine
        .measure_blocks(
            &[Block::Paragraph(ParaProps::new(), vec![run("AB", 0.0)])],
            200.0,
            true,
        )
        .max_width;
    assert_near(first.1, 50.0 + natural + 2.0, 1e-6, "run boundary advance");
}
#[test]
fn paragraph_end_and_explicit_break_have_different_final_tracking() {
    let normal = positions(vec![run("AB", 0.0)], Align::Right);
    let end = positions(vec![run("AB", 1.0)], Align::Right);
    let hard = positions(vec![run("AB\nCD", 1.0)], Align::Right);
    assert_near(end[0].1, normal[0].1 - 1.0, 1e-6, "paragraph end");
    assert_near(hard[0].1, normal[0].1 - 2.0, 1e-6, "explicit break");
}

#[test]
fn unicode_spacing_counts_sequences_not_utf8_bytes_or_combining_marks() {
    let actual = positions(vec![run("Ae\u{301}B", 1.0)], Align::Left);
    let split = positions(vec![run("Ae", 1.0), run("\u{301}B", 1.0)], Align::Left);
    assert_eq!(actual, split, "combining sequence spans run boundaries");
    assert_eq!(
        actual.iter().map(|x| x.0.as_str()).collect::<String>(),
        "Ae\u{301}B"
    );
    let base = positions(
        vec![run("A", 0.0), run("é", 0.0), run("B", 0.0)],
        Align::Left,
    );
    let spaced = positions(vec![run("AéB", 1.0)], Align::Left);
    let mut engine = ts();
    let width = engine
        .measure_blocks(
            &[Block::Paragraph(ParaProps::new(), vec![run("Aé", 0.0)])],
            200.0,
            true,
        )
        .max_width;
    let b = spaced.iter().find(|p| p.0 == "B").unwrap();
    assert_near(
        b.1,
        50.0 + width + 2.0,
        1e-6,
        "multibyte character counts once",
    );
    assert_eq!(base.iter().map(|x| x.0.as_str()).collect::<String>(), "AéB");
}

#[test]
fn forced_word_splitting_keeps_a_combining_sequence_on_one_line() {
    let p = ParaProps::new();
    let blocks = [Block::Paragraph(
        p,
        vec![run("AAAAe", 1.0), run("\u{301}BBBB", 1.0)],
    )];
    let (pages, pdf) = export(&blocks, PageGeom::new(125.0, 300.0, 50.0));
    let ops = &pages[0].ops;
    assert_eq!(
        full_text(&pdf.pdf)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>(),
        "AAAAe\u{301}BBBB"
    );
    let mut prior_baseline = None;
    for op in ops {
        if let Op::Text { text, baseline, .. } = op {
            if text.starts_with('\u{301}') {
                assert_eq!(Some(*baseline), prior_baseline);
            }
            prior_baseline = Some(*baseline);
        }
    }
}

#[test]
fn tracked_spaces_change_soft_wrapping_at_the_measured_width() {
    let mut r = run("AB CD EF GH", 1.0);
    r.style.family = "Liberation Serif".into();
    let (pages, pdf) = export(
        &[Block::Paragraph(ParaProps::new(), vec![r])],
        PageGeom::new(140.0, 300.0, 50.0),
    );
    let mut lines: Vec<(f64, String)> = Vec::new();
    for op in &pages[0].ops {
        if let Op::Text { text, baseline, .. } = op {
            if let Some((y, s)) = lines.last_mut() {
                if (*y - *baseline).abs() < 1e-6 {
                    s.push_str(text);
                    continue;
                }
            }
            lines.push((*baseline, text.clone()));
        }
    }
    assert_eq!(
        lines.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(),
        ["AB", "CD EF", "GH"]
    );
    assert_eq!(tokens(&pdf.pdf), ["AB", "CD", "EF", "GH"]);
}

#[test]
fn decorations_and_links_include_tracking_but_tabs_keep_their_stops() {
    let mut r = run("AB", 1.0);
    r.style.underline = true;
    r.style.highlight = Some(pdf_typeset::Rgb::BLACK);
    r.style.link = Some("https://example.test".into());
    let (pages, _) = export(
        &[Block::Paragraph(ParaProps::new(), vec![r])],
        PageGeom::new(300.0, 300.0, 50.0),
    );
    let mut engine = ts();
    let width = engine
        .measure_blocks(
            &[Block::Paragraph(ParaProps::new(), vec![run("AB", 0.0)])],
            200.0,
            true,
        )
        .max_width
        + 1.0;
    let link = pages[0]
        .ops
        .iter()
        .find_map(|op| match op {
            Op::Link { x, w, .. } => Some((*x, *w)),
            _ => None,
        })
        .unwrap();
    assert_near(link.1, width, 1e-6, "link spans tracked text");
    let end = pages[0]
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Line { x2, .. } => Some(*x2),
            _ => None,
        })
        .fold(0.0, f64::max);
    assert_near(end, link.0 + width, 1e-6, "underline ends with text");
    let p = positions(vec![run("A\tB", 1.0)], Align::Left);
    assert_near(
        p.iter().find(|p| p.0 == "B").unwrap().1,
        86.0,
        1e-6,
        "tab stop",
    );
}

#[test]
fn textbox_font_scaling_also_scales_tracking() {
    use pdf_typeset::{Rect, TextBoxSpec};
    let mut box_spec = TextBoxSpec::new(
        Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 100.0,
        },
        vec![Block::Paragraph(ParaProps::new(), vec![run("AB", 2.0)])],
    );
    box_spec.font_scale = Some(0.5);
    let mut engine = ts();
    let ops = engine.layout_text_box(&box_spec);
    fn text(ops: &[Op], out: &mut Vec<(String, f64, f64)>) {
        for op in ops {
            match op {
                Op::Text {
                    text: t, x, size, ..
                } => out.push((t.clone(), *x, *size)),
                Op::Group { ops, .. } => text(ops, out),
                _ => {}
            }
        }
    }
    let mut parts = Vec::new();
    text(&ops, &mut parts);
    let full = positions(vec![run("AB", 2.0)], Align::Left);
    assert_eq!(parts[0].2, 6.0);
    assert_near(
        parts[1].1 - parts[0].1,
        (full[1].1 - full[0].1) * 0.5,
        1e-6,
        "scaled gap",
    );
}

#[test]
fn zero_tracking_preserves_legacy_overwide_combining_break() {
    let (pages, _) = export(
        &[Block::Paragraph(
            ParaProps::new(),
            vec![run("e\u{301}", 0.0)],
        )],
        PageGeom::new(101.0, 300.0, 50.0),
    );
    let parts: Vec<_> = pages[0]
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Text { text, baseline, .. } => Some((text.as_str(), *baseline)),
            _ => None,
        })
        .collect();
    assert_eq!(parts.iter().map(|x| x.0).collect::<String>(), "e\u{301}");
    assert!(
        parts[1].1 > parts[0].1,
        "zero tracking retains the pre-tracking line breaker"
    );
}

#[test]
fn tracked_cjk_combining_sequence_survives_token_and_line_boundaries() {
    for width in [1.0, 17.0] {
        let (pages, _) = export(
            &[Block::Paragraph(
                ParaProps::new(),
                vec![run("中\u{301}B", 1.0)],
            )],
            PageGeom::new(100.0 + width, 300.0, 50.0),
        );
        let parts: Vec<_> = pages[0]
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { text, baseline, .. } => Some((text.as_str(), *baseline)),
                _ => None,
            })
            .collect();
        assert_eq!(parts.iter().map(|x| x.0).collect::<String>(), "中\u{301}B");
        let base = parts.iter().find(|x| x.0.contains('中')).unwrap().1;
        let mark = parts.iter().find(|x| x.0.contains('\u{301}')).unwrap().1;
        assert_near(mark, base, 1e-6, "mark stays with overwide CJK base");
    }
}
