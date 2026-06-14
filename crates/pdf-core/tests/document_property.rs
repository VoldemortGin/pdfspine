//! `OPEN-PROP-*` — parser-hardening / never-panic contract (PRD §9.6).
//!
//! Opening arbitrary or truncated bytes must always return a typed `Result`
//! (`Ok` or `Err`), never panic / OOM / hang. Full repair is M1d; M1c must
//! already never panic. Runs in the hard-safe owned-bytes path (`from_bytes`).

mod common;

use common::*;
use pdf_core::{DocumentStore, Limits, Object};
use proptest::prelude::*;

/// A small, bounded limit set so a hostile fixture can't make the harness OOM.
fn safe_limits() -> Limits {
    Limits::default()
        .with_max_objstm_objects(4096)
        .with_max_recursion_depth(64)
}

/// A known-good minimal document (for the truncation property).
fn good_doc() -> Vec<u8> {
    Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Count", Object::Integer(0)),
            ])),
        )
        .obj(3, 0, flate_stream([], b"some stream content"))
        .root(1, 0)
        .build()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// OPEN-PROP-001: opening arbitrary bytes never panics.
    #[test]
    fn open_prop_001_arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        // Either an Ok store or a typed Err — but never a panic.
        let _ = DocumentStore::from_bytes(bytes, safe_limits());
    }

    /// OPEN-PROP-002: truncating a valid file at any offset never panics.
    #[test]
    fn open_prop_002_truncation_never_panic(cut in 0usize..2048) {
        let full = good_doc();
        let end = cut.min(full.len());
        let truncated = full[..end].to_vec();
        let _ = DocumentStore::from_bytes(truncated, safe_limits());
    }

    /// OPEN-PROP-003: resolving arbitrary object numbers on an opened doc never
    /// panics (dangling / out-of-range refs are typed errors).
    #[test]
    fn open_prop_003_resolve_arbitrary_never_panic(num in any::<u32>(), gen in any::<u16>()) {
        let doc = DocumentStore::from_bytes(good_doc(), safe_limits()).unwrap();
        let _ = doc.get_object(num, gen);
        let _ = doc.resolve(pdf_core::ObjRef::new(num, gen));
    }
}

// Also fuzz a corpus of structurally-plausible-but-corrupt inputs: take the
// good doc and flip individual bytes. Must never panic.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn open_prop_004_bitflip_never_panic(pos in 0usize..1024, val in any::<u8>()) {
        let mut bytes = good_doc();
        if pos < bytes.len() {
            bytes[pos] = val;
        }
        let res = DocumentStore::from_bytes(bytes, safe_limits());
        // If it opened, walking it must also not panic.
        if let Ok(doc) = res {
            for num in 0..6u32 {
                let _ = doc.resolve(pdf_core::ObjRef::new(num, 0));
            }
        }
    }
}
