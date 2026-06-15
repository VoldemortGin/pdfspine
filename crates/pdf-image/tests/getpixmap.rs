//! `PIXMAP-IMGONLY-*` + `EXTRACT-IMAGE-*` — `get_pixmap`/`extract_image` on PDF
//! pages: image-only-page classifier, pixel-equality, vector-page rejection,
//! undecodable-image typed error (PRD §3.3 / §8.10).

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use pdf_core::{DocumentStore, Limits};
use pdf_image::getpixmap::{classify_page, extract_image, page_pixmap, PageClass};
use pdf_image::pixmap::Colorspace;

/// Flate-compresses raw bytes (zlib).
fn flate(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// Builds a 1-page PDF whose single page draws one Flate-encoded RGB image
/// XObject of `w`×`h` with `samples` (image-only content). Returns the PDF bytes.
fn build_image_only_pdf(w: u32, h: u32, samples: &[u8], content: &str) -> Vec<u8> {
    let img_data = flate(samples);
    build_pdf_with_objects(w, h, content, |out, offsets| {
        // obj 4: image XObject
        offsets[4] = out.len();
        out.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode \
                 /Length {} >>\nstream\n",
                img_data.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&img_data);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    })
}

/// Generic 5-object (catalog, pages, page, contents, obj4) hand-written
/// classic-xref PDF builder. `emit_obj4` writes object 4 (an image or whatever);
/// the page references `/Im0 4 0 R` and uses `content` as its content stream.
fn build_pdf_with_objects(
    w: u32,
    h: u32,
    content: &str,
    emit_obj4: impl FnOnce(&mut Vec<u8>, &mut [usize; 5]),
) -> Vec<u8> {
    let _ = (w, h);
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = [0usize; 5];

    offsets[1] = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets[2] = out.len();
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    offsets[3] = out.len();
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
          /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>\nendobj\n",
    );

    emit_obj4(&mut out, &mut offsets);

    // obj 5: content stream
    let content_off = out.len();
    out.extend_from_slice(
        format!(
            "5 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len() + 1
        )
        .as_bytes(),
    );

    // xref
    let xref_pos = out.len();
    out.extend_from_slice(b"xref\n0 6\n");
    out.extend_from_slice(b"0000000000 65535 f \n");
    for &off in &offsets[1..5] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("{content_off:010} 00000 n \n").as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{xref_pos}\n%%EOF").as_bytes());
    out
}

/// Opens hand-built PDF bytes and returns its first page dict.
fn first_page_dict(pdf: &[u8]) -> (DocumentStore, pdf_core::Dict) {
    let doc = DocumentStore::from_bytes(pdf.to_vec(), Limits::unbounded_decode()).unwrap();
    let page_refs = pdf_core::pagetree::page_refs(&doc);
    let page = doc.resolve(page_refs[0]).unwrap();
    let dict = page.as_dict().cloned().unwrap();
    (doc, dict)
}

fn sample_rgb(w: u32, h: u32) -> Vec<u8> {
    let mut s = Vec::new();
    for y in 0..h {
        for x in 0..w {
            s.push((x * 17) as u8);
            s.push((y * 23) as u8);
            s.push(((x + y) * 5) as u8);
        }
    }
    s
}

// --- PIXMAP-IMGONLY-001: classify a single-image page as image-only --------

#[test]
fn pixmap_imgonly_001_classify_image_only() {
    let (w, h) = (8u32, 6u32);
    let samples = sample_rgb(w, h);
    let pdf = build_image_only_pdf(w, h, &samples, "q 200 0 0 200 0 0 cm /Im0 Do Q");
    let (doc, page) = first_page_dict(&pdf);
    match classify_page(&doc, &page) {
        PageClass::ImageOnly { refs } => assert_eq!(refs.len(), 1),
        PageClass::Vector => panic!("expected image-only"),
    }
}

// --- PIXMAP-IMGONLY-002: get_pixmap == decoder output (pixel-equality) -----

#[test]
fn pixmap_imgonly_002_pixel_equality() {
    let (w, h) = (8u32, 6u32);
    let samples = sample_rgb(w, h);
    let pdf = build_image_only_pdf(w, h, &samples, "q 200 0 0 200 0 0 cm /Im0 Do Q");
    let (doc, page) = first_page_dict(&pdf);
    let pix = page_pixmap(&doc, &page, 1.0, false).unwrap();
    assert_eq!(pix.width, w);
    assert_eq!(pix.height, h);
    assert_eq!(pix.colorspace, Colorspace::Rgb);
    assert!(!pix.alpha);
    // Pixel-equality with the source raster (Flate raw → DecodedImage → Pixmap).
    assert_eq!(pix.samples(), &samples[..]);
}

// --- PIXMAP-IMGONLY-003: vector page → PdfUnsupportedError -----------------

#[test]
fn pixmap_imgonly_003_vector_page_unsupported() {
    let (w, h) = (4u32, 4u32);
    let samples = sample_rgb(w, h);
    // Content has a path-paint op (re/f) ⇒ vector page.
    let pdf = build_image_only_pdf(w, h, &samples, "0 0 10 10 re f");
    let (doc, page) = first_page_dict(&pdf);
    assert_eq!(classify_page(&doc, &page), PageClass::Vector);
    let err = page_pixmap(&doc, &page, 1.0, false).unwrap_err();
    assert_eq!(err.kind(), "unsupported");
}

// --- PIXMAP-IMGONLY-004: text page (BT/Tj) → vector ------------------------

#[test]
fn pixmap_imgonly_004_text_page_vector() {
    let (w, h) = (4u32, 4u32);
    let samples = sample_rgb(w, h);
    let pdf = build_image_only_pdf(
        w,
        h,
        &samples,
        "q 200 0 0 200 0 0 cm /Im0 Do Q BT /F1 12 Tf (hi) Tj ET",
    );
    let (doc, page) = first_page_dict(&pdf);
    assert_eq!(classify_page(&doc, &page), PageClass::Vector);
}

// --- PIXMAP-IMGONLY-005: scale arg scales output dims ----------------------

#[test]
fn pixmap_imgonly_005_scale() {
    let (w, h) = (8u32, 6u32);
    let samples = sample_rgb(w, h);
    let pdf = build_image_only_pdf(w, h, &samples, "q 200 0 0 200 0 0 cm /Im0 Do Q");
    let (doc, page) = first_page_dict(&pdf);
    let pix = page_pixmap(&doc, &page, 2.0, false).unwrap();
    assert_eq!(pix.width, 16);
    assert_eq!(pix.height, 12);
}

// --- PIXMAP-IMGONLY-006: alpha=true adds an opaque alpha channel ----------

#[test]
fn pixmap_imgonly_006_alpha_opaque() {
    let (w, h) = (4u32, 4u32);
    let samples = sample_rgb(w, h);
    let pdf = build_image_only_pdf(w, h, &samples, "q 200 0 0 200 0 0 cm /Im0 Do Q");
    let (doc, page) = first_page_dict(&pdf);
    let pix = page_pixmap(&doc, &page, 1.0, true).unwrap();
    assert!(pix.alpha);
    assert_eq!(pix.n, 4);
    // every alpha byte opaque
    for px in pix.samples().chunks_exact(4) {
        assert_eq!(px[3], 255);
    }
}

// --- PIXMAP-IMGONLY-007: undecodable image-only page → typed error ---------
// (and text extraction stays independent — verified via get_text in pytest)

#[test]
fn pixmap_imgonly_007_undecodable_typed_error() {
    let (w, h) = (4u32, 4u32);
    // A page claiming a DCTDecode image whose payload is garbage (not a JPEG).
    let pdf = build_pdf_with_objects(w, h, "q 200 0 0 200 0 0 cm /Im0 Do Q", |out, offsets| {
        offsets[4] = out.len();
        let junk = b"not a real jpeg";
        out.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode \
                 /Length {} >>\nstream\n",
                junk.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(junk);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    });
    let (doc, page) = first_page_dict(&pdf);
    // It IS classified image-only (the Do targets an Image XObject)...
    assert!(matches!(
        classify_page(&doc, &page),
        PageClass::ImageOnly { .. }
    ));
    // ...but decoding the junk DCT payload yields a typed error (no panic).
    let err = page_pixmap(&doc, &page, 1.0, false).unwrap_err();
    assert!(matches!(err.kind(), "decode" | "unsupported"));
}

// --- EXTRACT-IMAGE-001: raw raster → PNG-encoded descriptor ---------------

#[test]
fn extract_image_001_raw_to_png() {
    let (w, h) = (8u32, 6u32);
    let samples = sample_rgb(w, h);
    let pdf = build_image_only_pdf(w, h, &samples, "q 200 0 0 200 0 0 cm /Im0 Do Q");
    let doc = DocumentStore::from_bytes(pdf, Limits::unbounded_decode()).unwrap();
    let ext = extract_image(&doc, 4).unwrap();
    assert_eq!(ext.ext, "png");
    assert_eq!(ext.width, w as i32);
    assert_eq!(ext.height, h as i32);
    assert_eq!(ext.bpc, 8);
    assert_eq!(ext.colorspace, "DeviceRGB");
    assert_eq!(ext.components, 3);
    // The PNG bytes reopen to the original raster.
    let img = image::load_from_memory_with_format(&ext.image, image::ImageFormat::Png).unwrap();
    assert_eq!(img.to_rgb8().into_raw(), samples);
}

// --- EXTRACT-IMAGE-002: DCT image → jpeg passthrough ----------------------

#[test]
fn extract_image_002_dct_passthrough() {
    use image::codecs::jpeg::JpegEncoder;
    use image::ExtendedColorType;

    let (w, h) = (8u32, 8u32);
    let raw = sample_rgb(w, h);
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode(&raw, w, h, ExtendedColorType::Rgb8)
        .unwrap();

    let pdf = build_pdf_with_objects(w, h, "q 200 0 0 200 0 0 cm /Im0 Do Q", |out, offsets| {
        offsets[4] = out.len();
        out.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode \
                 /Length {} >>\nstream\n",
                jpeg.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&jpeg);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    });
    let doc = DocumentStore::from_bytes(pdf, Limits::unbounded_decode()).unwrap();
    let ext = extract_image(&doc, 4).unwrap();
    assert_eq!(ext.ext, "jpeg");
    // Passthrough: the bytes are the original JPEG verbatim.
    assert_eq!(ext.image, jpeg);
}

// --- EXTRACT-IMAGE-003: non-image xref → InvalidArgument ------------------

#[test]
fn extract_image_003_non_image() {
    let (w, h) = (4u32, 4u32);
    let samples = sample_rgb(w, h);
    let pdf = build_image_only_pdf(w, h, &samples, "q 200 0 0 200 0 0 cm /Im0 Do Q");
    let doc = DocumentStore::from_bytes(pdf, Limits::unbounded_decode()).unwrap();
    // obj 5 is the content stream (not an image).
    let err = extract_image(&doc, 5).unwrap_err();
    assert_eq!(err.kind(), "invalid-argument");
}
