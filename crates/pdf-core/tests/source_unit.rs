//! `SOURCE-*` — the bounds-checked `Source` backing-bytes abstraction.
//! PRD §9.2 / §9.6.1: every offset/len validated, never panic; owned/hard-safe
//! path; truncated-tail handled gracefully.

use pdf_core::source::{MmapMode, Source};
use pdf_core::{DocumentStore, Error, Limits};

#[test]
fn source_001_from_bytes_roundtrip() {
    // SOURCE-001
    let s = Source::from_bytes(b"hello world".to_vec());
    assert_eq!(s.bytes(), b"hello world");
    assert_eq!(s.len(), 11);
    assert!(!s.is_empty());
}

#[test]
fn source_002_empty_is_zero_length() {
    // SOURCE-002
    let s = Source::from_bytes(Vec::<u8>::new());
    assert!(matches!(s, Source::Empty));
    assert_eq!(s.len(), 0);
    assert!(s.is_empty());
    assert_eq!(s.bytes(), b"");
    // A zero-length slice at 0 is valid; any nonzero read errors (no panic).
    assert_eq!(s.slice(0, 0).unwrap(), b"");
    assert!(s.slice(0, 1).is_err());
}

#[test]
fn source_003_slice_in_range() {
    // SOURCE-003
    let s = Source::from_bytes(b"abcdefgh".to_vec());
    assert_eq!(s.slice(2, 3).unwrap(), b"cde");
    assert_eq!(s.slice_from(5).unwrap(), b"fgh");
    assert_eq!(s.byte_at(0).unwrap(), b'a');
    assert_eq!(s.slice_bytes(1, 2).unwrap().as_ref(), b"bc");
}

#[test]
fn source_004_out_of_bounds_typed_error() {
    // SOURCE-004: no panic, typed Error::Source.
    let s = Source::from_bytes(b"abc".to_vec());
    assert!(matches!(s.slice(2, 5), Err(Error::Source { .. })));
    assert!(matches!(s.slice(10, 1), Err(Error::Source { .. })));
    assert!(matches!(s.slice_from(99), Err(Error::Source { .. })));
    assert!(matches!(s.byte_at(99), Err(Error::Source { .. })));
    assert!(matches!(s.slice_bytes(2, 5), Err(Error::Source { .. })));
}

#[test]
fn source_005_length_overflow_typed_error() {
    // SOURCE-005: off + len wraps usize → typed error, not a panic.
    let s = Source::from_bytes(b"abc".to_vec());
    let err = s.slice(usize::MAX, 10).unwrap_err();
    assert!(matches!(err, Error::Source { .. }), "{err:?}");
    assert_eq!(err.kind(), "source");
}

#[test]
fn source_006_open_path_owned_hard_safe() {
    // SOURCE-006: open(path, Never) reads owned bytes.
    let dir = std::env::temp_dir();
    let path = dir.join(format!("pdfspine-source-006-{}.bin", std::process::id()));
    std::fs::write(&path, b"file contents here").unwrap();
    let s = Source::open(&path, MmapMode::Never).unwrap();
    assert_eq!(s.bytes(), b"file contents here");
    // Auto falls back to the same owned read in this build.
    let s2 = Source::open(&path, MmapMode::Auto).unwrap();
    assert_eq!(s2.bytes(), b"file contents here");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn source_007_truncated_tail_graceful() {
    // SOURCE-007: a buffer with no startxref opens-errors gracefully (no panic).
    // (A bare PDF header with no body / xref.)
    let mut bytes = b"%PDF-1.7\n".to_vec();
    bytes.extend_from_slice(&[0xE2, 0xE3, 0xCF, 0xD3, b'\n']);
    let res = DocumentStore::from_bytes(bytes, Limits::default());
    assert!(res.is_err(), "expected a typed error, got Ok");
    // The error is xref-shaped (no startxref), never a panic.
    let err = res.unwrap_err();
    assert!(matches!(err, Error::Xref { .. }), "{err:?}");
}
