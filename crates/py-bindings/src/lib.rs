// `py-bindings` is the single FFI chokepoint and the only first-party crate
// permitted to use `unsafe` (PyO3 generates FFI glue). It therefore does NOT
// `forbid(unsafe_code)`; instead it requires `unsafe` to be explicitly scoped.
#![deny(unsafe_op_in_unsafe_fn)]
//! PyO3 bindings exposing oxipdf's Rust core to Python as the `_core` module.
//!
//! M0 surface is intentionally tiny: just enough to prove the abi3 wheel builds
//! and imports (`__version__`, `version()`, and an identity-matrix probe that
//! exercises the geometry path through `pdf-api`).

use pdf_api::geom::Matrix;
use pyo3::prelude::*;

/// The package version (mirrors the Rust workspace version).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns the oxipdf version string.
#[pyfunction]
fn version() -> &'static str {
    VERSION
}

/// Returns the 6-tuple of the identity matrix `[a, b, c, d, e, f]`.
///
/// A trivial probe that proves the Python -> PyO3 -> `pdf-api` -> `pdf-core`
/// geometry path is wired end to end.
#[pyfunction]
fn identity_matrix() -> (f64, f64, f64, f64, f64, f64) {
    let m = Matrix::IDENTITY;
    (m.a, m.b, m.c, m.d, m.e, m.f)
}

/// The `_core` extension module.
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", VERSION)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(identity_matrix, m)?)?;
    Ok(())
}
