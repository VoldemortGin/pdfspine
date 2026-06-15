//! `JBIG2-*` — JBIG2Decode codec tests (PRD §8.4.1 documented subset).
//!
//! No pure-Rust JBIG2 *encoder* exists in the dep set, so the fixtures are
//! assembled in-test from the JBIG2 segment grammar (ISO/IEC 14492 §7): a Page
//! Information segment plus an immediate-lossless **generic region** that uses
//! **MMR** coding. MMR is exactly CCITT Group 4, so the region's coded data is
//! produced by the `fax` G4 encoder (the same encoder the CCITT tests use). This
//! exercises the real `hayro-jbig2` segment parser + generic-region decode path
//! end-to-end while staying a fully self-built fixture (PRD §10).
//!
//! Per §8.4.1: unsupported features / malformed input return a typed error for
//! that image only (never a panic), and a declared-huge raster trips the pixel
//! cap instead of OOMing.

mod codec_common;

use codec_common::*;

use pdf_image::codecs::{jbig2, ColorSpaceHint};

use fax::encoder::Encoder;
use fax::{Color, VecWriter};

/// Builds a packed (MSB-first, 1=black) bitmap from a predicate.
fn make_bitmap(width: u32, rows: u32, f: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    let row_bytes = (width as usize).div_ceil(8);
    let mut out = vec![0u8; row_bytes * rows as usize];
    for y in 0..rows {
        for x in 0..width {
            if f(x, y) {
                out[y as usize * row_bytes + (x / 8) as usize] |= 1 << (7 - (x % 8));
            }
        }
    }
    out
}

/// Encodes a packed bitmap to CCITT G4 (== JBIG2 generic-region MMR data).
fn encode_g4(bits: &[u8], width: u32, rows: u32) -> Vec<u8> {
    let row_bytes = (width as usize).div_ceil(8);
    let mut enc = Encoder::new(VecWriter::new());
    for r in 0..rows as usize {
        let row = &bits[r * row_bytes..(r + 1) * row_bytes];
        let pels = (0..width as usize).map(|x| {
            let bit = (row[x / 8] >> (7 - (x % 8))) & 1;
            if bit == 1 {
                Color::Black
            } else {
                Color::White
            }
        });
        enc.encode_line(pels, width as u16).expect("encode line");
    }
    enc.finish().expect("finish").finish()
}

/// Assembles an **embedded** JBIG2 stream (Annex D.3): a page-information
/// segment + one immediate-lossless generic region segment carrying MMR data.
fn build_jbig2_embedded(width: u32, height: u32, mmr_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();

    // --- Segment 0: Page Information (type 48), data length 19 ---
    push_segment_header(&mut out, 0, 48, &[], 19);
    out.extend_from_slice(&width.to_be_bytes()); // page width
    out.extend_from_slice(&height.to_be_bytes()); // page height
    out.extend_from_slice(&0u32.to_be_bytes()); // x resolution unknown
    out.extend_from_slice(&0u32.to_be_bytes()); // y resolution unknown
    out.push(0x00); // page flags: lossless off, OR combine, default pixel 0
    out.extend_from_slice(&0u16.to_be_bytes()); // striping: not striped

    // --- Segment 1: Immediate Lossless Generic Region (type 39) ---
    // data = region segment info (17) + generic flags (1, MMR=1) + mmr_data
    let region_data_len = 17 + 1 + mmr_data.len();
    push_segment_header(&mut out, 1, 39, &[], region_data_len as u32);
    // Region segment info (7.4.1): width, height, x, y (4 each) + flags (1).
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // x location
    out.extend_from_slice(&0u32.to_be_bytes()); // y location
    out.push(0x00); // external combination operator = OR, no colour ext
                    // Generic region flags (7.4.6.2): bit0 = MMR.
    out.push(0x01);
    out.extend_from_slice(mmr_data);

    out
}

/// Writes a JBIG2 segment header (7.2) with the short referred-to form and a
/// 1-byte page association.
fn push_segment_header(
    out: &mut Vec<u8>,
    segment_number: u32,
    segment_type: u8,
    referred: &[u32],
    data_length: u32,
) {
    out.extend_from_slice(&segment_number.to_be_bytes());
    // Flags: bit7 retain(=0 means retained), bit6 page-assoc-size(0=1byte),
    // bits0-5 = type.
    out.push(segment_type & 0x3F);
    // Referred-to count + retention flags (short form, count < 7).
    let count = referred.len() as u8;
    out.push((count & 0x07) << 5);
    // Referred-to segment numbers (segment_number <= 256 ⇒ 1 byte each).
    for &r in referred {
        out.push(r as u8);
    }
    // Page association (1 byte): page 1.
    out.push(0x01);
    // Segment data length.
    out.extend_from_slice(&data_length.to_be_bytes());
}

// --- JBIG2-GENERIC-001: decode a generic-region (MMR) bitmap --------------

#[test]
fn jbig2_generic_001_mmr_roundtrip() {
    let doc = empty_doc();
    let (w, h) = (16u32, 8u32);
    // Black where x even (1 = black in JBIG2).
    let original = make_bitmap(w, h, |x, _| x % 2 == 0);
    let mmr = encode_g4(&original, w, h);
    let stream = build_jbig2_embedded(w, h, &mmr);

    let params = dict([("Width", int(w as i64)), ("Height", int(h as i64))]);
    let img = jbig2::decode(&doc, &stream, &params).expect("decode jbig2 generic");
    assert_eq!((img.width, img.height), (w, h));
    assert_eq!(img.components, 1);
    assert_eq!(img.bits, 1);
    assert_eq!(img.colorspace, ColorSpaceHint::Gray);
    // 1 = black = set bit, matching our `original` packing.
    assert_eq!(img.data, original, "JBIG2 generic MMR round trip mismatch");
}

// --- JBIG2-GENERIC-002: a richer pattern ----------------------------------

#[test]
fn jbig2_generic_002_pattern() {
    let doc = empty_doc();
    let (w, h) = (24u32, 10u32);
    let original = make_bitmap(w, h, |x, y| (x + y) % 3 == 0);
    let mmr = encode_g4(&original, w, h);
    let stream = build_jbig2_embedded(w, h, &mmr);
    let params = dict([("Width", int(w as i64)), ("Height", int(h as i64))]);
    let img = jbig2::decode(&doc, &stream, &params).expect("decode jbig2 pattern");
    assert_eq!(img.data, original);
}

// --- JBIG2-ERR-001: garbage fails closed (no panic) -----------------------

#[test]
fn jbig2_err_001_garbage() {
    let doc = empty_doc();
    let params = dict([("Width", int(16)), ("Height", int(8))]);
    let err = jbig2::decode(&doc, b"\x00\x01\x02\x03not jbig2", &params).unwrap_err();
    assert!(matches!(
        err.kind(),
        "unsupported" | "decode" | "limit-exceeded"
    ));
}

// --- JBIG2-ERR-002: empty input fails closed ------------------------------

#[test]
fn jbig2_err_002_empty() {
    let doc = empty_doc();
    let res = jbig2::decode(&doc, &[], &dict([]));
    assert!(res.is_err());
}
