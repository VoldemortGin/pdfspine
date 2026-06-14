//! ASCIIHex / ASCII85 codec unit tests — the `AHX-DEC-*` and `A85-DEC-*`
//! catalogs (M1b).
//!
//! Covers AHX-DEC-001..007 and A85-DEC-001..007: terminators, decoding of
//! known fixtures, whitespace handling, partial/odd final groups, the `z` and
//! `<~` shortcuts, error reporting, and round-trips.
//!
//! Spec source of truth: ISO 32000-1 §7.4.2 (ASCIIHex), §7.4.3 (ASCII85).

use pdf_core::filters::{ascii85, ascii_hex};
use pdf_core::Limits;

/// Unbounded limits for round-trip decodes.
fn u() -> Limits {
    Limits::unbounded_decode()
}

// ---------------------------------------------------------------------------
// ASCIIHex
// ---------------------------------------------------------------------------

#[test]
fn ahx_dec_001_terminator_and_empty() {
    // AHX-DEC-001: bare terminator and empty input both yield no bytes.
    assert_eq!(ascii_hex::decode(b">", &u()).unwrap(), Vec::<u8>::new());
    assert_eq!(ascii_hex::decode(b"", &u()).unwrap(), Vec::<u8>::new());
}

#[test]
fn ahx_dec_002_known_decode() {
    // AHX-DEC-002: "48656C6C6F>" decodes to "Hello".
    assert_eq!(
        ascii_hex::decode(b"48656C6C6F>", &u()).unwrap(),
        b"Hello".to_vec()
    );
}

#[test]
fn ahx_dec_003_whitespace_skipped() {
    // AHX-DEC-003: interleaved whitespace is ignored.
    assert_eq!(
        ascii_hex::decode(b"48 65\n6C\t6C 6F>", &u()).unwrap(),
        b"Hello".to_vec()
    );
}

#[test]
fn ahx_dec_004_odd_digit_padded() {
    // AHX-DEC-004: a trailing odd digit is padded with a 0 nibble.
    assert_eq!(ascii_hex::decode(b"4>", &u()).unwrap(), vec![0x40]);
    assert_eq!(ascii_hex::decode(b"486>", &u()).unwrap(), vec![0x48, 0x60]);
}

#[test]
fn ahx_dec_005_terminator_stops_and_missing_tolerated() {
    // AHX-DEC-005: bytes after '>' are ignored; a missing '>' is tolerated.
    assert_eq!(ascii_hex::decode(b"48>FFFF", &u()).unwrap(), b"H".to_vec());
    assert_eq!(ascii_hex::decode(b"4865", &u()).unwrap(), b"He".to_vec());
}

#[test]
fn ahx_dec_006_invalid_char() {
    // AHX-DEC-006: a non-hex, non-whitespace char is a decode error.
    let e = ascii_hex::decode(b"48ZZ>", &u()).unwrap_err();
    assert_eq!(e.kind(), "decode");
}

#[test]
fn ahx_dec_007_round_trip() {
    // AHX-DEC-007: encode/decode round-trips over assorted byte strings.
    let cases: [&[u8]; 3] = [b"", b"\x00\xff\x10", b"Hello, World!"];
    for x in cases {
        assert_eq!(
            ascii_hex::decode(&ascii_hex::encode(x), &u()).unwrap(),
            x.to_vec()
        );
    }
}

// ---------------------------------------------------------------------------
// ASCII85
// ---------------------------------------------------------------------------

#[test]
fn a85_dec_001_terminator_and_empty() {
    // A85-DEC-001: bare "~>" terminator and empty input yield no bytes.
    assert_eq!(ascii85::decode(b"~>", &u()).unwrap(), Vec::<u8>::new());
    assert_eq!(ascii85::decode(b"", &u()).unwrap(), Vec::<u8>::new());
}

#[test]
fn a85_dec_002_known_bytes() {
    // A85-DEC-002: known fixture "Man " <-> "9jqo^~>".
    assert_eq!(ascii85::decode(b"9jqo^~>", &u()).unwrap(), b"Man ".to_vec());
    assert_eq!(ascii85::encode(b"Man "), b"9jqo^~>".to_vec());
}

#[test]
fn a85_dec_003_z_shortcut() {
    // A85-DEC-003: 'z' expands to four zero bytes.
    assert_eq!(ascii85::decode(b"z~>", &u()).unwrap(), vec![0, 0, 0, 0]);
}

#[test]
fn a85_dec_004_partial_final_groups() {
    // A85-DEC-004: 1/2/3-byte final groups round-trip.
    let cases: [&[u8]; 3] = [b"A", b"AB", b"ABC"];
    for x in cases {
        assert_eq!(
            ascii85::decode(&ascii85::encode(x), &u()).unwrap(),
            x.to_vec()
        );
    }
    // Hand-crafted decode of the known one-byte fixture.
    assert_eq!(ascii85::decode(b"5l~>", &u()).unwrap(), b"A".to_vec());
}

#[test]
fn a85_dec_005_whitespace_and_lead_in() {
    // A85-DEC-005: a leading "<~" and interleaved whitespace are skipped.
    let enc = ascii85::encode(b"hello world");
    let mut noisy = Vec::new();
    noisy.extend_from_slice(b"<~");
    for &c in &enc {
        noisy.push(c);
        noisy.push(b' ');
    }
    assert_eq!(
        ascii85::decode(&noisy, &u()).unwrap(),
        b"hello world".to_vec()
    );
}

#[test]
fn a85_dec_006_errors() {
    // A85-DEC-006: out-of-range char and a single-char final group are decode
    // errors (and must not panic).
    let e1 = ascii85::decode(b"vvvv~>", &u()).unwrap_err();
    assert_eq!(e1.kind(), "decode");
    let e2 = ascii85::decode(b"!~>", &u()).unwrap_err();
    assert_eq!(e2.kind(), "decode");
}

#[test]
fn a85_dec_007_round_trip() {
    // A85-DEC-007: encode/decode round-trips, exercising the 'z' shortcut.
    let cases: [&[u8]; 4] = [
        b"",
        b"\x00\x00\x00\x00",
        b"\x00\x00\x00\x00\x01",
        b"The quick brown fox",
    ];
    for x in cases {
        assert_eq!(
            ascii85::decode(&ascii85::encode(x), &u()).unwrap(),
            x.to_vec()
        );
    }
}
