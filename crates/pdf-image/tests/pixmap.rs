//! `PIXMAP-*` — `Pixmap` construction, save/tobytes, pixel access, alpha,
//! colorspace (PRD §8.10 / §3.3).

use pdf_image::codecs::{ColorSpaceHint, DecodedImage};
use pdf_image::pixmap::{Colorspace, Pixmap};

// --- PIXMAP-NEW-001: from raw RGB samples → fields ------------------------

#[test]
fn pixmap_new_001_rgb_fields() {
    let samples: Vec<u8> = (0..(4 * 3 * 3)).map(|i| i as u8).collect(); // 4x3 RGB
    let pm = Pixmap::new(4, 3, Colorspace::Rgb, false, samples.clone());
    assert_eq!(pm.width, 4);
    assert_eq!(pm.height, 3);
    assert_eq!(pm.n, 3);
    assert!(!pm.alpha);
    assert_eq!(pm.stride, 12);
    assert_eq!(pm.colorspace, Colorspace::Rgb);
    assert_eq!(pm.samples(), &samples[..]);
}

// --- PIXMAP-NEW-002: alpha bumps n + stride -------------------------------

#[test]
fn pixmap_new_002_alpha_geometry() {
    let pm = Pixmap::new(2, 2, Colorspace::Gray, true, vec![0u8; 2 * 2 * 2]);
    assert_eq!(pm.n, 2);
    assert_eq!(pm.stride, 4);
    assert!(pm.alpha);
}

// --- PIXMAP-NEW-003: try_new rejects a wrong-length buffer ----------------

#[test]
fn pixmap_new_003_try_new_length_check() {
    let ok = Pixmap::try_new(2, 2, Colorspace::Rgb, false, vec![0u8; 12]);
    assert!(ok.is_ok());
    let bad = Pixmap::try_new(2, 2, Colorspace::Rgb, false, vec![0u8; 11]);
    assert!(bad.is_err());
}

// --- PIXMAP-BLANK-001: blank ctor fills + sizes ---------------------------

#[test]
fn pixmap_blank_001_fill() {
    let pm = Pixmap::blank(3, 2, Colorspace::Rgb, false, 7).unwrap();
    assert_eq!(pm.samples().len(), 3 * 2 * 3);
    assert!(pm.samples().iter().all(|&b| b == 7));
    assert!(Pixmap::blank(0, 5, Colorspace::Gray, false, 0).is_err());
}

// --- PIXMAP-DECODED-001: from a DecodedImage (8-bit RGB) -------------------

#[test]
fn pixmap_decoded_001_rgb8() {
    let data: Vec<u8> = (0..(2 * 2 * 3)).map(|i| i as u8).collect();
    let img = DecodedImage::new(2, 2, 3, 8, ColorSpaceHint::Rgb, data.clone());
    let pm = Pixmap::from_decoded(&img).unwrap();
    assert_eq!(pm.colorspace, Colorspace::Rgb);
    assert_eq!(pm.n, 3);
    assert_eq!(pm.samples(), &data[..]);
}

// --- PIXMAP-DECODED-002: 1-bit gray upscales to 0/255 ---------------------

#[test]
fn pixmap_decoded_002_bilevel_upscale() {
    // 8x1 1-bit row: 0b1010_1010 = pixels 1,0,1,0,1,0,1,0
    let img = DecodedImage::new(8, 1, 1, 1, ColorSpaceHint::Gray, vec![0b1010_1010]);
    let pm = Pixmap::from_decoded(&img).unwrap();
    assert_eq!(pm.colorspace, Colorspace::Gray);
    assert_eq!(pm.samples(), &[255, 0, 255, 0, 255, 0, 255, 0]);
}

// --- PIXMAP-DECODED-003: 16-bit takes the high byte -----------------------

#[test]
fn pixmap_decoded_003_sixteen_bit() {
    // 2 gray pixels, big-endian 16-bit: 0x1234, 0xABCD → high bytes 0x12, 0xAB
    let img = DecodedImage::new(
        2,
        1,
        1,
        16,
        ColorSpaceHint::Gray,
        vec![0x12, 0x34, 0xAB, 0xCD],
    );
    let pm = Pixmap::from_decoded(&img).unwrap();
    assert_eq!(pm.samples(), &[0x12, 0xAB]);
}

// --- PIXMAP-SAVE-001: save_png round-trips through the `image` decoder -----

#[test]
fn pixmap_save_001_png_roundtrip() {
    let (w, h) = (5u32, 4u32);
    let mut samples = Vec::new();
    for y in 0..h {
        for x in 0..w {
            samples.push((x * 40) as u8);
            samples.push((y * 50) as u8);
            samples.push(99);
        }
    }
    let pm = Pixmap::new(w, h, Colorspace::Rgb, false, samples.clone());
    let png = pm.to_png_bytes().unwrap();
    let reopened = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
    assert_eq!(reopened.width(), w);
    assert_eq!(reopened.height(), h);
    assert_eq!(reopened.to_rgb8().into_raw(), samples);
}

// --- PIXMAP-SAVE-002: gray + alpha PNG round-trip -------------------------

#[test]
fn pixmap_save_002_gray_alpha_png() {
    let samples = vec![10, 255, 20, 128, 30, 64, 40, 0]; // 4 GA pixels
    let pm = Pixmap::new(2, 2, Colorspace::Gray, true, samples.clone());
    let png = pm.to_png_bytes().unwrap();
    let reopened = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
    assert_eq!(reopened.to_luma_alpha8().into_raw(), samples);
}

// --- PIXMAP-TOBYTES-001: tobytes("png") == to_png_bytes; pam carries alpha -

#[test]
fn pixmap_tobytes_001_formats() {
    let pm = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(pm.tobytes("png").unwrap(), pm.to_png_bytes().unwrap());
    let pam = pm.tobytes("pam").unwrap();
    assert!(pam.starts_with(b"P7\n"));
    assert!(pm.tobytes("bogus").is_err());
}

// --- PIXMAP-PIXEL-001: pixel get/set (COW preserves an older clone) --------

#[test]
fn pixmap_pixel_001_get_set() {
    let mut pm = Pixmap::new(2, 2, Colorspace::Rgb, false, vec![0u8; 12]);
    assert_eq!(pm.pixel(1, 1).unwrap(), vec![0, 0, 0]);
    pm.set_pixel(1, 1, &[9, 8, 7]).unwrap();
    assert_eq!(pm.pixel(1, 1).unwrap(), vec![9, 8, 7]);
    // out of range / wrong arity
    assert!(pm.set_pixel(2, 0, &[1, 2, 3]).is_err());
    assert!(pm.set_pixel(0, 0, &[1, 2]).is_err());
    assert!(pm.pixel(5, 5).is_none());
}

// --- PIXMAP-COW-001: a mutation does not disturb an Arc clone -------------

#[test]
fn pixmap_cow_001_clone_isolation() {
    let pm = Pixmap::new(2, 1, Colorspace::Gray, false, vec![1, 2]);
    let view = pm.samples_arc(); // simulate a live buffer export
    let mut pm2 = pm.clone();
    pm2.clear(0xFF);
    // The earlier export is untouched (copy-on-write).
    assert_eq!(&view[..], &[1, 2]);
    assert_eq!(pm2.samples(), &[0xFF, 0xFF]);
}

// --- PIXMAP-ALPHA-001: set_alpha touches only the alpha lane --------------

#[test]
fn pixmap_alpha_001_set_alpha() {
    let mut pm = Pixmap::new(2, 1, Colorspace::Rgb, true, vec![1, 2, 3, 50, 4, 5, 6, 60]);
    pm.set_alpha(200);
    assert_eq!(pm.samples(), &[1, 2, 3, 200, 4, 5, 6, 200]);
    // No-op without an alpha channel.
    let mut opaque = Pixmap::new(1, 1, Colorspace::Rgb, false, vec![7, 8, 9]);
    opaque.set_alpha(0);
    assert_eq!(opaque.samples(), &[7, 8, 9]);
}

// --- PIXMAP-SMASK-001: attach a gray /SMask as alpha ----------------------

#[test]
fn pixmap_smask_001_attach() {
    let pm = Pixmap::new(2, 1, Colorspace::Rgb, false, vec![10, 20, 30, 40, 50, 60]);
    let masked = pm.with_smask_gray(&[100, 200], 2, 1).unwrap();
    assert!(masked.alpha);
    assert_eq!(masked.n, 4);
    assert_eq!(masked.samples(), &[10, 20, 30, 100, 40, 50, 60, 200]);
}

// --- PIXMAP-INVERT-001: invert_irect flips color, keeps alpha -------------

#[test]
fn pixmap_invert_001_irect() {
    let mut pm = Pixmap::new(2, 1, Colorspace::Gray, true, vec![0, 255, 10, 100]);
    pm.invert_irect(0, 0, 1, 1); // only the first pixel
    assert_eq!(pm.samples(), &[255, 255, 10, 100]);
}

// --- PIXMAP-CMYK-001: CMYK saves as RGB PNG -------------------------------

#[test]
fn pixmap_cmyk_001_png_rgb() {
    // Pure cyan (0,255,255,0)→? and pure black via K.
    let pm = Pixmap::new(
        2,
        1,
        Colorspace::Cmyk,
        false,
        vec![255, 0, 0, 0, 0, 0, 0, 255],
    );
    let png = pm.to_png_bytes().unwrap();
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
    let rgb = img.to_rgb8().into_raw();
    // cyan: C=255 → R=0; black: K=255 → all 0
    assert_eq!(&rgb[0..3], &[0, 255, 255]);
    assert_eq!(&rgb[3..6], &[0, 0, 0]);
}

// --- PIXMAP-COPY-001: copy is independent (copy-on-write) -----------------

#[test]
fn pixmap_copy_001_independent() {
    let mut a = Pixmap::new(2, 2, Colorspace::Rgb, false, vec![0u8; 12]);
    let mut b = a.copy();
    b.set_pixel(0, 0, &[1, 2, 3]).unwrap();
    // Mutating the copy leaves the original untouched.
    assert_eq!(a.pixel(0, 0).unwrap(), vec![0, 0, 0]);
    assert_eq!(b.pixel(0, 0).unwrap(), vec![1, 2, 3]);
    // And vice-versa.
    a.set_pixel(1, 1, &[9, 9, 9]).unwrap();
    assert_eq!(b.pixel(1, 1).unwrap(), vec![0, 0, 0]);
}

// --- PIXMAP-SETRECT-001: set_rect fills a region, returns count -----------

#[test]
fn pixmap_setrect_001_fill_region() {
    let mut pm = Pixmap::new(4, 4, Colorspace::Rgb, false, vec![0u8; 4 * 4 * 3]);
    let n = pm.set_rect(1, 1, 3, 3, &[10, 20, 30]);
    assert_eq!(n, 4); // a 2x2 region
    assert_eq!(pm.pixel(1, 1).unwrap(), vec![10, 20, 30]);
    assert_eq!(pm.pixel(2, 2).unwrap(), vec![10, 20, 30]);
    assert_eq!(pm.pixel(0, 0).unwrap(), vec![0, 0, 0]); // outside
                                                        // An empty / inverted rect writes nothing.
    assert_eq!(pm.set_rect(2, 2, 2, 2, &[1, 1, 1]), 0);
}

#[test]
fn pixmap_setrect_002_alpha_untouched() {
    let mut pm = Pixmap::new(2, 1, Colorspace::Rgb, true, vec![0, 0, 0, 50, 0, 0, 0, 60]);
    pm.set_rect(0, 0, 2, 1, &[7, 8, 9]);
    // Color set, alpha left as-is.
    assert_eq!(pm.pixel(0, 0).unwrap(), vec![7, 8, 9, 50]);
    assert_eq!(pm.pixel(1, 0).unwrap(), vec![7, 8, 9, 60]);
}

// --- PIXMAP-SHRINK-001: 2x2 box-average downscale -------------------------

#[test]
fn pixmap_shrink_001_halves_dimensions() {
    // 4x4 gray, top-left 2x2 = 0, rest = 200; one shrink → 2x2.
    let mut samples = vec![200u8; 16];
    for y in 0..2 {
        for x in 0..2 {
            samples[y * 4 + x] = 0;
        }
    }
    let mut pm = Pixmap::new(4, 4, Colorspace::Gray, false, samples);
    pm.shrink(1);
    assert_eq!(pm.width, 2);
    assert_eq!(pm.height, 2);
    // The top-left output pixel averages the four 0s → 0.
    assert_eq!(pm.pixel(0, 0).unwrap(), vec![0]);
    // The bottom-right output pixel averages four 200s → 200.
    assert_eq!(pm.pixel(1, 1).unwrap(), vec![200]);
}

#[test]
fn pixmap_shrink_002_factor_and_floor() {
    let mut pm = Pixmap::new(8, 8, Colorspace::Rgb, false, vec![100u8; 8 * 8 * 3]);
    pm.shrink(2); // 8 -> 4 -> 2
    assert_eq!((pm.width, pm.height), (2, 2));
    // factor 0 is a no-op.
    pm.shrink(0);
    assert_eq!((pm.width, pm.height), (2, 2));
    // Averaging a uniform field preserves the value.
    assert_eq!(pm.pixel(0, 0).unwrap(), vec![100, 100, 100]);
}
