//! RunLengthDecode — ISO 32000-1 §7.4.5.
//!
//! A simple byte-oriented RLE. The stream is a sequence of runs, each led by a
//! length byte `n`:
//!
//! - `0..=127` — the next `n + 1` bytes are copied literally.
//! - `129..=255` — the next single byte is repeated `257 - n` times.
//! - `128` — end-of-data (EOD); decoding stops.
//!
//! A length byte not followed by its data (truncation) is a hard error.

use crate::error::{Error, Result};
use crate::limits::Limits;

const FILTER: &str = "RunLengthDecode";

/// Decodes RunLength-encoded `input` (PRD §8.3). Stops at a `128` EOD byte or at
/// end-of-input. A truncated run (length byte with missing data) → typed `Err`.
pub fn decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < input.len() {
        let len = input[i];
        i += 1;
        match len {
            128 => break, // EOD
            0..=127 => {
                let count = len as usize + 1;
                let end = i
                    .checked_add(count)
                    .filter(|&e| e <= input.len())
                    .ok_or_else(|| Error::decode(FILTER, "truncated literal run"))?;
                out.extend_from_slice(&input[i..end]);
                i = end;
            }
            129..=255 => {
                let count = 257 - len as usize;
                let byte = *input
                    .get(i)
                    .ok_or_else(|| Error::decode(FILTER, "truncated replicate run"))?;
                i += 1;
                out.resize(out.len() + count, byte);
            }
        }
        if out.len() > limits.max_decompressed_stream {
            return Err(Error::LimitExceeded(
                crate::error::LimitKind::DecompressedStream,
            ));
        }
    }
    Ok(out)
}

/// Encodes `input` with RunLength, appending a `128` EOD byte. Emits replicate
/// runs for stretches of 3+ identical bytes and literal runs otherwise.
/// Infallible (round-trips [`decode`]).
#[must_use]
pub fn encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = input.len();

    while i < n {
        // Measure a run of identical bytes starting at i (cap at 128).
        let b = input[i];
        let mut run = 1usize;
        while i + run < n && input[i + run] == b && run < 128 {
            run += 1;
        }
        if run >= 3 {
            // Replicate run: length byte = 257 - count.
            out.push((257 - run) as u8);
            out.push(b);
            i += run;
        } else {
            // Literal run: gather bytes until a 3+ repeat appears (cap 128).
            let start = i;
            let mut lit = 0usize;
            while i < n && lit < 128 {
                // Stop the literal if a 3-run begins here.
                let c = input[i];
                let mut ahead = 1usize;
                while i + ahead < n && input[i + ahead] == c && ahead < 3 {
                    ahead += 1;
                }
                if ahead >= 3 {
                    break;
                }
                i += 1;
                lit += 1;
            }
            out.push((lit - 1) as u8);
            out.extend_from_slice(&input[start..start + lit]);
        }
    }
    out.push(128); // EOD
    out
}
