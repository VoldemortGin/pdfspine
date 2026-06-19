//! Image path facade — `Pixmap`, `get_pixmap`, `extract_image`, image-document
//! opening (PRD §3.3 / §8.10). The PyO3 layer calls these free functions; the
//! orphan rule forbids inherent `impl Page` here.

use std::sync::Arc;

use pdf_core::geom::{IRect, Matrix};
use pdf_core::page::Page;
use pdf_core::{DocumentStore, Limits};

use pdf_image::getpixmap;
use pdf_image::imagedoc;
use pdf_render::{render_page, DisplayList as RenderDisplayList, RenderOptions};

use crate::error::{Error, Result};

// Re-export the value types the bindings need so they depend only on `pdf-api`.
pub use pdf_image::getpixmap::ExtractedImage;
pub use pdf_image::imagedoc::{ImageDocument, ImageFormat, ImageProfile};
pub use pdf_image::pixmap::{Colorspace, Pixmap};

/// The `Page.get_pixmap` request parameters (PyMuPDF, PRD §8.11). `matrix` and
/// `dpi` are alternative scales (dpi wins); `colorspace`/`alpha`/`clip` mirror
/// PyMuPDF. The bindings build this from the Python kwargs.
#[derive(Clone, Debug)]
pub struct RenderArgs {
    /// The render matrix (scale/rotate). Ignored when `dpi` is set.
    pub matrix: Matrix,
    /// Optional DPI → uniform `dpi/72` scale (overrides `matrix`).
    pub dpi: Option<u32>,
    /// The output colorspace (Gray / RGB / CMYK).
    pub colorspace: Colorspace,
    /// Whether the output carries an alpha channel.
    pub alpha: bool,
    /// Optional device-space clip rect `(x0, y0, x1, y1)`.
    pub clip: Option<IRect>,
}

impl Default for RenderArgs {
    fn default() -> Self {
        RenderArgs {
            matrix: Matrix::IDENTITY,
            dpi: None,
            colorspace: Colorspace::Rgb,
            alpha: false,
            clip: None,
        }
    }
}

impl RenderArgs {
    /// The uniform scale this request implies (for the image-only fast path,
    /// which scales the native raster). DPI wins; else the matrix's average
    /// linear magnitude.
    fn scale(&self) -> f64 {
        match self.dpi {
            Some(d) => f64::from(d) / 72.0,
            None => {
                let m = &self.matrix;
                let sx = (m.a * m.a + m.b * m.b).sqrt();
                let sy = (m.c * m.c + m.d * m.d).sqrt();
                ((sx + sy) / 2.0).max(f64::MIN_POSITIVE)
            }
        }
    }

    fn to_options(&self) -> RenderOptions {
        RenderOptions {
            matrix: self.matrix,
            dpi: self.dpi,
            colorspace: self.colorspace,
            alpha: self.alpha,
            clip: self.clip,
        }
    }
}

/// Renders `page` to a [`Pixmap`] for the in-scope image path (PRD §3.3).
///
/// `scale` multiplies the native image resolution (`1.0` = native; `dpi/72` for
/// a DPI request). `alpha` adds an alpha channel (from the image `/SMask`, else
/// fully opaque).
///
/// # Errors
///
/// - [`Error::Unsupported`] for a vector/text page (deferred to M6).
/// - A typed [`Error::Decode`] for an image-only page whose image fails to
///   decode — `get_text` on the same page is unaffected (it never enters here).
pub fn page_get_pixmap(page: &Page, scale: f64, alpha: bool) -> Result<Pixmap> {
    let doc = page.document();
    let dict = page
        .dict()
        .ok_or_else(|| Error::Syntax("page has no dictionary".to_string()))?;
    Ok(getpixmap::page_pixmap(doc, &dict, scale, alpha)?)
}

/// Renders `page` to a [`Pixmap`] under `args` — the full PyMuPDF `get_pixmap`
/// (PRD §8.11). Any page type renders: an image-only page takes the fast image
/// decode path (native raster × scale); every other page (vector / text / mixed)
/// is rasterized via [`pdf_render::render_page`] onto a CropBox-sized canvas.
///
/// # Errors
///
/// [`Error::Limit`] for an over-large target; propagated decode / parse errors.
pub fn page_render(page: &Page, args: &RenderArgs) -> Result<Pixmap> {
    let doc = page.document();
    // Image-only fast path: decode the page's image at native resolution × scale
    // (the scanned-document optimization). Only used when no clip / Gray-CMYK
    // conversion is requested, so the fast path's RGB raster matches the request.
    if args.clip.is_none() && args.colorspace == Colorspace::Rgb && page_is_image_only(page) {
        if let Some(dict) = page.dict() {
            if let Ok(pix) = getpixmap::page_pixmap(doc, &dict, args.scale(), args.alpha) {
                return Ok(pix);
            }
        }
        // Fall through to the full renderer on any image-only decode failure.
    }
    Ok(render_page(doc, page, &args.to_options())?)
}

/// A recorded, replayable page render — the PyMuPDF `DisplayList` (PRD §8.11).
/// Wraps [`pdf_render::DisplayList`] so the bindings depend only on `pdf-api`.
pub struct DisplayList {
    inner: RenderDisplayList,
    doc: Arc<DocumentStore>,
}

impl DisplayList {
    /// The number of recorded drawcalls (diagnostic).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the list recorded no drawcalls.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// The display list's source rect (the page CropBox), `(x0, y0, x1, y1)`.
    #[must_use]
    pub fn rect(&self) -> (f64, f64, f64, f64) {
        let r = self.inner.rect();
        (r.x0, r.y0, r.x1, r.y1)
    }

    /// Replays the recorded drawcalls into a [`Pixmap`] under `args` (PyMuPDF
    /// `DisplayList.get_pixmap`).
    ///
    /// # Errors
    ///
    /// Propagates render / decode errors.
    pub fn get_pixmap(&self, args: &RenderArgs) -> Result<Pixmap> {
        Ok(self.inner.get_pixmap(&self.doc, &args.to_options())?)
    }
}

/// Records `page`'s ordered drawcall stream into a [`DisplayList`] (PyMuPDF
/// `Page.get_displaylist`).
#[must_use]
pub fn page_get_displaylist(page: &Page) -> DisplayList {
    let doc = page.document().clone();
    let inner = RenderDisplayList::from_page(&doc, page);
    DisplayList { inner, doc }
}

/// Whether `page` is an image-only page (in scope for `get_pixmap`, PRD §3.3).
#[must_use]
pub fn page_is_image_only(page: &Page) -> bool {
    let doc = page.document();
    let Some(dict) = page.dict() else {
        return false;
    };
    matches!(
        getpixmap::classify_page(doc, &dict),
        getpixmap::PageClass::ImageOnly { .. }
    )
}

/// Extracts the image XObject at `xref` from `doc` (PyMuPDF
/// `Document.extract_image`).
///
/// # Errors
///
/// [`Error::Unsupported`] when `xref` is not an image XObject; decode errors
/// propagate.
pub fn document_extract_image(doc: &Arc<DocumentStore>, xref: u32) -> Result<ExtractedImage> {
    Ok(getpixmap::extract_image(doc, xref)?)
}

/// Opens raster `bytes` as an image document (PRD §8.10), sniffing the format.
///
/// # Errors
///
/// [`Error::Unsupported`] for non-image / corrupt input.
pub fn open_image_document(bytes: &[u8]) -> Result<ImageDocument> {
    Ok(imagedoc::open_image_document(bytes, None)?)
}

/// Converts an image input to PDF bytes (PRD §8.10, image inputs only).
///
/// # Errors
///
/// [`Error::Unsupported`] for non-image input (PyMuPDF `PdfUnsupportedError`).
pub fn image_to_pdf(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(imagedoc::convert_to_pdf(bytes, None)?)
}

/// The header profile of a raster image (PyMuPDF `image_profile` /
/// `Tools.image_profile`), or `None` for empty / unrecognized input.
#[must_use]
pub fn image_profile(bytes: &[u8]) -> Option<ImageProfile> {
    imagedoc::image_profile(bytes)
}

/// A [`Pixmap`] for page `index` of an already-opened image document, by
/// reopening the source bytes (the bindings keep the source for the image-doc
/// handle). Returns the decoded raster directly.
///
/// # Errors
///
/// [`Error::Unsupported`] for a bad format or out-of-range page.
pub fn image_document_page_pixmap(bytes: &[u8], index: usize) -> Result<Pixmap> {
    let doc = imagedoc::open_image_document(bytes, None)?;
    doc.pages
        .get(index)
        .cloned()
        .ok_or_else(|| Error::Unsupported(format!("image page index {index} out of range")))
}

/// The `Limits` used when reopening untrusted image inputs (hard-safe default).
#[must_use]
pub fn default_limits() -> Limits {
    Limits::default()
}

// --- fallible `Pixmap` wrappers (return `pdf_api::Result`) ----------------
//
// `Pixmap`'s own methods return `pdf_image::Result`; the bindings depend only on
// `pdf-api` (PRD §9.1), so these wrappers re-surface them with the unified error.

/// A blank [`Pixmap`] (PyMuPDF `Pixmap(cs, irect)`).
///
/// # Errors
///
/// [`Error::Unsupported`] / [`Error::Limit`] on a bad geometry.
pub fn pixmap_blank(
    width: u32,
    height: u32,
    colorspace: Colorspace,
    alpha: bool,
    fill: u8,
) -> Result<Pixmap> {
    Ok(Pixmap::blank(width, height, colorspace, alpha, fill)?)
}

/// Encodes a [`Pixmap`] in `format` (PyMuPDF `Pixmap.tobytes`).
///
/// # Errors
///
/// [`Error::Unsupported`] for an unknown format; encode errors propagate.
pub fn pixmap_tobytes(pix: &Pixmap, format: &str) -> Result<Vec<u8>> {
    Ok(pix.tobytes(format)?)
}

/// Writes a pixel into a [`Pixmap`] (PyMuPDF `Pixmap.set_pixel`).
///
/// # Errors
///
/// [`Error::Unsupported`] for an out-of-range coordinate or wrong arity.
pub fn pixmap_set_pixel(pix: &mut Pixmap, x: u32, y: u32, value: &[u8]) -> Result<()> {
    Ok(pix.set_pixel(x, y, value)?)
}
