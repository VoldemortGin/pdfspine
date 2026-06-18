//! `LZWDecode` — Lempel–Ziv–Welch codec (ISO 32000-1 §7.4.4.2; PRD §8.3).
//!
//! A PDF LZW stream packs variable-width codes **MSB-first** (most-significant
//! bit first), starting at 9 bits and growing to a maximum of 12 bits as the
//! code table fills, with reserved codes 256 (`ClearTable`) and 257
//! (`EndOfData`). This wraps the pure-Rust `weezl` backend
//! (`BitOrder::Msb`, code size 8) and layers the pdfspine decode policies on top.
//!
//! # EarlyChange — the #1 LZW bug
//!
//! The single most common LZW interoperability defect is mishandling the
//! `/EarlyChange` parameter. When `/EarlyChange` is `1` (**the PDF default**)
//! the encoder/decoder increases the code width *one code earlier* than the
//! table-size boundary would otherwise dictate; when it is `0` the width
//! increases exactly at the boundary. Getting this wrong corrupts every byte
//! past the first width transition, so the boolean is threaded through both
//! [`decode`] and [`encode`] and must match the producing side.
//!
//! `weezl` exposes the two variants as distinct constructors:
//!
//! - `EarlyChange == 1` (default) → `Decoder::with_tiff_size_switch(Msb, 8)` /
//!   `Encoder::with_tiff_size_switch(Msb, 8)` (the TIFF/PDF "size switch").
//! - `EarlyChange == 0` → `Decoder::new(Msb, 8)` / `Encoder::new(Msb, 8)`.
//!
//! # Policies
//!
//! - **Decompression-bomb guard** (PRD §9.6.2): a tiny LZW stream can expand to
//!   gigabytes. Decoding streams through a [`CapWriter`] that refuses to grow
//!   the output past [`Limits::max_decompressed_stream`], returning
//!   [`Error::LimitExceeded`] with [`LimitKind::DecompressedStream`] rather than
//!   over-allocating. We never call `weezl`'s unbounded one-shot `decode`.
//! - **Truncated / corrupt codes**: a stream that ends without an `EndOfData`
//!   marker, or that contains an out-of-range code, yields [`Error::Decode`]
//!   (`LZWDecode`) — never a panic.
//!
//! # Totality
//!
//! [`decode`] is total: empty input decodes to empty output, and arbitrary,
//! truncated or corrupt input yields a typed [`Error`] — never a panic. No
//! `unwrap`/`expect` is reachable on a fallible decode path.

use std::io::{self, Write};

use weezl::decode::Decoder;
use weezl::encode::Encoder;
use weezl::BitOrder;

use crate::error::{Error, LimitKind, Result};
use crate::limits::Limits;

/// Canonical filter name, used in stable error messages.
const FILTER: &str = "LZWDecode";

/// The LZW base code size for the PDF/TIFF variant: 8-bit literals, so codes
/// start at 9 bits and grow to 12.
const CODE_SIZE: u8 = 8;

/// An [`io::Write`] sink that appends into a `Vec` but refuses to grow past a
/// fixed cap, recording the overflow so the caller can distinguish a bomb-guard
/// trip from a genuine decode error.
///
/// On overflow it returns an `io::Error` (which `weezl`'s streaming loop
/// surfaces through its `status`); the [`CapWriter::overflow`] flag is the
/// authoritative signal, since the *kind* of the surfaced error is not
/// load-bearing.
struct CapWriter {
    /// Accumulated decoded bytes.
    out: Vec<u8>,
    /// Maximum permitted length of `out`.
    cap: usize,
    /// Set once a write would have exceeded `cap`.
    overflow: bool,
}

impl Write for CapWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // saturating_add so a pathological `cap`/`len` near `usize::MAX` cannot
        // wrap and spuriously admit the write.
        if self.out.len().saturating_add(buf.len()) > self.cap {
            self.overflow = true;
            return Err(io::Error::other("decompressed-stream limit"));
        }
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Decodes an `LZWDecode` stream into its original bytes (ISO 32000-1 §7.4.4.2).
///
/// `early_change` selects the PDF `/EarlyChange` variant: `true` is the default
/// (`/EarlyChange 1`, the TIFF size-switch) and `false` is `/EarlyChange 0`. It
/// **must** match the value the stream was produced with.
///
/// Empty input is **not** an error: `decode(&[], _, _)` is `Ok(vec![])`.
///
/// # Errors
///
/// - [`Error::Decode`] (`LZWDecode`) if the code stream is truncated (no
///   `EndOfData` marker) or corrupt (an out-of-range code).
/// - [`Error::LimitExceeded`] ([`LimitKind::DecompressedStream`]) if the output
///   would exceed `limits.max_decompressed_stream`.
pub fn decode(input: &[u8], early_change: bool, limits: &Limits) -> Result<Vec<u8>> {
    // An empty stream decodes to nothing; don't hand it to `weezl`, whose
    // end-marker-requiring `decode_all` would report it as truncated.
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut decoder = if early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, CODE_SIZE)
    } else {
        Decoder::new(BitOrder::Msb, CODE_SIZE)
    };

    let mut cw = CapWriter {
        out: Vec::new(),
        cap: limits.max_decompressed_stream,
        overflow: false,
    };

    // Stream the decode into the capped writer. `&[u8]` implements `BufRead`,
    // so the input slice is fed directly. `decode_all` requires the stream to
    // end on an `EndOfData` marker, so truncation is reported as an error.
    let result = decoder.into_stream(&mut cw).decode_all(input);

    match result.status {
        Ok(()) => Ok(cw.out),
        // The bomb guard is authoritative: if the writer tripped, the surfaced
        // io error is *our* limit error regardless of its kind.
        Err(_) if cw.overflow => Err(Error::LimitExceeded(LimitKind::DecompressedStream)),
        // Anything else (an invalid code, or no end marker before EOF) is a
        // genuine decode failure.
        Err(_) => Err(Error::decode(FILTER, "corrupt or truncated LZW stream")),
    }
}

/// Encodes `input` as an `LZWDecode` stream — the inverse of [`decode`] for the
/// matching `early_change` variant.
///
/// `early_change` selects the same `/EarlyChange` variant semantics as
/// [`decode`]: `true` for the default (`/EarlyChange 1`), `false` for
/// `/EarlyChange 0`. The two variants differ at code-width-increase boundaries,
/// so for structured inputs of non-trivial length the encoded bytes for the two
/// values differ (they may coincide for very short inputs that never reach a
/// boundary).
///
/// Round-trip invariant: for every `x` and either `ec`,
/// `decode(&encode(x, ec), ec, &Limits::unbounded_decode()) == Ok(x)`.
///
/// Infallible: writing into an in-memory `Vec<u8>` cannot fail, so no error
/// path is reachable.
#[must_use]
pub fn encode(input: &[u8], early_change: bool) -> Vec<u8> {
    let mut encoder = if early_change {
        Encoder::with_tiff_size_switch(BitOrder::Msb, CODE_SIZE)
    } else {
        Encoder::new(BitOrder::Msb, CODE_SIZE)
    };

    let mut out: Vec<u8> = Vec::new();
    // Writing to / finishing into an in-memory `Vec<u8>` is infallible (the
    // `io::Write` impl for `Vec` never errors, so `encode_all` cannot fail on a
    // write). On the unreachable error path we degrade to the empty stream
    // rather than panic, keeping this total.
    if encoder
        .into_stream(&mut out)
        .encode_all(input)
        .status
        .is_err()
    {
        return Vec::new();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unbounded() -> Limits {
        Limits::unbounded_decode()
    }

    #[test]
    fn empty_input_decodes_to_empty() {
        assert_eq!(decode(&[], true, &unbounded()).unwrap(), Vec::<u8>::new());
        assert_eq!(decode(&[], false, &unbounded()).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn round_trip_both_early_change_variants() {
        let cases: &[&[u8]] = &[
            b"",
            b"A",
            b"hello, hello, hello, world!",
            b"-----------------------------------------------------------------",
            b"The quick brown fox jumps over the lazy dog. The quick brown fox.",
        ];
        for &case in cases {
            for ec in [true, false] {
                let encoded = encode(case, ec);
                let decoded = decode(&encoded, ec, &unbounded()).unwrap();
                assert_eq!(decoded, case, "round-trip failed for early_change={ec}");
            }
        }
    }

    #[test]
    fn round_trip_large_structured_input() {
        let mut data = Vec::new();
        for i in 0..4096u32 {
            data.extend_from_slice(format!("row{i:04}-payload;").as_bytes());
        }
        for ec in [true, false] {
            let encoded = encode(&data, ec);
            assert_eq!(decode(&encoded, ec, &unbounded()).unwrap(), data);
        }
    }

    #[test]
    fn early_change_variants_differ_for_long_input() {
        // Long enough to cross at least one code-width-increase boundary, where
        // EarlyChange=1 vs 0 diverge.
        let mut data = Vec::new();
        for i in 0..2048u32 {
            data.extend_from_slice(format!("{i:08}").as_bytes());
        }
        assert_ne!(encode(&data, true), encode(&data, false));
    }

    #[test]
    fn cross_variant_decode_is_wrong_or_errors_but_never_panics() {
        // Decoding with the wrong EarlyChange must not panic; it either errors
        // or yields different bytes. This guards the totality contract.
        let mut data = Vec::new();
        for i in 0..2048u32 {
            data.extend_from_slice(format!("{i:08}").as_bytes());
        }
        let encoded = encode(&data, true);
        // Decoding with the wrong flag must not panic; it yields different bytes
        // or a typed error — both acceptable.
        if let Ok(wrong) = decode(&encoded, false, &unbounded()) {
            assert_ne!(wrong, data);
        }
    }

    #[test]
    fn truncated_stream_errors() {
        let encoded = encode(b"some reasonably long input to truncate safely", true);
        let truncated = &encoded[..encoded.len() / 2];
        match decode(truncated, true, &unbounded()) {
            Err(Error::Decode { filter, .. }) => assert_eq!(filter, FILTER),
            other => panic!("expected a decode error for a truncated LZW stream, got {other:?}"),
        }
    }

    #[test]
    fn bomb_guard_trips_limit() {
        // A highly compressible input that decodes to far more than the cap.
        let data = vec![b'A'; 200_000];
        let encoded = encode(&data, true);
        let limits = Limits {
            max_decompressed_stream: 1024,
            ..Limits::default()
        };
        match decode(&encoded, true, &limits) {
            Err(Error::LimitExceeded(LimitKind::DecompressedStream)) => {}
            other => panic!("expected LimitExceeded, got {other:?}"),
        }
    }
}
