#![forbid(unsafe_code)]
//! `pdf-ocr` — pluggable OCR for oxide-pdf (M8, PRD §3.2 #3 post-v1).
//!
//! Two surfaces, both PyMuPDF-compatible:
//!
//! - [`textpage_ocr`] — `Page.get_textpage_ocr`: rasterize a page, recognize it,
//!   and return a [`pdf_text::TextPage`] so `get_text` / `search_for` work on the
//!   OCR result.
//! - [`pdfocr_bytes`] — `Document.pdfocr_save` / `pdfocr_tobytes`: produce a
//!   searchable **sandwich** PDF (the page image with an invisible render-mode-3
//!   OCR text layer over it).
//!
//! # Engine-agnostic
//!
//! There is no formal OCR API standard, so the design is a small pluggable
//! [`OcrEngine`] trait with [`TesseractCli`] as the default adapter. Tesseract is
//! **not bundled** — exactly like PyMuPDF, the user must have the system
//! `tesseract` installed; this keeps the wheel pure-Rust. A cloud or in-process
//! engine can be added later by implementing [`OcrEngine`] alone.
//!
//! # `#![forbid(unsafe_code)]` / no panics
//!
//! All first-party code is safe and panic-free: a missing engine, a failed
//! recognition, or arbitrary input yields a typed [`Error`] (a missing engine is
//! `kind == "unsupported"`, mapping to `PdfUnsupportedError`).

pub mod engine;
pub mod error;
pub mod integration;
pub mod tesseract;

pub use engine::{OcrEngine, OcrWord};
pub use error::{Error, Result};
pub use integration::{pdfocr_bytes, textpage_ocr, OcrOptions};
pub use tesseract::TesseractCli;
