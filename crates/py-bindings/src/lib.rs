// `py-bindings` is the single FFI chokepoint and the only first-party crate
// permitted to use `unsafe` (PyO3 generates FFI glue). It therefore does NOT
// `forbid(unsafe_code)`; instead it requires `unsafe` to be explicitly scoped.
#![deny(unsafe_op_in_unsafe_fn)]
//! PyO3 bindings exposing oxipdf's Rust core to Python as the `_core` module.
//!
//! M1f exposes the read surface (PRD §7 / §9.2 / §9.4): `open`, a `Document`
//! handle and a `Page` handle, both using the **handle/index pattern** — each
//! `#[pyclass]` is `'static` and carries its own `Arc`-backed [`pdf_api`] value,
//! never a Rust borrow. Heavy work (`open`/`open_bytes`) runs with the GIL
//! released via [`Python::detach`]. Errors map to a typed exception hierarchy
//! rooted at `_core.PdfError` (PRD §9.3).

use pdf_api::geom::Matrix;
use pdf_api::{Document as ApiDocument, Error as ApiError, ParseMode};
use pyo3::create_exception;
use pyo3::exceptions::{PyFileNotFoundError, PyIndexError, PyOSError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

/// The package version (mirrors the Rust workspace version).
const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- exception hierarchy (PRD §9.3) ---------------------------------------

create_exception!(_core, PdfError, pyo3::exceptions::PyException);
create_exception!(_core, PdfSyntaxError, PdfError);
create_exception!(_core, PdfPasswordError, PdfError);
create_exception!(_core, PdfUnsupportedError, PdfError);
create_exception!(_core, PdfDecodeError, PdfError);
create_exception!(_core, PdfLimitError, PdfError);

/// Maps a `pdf_api::Error` onto the appropriate Python exception (PRD §9.3).
fn map_err(e: ApiError) -> PyErr {
    let msg = e.to_string();
    match e.kind() {
        "io" => {
            // Preserve FileNotFound vs generic OS error where we can.
            if let ApiError::Io(io) = &e {
                if io.kind() == std::io::ErrorKind::NotFound {
                    return PyFileNotFoundError::new_err(msg);
                }
            }
            PyOSError::new_err(msg)
        }
        "password" => PdfPasswordError::new_err(msg),
        "unsupported" => PdfUnsupportedError::new_err(msg),
        "decode" => PdfDecodeError::new_err(msg),
        "limit" => PdfLimitError::new_err(msg),
        _ => PdfSyntaxError::new_err(msg),
    }
}

// --- Page handle ----------------------------------------------------------

/// A page handle (PRD §9.2). Holds a cloned `pdf_api::Page` (its own `Arc` onto
/// the document store) — `'static`, no borrow crosses the boundary.
#[pyclass(name = "Page", module = "oxipdf._core", frozen)]
struct PyPage {
    page: pdf_api::Page,
}

/// Converts a `Rect` to the 4-tuple `(x0, y0, x1, y1)` the Python layer wraps.
fn rect_tuple(r: pdf_api::Rect) -> (f64, f64, f64, f64) {
    (r.x0, r.y0, r.x1, r.y1)
}

#[pymethods]
impl PyPage {
    /// The zero-based page index (PyMuPDF `page.number`).
    #[getter]
    fn number(&self) -> usize {
        self.page.number()
    }

    /// The page bound `CropBox ∩ MediaBox` as `(x0, y0, x1, y1)`.
    fn rect(&self) -> (f64, f64, f64, f64) {
        rect_tuple(self.page.rect())
    }

    /// Alias for [`PyPage::rect`] (PyMuPDF `page.bound()`).
    fn bound(&self) -> (f64, f64, f64, f64) {
        rect_tuple(self.page.bound())
    }

    /// The effective `/MediaBox` as `(x0, y0, x1, y1)`.
    fn mediabox(&self) -> (f64, f64, f64, f64) {
        rect_tuple(self.page.mediabox())
    }

    /// The effective `/CropBox` as `(x0, y0, x1, y1)`.
    fn cropbox(&self) -> (f64, f64, f64, f64) {
        rect_tuple(self.page.cropbox())
    }

    /// The normalized rotation ∈ {0, 90, 180, 270}.
    fn rotation(&self) -> i32 {
        self.page.rotation()
    }

    fn __repr__(&self) -> String {
        format!("<oxipdf._core.Page number={}>", self.page.number())
    }
}

// --- Document handle ------------------------------------------------------

/// A document handle (PRD §9.2 / §9.4). Holds a `pdf_api::Document` (cheap to
/// clone: `Arc` bumps) so every `Page` it produces is independent of this object.
#[pyclass(name = "Document", module = "oxipdf._core", frozen)]
struct PyDocument {
    doc: ApiDocument,
}

#[pymethods]
impl PyDocument {
    /// The page count (PyMuPDF `page_count` / `len(doc)`).
    #[getter]
    fn page_count(&self) -> usize {
        self.doc.page_count()
    }

    fn __len__(&self) -> usize {
        self.doc.page_count()
    }

    /// Loads the page at zero-based `index` (PyMuPDF `load_page`).
    fn load_page(&self, index: usize) -> PyResult<PyPage> {
        let page = self.doc.load_page(index).map_err(map_err)?;
        Ok(PyPage { page })
    }

    /// `doc[index]` — supports negative indices like PyMuPDF.
    fn __getitem__(&self, index: isize) -> PyResult<PyPage> {
        let n = self.doc.page_count();
        let idx = if index < 0 {
            let abs = (-index) as usize;
            if abs > n {
                return Err(PyIndexError::new_err("page index out of range"));
            }
            n - abs
        } else {
            index as usize
        };
        let page = self
            .doc
            .load_page(idx)
            .map_err(|_| PyIndexError::new_err("page index out of range"))?;
        Ok(PyPage { page })
    }

    /// Whether this is a PDF (always true; image docs are M5).
    #[getter]
    fn is_pdf(&self) -> bool {
        self.doc.is_pdf()
    }

    /// Whether the parse needed repair (PyMuPDF `is_repaired`).
    #[getter]
    fn is_repaired(&self) -> bool {
        self.doc.is_repaired()
    }

    /// Whether the document is encrypted (PyMuPDF `is_encrypted`).
    #[getter]
    fn is_encrypted(&self) -> bool {
        self.doc.is_encrypted()
    }

    /// Whether a password is still required (PyMuPDF `needs_pass`).
    #[getter]
    fn needs_pass(&self) -> bool {
        self.doc.needs_pass()
    }

    /// The advisory permission flags (PyMuPDF `permissions`).
    #[getter]
    fn permissions(&self) -> i32 {
        self.doc.permissions()
    }

    /// Authenticates `password` (PyMuPDF `authenticate`). Accepts `str` or
    /// `bytes`; returns `True` on success.
    fn authenticate(&self, password: &Bound<'_, PyAny>) -> PyResult<bool> {
        let pw: Vec<u8> = if let Ok(b) = password.cast::<PyBytes>() {
            b.as_bytes().to_vec()
        } else {
            password.extract::<String>()?.into_bytes()
        };
        Ok(self.doc.authenticate(&pw))
    }

    /// The document metadata as a dict with PyMuPDF keys (PRD §7 / §9.5).
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let md = self.doc.metadata();
        let d = PyDict::new(py);
        for (k, v) in md.as_pairs() {
            d.set_item(k, v)?;
        }
        Ok(d)
    }

    // --- low-level xref read API ---------------------------------------

    /// The cross-reference length (PyMuPDF `xref_length`).
    fn xref_length(&self) -> u32 {
        self.doc.xref_length()
    }

    /// The serialized source of object `num` (PyMuPDF `xref_object`).
    fn xref_object(&self, num: u32) -> PyResult<String> {
        self.doc.xref_object(num).map_err(map_err)
    }

    /// The serialized value of key `key` on object `num`, or `None`.
    fn xref_get_key(&self, num: u32, key: &str) -> PyResult<Option<String>> {
        self.doc.xref_get_key(num, key).map_err(map_err)
    }

    /// Whether object `num` is a stream (PyMuPDF `xref_is_stream`).
    fn xref_is_stream(&self, num: u32) -> PyResult<bool> {
        self.doc.xref_is_stream(num).map_err(map_err)
    }

    /// The decoded stream body of object `num` (PyMuPDF `xref_stream`).
    fn xref_stream<'py>(&self, py: Python<'py>, num: u32) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.doc.xref_stream(num).map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    fn __repr__(&self) -> String {
        format!(
            "<oxipdf._core.Document page_count={}>",
            self.doc.page_count()
        )
    }
}

// --- module-level open ----------------------------------------------------

/// Opens a document from a filesystem path (PyMuPDF `fitz.open(path)`). The heavy
/// parse runs with the GIL released (PRD §9.4).
#[pyfunction]
fn open(py: Python<'_>, path: &str) -> PyResult<PyDocument> {
    let doc = py
        .detach(|| ApiDocument::open_with(path, ParseMode::Lenient))
        .map_err(map_err)?;
    Ok(PyDocument { doc })
}

/// Opens a document from in-memory bytes (PyMuPDF `fitz.open(stream=…)`). The
/// heavy parse runs with the GIL released (PRD §9.4).
#[pyfunction]
fn open_bytes(py: Python<'_>, data: &[u8]) -> PyResult<PyDocument> {
    let owned = data.to_vec();
    let doc = py
        .detach(move || ApiDocument::open_bytes_with(owned, ParseMode::Lenient))
        .map_err(map_err)?;
    Ok(PyDocument { doc })
}

/// Returns the oxipdf version string.
#[pyfunction]
fn version() -> &'static str {
    VERSION
}

/// Returns the 6-tuple of the identity matrix `[a, b, c, d, e, f]` (geometry
/// path probe, retained from M0).
#[pyfunction]
fn identity_matrix() -> (f64, f64, f64, f64, f64, f64) {
    let m = Matrix::IDENTITY;
    (m.a, m.b, m.c, m.d, m.e, m.f)
}

/// The `_core` extension module.
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("__version__", VERSION)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(identity_matrix, m)?)?;
    m.add_function(wrap_pyfunction!(open, m)?)?;
    m.add_function(wrap_pyfunction!(open_bytes, m)?)?;

    m.add_class::<PyDocument>()?;
    m.add_class::<PyPage>()?;

    // Exception hierarchy (PRD §9.3).
    m.add("PdfError", py.get_type::<PdfError>())?;
    m.add("PdfSyntaxError", py.get_type::<PdfSyntaxError>())?;
    m.add("PdfPasswordError", py.get_type::<PdfPasswordError>())?;
    m.add("PdfUnsupportedError", py.get_type::<PdfUnsupportedError>())?;
    m.add("PdfDecodeError", py.get_type::<PdfDecodeError>())?;
    m.add("PdfLimitError", py.get_type::<PdfLimitError>())?;

    Ok(())
}
