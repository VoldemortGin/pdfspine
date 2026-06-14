//! LZW codec unit tests — the `LZW-DEC-*` catalog (M1b).
//!
//! Covers LZW-DEC-001..008: round-trips, EarlyChange differentiation, a large
//! pseudo-random round-trip, truncated/garbage tolerance, a decompression-bomb
//! guard, and the predictor-over-LZW pipeline.
//!
//! Spec source of truth: ISO 32000-1 §7.4.4, PRD §8.3.

use pdf_core::filters::lzw::{decode, encode};
use pdf_core::filters::predictor::{predict, unpredict, PredictorParams};
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
fn lzw_dec_001_empty_round_trip() {
    // LZW-DEC-001: empty input round-trips under both EarlyChange modes.
    assert_eq!(
        decode(&encode(b"", true), true, &u()).unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(
        decode(&encode(b"", false), false, &u()).unwrap(),
        Vec::<u8>::new()
    );
}

#[test]
fn lzw_dec_002_repeating_round_trip() {
    // LZW-DEC-002: simple repeating ASCII round-trips.
    let input = b"hello hello hello";
    assert_eq!(
        decode(&encode(input, true), true, &u()).unwrap(),
        input.to_vec()
    );
}

#[test]
fn lzw_dec_003_classic_example_round_trip() {
    // LZW-DEC-003: classic LZW example input round-trips (no hardcoded codes).
    let input = b"-----A---B";
    assert_eq!(
        decode(&encode(input, true), true, &u()).unwrap(),
        input.to_vec()
    );
}

#[test]
fn lzw_dec_004_early_change_differentiation() {
    // LZW-DEC-004: build a >600 byte structured input that crosses a code-width
    // boundary, then verify EarlyChange changes the bitstream yet each flag
    // decodes correctly, and the wrong flag never panics.
    let mut x: Vec<u8> = Vec::new();
    for i in 0..200 {
        x.extend_from_slice(format!("{i:08}").as_bytes());
    }
    assert!(x.len() > 600);

    let enc_true = encode(&x, true);
    let enc_false = encode(&x, false);
    assert_ne!(enc_true, enc_false);

    assert_eq!(decode(&enc_true, true, &u()).unwrap(), x.clone());
    assert_eq!(decode(&enc_false, false, &u()).unwrap(), x.clone());

    // Wrong flag: tolerate either Ok or Err, just no panic.
    let _ = decode(&enc_true, false, &u());
    let _ = decode(&enc_false, true, &u());
}

#[test]
fn lzw_dec_005_large_random_round_trip() {
    // LZW-DEC-005: ~8 KiB pseudo-random round-trip with EarlyChange=true.
    let data = lcg_fill(8 * 1024, 0xC0FF_EE01);
    assert_eq!(decode(&encode(&data, true), true, &u()).unwrap(), data);
}

#[test]
fn lzw_dec_006_truncated_and_garbage_no_panic() {
    // LZW-DEC-006: truncated stream and garbage bytes must not panic.
    let input: Vec<u8> = b"abc".iter().cloned().cycle().take(50).collect();
    let enc = encode(&input, true);
    let cut = &enc[..enc.len() - 1];
    if let Err(e) = decode(cut, true, &u()) {
        assert_eq!(e.kind(), "decode");
    }

    // Obviously-bad bytes: returns (Ok or Err) without panic.
    let _ = decode(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF], true, &u());
}

#[test]
fn lzw_dec_007_bomb_guard() {
    // LZW-DEC-007: a tiny output ceiling trips the decompressed-stream guard.
    let data = vec![b'A'; 50000];
    let enc = encode(&data, true);
    let mut l = Limits::default();
    l.max_decompressed_stream = 16;
    assert!(matches!(
        decode(&enc, true, &l),
        Err(pdf_core::Error::LimitExceeded(
            pdf_core::LimitKind::DecompressedStream
        ))
    ));
}

#[test]
fn lzw_dec_008_predictor_over_lzw() {
    // LZW-DEC-008: predict -> LZW encode -> LZW decode -> unpredict round-trips.
    let raw = lcg_fill(40, 0x1234_5678); // 5 rows * 8 columns, 1 color, 8 bpc.
    let p = PredictorParams {
        predictor: 12,
        colors: 1,
        bits_per_component: 8,
        columns: 8,
    };
    let pred = predict(&raw, &p).unwrap();
    let enc = encode(&pred, true);
    let dec_lzw = decode(&enc, true, &u()).unwrap();
    let back = unpredict(&dec_lzw, &p, &u()).unwrap();
    assert_eq!(back, raw);
}
