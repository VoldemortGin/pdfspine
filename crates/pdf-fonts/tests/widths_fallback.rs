//! `WIDTHS-005..011` — the simple-font advance fallback chain when a font has
//! **no** `/Widths` array (PRD §8.5; MuPDF `pdf_load_simple_font` parity):
//!
//! 1. an embedded `/FontFile2` (TrueType) / `/FontFile3` (OpenType / bare CFF)
//!    program → its own advances (`hmtx` / CFF charstring width, ×1000/upem);
//! 2. a Core-14 name → the built-in AFM table, including the WinAnsi
//!    `0x80–0x9F` / StandardEncoding punctuation (`quoteright`, `endash`, …);
//! 3. a non-embedded, non-standard name → the descriptor `/Flags`-selected
//!    standard substitute (Courier / Times / Helvetica × bold / italic);
//! 4. only then `/MissingWidth` (default 0).
//!
//! A present-but-short `/Widths` array is **not** repaired: out-of-range codes
//! stay on `/MissingWidth`, as in PyMuPDF.

mod common;

use common::*;
use pdf_core::{DocumentStore, Name, Object};
use pdf_fonts::liberation::{liberation_face, LiberationFamily};
use pdf_fonts::FontMapper;

/// Builds a [`FontMapper`] for the font object `num` in `doc`.
fn mapper_for(doc: &DocumentStore, num: u32) -> FontMapper {
    let obj = doc.get_object(num, 0).expect("font object");
    let dict = obj.as_dict().expect("font is a dict").clone();
    FontMapper::from_dict(&dict, doc)
}

/// A `/FontDescriptor` dict with the given `/Flags` plus optional extras.
fn descriptor(
    name: &str,
    flags: i64,
    extra: impl IntoIterator<Item = (&'static str, Object)>,
) -> Object {
    let mut d = dict([
        ("Type", name_obj("FontDescriptor")),
        ("FontName", name_obj(name)),
        ("Flags", Object::Integer(flags)),
        ("ItalicAngle", Object::Integer(0)),
        ("Ascent", Object::Integer(750)),
        ("Descent", Object::Integer(-250)),
        ("StemV", Object::Integer(80)),
    ]);
    for (k, v) in extra {
        d.insert(Name::new(k), v);
    }
    Object::Dictionary(d)
}

/// A simple font (`subtype`) without `/Widths`, optional `/Encoding` name and
/// optional descriptor object.
fn nowidths_font(
    subtype: &str,
    base_font: &str,
    encoding: Option<&str>,
    descriptor: Option<Object>,
) -> Object {
    let mut d = dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj(subtype)),
        ("BaseFont", name_obj(base_font)),
    ]);
    if let Some(e) = encoding {
        d.insert(Name::new("Encoding"), name_obj(e));
    }
    if let Some(desc) = descriptor {
        d.insert(Name::new("FontDescriptor"), desc);
    }
    Object::Dictionary(d)
}

fn build(font: Object) -> (DocumentStore, FontMapper) {
    let mut d = FontDoc::new();
    let num = d.add(font);
    let doc = d.open();
    let m = mapper_for(&doc, num);
    (doc, m)
}

fn approx(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= 1.0,
        "{what}: got {actual}, expected ≈{expected}"
    );
}

// WIDTHS-005: Core-14 WinAnsi high punctuation (0x80–0x9F) resolves to the AFM
// advance instead of 0 — Times-Roman quoteright 333 / endash 500 / ellipsis
// 1000 / quotedblleft 444; Helvetica quotedblleft/right 333, bullet 350,
// emdash 1000, Euro 556; Helvetica-Bold quoteright 278 / quotedblleft 500.
#[test]
fn widths_005_core14_winansi_high_punctuation() {
    let (_d, times) = build(nowidths_font(
        "Type1",
        "Times-Roman",
        Some("WinAnsiEncoding"),
        None,
    ));
    assert_eq!(times.width(0x92), 333.0); // quoteright
    assert_eq!(times.width(0x96), 500.0); // endash
    assert_eq!(times.width(0x85), 1000.0); // ellipsis
    assert_eq!(times.width(0x93), 444.0); // quotedblleft

    let (_d, helv) = build(nowidths_font(
        "Type1",
        "Helvetica",
        Some("WinAnsiEncoding"),
        None,
    ));
    assert_eq!(helv.width(0x93), 333.0); // quotedblleft
    assert_eq!(helv.width(0x94), 333.0); // quotedblright
    assert_eq!(helv.width(0x95), 350.0); // bullet
    assert_eq!(helv.width(0x97), 1000.0); // emdash
    assert_eq!(helv.width(0x80), 556.0); // Euro

    let (_d, hb) = build(nowidths_font(
        "Type1",
        "Helvetica-Bold",
        Some("WinAnsiEncoding"),
        None,
    ));
    assert_eq!(hb.width(0x92), 278.0); // quoteright
    assert_eq!(hb.width(0x93), 500.0); // quotedblleft
}

// WIDTHS-006: StandardEncoding (the default when `/Encoding` is absent) maps
// 0x27 → quoteright and 0x60 → quoteleft — both must carry an advance
// (Helvetica 222, Times 333) — and the `fi`/`fl` ligatures at 0xAE/0xAF too.
#[test]
fn widths_006_core14_standard_encoding_quotes_and_ligatures() {
    let (_d, helv) = build(nowidths_font("Type1", "Helvetica", None, None));
    assert_eq!(helv.width(0x27), 222.0); // quoteright
    assert_eq!(helv.width(0x60), 222.0); // quoteleft
    assert_eq!(helv.width(0xAE), 500.0); // fi
    assert_eq!(helv.width(0xAF), 500.0); // fl

    let (_d, times) = build(nowidths_font("Type1", "Times-Roman", None, None));
    assert_eq!(times.width(0x27), 333.0);
    assert_eq!(times.width(0xAE), 556.0); // fi
}

// WIDTHS-007: a non-embedded, non-standard name without `/Widths` takes the
// `/Flags`-selected standard substitute (MuPDF's substitute-font logic):
// 32 (non-serif) → Helvetica e=556 r=333; 34 (serif) → Times e=444;
// 33 (fixed) → Courier 600; ForceBold (bit 19) or StemV ≥ 120 → Helvetica-Bold
// r=389; ItalicAngle ≠ 0 with serif → Times-Italic A=611; no descriptor at
// all → Helvetica.
#[test]
fn widths_007_flags_select_standard_substitute() {
    let calibri = |flags: i64, extra: Vec<(&'static str, Object)>| {
        nowidths_font(
            "TrueType",
            "ABCDEF+Calibri",
            Some("WinAnsiEncoding"),
            Some(descriptor("ABCDEF+Calibri", flags, extra)),
        )
    };
    let (_d, sans) = build(calibri(32, vec![]));
    assert_eq!(sans.width(u32::from(b'e')), 556.0);
    assert_eq!(sans.width(u32::from(b'r')), 333.0);

    let (_d, serif) = build(calibri(34, vec![]));
    assert_eq!(serif.width(u32::from(b'e')), 444.0);

    let (_d, fixed) = build(calibri(33, vec![]));
    assert_eq!(fixed.width(u32::from(b'e')), 600.0);
    assert_eq!(fixed.width(u32::from(b'W')), 600.0);

    let (_d, force_bold) = build(calibri(32 | (1 << 18), vec![]));
    assert_eq!(force_bold.width(u32::from(b'r')), 389.0);

    let (_d, stemv_bold) = build(calibri(32, vec![("StemV", Object::Integer(130))]));
    assert_eq!(stemv_bold.width(u32::from(b'r')), 389.0);

    let (_d, italic) = build(calibri(34, vec![("ItalicAngle", Object::Integer(-12))]));
    assert_eq!(italic.width(u32::from(b'A')), 611.0);

    let (_d, bare) = build(nowidths_font(
        "TrueType",
        "ABCDEF+Calibri",
        Some("WinAnsiEncoding"),
        None,
    ));
    assert_eq!(bare.width(u32::from(b'e')), 556.0);
}

/// A TrueType font with the given embedded program as `/FontFile2`, no
/// `/Widths`, descriptor `flags`, and optional `/Encoding` name.
fn embedded_tt(
    program: &[u8],
    base_font: &str,
    flags: i64,
    encoding: Option<&str>,
) -> (DocumentStore, FontMapper) {
    let mut d = FontDoc::new();
    let ff = d.add(raw_stream(
        [("Length1", Object::Integer(program.len() as i64))],
        program,
    ));
    let desc = descriptor(base_font, flags, vec![("FontFile2", rref(ff, 0))]);
    let num = d.add(nowidths_font("TrueType", base_font, encoding, Some(desc)));
    let doc = d.open();
    let m = mapper_for(&doc, num);
    (doc, m)
}

// WIDTHS-008: an embedded TrueType program (`/FontFile2`, Liberation Sans)
// without `/Widths` supplies its own `hmtx` advances, normalised to 1000/em —
// non-symbolic (WinAnsi → Unicode → (3,1) cmap) and symbolic (Flags 4, no
// `/Encoding`; code → (3,0)/(1,0) cmap, else the Standard name) alike:
// e ≈ 556, space ≈ 278, W ≈ 944 (Liberation Sans is Arial-metric).
#[test]
fn widths_008_embedded_truetype_hmtx() {
    let sans = liberation_face(LiberationFamily::Sans, false, false);
    let (_d, m) = embedded_tt(sans, "ABCDEF+Calibri", 32, Some("WinAnsiEncoding"));
    approx(m.width(u32::from(b'e')), 556.0, "non-symbolic e");
    approx(m.width(0x20), 278.0, "non-symbolic space");
    approx(m.width(u32::from(b'W')), 944.0, "non-symbolic W");

    let (_d, sym) = embedded_tt(sans, "ABCDEF+Calibri", 4, None);
    approx(sym.width(u32::from(b'e')), 556.0, "symbolic e");
    approx(sym.width(u32::from(b'W')), 944.0, "symbolic W");
}

// WIDTHS-009: the embedded program outranks the Core-14 name — a font named
// `Helvetica` whose `/FontFile2` is Liberation *Serif* advances like Times
// (e ≈ 444, not 556). An unparseable program falls back to the name (556).
#[test]
fn widths_009_embedded_program_outranks_core14_name() {
    let serif = liberation_face(LiberationFamily::Serif, false, false);
    let (_d, m) = embedded_tt(serif, "Helvetica", 34, Some("WinAnsiEncoding"));
    approx(m.width(u32::from(b'e')), 444.0, "Liberation Serif e");

    let (_d, junk) = embedded_tt(
        b"not a font program",
        "Helvetica",
        32,
        Some("WinAnsiEncoding"),
    );
    assert_eq!(junk.width(u32::from(b'e')), 556.0);
}

// WIDTHS-010: a present-but-short `/Widths` array is authoritative — codes past
// its end stay on `/MissingWidth` (0 when absent; 500 when declared), never on
// the substitute / Core-14 advance (PyMuPDF reads them as 0 too).
#[test]
fn widths_010_truncated_widths_not_repaired() {
    let truncated = |missing: Option<i64>| {
        let mut extra = vec![];
        if let Some(mw) = missing {
            extra.push(("MissingWidth", Object::Integer(mw)));
        }
        let mut d = dict([
            ("Type", name_obj("Font")),
            ("Subtype", name_obj("TrueType")),
            ("BaseFont", name_obj("Helvetica")),
            ("Encoding", name_obj("WinAnsiEncoding")),
            ("FirstChar", Object::Integer(32)),
            ("LastChar", Object::Integer(33)),
            (
                "Widths",
                Object::Array(vec![Object::Integer(278), Object::Integer(278)]),
            ),
        ]);
        d.insert(
            Name::new("FontDescriptor"),
            descriptor("Helvetica", 32, extra),
        );
        Object::Dictionary(d)
    };
    let (_d, zero) = build(truncated(None));
    assert_eq!(zero.width(0x20), 278.0);
    assert_eq!(zero.width(u32::from(b'e')), 0.0);

    let (_d, five) = build(truncated(Some(500)));
    assert_eq!(five.width(u32::from(b'e')), 500.0);
}

// WIDTHS-011: a Type3 font without `/Widths` gets no standard substitute (its
// glyph space is `/FontMatrix`, not 1000/em) — advances stay 0.
#[test]
fn widths_011_type3_without_widths_gets_no_substitute() {
    let (_d, t3) = build(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type3")),
        (
            "FontMatrix",
            Object::Array(vec![
                Object::Real(0.001),
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(0.001),
                Object::Integer(0),
                Object::Integer(0),
            ]),
        ),
        ("CharProcs", Object::Dictionary(dict([]))),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ])));
    assert_eq!(t3.width(u32::from(b'e')), 0.0);
}
