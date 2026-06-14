//! `Limits` defaults + decompression-bomb guard — the `LIMITS-*` catalog (M1b).
//!
//! Spec source of truth: PRD §9.6.2 (pinned defaults) and §9.6 (never-OOM gate).
//! Each bomb test feeds a tiny input that would expand to far more than a small
//! cap and asserts the decoder returns [`pdf_core::Error::LimitExceeded`]
//! (bounded memory) rather than OOMing.

use pdf_core::filters::{flate, lzw, run_length};
use pdf_core::{Error, LimitKind, Limits};

fn tiny_cap() -> Limits {
    let mut l = Limits::default();
    l.max_decompressed_stream = 1024; // 1 KiB cap
    l
}

#[test]
fn limits_default_001_matches_prd_962() {
    // LIMITS-DEFAULT-001: Limits::default() matches the pinned §9.6.2 values.
    let d = Limits::default();
    assert_eq!(d.max_file_size, 4 * 1024 * 1024 * 1024);
    assert_eq!(d.max_objects, 8_388_608);
    assert_eq!(d.max_recursion_depth, 256);
    assert_eq!(d.max_decompressed_stream, 1024 * 1024 * 1024);
    assert_eq!(d.max_total_decompressed, 4 * 1024 * 1024 * 1024);
    assert_eq!(d.max_objstm_objects, 1_048_576);
    assert_eq!(d.max_decode_ratio, 200);
    // DEFAULT const and Default::default() agree.
    assert_eq!(Limits::DEFAULT, d);
}

#[test]
fn limits_bomb_001_flate() {
    // LIMITS-BOMB-001: a Flate bomb (tiny input, huge output) trips the limit.
    // 8 MiB of zeros compresses to a few KiB; decoding under a 1 KiB cap must
    // fail with LimitExceeded long before allocating 8 MiB.
    let bomb_src = vec![0u8; 8 * 1024 * 1024];
    let bomb = flate::encode(&bomb_src);
    assert!(bomb.len() < 64 * 1024, "bomb input should be small");

    let err = flate::decode(&bomb, &tiny_cap()).unwrap_err();
    assert!(matches!(
        err,
        Error::LimitExceeded(LimitKind::DecompressedStream)
    ));
    assert_eq!(err.kind(), "limit-exceeded");
}

#[test]
fn limits_bomb_002_lzw() {
    // LIMITS-BOMB-002: an LZW bomb trips the limit.
    let bomb_src = vec![0u8; 2 * 1024 * 1024];
    let bomb = lzw::encode(&bomb_src, true);
    assert!(bomb.len() < 256 * 1024, "lzw bomb input should be small");

    let err = lzw::decode(&bomb, true, &tiny_cap()).unwrap_err();
    assert!(matches!(
        err,
        Error::LimitExceeded(LimitKind::DecompressedStream)
    ));
}

#[test]
fn limits_bomb_003_runlength() {
    // LIMITS-BOMB-003: a RunLength bomb (replicate runs) trips the limit.
    // Each [0x81, b] expands to 128 bytes; 1000 such runs → 128 KiB from 2 KB.
    let mut bomb = Vec::new();
    for _ in 0..2000 {
        bomb.push(0x81); // 257-129 = 128 copies
        bomb.push(b'A');
    }
    bomb.push(128); // EOD

    let err = run_length::decode(&bomb, &tiny_cap()).unwrap_err();
    assert!(matches!(
        err,
        Error::LimitExceeded(LimitKind::DecompressedStream)
    ));
}
