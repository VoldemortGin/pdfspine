//! FlateDecode codec unit tests — the `FLATE-DEC-*` catalog (M1b).
//!
//! Spec source of truth: ISO 32000-1 §7.4.4, RFC 1950/1951, PRD §8.3 (trailing
//! garbage / raw-deflate fallback) and §9.6.2 (decompression-bomb guard).
//!
//! Every case asserts totality: well-formed input round-trips, malformed input
//! yields a typed [`pdf_core::Error`] (never a panic), and a tiny input cannot
//! be coaxed into an unbounded allocation.

use pdf_core::filters::flate::{decode, encode};
use pdf_core::{Error, LimitKind, Limits};

/// Fills `buf` with a deterministic pseudo-random byte stream (a small LCG —
/// numerical recipes constants) so tests stay reproducible without a `rand` dep.
fn lcg_fill(buf: &mut [u8], seed: u32) {
    let mut state = seed;
    for b in buf.iter_mut() {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *b = (state >> 24) as u8;
    }
}

#[test]
fn flate_dec_001_empty() {
    // FLATE-DEC-001: empty round-trips, and a literally empty input decodes
    // to empty (an empty stream is legal PDF, not a truncation).
    let lim = Limits::unbounded_decode();
    assert_eq!(decode(&encode(b""), &lim).unwrap(), Vec::<u8>::new());
    assert_eq!(decode(&[], &lim).unwrap(), Vec::<u8>::new());
}

#[test]
fn flate_dec_002_roundtrip_hello() {
    // FLATE-DEC-002: the canonical small round-trip.
    let lim = Limits::unbounded_decode();
    assert_eq!(decode(&encode(b"hello"), &lim).unwrap(), b"hello");
}

#[test]
fn flate_dec_003_known_zlib_bytes() {
    // FLATE-DEC-003: a fixed literal zlib stream decodes to a fixed string.
    // These bytes are the deterministic zlib (RFC 1950) encoding of b"hello"
    // at the default level: 78 9c header, the deflate body, then the Adler-32
    // trailer (0x00062c15). Verified against `encode(b"hello")`.
    let known: [u8; 13] = [
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,
    ];
    assert_eq!(
        decode(&known, &Limits::unbounded_decode()).unwrap(),
        b"hello"
    );
}

#[test]
fn flate_dec_004_roundtrip_64kib_pseudo_random() {
    // FLATE-DEC-004: 64 KiB of deterministic pseudo-random data round-trips.
    let mut data = vec![0u8; 64 * 1024];
    lcg_fill(&mut data, 0x1234_5678);
    let lim = Limits::unbounded_decode();
    assert_eq!(decode(&encode(&data), &lim).unwrap(), data);
}

#[test]
fn flate_dec_005_roundtrip_highly_compressible() {
    // FLATE-DEC-005: a 100k run of one byte round-trips AND actually compresses.
    let data = b"A".repeat(100_000);
    let enc = encode(&data);
    assert!(
        enc.len() < 100_000,
        "expected compression, got {} bytes",
        enc.len()
    );
    assert_eq!(decode(&enc, &Limits::unbounded_decode()).unwrap(), data);
}

#[test]
fn flate_dec_006_truncated_is_decode_error() {
    // FLATE-DEC-006: cutting a valid stream in half yields a decode error, no panic.
    let enc = encode(b"hello world, this is a reasonably long payload to truncate");
    let half = &enc[..enc.len() / 2];
    let err = decode(half, &Limits::unbounded_decode()).unwrap_err();
    assert_eq!(err.kind(), "decode", "unexpected error: {err:?}");
}

#[test]
fn flate_dec_007_corrupt_body_is_decode_error() {
    // FLATE-DEC-007: flipping bytes in the deflate body breaks Huffman/LZ77 →
    // a decode error (kind "decode"), never a panic.
    let mut enc = encode(b"hello world, the quick brown fox jumps over the lazy dog");
    // Corrupt several bytes in the middle of the compressed body (past the
    // 2-byte zlib header, before the 4-byte Adler trailer).
    let mid = enc.len() / 2;
    for b in &mut enc[mid..mid + 4] {
        *b ^= 0xff;
    }
    let err = decode(&enc, &Limits::unbounded_decode()).unwrap_err();
    assert_eq!(err.kind(), "decode", "unexpected error: {err:?}");
}

#[test]
fn flate_dec_008_trailing_garbage_ignored() {
    // FLATE-DEC-008: bytes after a valid zlib stream are silently ignored.
    let mut s = encode(b"hello");
    s.extend_from_slice(b"GARBAGE TRAILING");
    assert_eq!(decode(&s, &Limits::unbounded_decode()).unwrap(), b"hello");
}

#[test]
fn flate_dec_009_raw_deflate_fallback() {
    // FLATE-DEC-009: a bare RFC 1951 deflate stream (no zlib header/trailer)
    // decodes via the raw-deflate fallback. Strip the 2-byte zlib header and
    // 4-byte Adler trailer from a wrapped stream to obtain the raw body.
    let z = encode(b"hello");
    let raw = &z[2..z.len() - 4];
    assert_eq!(decode(raw, &Limits::unbounded_decode()).unwrap(), b"hello");
}

#[test]
fn flate_dec_010_bomb_guard() {
    // FLATE-DEC-010: a tiny input that expands past the ceiling trips the bomb
    // guard with LimitKind::DecompressedStream — bounded, never OOM.
    let mut lim = Limits::default();
    lim.max_decompressed_stream = 16;
    let enc = encode(&b"A".repeat(100_000));
    let err = decode(&enc, &lim).unwrap_err();
    assert!(
        matches!(err, Error::LimitExceeded(LimitKind::DecompressedStream)),
        "unexpected error: {err:?}"
    );
    assert_eq!(err.kind(), "limit-exceeded");
}
