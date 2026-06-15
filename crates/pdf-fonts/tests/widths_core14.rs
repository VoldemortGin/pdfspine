//! `WIDTHS-CORE14-*` — Core-14 standard advance widths wired into text
//! extraction (`FontMapper::width`): a base-14 simple font lacking a `/Widths`
//! array resolves each code → glyph name → standard AFM advance (PRD §6.5 #2 /
//! §8.5.2). `/Widths` stays authoritative; aliases (`Arial` → Helvetica) and
//! subset tags are honored; unknown variants fall back gracefully.

mod common;

use common::*;
use pdf_core::{DocumentStore, Object};
use pdf_fonts::FontMapper;

/// Builds a [`FontMapper`] for the font object `num` in `doc`.
fn mapper_for(doc: &DocumentStore, num: u32) -> FontMapper {
    let obj = doc.get_object(num, 0).expect("font object");
    let dict = obj.as_dict().expect("font is a dict").clone();
    FontMapper::from_dict(&dict, doc)
}

/// A simple Type1 font dict with `base_font` and no `/Widths`, returning the
/// built mapper.
fn base14_mapper(base_font: &str) -> (DocumentStore, FontMapper) {
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj(base_font)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    (doc, m)
}

// WIDTHS-CORE14-001: Helvetica with NO /Widths → standard advances
// (space=278, 'A'=667, 'i'=222, '0'=556).
#[test]
fn widths_core14_001_helvetica_anchors() {
    let (_doc, m) = base14_mapper("Helvetica");
    assert_eq!(m.width(0x20), 278.0); // space
    assert_eq!(m.width(0x41), 667.0); // A
    assert_eq!(m.width(0x69), 222.0); // i
    assert_eq!(m.width(0x6C), 222.0); // l
    assert_eq!(m.width(0x30), 556.0); // 0
}

// WIDTHS-CORE14-002: Times-Roman with NO /Widths → 'A'=722, space=250, '.'=250.
#[test]
fn widths_core14_002_times_anchors() {
    let (_doc, m) = base14_mapper("Times-Roman");
    assert_eq!(m.width(0x41), 722.0); // A
    assert_eq!(m.width(0x20), 250.0); // space
    assert_eq!(m.width(0x2E), 250.0); // .
}

// WIDTHS-CORE14-003: Courier is monospaced — every printable glyph is 600.
#[test]
fn widths_core14_003_courier_monospace() {
    let (_doc, m) = base14_mapper("Courier");
    for code in 0x20u32..=0x7E {
        assert_eq!(m.width(code), 600.0, "Courier U+{code:04X} should be 600");
    }
}

// WIDTHS-CORE14-004: an explicit /Widths array stays authoritative — the AFM
// advances do NOT override it even for a base-14 font.
#[test]
fn widths_core14_004_widths_override() {
    let mut d = FontDoc::new();
    // Helvetica 'A' would be 667 in AFM; /Widths forces 999 at code 65.
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("FirstChar", Object::Integer(65)),
        ("LastChar", Object::Integer(65)),
        ("Widths", Object::Array(vec![Object::Integer(999)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0x41), 999.0);
    // A code outside the /Widths range uses MissingWidth (0 here), NOT the AFM
    // advance — the presence of /Widths disables the Core-14 path entirely.
    assert_eq!(m.width(0x42), 0.0);
}

// WIDTHS-CORE14-005: the `Arial` alias maps to Helvetica metrics.
#[test]
fn widths_core14_005_arial_alias() {
    let (_doc, m) = base14_mapper("Arial");
    assert_eq!(m.width(0x20), 278.0); // space (Helvetica)
    assert_eq!(m.width(0x41), 667.0); // A (Helvetica)
    assert_eq!(m.width(0x69), 222.0); // i (Helvetica)
}

// WIDTHS-CORE14-006: bold/italic aliases — `Arial,Bold` → Helvetica-Bold
// ('A'=722), `TimesNewRoman,Bold` → Times-Bold ('A'=722).
#[test]
fn widths_core14_006_styled_aliases() {
    let (_doc, hb) = base14_mapper("Arial,Bold");
    assert_eq!(hb.width(0x41), 722.0); // Helvetica-Bold A

    let (_doc2, tb) = base14_mapper("TimesNewRoman,Bold");
    assert_eq!(tb.width(0x41), 722.0); // Times-Bold A
}

// WIDTHS-CORE14-007: a subset-tagged base-14 name (`ABCDEF+Helvetica`) still
// resolves to Helvetica metrics.
#[test]
fn widths_core14_007_subset_tag() {
    let (_doc, m) = base14_mapper("ABCDEF+Helvetica");
    assert_eq!(m.width(0x41), 667.0);
    assert_eq!(m.width(0x20), 278.0);
}

// WIDTHS-CORE14-008: Latin-1 glyphs resolve via the per-font overlay. WinAnsi
// code 0xC9 is 'Eacute' (667 in Helvetica), 0xE9 is 'eacute' (556).
#[test]
fn widths_core14_008_latin1_overlay() {
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0xC9), 667.0); // Eacute
    assert_eq!(m.width(0xE9), 556.0); // eacute
}

// WIDTHS-CORE14-009: /Differences remap a code to a different glyph name, and
// the AFM advance follows the *name*, not the original code. Map 0x80 → 'A'
// over a Helvetica base → 667.
#[test]
fn widths_core14_009_differences_follow_glyph_name() {
    let mut d = FontDoc::new();
    let enc = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Encoding")),
        ("BaseEncoding", name_obj("WinAnsiEncoding")),
        (
            "Differences",
            Object::Array(vec![Object::Integer(0x80), name_obj("A")]),
        ),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", rref(enc, 0)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0x80), 667.0); // 'A' advance via the remapped name
}

// WIDTHS-CORE14-010: robustness — a non-standard font name has no Core-14 path,
// so width falls back to MissingWidth (here 480) and never panics.
#[test]
fn widths_core14_010_non_standard_falls_back() {
    let mut d = FontDoc::new();
    let desc = d.add(Object::Dictionary(dict([
        ("Type", name_obj("FontDescriptor")),
        ("MissingWidth", Object::Integer(480)),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("CompletelyCustomFont")),
        ("FontDescriptor", rref(desc, 0)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0x41), 480.0);
}

// WIDTHS-CORE14-011: a base-14 glyph with no AFM metric (e.g. a code with no
// glyph name, or a name outside the font's tables) falls back to MissingWidth
// gracefully — no panic. Standard base maps few high codes; MissingWidth wins.
#[test]
fn widths_core14_011_unmapped_glyph_falls_back() {
    let mut d = FontDoc::new();
    let desc = d.add(Object::Dictionary(dict([
        ("Type", name_obj("FontDescriptor")),
        ("MissingWidth", Object::Integer(123)),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        // StandardEncoding leaves 0x00 unmapped (no glyph name) → MissingWidth.
        ("Encoding", name_obj("StandardEncoding")),
        ("FontDescriptor", rref(desc, 0)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0x00), 123.0); // no glyph name → MissingWidth fallback
    assert_eq!(m.width(0x41), 667.0); // but 'A' still resolves to AFM
}

// WIDTHS-CORE14-012: every base-14 mapper answers `width` for the full byte
// range without panicking and returns finite, non-negative values (property).
#[test]
fn widths_core14_012_property_no_panic_finite() {
    for base in [
        "Helvetica",
        "Helvetica-Bold",
        "Times-Roman",
        "Times-BoldItalic",
        "Courier",
        "Courier-BoldOblique",
        "Symbol",
        "ZapfDingbats",
        "Arial",
        "WeirdUnknownFont", // non-standard → falls back, still finite
    ] {
        let (_doc, m) = base14_mapper(base);
        for code in 0u32..=0xFF {
            let w = m.width(code);
            assert!(w.is_finite() && w >= 0.0, "{base} code {code} → {w}");
        }
    }
}

// WIDTHS-CORE14-013: the `core14_width` hook directly — exercises the glyph-name
// table (Helvetica), Courier monospace, and Symbol's flat default.
#[test]
fn widths_core14_013_hook_direct() {
    use pdf_fonts::widths::core14_width;
    assert_eq!(core14_width("Helvetica", "A"), Some(667.0));
    assert_eq!(core14_width("Helvetica", "space"), Some(278.0));
    assert_eq!(core14_width("Helvetica", "Aacute"), Some(667.0));
    assert_eq!(core14_width("Courier", "A"), Some(600.0));
    assert_eq!(core14_width("Courier-Bold", "m"), Some(600.0));
    // Symbol/ZapfDingbats: any glyph → flat default (no WinAnsi names).
    assert_eq!(core14_width("Symbol", "alpha"), Some(600.0));
    assert_eq!(core14_width("ZapfDingbats", "a1"), Some(788.0));
    // Unknown font key → None.
    assert_eq!(core14_width("NoSuchFont", "A"), None);
    // A glyph name the text font has no metric for → None (lets callers fall
    // back to MissingWidth).
    assert_eq!(core14_width("Helvetica", "nonexistentglyph"), None);
}
