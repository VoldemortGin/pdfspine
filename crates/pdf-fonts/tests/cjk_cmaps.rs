//! `CMAP-CJK-*` — bundled predefined CJK CMaps: the four UCS2 ToUnicode tables
//! (Adobe-GB1 / CNS1 / Japan1 / Korea1) give CID→Unicode so a Type0 font using a
//! predefined `/Encoding` extracts text without an embedded `/ToUnicode`
//! (PRD §8.5; ISO 32000-1 §9.7.5.2).

use pdf_fonts::predefined::{self, PredefinedKind};

fn u(cid: u32, name: &str) -> Option<String> {
    predefined::cid_to_unicode(name, cid).map(|s| s.to_string())
}

// === bundled UCS2 ToUnicode tables: CID → Unicode =========================

#[test]
fn cmap_cjk_gb1_cid_to_unicode() {
    // Adobe-GB1 via UniGB-UCS2-H (inverted): CID 4559 → 中 (U+4E2D).
    assert_eq!(u(4559, "UniGB-UCS2-H").as_deref(), Some("\u{4E2D}"));
    // ASCII range <0020>..<007e> base CID 1 → CID 34 is 'A' (0x41).
    assert_eq!(u(34, "UniGB-UCS2-H").as_deref(), Some("A"));
}

#[test]
fn cmap_cjk_cns1_cid_to_unicode() {
    // Adobe-CNS1 via UniCNS-UCS2-H: CID 595 → 一 (U+4E00).
    assert_eq!(u(595, "UniCNS-UCS2-H").as_deref(), Some("\u{4E00}"));
}

#[test]
fn cmap_cjk_jis_cid_to_unicode() {
    // Adobe-Japan1 via UniJIS-UCS2-H: range <3041>..<3093> base CID 842,
    // so CID 843 → あ (U+3042).
    assert_eq!(u(843, "UniJIS-UCS2-H").as_deref(), Some("\u{3042}"));
}

#[test]
fn cmap_cjk_ks_cid_to_unicode() {
    // Adobe-Korea1 via UniKS-UCS2-H: CID 1086 → 가 (U+AC00).
    assert_eq!(u(1086, "UniKS-UCS2-H").as_deref(), Some("\u{AC00}"));
}

#[test]
fn cmap_cjk_v_variant_resolves_same_table() {
    // The vertical name shares the horizontal collection's CID→Unicode table.
    assert_eq!(u(4559, "UniGB-UCS2-V").as_deref(), Some("\u{4E2D}"));
}

// === Kangxi-radical CJK fold (matches PyMuPDF) ============================

/// The CID a predefined encoding CMap assigns to a UCS2 source code point.
fn cid_for(name: &str, code: u32) -> Option<u32> {
    predefined::encoding_cmap(name.as_bytes()).and_then(|c| c.cid(code))
}

#[test]
fn cmap_cjk_kangxi_radical_folds_to_canonical_ideograph() {
    // PyMuPDF resolves the predefined-CMap path through the canonical CJK
    // ideograph, so a Kangxi radical source code folds to that ideograph:
    //   ⼀ U+2F00 → 一 U+4E00 ;  ⽇ U+2F47 → 日 U+65E5 .
    // (The CID is the one the encoding CMap assigns to the *radical* code; its
    // CID→Unicode must yield the *ideograph*, never the radical-presentation
    // code point.)
    for (name, radical, ideograph) in [
        ("UniGB-UCS2-H", 0x2F00u32, "\u{4E00}"),
        ("UniGB-UCS2-H", 0x2F47, "\u{65E5}"),
        ("UniJIS-UCS2-H", 0x2F00, "\u{4E00}"),
    ] {
        let cid = cid_for(name, radical).expect("encoding CMap maps the Kangxi radical code");
        assert_eq!(
            u(cid, name).as_deref(),
            Some(ideograph),
            "{name}: Kangxi U+{radical:04X} should fold to canonical ideograph",
        );
    }
}

#[test]
fn cmap_cjk_radicals_supplement_kept_verbatim() {
    // PyMuPDF does NOT fold the CJK Radicals Supplement block (U+2E80–U+2EFF):
    // it keeps those code points verbatim, even U+2E9F (whose NFKC decomposition
    // is U+6BCD 母). The fold is gated to the Kangxi block only.
    for (name, code) in [("UniGB-UCS2-H", 0x2E80u32), ("UniGB-UCS2-H", 0x2E9F)] {
        let cid = cid_for(name, code).expect("encoding CMap maps the CJK-radical-supplement code");
        let expected = char::from_u32(code).map(|c| c.to_string());
        assert_eq!(
            u(cid, name),
            expected,
            "{name}: CJK Radicals Supplement U+{code:04X} must be kept verbatim",
        );
    }
}

// === classification + bundling ===========================================

#[test]
fn cmap_cjk_classification_now_bundled() {
    // The four UCS2 families are now bundled (no longer the documented gap).
    assert_eq!(predefined::classify(b"UniGB-UCS2-H"), PredefinedKind::Cjk);
    assert_eq!(predefined::classify(b"UniCNS-UCS2-H"), PredefinedKind::Cjk);
    assert_eq!(predefined::classify(b"UniJIS-UCS2-H"), PredefinedKind::Cjk);
    assert_eq!(predefined::classify(b"UniKS-UCS2-H"), PredefinedKind::Cjk);
    // Identity unchanged.
    assert_eq!(
        predefined::classify(b"Identity-H"),
        PredefinedKind::Identity
    );
}

// === CMAP-CJK-FALLBACK-* : graceful None on unbundled / unknown ==========

#[test]
fn cmap_cjk_fallback_unbundled_name_is_none() {
    // A recognized-but-unbundled predefined name yields no CID→Unicode table.
    assert_eq!(predefined::cid_to_unicode("GBK-EUC-H", 4559), None);
    assert_eq!(predefined::cid_to_unicode("90ms-RKSJ-H", 843), None);
}

#[test]
fn cmap_cjk_fallback_unknown_name_is_none() {
    assert_eq!(predefined::cid_to_unicode("NotARealCMap", 1), None);
    assert_eq!(predefined::cid_to_unicode("", 1), None);
}

#[test]
fn cmap_cjk_fallback_unmapped_cid_is_none() {
    // A CID outside any range in the table → None, no panic.
    assert_eq!(predefined::cid_to_unicode("UniGB-UCS2-H", 9_999_999), None);
}
