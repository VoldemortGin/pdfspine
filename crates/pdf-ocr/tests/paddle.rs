//! `PADDLE-*` — pure-Rust PaddleOCR (PP-OCRv4) engine acceptance test.
//!
//! Loads the offline fixture `tests/fixtures/ocr_sample.png` (720×300, three
//! mixed CJK+Latin lines) into a [`Pixmap`], runs [`PaddleOcr`], and asserts the
//! three reference lines from `ocr_sample_ref.json` are each recognized with high
//! character similarity and a box near the reference. Deterministic and offline
//! (models + dict are embedded in the crate).
//!
//! Gated on the `paddle-ocr` feature (the engine itself is feature-gated).
#![cfg(feature = "paddle-ocr")]

use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_ocr::{OcrEngine, PaddleOcr};

/// Decodes the fixture PNG into an RGB [`Pixmap`] (n=3, alpha=false), matching
/// what `render_for_ocr` would hand the engine in the real pipeline.
fn load_sample() -> Pixmap {
    let bytes = include_bytes!("fixtures/ocr_sample.png");
    let img = image::load_from_memory(bytes)
        .expect("decode sample png")
        .to_rgb8();
    let (w, h) = (img.width(), img.height());
    Pixmap::new(w, h, Colorspace::Rgb, false, img.into_raw())
}

/// One reference line: expected text + bbox `[x0,y0,x1,y1]`.
struct Ref {
    text: &'static str,
    bbox: [f64; 4],
}

fn references() -> Vec<Ref> {
    vec![
        Ref {
            text: "pdfspine OCR test 2026",
            bbox: [42.0, 29.0, 487.0, 71.0],
        },
        Ref {
            text: "纯Rust实现的PDF文字识别",
            bbox: [40.0, 116.0, 509.0, 155.0],
        },
        Ref {
            text: "PaddleOCR via tract",
            bbox: [42.0, 203.0, 443.0, 236.0],
        },
    ]
}

/// The vertical center of a reference bbox.
fn ref_cy(b: &[f64; 4]) -> f64 {
    (b[1] + b[3]) / 2.0
}

#[test]
fn paddle_recognizes_three_reference_lines() {
    let pix = load_sample();
    let engine = PaddleOcr::new().expect("build PaddleOcr");
    let words = engine.recognize(&pix, "ch", 72.0).expect("recognize ok");

    // Diagnostic dump (visible with `--nocapture`).
    eprintln!("recognized {} word(s):", words.len());
    for w in &words {
        eprintln!(
            "  text={:?} bbox=[{:.0},{:.0},{:.0},{:.0}] conf={:.1}",
            w.text, w.bbox.x0, w.bbox.y0, w.bbox.x1, w.bbox.y1, w.confidence
        );
    }
    assert!(!words.is_empty(), "no words recognized at all");

    // Recognition COMPLETENESS, not one-box-per-line: text detection may legitimately
    // split a line into several word/segment boxes (RapidOCR merges per-line via a more
    // aggressive dilation/unclip; ours may emit finer boxes — both are valid OCR output).
    // So assert each reference LINE's text appears in the whitespace-stripped concatenation
    // of all recognized words, plus a loose geometry sanity (some box sits in the line's band).
    let joined: String = words
        .iter()
        .flat_map(|w| w.text.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    for r in references() {
        let needle: String = r.text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            joined.contains(&needle),
            "line {:?} not found in recognized text {:?}",
            r.text,
            joined
        );
        let rcy = ref_cy(&r.bbox);
        let in_band = words
            .iter()
            .any(|w| ((w.bbox.y0 + w.bbox.y1) / 2.0 - rcy).abs() <= 15.0);
        assert!(
            in_band,
            "no recognized box in the vertical band of line {:?}",
            r.text
        );
    }
}
