//! PNG and TIFF predictors — ISO 32000-1 §7.4.4.4, TIFF 6.0 §14, PNG spec §6.
//!
//! Predictors are a pre-processing transform applied **before** Flate/LZW
//! compression to make pixel/sample data more compressible. On decode they are
//! reversed ([`unpredict`]); on encode they are applied ([`predict`]). PDF
//! selects one via the `/Predictor` key in a Flate/LZW `/DecodeParms`:
//!
//! - `1` — no prediction (identity).
//! - `2` — TIFF predictor 2 (horizontal differencing on samples).
//! - `10..=15` — PNG predictors. `10` None, `11` Sub, `12` Up, `13` Average,
//!   `14` Paeth, `15` "optimum" (per-row tag byte chooses the filter). On
//!   decode all PNG values behave identically: each row is prefixed by a tag
//!   byte naming the filter actually used for that row.
//!
//! The row layout is governed by `/Colors` (samples per pixel, default 1),
//! `/BitsPerComponent` (default 8) and `/Columns` (samples per row, default 1).
//! The byte stride per row is `ceil(Colors * BitsPerComponent * Columns / 8)`;
//! the *pixel* stride (bytes per pixel, `bpp`) used by Sub/Average/Paeth is
//! `ceil(Colors * BitsPerComponent / 8)` (minimum 1) — sub-byte components pack
//! into bytes and the differencing then operates byte-wise per the PNG spec.

use crate::error::{Error, Result};
use crate::limits::Limits;
use crate::object::{Dict, Name, Object};

/// Resolved predictor configuration (PRD §8.3 / ISO 32000-1 §7.4.4.4).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct PredictorParams {
    /// `/Predictor` value (1, 2, or 10..=15).
    pub predictor: i64,
    /// `/Colors` — samples per pixel (default 1).
    pub colors: usize,
    /// `/BitsPerComponent` — bits per sample (default 8).
    pub bits_per_component: usize,
    /// `/Columns` — samples per row (default 1).
    pub columns: usize,
}

impl PredictorParams {
    /// Extracts predictor configuration from a filter's `/DecodeParms` dict.
    /// Returns `Ok(None)` when there is no parms dict or `/Predictor` is absent
    /// or `1` (no prediction). `filter` names the caller for error context.
    pub fn from_parms(parms: Option<&Dict>, filter: &'static str) -> Result<Option<Self>> {
        let Some(d) = parms else { return Ok(None) };
        let predictor = d
            .get(&Name::new("Predictor"))
            .and_then(Object::as_i64)
            .unwrap_or(1);
        if predictor <= 1 {
            return Ok(None);
        }
        let params = PredictorParams {
            predictor,
            colors: read_pos(d, "Colors", 1, filter)?,
            bits_per_component: read_pos(d, "BitsPerComponent", 8, filter)?,
            columns: read_pos(d, "Columns", 1, filter)?,
        };
        params.validate(filter)?;
        Ok(Some(params))
    }

    /// Validates that `predictor` is a value we implement and that the layout is
    /// representable.
    fn validate(&self, filter: &'static str) -> Result<()> {
        match self.predictor {
            2 | 10..=15 => {}
            _ => return Err(Error::filter(filter, "unsupported /Predictor value")),
        }
        if !matches!(self.bits_per_component, 1 | 2 | 4 | 8 | 16) {
            return Err(Error::filter(filter, "invalid /BitsPerComponent"));
        }
        if self.colors == 0 || self.columns == 0 {
            return Err(Error::filter(filter, "/Colors and /Columns must be >= 1"));
        }
        Ok(())
    }

    /// Bytes per row of samples (the row stride), excluding any PNG tag byte:
    /// `ceil(Colors * BitsPerComponent * Columns / 8)`.
    fn row_bytes(&self) -> usize {
        let bits = self
            .colors
            .saturating_mul(self.bits_per_component)
            .saturating_mul(self.columns);
        bits.div_ceil(8)
    }

    /// Bytes per pixel for byte-wise PNG differencing:
    /// `max(1, ceil(Colors * BitsPerComponent / 8))`.
    fn bpp(&self) -> usize {
        self.colors
            .saturating_mul(self.bits_per_component)
            .div_ceil(8)
            .max(1)
    }
}

/// Reads a positive integer parameter, defaulting when absent.
fn read_pos(d: &Dict, key: &str, default: usize, filter: &'static str) -> Result<usize> {
    match d.get(&Name::new(key)).and_then(Object::as_i64) {
        None => Ok(default),
        Some(v) if v >= 0 => Ok(v as usize),
        Some(_) => Err(Error::filter(
            filter,
            "predictor parameter must be non-negative",
        )),
    }
}

/// Reverses a predictor on freshly-decoded filter output (decode path).
///
/// `data` is the predictor-encoded byte stream; the returned bytes are the raw
/// samples. Respects `limits.max_decompressed_stream` (the output is the same
/// size as the input minus tag bytes, so this is a cheap guard).
pub fn unpredict(data: &[u8], p: &PredictorParams, limits: &Limits) -> Result<Vec<u8>> {
    if p.predictor == 2 {
        unpredict_tiff(data, p, limits)
    } else {
        unpredict_png(data, p, limits)
    }
}

/// Applies a predictor to raw samples (encode path), producing the
/// predictor-encoded byte stream that a later compression step consumes.
pub fn predict(data: &[u8], p: &PredictorParams) -> Result<Vec<u8>> {
    if p.predictor == 2 {
        predict_tiff(data, p)
    } else {
        predict_png(data, p)
    }
}

// --- PNG predictors (10..=15) --------------------------------------------------

const PNG_NONE: u8 = 0;
const PNG_SUB: u8 = 1;
const PNG_UP: u8 = 2;
const PNG_AVG: u8 = 3;
const PNG_PAETH: u8 = 4;

fn unpredict_png(data: &[u8], p: &PredictorParams, limits: &Limits) -> Result<Vec<u8>> {
    let row_bytes = p.row_bytes();
    if row_bytes == 0 {
        return Ok(Vec::new());
    }
    let stride = row_bytes + 1; // +1 tag byte per row
    if !data.len().is_multiple_of(stride) {
        return Err(Error::filter(
            "predictor",
            "PNG data length not a multiple of row stride",
        ));
    }
    let n_rows = data.len() / stride;
    let out_len = n_rows * row_bytes;
    if out_len > limits.max_decompressed_stream {
        return Err(Error::LimitExceeded(
            crate::error::LimitKind::DecompressedStream,
        ));
    }

    let bpp = p.bpp();
    let mut out = vec![0u8; out_len];
    // `prev` row of *reconstructed* bytes; starts all-zero (PNG spec).
    for r in 0..n_rows {
        let tag = data[r * stride];
        let src = &data[r * stride + 1..r * stride + 1 + row_bytes];
        let (before, cur) = out.split_at_mut(r * row_bytes);
        let cur = &mut cur[..row_bytes];
        let prev: &[u8] = if r == 0 {
            &[]
        } else {
            &before[(r - 1) * row_bytes..]
        };
        reconstruct_row(tag, src, cur, prev, bpp)?;
    }
    Ok(out)
}

/// Reconstructs one PNG row in place from filtered `src` into `cur`.
fn reconstruct_row(tag: u8, src: &[u8], cur: &mut [u8], prev: &[u8], bpp: usize) -> Result<()> {
    let up = |i: usize| -> u8 { prev.get(i).copied().unwrap_or(0) };
    match tag {
        PNG_NONE => cur.copy_from_slice(src),
        PNG_SUB => {
            for i in 0..src.len() {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                cur[i] = src[i].wrapping_add(a);
            }
        }
        PNG_UP => {
            for i in 0..src.len() {
                cur[i] = src[i].wrapping_add(up(i));
            }
        }
        PNG_AVG => {
            for i in 0..src.len() {
                let a = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                let b = up(i) as u16;
                cur[i] = src[i].wrapping_add(((a + b) / 2) as u8);
            }
        }
        PNG_PAETH => {
            for i in 0..src.len() {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let b = up(i);
                let c = if i >= bpp { up(i - bpp) } else { 0 };
                cur[i] = src[i].wrapping_add(paeth(a, b, c));
            }
        }
        _ => return Err(Error::filter("predictor", "invalid PNG row filter tag")),
    }
    Ok(())
}

fn predict_png(data: &[u8], p: &PredictorParams) -> Result<Vec<u8>> {
    let row_bytes = p.row_bytes();
    if row_bytes == 0 {
        return Ok(Vec::new());
    }
    if !data.len().is_multiple_of(row_bytes) {
        return Err(Error::filter(
            "predictor",
            "data length not a multiple of row width",
        ));
    }
    let n_rows = data.len() / row_bytes;
    let bpp = p.bpp();
    // Map /Predictor 10..=15 to a fixed PNG filter for every row. 15 ("optimum")
    // is also encoded with a single chosen filter per row; we use the requested
    // filter uniformly (decode handles any per-row tag regardless).
    let tag = match p.predictor {
        10 => PNG_NONE,
        11 => PNG_SUB,
        12 => PNG_UP,
        13 => PNG_AVG,
        14 | 15 => PNG_PAETH,
        _ => return Err(Error::filter("predictor", "unsupported PNG predictor")),
    };

    let mut out = Vec::with_capacity(n_rows * (row_bytes + 1));
    for r in 0..n_rows {
        let cur = &data[r * row_bytes..(r + 1) * row_bytes];
        let prev: &[u8] = if r == 0 {
            &[]
        } else {
            &data[(r - 1) * row_bytes..r * row_bytes]
        };
        out.push(tag);
        filter_row(tag, cur, prev, bpp, &mut out);
    }
    Ok(out)
}

/// Filters one PNG row, appending the filtered bytes to `out`.
fn filter_row(tag: u8, cur: &[u8], prev: &[u8], bpp: usize, out: &mut Vec<u8>) {
    let up = |i: usize| -> u8 { prev.get(i).copied().unwrap_or(0) };
    match tag {
        PNG_NONE => out.extend_from_slice(cur),
        PNG_SUB => {
            for i in 0..cur.len() {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                out.push(cur[i].wrapping_sub(a));
            }
        }
        PNG_UP => {
            for (i, &c) in cur.iter().enumerate() {
                out.push(c.wrapping_sub(up(i)));
            }
        }
        PNG_AVG => {
            for i in 0..cur.len() {
                let a = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                let b = up(i) as u16;
                out.push(cur[i].wrapping_sub(((a + b) / 2) as u8));
            }
        }
        PNG_PAETH => {
            for i in 0..cur.len() {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let b = up(i);
                let c = if i >= bpp { up(i - bpp) } else { 0 };
                out.push(cur[i].wrapping_sub(paeth(a, b, c)));
            }
        }
        _ => unreachable!("filter_row only called with a known tag"),
    }
}

/// The PNG Paeth predictor function (PNG spec §6.6). Ties resolve to `a`, then
/// `b`, then `c` (the spec's documented order).
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let pa = (b as i16 - c as i16).abs();
    let pb = (a as i16 - c as i16).abs();
    let pc = (a as i16 + b as i16 - 2 * c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

// --- TIFF predictor 2 ----------------------------------------------------------

fn unpredict_tiff(data: &[u8], p: &PredictorParams, limits: &Limits) -> Result<Vec<u8>> {
    if data.len() > limits.max_decompressed_stream {
        return Err(Error::LimitExceeded(
            crate::error::LimitKind::DecompressedStream,
        ));
    }
    let mut out = data.to_vec();
    tiff_each_row(&mut out, p, /*encode=*/ false)?;
    Ok(out)
}

fn predict_tiff(data: &[u8], p: &PredictorParams) -> Result<Vec<u8>> {
    let mut out = data.to_vec();
    tiff_each_row(&mut out, p, /*encode=*/ true)?;
    Ok(out)
}

/// Applies (encode) or reverses (decode) TIFF predictor 2 in place, row by row.
fn tiff_each_row(buf: &mut [u8], p: &PredictorParams, encode: bool) -> Result<()> {
    let row_bytes = p.row_bytes();
    if row_bytes == 0 {
        return Ok(());
    }
    if !buf.len().is_multiple_of(row_bytes) {
        return Err(Error::filter(
            "predictor",
            "TIFF data length not a multiple of row width",
        ));
    }
    let colors = p.colors;
    let bpc = p.bits_per_component;

    for row in buf.chunks_mut(row_bytes) {
        match bpc {
            8 => tiff_row_8(row, colors, encode),
            16 => tiff_row_16(row, colors, encode),
            1 | 2 | 4 => tiff_row_sub_byte(row, colors, bpc, p.columns, encode),
            _ => {
                return Err(Error::filter(
                    "predictor",
                    "TIFF2 BitsPerComponent unsupported",
                ))
            }
        }
    }
    Ok(())
}

/// 8-bit TIFF2: each sample = prior sample in the same color channel.
fn tiff_row_8(row: &mut [u8], colors: usize, encode: bool) {
    if encode {
        // Decode order matters: walk right-to-left so we read originals.
        for i in (colors..row.len()).rev() {
            row[i] = row[i].wrapping_sub(row[i - colors]);
        }
    } else {
        for i in colors..row.len() {
            row[i] = row[i].wrapping_add(row[i - colors]);
        }
    }
}

/// 16-bit TIFF2: samples are big-endian u16 per PDF/TIFF.
fn tiff_row_16(row: &mut [u8], colors: usize, encode: bool) {
    let n = row.len() / 2;
    let get = |row: &[u8], i: usize| -> u16 { u16::from_be_bytes([row[2 * i], row[2 * i + 1]]) };
    if encode {
        for i in (colors..n).rev() {
            let v = get(row, i).wrapping_sub(get(row, i - colors));
            row[2 * i..2 * i + 2].copy_from_slice(&v.to_be_bytes());
        }
    } else {
        for i in colors..n {
            let v = get(row, i).wrapping_add(get(row, i - colors));
            row[2 * i..2 * i + 2].copy_from_slice(&v.to_be_bytes());
        }
    }
}

/// Sub-byte TIFF2 (1/2/4 bpc): unpack samples, difference per color channel
/// modulo `2^bpc`, repack. `columns` bounds the real samples (the final byte
/// may be padded).
fn tiff_row_sub_byte(row: &mut [u8], colors: usize, bpc: usize, columns: usize, encode: bool) {
    let n_samples = colors * columns;
    let mask = (1u16 << bpc) - 1;
    // Unpack big-endian bit order into per-sample values.
    let mut samples = vec![0u16; n_samples];
    let mut bitpos = 0usize;
    for s in samples.iter_mut() {
        let mut v = 0u16;
        for _ in 0..bpc {
            let byte = row[bitpos / 8];
            let bit = (byte >> (7 - (bitpos % 8))) & 1;
            v = (v << 1) | bit as u16;
            bitpos += 1;
        }
        *s = v;
    }
    // Difference / integrate per color channel.
    if encode {
        for i in (colors..n_samples).rev() {
            samples[i] = samples[i].wrapping_sub(samples[i - colors]) & mask;
        }
    } else {
        for i in colors..n_samples {
            samples[i] = samples[i].wrapping_add(samples[i - colors]) & mask;
        }
    }
    // Repack.
    for b in row.iter_mut() {
        *b = 0;
    }
    let mut bitpos = 0usize;
    for &s in &samples {
        for k in (0..bpc).rev() {
            let bit = ((s >> k) & 1) as u8;
            row[bitpos / 8] |= bit << (7 - (bitpos % 8));
            bitpos += 1;
        }
    }
}
