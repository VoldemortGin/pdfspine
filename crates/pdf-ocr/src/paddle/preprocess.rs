//! Shared image helpers: `Pixmap` → RGB, cropping, resize, and normalization
//! into a `tract` `[1,3,H,W]` f32 tensor.

use image::{imageops::FilterType, RgbImage};
use tract_onnx::prelude::*;

use pdf_image::pixmap::{Colorspace, Pixmap};

/// Converts any [`Pixmap`] (Gray / RGB / CMYK, with or without alpha) into an
/// `image::RgbImage`. The OCR pipeline normally receives RGB (n=3, alpha=false)
/// from `render_for_ocr`, but this handles the other shapes defensively so a
/// directly-constructed Pixmap never panics.
pub(crate) fn pixmap_to_rgb(pix: &Pixmap) -> RgbImage {
    let (w, h) = (pix.width, pix.height);
    let samples = pix.samples();
    let n = pix.n as usize;
    let color = pix.colorspace.components() as usize; // 1 / 3 / 4 (excludes alpha)
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
            let _ = color; // `color` documents intent; pixels read explicitly above.
            img.put_pixel(x, y, image::Rgb(px));
        }
    }
    img
}

/// Normalizes an RGB image of exact size `w×h` into a `[1,3,h,w]` f32 tensor,
/// applying `(px/255 - mean[c]) / std[c]` per channel (channel order R,G,B).
pub(crate) fn to_tensor(img: &RgbImage, mean: [f32; 3], std: [f32; 3]) -> Tensor {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let arr = tract_ndarray::Array4::<f32>::from_shape_fn((1, 3, h, w), |(_, c, y, x)| {
        let p = img.get_pixel(x as u32, y as u32)[c] as f32 / 255.0;
        (p - mean[c]) / std[c]
    });
    arr.into_tensor()
}

/// Resizes `img` to exactly `(w, h)` with triangle (bilinear) filtering.
#[inline]
pub(crate) fn resize_exact(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    image::imageops::resize(img, w.max(1), h.max(1), FilterType::Triangle)
}

/// Extracts a rotated-rectangle region and renders it upright (horizontal) via
/// bilinear sampling along the rect's axes.
///
/// `quad` are the four corners in image pixel coordinates ordered top-left,
/// top-right, bottom-right, bottom-left along the rect's own axes (as produced
/// by `detect::unclip_rect`). The output width/height are the rect's side
/// lengths, so the text reads left-to-right in the result. A degenerate rect
/// yields a 1×1 image so downstream resize never sees a zero dimension.
pub(crate) fn crop_rotated(img: &RgbImage, quad: &[(f32, f32); 4]) -> RgbImage {
    let (tl, tr, br, bl) = (quad[0], quad[1], quad[2], quad[3]);
    // Side lengths: top/bottom → width axis, left/right → height axis.
    let dist = |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    let out_w = (dist(tl, tr).max(dist(bl, br))).round().max(1.0) as u32;
    let out_h = (dist(tl, bl).max(dist(tr, br))).round().max(1.0) as u32;
    if out_w <= 1 && out_h <= 1 {
        return RgbImage::new(1, 1);
    }
    let mut out = RgbImage::new(out_w, out_h);
    let iw = img.width() as i32;
    let ih = img.height() as i32;
    let fw = (out_w.max(2) - 1) as f32;
    let fh = (out_h.max(2) - 1) as f32;
    for oy in 0..out_h {
        let ty = oy as f32 / fh; // 0..1 along height (top→bottom)
                                 // Interpolate the left and right edges, then across.
        let lx = tl.0 + (bl.0 - tl.0) * ty;
        let ly = tl.1 + (bl.1 - tl.1) * ty;
        let rx = tr.0 + (br.0 - tr.0) * ty;
        let ry = tr.1 + (br.1 - tr.1) * ty;
        for ox in 0..out_w {
            let tx = ox as f32 / fw; // 0..1 along width (left→right)
            let sx = lx + (rx - lx) * tx;
            let sy = ly + (ry - ly) * tx;
            out.put_pixel(ox, oy, sample_bilinear(img, sx, sy, iw, ih));
        }
    }
    out
}

/// Bilinearly samples `img` at fractional `(x,y)`, clamping to image bounds.
fn sample_bilinear(img: &RgbImage, x: f32, y: f32, iw: i32, ih: i32) -> image::Rgb<u8> {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let at = |xx: i32, yy: i32| -> [f32; 3] {
        let cx = xx.clamp(0, iw - 1).max(0) as u32;
        let cy = yy.clamp(0, ih - 1).max(0) as u32;
        let p = img.get_pixel(cx, cy);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    let p00 = at(x0, y0);
    let p10 = at(x0 + 1, y0);
    let p01 = at(x0, y0 + 1);
    let p11 = at(x0 + 1, y0 + 1);
    let mut out = [0u8; 3];
    for c in 0..3 {
        let top = p00[c] + (p10[c] - p00[c]) * fx;
        let bot = p01[c] + (p11[c] - p01[c]) * fx;
        out[c] = (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8;
    }
    image::Rgb(out)
}

/// Crops an axis-aligned region from `img`, clamped to image bounds. Returns a
/// new owned `RgbImage`; an empty/degenerate region yields a 1×1 image so the
/// downstream resize never sees a zero dimension.
pub(crate) fn crop(img: &RgbImage, x0: i32, y0: i32, x1: i32, y1: i32) -> RgbImage {
    let iw = img.width() as i32;
    let ih = img.height() as i32;
    let cx0 = x0.clamp(0, iw);
    let cy0 = y0.clamp(0, ih);
    let cx1 = x1.clamp(0, iw);
    let cy1 = y1.clamp(0, ih);
    let cw = (cx1 - cx0).max(0) as u32;
    let ch = (cy1 - cy0).max(0) as u32;
    if cw == 0 || ch == 0 {
        return RgbImage::new(1, 1);
    }
    let mut out = RgbImage::new(cw, ch);
    for y in 0..ch {
        for x in 0..cw {
            out.put_pixel(x, y, *img.get_pixel(cx0 as u32 + x, cy0 as u32 + y));
        }
    }
    out
}
