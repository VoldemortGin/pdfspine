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
