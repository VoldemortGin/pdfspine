//! `OCR-*` — `pdf-ocr` integration tests (M8).
//!
//! Each test builds a **self-contained** single-page PDF in raw bytes (classic
//! xref) whose content stream shows known text via the embedded `ocrtest.ttf`
//! (a glyph subset of DejaVu Sans), renders it via [`pdf_render::render_page`],
//! and then exercises the public OCR surface: the [`pdf_ocr::OcrEngine`] trait
//! (Tesseract default), [`pdf_ocr::textpage_ocr`], and [`pdf_ocr::pdfocr_bytes`].
//!
//! Tesseract-dependent tests no-op skip when `tesseract` is absent.

use std::process::Command;
use std::sync::Arc;

use pdf_core::page::Page;
use pdf_core::{DocumentStore, Limits};
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_ocr::{textpage_ocr, OcrEngine, OcrOptions, TesseractCli};

// ============================================================================
// Minimal classic-xref PDF builder (mirrors crates/pdf-render/tests/render_page.rs).
// ============================================================================

/// A classic-xref PDF assembler from `(obj_num, body_bytes)` entries; obj 1 is
/// the catalog.
struct Pdf {
    objects: Vec<(u32, Vec<u8>)>,
}

impl Pdf {
    fn new() -> Self {
        Pdf {
            objects: Vec::new(),
        }
    }

    fn obj(mut self, num: u32, body: impl AsRef<[u8]>) -> Self {
        self.objects.push((num, body.as_ref().to_vec()));
        self
    }

    fn build(mut self) -> Vec<u8> {
        self.objects.sort_by_key(|(n, _)| *n);
        let max = self.objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = vec![0usize; (max + 1) as usize];
        for (num, body) in &self.objects {
            offsets[*num as usize] = out.len();
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_off = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", max + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for n in 1..=max {
            out.extend_from_slice(format!("{:010} 00000 n \n", offsets[n as usize]).as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                max + 1,
                xref_off
            )
            .as_bytes(),
        );
        out
    }
}

/// A stream object body: `<< dict /Length n >>\nstream\n…\nendstream`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(format!("<< {} /Length {} >>\nstream\n", dict, data.len()).as_bytes());
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

// ============================================================================
// A known-text page rendered via the embedded DejaVu subset font.
// ============================================================================

/// The 16KB DejaVu-Sans glyph subset shipped in the test fixtures.
const OCR_TTF: &[u8] = include_bytes!("fixtures/ocrtest.ttf");

/// A wide media box so the text is large and well-separated for OCR.
const MEDIA: &str = "[0 0 600 200]";
/// The media-box width in points (used to assert page-space coordinate mapping).
const MEDIA_W: f64 = 600.0;

/// The embedded simple TrueType font objects (resource `/F1`): object 10 is the
/// font dict (WinAnsiEncoding so A–Z map through the font cmap), 11 the
/// descriptor, 12 the FontFile2 (the raw `ocrtest.ttf`).
fn embedded_font_objs() -> Vec<(u32, Vec<u8>)> {
    // 95 flat advance widths (codes 32..=126). Exact widths only affect advance
    // spacing; a flat ~600/1000 em keeps glyphs from overlapping for OCR.
    let widths: String = (32..=126).map(|_| "600 ").collect();
    let font = format!(
        "<< /Type /Font /Subtype /TrueType /BaseFont /DejaVuSans \
         /FirstChar 32 /LastChar 126 /Widths [ {widths}] \
         /FontDescriptor 11 0 R /Encoding /WinAnsiEncoding >>"
    );
    let descriptor = b"<< /Type /FontDescriptor /FontName /DejaVuSans /Flags 32 \
         /FontBBox [-1021 -463 1793 1232] /ItalicAngle 0 /Ascent 928 \
         /Descent -236 /CapHeight 928 /StemV 80 /FontFile2 12 0 R >>"
        .to_vec();
    vec![
        (10, font.into_bytes()),
        (11, descriptor),
        (12, stream(&format!("/Length1 {}", OCR_TTF.len()), OCR_TTF)),
    ]
}

/// Builds a self-contained single-page PDF that shows `text` in the embedded
/// font, large and positioned near the top of a 600×200 media box.
fn build_text_pdf(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 50 Tf 1 0 0 1 40 80 Tm ({text}) Tj ET");
    let mut pdf = Pdf::new()
        .obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox {MEDIA} \
                 /Resources << /Font << /F1 10 0 R >> >> /Contents 4 0 R >>"
            )
            .into_bytes(),
        )
        .obj(4, stream("", content.as_bytes()));
    for (num, body) in embedded_font_objs() {
        pdf = pdf.obj(num, body);
    }
    pdf.build()
}

/// Opens raw PDF bytes and returns `(Arc<DocumentStore>, Page)` for page 0
/// (object 3, per the builder convention above).
fn open_page(bytes: Vec<u8>) -> (Arc<DocumentStore>, Page) {
    let doc = DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open pdf");
    let arc = Arc::new(doc);
    let refs = pdf_core::pagetree::page_refs(&arc);
    let page = Page::new(Arc::clone(&arc), 0, refs[0]);
    (arc, page)
}

/// Renders `text` to a Pixmap at `dpi` via the public renderer.
fn render_known_text_pixmap(text: &str, dpi: u32) -> Pixmap {
    let (doc, page) = open_page(build_text_pdf(text));
    let opts = pdf_render::RenderOptions {
        dpi: Some(dpi),
        colorspace: Colorspace::Rgb,
        alpha: false,
        ..pdf_render::RenderOptions::default()
    };
    pdf_render::render_page(&doc, &page, &opts).expect("render_page ok")
}

// ============================================================================
// Token-recall helper.
// ============================================================================

/// Case-insensitive recall of `expected` tokens over the recognized `words`:
/// the fraction of expected tokens that appear (case-insensitive) as some word.
fn token_recall(expected: &[&str], words: &[pdf_ocr::OcrWord]) -> f64 {
    let recovered = expected
        .iter()
        .filter(|exp| words.iter().any(|w| w.text.eq_ignore_ascii_case(exp)))
        .count();
    recovered as f64 / expected.len() as f64
}

// ============================================================================
// OCR-ENGINE: the engine recognizes the known tokens with high recall.
// ============================================================================

#[test]
fn ocr_engine_recognizes_known_tokens() {
    let engine = TesseractCli::new();
    if !engine.is_available() {
        return; // no-op skip when tesseract is absent
    }
    let pix = render_known_text_pixmap("HELLO OCR WORLD", 150);
    let words = engine.recognize(&pix, "eng", 150.0).expect("recognize ok");

    let expected = ["HELLO", "OCR", "WORLD"];
    let recall = token_recall(&expected, &words);
    assert!(
        recall >= 0.8,
        "token recall {recall} < 0.8; recognized = {:?}",
        words.iter().map(|w| &w.text).collect::<Vec<_>>()
    );

    let (w, h) = (pix.width as f64, pix.height as f64);
    for word in &words {
        let b = word.bbox;
        assert!(b.x1 > b.x0 && b.y1 > b.y0, "non-empty bbox: {b:?}");
        assert!(
            b.x0 >= 0.0 && b.x1 <= w && b.y0 >= 0.0 && b.y1 <= h,
            "bbox {b:?} inside pixmap {w}x{h}"
        );
    }
}

// ============================================================================
// OCR-TEXTPAGE: textpage_ocr yields an extractable TextPage in PAGE space.
// ============================================================================

#[test]
fn ocr_textpage_extracts_text() {
    let engine = TesseractCli::new();
    if !engine.is_available() {
        return;
    }
    let (_doc, page) = open_page(build_text_pdf("HELLO OCR WORLD"));
    let opts = OcrOptions {
        language: "eng".into(),
        dpi: 150,
        full: true,
    };
    let tp = textpage_ocr(&page, &engine, &opts).expect("textpage_ocr ok");

    let text = pdf_text::to_text(&tp, pdf_text::defaults::TEXT).to_lowercase();
    assert!(
        text.contains("hello") && text.contains("world"),
        "extracted text must contain hello/world, got {text:?}"
    );

    // Coordinates are mapped to PAGE space (points), not pixel space: the page
    // width is ~600pt (the media box), NOT 600 * 150/72 = 1250.
    assert!(
        (tp.width - MEDIA_W).abs() < 2.0,
        "TextPage width {} should be ~{MEDIA_W} (page points), not pixels",
        tp.width
    );
    // A top-of-page word sits well above the bottom (small y in y-down space).
    let block = tp.blocks.first().expect("at least one block");
    assert!(
        block.bbox.y0 < tp.height * 0.75,
        "top-of-page text y0 {} should be well under page height {}",
        block.bbox.y0,
        tp.height
    );
}

// ============================================================================
// OCR-SANDWICH: pdfocr_bytes produces a searchable sandwich PDF.
// ============================================================================

#[test]
fn ocr_sandwich_is_searchable() {
    let engine = TesseractCli::new();
    if !engine.is_available() {
        return;
    }
    let (arc, _page) = open_page(build_text_pdf("HELLO OCR WORLD"));
    let opts = OcrOptions {
        dpi: 150,
        ..OcrOptions::default()
    };
    let bytes = pdf_ocr::pdfocr_bytes(&arc, &engine, &opts).expect("pdfocr_bytes ok");

    // The image layer must be present (an /XObject image resource).
    assert!(
        find_subsequence(&bytes, b"/Image").is_some(),
        "sandwich must embed a page image XObject"
    );

    // Reopen the produced sandwich and extract its (invisible Tr-3) text layer.
    let reopened =
        DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("reopen sandwich");
    let arc2 = Arc::new(reopened);
    let refs = pdf_core::pagetree::page_refs(&arc2);
    assert!(!refs.is_empty(), "sandwich has at least one page");
    let page = Page::new(Arc::clone(&arc2), 0, refs[0]);

    let tp = pdf_text::build_textpage(&arc2, &page, &Limits::default());
    let text = pdf_text::to_text(&tp, pdf_text::defaults::TEXT).to_lowercase();
    assert!(
        text.contains("hello") || text.contains("world"),
        "sandwich text layer must be extractable, got {text:?}"
    );
}

/// `qpdf --check` accepts the sandwich PDF (skipped when qpdf is absent).
#[test]
fn ocr_sandwich_qpdf_clean() {
    let engine = TesseractCli::new();
    if !engine.is_available() {
        return;
    }
    if !qpdf_available() {
        return; // no-op skip when qpdf is absent
    }
    let (arc, _page) = open_page(build_text_pdf("HELLO OCR WORLD"));
    let opts = OcrOptions {
        dpi: 150,
        ..OcrOptions::default()
    };
    let bytes = pdf_ocr::pdfocr_bytes(&arc, &engine, &opts).expect("pdfocr_bytes ok");

    let mut path = std::env::temp_dir();
    path.push(format!("oxide_ocr_sandwich_{}.pdf", std::process::id()));
    std::fs::write(&path, &bytes).expect("write temp sandwich");

    let status = Command::new("qpdf")
        .arg("--check")
        .arg(&path)
        .status()
        .expect("run qpdf");
    let _ = std::fs::remove_file(&path);
    assert!(
        status.success(),
        "qpdf --check must accept the sandwich PDF"
    );
}

// ============================================================================
// OCR-ABSENT: a missing engine yields a typed "unsupported" error, no panic.
// ============================================================================

#[test]
fn ocr_absent_is_typed_error() {
    let engine = TesseractCli::new().with_binary("/nonexistent/tesseract-xyz");
    assert!(!engine.is_available(), "fake binary is not available");

    let pix = Pixmap::blank(16, 16, Colorspace::Rgb, false, 255).unwrap();
    let err = engine
        .recognize(&pix, "eng", 72.0)
        .expect_err("recognize must error when engine absent");
    assert_eq!(err.kind(), "unsupported");

    // textpage_ocr on a real page with the absent engine also errors as
    // "unsupported" (does not panic).
    let (_doc, page) = open_page(build_text_pdf("HELLO"));
    let err = textpage_ocr(&page, &engine, &OcrOptions::default())
        .expect_err("textpage_ocr must error when engine absent");
    assert_eq!(err.kind(), "unsupported");
}

// ============================================================================
// Small byte / process helpers.
// ============================================================================

/// Whether `qpdf` is on `PATH` (`qpdf --version` exits successfully).
fn qpdf_available() -> bool {
    Command::new("qpdf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The first index of `needle` within `haystack`, if any.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
