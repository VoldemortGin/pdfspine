#![forbid(unsafe_code)]
//! `pdf-markdown` — a deterministic Markdown → PDF renderer.
//!
//! This is an **original pdfspine extension**, not a port of anything in
//! PyMuPDF: a self-contained, deterministic *block-level* layouter that reuses
//! the existing drawing conventions (`pdf-edit` font embedding, `pdf-image`
//! decoding, `pdf-core` object writer). It is deliberately **not** a Story /
//! HTML+CSS typesetting engine — there is no CSS cascade, no HTML DOM, and no
//! inline float model. The same Markdown always produces the same PDF bytes.
//!
//! # Coverage
//!
//! CommonMark + the GFM extensions locked in PRD §9: headings H1–H6,
//! paragraphs, inline **bold** / *italic* / `code` / ~~strikethrough~~ / links
//! (blue text, no annotation), ordered / unordered / nested / task lists
//! (drawn checkboxes), blockquotes (left bar + indent), code blocks (Courier
//! over a light-gray background, newlines preserved), horizontal rules, GFM
//! tables (borders, measured column widths, in-cell wrapping), and images from
//! local paths / `data:` URIs (**no network fetch**; JPEG passes through as
//! DCT, other rasters decode via `pdf-image`).
//!
//! # Fonts (CJK Option A)
//!
//! Defaults are Base-14 (Helvetica / Helvetica-Bold / Courier — zero
//! embedding, WinAnsi). [`Options::font`] swaps the body/heading face for a
//! user TTF; [`Options::cjk_font`] adds a **per-character fallback** for
//! anything the active face cannot encode (e.g. CJK). Unset, unencodable
//! characters degrade to `?` / `.notdef` — never a panic. Embedded programs
//! are parsed once and written once per document (usage-subset Type0).

mod fonts;
mod images;
mod layout;
mod model;
mod render;

use std::path::PathBuf;

use pdf_core::error::{Error, Result};

/// A4 page width in PDF points (210 mm).
const A4_WIDTH_PT: f64 = 595.32;
/// A4 page height in PDF points (297 mm).
const A4_HEIGHT_PT: f64 = 841.92;
/// Default page margin on every side, in points.
const DEFAULT_MARGIN_PT: f64 = 72.0;
/// Default body font size, in points.
const DEFAULT_BODY_SIZE_PT: f64 = 11.0;

/// Rendering options for [`markdown_to_pdf`]. Every default is configurable;
/// [`Options::default`] is A4 · 72 pt margins · 11 pt Helvetica body.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Options {
    /// Page width in points (default A4, 595.32).
    pub page_width: f64,
    /// Page height in points (default A4, 841.92).
    pub page_height: f64,
    /// Top margin in points (default 72).
    pub margin_top: f64,
    /// Right margin in points (default 72).
    pub margin_right: f64,
    /// Bottom margin in points (default 72).
    pub margin_bottom: f64,
    /// Left margin in points (default 72).
    pub margin_left: f64,
    /// Body font size in points (default 11; headings scale from it).
    pub body_font_size: f64,
    /// Optional TTF/OTF program replacing the Base-14 body/heading faces
    /// (embedded once as Type0/Identity-H). Bold/italic render in this same
    /// face (a single program carries one style).
    pub font: Option<Vec<u8>>,
    /// Optional TTF/OTF program used as a **per-character fallback** for
    /// characters the active face cannot encode (e.g. CJK). Embedded once.
    pub cjk_font: Option<Vec<u8>>,
    /// Base directory for resolving *relative* image paths. Unset, only
    /// absolute paths and `data:` URIs are accepted.
    pub base_dir: Option<PathBuf>,
}

impl Options {
    /// The default option set: A4, 72 pt margins, 11 pt body, Base-14 fonts.
    #[must_use]
    pub fn new() -> Self {
        Options {
            page_width: A4_WIDTH_PT,
            page_height: A4_HEIGHT_PT,
            margin_top: DEFAULT_MARGIN_PT,
            margin_right: DEFAULT_MARGIN_PT,
            margin_bottom: DEFAULT_MARGIN_PT,
            margin_left: DEFAULT_MARGIN_PT,
            body_font_size: DEFAULT_BODY_SIZE_PT,
            font: None,
            cjk_font: None,
            base_dir: None,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Options::new()
    }
}

/// Renders `markdown` (CommonMark + GFM tables / strikethrough / task lists)
/// to a PDF and returns the serialized bytes. Deterministic: the same input
/// and options always produce identical bytes (no timestamps, no randomness,
/// no network access).
///
/// # Errors
///
/// [`Error::InvalidArgument`] for unusable geometry options, bad image data or
/// unresolvable image paths; [`Error::Unsupported`] for remote image URLs or
/// unparseable font programs; plus any propagated `pdf-core` write error.
/// Never panics on arbitrary input.
pub fn markdown_to_pdf(markdown: &str, options: &Options) -> Result<Vec<u8>> {
    validate(options)?;
    let fonts = fonts::FontSet::new(options)?;
    let mut blocks = model::parse_blocks(markdown);
    let images = images::resolve_images(&mut blocks, options)?;
    let pages = layout::layout(&blocks, &images, &fonts, options);
    render::build_pdf(&pages, &images, &fonts, options)
}

/// Validates the geometry options (typed errors, no panics downstream).
fn validate(o: &Options) -> Result<()> {
    let all_finite = [
        o.page_width,
        o.page_height,
        o.margin_top,
        o.margin_right,
        o.margin_bottom,
        o.margin_left,
        o.body_font_size,
    ]
    .iter()
    .all(|v| v.is_finite() && *v >= 0.0);
    if !all_finite {
        return Err(Error::InvalidArgument(
            "markdown_to_pdf: page geometry must be finite and non-negative",
        ));
    }
    if o.body_font_size <= 0.0 {
        return Err(Error::InvalidArgument(
            "markdown_to_pdf: body_font_size must be positive",
        ));
    }
    let content_w = o.page_width - o.margin_left - o.margin_right;
    let content_h = o.page_height - o.margin_top - o.margin_bottom;
    if content_w < o.body_font_size * 2.0 || content_h < o.body_font_size * 2.0 {
        return Err(Error::InvalidArgument(
            "markdown_to_pdf: margins leave no usable content area",
        ));
    }
    Ok(())
}
