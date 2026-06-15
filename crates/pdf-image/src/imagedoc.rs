//! Image-document support — PRD §8.10.
//!
//! Opens a raster image (PNG/JPEG/TIFF-multi-IFD/GIF/BMP/WEBP) as a one-page-
//! per-image document, and converts image inputs to PDF (`convert_to_pdf`).
//! Implemented in the M5-imagedoc unit; this is the compiling stub.

use crate::error::{Error, Result};
use crate::pixmap::Pixmap;

/// The raster container formats supported as image documents (PRD §8.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageFormat {
    /// PNG (all depths/palette/alpha/interlace).
    Png,
    /// JPEG (baseline + progressive; header read without full decode for
    /// `convert_to_pdf` passthrough).
    Jpeg,
    /// TIFF (multi-page — one page per IFD).
    Tiff,
    /// GIF.
    Gif,
    /// BMP.
    Bmp,
    /// WEBP.
    Webp,
}

impl ImageFormat {
    /// Best-effort format sniff from the leading magic bytes, or `None` if the
    /// signature is unrecognized.
    ///
    /// Stub: always returns `None` (the M5-imagedoc implementer fills the magic-
    /// byte table). Panic-free.
    #[must_use]
    pub fn sniff(_bytes: &[u8]) -> Option<ImageFormat> {
        None
    }
}

/// An opened image document: the decoded pages plus their source format.
///
/// One [`Pixmap`] per page (one IFD per page for multi-page TIFF; exactly one
/// page for the single-frame formats). The M5-imagedoc implementer adds the
/// `Document`/`Page`-trait surface (`page_count`, `MediaBox` from pixel size ×
/// DPI, the `q w 0 0 h 0 0 cm /Img Do Q` content) per PRD §8.10; the scaffold
/// pins only the data the loader produces.
#[derive(Clone, Debug)]
pub struct ImageDocument {
    /// The source container format.
    pub format: ImageFormat,
    /// One decoded raster per page.
    pub pages: Vec<Pixmap>,
}

impl ImageDocument {
    /// Number of pages (1 for single-frame formats; IFD count for multi-TIFF).
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Opens raster `bytes` of the given `format` as an [`ImageDocument`].
///
/// If `format` is `None` the implementer sniffs it via [`ImageFormat::sniff`].
///
/// Stub: returns [`Error::Unsupported`] (`"open_image_document"`) — panic-free.
pub fn open_image_document(_bytes: &[u8], _format: Option<ImageFormat>) -> Result<ImageDocument> {
    Err(Error::Unsupported("open_image_document"))
}

/// Converts an image input to a single-/multi-page PDF and returns the PDF
/// bytes (PRD §8.10, image inputs only).
///
/// Per PRD §8.10: JPEG → `/DCTDecode` passthrough (lossless); PNG/TIFF → decode
/// → Flate; alpha → `/SMask`; palette → `/Indexed`; CMYK → `/DeviceCMYK` + Adobe
/// `/Decode` inversion; 16-bit → BPC 16; honor EXIF/TIFF orientation. A
/// **non-image** input must yield [`Error::InvalidArgument`] (surfaced to Python
/// as `PdfUnsupportedError`), never a panic.
///
/// Stub: returns [`Error::Unsupported`] (`"convert_to_pdf"`) — panic-free.
pub fn convert_to_pdf(_bytes: &[u8], _format: Option<ImageFormat>) -> Result<Vec<u8>> {
    Err(Error::Unsupported("convert_to_pdf"))
}
