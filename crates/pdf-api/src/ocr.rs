//! OCR facade (M8) — `Page.get_textpage_ocr` / `Document.pdfocr_save` /
//! `Document.pdfocr_tobytes`. Thin wrappers over [`pdf_ocr`] that pin the
//! Tesseract default engine and flatten the error into the unified `pdf-api`
//! [`Error`]. The PyO3 layer calls these free functions (the orphan rule forbids
//! inherent `impl Page` here).

use std::sync::Arc;

use pdf_core::page::Page;
use pdf_core::DocumentStore;

use pdf_ocr::{OcrOptions, TesseractCli};

use crate::error::Result;
use pdf_text::TextPage;

/// Builds an OCR [`TextPage`] for `page` via the system Tesseract (PyMuPDF
/// `Page.get_textpage_ocr`). `full == false` (image-region-only OCR) currently
/// falls back to full-page OCR (documented deferral).
///
/// # Errors
///
/// [`Error::Unsupported`](crate::error::Error::Unsupported) when Tesseract is
/// unavailable; render / recognition errors propagate.
pub fn page_textpage_ocr(
    page: &Page,
    language: &str,
    dpi: u32,
    full: bool,
    tessdata: Option<&str>,
) -> Result<TextPage> {
    let engine = engine_with(tessdata);
    let opts = OcrOptions {
        language: language.to_string(),
        dpi,
        full,
    };
    Ok(pdf_ocr::textpage_ocr(page, &engine, &opts)?)
}

/// Builds the default Tesseract adapter, applying an explicit `tessdata` dir when
/// the caller passes one (PyMuPDF `tessdata` kwarg).
fn engine_with(tessdata: Option<&str>) -> TesseractCli {
    let engine = TesseractCli::new();
    match tessdata {
        Some(dir) if !dir.is_empty() => engine.with_tessdata(dir),
        _ => engine,
    }
}

/// Produces a searchable "sandwich" PDF for the whole document (PyMuPDF
/// `Document.pdfocr_tobytes`): each page is rendered, OCR'd, and rebuilt with the
/// page image plus an invisible OCR text layer.
///
/// # Errors
///
/// [`Error::Unsupported`](crate::error::Error::Unsupported) when Tesseract is
/// unavailable; render / recognition / save errors propagate.
pub fn document_pdfocr_bytes(
    doc: &Arc<DocumentStore>,
    language: &str,
    dpi: u32,
    tessdata: Option<&str>,
) -> Result<Vec<u8>> {
    let engine = engine_with(tessdata);
    let opts = OcrOptions {
        language: language.to_string(),
        dpi,
        full: true,
    };
    Ok(pdf_ocr::pdfocr_bytes(doc, &engine, &opts)?)
}

/// Whether the system Tesseract binary is runnable (used by Python to `skip`
/// OCR tests cleanly when Tesseract is absent).
#[must_use]
pub fn tesseract_available() -> bool {
    TesseractCli::new().is_available()
}
