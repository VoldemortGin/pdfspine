//! `ENCODING-008/009/010`, `WIDTHS-*`, `CID-*`, `ITERCODES-*`, `FONTMAP-*` —
//! the [`FontMapper`] built from self-constructed font dicts + a `DocumentStore`
//! (PRD §8.5; catalog M2a).

mod common;

use common::*;
use pdf_core::{DocumentStore, Name, Object};
use pdf_fonts::{FontKind, FontMapper};

/// Resolves the font object at `num` from `doc` and builds a [`FontMapper`].
fn mapper_for(doc: &DocumentStore, num: u32) -> FontMapper {
    let obj = doc.get_object(num, 0).expect("font object");
    let dict = obj.as_dict().expect("font is a dict").clone();
    FontMapper::from_dict(&dict, doc)
}

fn tu(m: &FontMapper, code: u32) -> Option<String> {
    m.to_unicode(code).map(|s| s.to_string())
}

// === simple-font encoding via the mapper =================================

#[test]
fn encoding_008_basencoding_plus_differences() {
    // WinAnsi base, but /Differences remaps 0x80 → "bullet".
    let mut d = FontDoc::new();
    let enc = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Encoding")),
        ("BaseEncoding", name_obj("WinAnsiEncoding")),
        (
            "Differences",
            Object::Array(vec![Object::Integer(0x80), name_obj("bullet")]),
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

    assert_eq!(m.kind(), FontKind::Simple);
    // 0x41 still 'A' from the base; 0x80 overridden to bullet (U+2022).
    assert_eq!(tu(&m, 0x41).as_deref(), Some("A"));
    assert_eq!(tu(&m, 0x80).as_deref(), Some("\u{2022}"));
}

#[test]
fn encoding_009_differences_over_implicit_base() {
    // No /BaseEncoding: implicit Standard base, /Differences adds Euro at 0x80.
    let mut d = FontDoc::new();
    let enc = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Encoding")),
        (
            "Differences",
            Object::Array(vec![Object::Integer(0x80), name_obj("Euro")]),
        ),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Times-Roman")),
        ("Encoding", rref(enc, 0)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(tu(&m, 0x80).as_deref(), Some("\u{20AC}"));
    // Standard base still resolves ASCII.
    assert_eq!(tu(&m, 0x41).as_deref(), Some("A"));
}

#[test]
fn encoding_010_symbolic_truetype_no_encoding_defaults_standard() {
    // A simple TrueType with no /Encoding: falls back to Standard; ASCII works.
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("SomeFont")),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(tu(&m, 0x41).as_deref(), Some("A"));
}

// === WIDTHS-* ============================================================

#[test]
fn widths_001_widths_indexed_by_firstchar() {
    // FirstChar 65; Widths [600 700 800] → code 65→600, 66→700, 67→800.
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("FirstChar", Object::Integer(65)),
        ("LastChar", Object::Integer(67)),
        (
            "Widths",
            Object::Array(vec![
                Object::Integer(600),
                Object::Integer(700),
                Object::Integer(800),
            ]),
        ),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(65), 600.0);
    assert_eq!(m.width(66), 700.0);
    assert_eq!(m.width(67), 800.0);
}

#[test]
fn widths_002_out_of_range_uses_missingwidth() {
    let mut d = FontDoc::new();
    let desc = d.add(Object::Dictionary(dict([
        ("Type", name_obj("FontDescriptor")),
        ("MissingWidth", Object::Integer(250)),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("FirstChar", Object::Integer(65)),
        ("Widths", Object::Array(vec![Object::Integer(600)])),
        ("FontDescriptor", rref(desc, 0)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(65), 600.0);
    // 90 is past the single-entry Widths → MissingWidth.
    assert_eq!(m.width(90), 250.0);
    // Below FirstChar → MissingWidth too.
    assert_eq!(m.width(10), 250.0);
}

#[test]
fn widths_003_absent_missingwidth_is_zero() {
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("FirstChar", Object::Integer(65)),
        ("Widths", Object::Array(vec![Object::Integer(600)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(200), 0.0);
}

#[test]
fn widths_004_absurd_values_clamped() {
    // Negative and absurdly-large widths round-trip through the PDF serializer
    // (NaN cannot — the serializer rejects it — so NaN clamping is asserted in
    // the widths-unit test below via `sanitize`).
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("FirstChar", Object::Integer(0)),
        (
            "Widths",
            Object::Array(vec![
                Object::Real(-50.0),
                Object::Real(1e30),
                Object::Integer(500),
            ]),
        ),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0), 0.0); // negative
    assert_eq!(m.width(1), 0.0); // absurd
    assert_eq!(m.width(2), 500.0); // sane
}

#[test]
fn widths_004_sanitize_nan_negative_absurd() {
    use pdf_fonts::widths::sanitize;
    assert_eq!(sanitize(f64::NAN), 0.0);
    assert_eq!(sanitize(f64::INFINITY), 0.0);
    assert_eq!(sanitize(-1.0), 0.0);
    assert_eq!(sanitize(1e30), 0.0);
    assert_eq!(sanitize(500.0), 500.0);
    assert_eq!(sanitize(0.0), 0.0);
}

#[test]
fn widths_core14_001_helvetica_no_widths_uses_afm() {
    // Unembedded standard-14 Helvetica, no /Widths: the Core-14 AFM advances now
    // apply during extraction — and override /MissingWidth (here 333) per glyph.
    let mut d = FontDoc::new();
    let desc = d.add(Object::Dictionary(dict([
        ("Type", name_obj("FontDescriptor")),
        ("MissingWidth", Object::Integer(333)),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("FontDescriptor", rref(desc, 0)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    // Standard AFM advances: space=278, 'A'=667, 'i'=222.
    assert_eq!(m.width(0x20), 278.0);
    assert_eq!(m.width(0x41), 667.0);
    assert_eq!(m.width(0x69), 222.0);
    // The hook now resolves an AFM value rather than returning None.
    assert_eq!(
        pdf_fonts::widths::core14_width("Helvetica", "A"),
        Some(667.0)
    );
}

// === ITERCODES-* =========================================================

#[test]
fn itercodes_001_simple_one_byte() {
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    let codes: Vec<(u32, u8)> = m.iter_codes(b"ABC").collect();
    assert_eq!(codes, vec![(0x41, 1), (0x42, 1), (0x43, 1)]);
}

#[test]
fn itercodes_002_identity_h_two_bytes() {
    let doc = identity_h_doc();
    let m = mapper_for(&doc.0, doc.1);
    // <0041> <0042> → two 2-byte codes.
    let codes: Vec<(u32, u8)> = m.iter_codes(&[0x00, 0x41, 0x00, 0x42]).collect();
    assert_eq!(codes, vec![(0x0041, 2), (0x0042, 2)]);
}

#[test]
fn itercodes_003_embedded_codespace_variable_length() {
    // Embedded CMap: 1-byte codes 0x00..0x7F, 2-byte codes 0x8000..0xFFFF.
    let program = b"begincodespacerange <00> <7F> <8000> <FFFF> endcodespacerange \
                    endcmap";
    let doc = type0_embedded_cmap_doc(program);
    let m = mapper_for(&doc.0, doc.1);
    // 0x41 is 1-byte; 0x81 0x00 is a 2-byte code.
    let codes: Vec<(u32, u8)> = m.iter_codes(&[0x41, 0x81, 0x00, 0x42]).collect();
    assert_eq!(codes, vec![(0x41, 1), (0x8100, 2), (0x42, 1)]);
}

#[test]
fn itercodes_004_odd_trailing_byte_no_panic() {
    let doc = identity_h_doc();
    let m = mapper_for(&doc.0, doc.1);
    // 3 bytes with 2-byte codes: last lone byte consumed as a 1-byte unit.
    let codes: Vec<(u32, u8)> = m.iter_codes(&[0x00, 0x41, 0x99]).collect();
    assert_eq!(codes, vec![(0x0041, 2), (0x99, 1)]);
}

// === CID-* ===============================================================

/// Builds a Type0/Identity-H font (CIDFontType2) and returns `(doc, font_num)`.
fn identity_h_doc() -> (DocumentStore, u32) {
    let mut d = FontDoc::new();
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("Sub+CIDFont")),
        ("DW", Object::Integer(1000)),
        (
            "W",
            Object::Array(vec![
                // 0x41 [500 600] → CID 0x41=500, 0x42=600.
                Object::Integer(0x41),
                Object::Array(vec![Object::Integer(500), Object::Integer(600)]),
                // 0x50 0x52 700 → CIDs 0x50..0x52 = 700.
                Object::Integer(0x50),
                Object::Integer(0x52),
                Object::Integer(700),
            ]),
        ),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("BaseFont", name_obj("Sub+CIDFont")),
        ("Encoding", name_obj("Identity-H")),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    (d.open(), font)
}

fn type0_embedded_cmap_doc(cmap_program: &[u8]) -> (DocumentStore, u32) {
    let mut d = FontDoc::new();
    let cmap = d.add(flate_stream(
        [("Type", name_obj("CMap")), ("CMapName", name_obj("Custom"))],
        cmap_program,
    ));
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType0")),
        ("BaseFont", name_obj("Embedded")),
        ("DW", Object::Integer(1000)),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("BaseFont", name_obj("Embedded")),
        ("Encoding", rref(cmap, 0)),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    (d.open(), font)
}

#[test]
fn cid_001_identity_h_code_is_cid_with_tounicode() {
    // Identity-H + a /ToUnicode that maps 0x0041 → 'A'.
    let mut d = FontDoc::new();
    let tu_stream = d.add(flate_stream(
        [("Type", name_obj("CMap"))],
        b"1 beginbfchar <0041> <0041> endbfchar",
    ));
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("X")),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("Encoding", name_obj("Identity-H")),
        ("ToUnicode", rref(tu_stream, 0)),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.kind(), FontKind::Type0);
    assert_eq!(m.cid(0x0041), 0x0041);
    assert_eq!(tu(&m, 0x0041).as_deref(), Some("A"));
}

#[test]
fn cid_002_w_array_form() {
    let (doc, font) = identity_h_doc();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0x41), 500.0);
    assert_eq!(m.width(0x42), 600.0);
}

#[test]
fn cid_003_w_range_form() {
    let (doc, font) = identity_h_doc();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0x50), 700.0);
    assert_eq!(m.width(0x51), 700.0);
    assert_eq!(m.width(0x52), 700.0);
}

#[test]
fn cid_004_dw_default_for_uncovered_cid() {
    let (doc, font) = identity_h_doc();
    let m = mapper_for(&doc, font);
    // 0x99 is in neither W entry → DW (1000).
    assert_eq!(m.width(0x99), 1000.0);
}

#[test]
fn cid_005_absent_dw_defaults_to_1000() {
    let mut d = FontDoc::new();
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("X")),
        // no /DW, no /W
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("Encoding", name_obj("Identity-H")),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.width(0x0041), 1000.0);
}

#[test]
fn cid_006_cidtogidmap_identity_default() {
    let (doc, font) = identity_h_doc();
    let m = mapper_for(&doc, font);
    // No /CIDToGIDMap → Identity: gid == cid == code.
    assert_eq!(m.gid(0x0041), 0x0041);
}

#[test]
fn cid_007_cidtogidmap_stream() {
    // CIDToGIDMap stream: CID 0→GID 0, CID 1→GID 0, CID 2→GID 7 (be u16).
    let map_bytes: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x07];
    let mut d = FontDoc::new();
    let cgmap = d.add(raw_stream([], &map_bytes));
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("X")),
        ("CIDToGIDMap", rref(cgmap, 0)),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("Encoding", name_obj("Identity-H")),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.gid(2), 7);
    assert_eq!(m.gid(1), 0);
    // CID outside the map → GID 0 (notdef).
    assert_eq!(m.gid(99), 0);
}

#[test]
fn cid_008_embedded_cmap_code_to_cid() {
    // Embedded CMap maps codes to non-identity CIDs.
    let program = b"begincodespacerange <0000> <FFFF> endcodespacerange \
                    1 begincidrange <0000> <00FF> 100 endcidrange \
                    endcmap";
    let doc = type0_embedded_cmap_doc(program);
    let m = mapper_for(&doc.0, doc.1);
    // code 0 → CID 100, code 5 → CID 105.
    assert_eq!(m.cid(0), 100);
    assert_eq!(m.cid(5), 105);
}

#[test]
fn cid_009_type0_without_tounicode_is_none() {
    let (doc, font) = identity_h_doc();
    let m = mapper_for(&doc, font);
    // No /ToUnicode on this font → documented CJK gap → None (no panic).
    assert_eq!(tu(&m, 0x0041), None);
}

// === FONTMAP-* ===========================================================

#[test]
fn fontmap_001_tounicode_overrides_encoding() {
    // WinAnsi would map 0x41→'A', but /ToUnicode remaps 0x41 → 'Z'.
    let mut d = FontDoc::new();
    let tu_stream = d.add(flate_stream(
        [("Type", name_obj("CMap"))],
        b"1 beginbfchar <41> <005A> endbfchar",
    ));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("ToUnicode", rref(tu_stream, 0)),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(tu(&m, 0x41).as_deref(), Some("Z"));
    // A code the ToUnicode does NOT cover falls through to encoding+AGL.
    assert_eq!(tu(&m, 0x42).as_deref(), Some("B"));
}

#[test]
fn fontmap_002_type3_simple_path() {
    // Type3 is a simple font: encoding + Widths.
    let mut d = FontDoc::new();
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type3")),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FirstChar", Object::Integer(65)),
        ("Widths", Object::Array(vec![Object::Integer(1000)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.kind(), FontKind::Simple);
    assert_eq!(tu(&m, 0x41).as_deref(), Some("A"));
    assert_eq!(m.width(65), 1000.0);
}

#[test]
fn fontmap_003_identity_v_resolved() {
    let mut d = FontDoc::new();
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("X")),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("Encoding", name_obj("Identity-V")),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    assert_eq!(m.kind(), FontKind::Type0);
    // Identity-V is still 2-byte, code==CID.
    let codes: Vec<(u32, u8)> = m.iter_codes(&[0x12, 0x34]).collect();
    assert_eq!(codes, vec![(0x1234, 2)]);
    assert_eq!(m.cid(0x1234), 0x1234);
}

#[test]
fn fontmap_004_unknown_predefined_cmap_no_panic() {
    // A known-but-unbundled predefined CJK CMap name (documented gap).
    let mut d = FontDoc::new();
    let descendant = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType0")),
        ("BaseFont", name_obj("X")),
    ])));
    let font = d.add(Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("Encoding", name_obj("UniGB-UCS2-H")),
        ("DescendantFonts", Object::Array(vec![rref(descendant, 0)])),
    ])));
    let doc = d.open();
    let m = mapper_for(&doc, font);
    // Best-effort: 2-byte iteration, code==CID, no /ToUnicode → None, no panic.
    let codes: Vec<(u32, u8)> = m.iter_codes(&[0x00, 0x41]).collect();
    assert_eq!(codes, vec![(0x0041, 2)]);
    assert_eq!(tu(&m, 0x0041), None);
    assert_eq!(m.width(0x0041), 1000.0); // DW default
}

// Confirm the predefined classification is what we documented.
#[test]
fn fontmap_predefined_classification() {
    use pdf_fonts::predefined::{classify, PredefinedKind};
    assert_eq!(classify(b"Identity-H"), PredefinedKind::Identity);
    assert_eq!(classify(b"Identity-V"), PredefinedKind::Identity);
    assert_eq!(classify(b"UniGB-UCS2-H"), PredefinedKind::KnownUnbundled);
    assert_eq!(classify(b"90ms-RKSJ-H"), PredefinedKind::KnownUnbundled);
    assert_eq!(classify(b"NotARealCMapName"), PredefinedKind::Unknown);
}

// Silence unused-import warnings if Name is only used transitively.
const _: fn() = || {
    let _ = Name::new("x");
};
