//! DBNet text detection: image → text boxes in original pixel coordinates.
//!
//! Pipeline (matching RapidOCR's PP-OCRv4 det config):
//!   1. resize so `min(h,w) ≈ 736` (limit_type=min), cap max side ≤ 2000,
//!      min side ≥ 30, both rounded to a multiple of 32 for the model;
//!   2. normalize per channel and run DBNet → prob map `[1,1,H,W]`;
//!   3. binarize at `thresh=0.3`, dilate the mask 2×2 (`use_dilation`);
//!   4. for each connected component compute the **minimum-area rotated
//!      rectangle** (rotating calipers over the component's convex hull);
//!   5. drop boxes whose mean prob (score_mode=fast) `< box_thresh=0.5`;
//!   6. unclip (inflate) the rotated rect by `area*unclip_ratio/perimeter`
//!      (`unclip_ratio=1.6`) outward along both axes;
//!   7. scale the rotated quad back to ORIGINAL image pixels;
//!   8. sort top-to-bottom then left-to-right.
//!
//! Each [`DetBox`] carries both the axis-aligned bounding box (the public
//! `OcrWord.bbox`) AND the four rotated-quad corners + angle, so the recognizer
//! can de-rotate skewed crops to horizontal. A ~0° min-area rect collapses to
//! the axis-aligned box, so upright text behaves exactly as before.

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

/// A detected text box in ORIGINAL image pixel coordinates.
///
/// `x0,y0,x1,y1` is the axis-aligned bounding box of the rotated quad (this is
/// the box surfaced as `OcrWord.bbox`). `quad` holds the four corners of the
/// minimum-area rotated rectangle, ordered top-left, top-right, bottom-right,
/// bottom-left along the rect's own axes; `angle` is the rect's rotation in
/// radians (the angle of its long/text axis from horizontal, in `(-π/2, π/2]`).
/// For upright text `angle ≈ 0` and the quad coincides with the AABB corners.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DetBox {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub score: f32,
    pub quad: [(f32, f32); 4],
    pub angle: f32,
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

    // 3) Connected components (8-connectivity), keeping each component's
    //    foreground pixels (in model/prob-map space, which equals model-input
    //    space — DBNet output is full resolution) so we can fit a rotated rect.
    let comps = connected_components(&dilated, pw, ph);

    // Scale from model space back to original image pixels.
    let sx = ow as f32 / mw as f32;
    let sy = oh as f32 / mh as f32;

    let mut boxes = Vec::new();
    for c in comps {
        // Skip tiny components (use the AABB extent as a cheap pre-filter).
        if (c.x1 - c.x0) < MIN_BOX_SIDE || (c.y1 - c.y0) < MIN_BOX_SIDE {
            continue;
        }
        // 4) Minimum-area rotated rectangle over the component's convex hull.
        let mar = min_area_rect(&c.pixels);
        // Skip degenerate rects (a thin line of pixels).
        if mar.w < MIN_BOX_SIDE as f32 || mar.h < MIN_BOX_SIDE as f32 {
            continue;
        }
        // 5) Fast score: mean prob over the rotated rect's polygon (RapidOCR's
        //    box_score_fast masks the box, not its AABB — essential for skewed
        //    boxes, whose AABB is mostly background).
        let score = mean_prob_quad(&prob, pw, ph, &rect_corners(&mar));
        if score < BOX_THRESH {
            continue;
        }
        // 6) Unclip: inflate the rect outward along both axes.
        let quad_model = unclip_rect(&mar);

        // 7) Scale the quad to original pixels.
        let mut quad = [(0.0f32, 0.0f32); 4];
        for (i, &(px, py)) in quad_model.iter().enumerate() {
            quad[i] = (px * sx, py * sy);
        }
        // Axis-aligned bbox of the (scaled) rotated quad → OcrWord.bbox.
        let (mut bx0, mut by0, mut bx1, mut by1) = (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        );
        for &(px, py) in &quad {
            bx0 = bx0.min(px);
            by0 = by0.min(py);
            bx1 = bx1.max(px);
            by1 = by1.max(py);
        }
        // Rect angle measured in original pixel space (sx/sy may differ, but for
        // OCR pages they are equal, so this is the true text-axis angle).
        let angle = (quad[1].1 - quad[0].1).atan2(quad[1].0 - quad[0].0);
        boxes.push(DetBox {
            x0: (bx0.floor() as i32).clamp(0, ow as i32),
            y0: (by0.floor() as i32).clamp(0, oh as i32),
            x1: (bx1.ceil() as i32).clamp(0, ow as i32),
            y1: (by1.ceil() as i32).clamp(0, oh as i32),
            score,
            quad,
            angle,
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

/// One connected component: its axis-aligned bbox (`x1`/`y1` exclusive, spanning
/// `[x0,x1) × [y0,y1)`) plus every foreground pixel `(x,y)` it contains (mask
/// coords), which feeds the rotated-rect fit.
struct Comp {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    pixels: Vec<(i32, i32)>,
}

/// 8-connected component extraction via iterative flood fill, collecting each
/// component's pixels and bounding box.
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
            // New component: flood fill, tracking the bbox and pixels.
            let (mut x0, mut y0) = (sx as i32, sy as i32);
            let (mut x1, mut y1) = (sx as i32, sy as i32);
            let mut pixels = Vec::new();
            stack.clear();
            stack.push((sx as i32, sy as i32));
            visited[idx] = true;
            while let Some((cx, cy)) = stack.pop() {
                x0 = x0.min(cx);
                y0 = y0.min(cy);
                x1 = x1.max(cx);
                y1 = y1.max(cy);
                pixels.push((cx, cy));
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
                pixels,
            });
        }
    }
    comps
}

/// A rotated rectangle: center, side lengths (`w` is the long/text axis, `h` the
/// short axis), and the text axis as a unit vector `(ux, uy)`. Lives in
/// mask/model space.
pub(crate) struct RotatedRect {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub ux: f32,
    pub uy: f32,
}

/// Computes the minimum-area enclosing rectangle of a point set via rotating
/// calipers over its convex hull. The returned rect's `w` is the longer side
/// (taken as the text axis) and `(ux,uy)` points along it.
///
/// For an axis-aligned point cloud this yields an axis-aligned rect (`uy≈0`), so
/// upright text collapses to today's behavior.
pub(crate) fn min_area_rect(points: &[(i32, i32)]) -> RotatedRect {
    let hull = convex_hull(points);
    // Degenerate hulls: fall back to the axis-aligned bbox.
    if hull.len() < 3 {
        let (mut x0, mut y0, mut x1, mut y1) = (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        );
        for &(px, py) in points {
            let (px, py) = (px as f32, py as f32);
            x0 = x0.min(px);
            y0 = y0.min(py);
            x1 = x1.max(px);
            y1 = y1.max(py);
        }
        return RotatedRect {
            cx: (x0 + x1) / 2.0,
            cy: (y0 + y1) / 2.0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
            ux: 1.0,
            uy: 0.0,
        };
    }

    let mut best_area = f32::INFINITY;
    let mut best = RotatedRect {
        cx: 0.0,
        cy: 0.0,
        w: 0.0,
        h: 0.0,
        ux: 1.0,
        uy: 0.0,
    };
    let n = hull.len();
    // Each hull edge is a candidate rect orientation (the optimal rect always
    // has one side flush with an edge of the hull).
    for i in 0..n {
        let (ax, ay) = hull[i];
        let (bx, by) = hull[(i + 1) % n];
        let (ex, ey) = (bx - ax, by - ay);
        let len = (ex * ex + ey * ey).sqrt();
        if len < f32::EPSILON {
            continue;
        }
        // Edge direction (u) and its perpendicular (v).
        let (ux, uy) = (ex / len, ey / len);
        let (vx, vy) = (-uy, ux);
        // Project every hull vertex onto (u, v).
        let (mut min_u, mut max_u) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_v, mut max_v) = (f32::INFINITY, f32::NEG_INFINITY);
        for &(hx, hy) in &hull {
            let pu = hx * ux + hy * uy;
            let pv = hx * vx + hy * vy;
            min_u = min_u.min(pu);
            max_u = max_u.max(pu);
            min_v = min_v.min(pv);
            max_v = max_v.max(pv);
        }
        let su = max_u - min_u;
        let sv = max_v - min_v;
        let area = su * sv;
        if area < best_area {
            best_area = area;
            // Center in (u,v) → back to xy.
            let mu = (min_u + max_u) / 2.0;
            let mv = (min_v + max_v) / 2.0;
            let cx = mu * ux + mv * vx;
            let cy = mu * uy + mv * vy;
            // Orient so w is the longer side (the text axis).
            if su >= sv {
                best = RotatedRect {
                    cx,
                    cy,
                    w: su,
                    h: sv,
                    ux,
                    uy,
                };
            } else {
                best = RotatedRect {
                    cx,
                    cy,
                    w: sv,
                    h: su,
                    ux: vx,
                    uy: vy,
                };
            }
        }
    }

    // Normalize the axis so the angle stays in (-π/2, π/2] (point rightward, and
    // for the vertical edge case point downward). Keeps `angle` ~0 for upright.
    if best.ux < 0.0 || (best.ux == 0.0 && best.uy < 0.0) {
        best.ux = -best.ux;
        best.uy = -best.uy;
    }
    best
}

/// Andrew's monotone-chain convex hull. Input mask pixels (`i32`), output hull
/// vertices in `f32`, counter-clockwise, without the duplicated endpoint.
fn convex_hull(points: &[(i32, i32)]) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
    }
    let mut pts: Vec<(i32, i32)> = points.to_vec();
    pts.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
    }

    // 2D cross product of OA × OB (i64 to avoid overflow).
    let cross = |o: (i32, i32), a: (i32, i32), b: (i32, i32)| -> i64 {
        (a.0 - o.0) as i64 * (b.1 - o.1) as i64 - (a.1 - o.1) as i64 * (b.0 - o.0) as i64
    };

    let mut lower: Vec<(i32, i32)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(i32, i32)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower.iter().map(|&(x, y)| (x as f32, y as f32)).collect()
}

/// The four corners of a rotated rect (no unclip), in mask space, ordered
/// top-left, top-right, bottom-right, bottom-left along the rect's own axes.
pub(crate) fn rect_corners(r: &RotatedRect) -> [(f32, f32); 4] {
    let hw = r.w / 2.0;
    let hh = r.h / 2.0;
    let (ux, uy) = (r.ux, r.uy);
    let (vx, vy) = (-uy, ux);
    let corner = |su: f32, sv: f32| -> (f32, f32) {
        (
            r.cx + su * hw * ux + sv * hh * vx,
            r.cy + su * hw * uy + sv * hh * vy,
        )
    };
    [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]
}

/// Mean probability over the (convex) quad polygon — the fast score for a
/// rotated box. Scans the quad's AABB and includes only pixels inside the
/// polygon (point-in-convex-polygon via consistent edge sign).
fn mean_prob_quad(prob: &[f32], w: usize, h: usize, quad: &[(f32, f32); 4]) -> f32 {
    let (mut x0, mut y0, mut x1, mut y1) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for &(px, py) in quad {
        x0 = x0.min(px);
        y0 = y0.min(py);
        x1 = x1.max(px);
        y1 = y1.max(py);
    }
    let ix0 = (x0.floor() as i32).clamp(0, w as i32);
    let iy0 = (y0.floor() as i32).clamp(0, h as i32);
    let ix1 = (x1.ceil() as i32).clamp(0, w as i32);
    let iy1 = (y1.ceil() as i32).clamp(0, h as i32);
    if ix1 <= ix0 || iy1 <= iy0 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    let mut count = 0u32;
    for y in iy0..iy1 {
        for x in ix0..ix1 {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            if point_in_quad(p, quad) {
                sum += prob[y as usize * w + x as usize];
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f32
    }
}

/// Point-in-convex-quad test: the point is inside iff it is on the same side of
/// every directed edge (all cross products share one sign).
fn point_in_quad(p: (f32, f32), quad: &[(f32, f32); 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross.abs() > f32::EPSILON {
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
    }
    true
}

/// Unclips (inflates) a rotated rect outward along both of its axes by
/// `distance = area * unclip_ratio / perimeter` (Vatti-clip approximation) and
/// returns the four corners ordered along the rect's own axes: top-left,
/// top-right, bottom-right, bottom-left (in mask space).
pub(crate) fn unclip_rect(r: &RotatedRect) -> [(f32, f32); 4] {
    let area = r.w * r.h;
    let perimeter = 2.0 * (r.w + r.h);
    let dist = if perimeter > 0.0 {
        area * UNCLIP_RATIO / perimeter
    } else {
        0.0
    };
    let hw = r.w / 2.0 + dist;
    let hh = r.h / 2.0 + dist;
    let (ux, uy) = (r.ux, r.uy);
    let (vx, vy) = (-uy, ux); // perpendicular (short axis)
                              // Corners: -u-v, +u-v, +u+v, -u+v.
    let corner = |su: f32, sv: f32| -> (f32, f32) {
        (
            r.cx + su * hw * ux + sv * hh * vx,
            r.cy + su * hw * uy + sv * hh * vy,
        )
    };
    [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]
}
