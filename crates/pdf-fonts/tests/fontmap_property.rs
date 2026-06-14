//! `FONTMAP-PROP-*` — property / never-panic invariants for the font mapper and
//! the shared CMap parser (PRD §8.5 defensive contract).

mod common;

use common::*;
use pdf_core::{DocumentStore, Object};
use pdf_fonts::cmap::CMap;
use pdf_fonts::{FontKind, FontMapper};
use proptest::prelude::*;

/// A simple Helvetica font with WinAnsi encoding + a small Widths array.
fn simple_doc() -> (DocumentStore, u32) {
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FirstChar", Object::Integer(32)),
        (
            "Widths",
            Object::Array((32..=126).map(|_| Object::Integer(500)).collect()),
        ),
    ])));
    (d.open(), font)
}

/// An Identity-H Type0 font.
fn type0_doc() -> (DocumentStore, u32) {
    let mut d = FontDoc::new();
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("X")),
        ("DW", Object::Integer(1000)),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("Encoding", name_obj("Identity-H")),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    (d.open(), font)
}

fn mapper(doc: &DocumentStore, num: u32) -> FontMapper {
    let obj = doc.get_object(num, 0).unwrap();
    FontMapper::from_dict(obj.as_dict().unwrap(), doc)
}

proptest! {
    // FONTMAP-PROP-001 + 002: iter_codes covers the whole input with no overlap
    // (lengths sum to input length) and never panics on arbitrary bytes.
    #[test]
    fn prop_itercodes_covers_input(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let (sdoc, snum) = simple_doc();
        let (tdoc, tnum) = type0_doc();
        for (doc, num) in [(&sdoc, snum), (&tdoc, tnum)] {
            let m = mapper(doc, num);
            let mut total = 0usize;
            for (_code, n) in m.iter_codes(&bytes) {
                prop_assert!(n >= 1);
                total += n as usize;
            }
            // Whole input consumed, no overlap, nothing dropped.
            prop_assert_eq!(total, bytes.len());
        }
    }

    // FONTMAP-PROP-003: to_unicode never panics; returns Option.
    #[test]
    fn prop_to_unicode_no_panic(code in any::<u32>()) {
        let (sdoc, snum) = simple_doc();
        let (tdoc, tnum) = type0_doc();
        let sm = mapper(&sdoc, snum);
        let tm = mapper(&tdoc, tnum);
        let _ = sm.to_unicode(code);
        let _ = tm.to_unicode(code);
    }

    // FONTMAP-PROP-004: width never panics, is finite and >= 0.
    #[test]
    fn prop_width_finite_nonneg(code in any::<u32>()) {
        let (sdoc, snum) = simple_doc();
        let (tdoc, tnum) = type0_doc();
        for m in [mapper(&sdoc, snum), mapper(&tdoc, tnum)] {
            let w = m.width(code);
            prop_assert!(w.is_finite());
            prop_assert!(w >= 0.0);
        }
    }

    // The shared CMap parser never panics on arbitrary bytes (used for both
    // ToUnicode and CID encodings, often on malformed real-world data).
    #[test]
    fn prop_cmap_parse_no_panic(bytes in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let mut no_use = |_: &[u8]| None;
        let cm = CMap::parse(&bytes, &mut no_use);
        // Querying it must also be panic-free.
        let _ = cm.to_unicode(0x41);
        let _ = cm.cid(0x41);
        let _ = cm.codespace();
    }

    // glyph_name_to_unicode never panics on arbitrary names.
    #[test]
    fn prop_glyph_name_no_panic(s in ".*") {
        let _ = pdf_fonts::glyphlist::glyph_name_to_unicode(&s);
    }
}

// A non-proptest smoke check that a totally empty / garbage font dict still
// yields a usable best-effort mapper (FontMapper::from_dict never fails).
#[test]
fn empty_font_dict_yields_best_effort_mapper() {
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([("Type", name_obj("Font"))])));
    let doc = d.open();
    let obj = doc.get_object(font, 0).unwrap();
    let m = FontMapper::from_dict(obj.as_dict().unwrap(), &doc);
    assert_eq!(m.kind(), FontKind::Simple);
    // ASCII still resolves via the Standard default.
    assert_eq!(
        m.to_unicode(0x41).map(|s| s.to_string()).as_deref(),
        Some("A")
    );
    assert_eq!(m.width(0x41), 0.0);
}
