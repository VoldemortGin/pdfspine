//! Image-document support — PRD §8.10.
//!
//! Opens a raster image (PNG/JPEG/TIFF-multi-IFD/GIF/BMP/WEBP) as a one-page-
//! per-image document, and converts image inputs to PDF (`convert_to_pdf`).
//!
//! # Page-sizing / DPI convention (PRD §8.10, PyMuPDF model)
//!
//! Each image frame becomes one PDF page whose `/MediaBox` is the image's
//! physical size in PDF points. The PyMuPDF convention is **1 pixel → 1 point at
//! 72 dpi**, honoring an embedded resolution when present: a page is
//! `width_px / dpi_x * 72` by `height_px / dpi_y * 72` points. When the source
//! carries no resolution metadata the DPI defaults to 72, so the `/MediaBox`
//! equals the pixel dimensions (a 100×50 PNG → `[0 0 100 50]`). The image is
//! placed full-bleed with the content stream `q w 0 0 h 0 0 cm /Img Do Q`.
//!
//! # Codec handling
//!
//! * **JPEG** → the original bytes are embedded verbatim as a `/DCTDecode`
//!   XObject (byte-equal passthrough, lossless, no re-encode); the colorspace
//!   is read from the JPEG SOF marker (1=Gray, 3=RGB/YCbCr, 4=CMYK).
//! * **PNG/BMP/GIF/WEBP/TIFF** → decoded with the `image` crate and embedded as
//!   `/FlateDecode` samples. Gray → `/DeviceGray`, RGB → `/DeviceRGB`, a palette
//!   PNG with ≤256 colors → `/Indexed`, an alpha channel → a separate
//!   `/SMask` (soft-mask) XObject.
//! * **Multi-IFD TIFF / animated GIF** → one PDF page per IFD / per frame.
//!
//! Non-image or corrupt input yields a typed [`Error`] (never a panic); decoded
//! dimensions are capped to guard against decompression bombs.

use std::io::Cursor;

use image::{AnimationDecoder, ColorType, DynamicImage, ImageDecoder};

use pdf_core::{Dict, DocumentStore, Limits, Name, ObjRef, Object, StreamObj};
use pdf_core::{SaveOptions, XrefStyle};

use crate::error::{Error, Result};
use crate::pixmap::{Colorspace, Pixmap};

/// Largest pixel dimension (width or height) accepted from untrusted input.
const MAX_DIMENSION: u32 = 200_000;
/// Largest decoded-pixel count accepted (decompression-bomb guard, ~256 MPx).
const MAX_PIXELS: u64 = 256 * 1024 * 1024;
/// Largest number of frames/IFDs accepted from a single container.
const MAX_FRAMES: usize = 4096;

/// The raster container formats supported as image documents (PRD §8.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageFormat {
    /// PNG (all depths/palette/alpha/interlace).
    Png,
    /// JPEG (baseline + progressive; header read without full decode for
    /// `convert_to_pdf` passthrough).
    Jpeg,
    /// TIFF (multi-page — one page per IFD).
    Tiff,
    /// GIF.
    Gif,
    /// BMP.
    Bmp,
    /// WEBP.
    Webp,
}

impl ImageFormat {
    /// Best-effort format sniff from the leading magic bytes, or `None` if the
    /// signature is unrecognized. Panic-free.
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some(ImageFormat::Png);
        }
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(ImageFormat::Jpeg);
        }
        if bytes.starts_with(b"II\x2A\x00") || bytes.starts_with(b"MM\x00\x2A") {
            return Some(ImageFormat::Tiff);
        }
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            return Some(ImageFormat::Gif);
        }
        if bytes.starts_with(b"BM") {
            return Some(ImageFormat::Bmp);
        }
        // WEBP: "RIFF" .... "WEBP"
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return Some(ImageFormat::Webp);
        }
        None
    }
}

/// An opened image document: the decoded pages plus their source format.
///
/// One [`Pixmap`] per page (one IFD per page for multi-page TIFF; exactly one
/// page for the single-frame formats).
#[derive(Clone, Debug)]
pub struct ImageDocument {
    /// The source container format.
    pub format: ImageFormat,
    /// One decoded raster per page.
    pub pages: Vec<Pixmap>,
}

impl ImageDocument {
    /// Number of pages (1 for single-frame formats; IFD count for multi-TIFF).
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Opens raster `bytes` of the given `format` as an [`ImageDocument`].
///
/// If `format` is `None` the format is sniffed via [`ImageFormat::sniff`]. The
/// decoded pages are normalized to 8-bit Gray/RGB samples plus an optional alpha
/// channel (the PyMuPDF [`Pixmap`] model). Non-image / corrupt input yields a
/// typed [`Error`].
pub fn open_image_document(bytes: &[u8], format: Option<ImageFormat>) -> Result<ImageDocument> {
    let format = resolve_format(bytes, format)?;
    let frames = decode_frames(bytes, format)?;
    let mut pages = Vec::with_capacity(frames.len());
    for frame in frames {
        pages.push(frame.into_pixmap()?);
    }
    Ok(ImageDocument { format, pages })
}

/// Converts an image input to a single-/multi-page PDF and returns the PDF
/// bytes (PRD §8.10, image inputs only).
///
/// JPEG → `/DCTDecode` passthrough (byte-equal); PNG/BMP/GIF/WEBP/TIFF → decode
/// → `/FlateDecode`; alpha → `/SMask`; palette PNG → `/Indexed`; one page per
/// frame/IFD. A **non-image** input yields [`Error::InvalidArgument`], never a
/// panic. The returned bytes reparse cleanly via [`DocumentStore::from_bytes`].
pub fn convert_to_pdf(bytes: &[u8], format: Option<ImageFormat>) -> Result<Vec<u8>> {
    let format = resolve_format(bytes, format)?;
    let frames = decode_frames(bytes, format)?;
    build_pdf(&frames)
}

/// Basic header properties of a raster image (PyMuPDF `image_profile` /
/// `Tools.image_profile`), read from the leading frame without a full decode
/// where the format allows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageProfile {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// EXIF/orientation code (`0` — pdfspine does not honor an EXIF rotation).
    pub orientation: i32,
    /// The orientation `(a, b, c, d, e, f)` matrix (identity for `orientation 0`).
    pub matrix: (f64, f64, f64, f64, f64, f64),
    /// Horizontal resolution in dpi (defaults to `96` when undeclared, per fitz).
    pub xres: i32,
    /// Vertical resolution in dpi (defaults to `96` when undeclared, per fitz).
    pub yres: i32,
    /// Number of color components (fitz `colorspace` key = `image.n()`).
    pub colorspace: i32,
    /// Bits per component (`8` for the formats pdfspine decodes).
    pub bpc: i32,
    /// MuPDF image-extension token (`"png"`, `"jpeg"`, `"gif"`, `"bmp"`,
    /// `"tiff"`, n/a otherwise).
    pub ext: String,
    /// Colorspace name (`"DeviceGray"`/`"DeviceRGB"`), or `""` when none.
    pub cs_name: String,
}

/// Reads the header profile of raster `bytes` (PyMuPDF `image_profile` /
/// `Tools.image_profile`).
///
/// Returns `None` for empty / too-short / unrecognized input (fitz returns
/// `None` there too). The format is sniffed from the magic bytes; the leading
/// frame supplies width/height/components/dpi.
#[must_use]
pub fn image_profile(bytes: &[u8]) -> Option<ImageProfile> {
    if bytes.len() < 8 {
        return None;
    }
    let format = ImageFormat::sniff(bytes)?;
    let frames = decode_frames(bytes, format).ok()?;
    let frame = frames.into_iter().next()?;

    let (colorspace, cs_name) = frame.colorspace_profile();
    let (xres, yres) = match frame.dpi() {
        Some((dx, dy)) if dx > 0.0 && dy > 0.0 => (dx.round() as i32, dy.round() as i32),
        _ => (96, 96),
    };
    Some(ImageProfile {
        width: frame.width(),
        height: frame.height(),
        orientation: 0,
        matrix: (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        xres,
        yres,
        colorspace,
        bpc: 8,
        ext: image_extension(format).to_string(),
        cs_name,
    })
}

/// The MuPDF image-extension token for a container format (PyMuPDF
/// `JM_image_extension`).
fn image_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Gif => "gif",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Webp => "n/a",
    }
}

/// Resolves an explicit or sniffed format, mapping non-image input to
/// [`Error::InvalidArgument`] (PRD §8.10 / §3.2 #2).
fn resolve_format(bytes: &[u8], format: Option<ImageFormat>) -> Result<ImageFormat> {
    match format {
        Some(f) => Ok(f),
        None => {
            ImageFormat::sniff(bytes).ok_or(Error::InvalidArgument("not a recognized image format"))
        }
    }
}

// ---------------------------------------------------------------------------
// Frame model
// ---------------------------------------------------------------------------

/// The colorspace of a decoded (non-JPEG) frame's color components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodedColor {
    Gray,
    Rgb,
}

/// One image frame, ready for both `Pixmap` production and PDF embedding.
enum Frame {
    /// JPEG passthrough: the original DCT bytes embedded verbatim.
    Jpeg {
        width: u32,
        height: u32,
        /// 1 = Gray, 3 = RGB/YCbCr, 4 = CMYK (from the SOF marker).
        components: u8,
        /// Embedded resolution in dpi (x, y), if declared.
        dpi: Option<(f64, f64)>,
        /// The original JPEG bytes, embedded byte-for-byte as `/DCTDecode`.
        data: Vec<u8>,
    },
    /// A decoded raster (PNG/BMP/GIF/WEBP/TIFF), Flate-embedded.
    Decoded {
        width: u32,
        height: u32,
        color: DecodedColor,
        /// Interleaved color samples (no alpha), row-major, 8-bit.
        data: Vec<u8>,
        /// De-interleaved alpha plane (one byte per pixel) for an `/SMask`.
        alpha: Option<Vec<u8>>,
        /// `Some((palette_rgb, indices))` for an `/Indexed` colorspace.
        palette: Option<PaletteImage>,
        /// Embedded resolution in dpi (x, y), if declared.
        dpi: Option<(f64, f64)>,
    },
}

/// A palette (`/Indexed`) representation: the RGB lookup table plus 1-byte
/// indices into it. Alpha (if any) is carried separately as an `/SMask`.
struct PaletteImage {
    /// Flat RGB lookup table (`3 * palette_len` bytes).
    palette_rgb: Vec<u8>,
    /// One index byte per pixel.
    indices: Vec<u8>,
}

impl Frame {
    fn width(&self) -> u32 {
        match self {
            Frame::Jpeg { width, .. } | Frame::Decoded { width, .. } => *width,
        }
    }

    fn height(&self) -> u32 {
        match self {
            Frame::Jpeg { height, .. } | Frame::Decoded { height, .. } => *height,
        }
    }

    fn dpi(&self) -> Option<(f64, f64)> {
        match self {
            Frame::Jpeg { dpi, .. } | Frame::Decoded { dpi, .. } => *dpi,
        }
    }

    /// The `(n_components, colorspace_name)` for an [`ImageProfile`]: the count
    /// fitz reports as its `colorspace` key plus the `DeviceGray`/`DeviceRGB`
    /// device name. Palette frames resolve to their materialized RGB color.
    fn colorspace_profile(&self) -> (i32, String) {
        match self {
            Frame::Jpeg { components, .. } => match components {
                1 => (1, "DeviceGray".to_string()),
                4 => (4, "DeviceCMYK".to_string()),
                _ => (3, "DeviceRGB".to_string()),
            },
            Frame::Decoded { color, palette, .. } => {
                if palette.is_some() {
                    return (3, "DeviceRGB".to_string());
                }
                match color {
                    DecodedColor::Gray => (1, "DeviceGray".to_string()),
                    DecodedColor::Rgb => (3, "DeviceRGB".to_string()),
                }
            }
        }
    }

    /// The page size in PDF points: `px / dpi * 72`, defaulting to 72 dpi
    /// (1 px → 1 pt) when no resolution is declared.
    fn page_size_pt(&self) -> (f64, f64) {
        let (dx, dy) = self.dpi().unwrap_or((72.0, 72.0));
        let dx = if dx > 0.0 { dx } else { 72.0 };
        let dy = if dy > 0.0 { dy } else { 72.0 };
        let w = f64::from(self.width()) / dx * 72.0;
        let h = f64::from(self.height()) / dy * 72.0;
        (w, h)
    }

    /// Builds the PyMuPDF [`Pixmap`] (8-bit Gray/RGB + optional alpha) for this
    /// frame. JPEG frames are decoded here; decoded frames are repackaged.
    fn into_pixmap(self) -> Result<Pixmap> {
        match self {
            Frame::Jpeg { data, .. } => {
                let img = image::load_from_memory_with_format(&data, image::ImageFormat::Jpeg)
                    .map_err(|_| Error::decode("DCTDecode", "JPEG decode failed"))?;
                Ok(dynimage_to_pixmap(&img))
            }
            Frame::Decoded {
                width,
                height,
                color,
                data,
                alpha,
                palette,
                ..
            } => {
                // Materialize palette -> RGB samples if needed.
                let (color, mut samples) = if let Some(pal) = palette {
                    let mut rgb = Vec::with_capacity(pal.indices.len() * 3);
                    let plen = pal.palette_rgb.len() / 3;
                    for &idx in &pal.indices {
                        let i = (idx as usize).min(plen.saturating_sub(1));
                        let base = i * 3;
                        rgb.extend_from_slice(&pal.palette_rgb[base..base + 3]);
                    }
                    (DecodedColor::Rgb, rgb)
                } else {
                    (color, data)
                };
                let cs = match color {
                    DecodedColor::Gray => Colorspace::Gray,
                    DecodedColor::Rgb => Colorspace::Rgb,
                };
                let has_alpha = alpha.is_some();
                if let Some(a) = alpha {
                    samples = interleave_alpha(&samples, &a, cs.components() as usize);
                }
                Ok(Pixmap::new(width, height, cs, has_alpha, samples))
            }
        }
    }
}

/// Interleaves a separate alpha plane back into color samples (`...RGBA` /
/// `...GA`) for the `Pixmap` buffer.
fn interleave_alpha(color: &[u8], alpha: &[u8], components: usize) -> Vec<u8> {
    let pixels = alpha.len();
    let mut out = Vec::with_capacity(pixels * (components + 1));
    for (p, &a) in alpha.iter().enumerate() {
        let base = p * components;
        out.extend_from_slice(&color[base..base + components]);
        out.push(a);
    }
    out
}

/// Converts an `image::DynamicImage` into a normalized 8-bit `Pixmap`.
fn dynimage_to_pixmap(img: &DynamicImage) -> Pixmap {
    let (w, h) = (img.width(), img.height());
    match img.color() {
        ColorType::L8 | ColorType::L16 => {
            Pixmap::new(w, h, Colorspace::Gray, false, img.to_luma8().into_raw())
        }
        ColorType::La8 | ColorType::La16 => Pixmap::new(
            w,
            h,
            Colorspace::Gray,
            true,
            img.to_luma_alpha8().into_raw(),
        ),
        ColorType::Rgba8 | ColorType::Rgba16 | ColorType::Rgba32F => {
            Pixmap::new(w, h, Colorspace::Rgb, true, img.to_rgba8().into_raw())
        }
        _ => Pixmap::new(w, h, Colorspace::Rgb, false, img.to_rgb8().into_raw()),
    }
}

// ---------------------------------------------------------------------------
// Frame decoding (per format)
// ---------------------------------------------------------------------------

fn decode_frames(bytes: &[u8], format: ImageFormat) -> Result<Vec<Frame>> {
    match format {
        ImageFormat::Jpeg => Ok(vec![decode_jpeg_passthrough(bytes)?]),
        ImageFormat::Gif => decode_gif(bytes),
        ImageFormat::Tiff => decode_tiff(bytes),
        ImageFormat::Png => Ok(vec![decode_png(bytes)?]),
        ImageFormat::Bmp => Ok(vec![decode_via_image(
            bytes,
            image::ImageFormat::Bmp,
            None,
        )?]),
        ImageFormat::Webp => Ok(vec![decode_via_image(
            bytes,
            image::ImageFormat::WebP,
            None,
        )?]),
    }
}

/// JPEG → byte-equal `/DCTDecode` passthrough. Reads the SOF marker for size +
/// component count without a full decode.
fn decode_jpeg_passthrough(bytes: &[u8]) -> Result<Frame> {
    let (width, height, components) =
        parse_jpeg_sof(bytes).ok_or(Error::decode("DCTDecode", "no JPEG SOF marker"))?;
    check_dimensions(width, height)?;
    let dpi = parse_jfif_dpi(bytes);
    Ok(Frame::Jpeg {
        width,
        height,
        components,
        dpi,
        data: bytes.to_vec(),
    })
}

/// Parses the JPEG Start-Of-Frame marker: returns `(width, height, components)`.
fn parse_jpeg_sof(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        // Skip fill bytes (0xFF 0xFF ...).
        let mut marker = bytes[i + 1];
        let mut j = i + 1;
        while marker == 0xFF && j + 1 < bytes.len() {
            j += 1;
            marker = bytes[j];
        }
        // Standalone markers without a length payload.
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            i = j + 1;
            continue;
        }
        let len_pos = j + 1;
        if len_pos + 2 > bytes.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([bytes[len_pos], bytes[len_pos + 1]]) as usize;
        // SOF markers carrying frame geometry (exclude DHT/DAC/DRI etc.).
        let is_sof = matches!(marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            // SOF payload: [precision(1)][height(2)][width(2)][components(1)]...
            let p = len_pos + 2;
            if p + 6 > bytes.len() {
                return None;
            }
            let height = u16::from_be_bytes([bytes[p + 1], bytes[p + 2]]) as u32;
            let width = u16::from_be_bytes([bytes[p + 3], bytes[p + 4]]) as u32;
            let components = bytes[p + 5];
            if width == 0 || height == 0 || components == 0 {
                return None;
            }
            return Some((width, height, components));
        }
        i = len_pos + seg_len;
    }
    None
}

/// Reads the JFIF APP0 density (dpi) when the unit is dpi (unit == 1).
fn parse_jfif_dpi(bytes: &[u8]) -> Option<(f64, f64)> {
    // APP0 at offset 2: FF E0 <len(2)> "JFIF\0" <ver(2)> <unit(1)> <x(2)> <y(2)>.
    if bytes.len() < 18 || bytes[2] != 0xFF || bytes[3] != 0xE0 {
        return None;
    }
    if &bytes[6..11] != b"JFIF\0" {
        return None;
    }
    let unit = bytes[13];
    let x = u16::from_be_bytes([bytes[14], bytes[15]]);
    let y = u16::from_be_bytes([bytes[16], bytes[17]]);
    match unit {
        1 if x > 0 && y > 0 => Some((f64::from(x), f64::from(y))), // dots per inch
        2 if x > 0 && y > 0 => Some((f64::from(x) * 2.54, f64::from(y) * 2.54)), // per-cm
        _ => None,
    }
}

/// Decodes a single-image format via the `image` crate, splitting an alpha
/// channel out into a separate plane (for `/SMask`). `dpi` overrides any
/// resolution computed from the decoder.
fn decode_via_image(
    bytes: &[u8],
    fmt: image::ImageFormat,
    dpi: Option<(f64, f64)>,
) -> Result<Frame> {
    let img = image::load_from_memory_with_format(bytes, fmt)
        .map_err(|_| Error::decode(codec_name(fmt), "decode failed"))?;
    check_dimensions(img.width(), img.height())?;
    Ok(dynimage_to_frame(&img, dpi))
}

/// Converts a decoded `DynamicImage` into a Flate-embed `Frame::Decoded`,
/// extracting an alpha plane when present.
fn dynimage_to_frame(img: &DynamicImage, dpi: Option<(f64, f64)>) -> Frame {
    let (width, height) = (img.width(), img.height());
    let has_alpha = img.color().has_alpha();
    let is_gray = matches!(
        img.color(),
        ColorType::L8 | ColorType::L16 | ColorType::La8 | ColorType::La16
    );
    if is_gray {
        if has_alpha {
            let raw = img.to_luma_alpha8().into_raw();
            let (color, alpha) = deinterleave(&raw, 1);
            Frame::Decoded {
                width,
                height,
                color: DecodedColor::Gray,
                data: color,
                alpha: Some(alpha),
                palette: None,
                dpi,
            }
        } else {
            Frame::Decoded {
                width,
                height,
                color: DecodedColor::Gray,
                data: img.to_luma8().into_raw(),
                alpha: None,
                palette: None,
                dpi,
            }
        }
    } else if has_alpha {
        let raw = img.to_rgba8().into_raw();
        let (color, alpha) = deinterleave(&raw, 3);
        Frame::Decoded {
            width,
            height,
            color: DecodedColor::Rgb,
            data: color,
            alpha: Some(alpha),
            palette: None,
            dpi,
        }
    } else {
        Frame::Decoded {
            width,
            height,
            color: DecodedColor::Rgb,
            data: img.to_rgb8().into_raw(),
            alpha: None,
            palette: None,
            dpi,
        }
    }
}

/// Splits an interleaved `color+alpha` buffer into `(color, alpha)` planes.
/// `components` is the count of color components (1 for gray, 3 for RGB).
fn deinterleave(interleaved: &[u8], components: usize) -> (Vec<u8>, Vec<u8>) {
    let stride = components + 1;
    let pixels = interleaved.len() / stride;
    let mut color = Vec::with_capacity(pixels * components);
    let mut alpha = Vec::with_capacity(pixels);
    for p in 0..pixels {
        let base = p * stride;
        color.extend_from_slice(&interleaved[base..base + components]);
        alpha.push(interleaved[base + components]);
    }
    (color, alpha)
}

/// PNG: honors palette (`/Indexed`) and the `pHYs` resolution chunk.
fn decode_png(bytes: &[u8]) -> Result<Frame> {
    let dpi = parse_png_phys_dpi(bytes);
    let is_palette = png_is_palette(bytes);
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|_| Error::decode("FlateDecode", "PNG decode failed"))?;
    check_dimensions(img.width(), img.height())?;

    if is_palette {
        if let Some(frame) = try_build_palette_frame(&img, dpi) {
            return Ok(frame);
        }
    }
    Ok(dynimage_to_frame(&img, dpi))
}

/// Builds an `/Indexed` frame when the image has ≤256 distinct RGB colors.
/// Returns `None` to fall back to direct RGB(A) embedding.
fn try_build_palette_frame(img: &DynamicImage, dpi: Option<(f64, f64)>) -> Option<Frame> {
    let (width, height) = (img.width(), img.height());
    let has_alpha = img.color().has_alpha();
    let rgba = img.to_rgba8();
    let pixels = rgba.as_raw();

    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut indices: Vec<u8> = Vec::with_capacity((width * height) as usize);
    let mut alpha: Vec<u8> = Vec::with_capacity((width * height) as usize);
    let mut lookup: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();

    for px in pixels.chunks_exact(4) {
        let rgb = [px[0], px[1], px[2]];
        let idx = match lookup.get(&rgb) {
            Some(&i) => i,
            None => {
                if palette.len() >= 256 {
                    return None; // too many colors for /Indexed; fall back
                }
                let i = palette.len() as u8;
                palette.push(rgb);
                lookup.insert(rgb, i);
                i
            }
        };
        indices.push(idx);
        alpha.push(px[3]);
    }

    let mut palette_rgb = Vec::with_capacity(palette.len() * 3);
    for c in &palette {
        palette_rgb.extend_from_slice(c);
    }
    let alpha = if has_alpha && alpha.iter().any(|&a| a != 255) {
        Some(alpha)
    } else {
        None
    };
    Some(Frame::Decoded {
        width,
        height,
        color: DecodedColor::Rgb,
        data: Vec::new(),
        alpha,
        palette: Some(PaletteImage {
            palette_rgb,
            indices,
        }),
        dpi,
    })
}

/// True when the PNG IHDR declares color-type 3 (palette).
fn png_is_palette(bytes: &[u8]) -> bool {
    // IHDR is the first chunk after the 8-byte signature: len(4) "IHDR"
    // width(4) height(4) bitdepth(1) colortype(1)...
    if bytes.len() < 26 || &bytes[12..16] != b"IHDR" {
        return false;
    }
    bytes[25] == 3
}

/// Reads PNG `pHYs` (pixels-per-unit-X/Y, unit) and converts to dpi when the
/// unit is metres (unit == 1).
fn parse_png_phys_dpi(bytes: &[u8]) -> Option<(f64, f64)> {
    // Walk chunks: each is len(4) type(4) data(len) crc(4); signature is 8 bytes.
    let mut i = 8usize;
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let ctype = &bytes[i + 4..i + 8];
        let data_start = i + 8;
        let data_end = data_start.checked_add(len)?;
        if data_end + 4 > bytes.len() {
            return None;
        }
        if ctype == b"pHYs" && len >= 9 {
            let ppux = u32::from_be_bytes([
                bytes[data_start],
                bytes[data_start + 1],
                bytes[data_start + 2],
                bytes[data_start + 3],
            ]);
            let ppuy = u32::from_be_bytes([
                bytes[data_start + 4],
                bytes[data_start + 5],
                bytes[data_start + 6],
                bytes[data_start + 7],
            ]);
            let unit = bytes[data_start + 8];
            if unit == 1 && ppux > 0 && ppuy > 0 {
                // pixels per metre -> dpi
                let dx = f64::from(ppux) * 0.0254;
                let dy = f64::from(ppuy) * 0.0254;
                return Some((dx, dy));
            }
            return None;
        }
        if ctype == b"IDAT" || ctype == b"IEND" {
            return None; // pHYs would precede image data
        }
        i = data_end + 4;
    }
    None
}

/// GIF: one PDF page per animation frame.
fn decode_gif(bytes: &[u8]) -> Result<Vec<Frame>> {
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|_| Error::decode("GIF", "GIF open failed"))?;
    let frames = decoder
        .into_frames()
        .take(MAX_FRAMES)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::decode("GIF", "GIF frame decode failed"))?;
    if frames.is_empty() {
        return Err(Error::decode("GIF", "no GIF frames"));
    }
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let buf = frame.into_buffer();
        check_dimensions(buf.width(), buf.height())?;
        let img = DynamicImage::ImageRgba8(buf);
        out.push(dynimage_to_frame(&img, None));
    }
    Ok(out)
}

/// TIFF: one PDF page per IFD. Splits the container into standalone single-IFD
/// TIFFs, then decodes each via the `image` crate, honoring per-IFD resolution.
fn decode_tiff(bytes: &[u8]) -> Result<Vec<Frame>> {
    let ifds = split_tiff_ifds(bytes, MAX_FRAMES)
        .ok_or(Error::decode("TIFF", "malformed TIFF container"))?;
    if ifds.is_empty() {
        return Err(Error::decode("TIFF", "no TIFF IFDs"));
    }
    let mut out = Vec::with_capacity(ifds.len());
    for ifd in &ifds {
        let dpi = parse_tiff_resolution(ifd);
        let mut decoder = image::codecs::tiff::TiffDecoder::new(Cursor::new(ifd.as_slice()))
            .map_err(|_| Error::decode("TIFF", "TIFF IFD open failed"))?;
        let (w, h) = decoder.dimensions();
        check_dimensions(w, h)?;
        let color = decoder.color_type();
        let orientation = decoder.orientation().ok();
        let total = decoder.total_bytes();
        if total > MAX_PIXELS.saturating_mul(8) {
            return Err(Error::LimitExceeded("TIFF image too large"));
        }
        let mut buf = vec![0u8; total as usize];
        decoder
            .read_image(&mut buf)
            .map_err(|_| Error::decode("TIFF", "TIFF IFD decode failed"))?;
        let mut img = buffer_to_dynimage(w, h, color, buf)
            .ok_or(Error::decode("TIFF", "unsupported TIFF color type"))?;
        if let Some(o) = orientation {
            img.apply_orientation(o);
        }
        out.push(dynimage_to_frame(&img, dpi));
    }
    Ok(out)
}

/// Reconstructs a `DynamicImage` from a decoder's raw output buffer.
fn buffer_to_dynimage(w: u32, h: u32, color: ColorType, buf: Vec<u8>) -> Option<DynamicImage> {
    use image::{GrayAlphaImage, GrayImage, RgbImage, RgbaImage};
    match color {
        ColorType::L8 => GrayImage::from_raw(w, h, buf).map(DynamicImage::ImageLuma8),
        ColorType::La8 => GrayAlphaImage::from_raw(w, h, buf).map(DynamicImage::ImageLumaA8),
        ColorType::Rgb8 => RgbImage::from_raw(w, h, buf).map(DynamicImage::ImageRgb8),
        ColorType::Rgba8 => RgbaImage::from_raw(w, h, buf).map(DynamicImage::ImageRgba8),
        ColorType::L16 => {
            let pixels = cast_u16_be_native(&buf);
            image::ImageBuffer::from_raw(w, h, pixels).map(DynamicImage::ImageLuma16)
        }
        ColorType::La16 => {
            let pixels = cast_u16_be_native(&buf);
            image::ImageBuffer::from_raw(w, h, pixels).map(DynamicImage::ImageLumaA16)
        }
        ColorType::Rgb16 => {
            let pixels = cast_u16_be_native(&buf);
            image::ImageBuffer::from_raw(w, h, pixels).map(DynamicImage::ImageRgb16)
        }
        ColorType::Rgba16 => {
            let pixels = cast_u16_be_native(&buf);
            image::ImageBuffer::from_raw(w, h, pixels).map(DynamicImage::ImageRgba16)
        }
        _ => None,
    }
}

/// Reinterprets a native-endian `u8` buffer as `u16` samples (the `image`
/// decoder writes native-endian 16-bit output).
fn cast_u16_be_native(buf: &[u8]) -> Vec<u16> {
    buf.chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect()
}

/// Reads TIFF XResolution/YResolution (+ResolutionUnit) into dpi from a
/// (standalone, single-IFD) TIFF buffer.
fn parse_tiff_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    let endian = match bytes.get(0..2)? {
        [0x49, 0x49] => Endian::Little,
        [0x4D, 0x4D] => Endian::Big,
        _ => return None,
    };
    if endian.read_u16(bytes, 2)? != 42 {
        return None;
    }
    let dir_off = usize::try_from(endian.read_u32(bytes, 4)?).ok()?;
    let count = usize::from(endian.read_u16(bytes, dir_off)?);
    let mut x_res = None;
    let mut y_res = None;
    let mut unit = 2u16; // default = inch
    for e in 0..count {
        let off = dir_off.checked_add(2)?.checked_add(e.checked_mul(12)?)?;
        let tag = endian.read_u16(bytes, off)?;
        match tag {
            282 => x_res = read_tiff_rational(bytes, endian, off),
            283 => y_res = read_tiff_rational(bytes, endian, off),
            296 => unit = endian.read_u16(bytes, off + 8)?,
            _ => {}
        }
    }
    let (xr, yr) = (x_res?, y_res?);
    match unit {
        2 if xr > 0.0 && yr > 0.0 => Some((xr, yr)), // inch
        3 if xr > 0.0 && yr > 0.0 => Some((xr * 2.54, yr * 2.54)), // cm
        _ => None,
    }
}

/// Reads a RATIONAL (num/den, two u32) from the entry at `entry_off` whose
/// value field holds an offset to the 8-byte rational.
fn read_tiff_rational(bytes: &[u8], endian: Endian, entry_off: usize) -> Option<f64> {
    let rat_off = usize::try_from(endian.read_u32(bytes, entry_off + 8)?).ok()?;
    let num = endian.read_u32(bytes, rat_off)?;
    let den = endian.read_u32(bytes, rat_off + 4)?;
    if den == 0 {
        return None;
    }
    Some(f64::from(num) / f64::from(den))
}

fn codec_name(fmt: image::ImageFormat) -> &'static str {
    match fmt {
        image::ImageFormat::Png => "FlateDecode",
        image::ImageFormat::Jpeg => "DCTDecode",
        image::ImageFormat::Bmp => "BMP",
        image::ImageFormat::WebP => "WEBP",
        image::ImageFormat::Tiff => "TIFF",
        image::ImageFormat::Gif => "GIF",
        _ => "image",
    }
}

/// Rejects dimensions that exceed the untrusted-input caps (bomb guard).
fn check_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::decode("image", "zero-sized image"));
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::LimitExceeded("image dimension exceeds cap"));
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::LimitExceeded("image pixel count exceeds cap"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PDF assembly (via pdf-core)
// ---------------------------------------------------------------------------

/// A minimal, openable zero-page seed PDF (catalog + empty page tree). We open
/// it via `DocumentStore`, then author the image pages and rewire `/Pages`.
fn empty_seed_pdf() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = [0usize; 3];
    offsets[0] = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets[1] = out.len();
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
    let xref_pos = out.len();
    out.extend_from_slice(b"xref\n0 3\n");
    out.extend_from_slice(b"0000000000 65535 f \n");
    out.extend_from_slice(format!("{:010} 00000 n \n", offsets[0]).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", offsets[1]).as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{xref_pos}\n").as_bytes());
    out.extend_from_slice(b"%%EOF");
    out
}

/// Assembles the multi-page image PDF and returns its serialized bytes.
fn build_pdf(frames: &[Frame]) -> Result<Vec<u8>> {
    if frames.is_empty() {
        return Err(Error::InvalidArgument("no image frames to convert"));
    }
    let doc = DocumentStore::from_bytes(empty_seed_pdf(), Limits::default())?;

    let pages_ref = ObjRef::new(2, 0);
    let mut kids: Vec<Object> = Vec::with_capacity(frames.len());

    for (n, frame) in frames.iter().enumerate() {
        let img_name = format!("Img{n}");
        let img_ref = add_image_xobject(&doc, frame, &img_name)?;
        let page_ref = add_page(&doc, frame, img_ref, &img_name, pages_ref)?;
        kids.push(Object::Reference(page_ref));
    }

    // Rewire the page tree: /Kids + /Count.
    let mut pages_dict = Dict::new();
    pages_dict.insert(Name::new("Type"), Object::Name(Name::new("Pages")));
    pages_dict.insert(Name::new("Kids"), Object::Array(kids));
    pages_dict.insert(Name::new("Count"), Object::Integer(frames.len() as i64));
    doc.update_object(pages_ref, Object::Dictionary(pages_dict))?;

    let opts = SaveOptions::default()
        .with_xref_style(XrefStyle::Table)
        .with_deflate(false);
    Ok(doc.save_to_vec(&opts)?)
}

/// Adds the image XObject (and any `/SMask`) and returns the image's `ObjRef`.
fn add_image_xobject(doc: &DocumentStore, frame: &Frame, _name: &str) -> Result<ObjRef> {
    match frame {
        Frame::Jpeg {
            width,
            height,
            components,
            data,
            ..
        } => {
            let cs = match components {
                1 => Object::Name(Name::new("DeviceGray")),
                4 => Object::Name(Name::new("DeviceCMYK")),
                _ => Object::Name(Name::new("DeviceRGB")),
            };
            let mut dict = image_xobject_dict(*width, *height, cs, 8);
            // Adobe CMYK JPEGs are stored inverted; invert via /Decode.
            if *components == 4 {
                dict.insert(
                    Name::new("Decode"),
                    Object::Array(vec![
                        Object::Integer(1),
                        Object::Integer(0),
                        Object::Integer(1),
                        Object::Integer(0),
                        Object::Integer(1),
                        Object::Integer(0),
                        Object::Integer(1),
                        Object::Integer(0),
                    ]),
                );
            }
            dict.insert(Name::new("Filter"), Object::Name(Name::new("DCTDecode")));
            // Embed the original JPEG bytes verbatim (byte-equal passthrough).
            let stream = StreamObj::new_encoded(dict, data.clone());
            Ok(doc.add_object(Object::Stream(stream))?)
        }
        Frame::Decoded {
            width,
            height,
            color,
            alpha,
            palette,
            ..
        } => {
            // Optional soft-mask first, so we can reference it.
            let smask_ref = match alpha {
                Some(a) => Some(add_smask(doc, *width, *height, a)?),
                None => None,
            };

            let (colorspace, samples, bpc) = if let Some(pal) = palette {
                let hival = (pal.palette_rgb.len() / 3).saturating_sub(1) as i64;
                let lut = StreamObj::new_encoded(
                    {
                        let mut d = Dict::new();
                        d.insert(
                            Name::new("Length"),
                            Object::Integer(pal.palette_rgb.len() as i64),
                        );
                        d
                    },
                    pal.palette_rgb.clone(),
                );
                let lut_ref = doc.add_object(Object::Stream(lut))?;
                let cs = Object::Array(vec![
                    Object::Name(Name::new("Indexed")),
                    Object::Name(Name::new("DeviceRGB")),
                    Object::Integer(hival),
                    Object::Reference(lut_ref),
                ]);
                (cs, pal.indices.clone(), 8)
            } else {
                let cs = match color {
                    DecodedColor::Gray => Object::Name(Name::new("DeviceGray")),
                    DecodedColor::Rgb => Object::Name(Name::new("DeviceRGB")),
                };
                (cs, frame_color_bytes(frame), 8)
            };

            let mut dict = image_xobject_dict(*width, *height, colorspace, bpc);
            if let Some(sm) = smask_ref {
                dict.insert(Name::new("SMask"), Object::Reference(sm));
            }
            // Flate-encode the samples ourselves and mark the filter.
            let encoded = flate_encode(&samples);
            dict.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
            let stream = StreamObj::new_encoded(dict, encoded);
            Ok(doc.add_object(Object::Stream(stream))?)
        }
    }
}

/// Borrows the color sample bytes for a decoded (non-palette) frame.
fn frame_color_bytes(frame: &Frame) -> Vec<u8> {
    match frame {
        Frame::Decoded { data, .. } => data.clone(),
        Frame::Jpeg { data, .. } => data.clone(),
    }
}

/// Adds a `/DeviceGray` soft-mask XObject from an alpha plane.
fn add_smask(doc: &DocumentStore, width: u32, height: u32, alpha: &[u8]) -> Result<ObjRef> {
    let mut dict = image_xobject_dict(width, height, Object::Name(Name::new("DeviceGray")), 8);
    let encoded = flate_encode(alpha);
    dict.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
    let stream = StreamObj::new_encoded(dict, encoded);
    Ok(doc.add_object(Object::Stream(stream))?)
}

/// Builds the shared `/XObject /Image` dictionary skeleton.
fn image_xobject_dict(width: u32, height: u32, colorspace: Object, bpc: i64) -> Dict {
    let mut d = Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new("Image")));
    d.insert(Name::new("Width"), Object::Integer(i64::from(width)));
    d.insert(Name::new("Height"), Object::Integer(i64::from(height)));
    d.insert(Name::new("ColorSpace"), colorspace);
    d.insert(Name::new("BitsPerComponent"), Object::Integer(bpc));
    d
}

/// Adds the page object (MediaBox + content stream + resources) referencing the
/// image XObject; returns its `ObjRef`.
fn add_page(
    doc: &DocumentStore,
    frame: &Frame,
    img_ref: ObjRef,
    img_name: &str,
    parent: ObjRef,
) -> Result<ObjRef> {
    let (w_pt, h_pt) = frame.page_size_pt();

    // Content: place the image full-bleed (q w 0 0 h 0 0 cm /ImgN Do Q).
    let content = format!("q {w_pt:.4} 0 0 {h_pt:.4} 0 0 cm /{img_name} Do Q\n");
    let content_ref = {
        let mut d = Dict::new();
        d.insert(Name::new("Length"), Object::Integer(content.len() as i64));
        let stream = StreamObj::new_encoded(d, content.into_bytes());
        doc.add_object(Object::Stream(stream))?
    };

    // Resources: /XObject << /ImgN img_ref >>.
    let mut xobjects = Dict::new();
    xobjects.insert(Name::new(img_name), Object::Reference(img_ref));
    let mut resources = Dict::new();
    resources.insert(Name::new("XObject"), Object::Dictionary(xobjects));

    let media = Object::Array(vec![
        Object::Real(0.0),
        Object::Real(0.0),
        Object::Real(w_pt),
        Object::Real(h_pt),
    ]);

    let mut page = Dict::new();
    page.insert(Name::new("Type"), Object::Name(Name::new("Page")));
    page.insert(Name::new("Parent"), Object::Reference(parent));
    page.insert(Name::new("MediaBox"), media);
    page.insert(Name::new("Contents"), Object::Reference(content_ref));
    page.insert(Name::new("Resources"), Object::Dictionary(resources));
    Ok(doc.add_object(Object::Dictionary(page))?)
}

/// Flate-compresses raw bytes (zlib) for image / smask streams.
fn flate_encode(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to a Vec is infallible.
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// TIFF IFD walking / splitting
// ---------------------------------------------------------------------------
//
// A (possibly multi-IFD) TIFF is split into one standalone single-IFD TIFF per
// IFD so each page/frame can be handed to the `image` crate independently. Each
// standalone buffer is rebuilt with a fresh 8-byte header pointing at a single
// directory, all external entry data (and every strip/tile's pixel bytes)
// copied into a trailing blob with the value/offset fields rewritten. This is
// the verified approach (round-tripped bit-exactly through `image`'s TIFF
// decoder). Every read is bounds/overflow-checked; malformed/cyclic/over-cap
// input yields `None`.

/// Hard ceiling on the size of any single external blob / strip copy and on the
/// total bytes emitted for one standalone IFD, to bound work on untrusted input.
const MAX_BLOB_BYTES: usize = 512 * 1024 * 1024;

/// TIFF byte order discovered from the header.
#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn read_u16(self, buf: &[u8], at: usize) -> Option<u16> {
        let b = buf.get(at..at.checked_add(2)?)?;
        let arr = [b[0], b[1]];
        Some(match self {
            Endian::Little => u16::from_le_bytes(arr),
            Endian::Big => u16::from_be_bytes(arr),
        })
    }

    fn read_u32(self, buf: &[u8], at: usize) -> Option<u32> {
        let b = buf.get(at..at.checked_add(4)?)?;
        let arr = [b[0], b[1], b[2], b[3]];
        Some(match self {
            Endian::Little => u32::from_le_bytes(arr),
            Endian::Big => u32::from_be_bytes(arr),
        })
    }

    fn u16_bytes(self, v: u16) -> [u8; 2] {
        match self {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        }
    }

    fn u32_bytes(self, v: u32) -> [u8; 4] {
        match self {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        }
    }
}

/// Bytes per element for a TIFF field type; unknown types yield 0 (no external data).
fn element_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => 0,             // unknown -> no addressable external data
    }
}

/// A parsed 12-byte IFD entry plus its raw inline value/offset field bytes.
struct Entry {
    tag: u16,
    field_type: u16,
    count: u32,
    /// Raw 4 bytes of the value/offset field, exactly as stored in the source.
    value_field: [u8; 4],
}

/// Total external byte length of an entry's data, or `None` on overflow.
fn entry_byte_len(e: &Entry) -> Option<usize> {
    element_size(e.field_type).checked_mul(usize::try_from(e.count).ok()?)
}

/// Reads the entries of the IFD at `offset` (does not follow the next-IFD link).
fn read_ifd_entries(buf: &[u8], endian: Endian, offset: usize) -> Option<Vec<Entry>> {
    let count = usize::from(endian.read_u16(buf, offset)?);
    let mut entries = Vec::with_capacity(count);
    let mut at = offset.checked_add(2)?;
    for _ in 0..count {
        let tag = endian.read_u16(buf, at)?;
        let field_type = endian.read_u16(buf, at.checked_add(2)?)?;
        let cnt = endian.read_u32(buf, at.checked_add(4)?)?;
        let raw = buf.get(at.checked_add(8)?..at.checked_add(12)?)?;
        entries.push(Entry {
            tag,
            field_type,
            count: cnt,
            value_field: [raw[0], raw[1], raw[2], raw[3]],
        });
        at = at.checked_add(12)?;
    }
    Some(entries)
}

/// Reads `count` integer values (SHORT or LONG) from `data` per `endian`.
fn read_offset_array(
    data: &[u8],
    endian: Endian,
    field_type: u16,
    count: usize,
) -> Option<Vec<u32>> {
    let mut out = Vec::with_capacity(count);
    match field_type {
        3 => {
            for i in 0..count {
                out.push(u32::from(endian.read_u16(data, i.checked_mul(2)?)?));
            }
        }
        4 => {
            for i in 0..count {
                out.push(endian.read_u32(data, i.checked_mul(4)?)?);
            }
        }
        _ => return None,
    }
    Some(out)
}

/// Returns the bytes that hold an entry's values: the inline 4-byte field when
/// `byte_len <= 4`, otherwise the external region at the stored offset.
fn entry_value_bytes<'a>(
    buf: &'a [u8],
    endian: Endian,
    e: &'a Entry,
    byte_len: usize,
) -> Option<&'a [u8]> {
    if byte_len <= 4 {
        Some(&e.value_field)
    } else {
        let off = usize::try_from(endian.read_u32(&e.value_field, 0)?).ok()?;
        buf.get(off..off.checked_add(byte_len)?)
    }
}

/// Splits a (possibly multi-IFD) TIFF into one standalone single-IFD TIFF per IFD.
/// Returns one `Vec<u8>` per IFD, each independently decodable, in on-file chain
/// order. Returns `None` if the bytes are not a valid/parseable TIFF container,
/// or if the structure is malformed, cyclic, or exceeds `max_ifds`/byte caps.
fn split_tiff_ifds(bytes: &[u8], max_ifds: usize) -> Option<Vec<Vec<u8>>> {
    // --- Header ---
    let endian = match bytes.get(0..2)? {
        [0x49, 0x49] => Endian::Little, // "II"
        [0x4D, 0x4D] => Endian::Big,    // "MM"
        _ => return None,
    };
    if endian.read_u16(bytes, 2)? != 42 {
        return None;
    }

    // --- Walk the IFD chain, collecting offsets (cycle/cap guarded) ---
    let mut ifd_offsets: Vec<usize> = Vec::new();
    let mut visited: Vec<usize> = Vec::new();
    let mut next = usize::try_from(endian.read_u32(bytes, 4)?).ok()?;
    while next != 0 {
        if ifd_offsets.len() >= max_ifds {
            return None;
        }
        if visited.contains(&next) {
            return None; // cycle
        }
        visited.push(next);
        ifd_offsets.push(next);

        let count = usize::from(endian.read_u16(bytes, next)?);
        // next-IFD link sits right after entry_count + entries.
        let link_at = next.checked_add(2)?.checked_add(count.checked_mul(12)?)?;
        next = usize::try_from(endian.read_u32(bytes, link_at)?).ok()?;
    }
    if ifd_offsets.is_empty() {
        return None;
    }

    let mut results = Vec::with_capacity(ifd_offsets.len());
    for &ifd_off in &ifd_offsets {
        results.push(build_standalone(bytes, endian, ifd_off)?);
    }
    Some(results)
}

/// Builds one standalone single-IFD TIFF for the IFD at `ifd_off`.
fn build_standalone(bytes: &[u8], endian: Endian, ifd_off: usize) -> Option<Vec<u8>> {
    let entries = read_ifd_entries(bytes, endian, ifd_off)?;
    let entry_count = u16::try_from(entries.len()).ok()?;

    // Layout of the standalone buffer:
    //   [0..8)        header
    //   [8..dir_end)  directory: entry_count(2) + entries(12*n) + next-link(4=0)
    //   [dir_end..)   external blob region (appended sequentially)
    let dir_start = 8usize;
    let dir_bytes = 2usize
        .checked_add(entries.len().checked_mul(12)?)?
        .checked_add(4)?;
    let blob_start = dir_start.checked_add(dir_bytes)?;

    // The 4-byte value/offset field to write for each entry (defaults to original).
    let mut new_value_field: Vec<[u8; 4]> = entries.iter().map(|e| e.value_field).collect();
    let mut blob: Vec<u8> = Vec::new();

    // Append `data` to the blob region, returning its absolute offset in the new buffer.
    let append_blob = |data: &[u8], blob: &mut Vec<u8>| -> Option<u32> {
        if data.len() > MAX_BLOB_BYTES {
            return None;
        }
        let abs = blob_start.checked_add(blob.len())?;
        if blob.len().checked_add(data.len())? > MAX_BLOB_BYTES {
            return None;
        }
        blob.extend_from_slice(data);
        u32::try_from(abs).ok()
    };

    // Locate StripByteCounts / TileByteCounts entries to pair with offsets.
    let find = |tag: u16| entries.iter().find(|e| e.tag == tag);

    // First pass: copy generic external data (everything except the strip/tile
    // *offset* arrays, which need special per-element rewriting below).
    for (i, e) in entries.iter().enumerate() {
        if e.tag == 273 || e.tag == 324 {
            continue; // StripOffsets / TileOffsets handled in the second pass
        }
        let byte_len = entry_byte_len(e)?;
        if byte_len > 4 {
            let data = entry_value_bytes(bytes, endian, e, byte_len)?;
            let new_off = append_blob(data, &mut blob)?;
            new_value_field[i] = endian.u32_bytes(new_off);
        }
    }

    // Second pass: rewrite StripOffsets(273)/TileOffsets(324) and copy pixel data.
    for (i, e) in entries.iter().enumerate() {
        let counts_tag = match e.tag {
            273 => 279, // StripOffsets   <-> StripByteCounts
            324 => 325, // TileOffsets    <-> TileByteCounts
            _ => continue,
        };
        let n = usize::try_from(e.count).ok()?;
        if n == 0 {
            return None;
        }

        // Read the per-strip/tile source offsets (inline or external).
        let off_byte_len = entry_byte_len(e)?;
        let off_data = entry_value_bytes(bytes, endian, e, off_byte_len)?;
        let src_offsets = read_offset_array(off_data, endian, e.field_type, n)?;

        // Read the matching byte counts; must exist and agree in length.
        let bc = find(counts_tag)?;
        if usize::try_from(bc.count).ok()? != n {
            return None;
        }
        let bc_byte_len = entry_byte_len(bc)?;
        let bc_data = entry_value_bytes(bytes, endian, bc, bc_byte_len)?;
        let byte_counts = read_offset_array(bc_data, endian, bc.field_type, n)?;

        // Copy each strip/tile and collect its new offset.
        let mut new_offsets: Vec<u32> = Vec::with_capacity(n);
        for k in 0..n {
            let src = usize::try_from(src_offsets[k]).ok()?;
            let len = usize::try_from(byte_counts[k]).ok()?;
            let pixels = bytes.get(src..src.checked_add(len)?)?;
            new_offsets.push(append_blob(pixels, &mut blob)?);
        }

        // Serialize the rewritten offset array in the entry's own element type.
        let mut arr_bytes: Vec<u8> = Vec::new();
        match e.field_type {
            3 => {
                for v in &new_offsets {
                    let small = u16::try_from(*v).ok()?; // SHORT can't hold a large new offset
                    arr_bytes.extend_from_slice(&endian.u16_bytes(small));
                }
            }
            4 => {
                for v in &new_offsets {
                    arr_bytes.extend_from_slice(&endian.u32_bytes(*v));
                }
            }
            _ => return None,
        }

        if arr_bytes.len() <= 4 {
            // Fits inline: write into the value field, zero-padded.
            let mut field = [0u8; 4];
            field[..arr_bytes.len()].copy_from_slice(&arr_bytes);
            new_value_field[i] = field;
        } else {
            let new_off = append_blob(&arr_bytes, &mut blob)?;
            new_value_field[i] = endian.u32_bytes(new_off);
        }
    }

    // --- Assemble the standalone buffer ---
    let total = blob_start.checked_add(blob.len())?;
    if total > MAX_BLOB_BYTES {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(total);

    // Header: byte-order mark, magic 42, IFD0 offset = 8.
    match endian {
        Endian::Little => out.extend_from_slice(&[0x49, 0x49]),
        Endian::Big => out.extend_from_slice(&[0x4D, 0x4D]),
    }
    out.extend_from_slice(&endian.u16_bytes(42));
    out.extend_from_slice(&endian.u32_bytes(u32::try_from(dir_start).ok()?));

    // Directory: entry_count, rewritten entries, next-IFD = 0.
    out.extend_from_slice(&endian.u16_bytes(entry_count));
    for (i, e) in entries.iter().enumerate() {
        out.extend_from_slice(&endian.u16_bytes(e.tag));
        out.extend_from_slice(&endian.u16_bytes(e.field_type));
        out.extend_from_slice(&endian.u32_bytes(e.count));
        out.extend_from_slice(&new_value_field[i]);
    }
    out.extend_from_slice(&endian.u32_bytes(0));

    // External blob region.
    out.extend_from_slice(&blob);

    Some(out)
}
