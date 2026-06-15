//! Core-14 built-in standard advance-width table (`std_widths`).
//!
//! These cover the factual AFM `WX` metrics exposed via `standard_font_widths`,
//! `StandardWidths::advance` and `string_advance`. Catalog IDs are tagged in
//! comments. `standard_font_widths` takes the 14 canonical keys directly;
//! friendly `/BaseFont` aliases would go through `normalize_standard_font`.

use pdf_fonts::std_widths::{standard_font_widths, string_advance};

// WIDTHS-STD14-001: Helvetica anchors — space=278, 'A'=667, 'i'=222.
#[test]
fn widths_std14_001_helvetica_anchors() {
    let h = standard_font_widths("Helvetica").expect("Helvetica is a standard font");
    assert_eq!(h.advance(' '), 278.0);
    assert_eq!(h.advance('A'), 667.0);
    assert_eq!(h.advance('i'), 222.0);
}

// WIDTHS-STD14-002: Times-Roman anchors — space=250, 'A'=722, '.'=250.
#[test]
fn widths_std14_002_times_roman_anchors() {
    let t = standard_font_widths("Times-Roman").expect("Times-Roman is a standard font");
    assert_eq!(t.advance(' '), 250.0);
    assert_eq!(t.advance('A'), 722.0);
    assert_eq!(t.advance('.'), 250.0);
}

// WIDTHS-STD14-003: Courier is monospaced — every printable glyph = 600.
#[test]
fn widths_std14_003_courier_monospace() {
    let c = standard_font_widths("Courier").expect("Courier is a standard font");
    for code in 0x20u32..=0x7E {
        let ch = char::from_u32(code).unwrap();
        assert_eq!(
            c.advance(ch),
            600.0,
            "Courier {ch:?} (U+{code:04X}) should be 600"
        );
    }
    // All Courier variants share the monospaced metrics.
    for name in ["Courier-Bold", "Courier-Oblique", "Courier-BoldOblique"] {
        let v = standard_font_widths(name).unwrap();
        assert_eq!(v.advance('A'), 600.0);
        assert_eq!(v.default_width(), 600.0);
    }
}

// WIDTHS-STD14-004: string_advance("Helvetica","Hello",12.0) == hand-summed.
// H=722, e=556, l=222, l=222, o=556 → 2278 → *12/1000 = 27.336.
#[test]
fn widths_std14_004_string_advance_helvetica_hello() {
    let got = string_advance("Helvetica", "Hello", 12.0);
    let expected = (722.0 + 556.0 + 222.0 + 222.0 + 556.0) * 12.0 / 1000.0;
    assert_eq!(expected, 27.336);
    assert!(
        (got - expected).abs() < 1e-9,
        "got {got}, expected {expected}"
    );
}

// WIDTHS-STD14-005: unknown unicode char returns the default and never panics;
// unknown font name in string_advance returns a finite value.
#[test]
fn widths_std14_005_unknown_char_and_font_no_panic() {
    let h = standard_font_widths("Helvetica").unwrap();
    // CJK '中' is not in the WinAnsi range → default (space) width.
    assert_eq!(h.advance('\u{4e2d}'), h.default_width());
    assert_eq!(h.advance('\u{4e2d}'), 278.0);

    // Unknown font name → finite approximation, never panics.
    let v = string_advance("NoSuchFont", "abc", 10.0);
    assert!(v.is_finite());
    assert!(v > 0.0);
}

// WIDTHS-STD14-006: glyph-name advance lookup (the extraction path). ASCII names
// resolve via the WinAnsi reverse map; Latin-1 names via the per-font overlay;
// an unknown glyph name returns None (caller falls back).
#[test]
fn widths_std14_006_glyph_advance_by_name() {
    let h = standard_font_widths("Helvetica").unwrap();
    assert_eq!(h.glyph_advance("A"), Some(667.0));
    assert_eq!(h.glyph_advance("space"), Some(278.0));
    assert_eq!(h.glyph_advance("i"), Some(222.0));
    assert_eq!(h.glyph_advance("zero"), Some(556.0));
    // Latin-1 overlay names.
    assert_eq!(h.glyph_advance("Eacute"), Some(667.0));
    assert_eq!(h.glyph_advance("eacute"), Some(556.0));
    assert_eq!(h.glyph_advance("germandbls"), Some(611.0));
    // Unknown name → None, never panics.
    assert_eq!(h.glyph_advance("nosuchglyph"), None);

    // Times shares the lookup mechanism with different metrics.
    let t = standard_font_widths("Times-Roman").unwrap();
    assert_eq!(t.glyph_advance("A"), Some(722.0));
    assert_eq!(t.glyph_advance("a"), Some(444.0));
}
