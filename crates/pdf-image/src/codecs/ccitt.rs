//! CCITTFaxDecode (Group 3 / Group 4 fax) image-XObject decode — PRD §8.4.
//!
//! Honors `/K /Columns(1728) /Rows /BlackIs1 /EncodedByteAlign` from
//! `/DecodeParms`. The decoder is `hayro-ccitt`, which uniformly covers all
//! three CCITT modes that `/K` selects:
//!
//! - `K  < 0` ⇒ Group 4 (pure 2-D, T.6 / MMR),
//! - `K == 0` ⇒ Group 3 1-D (T.4 / MH),
//! - `K  > 0` ⇒ Group 3 2-D (T.4 / MR).
//!
//! (`fax 0.2` is the alternate codec and the test-suite encoder, but it lacks
//! `EncodedByteAlign` and `K>0`, so the production decode path uses
//! `hayro-ccitt`.)
//!
//! Output is **1 bpc, 1 component**: a packed bitmap, 1 bit/pixel, MSB-first,
//! each row padded to a byte boundary (no extra row padding beyond that — the
//! PDF/`DecodedImage` convention). Bits follow the **standard 1-bpc DeviceGray**
//! convention — a **0-bit is black**, a **1-bit is white** (`/BlackIs1` taken
//! into account) — so the shared upsample (`bit 0 → 0`, `bit 1 → 255`) and the
//! stencil-mask path (`bit 0 → paint`) both render correctly without a per-codec
//! inversion. (Emitting the opposite, fax-native "ink = 1" polarity rendered
//! every CCITT image inverted — over-dark scans.)

use crate::codecs::{guard_dimensions, param_bool, param_i64, ColorSpaceHint, DecodedImage};
use crate::error::{Error, Result};

use pdf_core::{Dict, DocumentStore, Name, Object};

use hayro_ccitt::{decode as ccitt_decode, DecodeSettings, Decoder, DecoderContext, EncodingMode};

const CODEC: &str = "CCITTFaxDecode";

/// Accumulates decoded fax pixels into a packed 1-bpp, MSB-first, byte-aligned
/// raster in the standard 1-bpc DeviceGray polarity: a white pixel is a **1**
/// bit, a black pixel is a **0** bit. Each `next_line` pads the current row to a
/// byte boundary (padding bits are 0 and lie past the declared width, so they are
/// never sampled).
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
        // Stop accepting pixels past the declared width (defensive against a
        // decoder that over-runs a row).
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

    fn flush_row(&mut self) {
        if self.cur_bits > 0 {
            self.cur_byte <<= 8 - self.cur_bits;
            self.out.push(self.cur_byte);
            self.cur_byte = 0;
            self.cur_bits = 0;
        }
        self.col = 0;
    }
}

impl Decoder for PackedBitmap {
    fn push_pixel(&mut self, white: bool) {
        // DeviceGray polarity: white pixel → 1 bit, black pixel → 0 bit
        // (`white` already folds in `/BlackIs1` via the decoder's `invert_black`).
        self.push_bit(white);
    }

    fn push_pixel_chunk(&mut self, white: bool, chunk_count: u32) {
        for _ in 0..(chunk_count as u64 * 8) {
            self.push_bit(white);
        }
    }

    fn next_line(&mut self) {
        self.flush_row();
    }
}

/// Decodes a CCITTFaxDecode image stream to a 1-bpc [`DecodedImage`].
///
/// `data` is the encoded fax byte stream; `params` is the image stream dict
/// (its `/DecodeParms` carries `/K /Columns /Rows /BlackIs1 /EncodedByteAlign`);
/// `doc` resolves indirect references in `params`. Any malformed stream yields a
/// typed [`Error::Decode`] — never a panic.
pub fn decode(doc: &DocumentStore, data: &[u8], params: &Dict) -> Result<DecodedImage> {
    let parms = decode_parms(doc, params);

    // /Columns default 1728; /Rows / /Height optional (0 ⇒ unknown).
    let columns = parms_i64(doc, &parms, params, "Columns").unwrap_or(1728);
    let columns = u32::try_from(columns)
        .ok()
        .filter(|&c| c > 0)
        .ok_or_else(|| Error::decode(CODEC, "invalid /Columns"))?;

    let rows = parms_i64(doc, &parms, params, "Rows")
        .or_else(|| param_i64(doc, params, "Height"))
        .unwrap_or(0);
    let rows = u32::try_from(rows.max(0)).unwrap_or(0);

    let k = parms_i64(doc, &parms, params, "K").unwrap_or(0);
    let black_is_1 = parms_bool(doc, &parms, params, "BlackIs1", false);
    let byte_align = parms_bool(doc, &parms, params, "EncodedByteAlign", false);
    let end_of_block = parms_bool(doc, &parms, params, "EndOfBlock", true);
    let end_of_line = parms_bool(doc, &parms, params, "EndOfLine", false);

    let encoding = match k {
        k if k < 0 => EncodingMode::Group4,
        0 => EncodingMode::Group3_1D,
        k => EncodingMode::Group3_2D { k: k as u32 },
    };

    // When /Rows is unknown we still need a bound for the cap. Use the rows we
    // have if positive; otherwise cap on columns alone, then validate the
    // produced height afterwards.
    let cap_rows = if rows > 0 { rows } else { 1 };
    guard_dimensions(columns, cap_rows, CODEC)?;

    let settings = DecodeSettings {
        columns,
        rows,
        end_of_block,
        end_of_line,
        rows_are_byte_aligned: byte_align,
        encoding,
        invert_black: black_is_1,
    };

    let mut bitmap = PackedBitmap::new(columns, rows.max(1));
    let mut ctx = DecoderContext::new(settings);

    // hayro-ccitt is forbid(unsafe) and Result-returning, but it is an untrusted
    // leaf decoding adversarial input — wrap it fail-closed.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ccitt_decode(data, &mut bitmap, &mut ctx)
    }));
    match res {
        Ok(Ok(_)) => {}
        Ok(Err(_)) => return Err(Error::decode(CODEC, "CCITT decode failed")),
        Err(_) => return Err(Error::decode(CODEC, "CCITT decoder panicked")),
    }

    // Determine the produced height from the packed buffer (rows may be implicit
    // when /Rows was absent). Re-validate the final geometry against the cap.
    let row_bytes = (columns as usize).div_ceil(8);
    if row_bytes == 0 {
        return Err(Error::decode(CODEC, "invalid row width"));
    }
    let produced_rows = bitmap.out.len() / row_bytes;
    let height = if rows > 0 {
        rows
    } else {
        u32::try_from(produced_rows)
            .ok()
            .filter(|&h| h > 0)
            .ok_or_else(|| Error::decode(CODEC, "no rows decoded"))?
    };
    guard_dimensions(columns, height, CODEC)?;

    // Trim/validate to exactly height rows.
    let needed = row_bytes
        .checked_mul(height as usize)
        .ok_or(Error::LimitExceeded("CCITT raster size overflow"))?;
    if bitmap.out.len() < needed {
        return Err(Error::decode(CODEC, "fewer rows than declared"));
    }
    bitmap.out.truncate(needed);

    Ok(DecodedImage::new(
        columns,
        height,
        1,
        1,
        ColorSpaceHint::Gray,
        bitmap.out,
    ))
}

/// Resolves the `/DecodeParms` (a.k.a. `/DP`) dict if present. CCITT params may
/// be inline on the image dict's `/DecodeParms`, or absent (defaults apply).
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
        // /DecodeParms can be an array (one per filter); take the first dict.
        Object::Array(arr) => arr.into_iter().find_map(|o| match o {
            Object::Dictionary(d) => Some(d),
            Object::Reference(r) => doc.resolve(r).ok().and_then(|a| a.as_dict().cloned()),
            _ => None,
        }),
        _ => None,
    }
}

/// Reads an integer param from `/DecodeParms` first, falling back to the image
/// dict itself (some producers hoist CCITT params onto the image dict).
fn parms_i64(doc: &DocumentStore, parms: &Option<Dict>, params: &Dict, key: &str) -> Option<i64> {
    parms
        .as_ref()
        .and_then(|d| param_i64(doc, d, key))
        .or_else(|| param_i64(doc, params, key))
}

fn parms_bool(
    doc: &DocumentStore,
    parms: &Option<Dict>,
    params: &Dict,
    key: &str,
    default: bool,
) -> bool {
    if let Some(d) = parms.as_ref() {
        if d.contains_key(&Name::new(key)) {
            return param_bool(doc, d, key, default);
        }
    }
    param_bool(doc, params, key, default)
}
