//! Paragraph backgrounds share the flow / box / table-cell layout core.
mod common;
use common::*;
use pdf_typeset::{Block, LineSpacing, Op, PageGeom, ParaProps, Rgb, Run};

fn shaded(text: &str, color: Option<Rgb>, after: f64) -> Block {
    let mut props = ParaProps::new();
    props.shading = color;
    props.spacing = LineSpacing::Exact(20.0);
    props.space_after = after;
    Block::Paragraph(props, vec![Run::new(text, style(12.0))])
}

fn fills(ops: &[Op]) -> Vec<(f64, f64, f64, f64)> {
    ops.iter()
        .filter_map(|op| match op {
            Op::FillRect { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

#[test]
fn isolated_and_matching_backgrounds_treat_paragraph_spacing_differently() {
    let color = Some(Rgb::new(0.8, 0.5, 0.3));
    for (first, second_y) in [(None, 80.0), (color, 70.0)] {
        let blocks = [shaded("first", first, 10.0), shaded("second", color, 0.0)];
        let (pages, _) = export(&blocks, PageGeom::new(300.0, 300.0, 50.0));
        let rects = fills(&pages[0].ops);
        assert_eq!(
            rects.last(),
            Some(&(50.0, second_y, 200.0, 100.0 - second_y))
        );
    }
}

#[test]
fn default_background_is_absent_and_different_colors_do_not_join() {
    assert_eq!(ParaProps::new().shading, None);
    let blocks = [
        shaded("first", Some(Rgb::BLACK), 10.0),
        shaded("second", Some(Rgb::new(1.0, 1.0, 1.0)), 0.0),
    ];
    let (pages, _) = export(&blocks, PageGeom::new(300.0, 300.0, 50.0));
    assert_eq!(
        fills(&pages[0].ops),
        [(50.0, 50.0, 200.0, 20.0), (50.0, 80.0, 200.0, 20.0)]
    );
}

#[test]
fn shading_preserves_wrapping_pagination_and_all_text_ops() {
    let text = (0..100)
        .map(|i| format!("word{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let make = |fill| [shaded(&text, fill, 10.0), shaded("tail", fill, 0.0)];
    let geom = PageGeom::new(220.0, 150.0, 30.0);
    let (plain, plain_pdf) = export(&make(None), geom);
    let (painted, painted_pdf) = export(&make(Some(Rgb::BLACK)), geom);
    assert!(plain.len() > 1);
    assert_eq!(plain.len(), painted.len());
    assert_eq!(full_text(&plain_pdf.pdf), full_text(&painted_pdf.pdf));
    for (a, b) in plain.iter().zip(&painted) {
        assert_eq!(
            a.ops,
            b.ops
                .iter()
                .filter(|op| !matches!(op, Op::FillRect { .. }))
                .cloned()
                .collect::<Vec<_>>()
        );
        for (_, y, _, h) in fills(&b.ops) {
            assert!(
                y >= 30.0 && y + h <= 120.0,
                "background crosses page margins: {y}, {h}"
            );
        }
    }
}

#[test]
fn indents_empty_lines_and_short_exact_spacing_keep_background_behind_text() {
    let mut props = ParaProps::new();
    props.shading = Some(Rgb::BLACK);
    props.indent_left = 15.0;
    props.indent_right = 25.0;
    props.first_line_indent = 10.0;
    props.spacing = LineSpacing::Exact(8.0);
    let blocks = [Block::Paragraph(
        props,
        vec![Run::new("First\n\nThird", style(12.0))],
    )];
    let (pages, _) = export(&blocks, PageGeom::new(300.0, 300.0, 50.0));
    assert_eq!(fills(&pages[0].ops), [(65.0, 50.0, 160.0, 24.0)]);
    assert!(matches!(pages[0].ops[0], Op::FillRect { .. }));
    assert!(pages[0].ops[1..]
        .iter()
        .all(|op| !matches!(op, Op::FillRect { .. })));
}

#[test]
fn empty_paragraph_and_explicit_page_break_do_not_leak_background() {
    let color = Some(Rgb::BLACK);
    let blocks = [
        shaded("", color, 10.0),
        Block::PageBreak,
        shaded("next", color, 0.0),
    ];
    let (pages, _) = export(&blocks, PageGeom::new(300.0, 300.0, 50.0));
    assert_eq!(pages.len(), 2);
    for page in pages {
        assert_eq!(fills(&page.ops), [(50.0, 50.0, 200.0, 20.0)]);
    }
}

#[test]
fn textbox_and_table_cell_use_the_same_paragraph_backgrounds() {
    use pdf_typeset::{ColumnWidth, Rect, TableCell, TableRow, TableSpec, TextBoxSpec};
    let block = shaded("text", Some(Rgb::BLACK), 0.0);
    let mut engine = ts();
    let spec = TextBoxSpec::new(
        Rect {
            x0: 50.0,
            y0: 60.0,
            x1: 250.0,
            y1: 160.0,
        },
        vec![block.clone()],
    );
    let ops = engine.layout_text_box(&spec);
    fn nested_fills(ops: &[Op]) -> Vec<(f64, f64, f64, f64)> {
        let mut out = fills(ops);
        for op in ops {
            if let Op::Group { ops, .. } = op {
                out.extend(nested_fills(ops));
            }
        }
        out
    }
    assert_eq!(nested_fills(&ops), [(50.0, 60.0, 200.0, 20.0)]);
    let table = TableSpec::new(
        vec![ColumnWidth::Fixed(200.0)],
        vec![TableRow::new(vec![TableCell::new(vec![block])])],
    );
    let (pages, _) = export(&[Block::Table(table)], PageGeom::new(300.0, 300.0, 50.0));
    assert_eq!(nested_fills(&pages[0].ops), [(50.0, 50.0, 200.0, 20.0)]);
}
