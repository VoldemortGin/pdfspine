//! ASCII85Decode — ISO 32000-1 §7.4.3.
//!
//! Four binary bytes are encoded as five ASCII characters in the range `!`..`u`
//! (each a base-85 digit, value `c - '!'`). Special cases:
//!
//! - `z` stands for four zero bytes — but only at a 5-tuple boundary.
//! - `~>` marks end-of-data (EOD).
//! - White space between characters is ignored.
//! - An optional `<~` lead-in (used by some encoders) is tolerated.
//! - The final group may be partial: 2..=5 characters encode 1..=4 bytes; a
//!   1-character final group is malformed.

use crate::error::{Error, Result};
use crate::limits::Limits;

const FILTER: &str = "ASCII85Decode";

/// Decodes ASCII85-encoded `input` (PRD §8.3). Handles the `z` shortcut, the
/// `~>` terminator, whitespace, an optional `<~` lead-in and a partial final
/// group. Out-of-range characters, a `z` mid-group, or a 1-character final
/// group → typed `Err`.
pub fn decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut n = 0usize; // base-85 digits accumulated in the current group

    // Tolerate an optional leading "<~".
    let mut bytes = input;
    if let [b'<', b'~', rest @ ..] = bytes {
        bytes = rest;
    }

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;

        if b == b'~' {
            // EOD: "~>" (the '>' is conventional; accept '~' alone too).
            break;
        }
        if is_pdf_whitespace(b) {
            continue;
        }
        if b == b'z' {
            if n != 0 {
                return Err(Error::decode(FILTER, "'z' shortcut inside a group"));
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            check_limit(&out, limits)?;
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            return Err(Error::decode(FILTER, "character out of ASCII85 range"));
        }
        group[n] = b - b'!';
        n += 1;
        if n == 5 {
            decode_group(&group, 5, &mut out);
            check_limit(&out, limits)?;
            n = 0;
        }
    }

    // Flush a partial final group (2..=5 chars → 1..=4 bytes).
    if n == 1 {
        return Err(Error::decode(FILTER, "final group has a single character"));
    }
    if n > 0 {
        // Pad the missing low digits with 'u' (84), the max digit, per spec.
        for slot in group.iter_mut().take(5).skip(n) {
            *slot = 84;
        }
        decode_group(&group, n, &mut out);
        check_limit(&out, limits)?;
    }
    Ok(out)
}

/// Encodes `input` as ASCII85 with a trailing `~>` EOD marker. Uses the `z`
/// shortcut for all-zero groups. Infallible (round-trips [`decode`]).
#[must_use]
pub fn encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in input.chunks(4) {
        if chunk.len() == 4 && chunk == [0, 0, 0, 0] {
            out.push(b'z');
            continue;
        }
        // Pack big-endian, zero-padding a short final chunk.
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        let mut v = u32::from_be_bytes(word);
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = (v % 85) as u8 + b'!';
            v /= 85;
        }
        // A 4-byte chunk emits 5 chars; an n-byte chunk emits n+1 chars.
        let emit = chunk.len() + 1;
        out.extend_from_slice(&digits[..emit]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// Decodes a (possibly partial) base-85 group of `count` significant input
/// characters into `count - 1` bytes, appending to `out`.
fn decode_group(group: &[u8; 5], count: usize, out: &mut Vec<u8>) {
    let mut v: u32 = 0;
    for &d in group.iter() {
        v = v.wrapping_mul(85).wrapping_add(d as u32);
    }
    let bytes = v.to_be_bytes();
    // A full group (count 5) → 4 bytes; a partial group of `count` → count-1.
    out.extend_from_slice(&bytes[..count - 1]);
}

/// Errors if the accumulated output exceeds the configured ceiling.
fn check_limit(out: &[u8], limits: &Limits) -> Result<()> {
    if out.len() > limits.max_decompressed_stream {
        return Err(Error::LimitExceeded(
            crate::error::LimitKind::DecompressedStream,
        ));
    }
    Ok(())
}

/// The PDF white-space bytes (ISO 32000-1 §7.2.2).
fn is_pdf_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}
