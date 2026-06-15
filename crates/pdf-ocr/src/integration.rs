//! OCR integration: render a page, recognize, and build either an OCR
//! [`TextPage`] (`get_textpage_ocr`) or a searchable "sandwich" PDF
//! (`pdfocr_save` / `pdfocr_tobytes`).
//!
//! # Coordinate mapping
//!
//! A page is rendered at `dpi` via [`pdf_render::render_page`], whose device
//! transform is `page_transform(cropbox, rotate) · scale(s)` with `s = dpi/72`.
//! `page_transform` already lands in PyMuPDF **device space** (origin top-left,
//! y down, `/Rotate` applied), so a pixel `(px, py)` maps back to a page point
//! simply by dividing out the scale: `(px/s, py/s)`. That page-point space is
//! exactly the [`TextPage`] / `insert_text` convention, so no extra y-flip is
//! needed — `pdf_text` and `pdf_edit` perform the final flip into PDF user space
//! themselves.

use std::sync::Arc;

use pdf_core::geom::{Point, Rect};
use pdf_core::page::Page;
use pdf_core::{DocumentStore, Limits, SaveOptions};

use pdf_edit::content::PageContent;
use pdf_edit::merge::{insert_pdf, InsertOptions};
use pdf_image::imagedoc;
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_render::{render_page, RenderOptions};
use pdf_text::model::{Block, BlockKind, Char, Line, Span, TextPage};

use crate::engine::{OcrEngine, OcrWord};
use crate::error::{Error, Result};

/// Options shared by the OCR entry points (PyMuPDF defaults).
#[derive(Clone, Debug)]
pub struct OcrOptions {
    /// The engine language code(s), e.g. `"eng"` (Tesseract default).
    pub language: String,
    /// The render DPI fed to the rasterizer + the OCR engine. PyMuPDF defaults
    /// to 72 for `get_textpage_ocr`; the sandwich path benefits from a higher
    /// value (recognition quality), so callers pass it explicitly.
    pub dpi: u32,
    /// Whether to OCR the whole page (`true`) or only existing image regions
    /// (`false`). Only `full == true` is implemented; `false` currently falls
    /// back to full-page OCR (documented deferral).
    pub full: bool,
}

impl Default for OcrOptions {
    fn default() -> Self {
        OcrOptions {
            language: "eng".to_string(),
            dpi: 72,
            full: true,
        }
    }
}

/// Renders `page` at `dpi` to an RGB [`Pixmap`] for OCR input.
fn render_for_ocr(page: &Page, dpi: u32) -> Result<Pixmap> {
    if dpi == 0 {
        return Err(Error::InvalidArgument("dpi must be positive"));
    }
    let doc = page.document();
    let opts = RenderOptions {
        dpi: Some(dpi),
        colorspace: Colorspace::Rgb,
        alpha: false,
        ..RenderOptions::default()
    };
    Ok(render_page(doc, page, &opts)?)
}

/// Builds an OCR [`TextPage`] for `page` (PyMuPDF `Page.get_textpage_ocr`).
///
/// The page is rasterized at `opts.dpi`, recognized by `engine`, and each word's
/// pixel box is mapped to page-point device space (`/s`) to populate one text
/// [`Block`] (one [`Line`] per OCR word, one [`Span`] per line). `get_text` /
/// `search_for` over the returned page then work unchanged.
///
/// # Errors
///
/// Propagates render / engine errors (a missing engine is `kind == "unsupported"`).
pub fn textpage_ocr(page: &Page, engine: &dyn OcrEngine, opts: &OcrOptions) -> Result<TextPage> {
    let pix = render_for_ocr(page, opts.dpi)?;
    let words = engine.recognize(&pix, &opts.language, opts.dpi as f32)?;
    let s = f64::from(opts.dpi) / 72.0;
    let (pw, ph) = pdf_text::page_size(page.cropbox(), page.rotation());
    Ok(build_textpage(&words, s, pw, ph))
}

/// Maps OCR words (pixel space) to a [`TextPage`] in page-point device space.
/// `s = dpi/72`; `(width, height)` are the displayed page size in points.
fn build_textpage(words: &[OcrWord], s: f64, width: f64, height: f64) -> TextPage {
    let mut lines = Vec::with_capacity(words.len());
    let mut page_bbox = Rect::new(width, height, 0.0, 0.0); // grows to a union
    let mut any = false;

    for (i, w) in words.iter().enumerate() {
        let bbox =
            Rect::new(w.bbox.x0 / s, w.bbox.y0 / s, w.bbox.x1 / s, w.bbox.y1 / s).normalize();
        let size = (bbox.height()).max(1.0);
        // The text baseline origin: bottom-left of the cell (PyMuPDF span origin
        // is y-down, baseline ≈ bbox bottom minus the descender; we use the box
        // bottom, which is correct enough for extraction + search).
        let origin = Point::new(bbox.x0, bbox.y1);
        let chars = char_cells(&w.text, &bbox);
        let span = Span {
            bbox,
            font: "GlyphlessFont".into(),
            size,
            flags: 0,
            color: 0,
            ascender: 0.8,
            descender: -0.2,
            origin,
            chars,
            text: w.text.clone(),
        };
        lines.push(Line {
            bbox,
            wmode: 0,
            dir: (1.0, 0.0),
            spans: vec![span],
            seq: i,
        });
        page_bbox = if any { page_bbox | bbox } else { bbox };
        any = true;
    }

    let block_bbox = if any {
        page_bbox
    } else {
        Rect::new(0.0, 0.0, 0.0, 0.0)
    };
    let block = Block {
        bbox: block_bbox,
        kind: BlockKind::Text,
        lines,
        image: None,
        number: 0,
        seq: 0,
    };

    TextPage {
        width,
        height,
        blocks: if any { vec![block] } else { Vec::new() },
    }
}

/// Distributes per-character cells evenly across the word box (OCR gives no
/// per-glyph boxes; even spacing is the fitz-compatible `rawdict` approximation).
fn char_cells(text: &str, bbox: &Rect) -> Vec<Char> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len().max(1) as f64;
    let cell_w = bbox.width() / n;
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let x0 = bbox.x0 + cell_w * i as f64;
            let x1 = x0 + cell_w;
            Char {
                origin: Point::new(x0, bbox.y1),
                bbox: Rect::new(x0, bbox.y0, x1, bbox.y1),
                c,
            }
        })
        .collect()
}

/// Produces a searchable **sandwich PDF**: every page of `doc` is rendered to an
/// image, OCR'd, and rebuilt as a new page that draws the page image with an
/// invisible (render-mode-3) OCR text layer on top. The result is selectable /
/// searchable while looking identical to the rendered original (PyMuPDF
/// `Document.pdfocr_tobytes`).
///
/// # Errors
///
/// Propagates render / engine / save errors. A missing engine is `unsupported`.
pub fn pdfocr_bytes(
    doc: &Arc<DocumentStore>,
    engine: &dyn OcrEngine,
    opts: &OcrOptions,
) -> Result<Vec<u8>> {
    let page_refs = pdf_core::pagetree::page_refs(doc);
    if page_refs.is_empty() {
        return Err(Error::InvalidArgument("document has no pages"));
    }
    // The output document, assembled page by page.
    let out = DocumentStore::from_bytes(empty_pdf(), Limits::default())?;

    for (idx, &leaf) in page_refs.iter().enumerate() {
        let page = Page::new(Arc::clone(doc), idx, leaf);
        let single = sandwich_page(&page, engine, opts)?;
        let src = DocumentStore::from_bytes(single, Limits::default())?;
        insert_pdf(&out, &src, &InsertOptions::default())?;
    }

    out.save_to_vec(&SaveOptions::default().with_garbage(1))
        .map_err(Error::from)
}

/// Builds a one-page sandwich PDF for a single page: image layer + invisible
/// OCR text layer.
fn sandwich_page(page: &Page, engine: &dyn OcrEngine, opts: &OcrOptions) -> Result<Vec<u8>> {
    let pix = render_for_ocr(page, opts.dpi)?;
    let words = engine.recognize(&pix, &opts.language, opts.dpi as f32)?;
    let s = f64::from(opts.dpi) / 72.0;

    // Image layer: the rendered pixmap as a one-page PDF whose MediaBox is
    // `px / dpi * 72` points — i.e. the page size in points (1pt = 1px / s).
    let png = pix.to_png_bytes()?;
    let img_pdf = imagedoc::convert_to_pdf(&png, None)?;
    let single = DocumentStore::from_bytes(img_pdf, Limits::default())?;

    // Invisible text layer on page 0 of the freshly-built single-page doc.
    let pc = PageContent::new(&single, 0)?;
    let font_name = pc.add_resource("Font", "F", glyphless_font())?;
    let chunk = invisible_text_chunk(&pc, &font_name, &words, s);
    if !chunk.is_empty() {
        pc.append_content(&chunk)?;
    }

    single
        .save_to_vec(&SaveOptions::default().with_garbage(1))
        .map_err(Error::from)
}

/// Emits one `BT … ET` chunk drawing every OCR word in **render mode 3**
/// (invisible). Each word is positioned at its page-point top-left and scaled so
/// the (single-line) text roughly fills the word box width — extraction / search
/// only needs the text present and positioned, not pixel-perfect glyph metrics.
fn invisible_text_chunk(pc: &PageContent, font_name: &str, words: &[OcrWord], s: f64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"BT\n");
    out.extend_from_slice(b"3 Tr\n"); // invisible text render mode (Tr 3)
    for w in words {
        if w.text.trim().is_empty() {
            continue;
        }
        let bbox =
            Rect::new(w.bbox.x0 / s, w.bbox.y0 / s, w.bbox.x1 / s, w.bbox.y1 / s).normalize();
        let size = bbox.height().max(1.0);
        // Baseline origin in PDF user space: top-left page point -> bottom-left
        // user-space, then drop to the box bottom for the baseline.
        let origin = pc.to_user_space(Point::new(bbox.x0, bbox.y1));
        // Horizontal scale (Tz, percent) so the rendered glyph run ≈ box width.
        // Approximate the glyphless advance as `0.6 em` per char.
        let n = w.text.chars().count().max(1) as f64;
        let nominal = 0.6 * size * n;
        let tz = if nominal > 0.0 {
            (bbox.width() / nominal * 100.0).clamp(1.0, 1000.0)
        } else {
            100.0
        };
        out.extend_from_slice(format!("/{} {} Tf\n", font_name, fmt_num(size)).as_bytes());
        out.extend_from_slice(format!("{} Tz\n", fmt_num(tz)).as_bytes());
        out.extend_from_slice(
            format!("1 0 0 1 {} {} Tm\n", fmt_num(origin.x), fmt_num(origin.y)).as_bytes(),
        );
        let mut operand = vec![b'('];
        operand.extend_from_slice(&escape_pdf_literal(&winansi_bytes(&w.text)));
        operand.push(b')');
        out.extend_from_slice(&operand);
        out.extend_from_slice(b" Tj\n");
    }
    out.extend_from_slice(b"ET\n");
    out
}

/// A standard Helvetica Base-14 font object for the invisible text layer. The
/// glyphs are never painted (Tr 3), so the visible shapes are irrelevant — only
/// the WinAnsi character codes matter, so `get_text` recovers the words.
fn glyphless_font() -> pdf_core::object::Object {
    use pdf_core::object::{Name, Object};
    let mut d = pdf_core::object::Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("Font")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new("Type1")));
    d.insert(Name::new("BaseFont"), Object::Name(Name::new("Helvetica")));
    d.insert(
        Name::new("Encoding"),
        Object::Name(Name::new("WinAnsiEncoding")),
    );
    Object::Dictionary(d)
}

/// Encodes a string to WinAnsi bytes for a literal-string operand (ASCII passes
/// through; non-mappable characters become `?`). Mirrors the conservative
/// behavior of `pdf_edit`'s text path for the Base-14 route.
fn winansi_bytes(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if u < 0x100 {
                u as u8
            } else {
                b'?'
            }
        })
        .collect()
}

/// Formats a coordinate / scalar for a content operator (no trailing zeros).
/// Inlined from `pdf_edit::content` (which keeps it `pub(crate)`).
fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let mut s = format!("{v:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// Escapes bytes for a PDF literal-string `( … )` operand. Inlined from
/// `pdf_edit::content` (`pub(crate)` there).
fn escape_pdf_literal(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    for &b in bytes {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'(' => out.extend_from_slice(b"\\("),
            b')' => out.extend_from_slice(b"\\)"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            _ => out.push(b),
        }
    }
    out
}

/// A minimal empty single-document PDF used as the sandwich assembly target.
fn empty_pdf() -> Vec<u8> {
    b"%PDF-1.7\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[]/Count 0>>endobj\n\
trailer<</Root 1 0 R/Size 3>>\n\
%%EOF\n"
        .to_vec()
}
