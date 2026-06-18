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
use pdf_ocr::{OcrEngine, OcrWord, PaddleOcr};

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
            text: "oxide-pdf OCR test 2026",
            bbox: [42.0, 31.0, 504.0, 68.0],
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

/// Character-level similarity in `[0,1]`: `1 - levenshtein/max_len` over Unicode
/// scalar values (so CJK counts per character).
fn char_similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let (n, m) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let dist = prev[m];
    let max_len = n.max(m);
    1.0 - (dist as f64) / (max_len as f64)
}

/// The center of a reference bbox.
fn ref_center(b: &[f64; 4]) -> (f64, f64) {
    ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
}

/// The center of a recognized word's bbox.
fn word_center(w: &OcrWord) -> (f64, f64) {
    ((w.bbox.x0 + w.bbox.x1) / 2.0, (w.bbox.y0 + w.bbox.y1) / 2.0)
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

    for r in references() {
        // Find the output word whose text best matches this reference line.
        let best = words
            .iter()
            .map(|w| (char_similarity(&w.text, r.text), w))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let (sim, w) = best.unwrap_or_else(|| panic!("no words recognized at all"));
        assert!(
            sim >= 0.9,
            "line {:?}: best match {:?} similarity {:.3} < 0.9",
            r.text,
            w.text,
            sim
        );
        let (rcx, rcy) = ref_center(&r.bbox);
        let (wcx, wcy) = word_center(w);
        let dist = ((rcx - wcx).powi(2) + (rcy - wcy).powi(2)).sqrt();
        assert!(
            dist <= 15.0,
            "line {:?}: box center off by {:.1}px (>15): ref=({:.0},{:.0}) got=({:.0},{:.0})",
            r.text,
            dist,
            rcx,
            rcy,
            wcx,
            wcy
        );
    }
}
