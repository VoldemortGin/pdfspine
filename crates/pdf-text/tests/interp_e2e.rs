//! `INTERP-E2E-*` — assemble a self-built 1-page PDF and assert the positioned
//! glyphs' Unicode sequence + approximate positions through the full
//! `interpret_page` path (decode `/Contents`, resolve `/Resources`).

mod common;

use common::*;
use pdf_core::Object;

/// A font with width 500 for all WinAnsi codes (resource `F1`).
fn font_w500() -> Object {
    let widths: Vec<i64> = (0..95).map(|_| 500).collect();
    winansi_type1_with_metrics("Helvetica", 32, &widths, 718, -207)
}

// === INTERP-E2E-001: two words on two lines → seq + positions =============

#[test]
fn e2e_001_two_words_two_lines() {
    // Line 1 at y=700: "Hi". Line 2 (T* with leading 14) at y=686: "Yo".
    let content: &[u8] = b"BT /F1 12 Tf 14 TL 1 0 0 1 72 700 Tm (Hi) Tj T* (Yo) Tj ET";
    let (doc, page) = PageDoc::new()
        .font("F1", font_w500())
        .content(content)
        .open();
    let res = pdf_text::interpret_page(&doc, &page);

    assert_eq!(glyph_text(&res), "HiYo");
    assert_eq!(res.glyphs.len(), 4);

    // Line 1 baseline y = 700; line 2 baseline y = 700 - 14 = 686.
    assert_origin(&res.glyphs[0], 72.0, 700.0, 1e-9); // H
    assert_origin(&res.glyphs[1], 78.0, 700.0, 1e-9); // i (advance 6 = 500/1000*12)
    assert_origin(&res.glyphs[2], 72.0, 686.0, 1e-9); // Y
    assert_origin(&res.glyphs[3], 78.0, 686.0, 1e-9); // o

    // All bboxes well-formed (x0<=x1, y0<=y1 after normalize) and finite.
    for g in &res.glyphs {
        let b = g.bbox;
        assert!(b.x0.is_finite() && b.x1.is_finite() && b.y0.is_finite() && b.y1.is_finite());
        assert!(b.width() > 0.0, "glyph {:?} has zero width bbox", g.unicode);
        assert!(
            b.height() > 0.0,
            "glyph {:?} has zero height bbox",
            g.unicode
        );
    }
}

// === INTERP-E2E-002: /Contents array + /Resources resolved end-to-end =====

#[test]
fn e2e_002_contents_array_resources() {
    // Two content streams + the font referenced *indirectly* in /Resources
    // (exercises both the /Contents array join and indirect /Font resolution).
    let s1: &[u8] = b"BT /F1 10 Tf 1 0 0 1 100 500 Tm (Foo)";
    let s2: &[u8] = b" Tj ET";
    let mut pd = PageDoc::new();
    let font_num = pd.add(font_w500());
    let (doc, page) = pd
        .font_ref("F1", font_num)
        .content_streams(&[s1, s2])
        .open();
    let res = pdf_text::interpret_page(&doc, &page);
    assert_eq!(glyph_text(&res), "Foo");
    assert_origin(&res.glyphs[0], 100.0, 500.0, 1e-9);
}
