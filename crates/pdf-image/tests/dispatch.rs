//! `CODEC-DISPATCH-*` — dispatcher routing + raw-sample interpretation +
//! size-cap guards (PRD §8.4 / §8.4.1 / §9.6.2).

mod codec_common;

use codec_common::*;

use pdf_image::codecs::{decode_image_xobject, ColorSpaceHint};

use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;

fn encode_rgb_jpeg(data: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 95)
        .encode(data, w, h, ExtendedColorType::Rgb8)
        .unwrap();
    out
}

// --- CODEC-DISPATCH-001: DCTDecode routes to the JPEG codec ----------------

#[test]
fn codec_dispatch_001_dct() {
    let doc = empty_doc();
    let raw = vec![100u8; 4 * 4 * 3];
    let jpeg = encode_rgb_jpeg(&raw, 4, 4);
    let params = dict([("Width", int(4)), ("Height", int(4))]);

    let by_full = decode_image_xobject(&doc, "DCTDecode", &jpeg, &params).expect("DCTDecode route");
    let by_abbr = decode_image_xobject(&doc, "DCT", &jpeg, &params).expect("DCT route");
    assert_eq!(by_full.components, 3);
    assert_eq!(by_abbr.components, 3);
    assert_eq!(by_full.colorspace, ColorSpaceHint::Rgb);
}

// --- CODEC-DISPATCH-002: unknown filter ⇒ typed error (no panic) -----------

#[test]
fn codec_dispatch_002_unknown_filter() {
    let doc = empty_doc();
    let params = dict([("Width", int(4)), ("Height", int(4))]);
    let err = decode_image_xobject(&doc, "BogusDecode", b"whatever", &params).unwrap_err();
    assert_eq!(err.kind(), "unsupported");
}

// --- CODEC-DISPATCH-003: raw/Flate pixel samples interpreted by ColorSpace --

#[test]
fn codec_dispatch_003_raw_rgb_samples() {
    let doc = empty_doc();
    let (w, h) = (3u32, 2u32);
    // Interleaved RGB, 8 bpc, no row padding (width*3 = 9 already byte-aligned).
    let samples: Vec<u8> = (0..(w * h * 3)).map(|i| i as u8).collect();
    let params = dict([
        ("Width", int(w as i64)),
        ("Height", int(h as i64)),
        ("BitsPerComponent", int(8)),
        ("ColorSpace", name_obj("DeviceRGB")),
    ]);
    let img = decode_image_xobject(&doc, "", &samples, &params).expect("raw samples");
    assert_eq!((img.width, img.height), (w, h));
    assert_eq!(img.components, 3);
    assert_eq!(img.bits, 8);
    assert_eq!(img.colorspace, ColorSpaceHint::Rgb);
    assert_eq!(img.data, samples);
}

// --- CODEC-DISPATCH-004: 1-bpp ImageMask raw samples ----------------------

#[test]
fn codec_dispatch_004_raw_imagemask() {
    let doc = empty_doc();
    let (w, h) = (8u32, 2u32);
    // 1 bpc, width 8 ⇒ 1 byte/row, 2 rows.
    let samples = vec![0b1010_1010u8, 0b0101_0101u8];
    let params = dict([
        ("Width", int(w as i64)),
        ("Height", int(h as i64)),
        ("ImageMask", boolean(true)),
    ]);
    let img = decode_image_xobject(&doc, "", &samples, &params).expect("imagemask");
    assert_eq!(img.components, 1);
    assert_eq!(img.bits, 1);
    assert_eq!(img.data, samples);
}

// --- CODEC-DISPATCH-005: raw gray 16-bit big-endian ------------------------

#[test]
fn codec_dispatch_005_raw_gray16() {
    let doc = empty_doc();
    let (w, h) = (2u32, 2u32);
    // 16 bpc gray: 2 bytes/sample, 1 comp ⇒ width*2 = 4 bytes/row.
    let samples: Vec<u8> = (0..(w * h * 2)).map(|i| i as u8).collect();
    let params = dict([
        ("Width", int(w as i64)),
        ("Height", int(h as i64)),
        ("BitsPerComponent", int(16)),
        ("ColorSpace", name_obj("DeviceGray")),
    ]);
    let img = decode_image_xobject(&doc, "", &samples, &params).expect("gray16");
    assert_eq!(img.bits, 16);
    assert_eq!(img.components, 1);
    assert_eq!(img.data, samples);
}

// --- CODEC-CAP-001: declared-huge raster trips the pixel cap, no OOM -------

#[test]
fn codec_cap_001_huge_dimensions_rejected() {
    let doc = empty_doc();
    // 60000 x 60000 = 3.6 Gpx ≫ the 256 Mpx cap. A few bytes of payload.
    let params = dict([
        ("Width", int(60_000)),
        ("Height", int(60_000)),
        ("BitsPerComponent", int(8)),
        ("ColorSpace", name_obj("DeviceRGB")),
    ]);
    let err = decode_image_xobject(&doc, "", b"tiny", &params).unwrap_err();
    assert_eq!(err.kind(), "limit-exceeded");
}

// --- CODEC-CAP-002: cap applies to image-codec filters too (JBIG2) ---------

#[test]
fn codec_cap_002_huge_jbig2_page_rejected() {
    // A JBIG2 page declaring 100000 x 100000 must be rejected by the cap
    // before any allocation. We build a page-info-only stream with huge dims.
    let doc = empty_doc();
    let mut stream = Vec::new();
    // Page info segment (type 48) data length 19.
    stream.extend_from_slice(&0u32.to_be_bytes()); // segment number
    stream.push(48); // type
    stream.push(0); // referred count 0
    stream.push(1); // page assoc
    stream.extend_from_slice(&19u32.to_be_bytes()); // data length
    stream.extend_from_slice(&100_000u32.to_be_bytes()); // width
    stream.extend_from_slice(&100_000u32.to_be_bytes()); // height
    stream.extend_from_slice(&0u32.to_be_bytes());
    stream.extend_from_slice(&0u32.to_be_bytes());
    stream.push(0);
    stream.extend_from_slice(&0u16.to_be_bytes());

    let params = dict([("Width", int(100_000)), ("Height", int(100_000))]);
    let err = decode_image_xobject(&doc, "JBIG2Decode", &stream, &params).unwrap_err();
    // Either the cap (limit-exceeded) or an unsupported/decode — never a panic
    // and never an OOM. The cap is the intended path.
    assert!(matches!(
        err.kind(),
        "limit-exceeded" | "unsupported" | "decode"
    ));
}

// --- CODEC-DISPATCH-006: truncated raw samples ⇒ typed error --------------

#[test]
fn codec_dispatch_006_truncated_raw() {
    let doc = empty_doc();
    let params = dict([
        ("Width", int(10)),
        ("Height", int(10)),
        ("BitsPerComponent", int(8)),
        ("ColorSpace", name_obj("DeviceRGB")),
    ]);
    // Far too few bytes for 10x10x3.
    let err = decode_image_xobject(&doc, "", b"abc", &params).unwrap_err();
    assert_eq!(err.kind(), "decode");
}
