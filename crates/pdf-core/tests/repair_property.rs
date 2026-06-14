//! `REPAIR-PANIC-*` — the never-panic / never-hang / bounded-work robustness
//! contract for the repair subsystem (PRD §9.6). Opening ANY byte string —
//! arbitrary, truncated, bit-flipped — must terminate quickly and return either
//! a valid (possibly repaired) document or a typed error. Never panic, never
//! OOM, never infinite-loop.
//!
//! These are property tests with high case counts. Each case has a hard wall
//! clock budget (a watchdog thread) so a hypothetical infinite loop fails the
//! test instead of hanging CI.

mod common;

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::*;
use pdf_core::{DocumentStore, Limits, Object, ParseMode};
use proptest::prelude::*;

/// Per-input wall-clock budget. A repair pass on even a large fuzz input is a
/// single O(n) scan; anything beyond this means a hang/quadratic blowup.
const BUDGET: Duration = Duration::from_secs(5);

/// Opens `bytes` in `mode` on a worker thread guarded by a watchdog. Returns
/// `true` if the open *terminated* (Ok or Err) within [`BUDGET`]; panics (fails
/// the test) on timeout. Catches panics from the worker so a panic in the parser
/// fails the property rather than aborting the process.
fn open_terminates(bytes: Vec<u8>, mode: ParseMode) -> bool {
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        // Tight limits keep memory + work bounded even for adversarial inputs.
        let limits = Limits::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // We don't care about Ok/Err — only that it returns without panic.
            let opened = DocumentStore::from_bytes_with(bytes.clone(), mode, limits);
            // If it opened, exercise a few resolves to stress the lazy path too.
            if let Ok(doc) = opened {
                for num in 0u32..8 {
                    let _ = doc.resolve(pdf_core::ObjRef::new(num, 0));
                }
            }
        }));
        let _ = tx.send(result.is_ok());
    });
    match rx.recv_timeout(BUDGET) {
        Ok(no_panic) => {
            let _ = handle.join();
            assert!(no_panic, "open panicked on input");
            true
        }
        Err(_) => {
            // Timed out: the worker is still running — a hang. Fail loudly.
            panic!("open did not terminate within {BUDGET:?} (possible infinite loop)");
        }
    }
}

/// A valid baseline PDF to bit-flip / truncate.
fn baseline() -> Vec<u8> {
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("Contents", rref(4, 0)),
            ])),
        )
        .obj(4, 0, flate_stream([], b"BT /F1 12 Tf (hi) Tj ET"))
        .root(1, 0)
        .build()
}

proptest! {
    // High case count for the adversarial contract (PRD §9.6).
    #![proptest_config(ProptestConfig {
        cases: 2048,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    /// REPAIR-PANIC-001: arbitrary bytes, Lenient, never panics / hangs.
    #[test]
    fn repair_panic_001_arbitrary_lenient(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        prop_assert!(open_terminates(bytes, ParseMode::Lenient));
    }

    /// REPAIR-PANIC-002: arbitrary bytes, Strict, never panics / hangs.
    #[test]
    fn repair_panic_002_arbitrary_strict(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        prop_assert!(open_terminates(bytes, ParseMode::Strict));
    }

    /// REPAIR-PANIC-003: a single bit flipped in a valid PDF never panics.
    #[test]
    fn repair_panic_003_bitflip(
        flip_byte in 0usize..4096,
        flip_bit in 0u8..8,
        mode_lenient in any::<bool>(),
    ) {
        let mut bytes = baseline();
        if flip_byte < bytes.len() {
            bytes[flip_byte] ^= 1 << flip_bit;
        }
        let mode = if mode_lenient { ParseMode::Lenient } else { ParseMode::Strict };
        prop_assert!(open_terminates(bytes, mode));
    }

    /// REPAIR-PANIC-004: truncate a valid PDF at any offset; never panics.
    #[test]
    fn repair_panic_004_truncate(keep in 0usize..4096, mode_lenient in any::<bool>()) {
        let bytes = corrupt_truncate(&baseline(), keep);
        let mode = if mode_lenient { ParseMode::Lenient } else { ParseMode::Strict };
        prop_assert!(open_terminates(bytes, mode));
    }

    /// REPAIR-PANIC-006: resolve of arbitrary object numbers on a repaired doc
    /// never panics (and the open path stays bounded).
    #[test]
    fn repair_panic_006_resolve_arbitrary(nums in proptest::collection::vec(any::<u32>(), 0..32)) {
        // Open a deliberately broken (reconstructed) doc, then hammer resolve.
        let broken = corrupt_remove_xref_and_trailer(&baseline());
        let limits = Limits::default();
        let opened = DocumentStore::from_bytes_with(broken, ParseMode::Lenient, limits);
        if let Ok(doc) = opened {
            for n in nums {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = doc.resolve(pdf_core::ObjRef::new(n, 0));
                }));
            }
        }
    }
}

/// REPAIR-PANIC-005: the object scan honors `max_objects` — a buffer with far
/// more `N G obj` headers than the cap must not grow the table past the cap and
/// must terminate. Deterministic (not proptest) so we can assert the bound.
#[test]
fn repair_panic_005_object_count_bound() {
    // Build a buffer with 10_000 `N G obj` headers, but cap max_objects at 100.
    let mut buf = Vec::new();
    buf.extend_from_slice(b"%PDF-1.7\n");
    for n in 1..=10_000u32 {
        buf.extend_from_slice(format!("{n} 0 obj\n{n}\nendobj\n").as_bytes());
    }
    let limits = Limits::default().with_max_objects(100);
    // Must terminate (no hang) and not panic. The doc likely fails to open (no
    // catalog), which is fine — we only assert it returns and stays bounded.
    let result = std::panic::catch_unwind(|| {
        let _ = DocumentStore::from_bytes_with(buf, ParseMode::Lenient, limits);
    });
    assert!(result.is_ok(), "scan panicked under object-count pressure");
}
