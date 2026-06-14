#![no_main]
//! M1d fuzz target: the repair reconstruction pass directly (PRD §8.2 / §9.6).
//! Feeds arbitrary bytes into the full-file object scan / synthetic-xref builder
//! and asserts it terminates without panic. The scan is a single O(n) pass and
//! honors `Limits`, so even huge / adversarial inputs stay bounded.
//! Run with `cargo +nightly fuzz run fuzz_repair`.

use libfuzzer_sys::fuzz_target;
use pdf_core::repair::{reconstruct, Diagnostics};
use pdf_core::{Limits, Source};

fuzz_target!(|data: &[u8]| {
    let source = Source::from_bytes(data.to_vec());
    let limits = Limits::default();
    let mut diag = Diagnostics::new();
    // header_offset is fuzzed implicitly: try 0 and a small bias.
    for header_offset in [0usize, 1, data.len() / 2] {
        let mut d = diag.clone();
        let _ = reconstruct(&source, header_offset.min(data.len()), &limits, &mut d);
    }
    let _ = &mut diag;
});
