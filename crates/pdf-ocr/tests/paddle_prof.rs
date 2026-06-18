//! Manual profiling harness for the PaddleOCR pipeline (cold vs warm wall-clock
//! per page, on a representative multi-line page). `#[ignore]` so it never runs
//! in the normal suite. Run with:
//!   cargo test -p pdf-ocr --release --test paddle_prof -- --nocapture --ignored
#![cfg(feature = "paddle-ocr")]

use std::time::Instant;

use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_ocr::{OcrEngine, PaddleOcr};

fn load(path: &str) -> Pixmap {
    let img = image::open(path).expect("open").to_rgb8();
    let (w, h) = (img.width(), img.height());
    Pixmap::new(w, h, Colorspace::Rgb, false, img.into_raw())
}

#[test]
#[ignore]
fn prof_page() {
    // A representative multi-line page: stack several scan strips into one page.
    let strips: Vec<Pixmap> = (0..8)
        .map(|i| load(&format!("../../conformance/ocr/images/scan_{i:02}.png")))
        .collect();
    let w = strips.iter().map(|p| p.width).max().unwrap();
    let h: u32 = strips.iter().map(|p| p.height).sum();
    let mut buf = vec![255u8; (w * h * 3) as usize];
    let mut yoff = 0u32;
    for p in &strips {
        let s = p.samples();
        for y in 0..p.height {
            for x in 0..p.width {
                let src = (y as usize) * p.stride + (x as usize) * p.n as usize;
                let dst = (((yoff + y) * w + x) * 3) as usize;
                buf[dst] = s[src];
                buf[dst + 1] = s[src + 1];
                buf[dst + 2] = s[src + 2];
            }
        }
        yoff += p.height;
    }
    let page = Pixmap::new(w, h, Colorspace::Rgb, false, buf);
    eprintln!("page size = {w}x{h}");

    let engine = PaddleOcr::new().expect("engine");

    let t = Instant::now();
    let words = engine.recognize(&page, "ch", 150.0).expect("ocr");
    eprintln!(
        "COLD total = {} ms ({} words)",
        t.elapsed().as_millis(),
        words.len()
    );

    let t = Instant::now();
    let words = engine.recognize(&page, "ch", 150.0).expect("ocr");
    eprintln!(
        "WARM total = {} ms ({} words)",
        t.elapsed().as_millis(),
        words.len()
    );

    let t = Instant::now();
    let _ = engine.recognize(&page, "ch", 150.0).expect("ocr");
    eprintln!("WARM2 total = {} ms", t.elapsed().as_millis());
}
