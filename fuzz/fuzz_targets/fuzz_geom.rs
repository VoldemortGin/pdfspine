#![no_main]
//! M0 fuzz smoke target: geometry never panics on arbitrary coordinates.
//!
//! Real untrusted-byte targets (`fuzz_open`/`fuzz_lexer`/`fuzz_xref`/per-filter)
//! land in M1 per PRD §9.6. This target exists so the `fuzz-smoke` CI job has
//! something to build/run from M0.

use libfuzzer_sys::fuzz_target;
use pdf_core::geom::{Matrix, Point, Rect};

fn f(bytes: &[u8], i: usize) -> f64 {
    let mut a = [0u8; 8];
    let start = (i * 8) % bytes.len().max(1);
    for (k, slot) in a.iter_mut().enumerate() {
        *slot = bytes.get(start + k).copied().unwrap_or(0);
    }
    f64::from_le_bytes(a)
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    let m = Matrix::new(f(data, 0), f(data, 1), f(data, 2), f(data, 3), f(data, 4), f(data, 5));
    let p = Point::new(f(data, 6), f(data, 7));
    let r = Rect::new(f(data, 0), f(data, 1), f(data, 2), f(data, 3));

    // None of these may panic on arbitrary (incl. NaN/inf) coordinates.
    let _ = p.transform(&m);
    let _ = m.determinant();
    let _ = m.invert();
    let _ = r.normalize();
    let _ = r.round();
    let _ = r.area();
    let _ = r.quad().transform(&m).rect();
});
