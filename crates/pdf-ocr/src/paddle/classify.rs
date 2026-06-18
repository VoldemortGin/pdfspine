//! 180° text-angle classification: decides whether a crop is upside-down and,
//! if so, rotates it before recognition.
//!
//! Matches RapidOCR's PP-OCRv2 cls config: resize each crop to 3×48×192 (height
//! to 48, then width to 192 with right-pad), normalize `(px/255-0.5)/0.5`, run,
//! softmax the `[1,2]` logits; if `argmax==1` (label "180") with conf
//! `> cls_thresh=0.9`, rotate the crop 180°.

use image::RgbImage;
use tract_onnx::prelude::*;

use crate::error::{Error, Result};
use crate::paddle::model::Models;
use crate::paddle::preprocess::{resize_exact, to_tensor};

/// Classifier input geometry.
const CLS_H: u32 = 48;
const CLS_W: u32 = 192;
/// Confidence above which a "180" prediction triggers a rotation.
const CLS_THRESH: f32 = 0.9;
/// Symmetric `(px/255-0.5)/0.5` normalization.
const CLS_MEAN: [f32; 3] = [0.5, 0.5, 0.5];
const CLS_STD: [f32; 3] = [0.5, 0.5, 0.5];

/// Resizes `crop` into the fixed 48×192 classifier canvas: scale to height 48
/// preserving aspect, cap width at 192, then right-pad with black to 192.
fn fit_cls(crop: &RgbImage) -> RgbImage {
    let (cw, ch) = (crop.width().max(1), crop.height().max(1));
    let new_w = (((CLS_H as f32) * cw as f32) / ch as f32).round() as u32;
    let new_w = new_w.clamp(1, CLS_W);
    let resized = resize_exact(crop, new_w, CLS_H);
    let mut canvas = RgbImage::new(CLS_W, CLS_H);
    for y in 0..CLS_H {
        for x in 0..new_w {
            canvas.put_pixel(x, y, *resized.get_pixel(x, y));
        }
    }
    canvas
}

/// Returns the (possibly 180°-rotated) crop, ready for recognition.
///
/// On any classifier failure this returns the crop unchanged (orientation is a
/// best-effort refinement, not a hard requirement) — but model-load failures
/// still surface as typed errors via `models.cls()`.
pub(crate) fn classify_and_orient(models: &Models, crop: RgbImage) -> Result<RgbImage> {
    let fitted = fit_cls(&crop);
    let tensor = to_tensor(&fitted, CLS_MEAN, CLS_STD);
    let runnable = models.cls()?;
    let out = runnable
        .run(tvec!(tensor.into()))
        .map_err(|e| Error::Unsupported(format!("paddle: cls inference failed: {e}")))?;
    let view = out[0]
        .to_array_view::<f32>()
        .map_err(|e| Error::Unsupported(format!("paddle: bad cls output: {e}")))?;
    let logits: Vec<f32> = view.iter().copied().collect();
    if logits.len() < 2 {
        return Ok(crop);
    }
    // Softmax over the 2 logits.
    let m = logits[0].max(logits[1]);
    let e0 = (logits[0] - m).exp();
    let e1 = (logits[1] - m).exp();
    let p1 = e1 / (e0 + e1);
    if p1 > CLS_THRESH {
        Ok(rotate180(&crop))
    } else {
        Ok(crop)
    }
}

/// Rotates an image 180° (in place into a fresh buffer).
fn rotate180(img: &RgbImage) -> RgbImage {
    let (w, h) = (img.width(), img.height());
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.put_pixel(w - 1 - x, h - 1 - y, *img.get_pixel(x, y));
        }
    }
    out
}
