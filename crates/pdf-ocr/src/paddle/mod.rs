//! Pure-Rust PaddleOCR (PP-OCRv4) engine, gated behind the `paddle-ocr` feature.
//!
//! [`PaddleOcr`] is a second [`OcrEngine`](crate::engine::OcrEngine) (next to the
//! Tesseract CLI adapter) that runs the shipped PP-OCRv4 detection/recognition
//! and PP-OCRv2 angle-classification ONNX models on CPU via [`tract`], with no
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

/// A pure-Rust PaddleOCR engine running PP-OCRv4 ONNX models via `tract`.
///
/// Construct once with [`PaddleOcr::new`] and reuse: optimized model runnables
/// are cached per input-shape bucket across [`recognize`](OcrEngine::recognize)
/// calls, so the expensive optimization cost is paid at most once per shape.
pub struct PaddleOcr {
    models: Models,
}

impl PaddleOcr {
    /// Builds the engine. Model bytes are embedded in the binary, so this needs
    /// no filesystem access; it does not run any optimization yet (that happens
    /// lazily on first use of each input-shape bucket), so it is cheap.
    ///
    /// # Errors
    /// Returns [`Error::Unsupported`](crate::error::Error::Unsupported) only if
    /// the embedded recognition dictionary cannot be prepared (it always can in
    /// a correctly built binary); model parsing/optimization is deferred to the
    /// first [`recognize`](OcrEngine::recognize) call.
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

        let mut words = Vec::with_capacity(boxes.len());
        for b in boxes {
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
                continue;
            }
            words.push(OcrWord {
                text,
                bbox: Rect::new(b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64),
                // Combine detection + recognition confidence onto the [0,100]
                // Tesseract scale.
                confidence: (rec.confidence * b.score * 100.0).clamp(0.0, 100.0),
            });
        }
        Ok(words)
    }
}
