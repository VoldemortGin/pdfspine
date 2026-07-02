//! MD-2 facade wiring — `pdf_api::markdown_to_pdf` / `MarkdownOptions`
//! (pdfspine-original extension, PRD-NEXT §9). Thin-wrapper checks only: the
//! layout engine itself is covered in depth by `crates/pdf-markdown/tests`.
//! Every emitted PDF is read back through the same `pdf-api` facade.

use pdf_api::{markdown_to_pdf, Document, MarkdownOptions, TextOutput};

const SANS: &[u8] = include_bytes!("../../pdf-fonts/fonts/liberation/LiberationSans-Regular.ttf");

/// Full plain-text extraction of `bytes`, in page order, via the facade.
fn full_text(bytes: &[u8]) -> String {
    let doc = Document::open_bytes(bytes.to_vec()).expect("emitted PDF should reopen");
    let mut out = String::new();
    for i in 0..doc.page_count() {
        let page = doc.load_page(i).expect("page should load");
        if let TextOutput::Text(s) = pdf_api::get_text(&page, "text", None, None) {
            out.push_str(&s);
        }
    }
    out
}

#[test]
fn facade_renders_and_round_trips_through_the_facade() {
    let md = "# Title\n\nBody paragraph.\n\n- alpha\n- beta\n\n|H1|H2|\n|--|--|\n|c1|c2|\n";
    let bytes = markdown_to_pdf(md, &MarkdownOptions::new()).expect("render should succeed");
    let doc = Document::open_bytes(bytes.clone()).expect("emitted PDF should reopen");
    assert_eq!(doc.page_count(), 1);
    // Default geometry is A4 (595.32 × 841.92 pt).
    let page = doc.load_page(0).expect("page should load");
    let rect = page.rect();
    assert!(
        (rect.width() - 595.32).abs() < 0.01,
        "A4 width, got {rect:?}"
    );
    assert!(
        (rect.height() - 841.92).abs() < 0.01,
        "A4 height, got {rect:?}"
    );
    let text = full_text(&bytes);
    for needle in ["Title", "Body paragraph.", "alpha", "beta", "H1", "c2"] {
        assert!(text.contains(needle), "missing {needle:?} in {text:?}");
    }
}

#[test]
fn facade_options_pass_through_geometry_and_fonts() {
    let mut opts = MarkdownOptions::new();
    opts.page_width = 400.0;
    opts.page_height = 500.0;
    opts.margin_top = 36.0;
    opts.margin_right = 36.0;
    opts.margin_bottom = 36.0;
    opts.margin_left = 36.0;
    opts.body_font_size = 9.0;
    opts.font = Some(SANS.to_vec());
    let bytes = markdown_to_pdf("# H\n\nCustom geometry.", &opts).expect("render should succeed");
    let doc = Document::open_bytes(bytes.clone()).expect("emitted PDF should reopen");
    let page = doc.load_page(0).expect("page should load");
    let rect = page.rect();
    assert!(
        (rect.width() - 400.0).abs() < 0.01,
        "custom width, got {rect:?}"
    );
    assert!(
        (rect.height() - 500.0).abs() < 0.01,
        "custom height, got {rect:?}"
    );
    assert!(full_text(&bytes).contains("Custom geometry."));
    // The user TTF must arrive at the embedder (Type0/Identity-H program).
    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.contains("/Type0") && raw.contains("/FontFile2"));
}

#[test]
fn facade_maps_bad_input_to_typed_unsupported_errors() {
    // Unusable geometry (margins eat the page) — InvalidArgument upstream.
    let mut opts = MarkdownOptions::new();
    opts.margin_left = 400.0;
    opts.margin_right = 400.0;
    let err = markdown_to_pdf("x", &opts).expect_err("bad geometry must fail");
    assert_eq!(err.kind(), "unsupported", "got {err}");

    // Unparseable font program — Unsupported upstream.
    let mut opts = MarkdownOptions::new();
    opts.font = Some(b"not a font".to_vec());
    let err = markdown_to_pdf("x", &opts).expect_err("bad font must fail");
    assert_eq!(err.kind(), "unsupported", "got {err}");
}
