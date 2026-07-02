//! Shared helpers: render Markdown, then read the result back through the
//! repo's own parsing stack (`pdf-api` open + text extraction).

#![allow(dead_code)]

use pdf_markdown::{markdown_to_pdf, Options};

/// Renders with default options, panicking on error (test convenience).
pub fn render(md: &str) -> Vec<u8> {
    markdown_to_pdf(md, &Options::default()).expect("markdown_to_pdf should succeed")
}

/// Renders with explicit options.
pub fn render_with(md: &str, opts: &Options) -> Vec<u8> {
    markdown_to_pdf(md, opts).expect("markdown_to_pdf should succeed")
}

/// Opens the emitted bytes through the public facade.
pub fn open(bytes: &[u8]) -> pdf_api::Document {
    pdf_api::Document::open_bytes(bytes.to_vec()).expect("emitted PDF should reopen")
}

/// Number of pages in the emitted PDF.
pub fn page_count(bytes: &[u8]) -> usize {
    open(bytes).page_count()
}

/// Plain-text extraction of one page.
pub fn page_text(bytes: &[u8], page: usize) -> String {
    let doc = open(bytes);
    let page = doc.load_page(page).expect("page should load");
    match pdf_api::get_text(&page, "text", None, None) {
        pdf_api::TextOutput::Text(s) => s,
        other => panic!("expected plain text output, got {other:?}"),
    }
}

/// Plain-text extraction of the whole document, in page order.
pub fn full_text(bytes: &[u8]) -> String {
    let doc = open(bytes);
    let mut out = String::new();
    for i in 0..doc.page_count() {
        let page = doc.load_page(i).expect("page should load");
        if let pdf_api::TextOutput::Text(s) = pdf_api::get_text(&page, "text", None, None) {
            out.push_str(&s);
        }
    }
    out
}

/// The raw file bytes as a lossy string — content streams are written without
/// deflation, so operator-level assertions can grep this directly.
pub fn raw(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Asserts `haystack` contains `needles` in order (each found after the last).
pub fn assert_in_order(haystack: &str, needles: &[&str]) {
    let mut at = 0;
    for needle in needles {
        match haystack[at..].find(needle) {
            Some(i) => at += i + needle.len(),
            None => panic!("expected {needle:?} (in order) in:\n{haystack}"),
        }
    }
}
