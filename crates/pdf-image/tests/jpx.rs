//! `JPX-*` — JPXDecode (JPEG 2000) codec tests (PRD §8.4.1 documented subset).
//!
//! Assets are JP2-boxed files produced by OpenJPEG (via PIL). The codec is a
//! documented subset behind the §8.4.1 degradation contract: a decode failure /
//! unsupported feature is a typed error for that image only, never a panic, and
//! a declared-huge raster trips the pixel cap instead of OOMing.

mod codec_common;

use codec_common::*;

use pdf_image::codecs::{jpx, ColorSpaceHint};

// --- JPX-GRAY-001: baseline grayscale JP2 ---------------------------------

#[test]
fn jpx_gray_001_baseline() {
    let doc = empty_doc();
    let data = include_bytes!("assets/gray.jp2");
    let params = dict([]);
    let img = jpx::decode(&doc, data, &params).expect("decode gray jp2");
    assert_eq!((img.width, img.height), (8, 8));
    assert_eq!(img.components, 1);
    assert_eq!(img.bits, 8);
    assert_eq!(img.colorspace, ColorSpaceHint::Gray);
    assert_eq!(img.data.len(), 64);
}

// --- JPX-RGB-001: baseline sRGB JP2 ---------------------------------------

#[test]
fn jpx_rgb_001_baseline() {
    let doc = empty_doc();
    let data = include_bytes!("assets/rgb.jp2");
    let params = dict([]);
    let img = jpx::decode(&doc, data, &params).expect("decode rgb jp2");
    assert_eq!((img.width, img.height), (8, 8));
    assert_eq!(img.components, 3);
    assert_eq!(img.bits, 8);
    assert_eq!(img.colorspace, ColorSpaceHint::Rgb);
    assert_eq!(img.data.len(), 8 * 8 * 3);
}

// --- JPX-ERR-001: garbage fails closed (no panic) -------------------------

#[test]
fn jpx_err_001_garbage() {
    let doc = empty_doc();
    let params = dict([]);
    let err = jpx::decode(&doc, b"\x00\x00\x00\x0Cnope not jp2", &params).unwrap_err();
    // Subset / unsupported / decode all acceptable; must be a typed error.
    assert!(matches!(
        err.kind(),
        "unsupported" | "decode" | "limit-exceeded"
    ));
}

// --- JPX-ERR-002: empty input fails closed --------------------------------

#[test]
fn jpx_err_002_empty() {
    let doc = empty_doc();
    let res = jpx::decode(&doc, &[], &dict([]));
    assert!(res.is_err());
}
