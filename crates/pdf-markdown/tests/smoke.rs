//! `MD-SMOKE-*` — end-to-end smoke test (parse → layout → bytes → reopen).

use pdf_core::{DocumentStore, Limits};
use pdf_markdown::{markdown_to_pdf, Options};

#[test]
fn markdown_to_pdf_returns_openable_pdf_bytes() {
    let bytes = markdown_to_pdf("# Hello\n\nworld", &Options::default())
        .expect("markdown_to_pdf should succeed");

    assert!(!bytes.is_empty(), "output PDF must be non-empty");
    assert!(
        bytes.starts_with(b"%PDF"),
        "output must start with the %PDF signature"
    );

    // The bytes must reopen as a real document with at least one page.
    let doc = DocumentStore::from_bytes(bytes, Limits::default())
        .expect("emitted bytes should reopen as a PDF");
    assert!(
        pdf_core::pagetree::page_count(&doc) >= 1,
        "emitted PDF must have at least one page"
    );
}

#[test]
fn empty_markdown_still_yields_one_page() {
    let bytes = markdown_to_pdf("", &Options::default()).expect("empty input should succeed");
    let doc = DocumentStore::from_bytes(bytes, Limits::default()).expect("should reopen");
    assert_eq!(pdf_core::pagetree::page_count(&doc), 1);
}

#[test]
fn bad_geometry_is_a_typed_error() {
    let mut opts = Options::default();
    opts.margin_left = 400.0;
    opts.margin_right = 400.0; // A4 width is ~595 — no content area left
    let err = markdown_to_pdf("x", &opts).expect_err("should reject unusable margins");
    assert!(matches!(
        err,
        pdf_core::error::Error::InvalidArgument(_) | pdf_core::error::Error::Unsupported(_)
    ));
}
