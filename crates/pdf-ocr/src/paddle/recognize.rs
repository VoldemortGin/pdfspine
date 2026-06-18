//! CRNN+CTC text recognition: a (possibly rotated) crop → recognized string +
//! confidence.
//!
//! Matches RapidOCR's PP-OCRv4 rec config: resize the crop to height 48, width
//! `round(48 * w/h)`; bucket the width UP to a multiple of 32 (min 16); resize
//! to `(48, w_prop)` and right-pad with 0 to the bucket width; normalize
//! `(px/255-0.5)/0.5`; run → logits `[1,T,6625]`; CTC greedy decode (per
//! timestep argmax, skip blank index 0, collapse consecutive-equal indices);
//! confidence = mean of the max-softmax-prob over the kept (non-blank)
//! timesteps. Results below `text_score=0.5` are dropped by the caller.

use image::RgbImage;
use tract_onnx::prelude::*;

use crate::error::{Error, Result};
use crate::paddle::model::CharTable;
use crate::paddle::model::Models;
use crate::paddle::preprocess::{resize_exact, to_tensor};

/// Recognition input height.
const REC_H: u32 = 48;
/// Width bucket granularity (must match the rec runnable cache key).
const WIDTH_BUCKET: u32 = 32;
/// Minimum bucketed width.
const MIN_WIDTH: u32 = 16;
/// Symmetric `(px/255-0.5)/0.5` normalization.
const REC_MEAN: [f32; 3] = [0.5, 0.5, 0.5];
const REC_STD: [f32; 3] = [0.5, 0.5, 0.5];

/// A recognized text line: the decoded string and its mean per-step confidence
/// in `[0,1]`.
pub(crate) struct RecResult {
    pub text: String,
    pub confidence: f32,
}

/// Buckets the proportional width up to a multiple of `WIDTH_BUCKET`.
fn bucket_width(crop: &RgbImage) -> (u32, u32) {
    let (cw, ch) = (crop.width().max(1), crop.height().max(1));
    let prop = (((REC_H as f32) * cw as f32) / ch as f32).round() as u32;
    let prop = prop.max(1);
    let bucket = prop.div_ceil(WIDTH_BUCKET) * WIDTH_BUCKET;
    let bucket = bucket.max(MIN_WIDTH);
    (prop.min(bucket), bucket)
}

/// Recognizes the text in a single crop.
pub(crate) fn recognize(models: &Models, crop: &RgbImage) -> Result<RecResult> {
    let (prop_w, bucket_w) = bucket_width(crop);

    // Resize to (48, prop_w) then right-pad with black to (48, bucket_w).
    let resized = resize_exact(crop, prop_w, REC_H);
    let mut canvas = RgbImage::new(bucket_w, REC_H);
    for y in 0..REC_H {
        for x in 0..prop_w {
            canvas.put_pixel(x, y, *resized.get_pixel(x, y));
        }
    }

    let tensor = to_tensor(&canvas, REC_MEAN, REC_STD);
    let runnable = models.rec(bucket_w as usize)?;
    let out = runnable
        .run(tvec!(tensor.into()))
        .map_err(|e| Error::Unsupported(format!("paddle: rec inference failed: {e}")))?;

    let view = out[0]
        .to_array_view::<f32>()
        .map_err(|e| Error::Unsupported(format!("paddle: bad rec output: {e}")))?;
    let shape = view.shape();
    // Expect [1, T, C]; tolerate [T, C].
    let (t, c) = match shape.len() {
        3 => (shape[1], shape[2]),
        2 => (shape[0], shape[1]),
        _ => {
            return Err(Error::Unsupported(format!(
                "paddle: unexpected rec output rank {}",
                shape.len()
            )))
        }
    };
    let flat: Vec<f32> = view.iter().copied().collect();
    Ok(ctc_greedy_decode(&flat, t, c, &models.chars))
}

/// CTC greedy decode over the rec output laid out as `[t][c]` (row-major,
/// length `t*c`).
///
/// The PP-OCRv4 rec model already applies softmax in-graph, so each row is a
/// probability distribution over the `c` classes. Per timestep we take the
/// argmax class; we skip the blank (index 0) and collapse runs of the same
/// index, mapping kept indices through the dictionary. Confidence is the mean
/// over kept timesteps of the winning class's probability (the row max). A tiny
/// `softmax` guard handles the (unexpected) case of raw logits: if a row's
/// max value exceeds 1, we softmax that row to recover a probability.
fn ctc_greedy_decode(probs: &[f32], t: usize, c: usize, chars: &CharTable) -> RecResult {
    let mut text = String::new();
    let mut conf_sum = 0.0f32;
    let mut conf_n = 0u32;
    let mut prev: isize = -1;

    for ti in 0..t {
        let row = &probs[ti * c..ti * c + c];
        // argmax + value.
        let mut best_idx = 0usize;
        let mut best_val = f32::NEG_INFINITY;
        for (i, &v) in row.iter().enumerate() {
            if v > best_val {
                best_val = v;
                best_idx = i;
            }
        }
        // Collapse repeats and skip blank (index 0).
        if best_idx != 0 && best_idx as isize != prev {
            // Bound the dictionary lookup to the model's class count.
            if best_idx < chars.len() {
                text.push_str(chars.get(best_idx));
            }
            // Probability of the winning class. The model outputs softmax probs
            // already (row max in [0,1]); if it ever emits logits (max > 1),
            // recover the probability via a stable softmax of this row.
            let p = if best_val > 1.0 {
                let denom: f32 = row.iter().map(|&v| (v - best_val).exp()).sum();
                if denom > 0.0 {
                    1.0 / denom
                } else {
                    best_val
                }
            } else {
                best_val
            };
            conf_sum += p;
            conf_n += 1;
        }
        prev = best_idx as isize;
    }

    let confidence = if conf_n > 0 {
        conf_sum / conf_n as f32
    } else {
        0.0
    };
    RecResult { text, confidence }
}
