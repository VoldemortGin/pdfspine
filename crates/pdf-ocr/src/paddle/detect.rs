//! DBNet text detection: image → axis-aligned text boxes in original pixel
//! coordinates.
//!
//! Pipeline (matching RapidOCR's PP-OCRv4 det config):
//!   1. resize so `min(h,w) ≈ 736` (limit_type=min), cap max side ≤ 2000,
//!      min side ≥ 30, both rounded to a multiple of 32 for the model;
//!   2. normalize per channel and run DBNet → prob map `[1,1,H,W]`;
//!   3. binarize at `thresh=0.3`, dilate the mask 2×2 (`use_dilation`);
//!   4. extract one axis-aligned bbox per connected component;
//!   5. drop boxes whose mean prob (score_mode=fast) `< box_thresh=0.5`;
//!   6. unclip (inflate) each box by `area*unclip_ratio/perimeter`
//!      (`unclip_ratio=1.6`) on every side;
//!   7. scale boxes back to ORIGINAL image pixels;
//!   8. sort top-to-bottom then left-to-right.
//!
//! Rotated text is represented by its axis-aligned bounding box (v1): the
//! acceptance image is upright. This is a deliberate, documented simplification.

use image::RgbImage;
use tract_onnx::prelude::*;

use crate::error::{Error, Result};
use crate::paddle::model::Models;
use crate::paddle::preprocess::{resize_exact, to_tensor};

/// `limit_side_len`: resize so the SHORT side lands near this (limit_type=min).
const LIMIT_SIDE_LEN: u32 = 736;
/// Hard cap on the long side after resize.
const MAX_SIDE: u32 = 2000;
/// Hard floor on the short side after resize.
const MIN_SIDE: u32 = 30;
/// Model spatial dims must be multiples of this.
const STRIDE: u32 = 32;
/// Probability-map binarization threshold.
const BIN_THRESH: f32 = 0.3;
/// Minimum mean-prob (fast mode) for a kept box.
const BOX_THRESH: f32 = 0.5;
/// Box inflation ratio (Vatti-clip approximation).
const UNCLIP_RATIO: f32 = 1.6;
/// Minimum box side (px, in MODEL space) to keep a component.
const MIN_BOX_SIDE: i32 = 3;

/// ImageNet normalization (the variant RapidOCR's det model is trained with).
const DET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const DET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// A detected text box in ORIGINAL image pixel coordinates (axis-aligned,
/// `(x0,y0)` top-left, `(x1,y1)` bottom-right) with its detection score.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DetBox {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub score: f32,
}

/// Computes the model input size for `(w, h)`: scale the short side to
/// `LIMIT_SIDE_LEN`, clamp the long side to `MAX_SIDE` and short to `MIN_SIDE`,
/// then round both to a multiple of `STRIDE`. Returns `(model_w, model_h)`.
fn det_input_size(w: u32, h: u32) -> (u32, u32) {
    let short = w.min(h).max(1) as f32;
    let scale = LIMIT_SIDE_LEN as f32 / short;
    let mut mw = (w as f32 * scale).round() as u32;
    let mut mh = (h as f32 * scale).round() as u32;
    // Clamp long/short sides.
    let long = mw.max(mh);
    if long > MAX_SIDE {
        let s = MAX_SIDE as f32 / long as f32;
        mw = (mw as f32 * s).round() as u32;
        mh = (mh as f32 * s).round() as u32;
    }
    mw = mw.max(MIN_SIDE);
    mh = mh.max(MIN_SIDE);
    // Round up to multiple of STRIDE.
    mw = mw.div_ceil(STRIDE) * STRIDE;
    mh = mh.div_ceil(STRIDE) * STRIDE;
    (mw.max(STRIDE), mh.max(STRIDE))
}

/// Runs detection on `img`, returning boxes in original-image pixel coords.
pub(crate) fn detect(models: &Models, img: &RgbImage) -> Result<Vec<DetBox>> {
    let (ow, oh) = (img.width(), img.height());
    let (mw, mh) = det_input_size(ow, oh);

    let resized = resize_exact(img, mw, mh);
    let tensor = to_tensor(&resized, DET_MEAN, DET_STD);

    let runnable = models.det(mh as usize, mw as usize)?;
    let out = runnable
        .run(tvec!(tensor.into()))
        .map_err(|e| Error::Unsupported(format!("paddle: detection inference failed: {e}")))?;

    // Output prob map [1,1,H,W] (or [1,H,W]); read as a flat H*W f32 view.
    let view = out[0]
        .to_array_view::<f32>()
        .map_err(|e| Error::Unsupported(format!("paddle: bad detection output: {e}")))?;
    let shape = view.shape();
    let (ph, pw) = match shape.len() {
        4 => (shape[2], shape[3]),
        3 => (shape[1], shape[2]),
        2 => (shape[0], shape[1]),
        _ => {
            return Err(Error::Unsupported(format!(
                "paddle: unexpected detection output rank {}",
                shape.len()
            )))
        }
    };
    let prob: Vec<f32> = view.iter().copied().collect();
    debug_assert_eq!(prob.len(), ph * pw);

    // 1) Binarize.
    let mut mask = vec![false; ph * pw];
    for (m, &p) in mask.iter_mut().zip(prob.iter()) {
        *m = p >= BIN_THRESH;
    }
    // 2) Dilate 2×2 (structuring element anchored top-left, like cv2 with a
    //    2×2 kernel: a pixel turns on if itself or its right/below/diagonal
    //    neighbour was on). This thickens strokes so adjacent glyphs merge.
    let dilated = dilate_2x2(&mask, pw, ph);

    // 3) Connected components (8-connectivity) → axis-aligned bboxes in model
    //    (prob-map) space, which equals model-input space (DBNet output is full
    //    resolution).
    let comps = connected_components(&dilated, pw, ph);

    // Scale from model space back to original image pixels.
    let sx = ow as f32 / mw as f32;
    let sy = oh as f32 / mh as f32;

    let mut boxes = Vec::new();
    for c in comps {
        // Skip tiny components.
        if (c.x1 - c.x0) < MIN_BOX_SIDE || (c.y1 - c.y0) < MIN_BOX_SIDE {
            continue;
        }
        // 4) Fast score: mean prob inside the component's bbox.
        let score = mean_prob(&prob, pw, ph, c.x0, c.y0, c.x1, c.y1);
        if score < BOX_THRESH {
            continue;
        }
        // 5) Unclip: inflate by area*ratio/perimeter on each side.
        let (ux0, uy0, ux1, uy1) = unclip(c.x0, c.y0, c.x1, c.y1);

        // 6) Scale to original pixels and clamp.
        let bx0 = ((ux0 as f32) * sx).floor() as i32;
        let by0 = ((uy0 as f32) * sy).floor() as i32;
        let bx1 = ((ux1 as f32) * sx).ceil() as i32;
        let by1 = ((uy1 as f32) * sy).ceil() as i32;
        boxes.push(DetBox {
            x0: bx0.clamp(0, ow as i32),
            y0: by0.clamp(0, oh as i32),
            x1: bx1.clamp(0, ow as i32),
            y1: by1.clamp(0, oh as i32),
            score,
        });
    }

    // 7) Sort top-to-bottom, then left-to-right. Group rows by a y-tolerance so
    //    boxes on the same visual line read left-to-right.
    boxes.sort_by(|a, b| {
        let ay = (a.y0 + a.y1) / 2;
        let by = (b.y0 + b.y1) / 2;
        // Same line if vertical centers are within half the smaller box height.
        let tol = ((a.y1 - a.y0).min(b.y1 - b.y0) / 2).max(1);
        if (ay - by).abs() <= tol {
            a.x0.cmp(&b.x0)
        } else {
            ay.cmp(&by)
        }
    });

    Ok(boxes)
}

/// 2×2 dilation: output pixel `(x,y)` is on if any of `(x,y)`, `(x+1,y)`,
/// `(x,y+1)`, `(x+1,y+1)` was on in the input. Matches cv2.dilate with a 2×2
/// all-ones kernel (anchored at the top-left).
fn dilate_2x2(mask: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut on = mask[y * w + x];
            if !on && x + 1 < w {
                on = mask[y * w + x + 1];
            }
            if !on && y + 1 < h {
                on = mask[(y + 1) * w + x];
            }
            if !on && x + 1 < w && y + 1 < h {
                on = mask[(y + 1) * w + x + 1];
            }
            out[y * w + x] = on;
        }
    }
    out
}

/// An axis-aligned bounding box of one connected component, in mask coords
/// (`x1`/`y1` are exclusive: the box spans `[x0,x1) × [y0,y1)`).
struct Comp {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

/// 8-connected component bounding-box extraction via iterative flood fill.
fn connected_components(mask: &[bool], w: usize, h: usize) -> Vec<Comp> {
    let mut visited = vec![false; w * h];
    let mut comps = Vec::new();
    let mut stack: Vec<(i32, i32)> = Vec::new();
    for sy in 0..h {
        for sx in 0..w {
            let idx = sy * w + sx;
            if !mask[idx] || visited[idx] {
                continue;
            }
            // New component: flood fill, tracking the bbox.
            let (mut x0, mut y0) = (sx as i32, sy as i32);
            let (mut x1, mut y1) = (sx as i32, sy as i32);
            stack.clear();
            stack.push((sx as i32, sy as i32));
            visited[idx] = true;
            while let Some((cx, cy)) = stack.pop() {
                x0 = x0.min(cx);
                y0 = y0.min(cy);
                x1 = x1.max(cx);
                y1 = y1.max(cy);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = cx + dx;
                        let ny = cy + dy;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let nidx = ny as usize * w + nx as usize;
                        if mask[nidx] && !visited[nidx] {
                            visited[nidx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            // Make x1/y1 exclusive.
            comps.push(Comp {
                x0,
                y0,
                x1: x1 + 1,
                y1: y1 + 1,
            });
        }
    }
    comps
}

/// Mean probability over the bbox `[x0,x1) × [y0,y1)` (fast score mode).
fn mean_prob(prob: &[f32], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32) -> f32 {
    let x0 = x0.clamp(0, w as i32) as usize;
    let y0 = y0.clamp(0, h as i32) as usize;
    let x1 = x1.clamp(0, w as i32) as usize;
    let y1 = y1.clamp(0, h as i32) as usize;
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    let mut count = 0u32;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += prob[y * w + x];
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f32
    }
}

/// Vatti-clip-style unclip approximation for an axis-aligned box: offset each
/// edge outward by `distance = area * unclip_ratio / perimeter`.
fn unclip(x0: i32, y0: i32, x1: i32, y1: i32) -> (i32, i32, i32, i32) {
    let w = (x1 - x0).max(0) as f32;
    let h = (y1 - y0).max(0) as f32;
    let area = w * h;
    let perimeter = 2.0 * (w + h);
    if perimeter <= 0.0 {
        return (x0, y0, x1, y1);
    }
    let dist = area * UNCLIP_RATIO / perimeter;
    let d = dist.round() as i32;
    (x0 - d, y0 - d, x1 + d, y1 + d)
}
