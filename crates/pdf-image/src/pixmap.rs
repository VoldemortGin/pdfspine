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

use std::collections::{HashMap, HashSet};
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

    /// Tints the color components by a per-channel linear map between `black`
    /// and `white` (PyMuPDF `Pixmap.tint_with(black, white)`). `black` and
    /// `white` are packed `0xRRGGBB` ints; each color byte `c` is remapped to
    /// `black_comp + c * (white_comp - black_comp) / 255` (clamped to `0..=255`).
    /// Only `Gray`/`Rgb` pixmaps are affected — `Cmyk` is a no-op, matching
    /// PyMuPDF. Alpha is left untouched, copy-on-write.
    pub fn tint_with(&mut self, black: u32, white: u32) {
        let comps = self.colorspace.components() as usize;
        if !matches!(self.colorspace, Colorspace::Gray | Colorspace::Rgb) {
            return;
        }
        let unpack = |v: u32| [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        let (b, w) = (unpack(black), unpack(white));
        // For gray (1 component) use the red channel; for RGB use all three.
        let map: [(u8, u8); 3] = [(b[0], w[0]), (b[1], w[1]), (b[2], w[2])];
        let n = self.n as usize;
        let samples = self.samples_mut();
        for px in samples.chunks_exact_mut(n) {
            for (c, byte) in px.iter_mut().take(comps).enumerate() {
                let (bc, wc) = (map[c].0 as i32, map[c].1 as i32);
                let v = bc + (*byte as i32) * (wc - bc) / 255;
                *byte = v.clamp(0, 255) as u8;
            }
        }
    }

    /// Applies a gamma curve to the color components (PyMuPDF
    /// `Pixmap.gamma_with(gamma)`): each color byte `c` becomes
    /// `round(255 * (c / 255).powf(gamma))`. `gamma == 1.0` is a no-op. Alpha is
    /// left untouched, copy-on-write.
    pub fn gamma_with(&mut self, gamma: f64) {
        if gamma == 1.0 {
            return;
        }
        let mut lut = [0u8; 256];
        for (c, slot) in lut.iter_mut().enumerate() {
            let v = 255.0 * (c as f64 / 255.0).powf(gamma);
            *slot = v.round().clamp(0.0, 255.0) as u8;
        }
        let comps = self.colorspace.components() as usize;
        let n = self.n as usize;
        let samples = self.samples_mut();
        for px in samples.chunks_exact_mut(n) {
            for byte in px.iter_mut().take(comps) {
                *byte = lut[*byte as usize];
            }
        }
    }

    /// The number of distinct colors (PyMuPDF `Pixmap.color_count()`), counting
    /// only the color components and ignoring any alpha channel.
    #[must_use]
    pub fn color_count(&self) -> usize {
        let comps = self.colorspace.components() as usize;
        let n = self.n as usize;
        let mut seen: HashSet<&[u8]> = HashSet::new();
        for px in self.samples.chunks_exact(n) {
            seen.insert(&px[..comps]);
        }
        seen.len()
    }

    /// The most frequent color and its frequency (PyMuPDF
    /// `Pixmap.color_topusage()`): returns `(ratio, pixel)` where `pixel` is the
    /// color-component bytes (length `components`) of the most common color and
    /// `ratio` is its share of all pixels in `0..=1`. An empty pixmap yields
    /// `(1.0, vec![0; components])`.
    #[must_use]
    pub fn color_topusage(&self) -> (f64, Vec<u8>) {
        let comps = self.colorspace.components() as usize;
        let n = self.n as usize;
        let total = self.width as usize * self.height as usize;
        if total == 0 {
            return (1.0, vec![0u8; comps]);
        }
        let mut counts: HashMap<&[u8], usize> = HashMap::new();
        for px in self.samples.chunks_exact(n) {
            *counts.entry(&px[..comps]).or_insert(0) += 1;
        }
        let (color, count) = counts
            .into_iter()
            .max_by_key(|&(_, c)| c)
            .expect("non-empty pixmap has at least one color");
        (count as f64 / total as f64, color.to_vec())
    }

    /// Whether the pixmap contains only pure black and pure white color pixels
    /// (PyMuPDF `Pixmap.is_monochrome()`): every pixel's color components are all
    /// `0x00` or all `0xFF`.
    #[must_use]
    pub fn is_monochrome(&self) -> bool {
        let comps = self.colorspace.components() as usize;
        let n = self.n as usize;
        self.samples.chunks_exact(n).all(|px| {
            let color = &px[..comps];
            color.iter().all(|&b| b == 0) || color.iter().all(|&b| b == 255)
        })
    }

    /// Whether every pixel shares the same color (PyMuPDF
    /// `Pixmap.is_unicolor()`). An empty pixmap is considered unicolor.
    #[must_use]
    pub fn is_unicolor(&self) -> bool {
        let comps = self.colorspace.components() as usize;
        let n = self.n as usize;
        let mut iter = self.samples.chunks_exact(n);
        let Some(first) = iter.next() else {
            return true;
        };
        let first = &first[..comps];
        iter.all(|px| &px[..comps] == first)
    }

    /// A deterministic 16-byte content hash of the samples plus geometry
    /// (PyMuPDF `Pixmap.digest()`). PyMuPDF returns an MD5 here; this crate has
    /// no crypto-hash dependency, so this is a **stable, content-sensitive
    /// non-cryptographic** digest (two interleaved FNV-1a 64-bit hashes) — equal
    /// samples/geometry always produce the same bytes, and changes are very
    /// likely to differ.
    #[must_use]
    pub fn digest(&self) -> [u8; 16] {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h1 = FNV_OFFSET;
        // Mix geometry into the second hash's seed so identical samples with
        // different geometry still differ.
        let mut h2 = FNV_OFFSET;
        for v in [self.width, self.height, self.n as u32] {
            for byte in v.to_le_bytes() {
                h2 = (h2 ^ byte as u64).wrapping_mul(FNV_PRIME);
            }
        }
        for (i, &byte) in self.samples.iter().enumerate() {
            h1 = (h1 ^ byte as u64).wrapping_mul(FNV_PRIME);
            // Position-mix the second hash so byte transpositions differ.
            h2 = (h2 ^ (byte as u64).rotate_left((i & 63) as u32)).wrapping_mul(FNV_PRIME);
        }
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&h1.to_le_bytes());
        out[8..].copy_from_slice(&h2.to_le_bytes());
        out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_with_rgb_maps_each_channel() {
        // 1x1 RGB pixel (100, 150, 200).
        let mut pm = Pixmap::new(1, 1, Colorspace::Rgb, false, vec![100, 150, 200]);
        // black = (10, 20, 30), white = (250, 240, 230).
        pm.tint_with(0x0A_14_1E, 0xFA_F0_E6);
        let expected = |old: i32, b: i32, w: i32| (b + old * (w - b) / 255).clamp(0, 255) as u8;
        assert_eq!(
            pm.pixel(0, 0).unwrap(),
            vec![
                expected(100, 10, 250),
                expected(150, 20, 240),
                expected(200, 30, 230),
            ]
        );
    }

    #[test]
    fn tint_with_cmyk_is_noop() {
        let mut pm = Pixmap::new(1, 1, Colorspace::Cmyk, false, vec![10, 20, 30, 40]);
        pm.tint_with(0x00_00_00, 0xFF_FF_FF);
        assert_eq!(pm.pixel(0, 0).unwrap(), vec![10, 20, 30, 40]);
    }

    #[test]
    fn gamma_with_identity_is_noop() {
        let mut pm = Pixmap::new(1, 2, Colorspace::Gray, false, vec![0, 128]);
        pm.gamma_with(1.0);
        assert_eq!(pm.samples(), &[0, 128]);
    }

    #[test]
    fn gamma_with_nonidentity_changes_midtones() {
        // gamma 2.0 darkens midtones: 128 -> round(255 * (128/255)^2) = 64.
        let mut pm = Pixmap::new(1, 3, Colorspace::Gray, false, vec![0, 128, 255]);
        pm.gamma_with(2.0);
        let s = pm.samples();
        assert_eq!(s[0], 0);
        assert_eq!(s[2], 255);
        let want = (255.0_f64 * (128.0 / 255.0_f64).powf(2.0)).round() as u8;
        assert_eq!(s[1], want);
    }

    #[test]
    fn color_count_two_color_image() {
        // Two distinct RGB colors, each appearing twice.
        let samples = vec![
            255, 0, 0, // red
            0, 255, 0, // green
            255, 0, 0, // red
            0, 255, 0, // green
        ];
        let pm = Pixmap::new(2, 2, Colorspace::Rgb, false, samples);
        assert_eq!(pm.color_count(), 2);
    }

    #[test]
    fn color_count_ignores_alpha() {
        // Same RGB, different alpha -> still one color.
        let samples = vec![10, 20, 30, 100, 10, 20, 30, 200];
        let pm = Pixmap::new(2, 1, Colorspace::Rgb, true, samples);
        assert_eq!(pm.color_count(), 1);
    }

    #[test]
    fn color_topusage_ratio() {
        // 3 red, 1 green -> red is top with ratio 3/4.
        let samples = vec![
            255, 0, 0, // red
            255, 0, 0, // red
            255, 0, 0, // red
            0, 255, 0, // green
        ];
        let pm = Pixmap::new(2, 2, Colorspace::Rgb, false, samples);
        let (ratio, color) = pm.color_topusage();
        assert!((ratio - 0.75).abs() < 1e-9);
        assert_eq!(color, vec![255, 0, 0]);
    }

    #[test]
    fn color_topusage_empty() {
        let pm = Pixmap::new(0, 0, Colorspace::Rgb, false, vec![]);
        let (ratio, color) = pm.color_topusage();
        assert_eq!(ratio, 1.0);
        assert_eq!(color, vec![0, 0, 0]);
    }

    #[test]
    fn is_monochrome_true_and_false() {
        let mono = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![0, 0, 0, 255, 255, 255]);
        assert!(mono.is_monochrome());

        let not_mono = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![0, 0, 0, 255, 0, 255]);
        assert!(!not_mono.is_monochrome());
    }

    #[test]
    fn is_unicolor_true_and_false() {
        let uni = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![1, 2, 3, 1, 2, 3]);
        assert!(uni.is_unicolor());

        let multi = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![1, 2, 3, 4, 5, 6]);
        assert!(!multi.is_unicolor());

        let empty = Pixmap::new(0, 0, Colorspace::Rgb, false, vec![]);
        assert!(empty.is_unicolor());
    }

    #[test]
    fn digest_is_deterministic_and_content_sensitive() {
        let a = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![1, 2, 3, 4, 5, 6]);
        let b = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(a.digest(), b.digest());

        let c = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![1, 2, 3, 4, 5, 7]);
        assert_ne!(a.digest(), c.digest());

        // Byte transposition should differ too.
        let d = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![2, 1, 3, 4, 5, 6]);
        assert_ne!(a.digest(), d.digest());
    }
}
