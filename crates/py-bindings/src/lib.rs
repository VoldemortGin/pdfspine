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

use pdf_api::geom::{Matrix, Quad, Rect};
use pdf_api::{Document as ApiDocument, Error as ApiError, ParseMode, SearchOptions, TextOutput};
use pyo3::create_exception;
use pyo3::exceptions::{PyFileNotFoundError, PyIndexError, PyOSError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple};

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

// --- TextPage handle ------------------------------------------------------

/// A reusable text-extraction handle (PyMuPDF `TextPage`, PRD §9.4). Holds the
/// model built once from a [`Page`]; `Page.get_text(..., textpage=tp)` and
/// `Page.search_for(..., textpage=tp)` reuse it instead of re-parsing.
#[pyclass(name = "TextPage", module = "oxipdf._core", frozen)]
struct PyTextPage {
    page: pdf_api::Page,
    tp: pdf_api::TextPage,
}

#[pymethods]
// PyMuPDF's `TextPage.extractWORDS` / `extractDICT` etc. are camelCase by
// design — match the public API rather than renaming.
#[allow(non_snake_case)]
impl PyTextPage {
    /// The page width in device space.
    #[getter]
    fn width(&self) -> f64 {
        self.tp.width
    }

    /// The page height in device space.
    #[getter]
    fn height(&self) -> f64 {
        self.tp.height
    }

    /// Plain text (PyMuPDF `TextPage.extractText`).
    fn extractText(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        text_output_to_py(py, &self.page, "text", None, Some(&self.tp), false)
    }

    /// `words` tuples (PyMuPDF `TextPage.extractWORDS`).
    fn extractWORDS(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        text_output_to_py(py, &self.page, "words", None, Some(&self.tp), false)
    }

    /// `blocks` tuples (PyMuPDF `TextPage.extractBLOCKS`).
    fn extractBLOCKS(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        text_output_to_py(py, &self.page, "blocks", None, Some(&self.tp), false)
    }

    /// The structured dict (PyMuPDF `TextPage.extractDICT`).
    fn extractDICT(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        text_output_to_py(py, &self.page, "dict", None, Some(&self.tp), false)
    }

    /// The structured rawdict (PyMuPDF `TextPage.extractRAWDICT`).
    fn extractRAWDICT(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        text_output_to_py(py, &self.page, "rawdict", None, Some(&self.tp), false)
    }

    /// JSON string (PyMuPDF `TextPage.extractJSON`).
    fn extractJSON(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        text_output_to_py(py, &self.page, "json", None, Some(&self.tp), false)
    }

    fn __repr__(&self) -> String {
        format!(
            "<oxipdf._core.TextPage blocks={} {:.0}x{:.0}>",
            self.tp.blocks.len(),
            self.tp.width,
            self.tp.height
        )
    }
}

// --- get_text conversion (TextOutput → native Python object, PRD §9.4) ----

/// Runs `get_text` for `page`/`opt` (heavy work GIL-released) and converts the
/// neutral [`TextOutput`] into the native Python object PyMuPDF returns:
/// strings for text/markup/json; `list[tuple]` for blocks/words; `dict` for
/// dict/rawdict (PRD §9.4). `sort` orders blocks/words by `(y0, x0)`.
fn text_output_to_py(
    py: Python<'_>,
    page: &pdf_api::Page,
    opt: &str,
    flags: Option<u32>,
    tp: Option<&pdf_api::TextPage>,
    sort: bool,
) -> PyResult<Py<PyAny>> {
    // Heavy: build-or-reuse the model + serialize, GIL released (PRD §9.4).
    let out = py.detach(|| pdf_api::get_text(page, opt, flags, tp));
    // Only the final Python-object construction holds the GIL.
    match out {
        TextOutput::Text(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        TextOutput::Json(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        TextOutput::Blocks(mut v) => {
            if sort {
                v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
            }
            let list = PyList::empty(py);
            for b in v {
                let t = (b.0, b.1, b.2, b.3, b.4, b.5, b.6).into_pyobject(py)?;
                list.append(t)?;
            }
            Ok(list.into_any().unbind())
        }
        TextOutput::Words(mut v) => {
            if sort {
                v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
            }
            let list = PyList::empty(py);
            for w in v {
                let t = (w.0, w.1, w.2, w.3, w.4, w.5, w.6, w.7).into_pyobject(py)?;
                list.append(t)?;
            }
            Ok(list.into_any().unbind())
        }
        TextOutput::Dict(d) => Ok(textdict_to_py(py, &d, sort)?.into_any().unbind()),
    }
}

/// Converts a neutral [`pdf_api::TextDict`] into the real PyMuPDF-shaped Python
/// `dict`: tuples for bbox/origin/dir, `int` color, `str` text, `bytes` image
/// (empty until M5), nested `list`s of blocks/lines/spans/chars (PRD §9.4).
fn textdict_to_py<'py>(
    py: Python<'py>,
    d: &pdf_api::TextDict,
    sort: bool,
) -> PyResult<Bound<'py, PyDict>> {
    let root = PyDict::new(py);
    root.set_item("width", d.width)?;
    root.set_item("height", d.height)?;

    // Block order (optionally sorted by (y0, x0) of the block bbox).
    let mut order: Vec<usize> = (0..d.blocks.len()).collect();
    if sort {
        order.sort_by(|&i, &j| {
            let bi = block_bbox(&d.blocks[i]);
            let bj = block_bbox(&d.blocks[j]);
            bi.1.total_cmp(&bj.1).then(bi.0.total_cmp(&bj.0))
        });
    }

    let blocks = PyList::empty(py);
    for &i in &order {
        blocks.append(dict_block_to_py(py, &d.blocks[i])?)?;
    }
    root.set_item("blocks", blocks)?;
    Ok(root)
}

/// The bbox of a dict block (text or image).
fn block_bbox(b: &pdf_api::DictBlock) -> (f64, f64, f64, f64) {
    match b {
        pdf_api::DictBlock::Text(t) => t.bbox,
        pdf_api::DictBlock::Image(im) => im.bbox,
    }
}

fn dict_block_to_py<'py>(
    py: Python<'py>,
    block: &pdf_api::DictBlock,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match block {
        pdf_api::DictBlock::Text(b) => {
            d.set_item("number", b.number)?;
            d.set_item("type", 0i32)?;
            d.set_item("bbox", b.bbox)?;
            let lines = PyList::empty(py);
            for line in &b.lines {
                lines.append(dict_line_to_py(py, line)?)?;
            }
            d.set_item("lines", lines)?;
        }
        pdf_api::DictBlock::Image(b) => {
            d.set_item("number", b.number)?;
            d.set_item("type", 1i32)?;
            d.set_item("bbox", b.bbox)?;
            d.set_item("width", b.width)?;
            d.set_item("height", b.height)?;
            d.set_item("ext", &b.ext)?;
            d.set_item("colorspace", b.colorspace)?;
            d.set_item("xres", b.xres)?;
            d.set_item("yres", b.yres)?;
            d.set_item("bpc", b.bpc)?;
            d.set_item("transform", b.transform)?;
            d.set_item("size", b.size)?;
            // Image pixel bytes are deferred to M5 (empty until then).
            d.set_item("image", PyBytes::new(py, &b.image))?;
        }
    }
    Ok(d)
}

fn dict_line_to_py<'py>(py: Python<'py>, line: &pdf_api::DictLine) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("spans", {
        let spans = PyList::empty(py);
        for span in &line.spans {
            spans.append(dict_span_to_py(py, span)?)?;
        }
        spans
    })?;
    d.set_item("wmode", line.wmode)?;
    d.set_item("dir", line.dir)?;
    d.set_item("bbox", line.bbox)?;
    Ok(d)
}

fn dict_span_to_py<'py>(py: Python<'py>, span: &pdf_api::DictSpan) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("size", span.size)?;
    d.set_item("flags", span.flags)?;
    d.set_item("font", &span.font)?;
    d.set_item("color", span.color)?;
    d.set_item("ascender", span.ascender)?;
    d.set_item("descender", span.descender)?;
    d.set_item("origin", span.origin)?;
    d.set_item("bbox", span.bbox)?;
    // dict mode carries `text`; rawdict mode carries `chars`.
    if span.chars.is_empty() {
        d.set_item("text", &span.text)?;
    } else {
        let chars = PyList::empty(py);
        for ch in &span.chars {
            let c = PyDict::new(py);
            c.set_item("origin", ch.origin)?;
            c.set_item("bbox", ch.bbox)?;
            c.set_item("c", &ch.c)?;
            chars.append(c)?;
        }
        d.set_item("chars", chars)?;
    }
    Ok(d)
}

/// Converts a `Quad` into the PyMuPDF 8-tuple of corner coords
/// `(ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y)`; the Python layer wraps it
/// into a `fitz.Quad`.
fn quad_tuple(q: &Quad) -> (f64, f64, f64, f64, f64, f64, f64, f64) {
    (
        q.ul.x, q.ul.y, q.ur.x, q.ur.y, q.ll.x, q.ll.y, q.lr.x, q.lr.y,
    )
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

    // --- text extraction (PRD §8.6 / §9.4) -------------------------------

    /// Builds a reusable [`PyTextPage`] for this page (PyMuPDF
    /// `page.get_textpage`). `flags`/`clip` are accepted for API symmetry; the
    /// model is build-flag-agnostic (flags apply at serialization / search).
    #[pyo3(signature = (flags=None, clip=None))]
    fn get_textpage(
        &self,
        py: Python<'_>,
        flags: Option<u32>,
        clip: Option<(f64, f64, f64, f64)>,
    ) -> PyTextPage {
        let clip = clip.map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1, y1));
        let page = self.page.clone();
        // Heavy: interpret + layout. GIL released (PRD §9.4).
        let tp = py.detach(move || pdf_api::textpage(&page, flags.unwrap_or(0), clip));
        PyTextPage {
            page: self.page.clone(),
            tp,
        }
    }

    /// Extracts text in `option` (PyMuPDF `page.get_text`). Returns the native
    /// Python object per option: `str` for text/html/xhtml/xml/json/rawjson,
    /// `list[tuple]` for blocks/words, `dict` for dict/rawdict. `textpage`
    /// reuses a pre-built [`PyTextPage`]; `sort=True` orders blocks by `(y, x)`.
    #[pyo3(signature = (option="text", *, clip=None, flags=None, textpage=None, sort=false))]
    fn get_text(
        &self,
        py: Python<'_>,
        option: &str,
        clip: Option<(f64, f64, f64, f64)>,
        flags: Option<u32>,
        textpage: Option<&PyTextPage>,
        sort: bool,
    ) -> PyResult<Py<PyAny>> {
        let _ = clip; // clip-restricted extraction lands with textbox selection (M2 reserved).
        let tp = textpage.map(|t| &t.tp);
        text_output_to_py(py, &self.page, option, flags, tp, sort)
    }

    /// Searches the page for `needle` (PyMuPDF `page.search_for`). Returns a list
    /// of corner-coord 8-tuples (the Python layer wraps each into a `Quad` when
    /// `quads=True`, else its enclosing `Rect`). Geometry-only here; the wrapper
    /// chooses the value type.
    #[pyo3(signature = (needle, *, hit_max=0, quads=false, clip=None, flags=None, textpage=None))]
    #[allow(clippy::too_many_arguments)]
    fn search_for<'py>(
        &self,
        py: Python<'py>,
        needle: &str,
        hit_max: usize,
        quads: bool,
        clip: Option<(f64, f64, f64, f64)>,
        flags: Option<u32>,
        textpage: Option<&PyTextPage>,
    ) -> PyResult<Bound<'py, PyList>> {
        let _ = flags;
        let opts = SearchOptions {
            hit_max,
            clip: clip.map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1, y1)),
            quads,
        };
        let page = self.page.clone();
        let needle_owned = needle.to_string();
        let tp = textpage.map(|t| t.tp.clone());
        // Heavy: build-or-reuse + search, GIL released (PRD §9.4).
        let hits: Vec<Quad> =
            py.detach(move || pdf_api::search(&page, &needle_owned, opts, tp.as_ref()));
        let list = PyList::empty(py);
        for q in &hits {
            list.append(quad_tuple(q))?;
        }
        Ok(list)
    }

    // --- inventory (PRD §8.6) --------------------------------------------

    /// The page's fonts (PyMuPDF `page.get_fonts`). Returns a list of
    /// `(xref, ext, type, basefont, name, encoding, referencer)` tuples.
    #[pyo3(signature = (full=false))]
    fn get_fonts<'py>(&self, py: Python<'py>, full: bool) -> PyResult<Bound<'py, PyList>> {
        let _ = full;
        let fonts = pdf_api::get_fonts(&self.page);
        let list = PyList::empty(py);
        for f in fonts {
            let t = PyTuple::new(
                py,
                [
                    f.xref.into_pyobject(py)?.into_any(),
                    f.ext.into_pyobject(py)?.into_any(),
                    f.type_.into_pyobject(py)?.into_any(),
                    f.basefont.into_pyobject(py)?.into_any(),
                    f.name.into_pyobject(py)?.into_any(),
                    f.encoding.into_pyobject(py)?.into_any(),
                    f.referencer.into_pyobject(py)?.into_any(),
                ],
            )?;
            list.append(t)?;
        }
        Ok(list)
    }

    /// The page's images (PyMuPDF `page.get_images`). Returns a list of
    /// `(xref, smask, width, height, bpc, colorspace, alt_colorspace, name,
    /// filter, referencer)` tuples.
    #[pyo3(signature = (full=false))]
    fn get_images<'py>(&self, py: Python<'py>, full: bool) -> PyResult<Bound<'py, PyList>> {
        let _ = full;
        let images = pdf_api::get_images(&self.page);
        let list = PyList::empty(py);
        for im in images {
            let t = PyTuple::new(
                py,
                [
                    im.xref.into_pyobject(py)?.into_any(),
                    im.smask.into_pyobject(py)?.into_any(),
                    im.width.into_pyobject(py)?.into_any(),
                    im.height.into_pyobject(py)?.into_any(),
                    im.bpc.into_pyobject(py)?.into_any(),
                    im.colorspace.into_pyobject(py)?.into_any(),
                    im.alt_colorspace.into_pyobject(py)?.into_any(),
                    im.name.into_pyobject(py)?.into_any(),
                    im.filter.into_pyobject(py)?.into_any(),
                    im.referencer.into_pyobject(py)?.into_any(),
                ],
            )?;
            list.append(t)?;
        }
        Ok(list)
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

    // --- text convenience (PRD §9.5) -------------------------------------

    /// Extracts text from page `pno` (PyMuPDF `Document.get_page_text`). Loads
    /// the page, then defers to the page-level `get_text`.
    #[pyo3(signature = (pno, option="text", *, flags=None, sort=false))]
    fn get_page_text(
        &self,
        py: Python<'_>,
        pno: usize,
        option: &str,
        flags: Option<u32>,
        sort: bool,
    ) -> PyResult<Py<PyAny>> {
        let page = self.doc.load_page(pno).map_err(map_err)?;
        text_output_to_py(py, &page, option, flags, None, sort)
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
    m.add_class::<PyTextPage>()?;

    // Exception hierarchy (PRD §9.3).
    m.add("PdfError", py.get_type::<PdfError>())?;
    m.add("PdfSyntaxError", py.get_type::<PdfSyntaxError>())?;
    m.add("PdfPasswordError", py.get_type::<PdfPasswordError>())?;
    m.add("PdfUnsupportedError", py.get_type::<PdfUnsupportedError>())?;
    m.add("PdfDecodeError", py.get_type::<PdfDecodeError>())?;
    m.add("PdfLimitError", py.get_type::<PdfLimitError>())?;

    Ok(())
}
