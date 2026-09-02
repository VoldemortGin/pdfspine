//! `WIDTHS-012..015` — a Type3 font's `/Widths` live in its own glyph space
//! (ISO 32000-1 §9.6.5): each advance `(w, 0)` is mapped through the
//! `/FontMatrix` linear part into text space and the x component is stored
//! ×1000, so [`FontMapper::width`] keeps its 1000-unit contract for every
//! simple font. A missing / malformed matrix means the standard
//! `[0.001 0 0 0.001 0 0]` (the pre-fix behaviour); `/MissingWidth` is mapped
//! the same way.

mod common;

use common::*;
use pdf_core::{DocumentStore, Name, Object};
use pdf_fonts::FontMapper;

/// Builds a [`FontMapper`] for the font object `num` in `doc`.
fn mapper_for(doc: &DocumentStore, num: u32) -> FontMapper {
    let obj = doc.get_object(num, 0).expect("font object");
    let dict = obj.as_dict().expect("font is a dict").clone();
    FontMapper::from_dict(&dict, doc)
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
        (actual - expected).abs() <= 0.5,
        "{what}: expected ≈ {expected}, got {actual}"
    );
}

fn reals(vals: &[f64]) -> Object {
    Object::Array(vals.iter().copied().map(Object::Real).collect())
}

fn ints(vals: &[i64]) -> Object {
    Object::Array(vals.iter().copied().map(Object::Integer).collect())
}

/// A Type3 font covering codes `'a'..` (real glyph names via `/Differences`,
/// one shared CharProc) with the given `/Widths` values and optional
/// `/FontMatrix` (`None` → key absent) plus extra top-level entries.
fn type3(
    widths: &[f64],
    font_matrix: Option<Object>,
    extra: impl IntoIterator<Item = (&'static str, Object)>,
) -> Object {
    let mut charprocs = dict([]);
    let mut diffs = vec![Object::Integer(97)];
    for c in b'a'..=b'z' {
        let gname = (c as char).to_string();
        charprocs.insert(Name::new(&gname), raw_stream([], b"40 0 d0"));
        diffs.push(name_obj(&gname));
    }
    let mut d = dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type3")),
        ("FontBBox", ints(&[0, -20, 80, 70])),
        ("CharProcs", Object::Dictionary(charprocs)),
        (
            "Encoding",
            Object::Dictionary(dict([
                ("Type", name_obj("Encoding")),
                ("Differences", Object::Array(diffs)),
            ])),
        ),
        ("FirstChar", Object::Integer(97)),
        ("LastChar", Object::Integer(97 + widths.len() as i64 - 1)),
        ("Widths", reals(widths)),
        ("Resources", Object::Dictionary(dict([]))),
    ]);
    if let Some(m) = font_matrix {
        d.insert(Name::new("FontMatrix"), m);
    }
    for (k, v) in extra {
        d.insert(Name::new(k), v);
    }
    Object::Dictionary(d)
}

const PDFTEX: [f64; 6] = [0.01204, 0.0, 0.0, 0.01204, 0.0, 0.0];
const STANDARD: [f64; 6] = [0.001, 0.0, 0.0, 0.001, 0.0, 0.0];

// WIDTHS-012: the pdfTeX bitmap-font pattern — `FontMatrix 0.01204` with
// `/Widths` in that glyph space (Helvetica 'e' 556 → 46.18). The advance must
// come back in 1000-unit text space (≈ 556), not 46 (12× too narrow).
#[test]
fn widths_012_type3_fontmatrix_scales_widths_into_text_space() {
    // a=46.18 (556), b=46.18 (556), c=41.53 (500), d=46.18, e=46.18 (556).
    let (_d, m) = build(type3(
        &[46.18, 46.18, 41.53, 46.18, 46.18],
        Some(reals(&PDFTEX)),
        [],
    ));
    approx(m.width(u32::from(b'e')), 556.0, "e");
    approx(m.width(u32::from(b'c')), 500.0, "c");
    // Outside `/Widths` and no descriptor → MissingWidth 0 (unchanged).
    assert_eq!(m.width(u32::from(b'z')), 0.0);
}

// WIDTHS-013: the standard matrix leaves `/Widths` as-is; a missing or
// malformed (`/FontMatrix` absent / 5 entries / non-numeric) matrix is read
// as the standard one — the pre-fix behaviour.
#[test]
fn widths_013_type3_standard_or_malformed_fontmatrix_keeps_widths() {
    let (_d, std) = build(type3(&[556.0, 556.0, 500.0], Some(reals(&STANDARD)), []));
    assert_eq!(std.width(u32::from(b'a')), 556.0);
    assert_eq!(std.width(u32::from(b'c')), 500.0);

    let (_d, absent) = build(type3(&[556.0, 556.0, 500.0], None, []));
    assert_eq!(absent.width(u32::from(b'a')), 556.0);

    let (_d, short) = build(type3(
        &[556.0, 556.0, 500.0],
        Some(reals(&[0.01204, 0.0, 0.0, 0.01204, 0.0])),
        [],
    ));
    assert_eq!(short.width(u32::from(b'a')), 556.0);

    let (_d, junk) = build(type3(
        &[556.0, 556.0, 500.0],
        Some(Object::Array(vec![
            name_obj("x"),
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(0.01204),
            Object::Integer(0),
            Object::Integer(0),
        ])),
        [],
    ));
    assert_eq!(junk.width(u32::from(b'a')), 556.0);
}

// WIDTHS-014: a skewed (non-diagonal) matrix `[0.01 0 0.002 0.01 0 0]` — the
// advance `(w, 0)` only feels `a`: `w·a·1000` (the `c` skew multiplies y = 0).
#[test]
fn widths_014_type3_skewed_fontmatrix_uses_only_a_for_advances() {
    let (_d, m) = build(type3(
        &[50.0, 20.0],
        Some(reals(&[0.01, 0.0, 0.002, 0.01, 0.0, 0.0])),
        [],
    ));
    approx(m.width(u32::from(b'a')), 500.0, "a");
    approx(m.width(u32::from(b'b')), 200.0, "b");
}

// WIDTHS-015: a Type3 with a `/FontDescriptor` — its `/MissingWidth` is in the
// same glyph space and is mapped identically (41.53 → ≈ 500 for codes outside
// `/Widths`).
#[test]
fn widths_015_type3_missing_width_is_scaled_too() {
    let descriptor = Object::Dictionary(dict([
        ("Type", name_obj("FontDescriptor")),
        ("FontName", name_obj("T3")),
        ("Flags", Object::Integer(4)),
        ("FontBBox", ints(&[0, -20, 80, 70])),
        ("ItalicAngle", Object::Integer(0)),
        ("Ascent", Object::Integer(70)),
        ("Descent", Object::Integer(-20)),
        ("StemV", Object::Integer(0)),
        ("MissingWidth", Object::Real(41.53)),
    ]));
    let (_d, m) = build(type3(
        &[46.18],
        Some(reals(&PDFTEX)),
        [("FontDescriptor", descriptor)],
    ));
    approx(m.width(u32::from(b'a')), 556.0, "a");
    approx(m.width(u32::from(b'z')), 500.0, "z (MissingWidth)");
}
