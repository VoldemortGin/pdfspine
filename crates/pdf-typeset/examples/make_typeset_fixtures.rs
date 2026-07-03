//! Generate the committed typeset conformance fixtures (PRD §10 TS-7).
//!
//! Writes four deterministic, license-clean PDFs under `fixtures/typeset/`
//! using the **bundled-face resolver only** (`FontResolver::with_platform`,
//! fixed to [`Platform::MacOs`] substitution tables so the output is identical
//! on every host — no system-font dependence). The emitted bytes are
//! byte-reproducible (the TS-4 emit contract), so CI regenerates them and
//! asserts `git diff --exit-code -- fixtures/typeset` — the same no-drift norm
//! as `conformance/gt/make_ci_fixtures.py`.
//!
//! Fixtures:
//!
//! * `typeset-flow.pdf`     — two flow pages: mixed-size heading, justify,
//!   decorations, list labels, hanging indent, exact line spacing, a
//!   bordered/filled table, an image, alignment page (TS-4 surface).
//! * `typeset-box.pdf`      — one slide page: alpha backdrop, preset shapes,
//!   anchored / rotated / autofit / clipped text boxes (TS-5/TS-6 surface).
//!   The clipped box's text fits its rect on purpose: `pdf-render` does not
//!   soft-clip glyphs (text.rs fill_path mask = None), so an overflowing clip
//!   box would raster differently here than in external readers.
//! * `typeset-lo-doc.pdf`   — Letter page mirroring the `sample.docx` built by
//!   `conformance/gt/typeset_lo_oracle.py` (local-only LibreOffice oracle).
//! * `typeset-lo-slide.pdf` — 10×7.5 in slide mirroring that script's
//!   `sample.pptx`.
//!
//! Run from the repo root:
//!
//! ```text
//! cargo run -p pdf-typeset --example make_typeset_fixtures
//! ```

use std::path::{Path, PathBuf};

use pdf_typeset::{
    preset, Align, Block, BorderEdge, CellBorders, ColumnWidth, Fill, FixedPages, FontResolver,
    ImageSpec, LineSpacing, ListLabel, Op, PageGeom, PageOps, ParaProps, Platform, Rect, Rgb, Run,
    RunStyle, Stroke, TableCell, TableRow, TableSpec, TextBoxSpec, Typesetter, VAnchor,
};

const SANS: &str = "Liberation Sans";
const SERIF: &str = "Liberation Serif";

fn engine() -> Typesetter {
    // Fixed platform ⇒ fixed substitution tables ⇒ identical output on every
    // host; only bundled faces are loaded, so nothing system-dependent leaks.
    Typesetter::new(FontResolver::with_platform(Platform::MacOs))
}

fn style(size: f64) -> RunStyle {
    RunStyle::new(SANS, size)
}

fn para(text: &str, style: RunStyle) -> Block {
    Block::Paragraph(ParaProps::new(), vec![Run::new(text, style)])
}

/// A deterministic 4×4 RGB gradient PNG (encoded by the repo's own encoder).
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

// --------------------------------------------------------------------------
// typeset-flow.pdf — the TS-4 flow surface, two pages.
// --------------------------------------------------------------------------
fn flow_fixture() -> Vec<u8> {
    let mut heading = style(22.0);
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

    let list_item = |label: &str| {
        let mut p = ParaProps::new();
        p.indent_left = 24.0;
        p.list = Some(ListLabel::new(label, 6.0));
        p
    };

    let mut hanging = ParaProps::new();
    hanging.indent_left = 18.0;
    hanging.first_line_indent = -18.0;

    let mut exact = ParaProps::new();
    exact.spacing = LineSpacing::Exact(20.0);

    let mut centered = ParaProps::new();
    centered.align = Align::Center;
    let mut righted = ParaProps::new();
    righted.align = Align::Right;

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
    let gray = Some(Rgb::new(0.9, 0.9, 0.9));

    let blocks = vec![
        Block::Paragraph(
            ParaProps::new(),
            vec![
                Run::new("Typeset conformance ", heading),
                Run::new("flow ", style(14.0)),
                Run::new("fixture", italic),
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
        Block::Paragraph(
            list_item("1."),
            vec![Run::new("first list item body", style(12.0))],
        ),
        Block::Paragraph(
            list_item("2."),
            vec![Run::new("second list item body", style(12.0))],
        ),
        Block::Paragraph(
            hanging,
            vec![Run::new(
                "hanging indent paragraph whose first line starts at the \
                 margin while every following wrapped line is indented under it",
                style(12.0),
            )],
        ),
        Block::Paragraph(
            exact,
            vec![Run::new(
                "exact twenty point line spacing paragraph wrapped over \
                 enough words to produce several evenly spaced lines here",
                style(12.0),
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
                    cell("header A", gray),
                    cell("header B", gray),
                    cell("header C", gray),
                ]),
                TableRow::new(vec![
                    cell("alpha", None),
                    cell("beta gamma delta", None),
                    cell("epsilon", None),
                ]),
            ],
        )),
        Block::Image(ImageSpec::new(tiny_png(), 80.0, 60.0)),
        Block::PageBreak,
        para("Second page continues the flow fixture", style(14.0)),
        Block::Paragraph(
            centered,
            vec![Run::new("centered paragraph on page two", style(12.0))],
        ),
        Block::Paragraph(
            righted,
            vec![Run::new("right aligned paragraph on page two", style(12.0))],
        ),
    ];

    let mut engine = engine();
    let pages = engine.layout_flow(
        &blocks,
        &mut FixedPages::new(PageGeom::new(500.0, 640.0, 50.0)),
    );
    assert_eq!(pages.len(), 2, "flow fixture must paginate to two pages");
    let result = engine.emit(&pages).expect("emit flow fixture");
    assert!(
        result.warnings.is_empty(),
        "flow fixture must be warning-free: {:?}",
        result.warnings
    );
    result.pdf
}

// --------------------------------------------------------------------------
// typeset-box.pdf — the TS-5/TS-6 slide surface, one page.
// --------------------------------------------------------------------------
fn shape(name: &str, rect: Rect, fill: Rgb, stroke: Rgb) -> Op {
    let outline = preset::preset_outline(name, rect, &[]);
    assert!(!outline.degraded, "fixture preset {name} must be supported");
    Op::Path {
        segs: outline.segs,
        fill: Some(Fill::new(fill)),
        stroke: Some(Stroke::new(stroke, 1.2)),
    }
}

fn box_fixture() -> Vec<u8> {
    let mut engine = engine();
    let mut ops: Vec<Op> = vec![
        // Translucent backdrop (constant-alpha ExtGState).
        Op::Path {
            segs: preset::preset_outline(
                "rect",
                Rect {
                    x0: 40.0,
                    y0: 40.0,
                    x1: 560.0,
                    y1: 400.0,
                },
                &[],
            )
            .segs,
            fill: Some(Fill {
                color: Rgb::new(0.85, 0.92, 1.0),
                alpha: 0.5,
                even_odd: false,
            }),
            stroke: None,
        },
        // Preset shapes along the bottom band.
        shape(
            "roundRect",
            Rect {
                x0: 60.0,
                y0: 300.0,
                x1: 180.0,
                y1: 380.0,
            },
            Rgb::new(0.75, 0.85, 1.0),
            Rgb::new(0.2, 0.3, 0.6),
        ),
        shape(
            "star5",
            Rect {
                x0: 220.0,
                y0: 300.0,
                x1: 320.0,
                y1: 380.0,
            },
            Rgb::new(1.0, 0.85, 0.3),
            Rgb::new(0.8, 0.5, 0.1),
        ),
        shape(
            "rightArrow",
            Rect {
                x0: 360.0,
                y0: 310.0,
                x1: 540.0,
                y1: 370.0,
            },
            Rgb::new(0.6, 0.85, 0.6),
            Rgb::new(0.1, 0.4, 0.1),
        ),
    ];

    // Anchored boxes.
    let mut middle = TextBoxSpec::new(
        Rect {
            x0: 60.0,
            y0: 60.0,
            x1: 300.0,
            y1: 160.0,
        },
        vec![para("middle anchored box content", style(14.0))],
    );
    middle.v_anchor = VAnchor::Middle;
    ops.extend(engine.layout_text_box(&middle));

    let mut bottom = TextBoxSpec::new(
        Rect {
            x0: 60.0,
            y0: 180.0,
            x1: 300.0,
            y1: 280.0,
        },
        vec![para("bottom anchored box content", style(14.0))],
    );
    bottom.v_anchor = VAnchor::Bottom;
    ops.extend(engine.layout_text_box(&bottom));

    // Rotation: 90° hits Matrix::rotate's exact cardinal path, keeping the
    // committed bytes free of libm sin/cos variance across hosts.
    let mut rotated = TextBoxSpec::new(
        Rect {
            x0: 340.0,
            y0: 60.0,
            x1: 540.0,
            y1: 160.0,
        },
        vec![para("rotated box", style(16.0))],
    );
    rotated.rotation_deg = 90.0;
    ops.extend(engine.layout_text_box(&rotated));

    // Autofit: six hard-broken lines in a short box scale down losslessly.
    let mut autofit = TextBoxSpec::new(
        Rect {
            x0: 340.0,
            y0: 180.0,
            x1: 540.0,
            y1: 230.0,
        },
        vec![para("fit1\nfit2\nfit3\nfit4\nfit5\nfit6", style(14.0))],
    );
    autofit.font_scale = Some(1.0);
    ops.extend(engine.layout_text_box(&autofit));

    // Clipped box whose text FITS (see module docs: no overflowing clip in
    // committed raster fixtures — pdf-render draws glyphs unclipped).
    let mut clipped = TextBoxSpec::new(
        Rect {
            x0: 340.0,
            y0: 240.0,
            x1: 540.0,
            y1: 290.0,
        },
        vec![para("clipped box fits inside", style(12.0))],
    );
    clipped.clip = true;
    ops.extend(engine.layout_text_box(&clipped));

    let pages = vec![PageOps {
        width: 600.0,
        height: 440.0,
        ops,
    }];
    let result = engine.emit(&pages).expect("emit box fixture");
    assert!(
        result.warnings.is_empty(),
        "box fixture must be warning-free: {:?}",
        result.warnings
    );
    result.pdf
}

// --------------------------------------------------------------------------
// typeset-lo-doc.pdf — mirrors typeset_lo_oracle.py's sample.docx.
// Letter page, 1 in margins, Liberation Serif (LibreOffice's docx default).
// Keep this text in sync with DOC_TITLE/DOC_PARAS in typeset_lo_oracle.py.
// --------------------------------------------------------------------------
fn lo_doc_fixture() -> Vec<u8> {
    let mut title = RunStyle::new(SERIF, 24.0);
    title.bold = true;
    let mut title_para = ParaProps::new();
    title_para.space_after = 12.0;
    let body = |text: &str| {
        let mut p = ParaProps::new();
        p.space_after = 10.0;
        Block::Paragraph(p, vec![Run::new(text, RunStyle::new(SERIF, 12.0))])
    };

    let blocks = vec![
        Block::Paragraph(
            title_para,
            vec![Run::new("Typeset LibreOffice Oracle Sample", title)],
        ),
        body(
            "This document is authored twice: once as a minimal docx converted \
             to PDF by LibreOffice, and once through the pdf typeset engine \
             with the bundled Liberation Serif face. The two renderings are \
             rasterized by the same in repo renderer and compared with SSIM.",
        ),
        body(
            "Agreement is advisory rather than exact because line breaking, \
             leading and justification differ slightly between engines. The \
             expected band for this pair is between zero point eight zero and \
             zero point nine zero structural similarity.",
        ),
        body(
            "A large regression such as missing text, wrong font size, broken \
             wrapping or displaced margins drops the score far below the band \
             and is investigated locally before any release.",
        ),
    ];

    let mut engine = engine();
    let pages = engine.layout_flow(
        &blocks,
        &mut FixedPages::new(PageGeom::new(612.0, 792.0, 72.0)),
    );
    assert_eq!(pages.len(), 1, "LO doc fixture must fit one page");
    let result = engine.emit(&pages).expect("emit LO doc fixture");
    assert!(
        result.warnings.is_empty(),
        "LO doc fixture must be warning-free: {:?}",
        result.warnings
    );
    result.pdf
}

// --------------------------------------------------------------------------
// typeset-lo-slide.pdf — mirrors typeset_lo_oracle.py's sample.pptx.
// 10 × 7.5 in slide, two zero-inset text boxes. Keep the text and box rects
// in sync with SLIDE_TITLE/SLIDE_LINES in typeset_lo_oracle.py.
// --------------------------------------------------------------------------
fn lo_slide_fixture() -> Vec<u8> {
    let mut engine = engine();
    let mut ops: Vec<Op> = Vec::new();

    let mut title = style(28.0);
    title.bold = true;
    ops.extend(engine.layout_text_box(&TextBoxSpec::new(
        Rect {
            x0: 72.0,
            y0: 60.0,
            x1: 648.0,
            y1: 140.0,
        },
        vec![Block::Paragraph(
            ParaProps::new(),
            vec![Run::new("Typeset LibreOffice Oracle Slide", title)],
        )],
    )));

    let body: Vec<Block> = [
        "First body line rendered at eighteen points",
        "Second body line for the advisory comparison",
        "Third body line keeps the layout deliberately plain",
        "Fourth body line closes the sample slide",
    ]
    .iter()
    .map(|t| {
        let mut p = ParaProps::new();
        p.space_after = 8.0;
        Block::Paragraph(p, vec![Run::new(*t, style(18.0))])
    })
    .collect();
    ops.extend(engine.layout_text_box(&TextBoxSpec::new(
        Rect {
            x0: 72.0,
            y0: 180.0,
            x1: 648.0,
            y1: 460.0,
        },
        body,
    )));

    let pages = vec![PageOps {
        width: 720.0,
        height: 540.0,
        ops,
    }];
    let result = engine.emit(&pages).expect("emit LO slide fixture");
    assert!(
        result.warnings.is_empty(),
        "LO slide fixture must be warning-free: {:?}",
        result.warnings
    );
    result.pdf
}

fn main() {
    // Resolve fixtures/typeset relative to this crate so the example works
    // from any cwd (CI runs it from the repo root).
    let out_dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/typeset")
        .components()
        .collect();
    std::fs::create_dir_all(&out_dir).expect("create fixtures/typeset");

    for (name, bytes) in [
        ("typeset-flow.pdf", flow_fixture()),
        ("typeset-box.pdf", box_fixture()),
        ("typeset-lo-doc.pdf", lo_doc_fixture()),
        ("typeset-lo-slide.pdf", lo_slide_fixture()),
    ] {
        let path = out_dir.join(name);
        std::fs::write(&path, &bytes).expect("write fixture");
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
}
