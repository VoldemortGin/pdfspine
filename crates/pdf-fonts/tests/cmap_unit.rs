//! `CMAP-*` — the shared CMap parser: ToUnicode (bfchar/bfrange, UTF-16BE,
//! 1-to-many), CID (cidchar/cidrange), codespacerange and usecmap (PRD §8.5).

use pdf_fonts::cmap::CMap;

fn parse(src: &[u8]) -> CMap {
    let mut no_use = |_: &[u8]| None;
    CMap::parse(src, &mut no_use)
}

#[test]
fn cmap_001_bfchar_single_byte() {
    let src = b"1 beginbfchar <41> <0041> endbfchar";
    let cm = parse(src);
    assert_eq!(
        cm.to_unicode(0x41).map(|s| s.to_string()).as_deref(),
        Some("A")
    );
    assert_eq!(cm.to_unicode(0x42), None);
}

#[test]
fn cmap_002_bfrange_increment() {
    // <20>..<22> base <0041> → 0x20→A, 0x21→B, 0x22→C.
    let src = b"1 beginbfrange <20> <22> <0041> endbfrange";
    let cm = parse(src);
    assert_eq!(
        cm.to_unicode(0x20).map(|s| s.to_string()).as_deref(),
        Some("A")
    );
    assert_eq!(
        cm.to_unicode(0x21).map(|s| s.to_string()).as_deref(),
        Some("B")
    );
    assert_eq!(
        cm.to_unicode(0x22).map(|s| s.to_string()).as_deref(),
        Some("C")
    );
}

#[test]
fn cmap_003_bfrange_array_form() {
    let src = b"1 beginbfrange <10> <12> [ <0058> <0059> <005A> ] endbfrange";
    let cm = parse(src);
    assert_eq!(
        cm.to_unicode(0x10).map(|s| s.to_string()).as_deref(),
        Some("X")
    );
    assert_eq!(
        cm.to_unicode(0x11).map(|s| s.to_string()).as_deref(),
        Some("Y")
    );
    assert_eq!(
        cm.to_unicode(0x12).map(|s| s.to_string()).as_deref(),
        Some("Z")
    );
}

#[test]
fn cmap_004_utf16be_surrogate_pair() {
    // U+1F600 in UTF-16BE is D83D DE00.
    let src = b"1 beginbfchar <01> <D83DDE00> endbfchar";
    let cm = parse(src);
    assert_eq!(
        cm.to_unicode(0x01).map(|s| s.to_string()).as_deref(),
        Some("\u{1F600}")
    );
}

#[test]
fn cmap_005_one_to_many_ligature() {
    // "ffi" = 0066 0066 0069 in UTF-16BE.
    let src = b"1 beginbfchar <03> <006600660069> endbfchar";
    let cm = parse(src);
    assert_eq!(
        cm.to_unicode(0x03).map(|s| s.to_string()).as_deref(),
        Some("ffi")
    );
}

#[test]
fn cmap_006_codespacerange_byte_width() {
    let src = b"1 begincodespacerange <0000> <FFFF> endcodespacerange";
    let cm = parse(src);
    let cs = cm.codespace();
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].n_bytes, 2);
    assert_eq!(cs[0].low, 0x0000);
    assert_eq!(cs[0].high, 0xFFFF);
}

#[test]
fn cmap_007_cidchar_and_cidrange() {
    let src = b"1 begincidchar <0041> 65 endcidchar \
                1 begincidrange <0100> <0102> 256 endcidrange";
    let cm = parse(src);
    assert_eq!(cm.cid(0x41), Some(65));
    assert_eq!(cm.cid(0x100), Some(256));
    assert_eq!(cm.cid(0x101), Some(257));
    assert_eq!(cm.cid(0x102), Some(258));
    assert_eq!(cm.cid(0x200), None);
}

#[test]
fn cmap_008_usecmap_chaining() {
    // Parent supplies a cidrange; child supplies a codespacerange + usecmap.
    let parent_src = b"1 begincidrange <0000> <00FF> 0 endcidrange";
    let parent = parse(parent_src);

    let child_src = b"/Parent usecmap \
                      1 begincodespacerange <00> <FF> endcodespacerange \
                      1 begincidchar <41> 1000 endcidchar";
    let mut resolver = |name: &[u8]| {
        if name == b"Parent" {
            Some(parent.clone())
        } else {
            None
        }
    };
    let child = CMap::parse(child_src, &mut resolver);

    // Parent range visible.
    assert_eq!(child.cid(0x10), Some(0x10));
    // Child override wins (last-wins on overlap).
    assert_eq!(child.cid(0x41), Some(1000));
    // Child codespace present.
    assert_eq!(child.codespace().len(), 1);
}

#[test]
fn cmap_009_malformed_tokens_no_panic() {
    // Truncated hex, dangling operators, stray brackets — must not panic.
    let src = b"beginbfchar <41 garbage ] [ } endbfchar <  >  1 begincidrange <ZZ";
    let cm = parse(src);
    // It produced *something* without panicking; the exact contents are not
    // asserted, only that the call returned.
    let _ = cm.to_unicode(0x41);
    let _ = cm.cid(0x41);
}

#[test]
fn cmap_010_mixed_one_and_two_byte_codespace() {
    let src = b"2 begincodespacerange <00> <80> <8140> <FEFE> endcodespacerange";
    let cm = parse(src);
    let cs = cm.codespace();
    assert_eq!(cs.len(), 2);
    assert!(cs
        .iter()
        .any(|r| r.n_bytes == 1 && r.low == 0x00 && r.high == 0x80));
    assert!(cs
        .iter()
        .any(|r| r.n_bytes == 2 && r.low == 0x8140 && r.high == 0xFEFE));
}
