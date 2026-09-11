mod common;
use common::*;
use pdf_typeset::{
    Block, BlockPathStep, BorderEdge, ConnectionError, ConnectionReason, ParaProps,
    ParagraphConnection, ParagraphConnections, Rgb, Run,
};

#[test]
fn separator_is_solid_positive_finite_and_connections_are_read_only() {
    for width in [0.0, -1.0, 0.00001, f64::MAX, f64::NAN, f64::INFINITY] {
        assert!(ParagraphConnection::new(
            vec![BlockPathStep::Block(0)],
            vec![BlockPathStep::Block(1)],
            BorderEdge {
                width,
                color: Rgb::BLACK
            }
        )
        .is_err());
    }
    let c = ParagraphConnection::new(
        vec![BlockPathStep::Block(0)],
        vec![BlockPathStep::Block(1)],
        BorderEdge {
            width: 2.0,
            color: Rgb::BLACK,
        },
    )
    .unwrap();
    assert_eq!(c.from(), &[BlockPathStep::Block(0)]);
    assert_eq!(c.to(), &[BlockPathStep::Block(1)]);
    let overlay = ParagraphConnections::new(vec![c]);
    assert_eq!(overlay.connections().len(), 1);
    assert!(!overlay.is_empty());
    let _: Option<ConnectionError> = None;
    let _: Option<ConnectionReason> = None;
    let _ = Block::Paragraph(ParaProps::new(), vec![Run::new("A", style(12.0))]);
}

fn connection(from: usize, to: usize, width: f64) -> ParagraphConnections {
    ParagraphConnections::new(vec![ParagraphConnection::new(
        vec![BlockPathStep::Block(from)],
        vec![BlockPathStep::Block(to)],
        BorderEdge {
            width,
            color: Rgb::new(0.0, 0.0, 1.0),
        },
    )
    .unwrap()])
}
fn bordered(text: &str, color: Rgb) -> Block {
    let e = Some(pdf_typeset::ParagraphBorder::new(BorderEdge { width: 1.0, color }, 0.0).unwrap());
    let mut p = ParaProps::new();
    p.spacing = pdf_typeset::LineSpacing::Exact(14.0);
    p.borders = Some(Box::new(pdf_typeset::ParagraphBorders {
        top: e,
        right: e,
        bottom: e,
        left: e,
    }));
    Block::Paragraph(p, vec![Run::new(text, style(12.0))])
}
#[test]
fn incoming_separator_replaces_two_edges_reserves_once_and_does_not_bridge_sides() {
    let mut a = bordered("A", Rgb::new(1.0, 0.0, 0.0));
    if let Block::Paragraph(p, _) = &mut a {
        p.space_after = 6.0;
    }
    let b = bordered("B", Rgb::BLACK);
    let blocks = [a, b];
    let overlay = connection(0, 1, 3.0);
    let m = ts()
        .try_measure_blocks_with_connections(&blocks, 100.0, true, &overlay)
        .unwrap();
    assert_eq!(m.height, 39.0); // top1 + A14 + gap6 + separator3 + B14 + bottom1
    let pages = ts()
        .try_layout_flow_with_connections(
            &blocks,
            &mut pdf_typeset::FixedPages::new(pdf_typeset::PageGeom::new(100.0, 100.0, 0.0)),
            &overlay,
        )
        .unwrap();
    let ops = &pages[0].ops;
    let horizontal: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            pdf_typeset::Op::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                ..
            } if y1 == y2 => Some((*x1, *y1, *x2, *width)),
            _ => None,
        })
        .collect();
    assert_eq!(horizontal.len(), 3);
    assert_eq!(horizontal.iter().find(|h| h.3 == 3.0).unwrap().1, 22.5);
    let sides: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            pdf_typeset::Op::Line { x1, y1, x2, y2, .. } if x1 == x2 => Some((*y1, *y2)),
            _ => None,
        })
        .collect();
    assert_eq!(
        sides,
        vec![(0.0, 15.0), (0.0, 15.0), (21.0, 39.0), (21.0, 39.0)]
    );
}

#[test]
fn static_errors_are_before_page_provider_and_overlay_revalidates_current_blocks() {
    use pdf_typeset::{ConnectionReason::*, LayoutError};
    struct Never;
    impl pdf_typeset::PageProvider for Never {
        fn next_page(&mut self) -> pdf_typeset::PageGeom {
            panic!("static validation called provider")
        }
    }
    let blocks = [bordered("A", Rgb::BLACK), bordered("B", Rgb::BLACK)];
    for (input, overlay, reason) in [
        (blocks.to_vec(), connection(0, 9, 1.0), InvalidPath),
        (
            vec![blocks[0].clone(), Block::PageBreak],
            connection(0, 1, 1.0),
            NotParagraph,
        ),
        (
            vec![
                blocks[0].clone(),
                Block::PageBreak,
                Block::PageBreak,
                blocks[1].clone(),
            ],
            connection(0, 3, 1.0),
            NotAdjacent,
        ),
        (
            vec![blocks[0].clone(), para(" ", 12.0)],
            connection(0, 1, 1.0),
            NoUsableText,
        ),
    ] {
        let err = ts()
            .try_layout_flow_with_connections(&input, &mut Never, &overlay)
            .unwrap_err();
        assert!(matches!(err,LayoutError::Connection(ConnectionError {reason:r,..}) if r==reason));
    }
    let c = connection(0, 1, 1.0);
    let duplicated =
        ParagraphConnections::new(vec![c.connections()[0].clone(), c.connections()[0].clone()]);
    assert!(matches!(
        ts().try_layout_flow_with_connections(&blocks, &mut Never, &duplicated),
        Err(LayoutError::Connection(ConnectionError {
            reason: DuplicateIncoming,
            ..
        }))
    ));
}

fn set_props(block: &mut Block) -> &mut ParaProps {
    match block {
        Block::Paragraph(p, _) => p,
        _ => unreachable!(),
    }
}
fn fixed(height: f64) -> pdf_typeset::FixedPages {
    pdf_typeset::FixedPages::new(pdf_typeset::PageGeom::new(100.0, height, 0.0))
}
#[test]
fn full_page_source_suppresses_reserve_paint_and_finish_bottom_with_or_without_break() {
    for explicit in [false, true] {
        let mut a = bordered("A", Rgb::BLACK);
        let p = set_props(&mut a);
        p.spacing = pdf_typeset::LineSpacing::Exact(59.0);
        p.borders.as_mut().unwrap().bottom = Some(
            pdf_typeset::ParagraphBorder::new(
                BorderEdge {
                    width: 7.0,
                    color: Rgb::BLACK,
                },
                0.0,
            )
            .unwrap(),
        );
        let mut b = bordered("B", Rgb::BLACK);
        set_props(&mut b).spacing = pdf_typeset::LineSpacing::Exact(20.0);
        let blocks = if explicit {
            vec![a, Block::PageBreak, b]
        } else {
            vec![a, b]
        };
        let pages = ts()
            .try_layout_flow_with_connections(
                &blocks,
                &mut fixed(60.0),
                &connection(0, blocks.len() - 1, 5.0),
            )
            .unwrap();
        assert_eq!(pages.len(), 2);
        assert!(pages[0]
            .ops
            .iter()
            .any(|o| matches!(o,pdf_typeset::Op::Text{text,..} if text=="A")));
        assert!(!pages
            .iter()
            .flat_map(|p| &p.ops)
            .any(|o| matches!(o,pdf_typeset::Op::Line {width,..} if *width==7.0)));
        assert_eq!(pages[1].ops.iter().filter(|o|matches!(o,pdf_typeset::Op::Line {width,y1,y2,..} if *width==5.0 && y1==y2 && *y1==2.5)).count(),1);
    }
}
#[test]
fn separator_fit_error_is_typed_bounded_and_does_not_return_partial_operations() {
    struct Count {
        calls: usize,
    }
    impl pdf_typeset::PageProvider for Count {
        fn next_page(&mut self) -> pdf_typeset::PageGeom {
            self.calls += 1;
            assert!(self.calls <= 2);
            pdf_typeset::PageGeom::new(100.0, 60.0, 0.0)
        }
    }
    let a = bordered("A", Rgb::BLACK);
    let mut b = bordered("B", Rgb::BLACK);
    set_props(&mut b).spacing = pdf_typeset::LineSpacing::Exact(20.0);
    let blocks = [a, b];
    for width in [39.0, 40.0] {
        let mut provider = Count { calls: 0 };
        let out =
            ts().try_layout_flow_with_connections(&blocks, &mut provider, &connection(0, 1, width));
        if width == 39.0 {
            assert_eq!(out.unwrap().len(), 2)
        } else {
            assert!(matches!(
                out,
                Err(pdf_typeset::LayoutError::Connection(ConnectionError {
                    reason: ConnectionReason::UnusableGeometry,
                    ..
                }))
            ));
        }
        assert_eq!(provider.calls, 2);
    }
}
#[test]
fn separator_is_only_at_boundary_not_repeated_inside_either_paragraph() {
    let mut a = bordered("A1\nA2\nA3", Rgb::BLACK);
    set_props(&mut a).spacing = pdf_typeset::LineSpacing::Exact(20.0);
    let mut b = bordered("B1\nB2\nB3", Rgb::BLACK);
    set_props(&mut b).spacing = pdf_typeset::LineSpacing::Exact(20.0);
    let pages = ts()
        .try_layout_flow_with_connections(&[a, b], &mut fixed(45.0), &connection(0, 1, 3.0))
        .unwrap();
    let separators = pages
        .iter()
        .flat_map(|p| &p.ops)
        .filter(|o| matches!(o,pdf_typeset::Op::Line{width,y1,y2,..} if *width==3.0 && y1==y2))
        .count();
    assert_eq!(separators, 1);
    assert_eq!(pages.len(), 3);
    let text: Vec<_> = pages
        .iter()
        .flat_map(|p| &p.ops)
        .filter_map(|o| match o {
            pdf_typeset::Op::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, vec!["A1", "A2", "A3", "B1", "B2", "B3"]);
}

#[test]
fn incoming_first_line_rewraps_and_repositions_after_page_provider_changes_geometry() {
    struct Changing {
        calls: usize,
    }
    impl pdf_typeset::PageProvider for Changing {
        fn next_page(&mut self) -> pdf_typeset::PageGeom {
            self.calls += 1;
            if self.calls == 1 {
                pdf_typeset::PageGeom::new(300.0, 100.0, 20.0)
            } else {
                let mut g = pdf_typeset::PageGeom::new(100.0, 400.0, 20.0);
                g.margin_left = 30.0;
                g
            }
        }
    }
    let mut a = bordered("A", Rgb::BLACK);
    set_props(&mut a).spacing = pdf_typeset::LineSpacing::Exact(59.0);
    let b = bordered("alpha beta gamma delta epsilon", Rgb::BLACK);
    let expected = ts()
        .measure_blocks(std::slice::from_ref(&b), 50.0, true)
        .lines
        .len();
    assert!(expected > 1);
    let pages = ts()
        .try_layout_flow_with_connections(
            &[a, b],
            &mut Changing { calls: 0 },
            &connection(0, 1, 3.0),
        )
        .unwrap();
    assert_eq!(pages.len(), 2);
    let text: Vec<_> = pages[1]
        .ops
        .iter()
        .filter_map(|o| match o {
            pdf_typeset::Op::Text { x, baseline, .. } => Some((*x, *baseline)),
            _ => None,
        })
        .collect();
    let mut baselines: Vec<_> = text.iter().map(|p| p.1).collect();
    baselines.dedup();
    assert_eq!(baselines.len(), expected);
    assert!(text.iter().all(|p| p.0 >= 30.0));
}

#[test]
fn internal_page_origin_moves_but_internal_width_change_returns_typed_error() {
    struct Changing {
        calls: usize,
        shrink: bool,
    }
    impl pdf_typeset::PageProvider for Changing {
        fn next_page(&mut self) -> pdf_typeset::PageGeom {
            self.calls += 1;
            assert!(self.calls <= 2);
            let mut g = pdf_typeset::PageGeom::new(100.0, 85.0, 20.0);
            if self.calls == 2 {
                g.margin_left = 30.0;
                g.margin_right = if self.shrink { 20.0 } else { 10.0 };
            }
            g
        }
    }
    let mut a = bordered("A1\nA2\nA3", Rgb::BLACK);
    let p = set_props(&mut a);
    p.spacing = pdf_typeset::LineSpacing::Exact(20.0);
    p.indent_left = 4.0;
    p.first_line_indent = -2.0;
    let mut b = bordered("B", Rgb::BLACK);
    set_props(&mut b).indent_left = 4.0;
    set_props(&mut b).first_line_indent = -2.0;
    let blocks = [a, b];
    let overlay = connection(0, 1, 3.0);
    let pages = ts()
        .try_layout_flow_with_connections(
            &blocks,
            &mut Changing {
                calls: 0,
                shrink: false,
            },
            &overlay,
        )
        .unwrap();
    assert!(pages[1]
        .ops
        .iter()
        .any(|o| matches!(o,pdf_typeset::Op::Text{x,text,..} if text=="A3" && *x==34.0)));
    let error = ts()
        .try_layout_flow_with_connections(
            &blocks,
            &mut Changing {
                calls: 0,
                shrink: true,
            },
            &overlay,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        pdf_typeset::LayoutError::Connection(ConnectionError {
            reason: ConnectionReason::ChangingParagraphGeometry,
            ..
        })
    ));
}

fn cell_path(block: usize) -> Vec<BlockPathStep> {
    vec![
        BlockPathStep::Block(0),
        BlockPathStep::Row(0),
        BlockPathStep::Cell(0),
        BlockPathStep::Block(block),
    ]
}
#[test]
fn cell_paths_survive_autofit_clone_and_share_row_measurement_and_v_anchor() {
    use pdf_typeset::{
        ColumnWidth, Op, Rect, TableCell, TableRow, TableSpec, TextBoxSpec, VAnchor,
    };
    let mut cell = TableCell::new(vec![bordered("A", Rgb::BLACK), bordered("B", Rgb::BLACK)]);
    cell.padding = 2.0;
    cell.v_align = VAnchor::Bottom;
    let mut row = TableRow::new(vec![cell]);
    row.min_height = Some(80.0);
    let blocks = vec![Block::Table(TableSpec::new(
        vec![ColumnWidth::Fixed(100.0)],
        vec![row],
    ))];
    let overlay = ParagraphConnections::new(vec![ParagraphConnection::new(
        cell_path(0),
        cell_path(1),
        BorderEdge {
            width: 3.0,
            color: Rgb::new(0.0, 0.0, 1.0),
        },
    )
    .unwrap()]);
    let mut spec = TextBoxSpec::new(Rect::new(10.0, 20.0, 110.0, 120.0), blocks.clone());
    spec.font_scale = Some(0.5);
    spec.v_anchor = VAnchor::Middle;
    let m = ts()
        .try_measure_text_box_with_connections(&spec, &overlay)
        .unwrap();
    assert_eq!(m.height, 80.0);
    let ops = ts()
        .try_layout_text_box_with_connections(&spec, &overlay)
        .unwrap();
    assert_eq!(
        ops.iter()
            .filter(|o| matches!(o,Op::Line{width,..} if *width==3.0))
            .count(),
        1
    );
    assert!(ops
        .iter()
        .any(|o| matches!(o,Op::Text{size,..} if *size==6.0)));
    let flow = ts()
        .try_layout_flow_with_connections(&blocks, &mut fixed(100.0), &overlay)
        .unwrap();
    let separator = flow[0]
        .ops
        .iter()
        .find_map(|o| match o {
            Op::Line { width, y1, y2, .. } if *width == 3.0 && y1 == y2 => Some(*y1),
            _ => None,
        })
        .unwrap();
    assert_eq!(separator, 61.5); // bottom alignment: content33 starts45; separator center45+15+1.5
    assert_eq!(spec.blocks, blocks);
}
#[test]
fn nested_paths_cannot_cross_cells_or_target_ignored_cells_and_are_rechecked() {
    use pdf_typeset::{ColumnWidth, LayoutError, TableCell, TableRow, TableSpec};
    let blocks = vec![Block::Table(TableSpec::new(
        vec![ColumnWidth::Fixed(100.0), ColumnWidth::Fixed(100.0)],
        vec![TableRow::new(vec![
            TableCell::new(vec![bordered("A", Rgb::BLACK), bordered("B", Rgb::BLACK)]),
            TableCell::new(vec![bordered("C", Rgb::BLACK), bordered("D", Rgb::BLACK)]),
        ])],
    ))];
    let mut other = cell_path(0);
    other[2] = BlockPathStep::Cell(1);
    let cross = ParagraphConnections::new(vec![ParagraphConnection::new(
        cell_path(0),
        other,
        BorderEdge {
            width: 1.0,
            color: Rgb::BLACK,
        },
    )
    .unwrap()]);
    assert!(matches!(
        ts().try_measure_blocks_with_connections(&blocks, 200.0, true, &cross),
        Err(LayoutError::Connection(ConnectionError {
            reason: ConnectionReason::DifferentContainer,
            ..
        }))
    ));
    let overlay = ParagraphConnections::new(vec![ParagraphConnection::new(
        cell_path(0),
        cell_path(1),
        BorderEdge {
            width: 1.0,
            color: Rgb::BLACK,
        },
    )
    .unwrap()]);
    assert!(ts()
        .try_measure_blocks_with_connections(&blocks, 200.0, true, &overlay)
        .is_ok());
    let mut ignored = blocks.clone();
    if let Block::Table(t) = &mut ignored[0] {
        t.columns.truncate(1);
    }
    let mut ignored_from = cell_path(0);
    ignored_from[2] = BlockPathStep::Cell(1);
    let mut ignored_to = cell_path(1);
    ignored_to[2] = BlockPathStep::Cell(1);
    let ignored_overlay = ParagraphConnections::new(vec![ParagraphConnection::new(
        ignored_from,
        ignored_to,
        BorderEdge {
            width: 1.0,
            color: Rgb::BLACK,
        },
    )
    .unwrap()]);
    assert!(matches!(
        ts().try_measure_blocks_with_connections(&ignored, 200.0, true, &ignored_overlay),
        Err(LayoutError::Connection(ConnectionError {
            reason: ConnectionReason::InvalidPath,
            ..
        }))
    ));
    let mut edited = blocks;
    if let Block::Table(t) = &mut edited[0] {
        t.rows[0].cells[0].blocks.remove(1);
    }
    assert!(matches!(
        ts().try_measure_blocks_with_connections(&edited, 200.0, true, &overlay),
        Err(LayoutError::Connection(ConnectionError {
            reason: ConnectionReason::InvalidPath,
            ..
        }))
    ));
}
#[test]
fn empty_overlay_keeps_all_old_checked_outputs_and_error_types() {
    use pdf_typeset::{Measurement, Op, PageOps, Rect, SignedSpacingError, TextBoxSpec};
    let blocks = [bordered("default", Rgb::BLACK)];
    let empty = ParagraphConnections::default();
    let old: Result<Vec<PageOps>, SignedSpacingError> =
        ts().try_layout_flow(&blocks, &mut fixed(100.0));
    assert_eq!(
        old.unwrap(),
        ts().try_layout_flow_with_connections(&blocks, &mut fixed(100.0), &empty)
            .unwrap()
    );
    let spec = TextBoxSpec::new(Rect::new(0.0, 0.0, 100.0, 100.0), blocks.to_vec());
    let old: Result<Vec<Op>, SignedSpacingError> = ts().try_layout_text_box(&spec);
    assert_eq!(
        old.unwrap(),
        ts().try_layout_text_box_with_connections(&spec, &empty)
            .unwrap()
    );
    let old: Result<Measurement, SignedSpacingError> =
        ts().try_measure_blocks(&blocks, 100.0, true);
    assert_eq!(
        old.unwrap(),
        ts().try_measure_blocks_with_connections(&blocks, 100.0, true, &empty)
            .unwrap()
    );
    let old: Result<Measurement, SignedSpacingError> = ts().try_measure_text_box(&spec);
    assert_eq!(
        old.unwrap(),
        ts().try_measure_text_box_with_connections(&spec, &empty)
            .unwrap()
    );
}

#[test]
fn chain_consumes_each_separator_once_and_autofit_keeps_separator_geometry() {
    use pdf_typeset::{Op, Rect, TextBoxSpec};
    let mut blocks = vec![
        bordered("A", Rgb::BLACK),
        bordered("B", Rgb::BLACK),
        bordered("C", Rgb::BLACK),
    ];
    for block in &mut blocks {
        if let Block::Paragraph(p, runs) = block {
            p.spacing = pdf_typeset::LineSpacing::Multiple(1.0);
            runs[0].style.size = 24.0;
        }
    }
    let a = connection(0, 1, 3.0);
    let b = connection(1, 2, 5.0);
    let overlay =
        ParagraphConnections::new(vec![a.connections()[0].clone(), b.connections()[0].clone()]);
    let mut spec = TextBoxSpec::new(Rect::new(0.0, 0.0, 100.0, 40.0), blocks);
    spec.font_scale = Some(1.0);
    assert!(
        ts().try_measure_text_box_with_connections(&spec, &overlay)
            .unwrap()
            .height
            > 40.0
    );
    let ops = ts()
        .try_layout_text_box_with_connections(&spec, &overlay)
        .unwrap();
    for width in [3.0, 5.0] {
        assert_eq!(
            ops.iter()
                .filter(|o| matches!(o,Op::Line{width:w,y1,y2,..} if *w==width && y1==y2))
                .count(),
            1
        );
    }
    assert!(ops
        .iter()
        .filter_map(|o| match o {
            Op::Text { size, .. } => Some(*size),
            _ => None,
        })
        .all(|size| size < 24.0));
    let bottom = ops
        .iter()
        .filter_map(|o| match o {
            Op::Line { y1, y2, .. } => Some(y1.max(*y2)),
            _ => None,
        })
        .fold(0.0, f64::max);
    assert!(bottom <= 40.0 + 0.001);
}
