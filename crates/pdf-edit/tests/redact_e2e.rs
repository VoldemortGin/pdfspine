//! `REDACT-*` — destructive multi-surface redaction (PRD §8.8, P0 security).
//!
//! The acceptance gate (`REDACT-SECURITY-001`) runs over the **fully
//! decompressed** corpus (every stream + objstm expanded via `decompress_corpus`)
//! AND over `get_text()` — a compressed-only grep is a false pass and is
//! forbidden.

mod common;

use common::{
    dct_image_page, decompress_corpus, first_image_pixels, form_secret_doc, open, qpdf_check,
    rgb_image_page, save_bytes, save_full_deflate_bytes, text_secret_doc,
};

use pdf_core::error::Error;
use pdf_core::geom::Rect;
use pdf_edit::{add_redact_annot, annot_count, apply_redactions};
use pdf_text::interpret_page;

/// The plain extracted text of page 0 (concatenated glyph unicode).
fn page_text(doc: &pdf_core::DocumentStore) -> String {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page");
    interpret_page(doc, &page)
        .glyphs
        .iter()
        .map(|g| g.unicode.as_str())
        .collect()
}

// === REDACT-SECURITY ======================================================

#[test]
fn redact_security_001_secret_absent_everywhere_after_full_save() {
    // Build a page: visible "PUBLIC" then secret "TOPSECRET" on the same line.
    let (bytes, secret_rect) = text_secret_doc("PUBLIC ", "TOPSECRET");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1, "one redaction applied");

    // Full save, then DECOMPRESS every stream + objstm and grep.
    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    assert!(
        !contains(&corpus, b"TOPSECRET"),
        "secret must appear NOWHERE in the decompressed corpus"
    );
    // get_text of the reopened doc must not contain it either.
    let re = open(&out);
    let text = page_text(&re);
    assert!(
        !text.contains("TOPSECRET"),
        "get_text must not contain secret"
    );
    // Surrounding non-redacted text intact and unshifted.
    assert!(
        text.contains("PUBLIC"),
        "survivors must remain: got {text:?}"
    );
    // The "PUBLIC" survivor keeps its original x-origin (unshifted).
    let leaf = pdf_core::pagetree::page_refs(&re)[0];
    let page = pdf_core::pagetree::page_dict(&re, leaf).unwrap();
    let g = interpret_page(&re, &page).glyphs;
    let first = g.iter().find(|gl| gl.unicode == "P").expect("P glyph");
    assert!(
        (first.origin.x - 72.0).abs() < 0.5,
        "first survivor unshifted at x≈72, got {}",
        first.origin.x
    );
}

#[test]
fn redact_security_002_decompression_is_what_catches_it() {
    // With deflate=1 the compressed bytes wouldn't literally contain the secret
    // even WITHOUT redaction — proving the gate must decompress. Here we redact
    // and assert the DECOMPRESSED corpus is clean (the real gate).
    let (bytes, secret_rect) = text_secret_doc("HEADER ", "CLASSIFIED");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();

    let out = save_full_deflate_bytes(&doc);
    // The compressed file very likely does not contain the literal secret even
    // pre-redaction (deflate scrambles it) — so a compressed grep is meaningless.
    // The decompressed corpus is the sound check:
    let corpus = decompress_corpus(&out);
    assert!(!contains(&corpus, b"CLASSIFIED"));
    let re = open(&out);
    assert!(!page_text(&re).contains("CLASSIFIED"));
    assert!(page_text(&re).contains("HEADER"));
}

// === REDACT-TEXT ==========================================================

#[test]
fn redact_text_001_partial_line_drops_only_intersecting_glyphs() {
    let (bytes, secret_rect) = text_secret_doc("KEEP ", "DROP");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();

    let out = save_bytes(&doc);
    let re = open(&out);
    let text = page_text(&re);
    assert!(text.contains("KEEP"), "survivors kept: {text:?}");
    assert!(!text.contains("DROP"), "redacted glyphs gone: {text:?}");
    let corpus = decompress_corpus(&out);
    assert!(!contains(&corpus, b"DROP"));
}

#[test]
fn redact_text_002_multiple_rects() {
    // Two separate redaction rects, each over a distinct secret token.
    let char_w = 12.0 * 0.6;
    let x_a = 72.0;
    let x_b = x_a + "AAA SECRETONE BBB ".len() as f64 * char_w;
    let body = format!("BT /F1 12 Tf 1 0 0 1 {x_a} 700 Tm (AAA SECRETONE BBB SECRETTWO) Tj ET")
        .into_bytes();
    let bytes = common::simple_text_page(body);
    let doc = open(&bytes);
    // Rect over "SECRETONE": user x≈[72+4*7.2 .. 72+13*7.2].
    let one_x0 = x_a + 4.0 * char_w - 1.0;
    let one_x1 = x_a + 13.0 * char_w + 1.0;
    add_redact_annot(&doc, 0, Rect::new(one_x0, 82.0, one_x1, 96.0), None, None).unwrap();
    // Rect over "SECRETTWO".
    let two_x1 = x_b + "SECRETTWO".len() as f64 * char_w + 1.0;
    add_redact_annot(
        &doc,
        0,
        Rect::new(x_b - 1.0, 82.0, two_x1, 96.0),
        None,
        None,
    )
    .unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 2);
    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    assert!(!contains(&corpus, b"SECRETONE"));
    assert!(!contains(&corpus, b"SECRETTWO"));
    let text = page_text(&open(&out));
    assert!(text.contains("AAA"));
    assert!(text.contains("BBB"));
}

#[test]
fn redact_text_003_form_xobject_glyph_removed() {
    // Glyph drawn via a Form XObject under the rect must be gone from saved bytes.
    let (bytes, secret_rect) = form_secret_doc("VISIBLE ", "FORMSECRET");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();

    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    assert!(
        !contains(&corpus, b"FORMSECRET"),
        "secret drawn via a form must be removed from the form stream too"
    );
    let text = page_text(&open(&out));
    assert!(!text.contains("FORMSECRET"));
    assert!(text.contains("VISIBLE"));
}

#[test]
fn redact_text_004_count_and_full_preservation() {
    // A redaction rect that overlaps nothing preserves all text.
    let (bytes, _) = text_secret_doc("ALPHA ", "BETA");
    let doc = open(&bytes);
    // A rect far from the text (bottom of page).
    add_redact_annot(&doc, 0, Rect::new(10.0, 700.0, 60.0, 750.0), None, None).unwrap();
    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);
    let text = page_text(&open(&save_bytes(&doc)));
    assert!(text.contains("ALPHA"));
    assert!(text.contains("BETA"));
}

// === REDACT-IMAGE =========================================================

#[test]
fn redact_image_001_fully_covered_image_removed() {
    // Place a 4×4 red image at top-left (100,100) size 50×50; redact a rect that
    // fully covers it.
    let bytes = rgb_image_page(4, 4, (255, 0, 0), 100.0, 100.0, 50.0, 50.0);
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, Rect::new(90.0, 90.0, 160.0, 160.0), None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();

    let out = save_bytes(&doc);
    let re = open(&out);
    // The image XObject `Do` is gone from page content (no image inventory).
    let leaf = pdf_core::pagetree::page_refs(&re)[0];
    let page = pdf_core::pagetree::page_dict(&re, leaf).unwrap();
    let imgs = interpret_page(&re, &page).images;
    assert!(imgs.is_empty(), "fully-covered image must be removed");
}

#[test]
fn redact_image_002_raw_rgb_partial_pixels_zeroed() {
    // Image 8×8 solid white (255,255,255), placed at top-left (100,100) 80×80.
    // Redact the LEFT HALF only → left columns zeroed (black), right kept white.
    let bytes = rgb_image_page(8, 8, (255, 255, 255), 100.0, 100.0, 80.0, 80.0);
    let doc = open(&bytes);
    // Cover x [100,140] (left half), full vertical extent of the image.
    add_redact_annot(&doc, 0, Rect::new(100.0, 100.0, 140.0, 180.0), None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();

    let out = save_bytes(&doc);
    let re = open(&out);
    let (w, h, n, px) = first_image_pixels(&re);
    assert_eq!((w, h, n), (8, 8, 3));
    // Left columns (cols 0..~4) zeroed; right columns (cols ~5..8) still white.
    let pixel = |row: usize, col: usize, ch: usize| px[(row * w + col) * n + ch];
    assert_eq!(pixel(0, 0, 0), 0, "left column must be zeroed");
    assert_eq!(pixel(4, 1, 1), 0, "left column must be zeroed");
    assert_eq!(pixel(0, 7, 0), 255, "right column must be preserved white");
    assert_eq!(pixel(7, 7, 2), 255, "right column must be preserved white");
}

#[test]
fn redact_image_003_undecodable_image_fails_closed() {
    // A DCT (JPEG) image partially covered cannot be pixel-edited → fail closed.
    let bytes = dct_image_page(16, 16, 100.0, 100.0, 80.0, 80.0);
    let doc = open(&bytes);
    // Partial coverage (left half) so it isn't simply removed.
    add_redact_annot(&doc, 0, Rect::new(100.0, 100.0, 140.0, 180.0), None, None).unwrap();
    let err = apply_redactions(&doc, 0).unwrap_err();
    assert!(
        matches!(err, Error::Redaction(_)),
        "undecodable covered image must fail closed, got {err:?}"
    );
    assert_eq!(err.kind(), "redaction");
}

// === REDACT-COVER =========================================================

#[test]
fn redact_cover_001_fill_box_drawn() {
    let (bytes, secret_rect) = text_secret_doc("PRE ", "HIDE");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();

    let out = save_bytes(&doc);
    let re = open(&out);
    // The cover box is a filled drawing over the redaction region.
    let drawings = pdf_edit::get_drawings(&re, 0);
    assert!(
        drawings.iter().any(|d| d.fill.is_some()),
        "a filled cover box must be drawn"
    );
}

#[test]
fn redact_cover_002_redact_annots_removed() {
    let (bytes, secret_rect) = text_secret_doc("X ", "GONE");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    assert_eq!(annot_count(&doc, 0), 1);
    apply_redactions(&doc, 0).unwrap();
    // No /Redact annots after apply (here it was the only annot).
    assert_eq!(annot_count(&doc, 0), 0);
    let re = open(&save_bytes(&doc));
    assert_eq!(annot_count(&re, 0), 0, "redact annots removed on reopen");
}

// === REDACT-INCR ==========================================================

#[test]
fn redact_incr_001_incremental_rejected_after_redaction() {
    let (bytes, secret_rect) = text_secret_doc("A ", "SEC");
    let doc = open(&bytes);
    assert!(doc.can_save_incrementally(), "clean parse pre-redaction");
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();
    assert!(
        !doc.can_save_incrementally(),
        "redaction taints incremental save"
    );
    let err = doc
        .save_incremental(&pdf_core::SaveOptions::default())
        .unwrap_err();
    assert!(matches!(err, Error::IncrementalRequiresCleanParse));
}

#[test]
fn redact_incr_002_upgrade_to_full_save() {
    let (bytes, secret_rect) = text_secret_doc("B ", "SECRETUP");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();
    // OnRepaired::Upgrade → a full rewrite (secret absent).
    let out = doc
        .save_incremental(
            &pdf_core::SaveOptions::default()
                .with_on_repaired(pdf_core::writer::OnRepaired::Upgrade),
        )
        .unwrap();
    let corpus = decompress_corpus(&out);
    assert!(
        !contains(&corpus, b"SECRETUP"),
        "upgraded full save scrubs secret"
    );
}

// === REDACT-PROP ==========================================================

#[test]
fn redact_prop_001_no_annots_is_noop() {
    let (bytes, _) = text_secret_doc("ONLY ", "TEXT");
    let doc = open(&bytes);
    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 0, "no redact annots → no-op");
    assert!(doc.can_save_incrementally(), "no-op must not taint the doc");
    let text = page_text(&open(&save_bytes(&doc)));
    assert!(text.contains("ONLY"));
    assert!(text.contains("TEXT"));
}

#[test]
fn redact_prop_002_empty_region_preserves_all() {
    let (bytes, _) = text_secret_doc("FULL ", "INTACT");
    let doc = open(&bytes);
    // A zero-area / non-overlapping rect.
    add_redact_annot(&doc, 0, Rect::new(500.0, 500.0, 500.0, 500.0), None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();
    let text = page_text(&open(&save_bytes(&doc)));
    assert!(text.contains("FULL"));
    assert!(text.contains("INTACT"));
}

#[test]
fn redact_prop_003_qpdf_clean() {
    let (bytes, secret_rect) = text_secret_doc("KEEPME ", "REMOVEME");
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, secret_rect, None, None).unwrap();
    apply_redactions(&doc, 0).unwrap();
    let out = save_full_deflate_bytes(&doc);
    if let Some(ok) = qpdf_check(&out) {
        assert!(ok, "redacted save must pass qpdf --check");
    }
}

/// Whether `haystack` contains `needle` (byte search).
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
