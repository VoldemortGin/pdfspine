//! `image_profile` — raster header-profile probe (PyMuPDF `image_profile` /
//! `Tools.image_profile`, PRD §8.10).
//!
//! Validates the fitz-shaped profile (dimensions, component count, colorspace
//! name, extension token, 8-bit depth, identity orientation matrix) over the
//! checked-in JPEG assets, plus the `None` contract for empty / unrecognized
//! input.

use pdf_image::imagedoc::image_profile;

#[test]
fn profile_gray_jpeg() {
    let gray = include_bytes!("assets/gray.jpg");
    let prof = image_profile(gray).expect("gray jpeg profiles");
    assert_eq!(prof.ext, "jpeg");
    assert_eq!(prof.colorspace, 1);
    assert_eq!(prof.cs_name, "DeviceGray");
    assert_eq!(prof.bpc, 8);
    assert_eq!(prof.orientation, 0);
    assert_eq!(prof.matrix, (1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    assert!(prof.width > 0 && prof.height > 0);
}

#[test]
fn profile_rgb_jpeg() {
    let rgb = include_bytes!("assets/progressive_rgb.jpg");
    let prof = image_profile(rgb).expect("rgb jpeg profiles");
    assert_eq!(prof.ext, "jpeg");
    assert_eq!(prof.colorspace, 3);
    assert_eq!(prof.cs_name, "DeviceRGB");
}

#[test]
fn profile_cmyk_jpeg() {
    let cmyk = include_bytes!("assets/cmyk.jpg");
    let prof = image_profile(cmyk).expect("cmyk jpeg profiles");
    assert_eq!(prof.ext, "jpeg");
    assert_eq!(prof.colorspace, 4);
    assert_eq!(prof.cs_name, "DeviceCMYK");
}

#[test]
fn profile_rejects_non_image() {
    assert!(image_profile(b"").is_none());
    assert!(image_profile(b"abc").is_none());
    assert!(image_profile(b"not an image at all 1234").is_none());
}
