//! Image-XObject codecs (PRD §8.4 / §8.4.1).
//!
//! Each submodule decodes one PDF image filter to a raw [`DecodedImage`]
//! (interleaved samples, big-endian for >8 bpc), which the [`crate::pixmap`]
//! layer then turns into a `Pixmap` honoring `/ColorSpace` and `/Decode`.
//!
//! Codecs are **total** (PRD §8.1 / §8.4.1): any input yields a typed
//! [`crate::error::Error`] (`Decode` / `Unsupported` / `LimitExceeded`), never a
//! panic. JBIG2/JPX are the documented-subset codecs — on any unsupported region
//! / feature they return [`crate::error::Error::Unsupported`] for *that image*
//! only (the degradation contract).
//!
//! ## Module ownership (M5 parallel units)
//!
//! - [`dct`] — DCTDecode (JPEG, baseline + progressive; YCbCr/CMYK incl.
//!   inverted Adobe APP14). Decoder: `zune-jpeg` (cross-checked vs `jpeg-decoder`).
//! - [`ccitt`] — CCITTFaxDecode G3/G4 (`/K /Columns /Rows /BlackIs1 /EncodedByteAlign`).
//!   Decoder: `fax` (alt: `hayro-ccitt`).
//! - [`jbig2`] — JBIG2Decode, subset §8.4.1 (generic + symbol-dict + text region).
//!   Decoder: `hayro-jbig2`.
//! - [`jpx`] — JPXDecode / JPEG2000, subset §8.4.1 (baseline JP2/J2K, gray/sRGB/YCC).
//!   Decoder: `hayro-jpeg2000`.

pub mod ccitt;
pub mod dct;
pub mod jbig2;
pub mod jpx;

use crate::error::{Error, Result};

use pdf_core::colorspace::ColorSpace;
use pdf_core::{decode_stream, DecodeOutcome, Dict, DocumentStore, Name, Object};

/// Absolute ceiling on a decoded image's pixel count (width × height),
/// independent of the per-stream byte ceilings in [`pdf_core::Limits`]. This is
/// the decompression-bomb / never-OOM guard for the image codecs (PRD §9.6.2 /
/// §8.4.1): a header may *declare* an enormous raster while the encoded payload
/// is tiny, so the codecs check declared dimensions against this cap **before**
/// allocating an output buffer. 256 megapixels (≈ a 16384 × 16384 image) is far
/// above any real document page yet bounds the worst-case allocation
/// (256 Mpx × 4 components × 2 bytes ≈ 2 GiB) to a recoverable, typed error.
pub(crate) const MAX_IMAGE_PIXELS: u64 = 256 * 1024 * 1024;

/// Validates a declared image geometry against [`MAX_IMAGE_PIXELS`] and rejects
/// degenerate (zero) dimensions, returning the pixel count on success.
///
/// Called by every codec before allocating its output raster so a hostile
/// header (`/Width 60000 /Height 60000`) trips a typed
/// [`Error::LimitExceeded`] instead of an OOM abort.
pub(crate) fn guard_dimensions(width: u32, height: u32, codec: &'static str) -> Result<u64> {
    if width == 0 || height == 0 {
        return Err(Error::decode(codec, "zero image dimension"));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(Error::LimitExceeded("image pixel count exceeds cap"));
    }
    Ok(pixels)
}

/// Looks up `key` in `dict`, resolving it through `doc` if it is an indirect
/// reference. Returns `None` for a missing key or a `Null` value. Helper shared
/// by the codecs for reading optional `/DecodeParms` entries that PDFs may store
/// either inline or as indirect objects.
pub(crate) fn resolved(doc: &DocumentStore, dict: &Dict, key: &str) -> Option<Object> {
    let v = dict.get(&Name::new(key))?;
    let obj = match v {
        Object::Reference(r) => doc.resolve(*r).ok()?.as_ref().clone(),
        other => other.clone(),
    };
    if obj.is_null() {
        None
    } else {
        Some(obj)
    }
}

/// Reads an optional integer `/DecodeParms`-style entry (resolving indirect
/// refs).
pub(crate) fn param_i64(doc: &DocumentStore, dict: &Dict, key: &str) -> Option<i64> {
    resolved(doc, dict, key).and_then(|o| o.as_i64())
}

/// Reads an optional boolean entry (resolving indirect refs); absent ⇒ `default`.
pub(crate) fn param_bool(doc: &DocumentStore, dict: &Dict, key: &str, default: bool) -> bool {
    resolved(doc, dict, key)
        .and_then(|o| o.as_bool())
        .unwrap_or(default)
}

/// A colorspace hint carried alongside a decoded image.
///
/// The codec reports what it knows from the codec headers (e.g. JPEG component
/// count / Adobe transform, JPX enumerated colorspace). The PDF stream's
/// `/ColorSpace` is authoritative and reconciled by the [`crate::pixmap`] layer;
/// this hint disambiguates the cases a codec can resolve on its own (notably
/// JPEG CMYK vs YCCK and the Adobe inversion).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColorSpaceHint {
    /// Single-component grayscale.
    Gray,
    /// Three-component RGB (already converted from YCbCr if applicable).
    Rgb,
    /// Four-component CMYK (Adobe-inverted samples already un-inverted, or the
    /// inversion left to the `/Decode` array — documented by the implementer).
    Cmyk,
    /// Codec could not classify the colorspace; the PDF `/ColorSpace` decides.
    Unknown,
}

/// A raw decoded image as produced by a codec, before colorspace/`/Decode`
/// interpretation.
///
/// This is the **shared contract** every codec fills and the [`crate::pixmap`]
/// layer consumes. Fields are deliberately minimal; the parallel implementers
/// add helper methods (not new required fields) as needed.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Number of interleaved color components per pixel (1=gray, 3=rgb, 4=cmyk).
    pub components: u8,
    /// Bits per component (1, 2, 4, 8, or 16).
    pub bits: u8,
    /// Codec's colorspace classification (see [`ColorSpaceHint`]).
    pub colorspace: ColorSpaceHint,
    /// Interleaved sample bytes, row-major, no row padding. For 16 bpc the bytes
    /// are big-endian (PDF convention). Length is implementer-validated against
    /// `width`/`height`/`components`/`bits`.
    pub data: Vec<u8>,
}

impl DecodedImage {
    /// Constructs a [`DecodedImage`] from already-decoded fields. Provided so the
    /// codec submodules share one constructor; performs no validation in the
    /// scaffold (implementers add bounds checks).
    #[must_use]
    pub fn new(
        width: u32,
        height: u32,
        components: u8,
        bits: u8,
        colorspace: ColorSpaceHint,
        data: Vec<u8>,
    ) -> Self {
        DecodedImage {
            width,
            height,
            components,
            bits,
            colorspace,
            data,
        }
    }
}

/// Dispatches an image XObject (or inline image) to the codec named by its PDF
/// image filter and returns the raw [`DecodedImage`].
///
/// `filter` is the canonical PDF filter name as reported by
/// [`pdf_core::DecodeOutcome::ImageEncoded`] (`"DCTDecode"`, `"CCITTFaxDecode"`,
/// `"JBIG2Decode"`, `"JPXDecode"`). `data` is the codec-encoded byte payload
/// (already past any preceding Flate/LZW/ASCII filters in the chain). `params`
/// is the image stream dict (carrying `/Width /Height /BitsPerComponent
/// /ColorSpace /DecodeParms` etc.); `doc` resolves any indirect references in
/// `params` (e.g. a `/JBIG2Globals` stream, an indexed `/ColorSpace` lookup).
///
/// Scaffold behavior: returns [`Error::Unsupported`] for the recognized filters
/// (delegating to the per-codec stubs) and for any unrecognized name.
pub fn decode_image_xobject(
    doc: &DocumentStore,
    filter: &str,
    data: &[u8],
    params: &Dict,
) -> Result<DecodedImage> {
    match filter {
        "DCTDecode" | "DCT" => dct::decode(doc, data, params),
        "CCITTFaxDecode" | "CCF" => ccitt::decode(doc, data, params),
        "JBIG2Decode" => jbig2::decode(doc, data, params),
        "JPXDecode" | "JPX" => jpx::decode(doc, data, params),
        // Already-pixel filters: the sample bytes themselves are produced by
        // pdf-core's Flate/LZW/ASCII*/RunLength layer, then interpreted here as
        // raw samples per `/Width /Height /ColorSpace /BitsPerComponent`. An
        // empty / `""` filter means "no image filter" (raw or Flate/LZW already
        // applied upstream).
        "" | "FlateDecode" | "Fl" | "LZWDecode" | "LZW" | "ASCIIHexDecode" | "ASCII85Decode"
        | "RunLengthDecode" => decode_pixel_samples(doc, filter, data, params),
        _ => Err(Error::Unsupported("decode_image_xobject")),
    }
}

/// Number of color components implied by an image's `/ColorSpace`.
///
/// Resolves the colorspace through `doc` (it may be an indirect ref or an
/// `[/ICCBased N]` / `[/Indexed …]` array) and maps it to a component count and
/// a [`ColorSpaceHint`]. Conservative: anything unrecognized falls back to the
/// PDF default of `DeviceGray` (1 component) unless `/BitsPerComponent` and the
/// sample length disambiguate.
fn colorspace_components(doc: &DocumentStore, params: &Dict) -> (u8, ColorSpaceHint) {
    let Some(cs) = resolved(doc, params, "ColorSpace") else {
        return (1, ColorSpaceHint::Gray);
    };
    classify_colorspace(doc, &cs)
}

fn classify_colorspace(doc: &DocumentStore, cs: &Object) -> (u8, ColorSpaceHint) {
    match cs {
        Object::Name(n) => match n.as_str() {
            Some("DeviceGray" | "G" | "CalGray") => (1, ColorSpaceHint::Gray),
            Some("DeviceRGB" | "RGB" | "CalRGB" | "Lab") => (3, ColorSpaceHint::Rgb),
            Some("DeviceCMYK" | "CMYK") => (4, ColorSpaceHint::Cmyk),
            _ => (1, ColorSpaceHint::Unknown),
        },
        Object::Array(arr) => classify_colorspace_array(doc, arr),
        _ => (1, ColorSpaceHint::Unknown),
    }
}

fn classify_colorspace_array(doc: &DocumentStore, arr: &[Object]) -> (u8, ColorSpaceHint) {
    let Some(head) = arr.first().and_then(Object::as_name).and_then(Name::as_str) else {
        return (1, ColorSpaceHint::Unknown);
    };
    match head {
        // Indexed/palette: the *samples* are single-component indices; the
        // pixmap layer expands the palette. Treat as 1 component here.
        "Indexed" | "I" => (1, ColorSpaceHint::Unknown),
        "ICCBased" => {
            // /N is on the stream dict of the ICC profile (2nd array element).
            let n = arr
                .get(1)
                .and_then(|o| match o {
                    Object::Reference(r) => doc.resolve(*r).ok().map(|a| a.as_ref().clone()),
                    other => Some(other.clone()),
                })
                .and_then(|o| o.as_dict().and_then(|d| d.get(&Name::new("N")).cloned()))
                .and_then(|o| o.as_i64());
            match n {
                Some(1) => (1, ColorSpaceHint::Gray),
                Some(3) => (3, ColorSpaceHint::Rgb),
                Some(4) => (4, ColorSpaceHint::Cmyk),
                _ => (3, ColorSpaceHint::Rgb),
            }
        }
        "CalGray" => (1, ColorSpaceHint::Gray),
        "CalRGB" | "Lab" => (3, ColorSpaceHint::Rgb),
        "DeviceN" => {
            // 2nd element is the array of colorant names ⇒ component count.
            let n = arr.get(1).and_then(Object::as_array).map(|a| a.len() as u8);
            (n.unwrap_or(1), ColorSpaceHint::Unknown)
        }
        "Separation" => (1, ColorSpaceHint::Unknown),
        _ => (1, ColorSpaceHint::Unknown),
    }
}

/// Resolves an image stream's `/ColorSpace` (resolving an indirect ref) into a
/// [`ColorSpace`] for the colorspace-aware pixmap path (Indexed / Separation /
/// DeviceN / Lab expansion). `None` for an `/ImageMask`, an unresolvable space,
/// or a plain Device/ICC space the byte-layout already matches (those keep the
/// fast interleaved path).
#[must_use]
pub fn image_colorspace(doc: &DocumentStore, params: &Dict) -> Option<ColorSpace> {
    let cs = resolved(doc, params, "ColorSpace").or_else(|| resolved(doc, params, "CS"))?;
    ColorSpace::resolve(doc, &cs, None)
}

/// Reads an image stream's `/Decode` (or inline `/D`) array as per-component
/// `[lo, hi]` pairs, or an empty `Vec` when absent / malformed (the default
/// identity ranges).
#[must_use]
pub fn image_decode(doc: &DocumentStore, params: &Dict) -> Vec<[f32; 2]> {
    let Some(arr) = resolved(doc, params, "Decode")
        .or_else(|| resolved(doc, params, "D"))
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
    else {
        return Vec::new();
    };
    let flat: Vec<f32> = arr
        .iter()
        .filter_map(|o| o.as_f64().map(|v| v as f32))
        .collect();
    flat.chunks_exact(2).map(|c| [c[0], c[1]]).collect()
}

/// Whether an image's terminal filter is a self-decoding pixel codec (DCT / JPX)
/// that already yields device Gray/RGB/CMYK samples and applies `/Decode`
/// (Adobe-inversion) internally — so the generic colorspace/`/Decode`
/// post-processing in [`crate::pixmap::Pixmap::from_decoded_cs`] must be skipped
/// for it (to avoid double-applying `/Decode`).
#[must_use]
fn is_self_decoding_filter(doc: &DocumentStore, params: &Dict) -> bool {
    let f = resolved(doc, params, "Filter").or_else(|| resolved(doc, params, "F"));
    let name = match f {
        Some(Object::Name(n)) => n.as_str().map(str::to_string),
        Some(Object::Array(a)) => a
            .last()
            .and_then(Object::as_name)
            .and_then(Name::as_str)
            .map(str::to_string),
        _ => None,
    };
    matches!(
        name.as_deref(),
        Some("DCTDecode" | "DCT" | "JPXDecode" | "JPX")
    )
}

/// Decodes an image XObject **stream** end-to-end to a [`crate::pixmap::Pixmap`],
/// honoring its `/ColorSpace` (Indexed palette lookup, Separation/DeviceN tint
/// transform, Lab) and `/Decode` array (PRD §8.10 / P3-3).
///
/// This is the single colorspace-aware decode path shared by `get_pixmap`, the
/// renderer's image op, and the SVG sink, so palette/tint resolution lives in one
/// place. For DCT/JPX (self-decoding) images the colorspace/`/Decode` are already
/// resolved by the codec, so it falls back to the plain device pixmap.
///
/// # Errors
///
/// Propagates the codec's typed decode error, or [`Error::Decode`] for an
/// unsupported component count / geometry.
pub fn pixmap_from_stream(
    doc: &DocumentStore,
    params: &Dict,
    raw: &[u8],
) -> Result<crate::pixmap::Pixmap> {
    let decoded = decode_image_stream(doc, params, raw)?;
    if is_self_decoding_filter(doc, params) {
        return crate::pixmap::Pixmap::from_decoded(&decoded);
    }
    let cs = image_colorspace(doc, params);
    let decode = image_decode(doc, params);
    crate::pixmap::Pixmap::from_decoded_cs(&decoded, cs.as_ref(), &decode)
}

/// Like [`pixmap_from_stream`] but for an already-decoded [`DecodedImage`] plus
/// its source `params` (when the caller already ran the codec). Used by the
/// render/SVG sinks that decode once and reuse the [`DecodedImage`].
pub fn pixmap_from_decoded(
    doc: &DocumentStore,
    params: &Dict,
    decoded: &DecodedImage,
) -> Result<crate::pixmap::Pixmap> {
    if is_self_decoding_filter(doc, params) {
        return crate::pixmap::Pixmap::from_decoded(decoded);
    }
    let cs = image_colorspace(doc, params);
    let decode = image_decode(doc, params);
    crate::pixmap::Pixmap::from_decoded_cs(decoded, cs.as_ref(), &decode)
}

/// Interprets already-decompressed sample bytes (the output of pdf-core's
/// Flate/LZW/ASCII*/RunLength layer) as a raw raster per the image dict.
fn decode_pixel_samples(
    doc: &DocumentStore,
    _filter: &str,
    data: &[u8],
    params: &Dict,
) -> Result<DecodedImage> {
    let width = param_i64(doc, params, "Width")
        .or_else(|| param_i64(doc, params, "W"))
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| Error::decode("Image", "missing or invalid /Width"))?;
    let height = param_i64(doc, params, "Height")
        .or_else(|| param_i64(doc, params, "H"))
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| Error::decode("Image", "missing or invalid /Height"))?;
    guard_dimensions(width, height, "Image")?;

    let image_mask =
        param_bool(doc, params, "ImageMask", false) || param_bool(doc, params, "IM", false);
    let (components, hint, bits) = if image_mask {
        (1u8, ColorSpaceHint::Gray, 1u8)
    } else {
        let bpc = param_i64(doc, params, "BitsPerComponent")
            .or_else(|| param_i64(doc, params, "BPC"))
            .unwrap_or(8);
        let bits = match bpc {
            1 | 2 | 4 | 8 | 16 => bpc as u8,
            _ => return Err(Error::decode("Image", "unsupported /BitsPerComponent")),
        };
        let (c, h) = colorspace_components(doc, params);
        (c, h, bits)
    };

    // Required (unpadded-row) length: rows are byte-aligned per PDF, so compute
    // bytes-per-row from the bit width, then strip row padding into the packed
    // `DecodedImage` layout (no row padding) the pixmap layer expects.
    let bits_per_row = u64::from(width) * u64::from(components) * u64::from(bits);
    let row_bytes = bits_per_row.div_ceil(8) as usize;
    let needed = row_bytes
        .checked_mul(height as usize)
        .ok_or(Error::LimitExceeded("image raster size overflow"))?;
    if data.len() < needed {
        return Err(Error::decode("Image", "truncated sample data"));
    }

    // Strip per-row byte padding so the output has no row padding. For widths
    // whose bit-rows already land on a byte boundary this is a straight copy.
    let packed_row_bits = bits_per_row;
    let out = if packed_row_bits % 8 == 0 {
        data[..needed].to_vec()
    } else {
        let packed_row_bytes = (packed_row_bits / 8) as usize; // full bytes
        let trailing_bits = (packed_row_bits % 8) as usize;
        let mut out = Vec::with_capacity(height as usize * (packed_row_bytes + 1));
        for r in 0..height as usize {
            let row = &data[r * row_bytes..r * row_bytes + row_bytes];
            out.extend_from_slice(&row[..packed_row_bytes]);
            if trailing_bits > 0 {
                // Keep the partial final byte (high bits significant).
                out.push(row[packed_row_bytes]);
            }
        }
        out
    };

    Ok(DecodedImage::new(
        width, height, components, bits, hint, out,
    ))
}

/// Convenience entry that resolves and decompresses an image XObject *stream*
/// end-to-end: it runs pdf-core's filter chain (materializing a source-backed
/// `Raw` body and applying any Flate/LZW/ASCII* prefix), then dispatches the
/// terminal image filter (or the raw samples) to the right codec.
///
/// This is the path the pixmap / image-doc layers use; tests that already hold
/// the codec payload call [`decode_image_xobject`] directly.
pub fn decode_image_stream(doc: &DocumentStore, params: &Dict, raw: &[u8]) -> Result<DecodedImage> {
    match decode_stream(params, raw, doc.limits())? {
        DecodeOutcome::ImageEncoded { filter, bytes } => {
            decode_image_xobject(doc, filter, &bytes, params)
        }
        DecodeOutcome::Decoded(bytes) => decode_pixel_samples(doc, "", &bytes, params),
    }
}
