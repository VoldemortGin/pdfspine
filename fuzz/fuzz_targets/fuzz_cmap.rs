#![no_main]
//! CMap fuzz target: embedded CMap / ToUnicode programs are untrusted
//! content-stream-like token sequences. Parsing, code→CID / code→Unicode
//! lookups, the Unicode table and the CID inversion must never panic, OOM or
//! hang (ranges are stored compactly, never expanded per code).
//! Run with `cargo +nightly fuzz run fuzz_cmap`.

use libfuzzer_sys::fuzz_target;
use pdf_fonts::CMap;

fuzz_target!(|data: &[u8]| {
    let cmap = CMap::parse(data, &mut |_| None);
    let _ = cmap.codespace();
    for code in [
        0u32,
        1,
        0x20,
        0x41,
        0xff,
        0x100,
        0x8140,
        0xffff,
        0x10000,
        u32::MAX,
    ] {
        let _ = cmap.cid(code);
        let _ = cmap.to_unicode(code);
    }
    let _ = cmap.unicode_mappings(0xffff);
    let _ = cmap.invert_to_cid_unicode();
});
