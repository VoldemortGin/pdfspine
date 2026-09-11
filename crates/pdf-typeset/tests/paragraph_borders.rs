mod common;
use common::*;
use pdf_typeset::{
    Block, BorderEdge, LineSpacing, Op, PageGeom, ParaProps, ParagraphBorder, ParagraphBorders,
    Rgb, Run,
};

fn edge(width: f64, space: f64) -> ParagraphBorder {
    ParagraphBorder::new(
        BorderEdge {
            width,
            color: Rgb::BLACK,
        },
        space,
    )
    .unwrap()
}
fn paragraph(text: &str, width: f64, space: f64) -> Block {
    let mut p = ParaProps::new();
    p.spacing = LineSpacing::Exact(20.0);
    if width > 0.0 {
        let e = Some(edge(width, space));
        p.borders = Some(Box::new(ParagraphBorders {
            top: e,
            right: e,
            bottom: e,
            left: e,
        }));
    }
    Block::Paragraph(p, vec![Run::new(text, style(12.0))])
}
fn baseline(ops: &[Op]) -> Vec<f64> {
    ops.iter()
        .filter_map(|o| {
            if let Op::Text { baseline, .. } = o {
                Some(*baseline)
            } else {
                None
            }
        })
        .collect()
}
#[test]
fn dimensions_are_validated_without_pdf_hairlines() {
    for width in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(ParagraphBorder::new(
            BorderEdge {
                width,
                color: Rgb::BLACK
            },
            0.0
        )
        .is_err());
    }
    for space in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(ParagraphBorder::new(
            BorderEdge {
                width: 1.0,
                color: Rgb::BLACK
            },
            space
        )
        .is_err());
    }
    assert_eq!(ParaProps::new().borders, None);
}
#[test]
fn single_border_reserves_height_and_emits_solid_edges() {
    let g = PageGeom::new(300.0, 300.0, 50.0);
    let (plain, _) = export(&[paragraph("alpha", 0.0, 0.0)], g);
    let (bordered, _) = export(&[paragraph("alpha", 1.0, 4.0)], g);
    assert_eq!(
        baseline(&bordered[0].ops)[0] - baseline(&plain[0].ops)[0],
        5.0
    );
    assert_eq!(
        bordered[0]
            .ops
            .iter()
            .filter(|o| matches!(o, Op::Line { .. }))
            .count(),
        4
    );
}

fn props(block: &mut Block) -> &mut ParaProps {
    match block {
        Block::Paragraph(p, _) => p,
        _ => unreachable!(),
    }
}
fn horizontals(ops: &[Op]) -> Vec<(f64, f64, f64)> {
    ops.iter()
        .filter_map(|o| match o {
            Op::Line { x1, y1, x2, y2, .. } if y1 == y2 => Some((*x1, *y1, *x2)),
            _ => None,
        })
        .collect()
}
#[test]
fn joined_paragraphs_keep_spacing_but_ignore_internal_border_space() {
    let mut a = paragraph("alpha", 1.0, 0.0);
    props(&mut a).space_after = 12.0;
    let mut b = paragraph("beta", 1.0, 2.0);
    props(&mut b).space_before = 9.0;
    let (pages, _) = export(&[a, b], PageGeom::new(300.0, 300.0, 50.0));
    let y = baseline(&pages[0].ops);
    assert_eq!(y[1] - y[0], 41.0);
    let h = horizontals(&pages[0].ops);
    assert_eq!(h.len(), 2);
    assert_eq!(h[0], (49.0, 50.5, 251.0));
    assert_eq!(h[1], (47.0, 114.5, 253.0));
}
#[test]
fn hanging_or_changed_strokes_break_groups_positive_first_indent_does_not() {
    for (indent, expected_edges, pitch) in [(18.0, 2, 20.0), (-18.0, 4, 22.0)] {
        let a = paragraph("alpha", 1.0, 0.0);
        let mut b = paragraph("beta", 1.0, 0.0);
        props(&mut b).first_line_indent = indent;
        let (pages, _) = export(&[a, b], PageGeom::new(300.0, 300.0, 50.0));
        assert_eq!(horizontals(&pages[0].ops).len(), expected_edges);
        let y = baseline(&pages[0].ops);
        assert_eq!(y[1] - y[0], pitch);
    }
    let mut b = paragraph("beta", 1.0, 0.0);
    props(&mut b).borders.as_mut().unwrap().top = Some(
        ParagraphBorder::new(
            BorderEdge {
                width: 1.0,
                color: Rgb::new(0.0, 0.0, 1.0),
            },
            0.0,
        )
        .unwrap(),
    );
    let (pages, _) = export(
        &[paragraph("alpha", 1.0, 0.0), b],
        PageGeom::new(300.0, 300.0, 50.0),
    );
    assert_eq!(horizontals(&pages[0].ops).len(), 4);
}
#[test]
fn fragments_reserve_bottom_repeat_top_and_never_loop_on_oversized_lines() {
    let g = PageGeom::new(300.0, 100.0, 20.0);
    let (plain, _) = export(&[paragraph("a\nb\nc", 0.0, 0.0)], g);
    assert_eq!(plain.len(), 1);
    let (pages, pdf) = export(&[paragraph("a\nb\nc", 3.0, 0.0)], g);
    assert_eq!(pages.len(), 2);
    assert_eq!(
        full_text(&pdf.pdf).split_whitespace().collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    for p in &pages {
        let h = horizontals(&p.ops);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].1, 21.5);
        assert!(h[1].1 + 1.5 <= 80.0);
    }
    let (huge, _) = export(&[paragraph("oversized", 40.0, 0.0)], g);
    assert_eq!(huge.len(), 1);
    assert_eq!(horizontals(&huge[0].ops).len(), 2);
    let (explicit, _) = export(
        &[
            paragraph("a", 1.0, 0.0),
            Block::PageBreak,
            paragraph("b", 1.0, 0.0),
        ],
        g,
    );
    assert_eq!(explicit.len(), 2);
    assert!(explicit.iter().all(|p| horizontals(&p.ops).len() == 2));
}
#[test]
fn empty_styled_paragraph_is_bordered_and_measure_includes_outer_extents() {
    let blocks = [paragraph("", 1.0, 2.0), paragraph("alpha", 1.0, 2.0)];
    let mut engine = ts();
    let measurement = engine.measure_blocks(&blocks, 200.0, true);
    assert_eq!(measurement.height, 46.0);
    let (pages, _) = export(&blocks, PageGeom::new(300.0, 300.0, 50.0));
    assert_eq!(horizontals(&pages[0].ops).len(), 2);
}
#[test]
fn textbox_anchor_and_table_cell_use_the_same_bordered_height() {
    use pdf_typeset::{ColumnWidth, Rect, TableCell, TableRow, TableSpec, TextBoxSpec, VAnchor};
    let block = paragraph("alpha", 1.0, 2.0);
    let mut engine = ts();
    let mut spec = TextBoxSpec::new(
        Rect {
            x0: 50.0,
            y0: 50.0,
            x1: 250.0,
            y1: 150.0,
        },
        vec![block.clone()],
    );
    spec.v_anchor = VAnchor::Bottom;
    let ops = engine.layout_text_box(&spec);
    let h = horizontals(&ops);
    assert_eq!(h.len(), 2);
    assert_eq!(h[1].1, 149.5);
    let mut cell = TableCell::new(vec![block]);
    cell.padding = 0.0;
    let (pages, _) = export(
        &[Block::Table(TableSpec::new(
            vec![ColumnWidth::Fixed(200.0)],
            vec![TableRow::new(vec![cell])],
        ))],
        PageGeom::new(300.0, 300.0, 50.0),
    );
    assert_eq!(
        horizontals(&pages[0].ops),
        [(47.0, 50.5, 253.0), (47.0, 75.5, 253.0)]
    );
}

#[test]
fn shading_tracking_decorations_and_links_share_bordered_text_coordinates() {
    use pdf_typeset::CharacterSpacing;
    let mut block = paragraph("tracked linked text", 2.0, 1.0);
    if let Block::Paragraph(p, runs) = &mut block {
        p.shading = Some(Rgb::new(0.9, 0.8, 0.7));
        runs[0].style.character_spacing = CharacterSpacing::new(1.0).unwrap();
        runs[0].style.underline = true;
        runs[0].style.link = Some("https://example.com/".into());
    }
    let mut plain = block.clone();
    props(&mut plain).borders = None;
    let (a, _) = export(&[plain], PageGeom::new(300.0, 300.0, 50.0));
    let (b, pdf) = export(&[block], PageGeom::new(300.0, 300.0, 50.0));
    assert_eq!(full_text(&pdf.pdf).trim(), "tracked linked text");
    let text_a = baseline(&a[0].ops);
    let text_b = baseline(&b[0].ops);
    assert!(text_a.iter().zip(text_b).all(|(a, b)| b - a == 3.0));
    let links = |ops: &[Op]| {
        ops.iter()
            .filter_map(|o| match o {
                Op::Link { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let la = links(&a[0].ops);
    let lb = links(&b[0].ops);
    assert_eq!(la.len(), lb.len());
    for (a, b) in la.iter().zip(lb) {
        assert_eq!((a.0, a.1 + 3.0, a.2, a.3), b);
    }
    let first_text = b[0]
        .ops
        .iter()
        .position(|o| matches!(o, Op::Text { .. }))
        .unwrap();
    assert!(b[0].ops[..first_text]
        .iter()
        .any(|o| matches!(o, Op::FillRect { .. })));
    assert!(matches!(b[0].ops.last(), Some(Op::Line { width: 2.0, .. })));
}
#[test]
fn autofit_keeps_fixed_border_extents_while_shrinking_text() {
    use pdf_typeset::{Rect, TextBoxSpec};
    let mut block = paragraph("large\ntext", 2.0, 1.0);
    if let Block::Paragraph(p, runs) = &mut block {
        p.spacing = LineSpacing::default();
        runs[0].style.size = 30.0;
    }
    let mut spec = TextBoxSpec::new(
        Rect {
            x0: 40.0,
            y0: 50.0,
            x1: 240.0,
            y1: 85.0,
        },
        vec![block],
    );
    spec.font_scale = Some(1.0);
    let mut engine = ts();
    let ops = engine.layout_text_box(&spec);
    let text_sizes: Vec<_> = ops
        .iter()
        .filter_map(|o| match o {
            Op::Text { size, .. } => Some(*size),
            _ => None,
        })
        .collect();
    assert!(!text_sizes.is_empty());
    assert!(text_sizes.iter().all(|s| *s < 30.0));
    assert!(horizontals(&ops)
        .iter()
        .all(|(_, y, _)| *y >= 50.0 && *y <= 85.0));
    assert!(ops
        .iter()
        .filter_map(|o| match o {
            Op::Line { width, .. } => Some(*width),
            _ => None,
        })
        .all(|w| w == 2.0));
}

#[test]
fn asymmetric_edges_and_bottom_aligned_cell_reserve_only_present_edges() {
    use pdf_typeset::{ColumnWidth, TableCell, TableRow, TableSpec, VAnchor};
    let mut block = paragraph("alpha", 0.0, 0.0);
    props(&mut block).borders = Some(Box::new(ParagraphBorders {
        top: None,
        left: Some(edge(2.0, 3.0)),
        bottom: Some(edge(3.0, 4.0)),
        right: Some(edge(1.0, 5.0)),
    }));
    let mut engine = ts();
    assert_eq!(
        engine.measure_blocks(&[block.clone()], 200.0, true).height,
        27.0
    );
    let mut cell = TableCell::new(vec![block]);
    cell.v_align = VAnchor::Bottom;
    let mut row = TableRow::new(vec![cell]);
    row.min_height = Some(100.0);
    let (pages, _) = export(
        &[Block::Table(TableSpec::new(
            vec![ColumnWidth::Fixed(200.0)],
            vec![row],
        ))],
        PageGeom::new(300.0, 300.0, 50.0),
    );
    assert_eq!(horizontals(&pages[0].ops), [(45.0, 148.5, 256.0)]);
    let vertical: Vec<_> = pages[0]
        .ops
        .iter()
        .filter_map(|o| match o {
            Op::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                ..
            } if x1 == x2 => Some((*x1, *y1, *y2, *width)),
            _ => None,
        })
        .collect();
    assert_eq!(
        vertical,
        [(46.0, 123.0, 150.0, 2.0), (255.5, 123.0, 150.0, 1.0)]
    );
}

#[test]
fn suppressed_internal_bottom_space_does_not_force_an_early_page() {
    let mut a = paragraph("alpha", 1.0, 0.0);
    props(&mut a).borders.as_mut().unwrap().bottom = Some(edge(1.0, 49.0));
    let b = paragraph("beta", 1.0, 0.0);
    let mut engine = ts();
    assert_eq!(
        engine
            .measure_blocks(&[a.clone(), b.clone()], 260.0, true)
            .height,
        42.0
    );
    let (pages, pdf) = export(
        &[paragraph("before1\nbefore2", 0.0, 0.0), a, b],
        PageGeom::new(300.0, 140.0, 20.0),
    );
    assert_eq!(
        pages.len(),
        1,
        "42pt bordered group fits after 40pt plain text in a 100pt content area"
    );
    assert_eq!(
        full_text(&pdf.pdf).split_whitespace().collect::<Vec<_>>(),
        ["before1", "before2", "alpha", "beta"]
    );
}

#[test]
fn lookahead_still_moves_groups_without_a_legal_closing_edge() {
    let mut a = paragraph("alpha", 1.0, 0.0);
    props(&mut a).borders.as_mut().unwrap().bottom = Some(edge(1.0, 49.0));
    let mut b = paragraph("beta", 1.0, 0.0);
    props(&mut b).spacing = LineSpacing::Exact(40.0);
    let (pages, _) = export(
        &[paragraph("before1\nbefore2", 0.0, 0.0), a, b],
        PageGeom::new(300.0, 140.0, 20.0),
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(baseline(&pages[0].ops).len(), 2);
    for p in pages {
        assert!(horizontals(&p.ops).iter().all(|(_, y, _)| *y <= 120.0));
    }
}
#[test]
fn variable_width_pages_replan_future_border_paragraphs() {
    use pdf_typeset::PageProvider;
    struct Pages(usize);
    impl PageProvider for Pages {
        fn next_page(&mut self) -> PageGeom {
            self.0 += 1;
            PageGeom::new(if self.0 == 1 { 300.0 } else { 140.0 }, 140.0, 20.0)
        }
    }
    let mut a = paragraph("alpha\nalpha\nalpha", 1.0, 0.0);
    props(&mut a).borders.as_mut().unwrap().bottom = Some(edge(1.0, 24.0));
    let b = paragraph(
        "beta beta beta beta beta beta beta beta beta beta beta beta",
        1.0,
        0.0,
    );
    let blocks = [paragraph("before1\nbefore2", 0.0, 0.0), a, b];
    let mut engine = ts();
    let pages = engine.layout_flow(&blocks, &mut Pages(0));
    assert!(pages.len() > 1);
    for p in &pages {
        for (x1, y, x2) in horizontals(&p.ops) {
            assert_eq!(x1, 19.0);
            assert_eq!(x2, p.width - 19.0);
            assert!(y >= 20.0 && y <= p.height - 20.0);
        }
    }
    let pdf = engine.emit(&pages).unwrap();
    assert_eq!(full_text(&pdf.pdf).split_whitespace().count(), 17);
}

#[test]
fn real_split_restores_each_fragments_unsuppressed_edge_space() {
    let mut a = paragraph("alpha", 1.0, 0.0);
    props(&mut a).borders.as_mut().unwrap().bottom = Some(edge(1.0, 49.0));
    let mut b = paragraph("beta", 1.0, 0.0);
    props(&mut b).spacing = LineSpacing::Exact(60.0);
    let (pages, pdf) = export(
        &[paragraph("before", 0.0, 0.0), a, b],
        PageGeom::new(300.0, 140.0, 20.0),
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(
        horizontals(&pages[0].ops),
        [(19.0, 40.5, 281.0), (19.0, 110.5, 281.0)]
    );
    assert_eq!(
        horizontals(&pages[1].ops),
        [(19.0, 20.5, 281.0), (19.0, 81.5, 281.0)]
    );
    assert!(baseline(&pages[0].ops).iter().all(|y| *y < 110.0));
    assert!(baseline(&pages[1].ops).iter().all(|y| *y < 81.0));
    assert_eq!(
        full_text(&pdf.pdf).split_whitespace().collect::<Vec<_>>(),
        ["before", "alpha", "beta"]
    );
}
