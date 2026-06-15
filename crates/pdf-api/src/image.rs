//! Image path facade — `Pixmap`, `get_pixmap`, `extract_image`, image-document
//! opening (PRD §3.3 / §8.10). The PyO3 layer calls these free functions; the
//! orphan rule forbids inherent `impl Page` here.

use std::sync::Arc;

use pdf_core::page::Page;
use pdf_core::{DocumentStore, Limits};

use pdf_image::getpixmap;
use pdf_image::imagedoc;

use crate::error::{Error, Result};

// Re-export the value types the bindings need so they depend only on `pdf-api`.
pub use pdf_image::getpixmap::ExtractedImage;
pub use pdf_image::imagedoc::{ImageDocument, ImageFormat};
pub use pdf_image::pixmap::{Colorspace, Pixmap};

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
