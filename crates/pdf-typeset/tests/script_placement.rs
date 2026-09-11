//! Resolved script glyph placement keeps nominal line frames.
mod common;
use common::*;
use pdf_typeset::{Block, Op, PageGeom, ParaProps, ResolvedScriptPlacement, Run};

fn scripted(text: &str, size: f64, scale: f64, shift: f64) -> Run {
    let mut s = style(size);
    s.script_placement = Some(ResolvedScriptPlacement::new(scale, shift).unwrap());
    Run::new(text, s)
}
#[test]
fn mixed_script_changes_glyph_size_baseline_and_following_advance() {
    let blocks = [Block::Paragraph(
        ParaProps::new(),
        vec![
            Run::new("A", style(12.0)),
            scripted("2", 12.0, 0.6, 4.5),
            Run::new("B", style(12.0)),
        ],
    )];
    let (pages, _) = export(&blocks, PageGeom::new(300.0, 300.0, 30.0));
    let text: Vec<_> = pages[0]
        .ops
        .iter()
        .filter_map(|op| {
            if let Op::Text {
                text,
                x,
                baseline,
                size,
                ..
            } = op
            {
                Some((text.as_str(), *x, *baseline, *size))
            } else {
                None
            }
        })
        .collect();
    let a = text.iter().find(|t| t.0 == "A").expect("separate A");
    let s = text.iter().find(|t| t.0 == "2").expect("separate script");
    let b = text.iter().find(|t| t.0 == "B").expect("separate B");
    assert_near(s.3, 7.2, 1e-9, "glyph scale");
    assert_near(s.2, a.2 - 4.5, 1e-9, "upward shift");
    assert_near(b.2, a.2, 1e-9, "normal baseline");
    let mut engine = ts();
    let width = engine
        .measure_blocks(&[para("2", 7.2)], 200.0, true)
        .max_width;
    assert_near(b.1 - s.1, width, 1e-9, "effective advance");
}

fn texts(ops: &[Op]) -> Vec<(String, f64, f64, f64)> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            Op::Text {
                text,
                x,
                baseline,
                size,
                ..
            } => out.push((text.clone(), *x, *baseline, *size)),
            Op::Group { ops, .. } => out.extend(texts(ops)),
            _ => {}
        }
    }
    out
}
fn block(runs: Vec<Run>) -> Block {
    Block::Paragraph(ParaProps::new(), runs)
}
fn measure(b: &[Block]) -> pdf_typeset::Measurement {
    ts().measure_blocks(b, 200.0, true)
}

#[test]
fn validation_and_identity_preserve_original_bytes() {
    for v in [0.0, -1.0, 1.1, f64::NAN, f64::INFINITY] {
        assert!(ResolvedScriptPlacement::new(v, 0.0).is_err());
    }
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(ResolvedScriptPlacement::new(0.5, v).is_err());
    }
    assert!(style(12.0).script_placement.is_none());
    let g = PageGeom::new(300.0, 300.0, 30.0);
    let (_, plain) = export(&[para("A e\u{301} B\nC", 12.0)], g);
    let (_, identity) = export(
        &[block(vec![scripted("A e\u{301} B\nC", 12.0, 1.0, 0.0)])],
        g,
    );
    assert_eq!(plain.pdf, identity.pdf);
}

#[test]
fn all_script_keeps_nominal_strut_but_reduces_width() {
    use pdf_typeset::LineHeightRule;
    for rule in [LineHeightRule::FontMetrics, LineHeightRule::FontIndependent] {
        let mut engine = ts();
        engine.set_line_height_rule(rule);
        let normal = engine.measure_blocks(&[para("AB", 24.0)], 200.0, true);
        let small =
            engine.measure_blocks(&[block(vec![scripted("AB", 24.0, 0.6, 6.0)])], 200.0, true);
        assert_near(normal.height, small.height, 1e-9, "nominal line");
        assert_near(
            normal.max_width * 0.6,
            small.max_width,
            1e-9,
            "glyph widths",
        );
        let empty =
            engine.measure_blocks(&[block(vec![scripted("", 24.0, 0.6, 6.0)])], 200.0, true);
        assert_near(normal.height, empty.height, 1e-9, "nominal empty reference");
    }
}

#[test]
fn mixed_nominal_sizes_and_shift_envelope_measure_shared_line() {
    use pdf_typeset::LineHeightRule;
    let mut engine = ts();
    engine.set_line_height_rule(LineHeightRule::FontIndependent);
    let m = engine.measure_blocks(
        &[block(vec![
            Run::new("A", style(12.0)),
            scripted("2", 24.0, 0.6, 6.0),
        ])],
        200.0,
        true,
    );
    assert_near(m.height, 28.8, 1e-9, "24pt nominal strut");
    let m = engine.measure_blocks(
        &[block(vec![
            Run::new("A", style(12.0)),
            scripted("2", 12.0, 0.6, -6.0),
        ])],
        200.0,
        true,
    );
    assert_near(m.height, 19.44, 1e-9, "lowered glyph extends descent");
}

#[test]
fn exact_keeps_normals_while_atleast_accounts_for_shift_overflow() {
    use pdf_typeset::LineSpacing;
    let g = PageGeom::new(300.0, 300.0, 30.0);
    for spacing in [LineSpacing::Exact(18.0), LineSpacing::AtLeast(10.0)] {
        let mut p = ParaProps::new();
        p.spacing = spacing;
        let plain = [
            Block::Paragraph(p.clone(), vec![Run::new("A", style(12.0))]),
            para("Z", 12.0),
        ];
        let shifted = [
            Block::Paragraph(
                p,
                vec![Run::new("A", style(12.0)), scripted("2", 12.0, 0.6, 24.0)],
            ),
            para("Z", 12.0),
        ];
        let (a, _) = export(&plain, g);
        let (b, _) = export(&shifted, g);
        let a = texts(&a[0].ops);
        let b = texts(&b[0].ops);
        if matches!(spacing, LineSpacing::Exact(_)) {
            assert_eq!(a[0].2, b[0].2);
            assert_eq!(a[1].2, b[2].2);
        } else {
            assert!(b[0].2 > a[0].2);
            assert!(b[2].2 > a[1].2);
        }
        assert_near(b[1].2, b[0].2 - 24.0, 1e-9, "script offset");
    }
}

#[test]
fn derived_overflow_or_underflow_drops_only_new_script_run() {
    let mut wide = scripted("WIDE", 12.0, 0.6, 1.0);
    wide.style.character_spacing = pdf_typeset::CharacterSpacing::new(f64::MAX).unwrap();
    let b = block(vec![
        Run::new("A", style(12.0)),
        wide,
        scripted("BAD", 12.0, 0.6, f64::MAX),
        scripted("TINY", f64::MIN_POSITIVE, f64::MIN_POSITIVE, 0.0),
        Run::new("Z", style(12.0)),
    ]);
    let (p, _) = export(&[b], PageGeom::new(300.0, 300.0, 30.0));
    assert_eq!(
        texts(&p[0].ops)
            .iter()
            .map(|t| t.0.as_str())
            .collect::<String>(),
        "AZ"
    );
    let m = measure(&[block(vec![scripted("", 12.0, 0.6, f64::MAX)])]);
    assert_eq!(m.height, 0.0);
}

#[test]
fn tracking_stays_in_points_and_tabs_stay_at_stops() {
    use pdf_typeset::CharacterSpacing;
    let mut r = scripted("A\tBC", 12.0, 0.5, 3.0);
    r.style.character_spacing = CharacterSpacing::new(2.0).unwrap();
    let (p, _) = export(&[block(vec![r])], PageGeom::new(300.0, 300.0, 30.0));
    let t = texts(&p[0].ops);
    assert_near(t[1].1, 66.0, 1e-9, "tab position");
    let w = measure(&[para("B", 6.0)]).max_width;
    assert_near(t[2].1 - t[1].1, w + 2.0, 1e-9, "tracking not scaled");
}

#[test]
fn narrow_wrapping_preserves_tracked_combining_and_every_character() {
    use pdf_typeset::CharacterSpacing;
    let mut a = scripted("漢", 12.0, 0.6, 3.0);
    a.style.character_spacing = CharacterSpacing::new(1.0).unwrap();
    let mut b = scripted("\u{301}AB\nC", 12.0, 0.6, 3.0);
    b.style.character_spacing = a.style.character_spacing;
    let (p, _) = export(&[block(vec![a, b])], PageGeom::new(61.0, 300.0, 30.0));
    let t = texts(&p[0].ops);
    assert_eq!(
        t.iter().map(|t| t.0.as_str()).collect::<String>(),
        "漢\u{301}ABC"
    );
    assert!(
        t[0].0.contains('漢') && t[0].0.contains('\u{301}'),
        "tracked base and mark stay in the same text run: {t:?}"
    );
    assert!(t.last().unwrap().2 > t[0].2);
}

#[test]
fn shifted_highlight_decorations_and_link_share_effective_geometry() {
    use pdf_typeset::Rgb;
    let mut r = scripted("AB", 12.0, 0.5, 4.0);
    r.style.highlight = Some(Rgb::BLACK);
    r.style.underline = true;
    r.style.strike = true;
    r.style.link = Some("https://example.test".into());
    let mut normal = r.clone();
    normal.style.size = 6.0;
    normal.style.script_placement = None;
    let (a, _) = export(&[block(vec![normal])], PageGeom::new(300.0, 300.0, 30.0));
    let (b, _) = export(&[block(vec![r])], PageGeom::new(300.0, 300.0, 30.0));
    let delta = texts(&b[0].ops)[0].2 - texts(&a[0].ops)[0].2;
    for (a, b) in a[0].ops.iter().zip(&b[0].ops) {
        match (a, b) {
            (
                Op::FillRect {
                    y: ay,
                    w: aw,
                    h: ah,
                    ..
                },
                Op::FillRect {
                    y: by,
                    w: bw,
                    h: bh,
                    ..
                },
            )
            | (
                Op::Link {
                    y: ay,
                    w: aw,
                    h: ah,
                    ..
                },
                Op::Link {
                    y: by,
                    w: bw,
                    h: bh,
                    ..
                },
            ) => {
                assert_near(by - ay, delta, 1e-9, "shifted rectangle");
                assert_eq!(aw, bw);
                assert_eq!(ah, bh);
            }
            (
                Op::Line {
                    y1: ay, width: aw, ..
                },
                Op::Line {
                    y1: by, width: bw, ..
                },
            ) => {
                assert_near(by - ay, delta, 1e-9, "decoration shift");
                assert_eq!(aw, bw);
            }
            _ => {}
        }
    }
}

#[test]
fn textbox_autoshrink_scales_offset_once_and_preserves_script_scale() {
    use pdf_typeset::{Rect, TextBoxSpec};
    let content = vec![block(vec![
        Run::new("A", style(12.0)),
        scripted("2", 12.0, 0.6, 4.0),
    ])];
    let mut spec = TextBoxSpec::new(
        Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 100.0,
        },
        content,
    );
    spec.font_scale = Some(0.5);
    let mut engine = ts();
    let t = texts(&engine.layout_text_box(&spec));
    assert_near(t[0].3, 6.0, 1e-9, "normal scale");
    assert_near(t[1].3, 3.6, 1e-9, "effective scale once");
    assert_near(t[0].2 - t[1].2, 2.0, 1e-9, "shift scale once");
}

#[test]
fn border_lookahead_uses_script_envelope_for_real_page_closure() {
    use pdf_typeset::{BorderEdge, ParagraphBorder, ParagraphBorders, Rgb};
    let edge = |space| {
        Some(
            ParagraphBorder::new(
                BorderEdge {
                    width: 1.0,
                    color: Rgb::BLACK,
                },
                space,
            )
            .unwrap(),
        )
    };
    let mut a = ParaProps::new();
    a.borders = Some(Box::new(ParagraphBorders {
        top: edge(0.0),
        bottom: edge(30.0),
        left: edge(0.0),
        right: edge(0.0),
    }));
    let mut b = a.clone();
    b.borders.as_mut().unwrap().bottom = edge(0.0);
    let blocks = [
        Block::Paragraph(a, vec![Run::new("A", style(12.0))]),
        Block::Paragraph(b, vec![scripted("B", 12.0, 0.6, -24.0)]),
    ];
    let height = ts().measure_blocks(&blocks, 240.0, true).height;
    let (fit, _) = export(&blocks, PageGeom::new(300.0, height + 60.01, 30.0));
    assert_eq!(fit.len(), 1, "actual joined closure fits exactly");
    let (split, _) = export(&blocks, PageGeom::new(300.0, height + 59.9, 30.0));
    assert_eq!(
        split.len(),
        2,
        "expanded script envelope requires a true split"
    );
    for page in &split {
        assert!(page
            .ops
            .iter()
            .filter_map(|o| if let Op::Line { y1, y2, .. } = o {
                Some(y1.max(*y2))
            } else {
                None
            })
            .all(|y| y <= page.height - 30.0 + 1e-6));
    }
}

#[test]
fn bottom_aligned_box_and_table_reuse_measured_script_height() {
    use pdf_typeset::{Rect, TableCell, TableRow, TableSpec, TextBoxSpec, VAnchor};
    let content = vec![block(vec![scripted("2", 12.0, 0.6, -8.0)])];
    let h = measure(&content).height;
    let mut spec = TextBoxSpec::new(
        Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 100.0,
        },
        content.clone(),
    );
    let mut engine = ts();
    let top = texts(&engine.layout_text_box(&spec));
    spec.v_anchor = VAnchor::Bottom;
    let bottom = texts(&engine.layout_text_box(&spec));
    assert_near(
        bottom[0].2 - top[0].2,
        100.0 - h,
        1e-9,
        "box anchor uses full line height",
    );
    let mut cell = TableCell::new(content);
    cell.padding = 0.0;
    cell.v_align = VAnchor::Bottom;
    let mut row = TableRow::new(vec![cell]);
    row.min_height = Some(100.0);
    let (pages, _) = export(
        &[Block::Table(TableSpec::new(
            vec![pdf_typeset::ColumnWidth::Fixed(200.0)],
            vec![row],
        ))],
        PageGeom::new(300.0, 300.0, 30.0),
    );
    let table = texts(&pages[0].ops);
    assert_near(
        table[0].2,
        bottom[0].2 + 30.0,
        1e-9,
        "table and box agreement",
    );
}

#[test]
fn none_nearly_equal_sizes_keep_legacy_fragment_merge() {
    let g = PageGeom::new(300.0, 300.0, 30.0);
    let (plain, _) = export(&[block(vec![Run::new("AB", style(12.0))])], g);
    let (near, _) = export(
        &[block(vec![
            Run::new("A", style(12.0)),
            Run::new("B", style(12.0 + 1e-10)),
        ])],
        g,
    );
    assert_eq!(
        texts(&plain[0].ops),
        texts(&near[0].ops),
        "legacy EPS merge retains first size and baseline"
    );
}

#[test]
fn invalid_exact_falls_back_to_natural_script_baseline() {
    use pdf_typeset::LineSpacing;
    let g = PageGeom::new(300.0, 300.0, 30.0);
    let runs = vec![Run::new("A", style(12.0)), scripted("2", 12.0, 0.6, -12.0)];
    let (natural, _) = export(&[block(runs.clone())], g);
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut p = ParaProps::new();
        p.spacing = LineSpacing::Exact(invalid);
        let (fallback, _) = export(&[Block::Paragraph(p, runs.clone())], g);
        assert_eq!(texts(&natural[0].ops), texts(&fallback[0].ops));
    }
}

#[test]
fn list_marker_inherits_nominal_size_without_body_script_shift() {
    let mut p = ParaProps::new();
    p.list = Some(pdf_typeset::ListLabel::new("1.", 6.0));
    let (pages, _) = export(
        &[Block::Paragraph(p, vec![scripted("2", 12.0, 0.6, 4.5)])],
        PageGeom::new(300.0, 300.0, 30.0),
    );
    let t = texts(&pages[0].ops);
    let marker = t.iter().find(|t| t.0 == "1.").unwrap();
    let body = t.iter().find(|t| t.0 == "2").unwrap();
    assert_eq!(marker.3, 12.0);
    assert_near(
        marker.2 - body.2,
        4.5,
        1e-9,
        "marker uses shared nominal baseline",
    );
}
