mod common;
use common::*;
use pdf_typeset::{
    Block, BorderEdge, LineSpacing, Op, PageGeom, ParaProps, ParagraphBorder, ParagraphBorders,
    Rgb, Run,
};

fn border(dash: Option<(f64, f64)>) -> ParagraphBorder {
    let solid = ParagraphBorder::new(
        BorderEdge {
            width: 1.0,
            color: Rgb::BLACK,
        },
        2.0,
    )
    .unwrap();
    dash.map_or(solid, |(on, off)| solid.with_dash(on, off).unwrap())
}
fn paragraph(text: &str, dash: Option<(f64, f64)>) -> Block {
    let mut p = ParaProps::new();
    p.spacing = LineSpacing::Exact(20.0);
    let e = Some(border(dash));
    p.borders = Some(Box::new(ParagraphBorders {
        top: e,
        right: e,
        bottom: e,
        left: e,
    }));
    Block::Paragraph(p, vec![Run::new(text, style(12.0))])
}
#[test]
fn resolved_dash_validates_emitted_numbers_and_renderer_cycle() {
    let solid = border(None);
    assert_eq!(solid.dash(), None);
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.00001, f64::MAX] {
        assert!(solid.with_dash(value, 1.0).is_err());
        assert!(solid.with_dash(1.0, value).is_err());
    }
    assert!(solid
        .with_dash(f64::from(f32::MAX), f64::from(f32::MAX))
        .is_err());
    assert_eq!(
        solid.with_dash(0.0001, 1.0).unwrap().dash(),
        Some([0.0001, 1.0])
    );
    assert_eq!(solid.dash(), None);
}
#[test]
fn resolved_dash_emits_paths_without_moving_text() {
    let g = PageGeom::new(300.0, 300.0, 50.0);
    let (solid, _) = export(&[paragraph("alpha", None)], g);
    let (dashed, pdf) = export(&[paragraph("alpha", Some((8.0, 2.5)))], g);
    let texts = |ops: &[Op]| {
        ops.iter()
            .filter(|o| matches!(o, Op::Text { .. }))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(&solid[0].ops), texts(&dashed[0].ops));
    let styles = dashed[0]
        .ops
        .iter()
        .filter_map(|o| {
            if let Op::Path {
                stroke: Some(s), ..
            } = o
            {
                Some(s)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(styles.len(), 4);
    assert!(styles.iter().all(|s| s.dashes == [8.0, 2.5]));
    assert!(String::from_utf8_lossy(&pdf.pdf).contains("[8 2.5] 0 d"));
}

fn as_solid(ops: &[Op]) -> Vec<Op> {
    ops.iter().map(|op| match op {
        Op::Path { segs, fill: None, stroke: Some(s) } if !s.dashes.is_empty() => {
            let [pdf_typeset::PathSeg::MoveTo { x: x1, y: y1 }, pdf_typeset::PathSeg::LineTo { x: x2, y: y2 }] = segs.as_slice() else { panic!("border must be a separate straight edge") };
            Op::Line { x1: *x1, y1: *y1, x2: *x2, y2: *y2, width: s.width, color: s.color }
        }
        _ => op.clone(),
    }).collect()
}

#[test]
fn dash_preserves_group_spacing_measurement_and_page_fragments() {
    for geom in [
        PageGeom::new(300.0, 300.0, 50.0),
        PageGeom::new(300.0, 100.0, 20.0),
    ] {
        let make = |dash| {
            let mut a = paragraph("a\nb\nc", dash);
            let mut b = paragraph("d\ne", dash);
            if let Block::Paragraph(p, _) = &mut a {
                p.space_after = 12.0;
            }
            if let Block::Paragraph(p, _) = &mut b {
                p.space_before = 9.0;
                let edge = ParagraphBorder::new(border(None).stroke(), 4.0).unwrap();
                p.borders.as_mut().unwrap().top =
                    Some(dash.map_or(edge, |(on, off)| edge.with_dash(on, off).unwrap()));
            }
            vec![a, b]
        };
        let solid = make(None);
        let dashed = make(Some((8.0, 2.5)));
        assert_eq!(
            ts().measure_blocks(&solid, 200.0, true).height,
            ts().measure_blocks(&dashed, 200.0, true).height
        );
        let (a, _) = export(&solid, geom);
        let (b, pdf) = export(&dashed, geom);
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(&b) {
            assert_eq!(a.ops, as_solid(&b.ops));
        }
        assert_eq!(
            full_text(&pdf.pdf).split_whitespace().collect::<Vec<_>>(),
            ["a", "b", "c", "d", "e"]
        );
        if geom.height == 100.0 {
            assert!(b.len() > 1);
        }
        // Every emitted edge starts a separate path and explicitly resets phase.
        let count = b
            .iter()
            .flat_map(|p| &p.ops)
            .filter(|op| matches!(op, Op::Path { .. }))
            .count();
        assert_eq!(raw(&pdf.pdf).matches("[8 2.5] 0 d").count(), count);
    }
}

#[test]
fn changing_only_dash_breaks_sibling_group() {
    let geom = PageGeom::new(300.0, 300.0, 50.0);
    let paths = |second| {
        let (pages, _) = export(
            &[
                paragraph("a", Some((8.0, 2.5))),
                paragraph("b", Some(second)),
            ],
            geom,
        );
        pages[0]
            .ops
            .iter()
            .filter(|o| matches!(o, Op::Path { .. }))
            .count()
    };
    assert_eq!(paths((8.0, 2.5)), 6); // two sides per paragraph, outer top/bottom
    assert_eq!(paths((3.0, 1.0)), 8);
}

#[test]
fn autofit_preserves_dash_dimensions_and_container_geometry() {
    use pdf_typeset::{Rect, TextBoxSpec};
    let make = |dash| {
        let mut block = paragraph("large\ntext", dash);
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
        ts().layout_text_box(&spec)
    };
    let solid = make(None);
    let dashed = make(Some((8.0, 2.5)));
    assert_eq!(solid, as_solid(&dashed));
    assert!(dashed
        .iter()
        .any(|o| matches!(o, Op::Text { size, .. } if *size < 30.0)));
    assert_eq!(dashed.iter().filter(|o| matches!(o, Op::Path { stroke: Some(s), .. } if s.width == 1.0 && s.dashes == [8.0, 2.5])).count(), 4);
}

#[test]
fn rendered_top_edge_has_phase_zero_butt_dash_interiors() {
    let (pages, pdf) = export(
        &[paragraph("alpha", Some((8.0, 4.0)))],
        PageGeom::new(300.0, 150.0, 50.0),
    );
    let (x, y) = pages[0]
        .ops
        .iter()
        .find_map(|op| match op {
            Op::Path { segs, .. } => match segs.as_slice() {
                [pdf_typeset::PathSeg::MoveTo { x, y }, pdf_typeset::PathSeg::LineTo { y: y2, .. }]
                    if y == y2 =>
                {
                    Some((*x, *y))
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    let pix = render(&pdf.pdf, 0);
    let channels = usize::from(pix.colorspace.components()) + usize::from(pix.alpha);
    let sample = |dx: usize| {
        let col = x as usize + dx;
        let row = y.floor() as usize;
        let i = (row * pix.width as usize + col) * channels;
        pix.samples()[i]
    };
    // Sample strictly inside intervals, away from endpoints and corner joins.
    for cycle in 1..10 {
        assert!(sample(cycle * 12 + 3) < 80, "on interval {cycle}");
        assert!(sample(cycle * 12 + 9) > 245, "off interval {cycle}");
    }
}

#[test]
fn table_cells_keep_bordered_measurement_and_separate_groups() {
    use pdf_typeset::{ColumnWidth, TableCell, TableRow, TableSpec};
    let make = |dash| {
        let mut cell = TableCell::new(vec![paragraph("alpha", dash), paragraph("beta", dash)]);
        cell.padding = 0.0;
        Block::Table(TableSpec::new(
            vec![ColumnWidth::Fixed(200.0)],
            vec![TableRow::new(vec![cell])],
        ))
    };
    let g = PageGeom::new(300.0, 300.0, 50.0);
    let (a, _) = export(&[make(None)], g);
    let (b, pdf) = export(&[make(Some((8.0, 2.5)))], g);
    assert_eq!(a[0].ops, as_solid(&b[0].ops));
    assert_eq!(
        full_text(&pdf.pdf).split_whitespace().collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
}
