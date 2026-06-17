//! JBIG2Decode image-XObject decode — PRD §8.4.1 (documented subset).
//!
//! Decoder: `hayro-jbig2`, driven through the PDF **embedded** organization
//! (`Image::new_embedded(page, globals)`) so the `/DecodeParms /JBIG2Globals`
//! stream is honored. Output is **1 bpc, 1 component**, packed MSB-first,
//! byte-aligned rows. A JBIG2 foreground (set) pixel is **black**; the decoded
//! image is treated as standard 1-bpc DeviceGray (`0-bit = black`, `1-bit =
//! white`), so we emit `0` for foreground (black) and `1` for background (white).
//! This matches the shared upsample (`bit 0 → 0`, `bit 1 → 255`) and the
//! stencil-mask path (`bit 0 → paint`); emitting the opposite "set = 1" polarity
//! rendered JBIG2 images inverted.
//!
//! ## Documented subset (§8.4.1) & fail-closed
//!
//! Target coverage is **generic region + symbol dictionary + text region** —
//! the combinations behind the overwhelming majority of scanned PDFs. Halftone
//! and refinement regions are best-effort. Per the §8.4.1 degradation contract,
//! **any** unsupported segment / decode error / resource-cap hit returns a typed
//! error for *this image only* — it never panics and never aborts the document.
//! `hayro-jbig2` is `#![forbid(unsafe_code)]` and routes malformed input to
//! `Result`, but it is an untrusted leaf on adversarial data, so the whole
//! decode is additionally wrapped in `catch_unwind` (belt-and-suspenders).
//! A declared raster larger than the global pixel cap trips
//! [`Error::LimitExceeded`] before any allocation (no decompression bomb).

use crate::codecs::{guard_dimensions, ColorSpaceHint, DecodedImage};
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore, Name, Object};

use hayro_jbig2::{Decoder, Image};

const CODEC: &str = "JBIG2Decode";

/// Packs JBIG2 callbacks into a 1-bpp MSB-first byte-aligned raster in the
/// standard 1-bpc DeviceGray polarity: a foreground (set/black) pixel is a **0**
/// bit, a background (white) pixel is a **1** bit.
struct PackedBitmap {
    width: u32,
    out: Vec<u8>,
    cur_byte: u8,
    cur_bits: u8,
    col: u32,
}

impl PackedBitmap {
    fn new(width: u32, height: u32) -> Self {
        let row_bytes = (width as usize).div_ceil(8);
        PackedBitmap {
            width,
            out: Vec::with_capacity(row_bytes.saturating_mul(height as usize)),
            cur_byte: 0,
            cur_bits: 0,
            col: 0,
        }
    }

    #[inline]
    fn push_bit(&mut self, bit: bool) {
        if self.col >= self.width {
            return;
        }
        self.cur_byte = (self.cur_byte << 1) | u8::from(bit);
        self.cur_bits += 1;
        self.col += 1;
        if self.cur_bits == 8 {
            self.out.push(self.cur_byte);
            self.cur_byte = 0;
            self.cur_bits = 0;
        }
    }
}

impl Decoder for PackedBitmap {
    fn push_pixel(&mut self, black: bool) {
        // DeviceGray polarity: foreground (black) → 0 bit, background → 1 bit.
        self.push_bit(!black);
    }

    fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
        for _ in 0..(chunk_count as u64 * 8) {
            self.push_bit(!black);
        }
    }

    fn next_line(&mut self) {
        if self.cur_bits > 0 {
            self.cur_byte <<= 8 - self.cur_bits;
            self.out.push(self.cur_byte);
            self.cur_byte = 0;
            self.cur_bits = 0;
        }
        self.col = 0;
    }
}

/// Decodes a JBIG2Decode image stream to a 1-bpc [`DecodedImage`].
///
/// `data` is the JBIG2 embedded-stream payload (the page); `params` is the image
/// stream dict whose `/DecodeParms` may carry an indirect `/JBIG2Globals`
/// stream, resolved through `doc`.
pub fn decode(doc: &DocumentStore, data: &[u8], params: &Dict) -> Result<DecodedImage> {
    let globals = jbig2_globals(doc, params)?;
    let globals_slice = globals.as_deref();

    // Construct + decode, fully wrapped fail-closed.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode_inner(data, globals_slice)
    }));
    match result {
        Ok(inner) => inner,
        Err(_) => Err(Error::Unsupported(CODEC)),
    }
}

fn decode_inner(data: &[u8], globals: Option<&[u8]>) -> Result<DecodedImage> {
    let image = Image::new_embedded(data, globals).map_err(|_| Error::Unsupported(CODEC))?;
    let width = image.width();
    let height = image.height();
    guard_dimensions(width, height, CODEC)?;

    let mut bitmap = PackedBitmap::new(width, height);
    image
        .decode(&mut bitmap)
        .map_err(|_| Error::Unsupported(CODEC))?;

    // Validate / normalize the produced raster to exactly height rows.
    let row_bytes = (width as usize).div_ceil(8);
    let needed = row_bytes
        .checked_mul(height as usize)
        .ok_or(Error::LimitExceeded("JBIG2 raster size overflow"))?;
    if bitmap.out.len() < needed {
        return Err(Error::decode(
            CODEC,
            "JBIG2 produced fewer rows than declared",
        ));
    }
    bitmap.out.truncate(needed);

    Ok(DecodedImage::new(
        width,
        height,
        1,
        1,
        ColorSpaceHint::Gray,
        bitmap.out,
    ))
}

/// Resolves the optional `/DecodeParms /JBIG2Globals` stream into its raw
/// (filter-decoded by pdf-core) byte payload, if present.
fn jbig2_globals(doc: &DocumentStore, params: &Dict) -> Result<Option<Vec<u8>>> {
    let Some(parms) = decode_parms(doc, params) else {
        return Ok(None);
    };
    let Some(g) = parms.get(&Name::new("JBIG2Globals")) else {
        return Ok(None);
    };
    // /JBIG2Globals is a stream; resolve the ref then run pdf-core's decode so
    // any Flate/LZW wrapper on the globals stream is applied.
    let stream_obj = match g {
        Object::Reference(r) => doc.resolve(*r)?,
        // Inline stream object (rare but legal once resolved).
        other => std::sync::Arc::new(other.clone()),
    };
    let Some(stream) = stream_obj.as_stream() else {
        return Ok(None);
    };
    match doc.decode_stream(stream)? {
        pdf_core::DecodeOutcome::Decoded(bytes) => Ok(Some(bytes)),
        // A globals stream with a trailing image filter makes no sense; take the
        // raw bytes past the chain.
        pdf_core::DecodeOutcome::ImageEncoded { bytes, .. } => Ok(Some(bytes)),
    }
}

/// Resolves `/DecodeParms` (`/DP`) to a dict; tolerates the array form.
fn decode_parms(doc: &DocumentStore, params: &Dict) -> Option<Dict> {
    let obj = params
        .get(&Name::new("DecodeParms"))
        .or_else(|| params.get(&Name::new("DP")))?;
    let resolved = match obj {
        Object::Reference(r) => doc.resolve(*r).ok()?.as_ref().clone(),
        other => other.clone(),
    };
    match resolved {
        Object::Dictionary(d) => Some(d),
        Object::Array(arr) => arr.into_iter().find_map(|o| match o {
            Object::Dictionary(d) => Some(d),
            Object::Reference(r) => doc.resolve(r).ok().and_then(|a| a.as_dict().cloned()),
            _ => None,
        }),
        _ => None,
    }
}
