//! TS-1 model-construction tests: the input IR, op IR and warning channel
//! build and behave as the PRD §10 sketch specifies.

use pdf_typeset::ops::{FaceId, Fill, LineCap, LineJoin, Op, PageOps, PathSeg, Stroke};
use pdf_typeset::{
    Align, Block, BorderEdge, CellBorders, ColumnWidth, ExportResult, ExportWarning, ImageSpec,
    LineSpacing, ListLabel, Matrix, ParaProps, Rect, Rgb, Run, RunStyle, TableCell, TableRow,
    TableSpec, TextBoxSpec, VAnchor,
};

#[test]
fn run_style_defaults_are_plain_black() {
    let style = RunStyle::new("Calibri", 12.0);
    assert_eq!(style.family, "Calibri");
    assert_eq!(style.size, 12.0);
    assert!(!style.bold && !style.italic && !style.underline && !style.strike);
    assert_eq!(style.color, Rgb::BLACK);
    assert_eq!(style.highlight, None);
}

#[test]
fn paragraph_props_default_to_word_neutral_values() {
    let props = ParaProps::default();
    assert_eq!(props.align, Align::Left);
    assert_eq!(props.spacing, LineSpacing::Multiple(1.0));
    assert_eq!(props.space_before, 0.0);
    assert_eq!(props.space_after, 0.0);
    assert_eq!(props.indent_left, 0.0);
    assert_eq!(props.indent_right, 0.0);
    assert_eq!(props.first_line_indent, 0.0);
    assert_eq!(props.list, None);
}

#[test]
fn builds_a_mixed_paragraph_with_list_label_and_hanging_indent() {
    let mut bold = RunStyle::new("宋体", 10.5);
    bold.bold = true;
    bold.highlight = Some(Rgb::new(1.0, 1.0, 0.0));
    let mut props = ParaProps::new();
    props.align = Align::Justify;
    props.spacing = LineSpacing::Exact(18.0);
    props.space_before = 6.0;
    props.first_line_indent = -12.0; // hanging
    props.list = Some(ListLabel {
        text: "(a)".to_string(),
        gutter: 7.0,
    });
    let para = Block::Paragraph(
        props.clone(),
        vec![
            Run::new("重点", bold),
            Run::new(" plain tail", RunStyle::new("Calibri", 10.5)),
        ],
    );
    let Block::Paragraph(got_props, runs) = &para else {
        panic!("not a paragraph");
    };
    assert_eq!(*got_props, props);
    assert_eq!(runs.len(), 2);
    assert!(runs[0].style.bold);
    assert_eq!(runs[0].style.highlight, Some(Rgb::new(1.0, 1.0, 0.0)));
    assert_eq!(got_props.list.as_ref().map(|l| l.gutter), Some(7.0));
}

#[test]
fn builds_a_table_spec_with_per_edge_borders() {
    let edge = BorderEdge {
        width: 0.75,
        color: Rgb::new(0.45, 0.45, 0.45),
    };
    let mut cell = TableCell::new(vec![Block::Paragraph(
        ParaProps::default(),
        vec![Run::new("cell", RunStyle::new("Liberation Sans", 9.0))],
    )]);
    cell.fill = Some(Rgb::new(0.92, 0.92, 0.92));
    cell.borders = CellBorders {
        top: Some(edge),
        right: None,
        bottom: Some(edge),
        left: None,
    };
    cell.padding = 4.0;
    let mut row = TableRow::new(vec![cell.clone(), TableCell::new(vec![])]);
    row.min_height = Some(20.0);
    let table = TableSpec::new(
        vec![ColumnWidth::Fixed(120.0), ColumnWidth::Auto],
        vec![row],
    );
    assert_eq!(table.columns.len(), 2);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.rows[0].min_height, Some(20.0));
    let borders = table.rows[0].cells[0].borders;
    assert!(borders.top.is_some() && borders.right.is_none());
    assert_eq!(table.rows[0].cells[0].padding, 4.0);
    // Default borders paint nothing.
    assert_eq!(CellBorders::default(), CellBorders::default());
    assert!(CellBorders::default().bottom.is_none());
}

#[test]
fn text_box_spec_defaults_and_overrides() {
    let rect = Rect::new(100.0, 200.0, 300.0, 260.0);
    let boxed = TextBoxSpec::new(rect, vec![Block::PageBreak]);
    assert_eq!(boxed.rect, rect);
    assert_eq!(boxed.v_anchor, VAnchor::Top);
    assert!(boxed.wrap);
    assert_eq!(boxed.font_scale, None);
    assert_eq!(boxed.rotation_deg, 0.0);
    assert!(!boxed.clip);

    let mut auto_fit = TextBoxSpec::new(rect, vec![]);
    auto_fit.v_anchor = VAnchor::Middle;
    auto_fit.wrap = false;
    auto_fit.font_scale = Some(0.62);
    auto_fit.rotation_deg = 90.0;
    auto_fit.clip = true;
    assert_eq!(auto_fit.v_anchor, VAnchor::Middle);
    assert_eq!(auto_fit.font_scale, Some(0.62));
}

#[test]
fn image_spec_carries_bytes_and_display_size() {
    let img = ImageSpec::new(vec![0xFF, 0xD8, 0xFF], 240.0, 180.0);
    assert_eq!(img.data.len(), 3);
    assert_eq!((img.width, img.height), (240.0, 180.0));
    let block = Block::Image(img);
    assert!(matches!(block, Block::Image(_)));
}

#[test]
fn op_ir_carries_per_op_face_and_size_plus_shape_alpha_clip() {
    // Size-carrying text with open face ids (no fixed Face enum).
    let text = Op::Text {
        face: FaceId(3),
        size: 21.5,
        color: Rgb::BLACK,
        x: 72.0,
        baseline: 90.0,
        text: "mixed".to_string(),
    };
    assert!(FaceId(3) > FaceId(2));

    // A translucent even-odd donut fill with a round-join stroke.
    let ring = Op::Path {
        segs: vec![
            PathSeg::MoveTo { x: 0.0, y: 0.0 },
            PathSeg::CurveTo {
                x1: 1.0,
                y1: 0.0,
                x2: 2.0,
                y2: 1.0,
                x: 2.0,
                y: 2.0,
            },
            PathSeg::Close,
        ],
        fill: Some(Fill {
            alpha: 0.5,
            even_odd: true,
            ..Fill::new(Rgb::new(0.2, 0.4, 0.9))
        }),
        stroke: Some(Stroke {
            join: LineJoin::Round,
            cap: LineCap::Round,
            dashes: vec![3.0, 1.0],
            ..Stroke::new(Rgb::BLACK, 1.5)
        }),
    };
    // Defaults: opaque, butt/miter, solid, nonzero winding.
    let plain_fill = Fill::new(Rgb::BLACK);
    assert_eq!(plain_fill.alpha, 1.0);
    assert!(!plain_fill.even_odd);
    let plain_stroke = Stroke::new(Rgb::BLACK, 1.0);
    assert_eq!(plain_stroke.alpha, 1.0);
    assert_eq!(plain_stroke.cap, LineCap::Butt);
    assert_eq!(plain_stroke.join, LineJoin::Miter);
    assert!(plain_stroke.dashes.is_empty());

    // q cm W n … Q grouping for shape transforms / box clips.
    let group = Op::Group {
        transform: Some(Matrix::rotate(45.0)),
        clip: Some(vec![
            PathSeg::MoveTo { x: 0.0, y: 0.0 },
            PathSeg::LineTo { x: 10.0, y: 0.0 },
            PathSeg::LineTo { x: 10.0, y: 10.0 },
            PathSeg::Close,
        ]),
        ops: vec![text, ring],
    };
    let page = PageOps {
        width: 595.32,
        height: 841.92,
        ops: vec![group],
    };
    let Op::Group { ops, clip, .. } = &page.ops[0] else {
        panic!("not a group");
    };
    assert_eq!(ops.len(), 2);
    assert_eq!(clip.as_ref().map(Vec::len), Some(4));
}

#[test]
fn export_warnings_render_human_readable() {
    let warnings = vec![
        ExportWarning::FontSubstituted {
            requested: "宋体".to_string(),
            used: "Songti SC".to_string(),
        },
        ExportWarning::StyleApproximated {
            family: "Noto Sans Math".to_string(),
            bold: true,
            italic: false,
        },
        ExportWarning::GlyphFallback {
            ch: '中',
            family: "Calibri".to_string(),
        },
        ExportWarning::PresetDegraded {
            preset: "gear9".to_string(),
        },
        ExportWarning::GradientDegraded {
            kind: "linear".to_string(),
        },
        ExportWarning::BoxOverflowClipped { overflow_pt: 12.5 },
    ];
    let rendered: Vec<String> = warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        rendered[0],
        "font '宋体' not available; substituted 'Songti SC'"
    );
    assert!(rendered[1].contains("no bold face"));
    assert!(rendered[2].contains("U+4E2D"));
    assert!(rendered[3].contains("gear9"));
    assert!(rendered[4].contains("linear gradient"));
    assert!(rendered[5].contains("12.50 pt"));
    let result = ExportResult {
        pdf: Vec::new(),
        warnings,
    };
    assert_eq!(result.warnings.len(), 6);
}
