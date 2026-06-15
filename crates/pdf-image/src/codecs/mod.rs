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

use pdf_core::{Dict, DocumentStore};

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
        "JPXDecode" => jpx::decode(doc, data, params),
        _ => Err(Error::Unsupported("decode_image_xobject")),
    }
}
