//! Image sourcing for `![alt](src)` — local paths and `data:` URIs only.
//!
//! **No network access, ever**: `http:` / `https:` sources yield a typed error.
//! Relative paths resolve against `Options::base_dir`; without a base dir only
//! absolute paths (and `data:` URIs) are accepted, keeping resolution explicit
//! and deterministic.
//!
//! JPEG bytes embed verbatim as `/DCTDecode` (no re-encode); every other
//! supported raster (PNG/BMP/GIF/WEBP/TIFF — whatever `pdf-image` decodes) is
//! decoded to 8-bit Gray/RGB samples first (alpha composited over white) and
//! embeds as `/FlateDecode`.

use std::path::Path;

use pdf_core::error::{Error, Result};
use pdf_image::imagedoc::{image_profile, open_image_document, ImageFormat};
use pdf_image::pixmap::Colorspace;

use crate::model::{Block, ListItem};
use crate::Options;

/// A decoded, embed-ready image.
pub(crate) enum PreparedImage {
    /// Verbatim JPEG bytes (`/DCTDecode` passthrough).
    Jpeg {
        width: u32,
        height: u32,
        /// 1 = Gray, 3 = RGB/YCbCr, 4 = CMYK (Adobe-inverted, needs `/Decode`).
        components: u8,
        data: Vec<u8>,
    },
    /// Decoded 8-bit samples (`/FlateDecode`), interleaved row-major.
    Raw {
        width: u32,
        height: u32,
        /// `true` → 1 byte/pixel `/DeviceGray`, else 3 bytes/pixel `/DeviceRGB`.
        gray: bool,
        data: Vec<u8>,
    },
}

impl PreparedImage {
    /// Pixel dimensions.
    pub(crate) fn size(&self) -> (u32, u32) {
        match self {
            PreparedImage::Jpeg { width, height, .. }
            | PreparedImage::Raw { width, height, .. } => (*width, *height),
        }
    }
}

/// Walks `blocks` in document order, loading every image block's source and
/// assigning its index into the returned prepared-image list.
///
/// # Errors
///
/// Typed errors for remote URLs, unresolvable paths, malformed `data:` URIs
/// and undecodable image bytes (never panics).
pub(crate) fn resolve_images(blocks: &mut [Block], opts: &Options) -> Result<Vec<PreparedImage>> {
    let mut prepared = Vec::new();
    walk(blocks, opts, &mut prepared)?;
    Ok(prepared)
}

fn walk(blocks: &mut [Block], opts: &Options, out: &mut Vec<PreparedImage>) -> Result<()> {
    for block in blocks {
        match block {
            Block::Image { src, id } => {
                let bytes = load_source(src, opts)?;
                out.push(prepare(&bytes)?);
                *id = out.len() - 1;
            }
            Block::Quote(children) => walk(children, opts, out)?,
            Block::List { items, .. } => {
                for ListItem { blocks, .. } in items {
                    walk(blocks, opts, out)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Loads the raw bytes for an image source (path or `data:` URI).
fn load_source(src: &str, opts: &Options) -> Result<Vec<u8>> {
    if let Some(rest) = src.strip_prefix("data:") {
        return decode_data_uri(rest);
    }
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("http:") || lower.starts_with("https:") || lower.contains("://") {
        return Err(Error::Unsupported(
            "markdown_to_pdf: remote image URLs are not fetched (local paths and data: URIs only)",
        ));
    }
    let path = Path::new(src);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base) = &opts.base_dir {
        base.join(path)
    } else {
        return Err(Error::InvalidArgument(
            "markdown_to_pdf: relative image path requires Options::base_dir",
        ));
    };
    Ok(std::fs::read(full)?)
}

/// Decodes the payload of a `data:` URI (`rest` is everything after `data:`).
/// Only base64 payloads are supported.
fn decode_data_uri(rest: &str) -> Result<Vec<u8>> {
    let Some((meta, payload)) = rest.split_once(',') else {
        return Err(Error::InvalidArgument(
            "markdown_to_pdf: malformed data: URI (no comma)",
        ));
    };
    if !meta.ends_with(";base64") {
        return Err(Error::Unsupported(
            "markdown_to_pdf: only base64 data: URIs are supported",
        ));
    }
    base64_decode(payload).ok_or(Error::InvalidArgument(
        "markdown_to_pdf: invalid base64 in data: URI",
    ))
}

/// Decodes standard / URL-safe base64 (whitespace ignored, `=` padding).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some(u32::from(b - b'A')),
            b'a'..=b'z' => Some(u32::from(b - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(b - b'0') + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut n = 0u32;
    let mut padded = false;
    for &b in s.as_bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            padded = true;
            continue;
        }
        if padded {
            return None; // data after padding
        }
        acc = (acc << 6) | val(b)?;
        n += 1;
        if n == 4 {
            out.push((acc >> 16) as u8);
            out.push((acc >> 8) as u8);
            out.push(acc as u8);
            acc = 0;
            n = 0;
        }
    }
    match n {
        0 => {}
        2 => out.push((acc >> 4) as u8),
        3 => {
            out.push((acc >> 10) as u8);
            out.push((acc >> 2) as u8);
        }
        _ => return None,
    }
    Some(out)
}

/// Sniffs and decodes image `bytes` into an embed-ready form.
fn prepare(bytes: &[u8]) -> Result<PreparedImage> {
    let format = ImageFormat::sniff(bytes).ok_or(Error::InvalidArgument(
        "markdown_to_pdf: unrecognized image format",
    ))?;
    if format == ImageFormat::Jpeg {
        let profile = image_profile(bytes).ok_or(Error::InvalidArgument(
            "markdown_to_pdf: unparseable JPEG image",
        ))?;
        let components = match profile.colorspace {
            1 => 1u8,
            4 => 4u8,
            _ => 3u8,
        };
        return Ok(PreparedImage::Jpeg {
            width: profile.width,
            height: profile.height,
            components,
            data: bytes.to_vec(),
        });
    }
    let doc = open_image_document(bytes, Some(format))
        .map_err(|_| Error::InvalidArgument("markdown_to_pdf: image decode failed"))?;
    let Some(pix) = doc.pages.first() else {
        return Err(Error::InvalidArgument(
            "markdown_to_pdf: image has no frames",
        ));
    };
    let gray = pix.colorspace == Colorspace::Gray;
    let comps = usize::from(pix.colorspace.components());
    let stride = comps + usize::from(pix.alpha);
    let pixels = (pix.width as usize) * (pix.height as usize);
    let mut data = Vec::with_capacity(pixels * comps);
    for p in 0..pixels {
        let base = p * stride;
        let a = if pix.alpha {
            f64::from(pix.samples[base + comps]) / 255.0
        } else {
            1.0
        };
        for c in 0..comps {
            let v = f64::from(pix.samples[base + c]);
            // Composite over white so transparency degrades predictably.
            let out_v = (v * a + 255.0 * (1.0 - a)).round().clamp(0.0, 255.0);
            data.push(out_v as u8);
        }
    }
    Ok(PreparedImage::Raw {
        width: pix.width,
        height: pix.height,
        gray,
        data,
    })
}
