//! OCR facade (M8) — `Page.get_textpage_ocr` / `Document.pdfocr_save` /
//! `Document.pdfocr_tobytes`. Thin wrappers over [`pdf_ocr`] that select the OCR
//! engine and flatten the error into the unified `pdf-api` [`Error`]. The PyO3
//! layer calls these free functions (the orphan rule forbids inherent
//! `impl Page` here).
//!
//! # Engine selection
//!
//! The `engine` string picks the backend:
//!
//! - `"tesseract"` (default) — the system Tesseract CLI adapter (PyMuPDF
//!   compatible; `tessdata` applies here).
//! - `"paddle"` — pdfspine's pure-Rust PaddleOCR (PP-OCRv4) engine, stronger on
//!   mixed CJK+Latin text. Available only in the opt-in OCR build (the
//!   `paddle-ocr` feature, shipped as `pip install pdfspine[ocr]`); the lean
//!   base build compiles it out. `tessdata` is irrelevant to it.
//!
//! An unknown engine string, or `"paddle"` in the lean build (feature off),
//! yields the unified [`Error::Unsupported`](crate::error::Error::Unsupported).

use std::sync::Arc;

use pdf_core::page::Page;
use pdf_core::DocumentStore;

use pdf_ocr::{OcrEngine, OcrOptions, TesseractCli};

use crate::error::{Error, Result};
use pdf_text::TextPage;

/// Builds an OCR [`TextPage`] for `page` via the selected `engine` (PyMuPDF
/// `Page.get_textpage_ocr`). `full == false` (image-region-only OCR) currently
/// falls back to full-page OCR (documented deferral). See the module docs for
/// the `engine` values.
///
/// # Errors
///
/// [`Error::Unsupported`](crate::error::Error::Unsupported) when the selected
/// engine is unavailable (Tesseract not installed, an unknown engine string, or
/// `"paddle"` in the lean build without the OCR feature); render / recognition
/// errors propagate.
pub fn page_textpage_ocr(
    page: &Page,
    language: &str,
    dpi: u32,
    full: bool,
    tessdata: Option<&str>,
    engine: &str,
) -> Result<TextPage> {
    let opts = OcrOptions {
        language: language.to_string(),
        dpi,
        full,
    };
    with_engine(engine, tessdata, |eng| {
        Ok(pdf_ocr::textpage_ocr(page, eng, &opts)?)
    })
}

/// Produces a searchable "sandwich" PDF for the whole document (PyMuPDF
/// `Document.pdfocr_tobytes`): each page is rendered, OCR'd via the selected
/// `engine`, and rebuilt with the page image plus an invisible OCR text layer.
/// See the module docs for the `engine` values.
///
/// # Errors
///
/// [`Error::Unsupported`](crate::error::Error::Unsupported) when the selected
/// engine is unavailable; render / recognition / save errors propagate.
pub fn document_pdfocr_bytes(
    doc: &Arc<DocumentStore>,
    language: &str,
    dpi: u32,
    tessdata: Option<&str>,
    engine: &str,
) -> Result<Vec<u8>> {
    let opts = OcrOptions {
        language: language.to_string(),
        dpi,
        full: true,
    };
    with_engine(engine, tessdata, |eng| {
        Ok(pdf_ocr::pdfocr_bytes(doc, eng, &opts)?)
    })
}

/// Resolves `engine` to a concrete [`OcrEngine`] and runs `f` with it.
///
/// `"tesseract"` builds the [`TesseractCli`] adapter (applying `tessdata`);
/// `"paddle"` builds [`pdf_ocr::PaddleOcr`] in the OCR build (the `paddle-ocr`
/// feature; `tessdata` is ignored). Any other value — or `"paddle"` in the lean
/// build — returns [`Error::Unsupported`](crate::error::Error::Unsupported).
fn with_engine<T>(
    engine: &str,
    tessdata: Option<&str>,
    f: impl FnOnce(&dyn OcrEngine) -> Result<T>,
) -> Result<T> {
    match engine {
        "tesseract" => f(&tesseract_engine(tessdata)),
        #[cfg(feature = "paddle-ocr")]
        "paddle" => {
            let eng = pdf_ocr::PaddleOcr::new()?;
            f(&eng)
        }
        #[cfg(not(feature = "paddle-ocr"))]
        "paddle" => Err(Error::Unsupported(
            "OCR engine \"paddle\" is not available in this lean build of \
             pdfspine. Install the OCR build with `pip install pdfspine[ocr]` to \
             use the pure-Rust PaddleOCR engine (or fall back to \
             engine=\"tesseract\")."
                .to_string(),
        )),
        other => Err(Error::Unsupported(format!(
            "unknown OCR engine {other:?}; expected \"tesseract\" or \"paddle\" \
             (\"paddle\" requires the OCR build, `pip install pdfspine[ocr]`)"
        ))),
    }
}

/// Builds the default Tesseract adapter, applying an explicit `tessdata` dir when
/// the caller passes one (PyMuPDF `tessdata` kwarg).
fn tesseract_engine(tessdata: Option<&str>) -> TesseractCli {
    let engine = TesseractCli::new();
    match tessdata {
        Some(dir) if !dir.is_empty() => engine.with_tessdata(dir),
        _ => engine,
    }
}

/// Whether the system Tesseract binary is runnable (used by Python to `skip`
/// OCR tests cleanly when Tesseract is absent).
#[must_use]
pub fn tesseract_available() -> bool {
    TesseractCli::new().is_available()
}
