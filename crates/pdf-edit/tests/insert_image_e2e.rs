//! M4a image insertion — `INSERT-IMAGE-*` (PRD §8.8).
//!
//! Reparse oracle: insert an image → full save → reopen → the M2 interpreter's
//! image inventory lists it with the right CTM, and the registered XObject dict
//! carries the expected `/Filter` / `/ColorSpace`.

mod common;

use common::{blank_page, first_xobject_dict, open, page_content_bytes, page_images, save_reopen};

use pdf_core::geom::Rect;
use pdf_edit::{insert_image_jpeg, insert_image_rgb};

/// Builds a minimal **structurally valid** JPEG: SOI, an APP0/JFIF segment, a
/// baseline SOF0 frame declaring `width × height × components`, and EOI. It is
/// not decodable (no scan data) but exercises the header parse + `/DCTDecode`
/// passthrough, which is all `insert_image_jpeg` reads (the bytes are stored
/// verbatim, never decoded by us).
fn synthetic_jpeg(width: u16, height: u16, components: u8) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8]; // SOI
                                  // APP0 / JFIF
    v.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
    v.extend_from_slice(b"JFIF\0");
    v.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    // SOF0 baseline frame: len, precision=8, height, width, components, then
    // per-component (id, sampling, qtable) triples.
    let seg_len = 8 + 3 * components as usize;
    v.extend_from_slice(&[0xFF, 0xC0]);
    v.extend_from_slice(&(seg_len as u16).to_be_bytes());
    v.push(8); // precision
    v.extend_from_slice(&height.to_be_bytes());
    v.extend_from_slice(&width.to_be_bytes());
    v.push(components);
    for c in 0..components {
        v.extend_from_slice(&[c + 1, 0x11, 0x00]);
    }
    v.extend_from_slice(&[0xFF, 0xD9]); // EOI
    v
}

// === INSERT-IMAGE-* =======================================================

/// `INSERT-IMAGE-001`: JPEG bytes → image XObject with `/Filter /DCTDecode`
/// (passthrough, no re-encode → stored length == input length).
#[test]
fn insert_image_001_jpeg_dctdecode() {
    let jpeg = synthetic_jpeg(64, 48, 3);
    let doc = open(&blank_page(612, 792));
    insert_image_jpeg(&doc, 0, Rect::new(100.0, 100.0, 300.0, 250.0), &jpeg).unwrap();
    let re = save_reopen(&doc);

    let d = first_xobject_dict(&re, 0);
    assert!(
        d.get(&pdf_core::Name::new("Filter"))
            .and_then(pdf_core::Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"DCTDecode"),
        "expected /DCTDecode filter"
    );
    assert_eq!(
        d.get(&pdf_core::Name::new("Width"))
            .and_then(pdf_core::Object::as_i64),
        Some(64)
    );
    assert_eq!(
        d.get(&pdf_core::Name::new("Height"))
            .and_then(pdf_core::Object::as_i64),
        Some(48)
    );
    assert!(
        d.get(&pdf_core::Name::new("ColorSpace"))
            .and_then(pdf_core::Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"DeviceRGB"),
        "3-component JPEG → DeviceRGB"
    );
}

/// `INSERT-IMAGE-002`: the image is placed with a `cm` matrix mapping the unit
/// square to the rect; on reopen the interpreter lists it with the right CTM
/// (scale == rect size; translate == lower-left in user space).
#[test]
fn insert_image_002_placement_ctm() {
    let jpeg = synthetic_jpeg(10, 10, 1);
    let doc = open(&blank_page(612, 792));
    // Top-left rect (100,100)-(300,250) → user space y flips on a 792-high page.
    insert_image_jpeg(&doc, 0, Rect::new(100.0, 100.0, 300.0, 250.0), &jpeg).unwrap();
    let re = save_reopen(&doc);

    let images = page_images(&re, 0);
    assert_eq!(images.len(), 1, "expected exactly one placed image");
    let ctm = images[0].ctm;
    // Width 200, height 150.
    assert!((ctm.a - 200.0).abs() < 1e-3, "scale-x {}", ctm.a);
    assert!((ctm.d - 150.0).abs() < 1e-3, "scale-y {}", ctm.d);
    // Lower-left x == 100; lower-left y == 792 - 250 = 542.
    assert!((ctm.e - 100.0).abs() < 1e-3, "tx {}", ctm.e);
    assert!((ctm.f - 542.0).abs() < 1e-3, "ty {}", ctm.f);
}

/// `INSERT-IMAGE-003`: the XObject is registered under `/Resources /XObject` and
/// the content emits `Do`.
#[test]
fn insert_image_003_xobject_and_do() {
    let jpeg = synthetic_jpeg(8, 8, 3);
    let doc = open(&blank_page(612, 792));
    let name = insert_image_jpeg(&doc, 0, Rect::new(0.0, 0.0, 100.0, 100.0), &jpeg).unwrap();
    let re = save_reopen(&doc);
    let content = String::from_utf8_lossy(&page_content_bytes(&re, 0)).to_string();
    assert!(content.contains(" Do"), "no Do operator in {content}");
    assert!(content.contains(" cm"), "no cm operator in {content}");
    assert!(
        content.contains(&format!("/{name} Do")),
        "resource name {name} not referenced"
    );
}

/// `INSERT-IMAGE-004`: raw RGB pixels → `/FlateDecode` XObject with
/// `/DeviceRGB`, `/BitsPerComponent 8`, correct dimensions.
#[test]
fn insert_image_004_raw_rgb() {
    let (w, h) = (4u32, 3u32);
    let pixels = vec![0x80u8; (w * h * 3) as usize];
    let doc = open(&blank_page(612, 792));
    insert_image_rgb(&doc, 0, Rect::new(10.0, 10.0, 50.0, 40.0), w, h, &pixels).unwrap();
    let re = save_reopen(&doc);

    let d = first_xobject_dict(&re, 0);
    assert!(
        d.get(&pdf_core::Name::new("Filter"))
            .and_then(pdf_core::Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"FlateDecode"),
        "expected /FlateDecode"
    );
    assert!(d
        .get(&pdf_core::Name::new("ColorSpace"))
        .and_then(pdf_core::Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"DeviceRGB"));
    assert_eq!(
        d.get(&pdf_core::Name::new("BitsPerComponent"))
            .and_then(pdf_core::Object::as_i64),
        Some(8)
    );
    assert_eq!(
        d.get(&pdf_core::Name::new("Width"))
            .and_then(pdf_core::Object::as_i64),
        Some(4)
    );
}

/// `INSERT-IMAGE-005`: non-JPEG bytes (and a wrong-length RGB buffer) are
/// rejected with a typed error and never panic.
#[test]
fn insert_image_005_bad_input_rejected() {
    let doc = open(&blank_page(612, 792));
    let not_jpeg = vec![0u8; 20];
    assert!(
        insert_image_jpeg(&doc, 0, Rect::new(0.0, 0.0, 10.0, 10.0), &not_jpeg).is_err(),
        "non-JPEG should be rejected"
    );
    // RGB buffer with the wrong length.
    assert!(
        insert_image_rgb(&doc, 0, Rect::new(0.0, 0.0, 10.0, 10.0), 4, 4, &[0u8; 10]).is_err(),
        "short RGB buffer should be rejected"
    );
}
