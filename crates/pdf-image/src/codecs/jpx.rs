//! JPXDecode (JPEG 2000) image-XObject decode — PRD §8.4.1 (documented subset).
//!
//! Decoder: `hayro-jpeg2000`, which auto-detects both JP2-boxed files and raw
//! J2K codestreams (the two forms a PDF `/JPXDecode` stream may take). The
//! optional OpenJPEG (C) fallback is deliberately **not** wired (PRD §8.4.1: it
//! breaks the pure-Rust / `#![forbid(unsafe_code)]` / WASM-clean guarantees).
//!
//! ## Documented subset (§8.4.1) & fail-closed
//!
//! Target coverage is **baseline JP2/J2K with the common color spaces (gray,
//! sRGB, YCC) at 8/16-bit**; exotic component transforms / extended
//! capabilities are best-effort. sYCC is converted to RGB by the decoder; CIELab
//! and other ICC-profiled spaces surface as `Icc{…}` and are classified by their
//! channel count. Output is delivered as 8-bit interleaved samples (the decoder
//! scales >8-bit components down to 8-bit in its convenience packer).
//!
//! Per the §8.4.1 degradation contract, any decode failure / unsupported feature
//! / resource-cap hit returns a typed error for *this image only* — never a
//! panic. `hayro-jpeg2000` routes malformed input to `Result`, but its
//! decode/color-conversion path has `[0]`-indexing and `unwrap` sites that
//! assume post-validation invariants, so the entire `Image::new + decode` is
//! wrapped in `catch_unwind` (turning any stray panic into a typed error). A
//! declared raster beyond the global pixel cap trips [`Error::LimitExceeded`].

use crate::codecs::{guard_dimensions, ColorSpaceHint, DecodedImage};
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore};

use hayro_jpeg2000::{ColorSpace, DecodeSettings, DecoderContext, Image};

const CODEC: &str = "JPXDecode";

/// Decodes a JPXDecode (JPEG 2000) image stream to a [`DecodedImage`].
///
/// `data` is the JP2/J2K codestream; `params` is the image stream dict (for JPX,
/// `/ColorSpace` is often omitted and taken from the codestream); `doc` is
/// currently unused (the codestream is self-describing) but kept for the codec
/// signature contract.
pub fn decode(_doc: &DocumentStore, data: &[u8], _params: &Dict) -> Result<DecodedImage> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_inner(data)));
    match result {
        Ok(inner) => inner,
        Err(_) => Err(Error::Unsupported(CODEC)),
    }
}

fn decode_inner(data: &[u8]) -> Result<DecodedImage> {
    let settings = DecodeSettings::default();
    let image = Image::new(data, &settings).map_err(|_| Error::Unsupported(CODEC))?;

    let width = image.width();
    let height = image.height();
    guard_dimensions(width, height, CODEC)?;

    let has_alpha = image.has_alpha();
    // Classify the *color* channel layout (excluding alpha) into the
    // DecodedImage component model. The pixmap layer applies any /SMask alpha.
    let (color_components, hint) = match image.color_space() {
        ColorSpace::Gray => (1u8, ColorSpaceHint::Gray),
        ColorSpace::RGB => (3u8, ColorSpaceHint::Rgb),
        ColorSpace::CMYK => (4u8, ColorSpaceHint::Cmyk),
        ColorSpace::Icc { num_channels, .. } => classify_by_channels(*num_channels),
        ColorSpace::Unknown { num_channels } => classify_by_channels(*num_channels),
    };

    let mut ctx = DecoderContext::default();
    let decoded = image
        .decode(&mut ctx)
        .map_err(|_| Error::Unsupported(CODEC))?;

    // `data_u8()` is interleaved, 8-bit, alpha last (when present).
    let interleaved = decoded.data_u8();
    let total_channels = color_components as usize + usize::from(has_alpha);

    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|p| p.checked_mul(total_channels))
        .ok_or(Error::LimitExceeded("JPX raster size overflow"))?;
    if interleaved.len() != expected {
        return Err(Error::decode(CODEC, "JPX output size mismatch"));
    }

    // Drop the alpha channel from the color raster (alpha → /SMask is the
    // pixmap layer's job; DecodedImage carries color components only).
    let data = if has_alpha {
        strip_alpha(&interleaved, color_components as usize)
    } else {
        interleaved
    };

    Ok(DecodedImage::new(
        width,
        height,
        color_components,
        8,
        hint,
        data,
    ))
}

/// Maps a raw channel count (from an `Icc`/`Unknown` colorspace) to a component
/// model. 1 ⇒ gray, 3 ⇒ rgb, 4 ⇒ cmyk; anything else is reported as its channel
/// count with an [`ColorSpaceHint::Unknown`] hint (clamped to a sane component
/// value so the pixmap layer can still inspect it).
fn classify_by_channels(n: u8) -> (u8, ColorSpaceHint) {
    match n {
        1 => (1, ColorSpaceHint::Gray),
        3 => (3, ColorSpaceHint::Rgb),
        4 => (4, ColorSpaceHint::Cmyk),
        other => (other.max(1), ColorSpaceHint::Unknown),
    }
}

/// Removes the trailing alpha channel from an interleaved 8-bit raster with
/// `color + 1` channels per pixel, yielding a `color`-channel raster.
fn strip_alpha(interleaved: &[u8], color: usize) -> Vec<u8> {
    let stride = color + 1;
    let pixels = interleaved.len() / stride;
    let mut out = Vec::with_capacity(pixels * color);
    for px in interleaved.chunks_exact(stride) {
        out.extend_from_slice(&px[..color]);
    }
    out
}
