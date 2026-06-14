//! ASCIIHexDecode — ISO 32000-1 §7.4.2.
//!
//! Each byte is encoded as two hexadecimal digits (`0-9A-Fa-f`); whitespace
//! between digits is ignored; a `>` marks end-of-data (EOD). An odd number of
//! digits before the EOD is padded with a trailing `0` (the high nibble is the
//! lone digit). Bytes after `>` are ignored; a missing `>` at end-of-input is
//! tolerated (decode what was seen). Any other byte is a hard error.

use crate::error::{Error, Result};
use crate::limits::Limits;

const FILTER: &str = "ASCIIHexDecode";

/// Decodes ASCIIHex-encoded `input` (PRD §8.3). Stops at the first `>` (EOD) or
/// at end-of-input. Whitespace is skipped; an odd trailing digit is padded with
/// `0`. A non-hex, non-whitespace, non-`>` byte → typed `Err`.
pub fn decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut hi: Option<u8> = None;

    for &b in input {
        if b == b'>' {
            break; // EOD; ignore anything after.
        }
        if is_pdf_whitespace(b) {
            continue;
        }
        let nibble = hex_val(b).ok_or_else(|| Error::decode(FILTER, "invalid hex digit"))?;
        match hi.take() {
            None => hi = Some(nibble),
            Some(h) => {
                out.push((h << 4) | nibble);
                if out.len() > limits.max_decompressed_stream {
                    return Err(Error::LimitExceeded(
                        crate::error::LimitKind::DecompressedStream,
                    ));
                }
            }
        }
    }
    // Odd trailing digit: pad low nibble with 0.
    if let Some(h) = hi {
        out.push(h << 4);
    }
    Ok(out)
}

/// Encodes `input` as ASCIIHex with a trailing `>` EOD marker. Uses uppercase
/// digits. Infallible (round-trips [`decode`]).
#[must_use]
pub fn encode(input: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(input.len() * 2 + 1);
    for &b in input {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0x0f) as usize]);
    }
    out.push(b'>');
    out
}

/// The hex value of an ASCII digit byte, or `None` if not a hex digit.
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The PDF white-space bytes (ISO 32000-1 §7.2.2): NUL, TAB, LF, FF, CR, SP.
fn is_pdf_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}
