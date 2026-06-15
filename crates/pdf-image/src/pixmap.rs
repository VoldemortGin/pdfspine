//! `Pixmap` — the decoded-raster type for image documents and image-only PDF
//! pages (PRD §3.3 / §8.10).
//!
//! A `Pixmap` is the in-scope "render" output of the image path: a contiguous
//! 8-bit interleaved sample buffer plus its geometry/colorspace. It is produced
//! from a [`crate::codecs::DecodedImage`] (image XObject) or directly from an
//! image-document decode, and is what the Python `Pixmap` (buffer-protocol /
//! numpy, `save`/`tobytes`) wraps. Vector-page rasterization is M6 and out of
//! scope here. Implemented in the M5-pixmap unit; this is the compiling stub.

use crate::error::{Error, Result};

/// The colorspace of a [`Pixmap`]'s samples, matching the PyMuPDF model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Colorspace {
    /// 1 component per pixel.
    Gray,
    /// 3 components per pixel (R,G,B).
    Rgb,
    /// 4 components per pixel (C,M,Y,K).
    Cmyk,
}

impl Colorspace {
    /// Number of color components (excluding any alpha) for this colorspace.
    #[must_use]
    pub fn components(self) -> u8 {
        match self {
            Colorspace::Gray => 1,
            Colorspace::Rgb => 3,
            Colorspace::Cmyk => 4,
        }
    }
}

/// A decoded raster: 8-bit interleaved samples plus geometry and colorspace.
///
/// Field layout matches PRD §8.10 (`width, height, n, alpha, stride, samples,
/// colorspace`). `n` is the total sample count per pixel **including** alpha
/// (i.e. `colorspace.components() + alpha as u8`); `stride` is the byte length of
/// one pixel row (`width * n`, no extra padding in v1).
#[derive(Clone, Debug)]
pub struct Pixmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Components per pixel including alpha (PyMuPDF `n`).
    pub n: u8,
    /// Whether the last component is an alpha channel.
    pub alpha: bool,
    /// Bytes per row (`width * n` in v1; no row padding).
    pub stride: usize,
    /// Interleaved 8-bit samples, row-major, `stride * height` bytes.
    pub samples: Vec<u8>,
    /// The colorspace of the color components (excludes alpha).
    pub colorspace: Colorspace,
}

impl Pixmap {
    /// Constructs a [`Pixmap`] from raw 8-bit interleaved samples.
    ///
    /// `colorspace` + `alpha` determine `n` and `stride`. The scaffold computes
    /// `n`/`stride` and stores the buffer without validating its length against
    /// `width`/`height` (the M5-pixmap implementer adds that check and may make
    /// this fallible if it prefers — see `ARCHITECTURE.md` for the contract).
    #[must_use]
    pub fn new(
        width: u32,
        height: u32,
        colorspace: Colorspace,
        alpha: bool,
        samples: Vec<u8>,
    ) -> Self {
        let n = colorspace.components() + u8::from(alpha);
        let stride = width as usize * n as usize;
        Pixmap {
            width,
            height,
            n,
            alpha,
            stride,
            samples,
            colorspace,
        }
    }

    /// Borrows the interleaved sample buffer (PyMuPDF `Pixmap.samples`).
    #[must_use]
    pub fn samples(&self) -> &[u8] {
        &self.samples
    }

    /// Encodes this pixmap as a PNG into `out` (PyMuPDF `Pixmap.save(...)` /
    /// `tobytes("png")`).
    ///
    /// Stub: returns [`Error::Unsupported`] (`"Pixmap::save_png"`) — panic-free.
    pub fn save_png(&self, _out: &mut Vec<u8>) -> Result<()> {
        Err(Error::Unsupported("Pixmap::save_png"))
    }
}
