//! `ENCODING-*` + `GLYPHLIST-*` — base encodings, `/Differences`, and
//! glyph-name → Unicode resolution (PRD §8.5; catalog M2a).

use pdf_fonts::encodings::BaseEncoding;
use pdf_fonts::glyphlist::glyph_name_to_unicode;

fn uni(name: &str) -> Option<String> {
    glyph_name_to_unicode(name).map(|s| s.to_string())
}

/// Resolves a `(encoding, code)` straight through to Unicode.
fn enc_uni(enc: BaseEncoding, code: u8) -> Option<String> {
    let name = enc.glyph_name(code)?;
    glyph_name_to_unicode(name).map(|s| s.to_string())
}

// --- ENCODING-* ----------------------------------------------------------

#[test]
fn encoding_001_winansi_a_and_euro() {
    // 0x41 → "A" → U+0041; 0x80 → "Euro" → U+20AC.
    assert_eq!(BaseEncoding::WinAnsi.glyph_name(0x41), Some("A"));
    assert_eq!(enc_uni(BaseEncoding::WinAnsi, 0x41).as_deref(), Some("A"));
    assert_eq!(BaseEncoding::WinAnsi.glyph_name(0x80), Some("Euro"));
    assert_eq!(
        enc_uni(BaseEncoding::WinAnsi, 0x80).as_deref(),
        Some("\u{20AC}")
    );
}

#[test]
fn encoding_002_standard_exclamdown() {
    assert_eq!(BaseEncoding::Standard.glyph_name(0xA1), Some("exclamdown"));
    assert_eq!(
        enc_uni(BaseEncoding::Standard, 0xA1).as_deref(),
        Some("\u{00A1}")
    );
    // StandardEncoding has quoteright at 0x27 (not quotesingle).
    assert_eq!(BaseEncoding::Standard.glyph_name(0x27), Some("quoteright"));
}

#[test]
fn encoding_003_macroman_adieresis() {
    assert_eq!(BaseEncoding::MacRoman.glyph_name(0x80), Some("Adieresis"));
    assert_eq!(
        enc_uni(BaseEncoding::MacRoman, 0x80).as_deref(),
        Some("\u{00C4}")
    );
}

#[test]
fn encoding_004_pdfdoc_euro_and_breve() {
    // PDFDoc has Euro at 0xA0 and breve at 0x18.
    assert_eq!(BaseEncoding::PdfDoc.glyph_name(0xA0), Some("Euro"));
    assert_eq!(
        enc_uni(BaseEncoding::PdfDoc, 0xA0).as_deref(),
        Some("\u{20AC}")
    );
    assert_eq!(BaseEncoding::PdfDoc.glyph_name(0x18), Some("breve"));
    assert_eq!(
        enc_uni(BaseEncoding::PdfDoc, 0x18).as_deref(),
        Some("\u{02D8}")
    );
}

#[test]
fn encoding_005_symbol_alpha() {
    assert_eq!(BaseEncoding::Symbol.glyph_name(0x61), Some("alpha"));
    assert_eq!(
        enc_uni(BaseEncoding::Symbol, 0x61).as_deref(),
        Some("\u{03B1}")
    );
}

#[test]
fn encoding_006_zapf_a10() {
    // Canonical ZapfDingbats encoding: 0x41 ('A') → "a10" → U+2721 (Star of
    // David), 0x61 ('a') → "a60" → U+2741. The `aNN` Dingbat names resolve via
    // the bundled Adobe `zapfdingbats.txt` (same BSD-3-Clause source as the AGL).
    assert_eq!(BaseEncoding::ZapfDingbats.glyph_name(0x41), Some("a10"));
    assert_eq!(
        enc_uni(BaseEncoding::ZapfDingbats, 0x41).as_deref(),
        Some("\u{2721}")
    );
    assert_eq!(BaseEncoding::ZapfDingbats.glyph_name(0x61), Some("a60"));
    assert_eq!(
        enc_uni(BaseEncoding::ZapfDingbats, 0x61).as_deref(),
        Some("\u{2741}")
    );
}

#[test]
fn encoding_007_from_name() {
    assert_eq!(
        BaseEncoding::from_name(b"WinAnsiEncoding"),
        Some(BaseEncoding::WinAnsi)
    );
    assert_eq!(
        BaseEncoding::from_name(b"StandardEncoding"),
        Some(BaseEncoding::Standard)
    );
    assert_eq!(
        BaseEncoding::from_name(b"MacRomanEncoding"),
        Some(BaseEncoding::MacRoman)
    );
    assert_eq!(
        BaseEncoding::from_name(b"PDFDocEncoding"),
        Some(BaseEncoding::PdfDoc)
    );
    assert_eq!(BaseEncoding::from_name(b"BogusEncoding"), None);
}

#[test]
fn encoding_011_unmapped_simple_code_is_none() {
    // A code with no glyph in Symbol (e.g. 0x01) → no name → never panics.
    assert_eq!(BaseEncoding::Symbol.glyph_name(0x01), None);
}

// --- GLYPHLIST-* (AGL + algorithmic) -------------------------------------

#[test]
fn glyphlist_001_agl_named() {
    assert_eq!(uni("quotedblleft").as_deref(), Some("\u{201C}"));
    assert_eq!(uni("Euro").as_deref(), Some("\u{20AC}"));
    assert_eq!(uni("A").as_deref(), Some("A"));
}

#[test]
fn glyphlist_002_agl_ligature_fi() {
    assert_eq!(uni("fi").as_deref(), Some("\u{FB01}"));
    assert_eq!(uni("fl").as_deref(), Some("\u{FB02}"));
}

#[test]
fn glyphlist_003_uni_hex() {
    assert_eq!(uni("uni20AC").as_deref(), Some("\u{20AC}"));
    assert_eq!(uni("uni0041").as_deref(), Some("A"));
    // Multi-group uniXXXXXXXX → concatenation.
    assert_eq!(uni("uni004100420043").as_deref(), Some("ABC"));
}

#[test]
fn glyphlist_004_u_hex_astral() {
    assert_eq!(uni("u1F600").as_deref(), Some("\u{1F600}"));
    assert_eq!(uni("u0041").as_deref(), Some("A"));
}

#[test]
fn glyphlist_005_underscore_ligature() {
    // f_f_i → U+0066 U+0066 U+0069.
    assert_eq!(uni("f_f_i").as_deref(), Some("ffi"));
}

#[test]
fn glyphlist_006_dot_suffix_strip() {
    // a.sc → base glyph "a" → U+0061.
    assert_eq!(uni("a.sc").as_deref(), Some("a"));
    assert_eq!(uni("A.alt").as_deref(), Some("A"));
}

#[test]
fn glyphlist_007_cid_gid_notdef_unresolved() {
    assert_eq!(uni("cid12"), None);
    assert_eq!(uni("g42"), None);
    assert_eq!(uni(".notdef"), None);
    assert_eq!(uni("notdef"), None);
}

#[test]
fn glyphlist_008_unknown_name_none() {
    assert_eq!(uni("totallybogusglyphname123"), None);
    assert_eq!(uni(""), None);
}

// --- ENCODING dict / Differences are tested via the FontMapper in
//     mapper_unit.rs (ENCODING-008/009/010), since they require a font dict.
