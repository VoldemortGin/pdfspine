//! TS-4 emission: deterministic bytes, one embed per face per document
//! (4-style lock), deduplicated alpha ExtGStates, group transforms / clips,
//! image XObjects, and degrade-never-panic on malformed op input.

mod common;

use common::*;
use pdf_typeset::{
    Block, ColumnWidth, FaceId, Fill, ImageSpec, Matrix, Op, PageGeom, PageOps, ParaProps, PathSeg,
    Rgb, Run, Stroke, TableCell, TableRow, TableSpec,
};

/// One representative multi-feature document (paragraphs, table, box).
fn fixture() -> Vec<Block> {
    let mut bold = style(12.0);
    bold.bold = true;
    vec![
        Block::Paragraph(
            ParaProps::new(),
            vec![Run::new("Mixed ", style(12.0)), Run::new("weights", bold)],
        ),
        Block::Table(TableSpec::new(
            vec![ColumnWidth::Fixed(80.0), ColumnWidth::Auto],
            vec![TableRow::new(vec![
                TableCell::new(vec![para("cell A", 10.0)]),
                TableCell::new(vec![para("cell B", 10.0)]),
            ])],
        )),
    ]
}

#[test]
fn repeated_runs_are_byte_identical() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let (_, a) = export(&fixture(), geom);
    let (_, b) = export(&fixture(), geom);
    assert_eq!(
        a.pdf, b.pdf,
        "same input + same font environment ⇒ same bytes"
    );
}

#[test]
fn four_style_slots_embed_exactly_four_subset_fontfiles() {
    let geom = PageGeom::new(400.0, 500.0, 50.0);
    let mut runs = Vec::new();
    for (bold, italic, text) in [
        (false, false, "regular "),
        (true, false, "bold "),
        (false, true, "italic "),
        (true, true, "bold-italic"),
    ] {
        let mut s = style(12.0);
        s.bold = bold;
        s.italic = italic;
        runs.push(Run::new(text, s));
    }
    let (_, result) = export(&[Block::Paragraph(ParaProps::new(), runs)], geom);
    let raw = raw(&result.pdf);
    assert_eq!(
        raw.matches("/FontFile2").count(),
        4,
        "regular/bold/italic/bold-italic = 4 embedded faces"
    );
    assert!(
        raw.matches("+LiberationSans").count() > 0,
        "usage-based subsets carry the ABCDEF+ tag"
    );
    assert_eq!(
        tokens(&result.pdf),
        ["regular", "bold", "italic", "bold-italic"]
    );
}

#[test]
fn same_face_embeds_once_across_pages() {
    let geom = PageGeom::new(400.0, 200.0, 50.0);
    let blocks = vec![
        para("page one", 12.0),
        Block::PageBreak,
        para("page two", 12.0),
    ];
    let (_, result) = export(&blocks, geom);
    assert_eq!(raw(&result.pdf).matches("/FontFile2").count(), 1);
}

#[test]
fn alpha_extgstates_deduplicate_across_ops() {
    let square = |x: f64| {
        vec![
            PathSeg::MoveTo { x, y: 50.0 },
            PathSeg::LineTo {
                x: x + 40.0,
                y: 50.0,
            },
            PathSeg::LineTo {
                x: x + 40.0,
                y: 90.0,
            },
            PathSeg::LineTo { x, y: 90.0 },
            PathSeg::Close,
        ]
    };
    let half = |x: f64| Op::Path {
        segs: square(x),
        fill: Some(Fill {
            color: Rgb::new(0.2, 0.4, 0.9),
            alpha: 0.5,
            even_odd: false,
        }),
        stroke: None,
    };
    let quarter = Op::Path {
        segs: square(150.0),
        fill: Some(Fill {
            color: Rgb::new(0.9, 0.2, 0.2),
            alpha: 0.25,
            even_odd: false,
        }),
        stroke: Some(Stroke::new(Rgb::BLACK, 1.0)),
    };
    let page = PageOps {
        width: 300.0,
        height: 300.0,
        ops: vec![half(20.0), half(70.0), quarter],
    };
    let result = ts().emit(&[page]).expect("emit");
    let raw = raw(&result.pdf);
    assert_eq!(
        raw.matches("/ca ").count(),
        2,
        "two distinct alpha pairs ⇒ exactly two ExtGState objects"
    );
    assert!(raw.contains("/GS0 gs") && raw.contains("/GS1 gs"));
    assert!(raw.contains("\nB\n"), "fill+stroke paints with B");
    assert!(ink_pixels(&render(&result.pdf, 0)) > 100);
}

#[test]
fn group_transform_conjugates_with_the_page_flip() {
    let rect = Op::FillRect {
        x: 10.0,
        y: 20.0,
        w: 30.0,
        h: 40.0,
        color: Rgb::BLACK,
    };
    let page = PageOps {
        width: 300.0,
        height: 300.0,
        ops: vec![Op::Group {
            transform: Some(Matrix::translate(10.0, 20.0)),
            clip: None,
            ops: vec![rect],
        }],
    };
    let result = ts().emit(&[page]).expect("emit");
    // Top-left translate (10, 20) = PDF-space translate (10, −20).
    assert!(
        raw(&result.pdf).contains("1 0 0 1 10 -20 cm"),
        "conjugated cm missing:\n{}",
        raw(&result.pdf)
    );
}

#[test]
fn unregistered_face_id_is_skipped_not_panicked() {
    let page = PageOps {
        width: 200.0,
        height: 200.0,
        ops: vec![Op::Text {
            face: FaceId(7),
            size: 12.0,
            color: Rgb::BLACK,
            x: 10.0,
            baseline: 20.0,
            text: "ghost".to_string(),
        }],
    };
    let result = ts().emit(&[page]).expect("emit degrades");
    let raw = raw(&result.pdf);
    assert!(!raw.contains("BT"), "unregistered face draws nothing");
    assert_eq!(open(&result.pdf).page_count(), 1);
}

/// A tiny in-memory PNG built through the repo's own encoder.
fn tiny_png() -> Vec<u8> {
    let pix = pdf_api::Pixmap::new(
        2,
        2,
        pdf_api::Colorspace::Rgb,
        false,
        vec![
            255, 0, 0, 0, 255, 0, //
            0, 0, 255, 255, 255, 0,
        ],
    );
    let mut out = Vec::new();
    pix.save_png(&mut out).expect("png encode");
    out
}

#[test]
fn images_embed_once_per_id_and_flow_blocks_place_them() {
    // Manual placement: one prepared image shown on two pages.
    let mut engine = ts();
    let id = engine
        .add_image(&ImageSpec::new(tiny_png(), 40.0, 30.0))
        .expect("decodable image");
    let show = Op::Image {
        id,
        x: 20.0,
        y: 20.0,
        w: 40.0,
        h: 30.0,
    };
    let page = |op: Op| PageOps {
        width: 200.0,
        height: 200.0,
        ops: vec![op],
    };
    let result = engine
        .emit(&[page(show.clone()), page(show)])
        .expect("emit");
    let raw_str = raw(&result.pdf);
    assert_eq!(
        raw_str.matches("/Subtype /Image").count(),
        1,
        "one XObject for both pages"
    );
    assert_eq!(raw_str.matches("/Im0 Do").count(), 2);
    // Image-only pages take pdf-api's native-resolution fast path: the 2×2
    // fixture rasters to its 4 colored pixels.
    assert!(ink_pixels(&render(&result.pdf, 0)) >= 4);

    // Flow placement: a Block::Image lands at the cursor and rasters.
    let geom = PageGeom::new(300.0, 300.0, 50.0);
    let blocks = vec![
        para("above", 12.0),
        Block::Image(ImageSpec::new(tiny_png(), 60.0, 45.0)),
    ];
    let (pages, flow_result) = export(&blocks, geom);
    assert!(
        pages[0]
            .ops
            .iter()
            .any(|op| matches!(op, Op::Image { w, h, .. } if *w == 60.0 && *h == 45.0)),
        "flow image keeps its display size"
    );
    assert!(flow_result.warnings.is_empty());
}

#[test]
fn undecodable_image_degrades_to_a_warning() {
    let geom = PageGeom::new(300.0, 300.0, 50.0);
    let blocks = vec![Block::Image(ImageSpec::new(vec![0xde, 0xad], 40.0, 40.0))];
    let (pages, result) = export(&blocks, geom);
    assert!(!pages[0].ops.iter().any(|op| matches!(op, Op::Image { .. })));
    assert_eq!(result.warnings.len(), 1);
    assert!(matches!(
        result.warnings[0],
        pdf_typeset::ExportWarning::ImageDropped { .. }
    ));
}
