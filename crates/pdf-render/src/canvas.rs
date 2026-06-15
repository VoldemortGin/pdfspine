//! The raster target — a [`Canvas`] wrapping the rasterizer's pixmap (M6a).
//!
//! Module owner: **M6a** (vector + canvas). The struct shape and the four method
//! signatures below are the frozen contract the other modules paint onto
//! ([`crate::vector`], [`crate::text`], [`crate::image`] all take `&mut Canvas`);
//! M6a fills in the bodies. See `ARCHITECTURE.md`.

use tiny_skia::Pixmap as SkPixmap;

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

    /// Mutable access to the backing rasterizer pixmap (used by [`crate::vector`]
    /// / [`crate::text`] / [`crate::image`] to issue fill/stroke/composite
    /// drawcalls).
    pub(crate) fn pixmap_mut(&mut self) -> &mut SkPixmap {
        &mut self.pixmap
    }

    /// Shared access to the backing rasterizer pixmap.
    #[must_use]
    pub(crate) fn pixmap(&self) -> &SkPixmap {
        &self.pixmap
    }

    /// Consumes the canvas and produces the public output [`Pixmap`] in the
    /// configured colorspace, un-premultiplying alpha and dropping the alpha
    /// channel when `out_alpha` is false (PRD §8.10 sample format).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Unsupported`] until M6 implements the
    /// premultiplied-sRGBA → [`Colorspace`] conversion. The `out_*` fields are
    /// read here (suppressing dead-field warnings on the scaffold).
    pub fn into_pixmap(self) -> Result<Pixmap> {
        let _ = (self.pixmap(), self.out_colorspace, self.out_alpha);
        Err(Error::Unsupported("Canvas::into_pixmap"))
    }
}
