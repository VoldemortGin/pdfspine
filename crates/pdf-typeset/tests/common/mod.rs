//! Shared TS-4/TS-5 test helpers: deterministic engine construction (bundled
//! faces only — no system-font dependence), fixture builders, and read-back
//! through the repo's own parsing / rendering stack (`pdf-api`).

#![allow(dead_code)]

use pdf_typeset::{
    Block, FixedPages, FontResolver, PageGeom, PageOps, ParaProps, Platform, Run, RunStyle,
    Typesetter,
};

/// The deterministic test family (always bundled).
pub const FAMILY: &str = "Liberation Sans";

/// A fully deterministic engine: bundled Liberation/Noto faces only.
pub fn ts() -> Typesetter {
    Typesetter::new(FontResolver::with_platform(Platform::MacOs))
}

/// A plain black `Liberation Sans` style at `size` pt.
pub fn style(size: f64) -> RunStyle {
    RunStyle::new(FAMILY, size)
}

/// A default-props paragraph of one plain run.
pub fn para(text: &str, size: f64) -> Block {
    Block::Paragraph(ParaProps::new(), vec![Run::new(text, style(size))])
}

/// Lays `blocks` out on fixed `geom` pages and emits, returning the laid-out
/// op pages plus the finished PDF bytes and warnings.
pub fn export(blocks: &[Block], geom: PageGeom) -> (Vec<PageOps>, pdf_typeset::ExportResult) {
    let mut engine = ts();
    let pages = engine.layout_flow(blocks, &mut FixedPages::new(geom));
    let result = engine.emit(&pages).expect("emit should succeed");
    (pages, result)
}

/// Opens emitted bytes through the public facade.
pub fn open(bytes: &[u8]) -> pdf_api::Document {
    pdf_api::Document::open_bytes(bytes.to_vec()).expect("emitted PDF should reopen")
}

/// `get_text("words")` of one page: `(x0, y0, x1, y1, text, block, line, word)`
/// in top-left page coordinates.
pub fn words(bytes: &[u8], page: usize) -> Vec<pdf_api::WordTuple> {
    let doc = open(bytes);
    let page = doc.load_page(page).expect("page should load");
    match pdf_api::get_text(&page, "words", None, None) {
        pdf_api::TextOutput::Words(w) => w,
        other => panic!("expected words output, got {other:?}"),
    }
}

/// Plain-text extraction of the whole document, in page order.
pub fn full_text(bytes: &[u8]) -> String {
    let doc = open(bytes);
    let mut out = String::new();
    for i in 0..doc.page_count() {
        let page = doc.load_page(i).expect("page should load");
        if let pdf_api::TextOutput::Text(s) = pdf_api::get_text(&page, "text", None, None) {
            out.push_str(&s);
        }
    }
    out
}

/// Whitespace-split tokens of the whole document (read-back scoring).
pub fn tokens(bytes: &[u8]) -> Vec<String> {
    full_text(bytes)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// The raw file bytes as a lossy string — content streams are written without
/// deflation, so operator-level assertions can grep this directly.
pub fn raw(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Renders one page to a pixmap through the repo's own rasterizer.
pub fn render(bytes: &[u8], page: usize) -> pdf_api::Pixmap {
    let doc = open(bytes);
    let page = doc.load_page(page).expect("page should load");
    pdf_api::page_render(&page, &pdf_api::RenderArgs::default()).expect("render should succeed")
}

/// The number of non-white pixels of a rendered page (blankness checks).
pub fn ink_pixels(pix: &pdf_api::Pixmap) -> usize {
    let n = usize::from(pix.colorspace.components()) + usize::from(pix.alpha);
    pix.samples()
        .chunks(n)
        .filter(|px| px.iter().take(3).any(|&c| c < 240))
        .count()
}

/// Liberation Sans vertical metrics as em fractions:
/// `(ascent, descent_magnitude, line_gap)` — read from the bundled program so
/// baseline expectations carry no magic numbers.
pub fn liberation_metrics() -> (f64, f64, f64) {
    let bytes = pdf_fonts::liberation::liberation_face(
        pdf_fonts::liberation::LiberationFamily::Sans,
        false,
        false,
    );
    let face = ttf_parser::Face::parse(bytes, 0).expect("bundled face parses");
    let upem = f64::from(face.units_per_em());
    (
        f64::from(face.ascender()) / upem,
        f64::from(face.descender()).abs() / upem,
        f64::from(face.line_gap()).max(0.0) / upem,
    )
}

/// The natural single-spaced Liberation Sans line height at `size` pt.
pub fn natural_line_height(size: f64) -> f64 {
    let (asc, desc, gap) = liberation_metrics();
    (asc + desc + gap) * size
}

/// Asserts `|a - b| <= tol` with a readable failure.
#[track_caller]
pub fn assert_near(a: f64, b: f64, tol: f64, what: &str) {
    assert!(
        (a - b).abs() <= tol,
        "{what}: {a} vs {b} (|Δ| = {} > {tol})",
        (a - b).abs()
    );
}
