//! PaddleOCR engine adapter, gated behind the `paddle-ocr` feature.
//!
//! [`PaddleOcr`] is a second [`OcrEngine`](crate::engine::OcrEngine) (next to the
//! Tesseract CLI adapter) that runs the pure-Rust PP-OCRv5 detection/recognition
//! and PP-LCNet text-line-orientation models. The inference core is **not** in
//! pdfspine: it lives in the sibling [`ocrspine`] crate (domain-neutral OCR:
//! image → words), which ships the ONNX weights and the tract pipeline once for
//! the whole Spine family. This module is the **PDF-side bridge**: it converts a
//! rendered [`Pixmap`] into an [`ocrspine::OcrImage`], delegates recognition to
//! [`ocrspine::PaddleOcr`], and maps each result back into pdf-ocr's
//! [`OcrWord`] (in PDF-pixel space, with a [`pdf_core::geom::Rect`] bbox and the
//! Tesseract-scale confidence the integration layer already understands).
//!
//! Keeping the inference in `ocrspine` removes the duplicate PP-OCRv5
//! implementation and the duplicate multi-MB ONNX weights that previously lived
//! under `pdf-ocr/`. The PDF-specific glue (page → pixmap, pixmap → RGB, word →
//! `TextPage` / sandwich layer / image-table) stays here in pdfspine.

use image::RgbImage;

// The `ocrspine` engine trait, aliased so it does not clash with pdf-ocr's own
// `OcrEngine` (this adapter implements the latter and calls the former).
use ocrspine::OcrEngine as OcrspineEngine;

use pdf_core::geom::Rect;
use pdf_image::pixmap::{Colorspace, Pixmap};

use crate::engine::{OcrEngine, OcrWord};
use crate::error::{Error, Result};

/// A PaddleOCR engine: a thin PDF-side adapter over [`ocrspine::PaddleOcr`].
///
/// Construct once with [`PaddleOcr::new`] and reuse: the wrapped `ocrspine`
/// engine caches its optimized model runnables per input-shape bucket across
/// [`recognize`](OcrEngine::recognize) calls, so the optimization cost is paid at
/// most once per shape.
pub struct PaddleOcr {
    inner: ocrspine::PaddleOcr,
}

impl PaddleOcr {
    /// Builds the engine. The ONNX model files (shipped by `ocrspine`) are loaded
    /// lazily from disk on first use, not at construction, so this is cheap.
    ///
    /// # Errors
    /// Returns [`Error::Unsupported`](crate::error::Error::Unsupported) if the
    /// underlying engine cannot be prepared; model loading/parsing/optimization is
    /// deferred to the first [`recognize`](OcrEngine::recognize) call — a missing
    /// model directory surfaces there as `Unsupported`.
    pub fn new() -> Result<Self> {
        Ok(PaddleOcr {
            inner: ocrspine::PaddleOcr::new().map_err(map_ocrspine_err)?,
        })
    }
}

impl OcrEngine for PaddleOcr {
    /// Recognizes the words in `image`. `lang` is ignored (the PP-OCRv5 model is a
    /// CJK+Latin multilingual recognizer) and `dpi` is unused (boxes are emitted
    /// in image pixel coordinates, which the integration layer maps to page
    /// space). Empty / low-confidence results are skipped by `ocrspine`.
    fn recognize(&self, image: &Pixmap, _lang: &str, _dpi: f32) -> Result<Vec<OcrWord>> {
        let rgb = pixmap_to_rgb(image);
        let (w, h) = (rgb.width(), rgb.height());
        let ocr_image =
            ocrspine::OcrImage::from_rgb(w, h, rgb.into_raw()).map_err(map_ocrspine_err)?;
        let words = self.inner.recognize(&ocr_image).map_err(map_ocrspine_err)?;

        // Map ocrspine's `OcrWord` (BBox + quad) into pdf-ocr's `OcrWord` (Rect,
        // no quad): the PDF pipeline only needs the axis-aligned box, the text and
        // the [0,100] confidence. The quad is dropped — the integration layer
        // (TextPage / sandwich / image-table) works purely off the bbox.
        Ok(words
            .into_iter()
            .map(|w| OcrWord {
                text: w.text,
                bbox: Rect::new(w.bbox.x0, w.bbox.y0, w.bbox.x1, w.bbox.y1),
                confidence: w.confidence,
            })
            .collect())
    }
}

/// Maps an [`ocrspine::OcrError`] into pdf-ocr's [`Error`].
///
/// Every `ocrspine` failure mode (a missing/unusable model, a bad argument, an
/// I/O or decode error) is surfaced as `Unsupported` here — the PaddleOCR engine
/// being unavailable maps to `PdfUnsupportedError` at the Python boundary, exactly
/// as a missing Tesseract binary does. The underlying `ocrspine` cause is kept
/// verbatim (so the failing model path stays legible), and the pdfspine-facing
/// remediation (`pip install pdfspine[ocr]`) is appended — that is the install
/// the user runs, not `ocrspine`'s internal `OCRSPINE_MODELS` hint.
fn map_ocrspine_err(e: ocrspine::OcrError) -> Error {
    Error::Unsupported(format!(
        "ocrspine: {e} (install the OCR build with `pip install pdfspine[ocr]`)"
    ))
}

/// Converts any [`Pixmap`] (Gray / RGB / CMYK, with or without alpha) into an
/// `image::RgbImage`. The OCR pipeline normally receives RGB (n=3, alpha=false)
/// from `render_for_ocr`, but this handles the other shapes defensively so a
/// directly-constructed Pixmap never panics.
fn pixmap_to_rgb(pix: &Pixmap) -> RgbImage {
    let (w, h) = (pix.width, pix.height);
    let samples = pix.samples();
    let n = pix.n as usize;
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let base = (y as usize) * pix.stride + (x as usize) * n;
            let px = match pix.colorspace {
                Colorspace::Gray => {
                    let g = samples.get(base).copied().unwrap_or(0);
                    [g, g, g]
                }
                Colorspace::Rgb => [
                    samples.get(base).copied().unwrap_or(0),
                    samples.get(base + 1).copied().unwrap_or(0),
                    samples.get(base + 2).copied().unwrap_or(0),
                ],
                Colorspace::Cmyk => {
                    // Naive subtractive CMYK→RGB (additive K applied last).
                    let c = samples.get(base).copied().unwrap_or(0) as f32 / 255.0;
                    let m = samples.get(base + 1).copied().unwrap_or(0) as f32 / 255.0;
                    let ye = samples.get(base + 2).copied().unwrap_or(0) as f32 / 255.0;
                    let k = samples.get(base + 3).copied().unwrap_or(0) as f32 / 255.0;
                    let r = 255.0 * (1.0 - c) * (1.0 - k);
                    let g = 255.0 * (1.0 - m) * (1.0 - k);
                    let b = 255.0 * (1.0 - ye) * (1.0 - k);
                    [r as u8, g as u8, b as u8]
                }
                // `Colorspace` is `#[non_exhaustive]`; treat any future space's
                // first component as gray (defensive, never panics).
                _ => {
                    let g = samples.get(base).copied().unwrap_or(0);
                    [g, g, g]
                }
            };
            img.put_pixel(x, y, image::Rgb(px));
        }
    }
    img
}
