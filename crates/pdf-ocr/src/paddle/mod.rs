//! Pure-Rust PaddleOCR (PP-OCRv5) engine, gated behind the `paddle-ocr` feature.
//!
//! [`PaddleOcr`] is a second [`OcrEngine`](crate::engine::OcrEngine) (next to the
//! Tesseract CLI adapter) that runs the shipped PP-OCRv5 detection/recognition
//! and PP-LCNet text-line-orientation ONNX models on CPU via [`tract`], with no
//! Python and no C/C++ runtime. It is higher-accuracy on mixed CJK+Latin text
//! than the Tesseract default and needs no external binary.
//!
//! Pipeline per page image: **detect** text boxes (minimum-area rotated rects) →
//! for each box **crop** (axis-aligned for upright text, or de-rotated to
//! horizontal via the rotated quad for skewed text) → **classify** orientation
//! (rotate 180° if needed) → **recognize** (CRNN+CTC) → emit one [`OcrWord`] per
//! box with the box in image pixel coordinates and confidence on the Tesseract
//! `[0,100]` scale.
//!
//! Rotated/skewed text is detected as a rotated rectangle and de-rotated before
//! recognition; the public [`OcrWord::bbox`](crate::OcrWord::bbox) remains the
//! axis-aligned bounding box of that rotated quad.

mod classify;
mod detect;
mod model;
mod preprocess;
mod recognize;

use pdf_core::geom::Rect;
use rayon::prelude::*;

use crate::engine::{OcrEngine, OcrWord};
use crate::error::Result;
use pdf_image::pixmap::Pixmap;

use self::model::Models;

/// Below this recognizer confidence (`[0,1]`) a result is dropped (`text_score`).
const TEXT_SCORE: f32 = 0.5;

/// Boxes within this angle of horizontal (radians, ≈2.9°) are treated as upright
/// and use the axis-aligned crop path — keeping upright text byte-for-byte as it
/// was before rotated-rect detection.
const UPRIGHT_ANGLE_RAD: f32 = 0.05;

/// A pure-Rust PaddleOCR engine running PP-OCRv5 ONNX models via `tract`.
///
/// Construct once with [`PaddleOcr::new`] and reuse: optimized model runnables
/// are cached per input-shape bucket across [`recognize`](OcrEngine::recognize)
/// calls, so the expensive optimization cost is paid at most once per shape.
pub struct PaddleOcr {
    models: Models,
}

impl PaddleOcr {
    /// Builds the engine. The ONNX model files are loaded lazily from disk on
    /// first use (not at construction), and no optimization runs yet (that also
    /// happens lazily on first use of each input-shape bucket), so this is cheap.
    ///
    /// # Errors
    /// Returns [`Error::Unsupported`](crate::error::Error::Unsupported) only if
    /// the embedded recognition dictionary cannot be prepared (it always can in
    /// a correctly built binary); model loading/parsing/optimization is deferred
    /// to the first [`recognize`](OcrEngine::recognize) call — a missing model
    /// directory surfaces there as `Unsupported`, pointing at `pdfspine[ocr]`.
    pub fn new() -> Result<Self> {
        Ok(PaddleOcr {
            models: Models::new()?,
        })
    }
}

impl OcrEngine for PaddleOcr {
    /// Recognizes the words in `image`. `lang` is ignored (the `ch` model is a
    /// CJK+Latin multilingual recognizer) and `dpi` is unused (boxes are emitted
    /// in image pixel coordinates, which the integration layer maps to page
    /// space). Empty / low-confidence results are skipped.
    fn recognize(&self, image: &Pixmap, _lang: &str, _dpi: f32) -> Result<Vec<OcrWord>> {
        let rgb = preprocess::pixmap_to_rgb(image);
        let boxes = detect::detect(&self.models, &rgb)?;

        // Recognize each detected box in parallel. The work is CPU-bound (crop →
        // classify → CRNN+CTC) and a scanned page yields dozens–hundreds of boxes,
        // so `par_iter` near-linearly cuts wall time across cores. The shared
        // runnable cache (`self.models`, `&self` + `Mutex`/`OnceLock`) is
        // thread-safe to share, so no per-box state is duplicated.
        //
        // DETERMINISM: `par_iter().map(..).collect::<Vec<_>>()` is an *indexed*
        // collect — output position equals input box index — so the per-box
        // `Option<OcrWord>` vector is byte-identical to the sequential version
        // regardless of completion order. We then drop the skipped (`None`) boxes
        // in that same order. Any per-box error short-circuits the whole call.
        let per_box: Vec<Option<OcrWord>> = boxes
            .par_iter()
            .map(|b| -> Result<Option<OcrWord>> {
                // Upright (~0°) boxes use the exact axis-aligned crop path as before;
                // skewed boxes are de-rotated to horizontal via the rotated quad so
                // the recognizer (which needs horizontal text) sees an upright line.
                let crop = if b.angle.abs() <= UPRIGHT_ANGLE_RAD {
                    preprocess::crop(&rgb, b.x0, b.y0, b.x1, b.y1)
                } else {
                    preprocess::crop_rotated(&rgb, &b.quad)
                };
                let oriented = classify::classify_and_orient(&self.models, crop)?;
                let rec = recognize::recognize(&self.models, &oriented)?;
                let text = rec.text.trim().to_string();
                if text.is_empty() || rec.confidence < TEXT_SCORE {
                    return Ok(None);
                }
                Ok(Some(OcrWord {
                    text,
                    bbox: Rect::new(b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64),
                    // Combine detection + recognition confidence onto the [0,100]
                    // Tesseract scale.
                    confidence: (rec.confidence * b.score * 100.0).clamp(0.0, 100.0),
                }))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(per_box.into_iter().flatten().collect())
    }
}
