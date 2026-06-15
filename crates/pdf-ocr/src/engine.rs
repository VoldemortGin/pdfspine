//! The pluggable OCR engine seam (PRD §3.2 #3, post-v1 design).
//!
//! There is no formal OCR API standard, so oxide-pdf follows the agreed pattern:
//! a small engine trait ([`OcrEngine`]) that any backend can implement, with
//! [`crate::tesseract::TesseractCli`] as the default adapter. A cloud OCR
//! service or an in-process engine can be dropped in later by implementing this
//! one trait — nothing else in the OCR pipeline depends on Tesseract directly.

use pdf_core::geom::Rect;

use crate::error::Result;
use pdf_image::pixmap::Pixmap;

/// One recognized word, in **image pixel coordinates** (origin top-left, y down,
/// matching the rendered [`Pixmap`]). The pipeline maps this into PDF page space
/// (`pdf_text` device/page transform) when building a [`pdf_text::TextPage`] or
/// the invisible sandwich text layer.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrWord {
    /// The recognized text (a single whitespace-delimited token).
    pub text: String,
    /// The word's bounding box in image pixel coordinates (`x0,y0` top-left,
    /// `x1,y1` bottom-right).
    pub bbox: Rect,
    /// The engine's confidence in `[0.0, 100.0]` (Tesseract scale). Words the
    /// engine reports with a negative confidence (its "no word" rows) are
    /// dropped by the adapter and never surface here.
    pub confidence: f32,
}

/// A pluggable OCR backend. Implementors recognize text in a rasterized page or
/// image region and return per-word boxes in pixel space.
///
/// The trait is intentionally tiny — one method — so a non-Tesseract engine
/// (e.g. a cloud API) can be wired in without touching the rest of the pipeline.
pub trait OcrEngine {
    /// Recognizes the words in `image`.
    ///
    /// - `lang` is an engine language code (Tesseract: `"eng"`, `"deu"`, …;
    ///   multiple may be joined with `+`).
    /// - `dpi` is the resolution `image` was rendered at, passed through to the
    ///   engine so it can size text correctly. It does **not** rescale the
    ///   returned boxes — those are always in `image` pixel coordinates.
    ///
    /// # Errors
    ///
    /// A typed [`crate::error::Error`] when the engine is unavailable
    /// (`kind == "unsupported"`) or fails; implementors must never panic.
    fn recognize(&self, image: &Pixmap, lang: &str, dpi: f32) -> Result<Vec<OcrWord>>;
}
