//! RunLength codec unit tests — the `RL-DEC-*` catalog (M1b).
//!
//! Covers RL-DEC-001..006: the EOD marker, literal and replicate runs, the
//! terminator stopping decode, truncation errors, and round-trips.
//!
//! Spec source of truth: ISO 32000-1 §7.4.5, PRD §8.3.

use pdf_core::filters::run_length::{decode, encode};
use pdf_core::Limits;

/// Unbounded limits for round-trip decodes.
fn u() -> Limits {
    Limits::unbounded_decode()
}

/// Deterministic LCG fill (no `rand` dependency).
fn lcg_fill(n: usize, mut seed: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push((seed >> 24) as u8);
    }
    v
}

#[test]
fn rl_dec_001_eod_and_empty() {
    // RL-DEC-001: a lone EOD byte (128) and empty input yield no bytes.
    assert_eq!(decode(b"\x80", &u()).unwrap(), Vec::<u8>::new());
    assert_eq!(decode(b"", &u()).unwrap(), Vec::<u8>::new());
}

#[test]
fn rl_dec_002_literal_run() {
    // RL-DEC-002: length byte 2 copies the next 3 literal bytes.
    assert_eq!(
        decode(&[0x02u8, b'a', b'b', b'c', 0x80], &u()).unwrap(),
        b"abc".to_vec()
    );
}

#[test]
fn rl_dec_003_replicate_run() {
    // RL-DEC-003: 0xFD (253) replicates the next byte 257-253 = 4 times.
    assert_eq!(
        decode(&[0xFDu8, b'x', 0x80], &u()).unwrap(),
        b"xxxx".to_vec()
    );
}

#[test]
fn rl_dec_004_terminator_stops_decode() {
    // RL-DEC-004: 0x80 terminates; trailing bytes after it are ignored.
    assert_eq!(
        decode(&[0x00u8, b'A', 0x80, 0x00, b'B'], &u()).unwrap(),
        b"A".to_vec()
    );
}

#[test]
fn rl_dec_005_truncated() {
    // RL-DEC-005: truncated literal and replicate runs are decode errors.
    let e1 = decode(&[0x05u8, b'a'], &u()).unwrap_err();
    assert_eq!(e1.kind(), "decode");
    let e2 = decode(&[0xFDu8], &u()).unwrap_err();
    assert_eq!(e2.kind(), "decode");
}

#[test]
fn rl_dec_006_round_trip() {
    // RL-DEC-006: encode/decode round-trips over assorted buffers.
    let alt: Vec<u8> = b"\x00\xff".repeat(100);
    let rnd = lcg_fill(4 * 1024, 0xDEAD_BEEF);
    let cases: Vec<Vec<u8>> = vec![b"".to_vec(), b"aaaaaabbbbcdefff".to_vec(), alt, rnd];
    for x in cases {
        assert_eq!(decode(&encode(&x), &u()).unwrap(), x);
    }
}
