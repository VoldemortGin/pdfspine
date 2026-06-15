//! `Pixmap` — the decoded-raster type for image documents and image-only PDF
//! pages (PRD §3.3 / §8.10).
//!
//! A `Pixmap` is the in-scope "render" output of the image path: a contiguous
//! 8-bit interleaved sample buffer plus its geometry/colorspace. It is produced
//! from a [`crate::codecs::DecodedImage`] (image XObject) or directly from an
//! image-document decode, and is what the Python `Pixmap` (buffer-protocol /
//! numpy, `save`/`tobytes`) wraps. Vector-page rasterization is M6 and out of
//! scope here.
//!
//! # Sample storage and the COW buffer contract (PRD §9.4)
//!
//! Samples live in an `Arc<[u8]>` so the PyO3 layer can clone the `Arc` into a
//! `Py_buffer` and let a `memoryview` / numpy view outlive the `Pixmap`. Every
//! mutator goes through [`Pixmap::samples_mut`], which performs copy-on-write
//! (`Arc::make_mut`) — if any external clone of the `Arc` is alive (a live
//! buffer export, or another `Pixmap` sharing the buffer), the mutation lands in
//! a fresh allocation and never disturbs the bytes a view points at.

use std::sync::Arc;

use crate::codecs::DecodedImage;
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

    /// The PyMuPDF colorspace name (`"DeviceGray"` / `"DeviceRGB"` /
    /// `"DeviceCMYK"`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Colorspace::Gray => "DeviceGray",
            Colorspace::Rgb => "DeviceRGB",
            Colorspace::Cmyk => "DeviceCMYK",
        }
    }
}

/// A decoded raster: 8-bit interleaved samples plus geometry and colorspace.
///
/// Field layout matches PRD §8.10 (`width, height, n, alpha, stride, samples,
/// colorspace`). `n` is the total sample count per pixel **including** alpha
/// (i.e. `colorspace.components() + alpha as u8`); `stride` is the byte length of
/// one pixel row (`width * n`, no extra padding in v1).
///
/// `samples` is an `Arc<[u8]>` for the FFI copy-on-write buffer contract — see
/// the module docs and [`Pixmap::samples_mut`].
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
    pub samples: Arc<[u8]>,
    /// The colorspace of the color components (excludes alpha).
    pub colorspace: Colorspace,
}

impl Pixmap {
    /// Constructs a [`Pixmap`] from raw 8-bit interleaved samples.
    ///
    /// `colorspace` + `alpha` determine `n` and `stride`. The samples length is
    /// **not** validated here (callers in this crate always pass a buffer sized
    /// `width * height * n`); see [`Pixmap::try_new`] for the checked form.
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
            samples: Arc::from(samples),
            colorspace,
        }
    }

    /// Constructs a [`Pixmap`], validating that `samples.len()` equals
    /// `width * height * n`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if the sample buffer length is wrong.
    pub fn try_new(
        width: u32,
        height: u32,
        colorspace: Colorspace,
        alpha: bool,
        samples: Vec<u8>,
    ) -> Result<Self> {
        let n = colorspace.components() + u8::from(alpha);
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|p| p.checked_mul(n as usize))
            .ok_or(Error::LimitExceeded("pixmap sample size overflow"))?;
        if samples.len() != expected {
            return Err(Error::InvalidArgument("pixmap sample length mismatch"));
        }
        Ok(Self::new(width, height, colorspace, alpha, samples))
    }

    /// A blank [`Pixmap`] of the given geometry/colorspace, every byte set to
    /// `fill` (PyMuPDF `Pixmap(cs, irect)` + `clear_with`). With `alpha`, the
    /// alpha bytes are `fill` too (use [`Pixmap::clear`] / [`Pixmap::set_alpha`]
    /// to set them independently).
    ///
    /// # Errors
    ///
    /// [`Error::LimitExceeded`] on a size overflow, or [`Error::InvalidArgument`]
    /// for a zero dimension.
    pub fn blank(
        width: u32,
        height: u32,
        colorspace: Colorspace,
        alpha: bool,
        fill: u8,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidArgument("zero pixmap dimension"));
        }
        let n = colorspace.components() + u8::from(alpha);
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|p| p.checked_mul(n as usize))
            .ok_or(Error::LimitExceeded("pixmap sample size overflow"))?;
        Ok(Self::new(width, height, colorspace, alpha, vec![fill; len]))
    }

    /// Builds a [`Pixmap`] from a codec [`DecodedImage`], normalizing the samples
    /// to 8-bit interleaved Gray/RGB/CMYK (PRD §8.10).
    ///
    /// Handles the codec's `bits` (1/2/4/8/16 → upscaled to 8 bpc) and maps the
    /// component count / [`ColorSpaceHint`] to a [`Colorspace`]. The image's own
    /// alpha is not carried here (an XObject's transparency is a separate
    /// `/SMask`); the result has `alpha = false`. Use [`Pixmap::with_smask_gray`]
    /// to attach a decoded soft-mask.
    ///
    /// # Errors
    ///
    /// [`Error::Decode`] for an unsupported component count / bit depth, or a
    /// sample buffer too short for the declared geometry.
    pub fn from_decoded(img: &DecodedImage) -> Result<Self> {
        let cs = match (img.components, img.colorspace) {
            (1, _) => Colorspace::Gray,
            (3, _) => Colorspace::Rgb,
            (4, _) => Colorspace::Cmyk,
            _ => return Err(Error::decode("Pixmap", "unsupported component count")),
        };
        let samples = upsample_to_8bit(&img.data, img.width, img.height, img.components, img.bits)?;
        Self::try_new(img.width, img.height, cs, false, samples)
    }

    /// Attaches a `DeviceGray` 8-bit soft-mask plane (`/SMask`) as an alpha
    /// channel, interleaving it into a new buffer. `mask` must hold exactly
    /// `width * height` bytes; if its geometry differs it is nearest-neighbor
    /// resampled to this pixmap's size.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if this pixmap already has alpha, or `mask` is
    /// empty.
    pub fn with_smask_gray(self, mask: &[u8], mask_w: u32, mask_h: u32) -> Result<Self> {
        if self.alpha {
            return Err(Error::InvalidArgument("pixmap already has alpha"));
        }
        if mask.is_empty() || mask_w == 0 || mask_h == 0 {
            return Err(Error::InvalidArgument("empty soft-mask"));
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let comps = self.colorspace.components() as usize;
        let mut out = Vec::with_capacity(w * h * (comps + 1));
        let src = &self.samples;
        for y in 0..h {
            // Map this row to the mask's row space (nearest-neighbor).
            let my = (y * mask_h as usize) / h;
            for x in 0..w {
                let mx = (x * mask_w as usize) / w;
                let base = (y * w + x) * comps;
                out.extend_from_slice(&src[base..base + comps]);
                let a = mask[(my * mask_w as usize + mx).min(mask.len() - 1)];
                out.push(a);
            }
        }
        Self::try_new(self.width, self.height, self.colorspace, true, out)
    }

    /// Borrows the interleaved sample buffer (PyMuPDF `Pixmap.samples`).
    #[must_use]
    pub fn samples(&self) -> &[u8] {
        &self.samples
    }

    /// A clone of the backing `Arc<[u8]>` — used by the PyO3 layer to hand a
    /// buffer export an owning reference that outlives this `Pixmap` (PRD §9.4).
    #[must_use]
    pub fn samples_arc(&self) -> Arc<[u8]> {
        Arc::clone(&self.samples)
    }

    /// A mutable view of the samples, copy-on-write: if any other `Arc` clone is
    /// alive (a live buffer export or a shared `Pixmap`), this allocates a fresh
    /// buffer first so the mutation never disturbs an outstanding view (PRD §9.4).
    fn samples_mut(&mut self) -> &mut [u8] {
        Arc::make_mut(&mut self.samples)
    }

    /// The byte offset of pixel `(x, y)` within [`Pixmap::samples`], or `None`
    /// out of bounds.
    #[must_use]
    fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(y as usize * self.stride + x as usize * self.n as usize)
    }

    /// Reads pixel `(x, y)` as `n` interleaved component bytes (PyMuPDF
    /// `Pixmap.pixel`), or `None` if out of range.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Vec<u8>> {
        let off = self.pixel_offset(x, y)?;
        Some(self.samples[off..off + self.n as usize].to_vec())
    }

    /// Writes pixel `(x, y)` from `value` (one byte per component, length `n`),
    /// copy-on-write (PyMuPDF `Pixmap.set_pixel`).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an out-of-range coordinate or a `value`
    /// whose length isn't `n`.
    pub fn set_pixel(&mut self, x: u32, y: u32, value: &[u8]) -> Result<()> {
        if value.len() != self.n as usize {
            return Err(Error::InvalidArgument("pixel value length != n"));
        }
        let off = self
            .pixel_offset(x, y)
            .ok_or(Error::InvalidArgument("pixel coordinate out of range"))?;
        let n = self.n as usize;
        self.samples_mut()[off..off + n].copy_from_slice(value);
        Ok(())
    }

    /// Sets every alpha byte to `value` (no-op without an alpha channel),
    /// copy-on-write (PyMuPDF `Pixmap.set_alpha` with a constant).
    pub fn set_alpha(&mut self, value: u8) {
        if !self.alpha {
            return;
        }
        let n = self.n as usize;
        let samples = self.samples_mut();
        for px in samples.chunks_exact_mut(n) {
            px[n - 1] = value;
        }
    }

    /// Fills the whole sample buffer with `value`, copy-on-write (PyMuPDF
    /// `Pixmap.clear_with(value)`).
    pub fn clear(&mut self, value: u8) {
        self.samples_mut().fill(value);
    }

    /// Inverts the color components within `irect` (`(x0, y0, x1, y1)`,
    /// half-open, clamped to bounds), leaving any alpha untouched, copy-on-write
    /// (PyMuPDF `Pixmap.invert_irect`).
    pub fn invert_irect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        let x1 = x1.min(self.width);
        let y1 = y1.min(self.height);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let n = self.n as usize;
        let stride = self.stride;
        let color = self.colorspace.components() as usize;
        let samples = self.samples_mut();
        for y in y0..y1 {
            let row = y as usize * stride;
            for x in x0..x1 {
                let base = row + x as usize * n;
                for c in 0..color {
                    samples[base + c] = !samples[base + c];
                }
            }
        }
    }

    /// An independent copy of this pixmap (PyMuPDF `Pixmap.copy`). The samples
    /// `Arc` is shared until either copy is mutated (copy-on-write), so this is
    /// cheap and safe.
    #[must_use]
    pub fn copy(&self) -> Pixmap {
        self.clone()
    }

    /// Fills the rectangle `(x0, y0, x1, y1)` (half-open, clamped to bounds) with
    /// the color components `color` (one byte per color component, alpha left
    /// untouched), copy-on-write (PyMuPDF `Pixmap.set_rect`). Returns the number
    /// of pixels written. `color` shorter than the color-component count is
    /// zero-padded; longer is truncated.
    pub fn set_rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, color: &[u8]) -> usize {
        let x1 = x1.min(self.width);
        let y1 = y1.min(self.height);
        if x0 >= x1 || y0 >= y1 {
            return 0;
        }
        let n = self.n as usize;
        let stride = self.stride;
        let comps = self.colorspace.components() as usize;
        let samples = self.samples_mut();
        let mut count = 0usize;
        for y in y0..y1 {
            let row = y as usize * stride;
            for x in x0..x1 {
                let base = row + x as usize * n;
                for c in 0..comps {
                    samples[base + c] = color.get(c).copied().unwrap_or(0);
                }
                count += 1;
            }
        }
        count
    }

    /// Halves the pixmap's dimensions `factor` times by 2×2 box-averaging
    /// (PyMuPDF `Pixmap.shrink(factor)`). `factor == 0` is a no-op; each step
    /// rounds the new dimension down. Stops if a dimension would reach zero.
    pub fn shrink(&mut self, factor: u8) {
        for _ in 0..factor {
            if self.width < 2 || self.height < 2 {
                break;
            }
            *self = self.box_halved();
        }
    }

    /// One 2×2 box-average downscale step (used by [`Pixmap::shrink`]).
    fn box_halved(&self) -> Pixmap {
        let n = self.n as usize;
        let nw = self.width / 2;
        let nh = self.height / 2;
        let src = &self.samples;
        let src_stride = self.stride;
        let mut out = vec![0u8; nw as usize * nh as usize * n];
        for y in 0..nh as usize {
            for x in 0..nw as usize {
                let (sx, sy) = (x * 2, y * 2);
                let o = (y * nw as usize + x) * n;
                for c in 0..n {
                    let p00 = src[sy * src_stride + sx * n + c] as u32;
                    let p01 = src[sy * src_stride + (sx + 1) * n + c] as u32;
                    let p10 = src[(sy + 1) * src_stride + sx * n + c] as u32;
                    let p11 = src[(sy + 1) * src_stride + (sx + 1) * n + c] as u32;
                    out[o + c] = ((p00 + p01 + p10 + p11) / 4) as u8;
                }
            }
        }
        Pixmap::new(nw, nh, self.colorspace, self.alpha, out)
    }

    /// Encodes this pixmap as a PNG into `out` (PyMuPDF `Pixmap.save(...)` /
    /// `tobytes("png")`).
    ///
    /// CMYK is converted to RGB first (PNG has no CMYK). Gray/RGB with or without
    /// alpha map directly to PNG color types.
    ///
    /// # Errors
    ///
    /// [`Error::Decode`] if the `image` encoder rejects the buffer (should not
    /// happen for well-formed pixmaps).
    pub fn save_png(&self, out: &mut Vec<u8>) -> Result<()> {
        use image::codecs::png::PngEncoder;
        use image::{ColorType, ImageEncoder};

        // PNG has no CMYK; convert to RGB(A) for output.
        let (color, bytes): (ColorType, std::borrow::Cow<'_, [u8]>) = match self.colorspace {
            Colorspace::Cmyk => {
                let rgb = self.to_rgb_bytes();
                (
                    if self.alpha {
                        ColorType::Rgba8
                    } else {
                        ColorType::Rgb8
                    },
                    std::borrow::Cow::Owned(rgb),
                )
            }
            Colorspace::Gray => (
                if self.alpha {
                    ColorType::La8
                } else {
                    ColorType::L8
                },
                std::borrow::Cow::Borrowed(&self.samples),
            ),
            Colorspace::Rgb => (
                if self.alpha {
                    ColorType::Rgba8
                } else {
                    ColorType::Rgb8
                },
                std::borrow::Cow::Borrowed(&self.samples),
            ),
        };
        let encoder = PngEncoder::new(&mut *out);
        encoder
            .write_image(&bytes, self.width, self.height, color.into())
            .map_err(|_| Error::decode("Pixmap", "PNG encode failed"))?;
        Ok(())
    }

    /// Encodes this pixmap as PNG and returns the bytes (PyMuPDF
    /// `Pixmap.tobytes("png")`).
    ///
    /// # Errors
    ///
    /// See [`Pixmap::save_png`].
    pub fn to_png_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.save_png(&mut out)?;
        Ok(out)
    }

    /// Encodes this pixmap in `format` (`"png"`, `"ppm"`/`"pnm"`, or `"pam"`),
    /// matching the common PyMuPDF `Pixmap.tobytes(output=…)` cases (PRD §8.10).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for an unrecognized format; encode errors propagate.
    pub fn tobytes(&self, format: &str) -> Result<Vec<u8>> {
        match format.to_ascii_lowercase().as_str() {
            "png" => self.to_png_bytes(),
            "ppm" | "pnm" | "pgm" => Ok(self.to_pnm_bytes()),
            "pam" => Ok(self.to_pam_bytes()),
            _ => Err(Error::Unsupported("Pixmap::tobytes format")),
        }
    }

    /// Writes a binary PNM (P5 gray / P6 RGB, CMYK→RGB), alpha dropped.
    fn to_pnm_bytes(&self) -> Vec<u8> {
        let (magic, comps, data): (&str, usize, std::borrow::Cow<'_, [u8]>) = match self.colorspace
        {
            Colorspace::Gray => ("P5", 1, self.color_only_bytes()),
            Colorspace::Rgb => ("P6", 3, self.color_only_bytes()),
            Colorspace::Cmyk => (
                "P6",
                3,
                std::borrow::Cow::Owned(self.cmyk_to_rgb_color_only()),
            ),
        };
        let mut out = format!("{magic}\n{} {}\n255\n", self.width, self.height).into_bytes();
        let _ = comps;
        out.extend_from_slice(&data);
        out
    }

    /// Writes a binary PAM (supports alpha + CMYK natively).
    fn to_pam_bytes(&self) -> Vec<u8> {
        let (depth, maxval, tupltype) = match (self.colorspace, self.alpha) {
            (Colorspace::Gray, false) => (1, 255, "GRAYSCALE"),
            (Colorspace::Gray, true) => (2, 255, "GRAYSCALE_ALPHA"),
            (Colorspace::Rgb, false) => (3, 255, "RGB"),
            (Colorspace::Rgb, true) => (4, 255, "RGB_ALPHA"),
            (Colorspace::Cmyk, false) => (4, 255, "CMYK"),
            (Colorspace::Cmyk, true) => (5, 255, "CMYK_ALPHA"),
        };
        let header = format!(
            "P7\nWIDTH {}\nHEIGHT {}\nDEPTH {depth}\nMAXVAL {maxval}\nTUPLTYPE {tupltype}\nENDHDR\n",
            self.width, self.height
        );
        let mut out = header.into_bytes();
        out.extend_from_slice(&self.samples);
        out
    }

    /// The color components (no alpha) as a borrowed/owned contiguous buffer.
    fn color_only_bytes(&self) -> std::borrow::Cow<'_, [u8]> {
        if !self.alpha {
            return std::borrow::Cow::Borrowed(&self.samples);
        }
        let comps = self.colorspace.components() as usize;
        let n = self.n as usize;
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * comps);
        for px in self.samples.chunks_exact(n) {
            out.extend_from_slice(&px[..comps]);
        }
        std::borrow::Cow::Owned(out)
    }

    /// Converts CMYK (+ optional alpha) to interleaved RGB(A) bytes.
    fn to_rgb_bytes(&self) -> Vec<u8> {
        debug_assert_eq!(self.colorspace, Colorspace::Cmyk);
        let n = self.n as usize;
        let out_n = if self.alpha { 4 } else { 3 };
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * out_n);
        for px in self.samples.chunks_exact(n) {
            let (r, g, b) = cmyk_to_rgb(px[0], px[1], px[2], px[3]);
            out.push(r);
            out.push(g);
            out.push(b);
            if self.alpha {
                out.push(px[4]);
            }
        }
        out
    }

    /// CMYK → RGB, color-only (no alpha), for PNM output.
    fn cmyk_to_rgb_color_only(&self) -> Vec<u8> {
        let n = self.n as usize;
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * 3);
        for px in self.samples.chunks_exact(n) {
            let (r, g, b) = cmyk_to_rgb(px[0], px[1], px[2], px[3]);
            out.push(r);
            out.push(g);
            out.push(b);
        }
        out
    }
}

/// Naive CMYK → RGB (`c/m/y/k` in 0..=255, additive complement). Matches the
/// common viewer approximation; ICC-accurate conversion is M6.
fn cmyk_to_rgb(c: u8, m: u8, y: u8, k: u8) -> (u8, u8, u8) {
    let inv = |comp: u8| -> u8 {
        let v = (255u16 - comp as u16) * (255u16 - k as u16) / 255;
        v as u8
    };
    (inv(c), inv(m), inv(y))
}

/// Upsamples packed `bits`-per-component samples (1/2/4/8/16) to a contiguous
/// 8-bit interleaved buffer, stripping per-row padding (rows are byte-aligned in
/// the PDF sample layout the codecs hand us).
fn upsample_to_8bit(
    data: &[u8],
    width: u32,
    height: u32,
    components: u8,
    bits: u8,
) -> Result<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    let comps = components as usize;
    let out_len = w
        .checked_mul(h)
        .and_then(|p| p.checked_mul(comps))
        .ok_or(Error::LimitExceeded("pixmap sample size overflow"))?;

    match bits {
        8 => {
            if data.len() < out_len {
                return Err(Error::decode("Pixmap", "8-bit sample buffer too short"));
            }
            Ok(data[..out_len].to_vec())
        }
        16 => {
            // Big-endian 16-bit per the PDF convention; take the high byte.
            let need = out_len
                .checked_mul(2)
                .ok_or(Error::LimitExceeded("pixmap sample size overflow"))?;
            if data.len() < need {
                return Err(Error::decode("Pixmap", "16-bit sample buffer too short"));
            }
            Ok(data.chunks_exact(2).take(out_len).map(|c| c[0]).collect())
        }
        1 | 2 | 4 => {
            let samples_per_row = w * comps;
            let row_bytes = (samples_per_row * bits as usize).div_ceil(8);
            if data.len() < row_bytes * h {
                return Err(Error::decode("Pixmap", "sub-byte sample buffer too short"));
            }
            let max = (1u16 << bits) - 1;
            let scale = 255u16 / max; // exact for 1/2/4 bpc
            let mut out = Vec::with_capacity(out_len);
            for y in 0..h {
                let row = &data[y * row_bytes..y * row_bytes + row_bytes];
                let mut bit = 0usize;
                for _ in 0..samples_per_row {
                    let v = read_bits(row, bit, bits as usize);
                    out.push((v as u16 * scale) as u8);
                    bit += bits as usize;
                }
            }
            Ok(out)
        }
        _ => Err(Error::decode("Pixmap", "unsupported bit depth")),
    }
}

/// Reads `count` (<=8) bits at bit-offset `bit` from `row` (MSB-first), the PDF
/// sample bit order.
fn read_bits(row: &[u8], bit: usize, count: usize) -> u8 {
    let mut v = 0u8;
    for i in 0..count {
        let b = bit + i;
        let byte = row[b / 8];
        let shift = 7 - (b % 8);
        v = (v << 1) | ((byte >> shift) & 1);
    }
    v
}
