//! The raster target — a [`Canvas`] wrapping the rasterizer's pixmap (M6a).
//!
//! Module owner: **M6a** (vector + canvas). The struct shape and the four method
//! signatures below are the frozen contract the other modules paint onto
//! ([`crate::vector`], [`crate::text`], [`crate::image`] all take `&mut Canvas`);
//! M6a fills in the bodies. See `ARCHITECTURE.md`.

use tiny_skia::{Mask, Pixmap as SkPixmap, Transform};

use pdf_core::geom::Matrix;
use pdf_image::pixmap::{Colorspace, Pixmap};

use crate::error::{Error, Result};

/// A rasterization target: an anti-aliased RGBA pixel buffer plus the base
/// device transform that maps PDF user space onto it.
///
/// Wraps a [`tiny_skia::Pixmap`] (premultiplied sRGBA, top-left origin). The
/// `base_transform` is the page→device matrix derived from the render matrix /
/// DPI and the y-flip (PDF is bottom-left origin, the pixmap is top-left); every
/// drawcall composes its own CTM on top of it.
///
/// Converting to the public output type is [`Canvas::into_pixmap`], which
/// un-premultiplies and maps to the requested [`Colorspace`].
pub struct Canvas {
    /// The backing rasterizer pixmap (premultiplied sRGBA8, top-left origin).
    pixmap: SkPixmap,
    /// Page user-space → device-pixel transform (DPI/scale + y-flip baked in).
    base_transform: Matrix,
    /// The colorspace the final [`Pixmap`] should be produced in.
    out_colorspace: Colorspace,
    /// Whether the final [`Pixmap`] should carry an alpha channel.
    out_alpha: bool,
    /// The current clip mask (an 8-bit coverage mask in device pixels), or
    /// `None` for an unclipped canvas. Intersected by [`crate::vector::set_clip`]
    /// and snapshotted by [`Canvas::save`] / [`Canvas::restore`] (PDF `q`/`Q`).
    clip: Option<Mask>,
    /// The saved graphics-state clip snapshots (the `q`/`Q` clip stack).
    clip_stack: Vec<Option<Mask>>,
}

impl Canvas {
    /// Builds a blank `w × h` device-pixel canvas.
    ///
    /// `base_transform` maps PDF user space to device pixels (DPI/scale + y-flip
    /// already composed). `out_colorspace`/`out_alpha` select the format of the
    /// [`Pixmap`] that [`Canvas::into_pixmap`] later produces. The backing buffer
    /// starts fully transparent (the caller fills a background if opaque output
    /// is wanted).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a zero dimension, or
    /// [`Error::LimitExceeded`] if `w × h` exceeds the render-target ceiling
    /// (the rasterizer cannot allocate the buffer).
    pub fn blank(
        width: u32,
        height: u32,
        base_transform: Matrix,
        out_colorspace: Colorspace,
        out_alpha: bool,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidArgument("zero canvas dimension"));
        }
        let pixmap =
            SkPixmap::new(width, height).ok_or(Error::LimitExceeded("render target too large"))?;
        Ok(Self {
            pixmap,
            base_transform,
            out_colorspace,
            out_alpha,
            clip: None,
            clip_stack: Vec::new(),
        })
    }

    /// The canvas width in device pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    /// The canvas height in device pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// The page user-space → device-pixel base transform.
    #[must_use]
    pub fn base_transform(&self) -> Matrix {
        self.base_transform
    }

    /// Fills the **entire** backing buffer with a solid sRGBA color, ignoring
    /// the clip (the page background paint — PDF pages render onto opaque white
    /// by default). Premultiplication is handled by the rasterizer.
    pub fn fill_background(&mut self, rgba: [u8; 4]) {
        let color = tiny_skia::Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        self.pixmap.fill(color);
    }

    /// Mutable access to the backing rasterizer pixmap (used by [`crate::vector`]
    /// / [`crate::text`] / [`crate::image`] to issue fill/stroke/composite
    /// drawcalls).
    pub(crate) fn pixmap_mut(&mut self) -> &mut SkPixmap {
        &mut self.pixmap
    }

    /// Shared access to the backing rasterizer pixmap.
    ///
    /// Part of the frozen crate API (`ARCHITECTURE.md`); consumed by the sibling
    /// render modules (M6b text / M6c image) — `allow(dead_code)` because M6a
    /// alone does not read it back.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn pixmap(&self) -> &SkPixmap {
        &self.pixmap
    }

    /// The current device-space clip mask, if any (passed to the tiny-skia
    /// `fill_path`/`stroke_path` drawcalls so painting is restricted to it).
    pub(crate) fn clip(&self) -> Option<&Mask> {
        self.clip.as_ref()
    }

    /// Composes a PDF user-space CTM with the canvas base transform and converts
    /// the result to a rasterizer [`Transform`].
    ///
    /// Both matrices use the PyMuPDF / PDF row-vector convention
    /// (`(x, y) → (a·x + c·y + e, b·x + d·y + f)`), which is exactly tiny-skia's
    /// `Transform::from_row(sx, ky, kx, sy, tx, ty)` layout. The CTM is applied
    /// first, then the base transform: `device = ctm · base_transform`.
    pub(crate) fn device_transform(&self, ctm: Matrix) -> Transform {
        let m = Matrix::concat(&ctm, &self.base_transform);
        Transform::from_row(
            m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32,
        )
    }

    /// Intersects the current clip region with `mask` (an already-rasterized
    /// device-space coverage mask). The first clip on a canvas simply adopts the
    /// mask; subsequent clips multiply coverage (clip regions only shrink).
    pub(crate) fn intersect_clip(&mut self, mask: Mask) {
        match &mut self.clip {
            Some(existing) => {
                for (a, b) in existing.data_mut().iter_mut().zip(mask.data().iter()) {
                    // Premultiply-style coverage intersection: a·b / 255.
                    *a = ((u16::from(*a) * u16::from(*b) + 127) / 255) as u8;
                }
            }
            None => self.clip = Some(mask),
        }
    }

    /// Saves the current clip state (PDF `q`). Pairs with [`Canvas::restore`].
    ///
    /// Part of the frozen crate API consumed by the M6d page driver (`render.rs`
    /// q/Q handling); `allow(dead_code)` as M6a exercises it only in unit tests.
    #[allow(dead_code)]
    pub(crate) fn save(&mut self) {
        self.clip_stack.push(self.clip.clone());
    }

    /// Restores the most recently saved clip state (PDF `Q`). A `Q` without a
    /// matching `q` is a tolerant no-op.
    ///
    /// Part of the frozen crate API consumed by the M6d page driver; see
    /// [`Canvas::save`].
    #[allow(dead_code)]
    pub(crate) fn restore(&mut self) {
        if let Some(prev) = self.clip_stack.pop() {
            self.clip = prev;
        }
    }

    /// Consumes the canvas and produces the public output [`Pixmap`] in the
    /// configured colorspace, un-premultiplying alpha and dropping the alpha
    /// channel when `out_alpha` is false (PRD §8.10 sample format).
    ///
    /// The backing tiny-skia pixmap holds premultiplied sRGBA8; each pixel is
    /// demultiplied to straight sRGBA, then mapped to the requested colorspace
    /// (Gray via the Rec.601 luma of the straight RGB; CMYK via a naive
    /// `c = 1 - r` style inversion with `k` factored out — adequate for the
    /// scaffold, ICC deferred per `ARCHITECTURE.md`).
    ///
    /// # Errors
    ///
    /// [`Error::Image`] if the output [`Pixmap`] cannot be constructed (it never
    /// is for a valid canvas — the sample buffer is sized exactly).
    pub fn into_pixmap(self) -> Result<Pixmap> {
        let width = self.pixmap.width();
        let height = self.pixmap.height();
        let cs = self.out_colorspace;
        let alpha = self.out_alpha;
        // Only the three Device colorspaces have a defined sample mapping here.
        if !matches!(cs, Colorspace::Rgb | Colorspace::Gray | Colorspace::Cmyk) {
            return Err(Error::Unsupported("Canvas::into_pixmap colorspace"));
        }
        let n = cs.components() as usize + usize::from(alpha);
        let mut samples = Vec::with_capacity(width as usize * height as usize * n);

        for px in self.pixmap.pixels() {
            let c = px.demultiply();
            let (r, g, b, a) = (c.red(), c.green(), c.blue(), c.alpha());
            match cs {
                Colorspace::Rgb => {
                    samples.push(r);
                    samples.push(g);
                    samples.push(b);
                }
                Colorspace::Gray => {
                    // Rec.601 luma of the straight (un-premultiplied) color.
                    let y = (0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b))
                        .round()
                        .clamp(0.0, 255.0) as u8;
                    samples.push(y);
                }
                Colorspace::Cmyk => {
                    let rf = f32::from(r) / 255.0;
                    let gf = f32::from(g) / 255.0;
                    let bf = f32::from(b) / 255.0;
                    let k = 1.0 - rf.max(gf).max(bf);
                    let inv = 1.0 - k;
                    let (cc, mm, yy) = if inv <= f32::EPSILON {
                        (0.0, 0.0, 0.0)
                    } else {
                        (
                            (1.0 - rf - k) / inv,
                            (1.0 - gf - k) / inv,
                            (1.0 - bf - k) / inv,
                        )
                    };
                    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    samples.push(q(cc));
                    samples.push(q(mm));
                    samples.push(q(yy));
                    samples.push(q(k));
                }
                _ => unreachable!("colorspace guarded above"),
            }
            if alpha {
                samples.push(a);
            }
        }

        Ok(Pixmap::try_new(width, height, cs, alpha, samples)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Mask;

    /// RENDER-VEC-CANVAS-SAVE-RESTORE: `save`/`restore` snapshot and pop the
    /// clip stack (the q/Q contract M6d drives). A `Q` without a `q` is a no-op.
    #[test]
    fn save_restore_clip_stack() {
        let mut c = Canvas::blank(10, 10, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
        assert!(c.clip().is_none());

        c.save(); // snapshot the (None) clip
        let mut m = Mask::new(10, 10).unwrap();
        m.data_mut().iter_mut().for_each(|b| *b = 255);
        c.intersect_clip(m);
        assert!(c.clip().is_some(), "clip set after intersect");

        c.restore(); // back to the snapshot
        assert!(c.clip().is_none(), "clip restored to None by Q");

        // An extra restore (Q without q) is tolerant.
        c.restore();
        assert!(c.clip().is_none());
    }

    /// RENDER-VEC-CANVAS-INTERSECT: a second clip only shrinks coverage — a
    /// half-coverage mask intersected with full coverage stays ~half.
    #[test]
    fn intersect_clip_multiplies_coverage() {
        let mut c = Canvas::blank(4, 4, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
        let mut full = Mask::new(4, 4).unwrap();
        full.data_mut().iter_mut().for_each(|b| *b = 255);
        c.intersect_clip(full);
        let mut half = Mask::new(4, 4).unwrap();
        half.data_mut().iter_mut().for_each(|b| *b = 128);
        c.intersect_clip(half);
        let clip = c.clip().unwrap();
        assert!(clip.data().iter().all(|&b| b == 128));
    }

    /// RENDER-VEC-CANVAS-PIXMAP-GRAY: a known premultiplied RGB pixel converts
    /// to the Rec.601 luma when the output colorspace is Gray.
    #[test]
    fn into_pixmap_gray_luma() {
        let mut c = Canvas::blank(1, 1, Matrix::IDENTITY, Colorspace::Gray, false).unwrap();
        // Paint the single pixel opaque red (255,0,0).
        let red = tiny_skia::Color::from_rgba8(255, 0, 0, 255);
        c.pixmap_mut().fill(red);
        let pm = c.into_pixmap().unwrap();
        assert_eq!(pm.colorspace, Colorspace::Gray);
        assert_eq!(pm.n, 1);
        // round(0.299*255) = 76.
        assert_eq!(pm.samples[0], 76);
    }
}
