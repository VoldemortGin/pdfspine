#![no_main]
//! M1d fuzz target: opening ANY byte string must never panic / OOM / hang
//! (PRD §9.6). Drives `DocumentStore::from_bytes_with` in both parse modes and,
//! when an open succeeds, exercises the lazy resolve path on a few object
//! numbers. Run with `cargo +nightly fuzz run fuzz_open`.

use libfuzzer_sys::fuzz_target;
use pdf_core::{DocumentStore, Limits, ObjRef, ParseMode};

fuzz_target!(|data: &[u8]| {
    // Default (bomb-bounded) limits keep work + memory bounded for adversarial
    // inputs; this is the "never OOM" gate (PRD §9.6.2).
    let limits = Limits::default();

    for mode in [ParseMode::Lenient, ParseMode::Strict] {
        if let Ok(doc) = DocumentStore::from_bytes_with(data.to_vec(), mode, limits) {
            // Touch a handful of object numbers to stress lazy load + resolve.
            for num in 0u32..16 {
                let _ = doc.resolve(ObjRef::new(num, 0));
                let _ = doc.get_object(num, 0);
            }
            let _ = doc.repair_report();
            let _ = doc.warnings();
        }
    }
});
