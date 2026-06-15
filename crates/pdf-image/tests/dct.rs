//! `DCT-*` — DCTDecode (JPEG) codec tests (PRD §8.4 / §11.1).
//!
//! Assets in `tests/assets/` are generated once (PIL/OpenJPEG); baseline JPEGs
//! are also synthesized in-test via the `image` crate's `JpegEncoder` so the
//! zune-jpeg primary can be cross-checked against the `jpeg-decoder` oracle on
//! freshly-encoded pixels (PRD §8.4.1 multi-oracle discipline).

mod codec_common;

use codec_common::*;

use pdf_image::codecs::{dct, ColorSpaceHint};

use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;

/// Encodes raw samples to a baseline JPEG using the `image` crate.
fn encode_jpeg(data: &[u8], w: u32, h: u32, ct: ExtendedColorType, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode(data, w, h, ct).expect("jpeg encode");
    out
}

/// Independent oracle decode via `jpeg-decoder`.
fn oracle_decode(jpeg: &[u8]) -> (jpeg_decoder::ImageInfo, Vec<u8>) {
    let mut d = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpeg));
    let px = d.decode().expect("oracle decode");
    let info = d.info().expect("oracle info");
    (info, px)
}

// --- DCT-RGB-001: baseline RGB, dimensions + plausibility -----------------

#[test]
fn dct_rgb_001_baseline_dimensions_and_pixels() {
    let doc = empty_doc();
    // 8x8 RGB gradient.
    let (w, h) = (8u32, 8u32);
    let mut raw = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            raw.push((x * 32) as u8);
            raw.push((y * 32) as u8);
            raw.push(128);
        }
    }
    let jpeg = encode_jpeg(&raw, w, h, ExtendedColorType::Rgb8, 92);
    let params = dict([("Width", int(w as i64)), ("Height", int(h as i64))]);

    let img = dct::decode(&doc, &jpeg, &params).expect("decode rgb jpeg");
    assert_eq!(img.width, w);
    assert_eq!(img.height, h);
    assert_eq!(img.components, 3);
    assert_eq!(img.bits, 8);
    assert_eq!(img.colorspace, ColorSpaceHint::Rgb);
    assert_eq!(img.data.len(), (w * h * 3) as usize);
    // Blue channel was a constant ~128; spot-check it survived the round trip.
    let blue0 = img.data[2] as i32;
    assert!((blue0 - 128).abs() < 30, "blue {blue0} not near 128");
}

// --- DCT-GRAY-001: baseline grayscale -------------------------------------

#[test]
fn dct_gray_001_baseline_gray() {
    let doc = empty_doc();
    let gray = include_bytes!("assets/gray.jpg");
    let params = dict([("Width", int(8)), ("Height", int(8))]);
    let img = dct::decode(&doc, gray, &params).expect("decode gray jpeg");
    assert_eq!((img.width, img.height), (8, 8));
    assert_eq!(img.components, 1);
    assert_eq!(img.bits, 8);
    assert_eq!(img.colorspace, ColorSpaceHint::Gray);
    assert_eq!(img.data.len(), 64);
}

// --- DCT-PROG-001: progressive JPEG ---------------------------------------

#[test]
fn dct_prog_001_progressive_rgb() {
    let doc = empty_doc();
    let prog = include_bytes!("assets/progressive_rgb.jpg");
    let params = dict([("Width", int(8)), ("Height", int(8))]);
    let img = dct::decode(&doc, prog, &params).expect("decode progressive jpeg");
    assert_eq!((img.width, img.height), (8, 8));
    assert_eq!(img.components, 3);
    assert_eq!(img.data.len(), 8 * 8 * 3);
}

// --- DCT-XCHECK-001: zune vs jpeg-decoder agreement -----------------------

#[test]
fn dct_xcheck_001_zune_vs_jpeg_decoder_rgb() {
    let doc = empty_doc();
    let (w, h) = (16u32, 12u32);
    let mut raw = Vec::new();
    for y in 0..h {
        for x in 0..w {
            raw.push(((x * 13) % 256) as u8);
            raw.push(((y * 17) % 256) as u8);
            raw.push(((x + y) * 7 % 256) as u8);
        }
    }
    // High quality so the two decoders agree closely.
    let jpeg = encode_jpeg(&raw, w, h, ExtendedColorType::Rgb8, 100);

    let ours = dct::decode(
        &doc,
        &jpeg,
        &dict([("Width", int(w as i64)), ("Height", int(h as i64))]),
    )
    .expect("zune decode");
    let (info, oracle) = oracle_decode(&jpeg);
    assert_eq!(info.width as u32, ours.width);
    assert_eq!(info.height as u32, ours.height);
    assert_eq!(oracle.len(), ours.data.len());

    // Both decode the same JPEG; allow a tiny tolerance for IDCT rounding diffs.
    let mut max_diff = 0i32;
    for (a, b) in ours.data.iter().zip(oracle.iter()) {
        max_diff = max_diff.max((*a as i32 - *b as i32).abs());
    }
    assert!(
        max_diff <= 3,
        "zune vs jpeg-decoder max channel diff {max_diff} > 3"
    );
}

// --- DCT-CMYK-001: native CMYK component count ----------------------------

#[test]
fn dct_cmyk_001_native_cmyk_components() {
    let doc = empty_doc();
    let cmyk = include_bytes!("assets/cmyk.jpg");
    let params = dict([("Width", int(4)), ("Height", int(4))]);
    let img = dct::decode(&doc, cmyk, &params).expect("decode cmyk jpeg");
    assert_eq!((img.width, img.height), (4, 4));
    assert_eq!(img.components, 4);
    assert_eq!(img.colorspace, ColorSpaceHint::Cmyk);
    assert_eq!(img.data.len(), 4 * 4 * 4);
}

// --- DCT-CMYK-DECODE-001: /Decode array inverts CMYK ----------------------

#[test]
fn dct_cmyk_decode_001_decode_inversion() {
    let doc = empty_doc();
    let cmyk = include_bytes!("assets/cmyk.jpg");

    // Identity /Decode [0 1 …] keeps zune's *raw* (Adobe-inverted) CMYK samples.
    let identity = array((0..4).flat_map(|_| [int(0), int(1)]));
    let raw = dct::decode(
        &doc,
        cmyk,
        &dict([("Width", int(4)), ("Height", int(4)), ("Decode", identity)]),
    )
    .expect("decode cmyk identity /Decode");

    // Inverting /Decode [1 0 …] must produce the exact per-byte complement.
    let inv = array((0..4).flat_map(|_| [int(1), int(0)]));
    let inverted = dct::decode(
        &doc,
        cmyk,
        &dict([("Width", int(4)), ("Height", int(4)), ("Decode", inv)]),
    )
    .expect("decode cmyk inverting /Decode");

    assert_eq!(raw.data.len(), inverted.data.len());
    assert_ne!(
        raw.data, inverted.data,
        "/Decode [1 0 …] must change samples"
    );
    for (a, b) in raw.data.iter().zip(inverted.data.iter()) {
        assert_eq!(*a, 255 - *b, "inverting /Decode is not the byte complement");
    }

    // With NO /Decode at all and an Adobe APP14 marker present, our default
    // un-inverts the Adobe samples — which equals the inverting-/Decode result.
    let app14_default = dct::decode(&doc, cmyk, &dict([("Width", int(4)), ("Height", int(4))]))
        .expect("decode cmyk no /Decode");
    assert_eq!(
        app14_default.data, inverted.data,
        "APP14 default un-inversion should match /Decode [1 0 …]"
    );
}

// --- DCT-ERR-001: garbage input fails closed (no panic) -------------------

#[test]
fn dct_err_001_garbage_is_typed_error() {
    let doc = empty_doc();
    let params = dict([("Width", int(8)), ("Height", int(8))]);
    let err = dct::decode(&doc, b"not a jpeg at all", &params).unwrap_err();
    assert_eq!(err.kind(), "decode");
}

// --- DCT-ERR-002: truncated JPEG fails closed -----------------------------

#[test]
fn dct_err_002_truncated_jpeg() {
    let doc = empty_doc();
    let gray = include_bytes!("assets/gray.jpg");
    let truncated = &gray[..gray.len() / 2];
    let params = dict([("Width", int(8)), ("Height", int(8))]);
    let res = dct::decode(&doc, truncated, &params);
    // Never a panic and never an Ok with wrong size — accept only an Err here.
    assert!(res.is_err(), "truncated jpeg must not decode cleanly");
}
