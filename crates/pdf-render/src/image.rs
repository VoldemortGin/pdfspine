//! Image & shading rendering — composite a decoded image under a CTM (M6c).
//!
//! Module owner: **M6c** (image + shading/pattern). Maps a decoded
//! [`pdf_image::pixmap::Pixmap`] (the unit square `[0,1]²` in image space) onto
//! the page via the placement matrix and paints it on the [`Canvas`], and
//! evaluates axial/radial shadings into tiny-skia gradients.
//!
//! ## The image affine
//!
//! A PDF image source maps the **unit square** `[0,1]×[0,1]` onto the page via
//! the image-placement matrix (`ctm`), with sample **row 0 at the top** of that
//! square (image space is y-down, the unit square is y-up). The full
//! pixel-space → device transform composed here is
//!
//! ```text
//! pixel_to_device = pixel_to_unit · ctm · base_transform
//! ```
//!
//! where `pixel_to_unit` maps pixel `(px, py)` to `(px/W, 1 − py/H)` (the y-flip
//! that puts row 0 at `v = 1`). tiny-skia's [`Canvas::pixmap_mut`]
//! `draw_pixmap(0, 0, …, transform, …)` then fills the pixel-space rect
//! `(0,0,W,H)` mapped by that transform, sampling the source with **bilinear**
//! filtering (nearest as the degenerate fallback).
//!
//! ## Colorspace → RGBA
//!
//! The source [`Pixmap`] (Gray / RGB / CMYK, with an optional `/SMask` alpha
//! plane) is converted to tiny-skia **premultiplied RGBA8** first: Gray expands
//! to `R=G=B`, CMYK uses the crate's naive additive complement, and any alpha
//! channel premultiplies the color. Stencil image masks paint a constant fill
//! color through the 1-bpp mask.
//!
//! ## Deferrals
//!
//! Tiling patterns (type 1), PostScript (type 4) functions, shading types 1/4–7
//! (function-based / mesh), transparency-group isolation/knockout, and blend
//! modes other than normal `SrcOver` are **deferred** (documented gaps; the
//! orchestrator wires the interpreter side in M6d).

use pdf_core::geom::Matrix;
use pdf_image::pixmap::{Colorspace, Pixmap};
use tiny_skia::{
    FilterQuality, GradientStop, LinearGradient, Paint as SkPaint, PixmapPaint, PixmapRef,
    RadialGradient, Rect as SkRect, Shader, SpreadMode, Transform,
};

use crate::canvas::Canvas;
use crate::error::{Error, Result};
use crate::vector::Paint;

/// Reads device pixel `(x, y)` back as **un-premultiplied** RGBA8, or `None` if
/// out of bounds.
///
/// Test-support entry for M6c: lets image/shading tests verify composited
/// device pixels without depending on M6a's [`Canvas::into_pixmap`] (still a
/// stub during parallel development). Accesses the canvas's backing pixmap via
/// the frozen `pub(crate)` accessor.
#[must_use]
pub fn sample_device_rgba(canvas: &Canvas, x: u32, y: u32) -> Option<[u8; 4]> {
    let c = canvas.pixmap().pixel(x, y)?.demultiply();
    Some([c.red(), c.green(), c.blue(), c.alpha()])
}

/// Composites the decoded `image` onto `canvas`.
///
/// `ctm` maps the unit square onto the page (the image-placement matrix); it is
/// composed with the canvas base transform and the pixel→unit y-flip to build
/// the pixel-space → device affine. `alpha` (0–255) is an extra constant-alpha
/// factor (e.g. graphics-state `ca`). The source pixmap's colorspace and any
/// `/SMask` alpha plane are honored. Placement outside the canvas is clipped by
/// the rasterizer (no panic).
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a zero-dimension source pixmap. A degenerate
/// or non-finite CTM is treated as a no-op (nothing is painted).
pub fn draw_image(canvas: &mut Canvas, image: &Pixmap, ctm: Matrix, alpha: u8) -> Result<()> {
    if image.width == 0 || image.height == 0 {
        return Err(Error::InvalidArgument("zero image dimension"));
    }
    if alpha == 0 {
        return Ok(());
    }
    let Some(transform) = pixel_to_device(image.width, image.height, ctm, canvas.base_transform())
    else {
        return Ok(()); // degenerate / non-finite placement: nothing to paint.
    };

    let rgba = pixmap_to_skia_rgba(image);
    let Some(src) = rgba else {
        return Ok(());
    };
    let src_ref = PixmapRef::from_bytes(&src, image.width, image.height)
        .ok_or(Error::InvalidArgument("image pixmap build failed"))?;

    let paint = PixmapPaint {
        opacity: alpha as f32 / 255.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: FilterQuality::Bilinear,
    };
    canvas
        .pixmap_mut()
        .draw_pixmap(0, 0, src_ref, &paint, transform, None);
    Ok(())
}

/// Paints the current fill `color` through a 1-bpp **stencil** image mask
/// (`/ImageMask true`).
///
/// `bits` is the packed 1-bit-per-pixel mask, MSB-first, **rows byte-aligned**
/// (the PDF sample layout). With the default `/Decode [0 1]`, a sample of `0`
/// means "paint" and `1` means "leave transparent"; `decode_inverted` (a
/// `/Decode [1 0]` array on the image dict) swaps that mapping so a sample of
/// `1` paints and `0` is transparent. `ctm`/`alpha` behave as in [`draw_image`].
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a zero dimension or a `bits` buffer too short.
#[allow(clippy::too_many_arguments)]
pub fn draw_image_mask(
    canvas: &mut Canvas,
    bits: &[u8],
    width: u32,
    height: u32,
    color: Paint,
    ctm: Matrix,
    alpha: u8,
    decode_inverted: bool,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::InvalidArgument("zero image dimension"));
    }
    let row_bytes = (width as usize).div_ceil(8);
    if bits.len() < row_bytes * height as usize {
        return Err(Error::InvalidArgument("stencil mask buffer too short"));
    }
    if alpha == 0 {
        return Ok(());
    }
    let Some(transform) = pixel_to_device(width, height, ctm, canvas.base_transform()) else {
        return Ok(());
    };

    // Build a premultiplied RGBA stencil: fill color where the sample says
    // "paint", else clear. The default `/Decode [0 1]` paints where the bit is
    // 0; `/Decode [1 0]` (`decode_inverted`) paints where the bit is 1.
    let [r, g, b, _] = color.rgba;
    let paint_bit = u8::from(decode_inverted);
    let mut out = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let row = &bits[y * row_bytes..y * row_bytes + row_bytes];
        for x in 0..width as usize {
            let byte = row[x / 8];
            let bit = (byte >> (7 - (x % 8))) & 1;
            if bit == paint_bit {
                let o = (y * width as usize + x) * 4;
                // Fully opaque; premultiplied == straight at a == 255.
                out[o] = r;
                out[o + 1] = g;
                out[o + 2] = b;
                out[o + 3] = 255;
            }
        }
    }
    let src = PixmapRef::from_bytes(&out, width, height)
        .ok_or(Error::InvalidArgument("stencil pixmap build failed"))?;
    let paint = PixmapPaint {
        opacity: alpha as f32 / 255.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: FilterQuality::Bilinear,
    };
    canvas
        .pixmap_mut()
        .draw_pixmap(0, 0, src, &paint, transform, None);
    Ok(())
}

/// Builds the pixel-space → device [`Transform`], or `None` if the result is
/// degenerate (non-invertible / non-finite) and should be skipped.
fn pixel_to_device(w: u32, h: u32, ctm: Matrix, base: Matrix) -> Option<Transform> {
    // pixel (px,py) -> unit (px/w, 1 - py/h): row 0 at the top of the square.
    let pixel_to_unit = Matrix::new(1.0 / w as f64, 0.0, 0.0, -1.0 / h as f64, 0.0, 1.0);
    let full = pixel_to_unit * ctm * base;
    let t = Transform::from_row(
        full.a as f32,
        full.b as f32,
        full.c as f32,
        full.d as f32,
        full.e as f32,
        full.f as f32,
    );
    if !transform_is_finite(&t) {
        return None;
    }
    // A non-invertible (collapsed) transform paints nothing useful.
    if (full.a * full.d - full.b * full.c).abs() < 1e-12 {
        return None;
    }
    Some(t)
}

fn transform_is_finite(t: &Transform) -> bool {
    [t.sx, t.ky, t.kx, t.sy, t.tx, t.ty]
        .iter()
        .all(|v| v.is_finite())
}

/// Converts a source [`Pixmap`] to tiny-skia premultiplied RGBA8 bytes, or
/// `None` if the buffer is malformed (defensive; the codec layer validates).
fn pixmap_to_skia_rgba(image: &Pixmap) -> Option<Vec<u8>> {
    let w = image.width as usize;
    let h = image.height as usize;
    let n = image.n as usize;
    let comps = image.colorspace.components() as usize;
    let src = image.samples();
    if src.len() < w * h * n {
        return None;
    }
    let mut out = vec![0u8; w * h * 4];
    for (i, px) in src.chunks_exact(n).take(w * h).enumerate() {
        let (r, g, b) = match image.colorspace {
            Colorspace::Gray => (px[0], px[0], px[0]),
            Colorspace::Rgb => (px[0], px[1], px[2]),
            Colorspace::Cmyk => cmyk_to_rgb(px[0], px[1], px[2], px[3]),
            // Future colorspaces: treat the first component as gray (defensive).
            _ => (px[0], px[0], px[0]),
        };
        let a = if image.alpha { px[comps] } else { 255 };
        let o = i * 4;
        // Premultiply.
        out[o] = premul(r, a);
        out[o + 1] = premul(g, a);
        out[o + 2] = premul(b, a);
        out[o + 3] = a;
    }
    Some(out)
}

#[inline]
fn premul(c: u8, a: u8) -> u8 {
    ((c as u16 * a as u16 + 127) / 255) as u8
}

/// CMYK → RGB, analytic (non-ICC) with a SWOP-like **black point**, matching
/// the `pdf_core::colorspace` vector path: the K axis maps white → a per-channel
/// ink floor (fitz's darkest-K `(34,31,31)`) rather than → 0, then is scaled by
/// the CMY ink complement. A zero floor reduces this to the prior naive additive
/// complement, so only the neutral/black region shifts toward fitz. Full
/// ICC-accurate conversion (saturated-primary absorption) is deferred.
#[inline]
fn cmyk_to_rgb(c: u8, m: u8, y: u8, k: u8) -> (u8, u8, u8) {
    // Per-channel K floor (fitz SWOP darkest-K `(34,31,31)`).
    let ch = |ink: u8, floor: u16| -> u8 {
        // k_axis = floor + (255-floor)*(255-k)/255, in 0..=255.
        let k_axis = floor + (255 - floor) * (255 - k as u16) / 255;
        (k_axis * (255 - ink as u16) / 255) as u8
    };
    (ch(c, 34), ch(m, 31), ch(y, 31))
}

// === Shadings (sh operator + shading patterns, types 2 & 3) ================

// The shading color + PDF-function evaluator now live in the shared
// `pdf_core::colorspace` module (one evaluator for the image, shading and vector
// `scn` paths — P3-3); re-exported here so the existing `pdf_render::image::{…}`
// API (used by the shading draw fns + tests) is unchanged.
pub use pdf_core::colorspace::{PdfFunction, ShadingColor};

/// Fills the canvas with an **axial** (type 2) shading: a linear gradient from
/// `start` to `end` (in shading/user space) whose color ramps via `func`.
///
/// `cs` is the shading colorspace (selects the component → RGB mapping).
/// `extend` is the `/Extend [e0 e1]` pair (clamp past the endpoints). `ctm`
/// composes onto the base transform; `alpha` is a constant-alpha factor. The
/// whole canvas is filled (the caller clips to the shading region in M6d).
///
/// # Errors
///
/// Never errors for finite input; a degenerate axis (`start == end`) is a no-op.
#[allow(clippy::too_many_arguments)]
pub fn draw_axial_shading(
    canvas: &mut Canvas,
    start: (f64, f64),
    end: (f64, f64),
    func: &PdfFunction,
    cs: Colorspace,
    extend: (bool, bool),
    ctm: Matrix,
    alpha: u8,
) -> Result<()> {
    if alpha == 0 {
        return Ok(());
    }
    let Some(grad_xf) = user_to_device(canvas, ctm) else {
        return Ok(());
    };
    let stops = build_stops(func, cs, alpha);
    let p0 = tiny_skia::Point::from_xy(start.0 as f32, start.1 as f32);
    let p1 = tiny_skia::Point::from_xy(end.0 as f32, end.1 as f32);
    let spread = spread_mode(extend);
    let Some(shader) = LinearGradient::new(p0, p1, stops, spread, grad_xf) else {
        return Ok(()); // degenerate axis: tiny-skia returns None.
    };
    fill_canvas_with_shader(canvas, shader)
}

/// Fills the canvas with a **radial** (type 3) shading: two circles
/// `start = (cx0, cy0, r0)` → `end = (cx1, cy1, r1)` ramped via `func`.
///
/// tiny-skia's radial gradient is concentric (shared center), so a moving center
/// is approximated by the end circle's center (documented gap for the rare
/// non-concentric case). Other parameters match [`draw_axial_shading`].
///
/// # Errors
///
/// Never errors for finite input; a zero-radius outer circle is a no-op.
#[allow(clippy::too_many_arguments)]
pub fn draw_radial_shading(
    canvas: &mut Canvas,
    start: (f64, f64, f64),
    end: (f64, f64, f64),
    func: &PdfFunction,
    cs: Colorspace,
    extend: (bool, bool),
    ctm: Matrix,
    alpha: u8,
) -> Result<()> {
    if alpha == 0 {
        return Ok(());
    }
    let Some(grad_xf) = user_to_device(canvas, ctm) else {
        return Ok(());
    };
    let stops = build_stops(func, cs, alpha);
    // Concentric approximation: gradient centered at the end circle, radius r1.
    let center = tiny_skia::Point::from_xy(end.0 as f32, end.1 as f32);
    let focal = tiny_skia::Point::from_xy(start.0 as f32, start.1 as f32);
    let radius = end.2.max(start.2) as f32;
    if radius <= 0.0 {
        return Ok(());
    }
    let spread = spread_mode(extend);
    let Some(shader) = RadialGradient::new(focal, center, radius, stops, spread, grad_xf) else {
        return Ok(());
    };
    fill_canvas_with_shader(canvas, shader)
}

/// The user→device [`Transform`] (`ctm · base`), carried by the shader so it
/// maps user-space gradient geometry into device pixels. `None` if non-finite.
fn user_to_device(canvas: &Canvas, ctm: Matrix) -> Option<Transform> {
    let full = ctm * canvas.base_transform();
    let t = Transform::from_row(
        full.a as f32,
        full.b as f32,
        full.c as f32,
        full.d as f32,
        full.e as f32,
        full.f as f32,
    );
    transform_is_finite(&t).then_some(t)
}

/// Fills the whole device canvas with `shader` (which already carries the
/// user→device transform and the baked-in stop alpha).
fn fill_canvas_with_shader(canvas: &mut Canvas, shader: Shader) -> Result<()> {
    let (w, h) = (canvas.width(), canvas.height());
    let Some(rect) = SkRect::from_xywh(0.0, 0.0, w as f32, h as f32) else {
        return Ok(());
    };
    let paint = SkPaint {
        shader,
        anti_alias: true,
        ..SkPaint::default()
    };
    canvas
        .pixmap_mut()
        .fill_rect(rect, &paint, Transform::identity(), None);
    Ok(())
}

/// Builds gradient stops by sampling `func` across `[0, 1]`, mapping each shading
/// color to RGB and folding the constant `alpha` into the stop alpha.
fn build_stops(func: &PdfFunction, cs: Colorspace, alpha: u8) -> Vec<GradientStop> {
    const N: usize = 32;
    let mut stops = Vec::with_capacity(N + 1);
    for i in 0..=N {
        let t = i as f32 / N as f32;
        let ShadingColor(comps) = func.eval(t);
        let (r, g, b) = shading_color_to_rgb(&comps, cs);
        stops.push(GradientStop::new(
            t,
            tiny_skia::Color::from_rgba8(r, g, b, alpha),
        ));
    }
    stops
}

/// Maps a normalized shading color (1–4 components) to 8-bit RGB.
fn shading_color_to_rgb(comps: &[f32], cs: Colorspace) -> (u8, u8, u8) {
    let q = |v: f32| (clamp(v, 0.0, 1.0) * 255.0 + 0.5) as u8;
    match cs {
        Colorspace::Gray => {
            let g = q(*comps.first().unwrap_or(&0.0));
            (g, g, g)
        }
        Colorspace::Rgb => (
            q(*comps.first().unwrap_or(&0.0)),
            q(*comps.get(1).unwrap_or(&0.0)),
            q(*comps.get(2).unwrap_or(&0.0)),
        ),
        Colorspace::Cmyk => {
            let c = q(*comps.first().unwrap_or(&0.0));
            let m = q(*comps.get(1).unwrap_or(&0.0));
            let y = q(*comps.get(2).unwrap_or(&0.0));
            let k = q(*comps.get(3).unwrap_or(&0.0));
            cmyk_to_rgb(c, m, y, k)
        }
        // Future colorspaces: treat the first component as gray (defensive).
        _ => {
            let g = q(*comps.first().unwrap_or(&0.0));
            (g, g, g)
        }
    }
}

fn spread_mode(extend: (bool, bool)) -> SpreadMode {
    // PDF /Extend clamps past the endpoints; map "extend both/either" to Pad,
    // and no-extend to Pad as well (tiny-skia has no transparent-outside mode;
    // the caller clips the shading region). Documented simplification.
    let _ = extend;
    SpreadMode::Pad
}

#[inline]
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::cmyk_to_rgb;

    #[test]
    fn cmyk_black_point_matches_fitz() {
        // Pure process black → fitz SWOP darkest-K `(34,31,31)` (P3-3r), not
        // pure black — matches the `pdf_core::colorspace` vector path.
        assert_eq!(cmyk_to_rgb(0, 0, 0, 255), (34, 31, 31));
        // Registration black (full ink) still → (0,0,0).
        assert_eq!(cmyk_to_rgb(255, 255, 255, 255), (0, 0, 0));
        // No ink → white.
        assert_eq!(cmyk_to_rgb(0, 0, 0, 0), (255, 255, 255));
        // Pure CMY primaries keep the naive complement (K axis untouched).
        assert_eq!(cmyk_to_rgb(255, 0, 0, 0), (0, 255, 255));
        assert_eq!(cmyk_to_rgb(0, 0, 255, 0), (255, 255, 0));
    }
}
