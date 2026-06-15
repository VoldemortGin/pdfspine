//! `FlateDecode` — zlib/deflate codec (ISO 32000-1 §7.4.4; RFC 1950/1951).
//!
//! The most common PDF stream filter. The encoded bytes are a zlib stream
//! (RFC 1950: a 2-byte header, raw DEFLATE data, then an Adler-32 trailer),
//! and decoding produces the original bytes. This module wraps the pure-Rust
//! `miniz_oxide` backend of `flate2` and layers the oxide-pdf decode policies on
//! top (PRD §8.3, §9.6).
//!
//! # Policies
//!
//! - **Trailing garbage** (PRD §8.3): real-world producers pad streams with a
//!   newline or stray bytes after the zlib trailer. We decode in a streaming
//!   loop and stop the instant the decompressor reports end-of-stream, so any
//!   bytes following a valid stream are silently ignored rather than treated as
//!   corruption.
//! - **Missing zlib header / raw deflate fallback** (PRD §8.3): some producers
//!   emit a bare DEFLATE stream (RFC 1951) with no zlib (RFC 1950) wrapper, or
//!   a wrapper whose header bytes are invalid. We try the zlib-wrapped decode
//!   first; if it fails, we retry the same input as raw deflate. Only if *both*
//!   fail do we surface a [`Error::Decode`] (carrying the zlib error message).
//! - **Decompression-bomb guard** (PRD §9.6.2): a tiny input can expand to
//!   gigabytes. Decoding is bounded: we inflate into a fixed scratch buffer and
//!   refuse to grow the output past [`Limits::max_decompressed_stream`],
//!   returning [`Error::LimitExceeded`] with [`LimitKind::DecompressedStream`]
//!   *before* over-allocating. We never call an unbounded `read_to_end`.
//!
//! # Totality
//!
//! [`decode`] is total: empty input decodes to empty output, and arbitrary,
//! truncated or corrupt input yields a typed [`Error`] — never a panic. No
//! `unwrap`/`expect` is reachable on a fallible decode path.

use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};
use std::io::Write;

use crate::error::{Error, LimitKind, Result};
use crate::limits::Limits;

/// Canonical filter name, used in stable error messages.
const FILTER: &str = "FlateDecode";

/// Size of the scratch buffer the streaming inflate loop produces into (64 KiB).
const SCRATCH: usize = 64 * 1024;

/// Decodes a `FlateDecode` (zlib/deflate) stream into its original bytes.
///
/// Implements the trailing-garbage, raw-deflate-fallback and
/// decompression-bomb policies described in the module docs. Empty input is
/// **not** an error: `decode(&[], _)` is `Ok(vec![])`.
///
/// # Errors
///
/// - [`Error::Decode`] (`FILTER`) if the input is neither a valid zlib stream
///   nor valid raw deflate (truncated or corrupt).
/// - [`Error::LimitExceeded`] ([`LimitKind::DecompressedStream`]) if the output
///   would exceed `limits.max_decompressed_stream`.
pub fn decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>> {
    // An empty stream is legal and decodes to nothing (PRD §8.3): don't even
    // hand it to the decompressor, which would report a truncated header.
    if input.is_empty() {
        return Ok(Vec::new());
    }

    // Primary path: zlib-wrapped deflate (RFC 1950).
    match inflate(input, true, limits) {
        Ok(out) => Ok(out),
        // A limit trip is authoritative — never "retry" a bomb as raw deflate.
        Err(e @ Error::LimitExceeded(_)) => Err(e),
        // Bad/absent zlib header (or any decode failure): retry as raw deflate
        // (RFC 1951). If that also fails, surface the original zlib error.
        Err(zlib_err) => match inflate(input, false, limits) {
            Ok(out) => Ok(out),
            Err(e @ Error::LimitExceeded(_)) => Err(e),
            Err(_) => Err(zlib_err),
        },
    }
}

/// Encodes `input` as a zlib-wrapped deflate stream (RFC 1950) at the default
/// compression level — the inverse of [`decode`] for well-formed input.
///
/// Round-trip invariant: `decode(&encode(x), &Limits::unbounded_decode())`
/// equals `Ok(x)` for every `x`.
///
/// Infallible: writing into an in-memory `Vec<u8>` cannot fail, so no error
/// path is reachable.
#[must_use]
pub fn encode(input: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to / finishing into an in-memory `Vec<u8>` cannot fail (the
    // `io::Write` impl for `Vec` is infallible). On the unreachable error path
    // we degrade to the empty stream rather than panic, keeping this total.
    if enc.write_all(input).is_err() {
        return Vec::new();
    }
    enc.finish().unwrap_or_default()
}

/// Bounded, streaming inflate. Decompresses `input` (treating it as zlib-wrapped
/// when `zlib_header`, else as raw deflate per RFC 1951) into a fresh `Vec`,
/// stopping at end-of-stream (ignoring any trailing bytes) and refusing to grow
/// the output past `limits.max_decompressed_stream`.
fn inflate(input: &[u8], zlib_header: bool, limits: &Limits) -> Result<Vec<u8>> {
    let mut d = Decompress::new(zlib_header);
    let mut scratch = vec![0u8; SCRATCH];
    let mut out: Vec<u8> = Vec::new();
    let mut inpos = 0usize;

    loop {
        let prev_in = d.total_in();
        let prev_out = d.total_out();

        let status = d
            .decompress(&input[inpos..], &mut scratch, FlushDecompress::None)
            .map_err(|_| Error::decode(FILTER, "corrupt deflate stream"))?;

        // `total_in`/`total_out` are cumulative; the deltas are this call's work.
        let consumed = (d.total_in() - prev_in) as usize;
        let produced = (d.total_out() - prev_out) as usize;
        inpos += consumed;

        if produced > 0 {
            // Bomb guard: refuse to grow past the configured ceiling.
            if out.len().saturating_add(produced) > limits.max_decompressed_stream {
                return Err(Error::LimitExceeded(LimitKind::DecompressedStream));
            }
            out.extend_from_slice(&scratch[..produced]);
        }

        match status {
            // Clean end of the deflate/zlib stream: any remaining input is
            // trailing garbage and is intentionally ignored (PRD §8.3).
            Status::StreamEnd => return Ok(out),
            Status::Ok | Status::BufError => {
                // No forward progress and no more input ⇒ the stream ended
                // mid-symbol: truncated/corrupt.
                if consumed == 0 && produced == 0 {
                    return Err(Error::decode(FILTER, "truncated deflate stream"));
                }
                // Otherwise keep looping: more output to drain, or more input
                // to feed.
            }
        }
    }
}
