//! Kitchen-sink layout (TS-4/TS-5 gate): one document exercising multi-size
//! mixed runs, justify, decorations, lists, a bordered/filled table, an image,
//! CJK fallback, and a slide-style page of anchored / rotated / clipped /
//! autofit text boxes plus alpha paths. Read back through the repo's own
//! text extraction and rasterized through the repo's own renderer; the PNGs
//! are written to `target/` for visual inspection.

mod common;

use common::*;
use pdf_typeset::{
    Align, Block, BorderEdge, CellBorders, ColumnWidth, Fill, ImageSpec, ListLabel, Op, PageGeom,
    PageOps, ParaProps, PathSeg, Rect, Rgb, Run, RunStyle, TableCell, TableRow, TableSpec,
    TextBoxSpec, Typesetter, VAnchor,
};

fn tiny_png() -> Vec<u8> {
    let pix = pdf_api::Pixmap::new(
        4,
        4,
        pdf_api::Colorspace::Rgb,
        false,
        (0u8..16)
            .flat_map(|i| [15 * i, 255 - 12 * i, 128])
            .collect(),
    );
    let mut out = Vec::new();
    pix.save_png(&mut out).expect("png encode");
    out
}

fn flow_blocks() -> Vec<Block> {
    let mut heading = style(24.0);
    heading.bold = true;
    let mut italic = style(12.0);
    italic.italic = true;
    let mut marked = style(12.0);
    marked.underline = true;
    marked.highlight = Some(Rgb::new(1.0, 0.95, 0.4));
    let mut struck = style(12.0);
    struck.strike = true;
    struck.color = Rgb::new(0.7, 0.1, 0.1);

    let mut justified = ParaProps::new();
    justified.align = Align::Justify;
    justified.space_before = 6.0;
    justified.space_after = 6.0;

    let mut listish = ParaProps::new();
    listish.indent_left = 24.0;
    listish.list = Some(ListLabel {
        text: "1.".to_string(),
        gutter: 6.0,
    });

    let border = BorderEdge {
        width: 0.8,
        color: Rgb::new(0.3, 0.3, 0.3),
    };
    let all_edges = CellBorders {
        top: Some(border),
        right: Some(border),
        bottom: Some(border),
        left: Some(border),
    };
    let cell = |text: &str, fill: Option<Rgb>| {
        let mut c = TableCell::new(vec![Block::Paragraph(
            ParaProps::new(),
            vec![Run::new(text, style(10.0))],
        )]);
        c.padding = 4.0;
        c.borders = all_edges;
        c.fill = fill;
        c
    };

    vec![
        Block::Paragraph(
            ParaProps::new(),
            vec![
                Run::new("Typeset ", heading),
                Run::new("kitchen ", style(14.0)),
                Run::new("sink", italic.clone()),
            ],
        ),
        Block::Paragraph(
            justified,
            vec![Run::new(
                "This justified paragraph flows across several lines so the \
                 interior lines stretch their inter word spaces to reach the \
                 right edge of the measured column exactly as required",
                style(12.0),
            )],
        ),
        Block::Paragraph(
            ParaProps::new(),
            vec![
                Run::new("marked ", marked),
                Run::new("struck ", struck),
                Run::new("plain", style(12.0)),
            ],
        ),
        Block::Paragraph(listish, vec![Run::new("first list item body", style(12.0))]),
        Block::Paragraph(
            ParaProps::new(),
            vec![Run::new(
                "宋体中文回退 CJK fallback line",
                RunStyle::new("宋体", 12.0),
            )],
        ),
        Block::Table(TableSpec::new(
            vec![
                ColumnWidth::Fixed(90.0),
                ColumnWidth::Auto,
                ColumnWidth::Auto,
            ],
            vec![
                TableRow::new(vec![
                    cell("header A", Some(Rgb::new(0.9, 0.9, 0.9))),
                    cell("header B", Some(Rgb::new(0.9, 0.9, 0.9))),
                    cell("header C", Some(Rgb::new(0.9, 0.9, 0.9))),
                ]),
                TableRow::new(vec![
                    cell("alpha", None),
                    cell("beta gamma delta", None),
                    cell("epsilon", None),
                ]),
            ],
        )),
        Block::Image(ImageSpec::new(tiny_png(), 80.0, 60.0)),
    ]
}

/// The slide-style page: boxes + shapes assembled the way ppt-render will.
fn slide_page(engine: &mut Typesetter) -> PageOps {
    let mut ops: Vec<Op> = Vec::new();

    // A translucent backdrop path behind everything.
    ops.push(Op::Path {
        segs: vec![
            PathSeg::MoveTo { x: 40.0, y: 40.0 },
            PathSeg::LineTo { x: 560.0, y: 40.0 },
            PathSeg::LineTo { x: 560.0, y: 400.0 },
            PathSeg::LineTo { x: 40.0, y: 400.0 },
            PathSeg::Close,
        ],
        fill: Some(Fill {
            color: Rgb::new(0.85, 0.92, 1.0),
            alpha: 0.5,
            even_odd: false,
        }),
        stroke: None,
    });

    let mut middle = TextBoxSpec::new(
        Rect {
            x0: 60.0,
            y0: 60.0,
            x1: 300.0,
            y1: 160.0,
        },
        vec![para("middle anchored box content", 14.0)],
    );
    middle.v_anchor = VAnchor::Middle;
    ops.extend(engine.layout_text_box(&middle));

    let mut rotated = TextBoxSpec::new(
        Rect {
            x0: 340.0,
            y0: 60.0,
            x1: 540.0,
            y1: 160.0,
        },
        vec![para("rotated box", 16.0)],
    );
    rotated.rotation_deg = 30.0;
    ops.extend(engine.layout_text_box(&rotated));

    let mut clipped = TextBoxSpec::new(
        Rect {
            x0: 60.0,
            y0: 200.0,
            x1: 300.0,
            y1: 250.0,
        },
        vec![para(
            "clip one\nclip two\nclip three\nclip four\nclip five",
            12.0,
        )],
    );
    clipped.clip = true;
    ops.extend(engine.layout_text_box(&clipped));

    let mut autofit = TextBoxSpec::new(
        Rect {
            x0: 340.0,
            y0: 200.0,
            x1: 540.0,
            y1: 250.0,
        },
        vec![para("fit1\nfit2\nfit3\nfit4\nfit5\nfit6", 14.0)],
    );
    autofit.font_scale = Some(1.0);
    ops.extend(engine.layout_text_box(&autofit));

    PageOps {
        width: 600.0,
        height: 440.0,
        ops,
    }
}

#[test]
fn kitchen_sink_reads_back_and_rasters() {
    let mut engine = Typesetter::with_system_fonts();
    let mut pages = engine.layout_flow(
        &flow_blocks(),
        &mut pdf_typeset::FixedPages::new(PageGeom::new(500.0, 640.0, 50.0)),
    );
    assert_eq!(pages.len(), 1, "flow fixture fits one page");
    let slide = slide_page(&mut engine);
    pages.push(slide);
    let result = engine.emit(&pages).expect("emit");

    // Read-back: every deterministic Latin sentinel survives, in order.
    let text = full_text(&result.pdf);
    for token in [
        "Typeset",
        "kitchen",
        "sink",
        "justified",
        "marked",
        "struck",
        "plain",
        "1.",
        "first",
        "header",
        "alpha",
        "beta",
        "epsilon",
        "middle",
        "anchored",
        "rotated",
        "clip",
        "fit1",
        "fit6",
    ] {
        assert!(text.contains(token), "missing {token:?} in:\n{text}");
    }

    // Both pages raster with real ink through the repo's own renderer.
    for page in 0..2 {
        let pix = render(&result.pdf, page);
        assert!(
            ink_pixels(&pix) > 500,
            "page {page} unexpectedly near-blank"
        );
        let mut png = Vec::new();
        pix.save_png(&mut png).expect("png");
        let path = format!(
            "{}/typeset-kitchen-sink-p{page}.png",
            env!("CARGO_TARGET_TMPDIR")
        );
        std::fs::write(&path, png).expect("write inspection png");
        println!("wrote {path}");
    }

    // The autofit box kept every line and the clipped box warned exactly once.
    let overflow: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, pdf_typeset::ExportWarning::BoxOverflowClipped { .. }))
        .collect();
    assert_eq!(overflow.len(), 1, "{:?}", result.warnings);
}
