//! DCTDecode (JPEG) image-XObject decode — PRD §8.4 / §11.1.
//!
//! Baseline + progressive JPEG; YCbCr/CMYK incl. inverted Adobe APP14. Primary
//! decoder `zune-jpeg`, cross-checked against `jpeg-decoder` in the test suite.
//! Implemented in the M5-codecs unit.
//!
//! ## Colorspace handling
//!
//! - **1 component** ⇒ grayscale ([`ColorSpaceHint::Gray`]).
//! - **3 components** ⇒ `zune-jpeg` converts YCbCr → RGB by default; output is
//!   RGB ([`ColorSpaceHint::Rgb`]).
//! - **4 components** ⇒ requested as native CMYK (no RGB conversion). The input
//!   may be true CMYK or Adobe **YCCK** (APP14 transform = 2); `zune-jpeg`
//!   converts YCCK → CMYK for us, so the output is always 4-channel CMYK.
//!
//! ## Adobe APP14 inversion & the PDF `/Decode` array
//!
//! Adobe CMYK/YCCK JPEGs store **inverted** ink values (0 = full ink). PDF
//! producers compensate with a `/Decode [1 0 1 0 1 0 1 0]` array on the image
//! dict. We therefore leave the samples exactly as `zune-jpeg` produced them
//! (so a present `/Decode` array stays meaningful) and additionally apply a
//! `/Decode`-driven inversion here when the array requests it, matching the PDF
//! imaging model. When the JPEG carries an Adobe APP14 marker but the dict has
//! **no** `/Decode`, we invert the CMYK samples so the un-inverted ink values
//! reach the pixmap layer (the common Acrobat-produced case).

use crate::codecs::{guard_dimensions, ColorSpaceHint, DecodedImage};
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore, Name, Object};

use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

const CODEC: &str = "DCTDecode";

/// Decodes a DCTDecode (JPEG) image stream to a [`DecodedImage`].
///
/// `data` is the raw JFIF/JPEG byte stream; `params` is the image stream dict
/// (`/Width /Height /ColorSpace /Decode /DecodeParms`); `doc` resolves indirect
/// references in `params` (currently only `/Decode`, which is virtually always
/// inline). Any malformed/truncated/unsupported JPEG yields a typed
/// [`Error::Decode`] — never a panic (the §8.4.1 degradation contract).
pub fn decode(doc: &DocumentStore, data: &[u8], params: &Dict) -> Result<DecodedImage> {
    // 1) Read the JPEG header to learn the *input* colorspace, so we can request
    //    a faithful output colorspace (keep native CMYK; let YCbCr→RGB happen).
    let mut probe = JpegDecoder::new(ZCursor::new(data));
    probe
        .decode_headers()
        .map_err(|_| Error::decode(CODEC, "invalid JPEG header"))?;
    let input_cs = probe
        .input_colorspace()
        .ok_or_else(|| Error::decode(CODEC, "unknown JPEG colorspace"))?;
    let (dims_w, dims_h) = probe
        .dimensions()
        .ok_or_else(|| Error::decode(CODEC, "unknown JPEG dimensions"))?;

    let out_cs = output_colorspace(input_cs);
    let width = u32::try_from(dims_w).map_err(|_| Error::decode(CODEC, "JPEG width overflow"))?;
    let height = u32::try_from(dims_h).map_err(|_| Error::decode(CODEC, "JPEG height overflow"))?;
    guard_dimensions(width, height, CODEC)?;

    // 2) Decode for real with the chosen output colorspace.
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(out_cs);
    let mut dec = JpegDecoder::new_with_options(ZCursor::new(data), opts);
    let mut pixels = dec
        .decode()
        .map_err(|_| Error::decode(CODEC, "JPEG decode failed"))?;

    let components = match out_cs.num_components() {
        1 => 1u8,
        3 => 3u8,
        4 => 4u8,
        _ => return Err(Error::decode(CODEC, "unsupported JPEG component count")),
    };
    let hint = match components {
        1 => ColorSpaceHint::Gray,
        3 => ColorSpaceHint::Rgb,
        4 => ColorSpaceHint::Cmyk,
        _ => ColorSpaceHint::Unknown,
    };

    // Validate the buffer length against geometry (defensive; zune is total).
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|p| p.checked_mul(components as usize))
        .ok_or(Error::LimitExceeded("JPEG raster size overflow"))?;
    if pixels.len() != expected {
        return Err(Error::decode(CODEC, "JPEG output size mismatch"));
    }

    // 3) CMYK Adobe-inversion / `/Decode` handling.
    if components == 4 {
        let adobe =
            matches!(input_cs, ColorSpace::CMYK | ColorSpace::YCCK) && jpeg_has_adobe_app14(data);
        apply_cmyk_decode(&mut pixels, components, decode_array(doc, params), adobe);
    } else if let Some(arr) = decode_array(doc, params) {
        // For gray/RGB, honor a non-default `/Decode` that inverts samples.
        apply_generic_decode(&mut pixels, components, &arr);
    }

    Ok(DecodedImage::new(
        width, height, components, 8, hint, pixels,
    ))
}

/// Chooses the `zune-jpeg` output colorspace given the JPEG's input colorspace:
/// keep CMYK native (and convert YCCK→CMYK), pass through gray, convert all
/// 3-component variants to RGB.
fn output_colorspace(input: ColorSpace) -> ColorSpace {
    match input.num_components() {
        1 => ColorSpace::Luma,
        4 => ColorSpace::CMYK, // YCCK→CMYK, CMYK passthrough
        _ => ColorSpace::RGB,  // YCbCr/RGB → RGB
    }
}

/// Returns `true` if the JPEG byte stream contains an Adobe `APP14` marker
/// (`FF EE` with an `"Adobe"` identifier). Used to detect the inverted-CMYK
/// convention when the PDF dict has no explicit `/Decode`.
fn jpeg_has_adobe_app14(data: &[u8]) -> bool {
    let mut i = 2usize; // skip SOI
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        // Standalone markers without length.
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        if marker == 0xDA {
            break; // start of scan: header markers are done
        }
        let len = ((data[i + 2] as usize) << 8) | data[i + 3] as usize;
        if len < 2 {
            break;
        }
        if marker == 0xEE {
            let seg_start = i + 4;
            let seg_end = (i + 2 + len).min(data.len());
            if seg_end >= seg_start + 5 && &data[seg_start..seg_start + 5] == b"Adobe" {
                return true;
            }
        }
        i += 2 + len;
    }
    false
}

/// Reads the `/Decode` array (per-component `[min max …]` pairs) if present.
fn decode_array(doc: &DocumentStore, params: &Dict) -> Option<Vec<f64>> {
    let obj = match params
        .get(&Name::new("Decode"))
        .or_else(|| params.get(&Name::new("D")))?
    {
        Object::Reference(r) => doc.resolve(*r).ok()?.as_ref().clone(),
        other => other.clone(),
    };
    let arr = obj.as_array()?;
    let vals: Vec<f64> = arr.iter().filter_map(Object::as_f64).collect();
    if vals.len() == arr.len() && !vals.is_empty() {
        Some(vals)
    } else {
        None
    }
}

/// Applies CMYK inversion semantics. `decode` (if present) is the `/Decode`
/// array; `adobe_no_decode_invert` requests an inversion when an Adobe APP14
/// marker was present but no `/Decode` array compensated for it.
fn apply_cmyk_decode(
    pixels: &mut [u8],
    components: u8,
    decode: Option<Vec<f64>>,
    adobe_no_decode_invert: bool,
) {
    if let Some(arr) = decode {
        apply_generic_decode(pixels, components, &arr);
    } else if adobe_no_decode_invert {
        for b in pixels.iter_mut() {
            *b = 255 - *b;
        }
    }
}

/// Applies a per-component `/Decode` array to 8-bit samples. A pair
/// `[1 0]` inverts that component (`out = 255 - in`); `[0 1]` is identity.
/// Other linear maps are applied as `out = dmin + in/255 * (dmax - dmin)`.
fn apply_generic_decode(pixels: &mut [u8], components: u8, decode: &[f64]) {
    let c = components as usize;
    if decode.len() < c * 2 {
        return;
    }
    // Fast path: every pair is identity → nothing to do.
    let mut all_identity = true;
    for k in 0..c {
        let dmin = decode[2 * k];
        let dmax = decode[2 * k + 1];
        if !((dmin - 0.0).abs() < 1e-9 && (dmax - 1.0).abs() < 1e-9) {
            all_identity = false;
            break;
        }
    }
    if all_identity {
        return;
    }
    for (i, b) in pixels.iter_mut().enumerate() {
        let k = i % c;
        let dmin = decode[2 * k];
        let dmax = decode[2 * k + 1];
        let v = f64::from(*b) / 255.0;
        let mapped = dmin + v * (dmax - dmin);
        *b = (mapped * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}
