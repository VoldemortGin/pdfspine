//! `MD-IMG-*` — image embedding: `data:` URIs and local paths, JPEG DCT
//! passthrough, PNG decode via `pdf-image`. Strictly no network.

mod common;

use common::{assert_in_order, full_text, page_count, raw, render, render_with};
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_markdown::{markdown_to_pdf, Options};

/// A real JPEG from the repo's own test assets (grayscale).
const JPEG: &[u8] = include_bytes!("../../pdf-image/tests/assets/gray.jpg");

/// Minimal standard-alphabet base64 encoder for building `data:` URIs.
fn base64_encode(data: &[u8]) -> String {
    const AL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(AL[(n >> 18) as usize & 63] as char);
        out.push(AL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            AL[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            AL[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// A tiny in-memory PNG (2×2 RGB) built with the repo's own encoder.
fn tiny_png() -> Vec<u8> {
    let samples = vec![
        255, 0, 0, 0, 255, 0, //
        0, 0, 255, 255, 255, 0,
    ];
    Pixmap::new(2, 2, Colorspace::Rgb, false, samples)
        .to_png_bytes()
        .expect("PNG encode should succeed")
}

#[test]
fn png_data_uri_embeds_as_flate_rgb_xobject() {
    let uri = format!("data:image/png;base64,{}", base64_encode(&tiny_png()));
    let bytes = render(&format!("before\n\n![alt]({uri})\n\nafter"));
    assert_eq!(page_count(&bytes), 1);
    assert_in_order(&full_text(&bytes), &["before", "after"]);
    let raw = raw(&bytes);
    assert!(raw.contains("/Subtype /Image"), "image XObject missing");
    assert!(raw.contains("/DeviceRGB"), "RGB colorspace expected");
    assert!(raw.contains("/Im0 Do"), "image placement op missing");
}

#[test]
fn jpeg_data_uri_passes_through_as_dctdecode() {
    let uri = format!("data:image/jpeg;base64,{}", base64_encode(JPEG));
    let bytes = render(&format!("![photo]({uri})"));
    let raw = raw(&bytes);
    assert!(raw.contains("/DCTDecode"), "JPEG must embed as DCT");
    assert!(raw.contains("/DeviceGray"), "gray JPEG colorspace expected");
    // The emitted PDF must reopen cleanly…
    assert_eq!(page_count(&bytes), 1);
    // …and carry the JPEG body verbatim (byte-equal passthrough, no re-encode).
    assert!(
        bytes.windows(JPEG.len()).any(|w| w == JPEG),
        "JPEG bytes must pass through unre-encoded"
    );
}

#[test]
fn local_absolute_path_and_base_dir_resolution() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("md-img");
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let png_path = dir.join("pic.png");
    std::fs::write(&png_path, tiny_png()).expect("write png");

    // Absolute path, no base_dir.
    let md = format!("![p]({})", png_path.display());
    let bytes = render(&md.replace('\\', "/"));
    assert!(raw(&bytes).contains("/Subtype /Image"));

    // Relative path + base_dir.
    let mut opts = Options::default();
    opts.base_dir = Some(dir.clone());
    let bytes = render_with("![p](pic.png)", &opts);
    assert!(raw(&bytes).contains("/Subtype /Image"));

    // Relative path without base_dir → typed error.
    let err = markdown_to_pdf("![p](pic.png)", &Options::default())
        .expect_err("relative path without base_dir must fail");
    assert!(matches!(err, pdf_core::error::Error::InvalidArgument(_)));
}

#[test]
fn remote_urls_are_rejected_not_fetched() {
    for src in [
        "https://example.com/a.png",
        "http://example.com/a.png",
        "ftp://example.com/a.png",
    ] {
        let err = markdown_to_pdf(&format!("![x]({src})"), &Options::default())
            .expect_err("remote URL must be rejected");
        assert!(matches!(err, pdf_core::error::Error::Unsupported(_)));
    }
}

#[test]
fn bad_image_data_yields_typed_error() {
    // Valid base64, not an image.
    let err = markdown_to_pdf("![x](data:image/png;base64,aGVsbG8=)", &Options::default())
        .expect_err("non-image bytes must fail");
    assert!(matches!(err, pdf_core::error::Error::InvalidArgument(_)));

    // Broken base64.
    let err = markdown_to_pdf("![x](data:image/png;base64,@@@@)", &Options::default())
        .expect_err("bad base64 must fail");
    assert!(matches!(err, pdf_core::error::Error::InvalidArgument(_)));
}

#[test]
fn wide_image_scales_down_to_content_width() {
    // 2000 px wide → must scale to the ~451 pt content width (A4 - margins).
    let samples = vec![128u8; 2000 * 2 * 3];
    let png = Pixmap::new(2000, 2, Colorspace::Rgb, false, samples)
        .to_png_bytes()
        .expect("png");
    let uri = format!("data:image/png;base64,{}", base64_encode(&png));
    let bytes = render(&format!("![wide]({uri})"));
    let raw = raw(&bytes);
    // The placement `cm` matrix must use a width ≤ the content width, not 2000.
    assert!(!raw.contains("2000 0 0"), "image must not render at 2000pt");
    assert!(raw.contains("/Im0 Do"));
}
