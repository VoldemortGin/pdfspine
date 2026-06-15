// `py-bindings` is the single FFI chokepoint and the only first-party crate
// permitted to use `unsafe` (PyO3 generates FFI glue). It therefore does NOT
// `forbid(unsafe_code)`; instead it requires `unsafe` to be explicitly scoped.
#![deny(unsafe_op_in_unsafe_fn)]
//! PyO3 bindings exposing oxide_pdf's Rust core to Python as the `_core` module.
//!
//! M1f exposes the read surface (PRD §7 / §9.2 / §9.4): `open`, a `Document`
//! handle and a `Page` handle, both using the **handle/index pattern** — each
//! `#[pyclass]` is `'static` and carries its own `Arc`-backed [`pdf_api`] value,
//! never a Rust borrow. Heavy work (`open`/`open_bytes`) runs with the GIL
//! released via [`Python::detach`]. Errors map to a typed exception hierarchy
//! rooted at `_core.PdfError` (PRD §9.3).

use std::ffi::{c_int, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use pdf_api::geom::{IRect, Matrix, Point, Quad, Rect};
use pdf_api::{
    Align, AnnotHandle, Colorspace, DisplayList as ApiDisplayList, Document as ApiDocument,
    DrawItem, Drawing, Error as ApiError, FinishParams, ParseMode, Pixmap as ApiPixmap, RenderArgs,
    ScrubOptions, SearchOptions, ShapeHandle, TextOutput, WidgetHandle,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyFileNotFoundError, PyIndexError, PyOSError, PyValueError};
use pyo3::ffi;
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
create_exception!(_core, PdfRedactionError, PdfError);

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
        "redaction" => PdfRedactionError::new_err(msg),
        _ => PdfSyntaxError::new_err(msg),
    }
}

/// Maps a PyMuPDF `encryption` constant + passwords/permissions to an
/// [`pdf_api::EncryptSpec`] (PRD §8.4). `encryption` values mirror PyMuPDF:
/// `1` = RC4-128, `2` = AES-128, `3`/`4`/`6` = AES-256 (always authored as R6).
fn encrypt_spec(
    encryption: i32,
    user_pw: Option<&str>,
    owner_pw: Option<&str>,
    permissions: i32,
) -> PyResult<pdf_api::EncryptSpec> {
    let method = match encryption {
        1 => pdf_api::EncryptMethod::Rc4_128,
        2 => pdf_api::EncryptMethod::Aes128,
        3..=6 => pdf_api::EncryptMethod::Aes256R6,
        other => {
            return Err(PdfUnsupportedError::new_err(format!(
                "unsupported encryption method: {other}"
            )))
        }
    };
    Ok(pdf_api::EncryptSpec {
        user_pw: user_pw.unwrap_or("").as_bytes().to_vec(),
        owner_pw: owner_pw.unwrap_or("").as_bytes().to_vec(),
        permissions,
        method,
    })
}

/// Builds the `pdf_api::SaveOptions` for a save call from PyMuPDF-style kwargs.
fn build_save_opts(
    garbage: u8,
    deflate: bool,
    encryption: Option<i32>,
    user_pw: Option<&str>,
    owner_pw: Option<&str>,
    permissions: i32,
) -> PyResult<pdf_api::SaveOptions> {
    let mut opts = pdf_api::SaveOptions::default()
        .with_garbage(garbage)
        .with_deflate(deflate);
    if let Some(enc) = encryption {
        if enc != 0 {
            let spec = encrypt_spec(enc, user_pw, owner_pw, permissions)?;
            opts = opts.with_encrypt(spec);
        }
    }
    Ok(opts)
}

// --- Page handle ----------------------------------------------------------

/// A page handle (PRD §9.2). Holds a cloned `pdf_api::Page` (its own `Arc` onto
/// the document store) — `'static`, no borrow crosses the boundary.
#[pyclass(name = "Page", module = "oxide_pdf._core", frozen)]
struct PyPage {
    page: pdf_api::Page,
}

/// Converts a `Rect` to the 4-tuple `(x0, y0, x1, y1)` the Python layer wraps.
fn rect_tuple(r: pdf_api::Rect) -> (f64, f64, f64, f64) {
    (r.x0, r.y0, r.x1, r.y1)
}

/// Builds a [`Rect`] from the `(x0, y0, x1, y1)` 4-tuple the Python layer sends.
fn rect_of(t: (f64, f64, f64, f64)) -> Rect {
    Rect::new(t.0, t.1, t.2, t.3)
}

/// Builds a [`Point`] from the `(x, y)` 2-tuple the Python layer sends.
fn point_of(t: (f64, f64)) -> Point {
    Point::new(t.0, t.1)
}

/// Converts a `Point` to the `(x, y)` 2-tuple the Python layer wraps.
fn point_tuple(p: Point) -> (f64, f64) {
    (p.x, p.y)
}

/// The corner-coord 8-tuple `(ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y)`
/// the Python layer sends for a [`Quad`].
type QuadTuple = (f64, f64, f64, f64, f64, f64, f64, f64);

/// Builds a [`Quad`] from the corner-coord 8-tuple
/// `(ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y)` the Python layer sends.
fn quad_of(t: QuadTuple) -> Quad {
    Quad {
        ul: Point::new(t.0, t.1),
        ur: Point::new(t.2, t.3),
        ll: Point::new(t.4, t.5),
        lr: Point::new(t.6, t.7),
    }
}

/// Maps a packed `0x00RRGGBB` color to the PyMuPDF `(r, g, b)` float tuple.
fn unpack_color(rgb: u32) -> (f64, f64, f64) {
    (
        f64::from((rgb >> 16) & 0xff) / 255.0,
        f64::from((rgb >> 8) & 0xff) / 255.0,
        f64::from(rgb & 0xff) / 255.0,
    )
}

// --- TextPage handle ------------------------------------------------------

/// A reusable text-extraction handle (PyMuPDF `TextPage`, PRD §9.4). Holds the
/// model built once from a [`Page`]; `Page.get_text(..., textpage=tp)` and
/// `Page.search_for(..., textpage=tp)` reuse it instead of re-parsing.
#[pyclass(name = "TextPage", module = "oxide_pdf._core", frozen)]
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
            "<oxide_pdf._core.TextPage blocks={} {:.0}x{:.0}>",
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

// --- Annot handle (PRD §8.8 / §9.4) ---------------------------------------

/// An annotation handle (PyMuPDF `Annot`). Owns an `AnnotHandle` (its own
/// `Arc` onto the store + the annot xref) — `'static`, no borrow crosses the
/// boundary.
#[pyclass(name = "Annot", module = "oxide_pdf._core", frozen)]
struct PyAnnot {
    annot: AnnotHandle,
}

#[pymethods]
impl PyAnnot {
    /// The annotation object number (PyMuPDF `Annot.xref`).
    #[getter]
    fn xref(&self) -> u32 {
        self.annot.xref()
    }

    /// The annotation `/Rect` as `(x0, y0, x1, y1)` (PyMuPDF `Annot.rect`).
    #[getter]
    fn rect(&self) -> (f64, f64, f64, f64) {
        rect_tuple(self.annot.rect())
    }

    /// The annotation subtype name string, e.g. `"Highlight"` (PyMuPDF
    /// `Annot.type` is `(int, str)`; the Python layer builds the pair).
    #[getter]
    fn type_string(&self) -> String {
        self.annot.type_string()
    }

    /// The annotation info dict `{content, name, title, …}` (PyMuPDF
    /// `Annot.info`).
    fn info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let info = self.annot.info();
        let d = PyDict::new(py);
        d.set_item("content", info.content)?;
        d.set_item("name", info.name)?;
        d.set_item("title", info.title)?;
        Ok(d)
    }

    /// The `{stroke, fill}` color dict (PyMuPDF `Annot.colors`); each value is an
    /// `(r, g, b)` tuple or `None`.
    fn colors<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let (stroke, fill) = self.annot.colors();
        let d = PyDict::new(py);
        d.set_item("stroke", stroke)?;
        d.set_item("fill", fill)?;
        Ok(d)
    }

    /// The constant opacity `/CA` (PyMuPDF `Annot.opacity`).
    #[getter]
    fn opacity(&self) -> f64 {
        self.annot.opacity()
    }

    /// The border width (PyMuPDF `Annot.border` is a dict; the Python layer
    /// wraps this scalar width).
    #[getter]
    fn border_width(&self) -> f64 {
        self.annot.border_width()
    }

    /// The annotation flags `/F` (PyMuPDF `Annot.flags`).
    #[getter]
    fn flags(&self) -> i64 {
        self.annot.flags()
    }

    /// The `/Vertices` as `(x, y)` tuples (PyMuPDF `Annot.vertices`).
    fn vertices(&self) -> Vec<(f64, f64)> {
        self.annot.vertices().into_iter().map(point_tuple).collect()
    }

    /// Whether an `/AP /N` appearance stream is present (PyMuPDF
    /// `Annot.has_appearance`-style check).
    #[getter]
    fn has_appearance(&self) -> bool {
        self.annot.has_appearance()
    }

    /// Sets the `/Rect` (PyMuPDF `Annot.set_rect`).
    fn set_rect(&self, rect: (f64, f64, f64, f64)) -> PyResult<()> {
        self.annot.set_rect(rect_of(rect)).map_err(map_err)
    }

    /// Sets stroke/fill colors (PyMuPDF `Annot.set_colors`). Each `None` leaves
    /// the key untouched.
    #[pyo3(signature = (stroke=None, fill=None))]
    fn set_colors(
        &self,
        stroke: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
    ) -> PyResult<()> {
        self.annot.set_colors(stroke, fill).map_err(map_err)
    }

    /// Sets the constant opacity `/CA` (PyMuPDF `Annot.set_opacity`).
    fn set_opacity(&self, opacity: f64) -> PyResult<()> {
        self.annot.set_opacity(opacity).map_err(map_err)
    }

    /// Sets the border width (PyMuPDF `Annot.set_border`).
    #[pyo3(signature = (width=1.0))]
    fn set_border(&self, width: f64) -> PyResult<()> {
        self.annot.set_border(width).map_err(map_err)
    }

    /// Sets the annotation flags `/F` (PyMuPDF `Annot.set_flags`).
    fn set_flags(&self, flags: i64) -> PyResult<()> {
        self.annot.set_flags(flags).map_err(map_err)
    }

    /// Sets info fields (PyMuPDF `Annot.set_info`). Each `None` leaves the key
    /// untouched.
    #[pyo3(signature = (content=None, title=None, name=None))]
    fn set_info(
        &self,
        content: Option<&str>,
        title: Option<&str>,
        name: Option<&str>,
    ) -> PyResult<()> {
        self.annot.set_info(content, title, name).map_err(map_err)
    }

    /// Regenerates the `/AP /N` appearance stream (PyMuPDF `Annot.update`).
    fn update(&self) -> PyResult<()> {
        self.annot.update().map_err(map_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "<oxide_pdf._core.Annot type={} xref={}>",
            self.annot.type_string(),
            self.annot.xref()
        )
    }
}

// --- Widget handle (PRD §8.8 / §9.4) --------------------------------------

/// A form-widget handle (PyMuPDF `Widget`). Owns a `WidgetHandle`.
#[pyclass(name = "Widget", module = "oxide_pdf._core", frozen)]
struct PyWidget {
    widget: WidgetHandle,
}

#[pymethods]
impl PyWidget {
    /// The widget object number (PyMuPDF `Widget.xref`).
    #[getter]
    fn xref(&self) -> u32 {
        self.widget.xref()
    }

    /// The widget `/Rect` as `(x0, y0, x1, y1)` (PyMuPDF `Widget.rect`).
    #[getter]
    fn rect(&self) -> (f64, f64, f64, f64) {
        rect_tuple(self.widget.rect())
    }

    /// The PyMuPDF field-type integer (PyMuPDF `Widget.field_type`).
    #[getter]
    fn field_type(&self) -> i32 {
        field_type_int(self.widget.field_type())
    }

    /// The field-type string (PyMuPDF `Widget.field_type_string`).
    #[getter]
    fn field_type_string(&self) -> String {
        self.widget.field_type_string()
    }

    /// The fully-qualified field name (PyMuPDF `Widget.field_name`).
    #[getter]
    fn field_name(&self) -> Option<String> {
        let n = self.widget.field_name();
        if n.is_empty() {
            None
        } else {
            Some(n)
        }
    }

    /// The field label `/TU` (PyMuPDF `Widget.field_label`).
    #[getter]
    fn field_label(&self) -> Option<String> {
        self.widget.field_label()
    }

    /// The current field value (PyMuPDF `Widget.field_value`).
    #[getter]
    fn field_value(&self) -> Option<String> {
        self.widget.field_value()
    }

    /// The field flags `/Ff` (PyMuPDF `Widget.field_flags`).
    #[getter]
    fn field_flags(&self) -> i64 {
        self.widget.field_flags()
    }

    /// The choice option values (PyMuPDF `Widget.choice_values`).
    #[getter]
    fn choice_values(&self) -> Vec<String> {
        self.widget.choice_values()
    }

    /// The checkbox/radio on-state names (PyMuPDF `Widget.button_states`).
    #[getter]
    fn button_states(&self) -> Vec<String> {
        self.widget.button_states()
    }

    /// Sets the field value (PyMuPDF `Widget.field_value = …`).
    fn set_field_value(&self, value: &str) -> PyResult<()> {
        self.widget.set_field_value(value).map_err(map_err)
    }

    /// Writes the field value back, regenerating appearances (PyMuPDF
    /// `Widget.update`). The Python layer sets `field_value` then calls this.
    #[pyo3(signature = (value=None))]
    fn update(&self, value: Option<&str>) -> PyResult<()> {
        if let Some(v) = value {
            self.widget.set_field_value(v).map_err(map_err)?;
        }
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!(
            "<oxide_pdf._core.Widget field={:?} xref={}>",
            self.widget.field_name(),
            self.widget.xref()
        )
    }
}

/// Maps a [`pdf_api::FieldType`] to the PyMuPDF `PDF_WIDGET_TYPE_*` integer.
fn field_type_int(ft: pdf_api::FieldType) -> i32 {
    use pdf_api::FieldType as F;
    match ft {
        F::Unknown => 0,
        F::PushButton => 1,
        F::CheckBox => 2,
        F::RadioButton => 3,
        F::Text => 4,
        F::ListBox => 5,
        F::ComboBox => 6,
        F::Signature => 7,
    }
}

// --- Shape handle (PRD §8.8 / §9.4) ---------------------------------------

/// A path/paint builder over one page (PyMuPDF `Shape`). Wraps the owned
/// [`ShapeHandle`] in an `Option` so `commit` (which consumes the handle) can
/// take it out of the `&mut self`.
#[pyclass(name = "Shape", module = "oxide_pdf._core")]
struct PyShape {
    shape: Option<ShapeHandle>,
}

impl PyShape {
    /// Mutable access to the live handle, erroring once committed.
    fn handle(&mut self) -> PyResult<&mut ShapeHandle> {
        self.shape
            .as_mut()
            .ok_or_else(|| PdfError::new_err("Shape already committed"))
    }
}

#[pymethods]
impl PyShape {
    /// Records a straight segment (PyMuPDF `Shape.draw_line`).
    fn draw_line(&mut self, p1: (f64, f64), p2: (f64, f64)) -> PyResult<()> {
        self.handle()?.draw_line(point_of(p1), point_of(p2));
        Ok(())
    }

    /// Records a rectangle (PyMuPDF `Shape.draw_rect`).
    fn draw_rect(&mut self, rect: (f64, f64, f64, f64)) -> PyResult<()> {
        self.handle()?.draw_rect(rect_of(rect));
        Ok(())
    }

    /// Records a circle (PyMuPDF `Shape.draw_circle`).
    fn draw_circle(&mut self, center: (f64, f64), radius: f64) -> PyResult<()> {
        self.handle()?.draw_circle(point_of(center), radius);
        Ok(())
    }

    /// Records an ellipse fitting `rect` (PyMuPDF `Shape.draw_oval`).
    fn draw_oval(&mut self, rect: (f64, f64, f64, f64)) -> PyResult<()> {
        self.handle()?.draw_oval(rect_of(rect));
        Ok(())
    }

    /// Records a cubic Bézier (PyMuPDF `Shape.draw_bezier`).
    fn draw_bezier(
        &mut self,
        p1: (f64, f64),
        p2: (f64, f64),
        p3: (f64, f64),
        p4: (f64, f64),
    ) -> PyResult<()> {
        self.handle()?
            .draw_bezier(point_of(p1), point_of(p2), point_of(p3), point_of(p4));
        Ok(())
    }

    /// Records a polyline (PyMuPDF `Shape.draw_polyline`).
    fn draw_polyline(&mut self, points: Vec<(f64, f64)>) -> PyResult<()> {
        let pts: Vec<Point> = points.into_iter().map(point_of).collect();
        self.handle()?.draw_polyline(pts);
        Ok(())
    }

    /// Records a smooth curve (PyMuPDF `Shape.draw_curve`).
    fn draw_curve(&mut self, points: Vec<(f64, f64)>) -> PyResult<()> {
        let pts: Vec<Point> = points.into_iter().map(point_of).collect();
        self.handle()?.draw_curve(pts);
        Ok(())
    }

    /// Finishes the current styled block (PyMuPDF `Shape.finish`).
    #[pyo3(signature = (color=None, fill=None, width=1.0, dashes=None, even_odd=false, close_path=false))]
    fn finish(
        &mut self,
        color: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
        width: f64,
        dashes: Option<String>,
        even_odd: bool,
        close_path: bool,
    ) -> PyResult<()> {
        self.handle()?.finish(FinishParams {
            color,
            fill,
            width,
            dashes,
            even_odd,
            close_path,
        });
        Ok(())
    }

    /// Writes all recorded blocks to the page (PyMuPDF `Shape.commit`). Heavy
    /// work runs with the GIL released.
    #[pyo3(signature = (overlay=true))]
    fn commit(&mut self, py: Python<'_>, overlay: bool) -> PyResult<()> {
        let _ = overlay;
        let handle = self
            .shape
            .take()
            .ok_or_else(|| PdfError::new_err("Shape already committed"))?;
        py.detach(|| handle.commit()).map_err(map_err)
    }
}

/// Converts a [`Drawing`] to the PyMuPDF `get_drawings` dict shape: `type`,
/// `rect`, `color`, `fill`, `width`, `dashes`, `closePath`, `even_odd`, and
/// `items` (a list of operator tuples).
fn drawing_to_py<'py>(py: Python<'py>, d: &Drawing) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("type", d.type_str())?;
    out.set_item("rect", rect_tuple(d.rect))?;
    out.set_item("color", d.color.map(unpack_color))?;
    out.set_item("fill", d.fill.map(unpack_color))?;
    out.set_item("width", d.width)?;
    out.set_item("dashes", &d.dashes)?;
    out.set_item("closePath", d.close_path)?;
    out.set_item("even_odd", d.even_odd)?;
    let items = PyList::empty(py);
    for it in &d.items {
        match it {
            DrawItem::Line(a, b) => {
                let t = ("l", point_tuple(*a), point_tuple(*b)).into_pyobject(py)?;
                items.append(t)?;
            }
            DrawItem::Curve(a, b, c, e) => {
                let t = (
                    "c",
                    point_tuple(*a),
                    point_tuple(*b),
                    point_tuple(*c),
                    point_tuple(*e),
                )
                    .into_pyobject(py)?;
                items.append(t)?;
            }
            DrawItem::Rect(r) => {
                let t = ("re", rect_tuple(*r)).into_pyobject(py)?;
                items.append(t)?;
            }
        }
    }
    out.set_item("items", items)?;
    Ok(out)
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

    // --- get_pixmap (PRD §3.3 / §8.10) -----------------------------------

    /// Renders the page to a [`PyPixmap`] (PyMuPDF `Page.get_pixmap`, PRD §8.11).
    ///
    /// Any page renders: an image-only page takes the fast native-raster path;
    /// vector / text / mixed pages are rasterized full-page via `pdf_render`.
    /// `matrix` is a `(a, b, c, d, e, f)` tuple (scale/rotate); `dpi` overrides it
    /// with `dpi/72`. `colorspace` selects Gray/RGB/CMYK output; `alpha` adds an
    /// alpha channel; `clip` is a device-space `(x0, y0, x1, y1)` sub-rectangle.
    #[pyo3(signature = (*, matrix=None, dpi=None, colorspace=None, alpha=false, clip=None))]
    fn get_pixmap(
        &self,
        py: Python<'_>,
        matrix: Option<(f64, f64, f64, f64, f64, f64)>,
        dpi: Option<f64>,
        colorspace: Option<Bound<'_, PyAny>>,
        alpha: bool,
        clip: Option<(f64, f64, f64, f64)>,
    ) -> PyResult<PyPixmap> {
        let args = build_render_args(matrix, dpi, colorspace, alpha, clip)?;
        let pix = py
            .detach(|| pdf_api::page_render(&self.page, &args))
            .map_err(map_err)?;
        Ok(PyPixmap::new(pix))
    }

    /// Records the page's ordered drawcall stream into a [`PyDisplayList`]
    /// (PyMuPDF `Page.get_displaylist`). Replay it with `dl.get_pixmap(...)`.
    fn get_displaylist(&self, py: Python<'_>) -> PyResult<PyDisplayList> {
        let inner = py.detach(|| pdf_api::page_get_displaylist(&self.page));
        Ok(PyDisplayList {
            inner: Arc::new(inner),
        })
    }

    /// Whether this page is an image-only page (in scope for `get_pixmap`).
    #[getter]
    fn is_image_only(&self) -> bool {
        pdf_api::page_is_image_only(&self.page)
    }

    // --- links + label + rotation (PRD §8.9) -----------------------------

    /// The link annotations on this page (PyMuPDF `Page.get_links`). Each link is
    /// a dict with `kind` (0=none, 1=goto, 2=uri), `from` (rect tuple), and
    /// `uri`/`page` as applicable, plus `xref`.
    fn get_links<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let links = pdf_api::page_get_links(&self.page);
        let out = PyList::empty(py);
        for link in links {
            let d = PyDict::new(py);
            match link.kind {
                pdf_api::LinkKind::Uri(uri) => {
                    d.set_item("kind", 2)?;
                    d.set_item("uri", uri)?;
                }
                pdf_api::LinkKind::Goto(page) => {
                    d.set_item("kind", 1)?;
                    d.set_item("page", page)?;
                }
                pdf_api::LinkKind::None => {
                    d.set_item("kind", 0)?;
                }
            }
            d.set_item("from", rect_tuple(link.from))?;
            d.set_item("xref", link.xref)?;
            out.append(d)?;
        }
        Ok(out)
    }

    /// Inserts a link. `link` is a dict with `kind` (1=goto, 2=uri), `from`
    /// (4-tuple rect), and `uri` or `page` (PyMuPDF `Page.insert_link`).
    fn insert_link(&self, link: &Bound<'_, PyDict>) -> PyResult<()> {
        let from: (f64, f64, f64, f64) = link
            .get_item("from")?
            .ok_or_else(|| PdfError::new_err("insert_link requires 'from' rect"))?
            .extract()?;
        let rect = Rect::new(from.0, from.1, from.2, from.3);
        let kind: i32 = link
            .get_item("kind")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(2);
        match kind {
            2 => {
                let uri: String = link
                    .get_item("uri")?
                    .ok_or_else(|| PdfError::new_err("uri link requires 'uri'"))?
                    .extract()?;
                pdf_api::page_insert_link_uri(&self.page, rect, &uri).map_err(map_err)
            }
            1 => {
                let page: i32 = link
                    .get_item("page")?
                    .ok_or_else(|| PdfError::new_err("goto link requires 'page'"))?
                    .extract()?;
                pdf_api::page_insert_link_goto(&self.page, rect, page).map_err(map_err)
            }
            other => Err(PdfUnsupportedError::new_err(format!(
                "unsupported link kind: {other}"
            ))),
        }
    }

    /// Deletes a link annotation by its `xref` (PyMuPDF `Page.delete_link`).
    fn delete_link(&self, xref: u32) -> PyResult<()> {
        pdf_api::page_delete_link(&self.page, xref).map_err(map_err)
    }

    /// The page label (PyMuPDF `Page.get_label`).
    fn get_label(&self) -> String {
        pdf_api::page_get_label(&self.page)
    }

    /// Sets the page rotation (PyMuPDF `Page.set_rotation`).
    fn set_rotation(&self, rotation: i64) -> PyResult<()> {
        pdf_api::page_set_rotation(&self.page, rotation).map_err(map_err)
    }

    // --- content insertion (PRD §8.8 / §9.4) -----------------------------

    /// Inserts `text` at `point` (PyMuPDF `Page.insert_text`). Heavy work runs
    /// with the GIL released. Returns the number of lines written.
    #[pyo3(signature = (point, text, *, fontname="helv", fontsize=11.0, color=(0.0,0.0,0.0), fontfile=None))]
    #[allow(clippy::too_many_arguments)]
    fn insert_text(
        &self,
        py: Python<'_>,
        point: (f64, f64),
        text: &str,
        fontname: &str,
        fontsize: f64,
        color: (f64, f64, f64),
        fontfile: Option<Vec<u8>>,
    ) -> PyResult<usize> {
        let page = self.page.clone();
        let text = text.to_string();
        let fontname = fontname.to_string();
        py.detach(move || {
            pdf_api::page_insert_text(
                &page,
                point_of(point),
                &text,
                &fontname,
                fontsize,
                color,
                fontfile.as_deref(),
            )
        })
        .map_err(map_err)
    }

    /// Inserts wrapped `text` into `rect` (PyMuPDF `Page.insert_textbox`).
    /// `align`: 0=left, 1=center, 2=right, 3=justify. Returns free height.
    #[pyo3(signature = (rect, text, *, fontname="helv", fontsize=11.0, color=(0.0,0.0,0.0), align=0, fontfile=None))]
    #[allow(clippy::too_many_arguments)]
    fn insert_textbox(
        &self,
        py: Python<'_>,
        rect: (f64, f64, f64, f64),
        text: &str,
        fontname: &str,
        fontsize: f64,
        color: (f64, f64, f64),
        align: i32,
        fontfile: Option<Vec<u8>>,
    ) -> PyResult<f64> {
        let page = self.page.clone();
        let text = text.to_string();
        let fontname = fontname.to_string();
        let align = align_of(align);
        py.detach(move || {
            pdf_api::page_insert_textbox(
                &page,
                rect_of(rect),
                &text,
                &fontname,
                fontsize,
                color,
                align,
                fontfile.as_deref(),
            )
        })
        .map_err(map_err)
    }

    /// Inserts an image into `rect` (PyMuPDF `Page.insert_image`). `stream` is
    /// the image bytes; JPEG is passthrough, otherwise raw RGB requires
    /// `width`/`height`. Heavy work runs with the GIL released.
    #[pyo3(signature = (rect, *, stream, width=None, height=None))]
    fn insert_image(
        &self,
        py: Python<'_>,
        rect: (f64, f64, f64, f64),
        stream: Vec<u8>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> PyResult<()> {
        let page = self.page.clone();
        py.detach(move || -> Result<(), ApiError> {
            match (width, height) {
                (Some(w), Some(h)) => {
                    pdf_api::page_insert_image_rgb(&page, rect_of(rect), w, h, &stream)?;
                }
                _ => {
                    pdf_api::page_insert_image_jpeg(&page, rect_of(rect), &stream)?;
                }
            }
            Ok(())
        })
        .map_err(map_err)
    }

    // --- vector drawing (PRD §8.8) ---------------------------------------

    /// Draws a line (PyMuPDF `Page.draw_line`).
    #[pyo3(signature = (p1, p2, *, color=(0.0,0.0,0.0), width=1.0))]
    fn draw_line(
        &self,
        p1: (f64, f64),
        p2: (f64, f64),
        color: (f64, f64, f64),
        width: f64,
    ) -> PyResult<()> {
        pdf_api::page_draw_line(&self.page, point_of(p1), point_of(p2), color, width)
            .map_err(map_err)
    }

    /// Draws a rectangle (PyMuPDF `Page.draw_rect`).
    #[pyo3(signature = (rect, *, color=(0.0,0.0,0.0), fill=None, width=1.0))]
    fn draw_rect(
        &self,
        rect: (f64, f64, f64, f64),
        color: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
        width: f64,
    ) -> PyResult<()> {
        pdf_api::page_draw_rect(&self.page, rect_of(rect), color, fill, width).map_err(map_err)
    }

    /// Draws a circle (PyMuPDF `Page.draw_circle`).
    #[pyo3(signature = (center, radius, *, color=(0.0,0.0,0.0), fill=None, width=1.0))]
    fn draw_circle(
        &self,
        center: (f64, f64),
        radius: f64,
        color: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
        width: f64,
    ) -> PyResult<()> {
        pdf_api::page_draw_circle(&self.page, point_of(center), radius, color, fill, width)
            .map_err(map_err)
    }

    /// Draws an oval fitting `rect` (PyMuPDF `Page.draw_oval`).
    #[pyo3(signature = (rect, *, color=(0.0,0.0,0.0), fill=None, width=1.0))]
    fn draw_oval(
        &self,
        rect: (f64, f64, f64, f64),
        color: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
        width: f64,
    ) -> PyResult<()> {
        pdf_api::page_draw_oval(&self.page, rect_of(rect), color, fill, width).map_err(map_err)
    }

    /// Draws a cubic Bézier (PyMuPDF `Page.draw_bezier`).
    #[pyo3(signature = (p1, p2, p3, p4, *, color=(0.0,0.0,0.0), width=1.0))]
    #[allow(clippy::too_many_arguments)]
    fn draw_bezier(
        &self,
        p1: (f64, f64),
        p2: (f64, f64),
        p3: (f64, f64),
        p4: (f64, f64),
        color: (f64, f64, f64),
        width: f64,
    ) -> PyResult<()> {
        pdf_api::page_draw_bezier(
            &self.page,
            point_of(p1),
            point_of(p2),
            point_of(p3),
            point_of(p4),
            color,
            width,
        )
        .map_err(map_err)
    }

    /// Draws a polyline (PyMuPDF `Page.draw_polyline`).
    #[pyo3(signature = (points, *, color=(0.0,0.0,0.0), width=1.0))]
    fn draw_polyline(
        &self,
        points: Vec<(f64, f64)>,
        color: (f64, f64, f64),
        width: f64,
    ) -> PyResult<()> {
        let pts: Vec<Point> = points.into_iter().map(point_of).collect();
        pdf_api::page_draw_polyline(&self.page, &pts, color, width).map_err(map_err)
    }

    /// Begins a [`PyShape`] over this page (PyMuPDF `Page.new_shape`).
    fn new_shape(&self) -> PyShape {
        PyShape {
            shape: Some(pdf_api::page_new_shape(&self.page)),
        }
    }

    // --- annotations (PRD §8.8) ------------------------------------------

    /// Adds a `/Text` (sticky-note) annotation (PyMuPDF `Page.add_text_annot`).
    #[pyo3(signature = (point, text, *, icon="Note"))]
    fn add_text_annot(&self, point: (f64, f64), text: &str, icon: &str) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_text_annot(
            &self.page,
            point_of(point),
            text,
            icon,
        ))
    }

    /// Adds a `/FreeText` annotation (PyMuPDF `Page.add_freetext_annot`).
    #[pyo3(signature = (rect, text, *, fontsize=11.0, text_color=(0.0,0.0,0.0), fill_color=None, align=0))]
    fn add_freetext_annot(
        &self,
        rect: (f64, f64, f64, f64),
        text: &str,
        fontsize: f64,
        text_color: (f64, f64, f64),
        fill_color: Option<(f64, f64, f64)>,
        align: i64,
    ) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_freetext_annot(
            &self.page,
            rect_of(rect),
            text,
            fontsize,
            text_color,
            fill_color,
            align,
        ))
    }

    /// Adds a `/Highlight` annotation over `quads` (PyMuPDF
    /// `Page.add_highlight_annot`).
    fn add_highlight_annot(&self, quads: Vec<QuadTuple>) -> PyResult<PyAnnot> {
        let qs: Vec<Quad> = quads.into_iter().map(quad_of).collect();
        wrap_annot(pdf_api::page_add_highlight_annot(&self.page, &qs))
    }

    /// Adds an `/Underline` annotation (PyMuPDF `Page.add_underline_annot`).
    fn add_underline_annot(&self, quads: Vec<QuadTuple>) -> PyResult<PyAnnot> {
        let qs: Vec<Quad> = quads.into_iter().map(quad_of).collect();
        wrap_annot(pdf_api::page_add_underline_annot(&self.page, &qs))
    }

    /// Adds a `/StrikeOut` annotation (PyMuPDF `Page.add_strikeout_annot`).
    fn add_strikeout_annot(&self, quads: Vec<QuadTuple>) -> PyResult<PyAnnot> {
        let qs: Vec<Quad> = quads.into_iter().map(quad_of).collect();
        wrap_annot(pdf_api::page_add_strikeout_annot(&self.page, &qs))
    }

    /// Adds a `/Squiggly` annotation (PyMuPDF `Page.add_squiggly_annot`).
    fn add_squiggly_annot(&self, quads: Vec<QuadTuple>) -> PyResult<PyAnnot> {
        let qs: Vec<Quad> = quads.into_iter().map(quad_of).collect();
        wrap_annot(pdf_api::page_add_squiggly_annot(&self.page, &qs))
    }

    /// Adds a `/Square` (rect) annotation (PyMuPDF `Page.add_rect_annot`).
    #[pyo3(signature = (rect, *, color=(0.0,0.0,0.0), fill=None))]
    fn add_rect_annot(
        &self,
        rect: (f64, f64, f64, f64),
        color: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
    ) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_rect_annot(
            &self.page,
            rect_of(rect),
            color,
            fill,
        ))
    }

    /// Adds a `/Circle` annotation (PyMuPDF `Page.add_circle_annot`).
    #[pyo3(signature = (rect, *, color=(0.0,0.0,0.0), fill=None))]
    fn add_circle_annot(
        &self,
        rect: (f64, f64, f64, f64),
        color: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
    ) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_circle_annot(
            &self.page,
            rect_of(rect),
            color,
            fill,
        ))
    }

    /// Adds a `/Line` annotation (PyMuPDF `Page.add_line_annot`).
    #[pyo3(signature = (p1, p2, *, color=(0.0,0.0,0.0)))]
    fn add_line_annot(
        &self,
        p1: (f64, f64),
        p2: (f64, f64),
        color: Option<(f64, f64, f64)>,
    ) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_line_annot(
            &self.page,
            point_of(p1),
            point_of(p2),
            color,
        ))
    }

    /// Adds a `/Polygon` annotation (PyMuPDF `Page.add_polygon_annot`).
    #[pyo3(signature = (points, *, color=(0.0,0.0,0.0), fill=None))]
    fn add_polygon_annot(
        &self,
        points: Vec<(f64, f64)>,
        color: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
    ) -> PyResult<PyAnnot> {
        let pts: Vec<Point> = points.into_iter().map(point_of).collect();
        wrap_annot(pdf_api::page_add_polygon_annot(
            &self.page, &pts, color, fill,
        ))
    }

    /// Adds a `/PolyLine` annotation (PyMuPDF `Page.add_polyline_annot`).
    #[pyo3(signature = (points, *, color=(0.0,0.0,0.0)))]
    fn add_polyline_annot(
        &self,
        points: Vec<(f64, f64)>,
        color: Option<(f64, f64, f64)>,
    ) -> PyResult<PyAnnot> {
        let pts: Vec<Point> = points.into_iter().map(point_of).collect();
        wrap_annot(pdf_api::page_add_polyline_annot(&self.page, &pts, color))
    }

    /// Adds an `/Ink` annotation (PyMuPDF `Page.add_ink_annot`). `strokes` is a
    /// list of point lists.
    #[pyo3(signature = (strokes, *, color=(0.0,0.0,0.0)))]
    fn add_ink_annot(
        &self,
        strokes: Vec<Vec<(f64, f64)>>,
        color: Option<(f64, f64, f64)>,
    ) -> PyResult<PyAnnot> {
        let ss: Vec<Vec<Point>> = strokes
            .into_iter()
            .map(|s| s.into_iter().map(point_of).collect())
            .collect();
        wrap_annot(pdf_api::page_add_ink_annot(&self.page, &ss, color))
    }

    /// Adds a `/Stamp` annotation (PyMuPDF `Page.add_stamp_annot`).
    #[pyo3(signature = (rect, *, stamp="Approved"))]
    fn add_stamp_annot(&self, rect: (f64, f64, f64, f64), stamp: &str) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_stamp_annot(
            &self.page,
            rect_of(rect),
            stamp,
        ))
    }

    /// Adds a `/FileAttachment` annotation embedding `bytes` (PyMuPDF
    /// `Page.add_file_annot`).
    fn add_file_annot(
        &self,
        point: (f64, f64),
        bytes: Vec<u8>,
        filename: &str,
    ) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_file_annot(
            &self.page,
            point_of(point),
            &bytes,
            filename,
        ))
    }

    /// Adds a `/Redact` annotation over `rect` (PyMuPDF `Page.add_redact_annot`).
    #[pyo3(signature = (rect, *, fill=None, text=None))]
    fn add_redact_annot(
        &self,
        rect: (f64, f64, f64, f64),
        fill: Option<(f64, f64, f64)>,
        text: Option<&str>,
    ) -> PyResult<PyAnnot> {
        wrap_annot(pdf_api::page_add_redact_annot(
            &self.page,
            rect_of(rect),
            fill,
            text,
        ))
    }

    /// The annotations on this page (PyMuPDF `Page.annots`).
    fn annots(&self) -> PyResult<Vec<PyAnnot>> {
        let handles = pdf_api::page_annots(&self.page).map_err(map_err)?;
        Ok(handles.into_iter().map(|annot| PyAnnot { annot }).collect())
    }

    /// The first annotation, or `None` (PyMuPDF `Page.first_annot`).
    #[getter]
    fn first_annot(&self) -> PyResult<Option<PyAnnot>> {
        let h = pdf_api::page_first_annot(&self.page).map_err(map_err)?;
        Ok(h.map(|annot| PyAnnot { annot }))
    }

    /// The annotation xrefs on this page (PyMuPDF `Page.annot_xrefs`).
    fn annot_xrefs(&self) -> Vec<u32> {
        pdf_api::page_annot_xrefs(&self.page)
    }

    /// The annotation names on this page (PyMuPDF `Page.annot_names`).
    fn annot_names(&self) -> Vec<String> {
        pdf_api::page_annot_names(&self.page)
    }

    /// Deletes the annotation `xref` (PyMuPDF `Page.delete_annot`).
    fn delete_annot(&self, xref: u32) -> PyResult<()> {
        pdf_api::page_delete_annot(&self.page, xref).map_err(map_err)
    }

    // --- redaction / drawings (PRD §8.8) ---------------------------------

    /// Applies redaction annotations destructively (PyMuPDF
    /// `Page.apply_redactions`). Heavy work runs with the GIL released. Returns
    /// the number of redactions applied.
    #[pyo3(signature = (*_args, **_kwargs))]
    fn apply_redactions(
        &self,
        py: Python<'_>,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<usize> {
        let page = self.page.clone();
        py.detach(move || pdf_api::page_apply_redactions(&page))
            .map_err(map_err)
    }

    /// The vector drawings on this page in device space (PyMuPDF
    /// `Page.get_drawings`). Returns a list of dicts.
    fn get_drawings<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let page = self.page.clone();
        let drawings = py.detach(move || pdf_api::page_get_drawings(&page));
        let out = PyList::empty(py);
        for d in &drawings {
            out.append(drawing_to_py(py, d)?)?;
        }
        Ok(out)
    }

    /// The raw (user-space) vector drawings (PyMuPDF `Page.get_cdrawings`).
    fn get_cdrawings<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let page = self.page.clone();
        let drawings = py.detach(move || pdf_api::page_get_cdrawings(&page));
        let out = PyList::empty(py);
        for d in &drawings {
            out.append(drawing_to_py(py, d)?)?;
        }
        Ok(out)
    }

    // --- forms (PRD §8.8) ------------------------------------------------

    /// The form widgets on this page (PyMuPDF `Page.widgets`).
    fn widgets(&self) -> Vec<PyWidget> {
        pdf_api::page_widgets(&self.page)
            .into_iter()
            .map(|widget| PyWidget { widget })
            .collect()
    }

    /// The first widget, or `None` (PyMuPDF `Page.first_widget`).
    #[getter]
    fn first_widget(&self) -> Option<PyWidget> {
        pdf_api::page_first_widget(&self.page).map(|widget| PyWidget { widget })
    }

    fn __repr__(&self) -> String {
        format!("<oxide_pdf._core.Page number={}>", self.page.number())
    }
}

/// Wraps an `AnnotHandle` result into a [`PyAnnot`], mapping errors.
fn wrap_annot(r: Result<AnnotHandle, ApiError>) -> PyResult<PyAnnot> {
    r.map(|annot| PyAnnot { annot }).map_err(map_err)
}

/// Maps a PyMuPDF align integer (0=left, 1=center, 2=right, 3=justify) to
/// [`Align`].
fn align_of(align: i32) -> Align {
    match align {
        1 => Align::Center,
        2 => Align::Right,
        3 => Align::Justify,
        _ => Align::Left,
    }
}

// --- Document handle ------------------------------------------------------

/// A document handle (PRD §9.2 / §9.4). Holds a `pdf_api::Document` (cheap to
/// clone: `Arc` bumps) so every `Page` it produces is independent of this object.
#[pyclass(name = "Document", module = "oxide_pdf._core", frozen)]
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

    // --- extract_image (PRD §8.10) ---------------------------------------

    /// Extracts the image XObject at object number `xref` (PyMuPDF
    /// `Document.extract_image`). Returns a dict with `ext`, `colorspace`,
    /// `bpc`, `width`, `height`, `n` (components), `smask`, and `image` (bytes).
    fn extract_image<'py>(&self, py: Python<'py>, xref: u32) -> PyResult<Bound<'py, PyDict>> {
        let store = self.doc.store();
        let ext = py
            .detach(|| pdf_api::document_extract_image(store, xref))
            .map_err(map_err)?;
        let d = PyDict::new(py);
        d.set_item("ext", ext.ext)?;
        d.set_item("colorspace", ext.colorspace)?;
        d.set_item("bpc", ext.bpc)?;
        d.set_item("width", ext.width)?;
        d.set_item("height", ext.height)?;
        d.set_item("n", ext.components)?;
        d.set_item("smask", ext.smask)?;
        d.set_item("image", PyBytes::new(py, &ext.image))?;
        Ok(d)
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

    // --- save (PRD §8.7 / §8.4) ------------------------------------------

    /// Full-saves to `path` (PyMuPDF `Document.save`). Heavy work runs with the
    /// GIL released. `encryption`: PyMuPDF method constant (`1`=RC4-128,
    /// `2`=AES-128, `3..=6`=AES-256-R6); `incremental=True` appends in place.
    #[pyo3(signature = (
        path, *, garbage=0, deflate=false, incremental=false,
        encryption=None, owner_pw=None, user_pw=None, permissions=-1
    ))]
    #[allow(clippy::too_many_arguments)]
    fn save(
        &self,
        py: Python<'_>,
        path: &str,
        garbage: u8,
        deflate: bool,
        incremental: bool,
        encryption: Option<i32>,
        owner_pw: Option<&str>,
        user_pw: Option<&str>,
        permissions: i32,
    ) -> PyResult<()> {
        if incremental {
            let opts = pdf_api::SaveOptions::default();
            let bytes = py
                .detach(|| self.doc.save_incremental(&opts))
                .map_err(map_err)?;
            std::fs::write(path, bytes).map_err(|e| PyOSError::new_err(e.to_string()))?;
            return Ok(());
        }
        let opts = build_save_opts(garbage, deflate, encryption, user_pw, owner_pw, permissions)?;
        py.detach(|| self.doc.save_to_path(path, &opts))
            .map_err(map_err)
    }

    /// Full-saves to bytes (PyMuPDF `Document.tobytes`/`write`).
    #[pyo3(signature = (
        *, garbage=0, deflate=false, incremental=false,
        encryption=None, owner_pw=None, user_pw=None, permissions=-1
    ))]
    #[allow(clippy::too_many_arguments)]
    fn tobytes<'py>(
        &self,
        py: Python<'py>,
        garbage: u8,
        deflate: bool,
        incremental: bool,
        encryption: Option<i32>,
        owner_pw: Option<&str>,
        user_pw: Option<&str>,
        permissions: i32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = if incremental {
            let opts = pdf_api::SaveOptions::default();
            py.detach(|| self.doc.save_incremental(&opts))
                .map_err(map_err)?
        } else {
            let opts =
                build_save_opts(garbage, deflate, encryption, user_pw, owner_pw, permissions)?;
            py.detach(|| self.doc.save_to_bytes(&opts))
                .map_err(map_err)?
        };
        Ok(PyBytes::new(py, &bytes))
    }

    /// PyMuPDF deprecated alias for an incremental save to `path`.
    #[allow(non_snake_case)]
    fn saveIncr(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let opts = pdf_api::SaveOptions::default();
        let bytes = py
            .detach(|| self.doc.save_incremental(&opts))
            .map_err(map_err)?;
        std::fs::write(path, bytes).map_err(|e| PyOSError::new_err(e.to_string()))?;
        Ok(())
    }

    // --- metadata write (PRD §8.9) ---------------------------------------

    /// Sets `/Info` metadata from a dict (PyMuPDF `set_metadata`).
    fn set_metadata(&self, meta: &Bound<'_, PyDict>) -> PyResult<()> {
        let mut fields: Vec<(String, String)> = Vec::new();
        for (k, v) in meta.iter() {
            let key: String = k.extract()?;
            let val: String = v.extract().unwrap_or_default();
            fields.push((key, val));
        }
        self.doc.set_metadata(&fields).map_err(map_err)
    }

    /// PyMuPDF deprecated alias for `set_metadata`.
    #[allow(non_snake_case)]
    fn setMetadata(&self, meta: &Bound<'_, PyDict>) -> PyResult<()> {
        self.set_metadata(meta)
    }

    /// The catalog XMP metadata string (PyMuPDF `get_xml_metadata`).
    fn get_xml_metadata(&self) -> String {
        self.doc.get_xml_metadata().unwrap_or_default()
    }

    /// Sets the catalog XMP metadata stream (PyMuPDF `set_xml_metadata`).
    fn set_xml_metadata(&self, xml: &str) -> PyResult<()> {
        self.doc.set_xml_metadata(xml).map_err(map_err)
    }

    // --- TOC (PRD §8.9) ---------------------------------------------------

    /// The outline as a list of `[level, title, page]` (PyMuPDF `get_toc`).
    #[pyo3(signature = (simple=true))]
    fn get_toc<'py>(&self, py: Python<'py>, simple: bool) -> PyResult<Bound<'py, PyList>> {
        let _ = simple;
        let rows = self.doc.get_toc();
        let list = PyList::empty(py);
        for (level, title, page) in rows {
            let row = PyList::new(
                py,
                [
                    level.into_pyobject(py)?.into_any(),
                    title.into_pyobject(py)?.into_any(),
                    page.into_pyobject(py)?.into_any(),
                ],
            )?;
            list.append(row)?;
        }
        Ok(list)
    }

    /// PyMuPDF deprecated alias for `get_toc`.
    #[allow(non_snake_case)]
    #[pyo3(signature = (simple=true))]
    fn getToC<'py>(&self, py: Python<'py>, simple: bool) -> PyResult<Bound<'py, PyList>> {
        self.get_toc(py, simple)
    }

    /// Builds the outline from a list of `[level, title, page]` (PyMuPDF
    /// `set_toc`). Raises on a level jump.
    fn set_toc(&self, toc: &Bound<'_, PyList>) -> PyResult<()> {
        let mut entries: Vec<(i32, String, i32)> = Vec::with_capacity(toc.len());
        for item in toc.iter() {
            let seq: Vec<Bound<'_, PyAny>> = item.extract()?;
            if seq.len() < 3 {
                return Err(PdfError::new_err("TOC entry must be [level, title, page]"));
            }
            let level: i32 = seq[0].extract()?;
            let title: String = seq[1].extract()?;
            let page: i32 = seq[2].extract()?;
            entries.push((level, title, page));
        }
        self.doc.set_toc(&entries).map_err(map_err)
    }

    /// PyMuPDF deprecated alias for `set_toc`.
    #[allow(non_snake_case)]
    fn setToC(&self, toc: &Bound<'_, PyList>) -> PyResult<()> {
        self.set_toc(toc)
    }

    // --- page ops + merge (PRD §8.7) -------------------------------------

    /// Inserts pages from `src` (PyMuPDF `insert_pdf`).
    #[pyo3(signature = (src, from_page=None, to_page=None, start_at=None))]
    fn insert_pdf(
        &self,
        py: Python<'_>,
        src: &PyDocument,
        from_page: Option<usize>,
        to_page: Option<usize>,
        start_at: Option<usize>,
    ) -> PyResult<()> {
        let srcdoc = src.doc.clone();
        py.detach(|| self.doc.insert_pdf(&srcdoc, from_page, to_page, start_at))
            .map_err(map_err)
    }

    /// PyMuPDF deprecated alias for `insert_pdf`.
    #[allow(non_snake_case)]
    #[pyo3(signature = (src, from_page=None, to_page=None, start_at=None))]
    fn insertPDF(
        &self,
        py: Python<'_>,
        src: &PyDocument,
        from_page: Option<usize>,
        to_page: Option<usize>,
        start_at: Option<usize>,
    ) -> PyResult<()> {
        self.insert_pdf(py, src, from_page, to_page, start_at)
    }

    /// Inserts a blank page (PyMuPDF `new_page`). Returns the new page.
    #[pyo3(signature = (pno=-1, width=595.0, height=842.0))]
    fn new_page(&self, pno: isize, width: f64, height: f64) -> PyResult<PyPage> {
        let n = self.doc.page_count();
        let index = if pno < 0 { n } else { (pno as usize).min(n) };
        self.doc
            .new_page(Some(index), width, height)
            .map_err(map_err)?;
        let page = self.doc.load_page(index).map_err(map_err)?;
        Ok(PyPage { page })
    }

    /// PyMuPDF deprecated alias for `new_page`.
    #[allow(non_snake_case)]
    #[pyo3(signature = (pno=-1, width=595.0, height=842.0))]
    fn newPage(&self, pno: isize, width: f64, height: f64) -> PyResult<PyPage> {
        self.new_page(pno, width, height)
    }

    /// Deletes the page at `pno` (PyMuPDF `delete_page`).
    fn delete_page(&self, pno: usize) -> PyResult<()> {
        self.doc.delete_page(pno).map_err(map_err)
    }

    /// Keeps only `pages` in the given order (PyMuPDF `select`).
    fn select(&self, pages: Vec<usize>) -> PyResult<()> {
        self.doc.select(&pages).map_err(map_err)
    }

    // --- links + labels (PRD §8.9) ---------------------------------------

    /// The page label of physical page `pno` (PyMuPDF `Page.get_label`, also
    /// exposed at the document level for convenience).
    fn get_page_label(&self, pno: usize) -> String {
        self.doc.get_label(pno)
    }

    // --- forms (PRD §8.8) ------------------------------------------------

    /// Whether the document has an interactive form (PyMuPDF `is_form_pdf`).
    #[getter]
    fn is_form_pdf(&self) -> bool {
        self.doc.is_form_pdf()
    }

    /// The fully-qualified names of every terminal form field (PyMuPDF
    /// `Document.get_form_fields`-style listing).
    fn form_field_names(&self) -> Vec<String> {
        self.doc.form_field_names()
    }

    /// Sets a form field value by name (PyMuPDF `Document` form fill helper).
    fn form_fill(&self, name: &str, value: &str) -> PyResult<()> {
        self.doc.form_fill(name, value).map_err(map_err)
    }

    /// Flattens the form: bakes widget appearances into page content and removes
    /// `/AcroForm` + widgets (PyMuPDF `Document` flatten helper).
    fn form_flatten(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| self.doc.form_flatten()).map_err(map_err)
    }

    // --- embedded files (PRD §8.8) ---------------------------------------

    /// Embeds `data` under `name` (PyMuPDF `Document.embfile_add`).
    #[pyo3(signature = (name, data, *, filename=None, ufilename=None, desc=None))]
    fn embfile_add(
        &self,
        name: &str,
        data: Vec<u8>,
        filename: Option<&str>,
        ufilename: Option<&str>,
        desc: Option<&str>,
    ) -> PyResult<()> {
        self.doc
            .embfile_add(name, &data, filename, ufilename, desc)
            .map_err(map_err)
    }

    /// Reads the embedded file `name` byte-exact (PyMuPDF `Document.embfile_get`).
    fn embfile_get<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.doc.embfile_get(name).map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Removes the embedded file `name` (PyMuPDF `Document.embfile_del`).
    fn embfile_del(&self, name: &str) -> PyResult<()> {
        self.doc.embfile_del(name).map_err(map_err)
    }

    /// The embedded-file names (PyMuPDF `Document.embfile_names`).
    fn embfile_names(&self) -> Vec<String> {
        self.doc.embfile_names()
    }

    /// The number of embedded files (PyMuPDF `Document.embfile_count`).
    fn embfile_count(&self) -> usize {
        self.doc.embfile_count()
    }

    /// The metadata of embedded file `name` as a dict (PyMuPDF
    /// `Document.embfile_info`).
    fn embfile_info<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyDict>> {
        let info = self.doc.embfile_info(name).map_err(map_err)?;
        let d = PyDict::new(py);
        d.set_item("name", info.name)?;
        d.set_item("filename", info.filename)?;
        d.set_item("ufilename", info.ufilename)?;
        d.set_item("desc", info.desc)?;
        d.set_item("size", info.size)?;
        d.set_item("length", info.length)?;
        Ok(d)
    }

    // --- scrub / bake (PRD §8.8) -----------------------------------------

    /// Removes sensitive data (PyMuPDF `Document.scrub`). Heavy work runs with
    /// the GIL released.
    #[pyo3(signature = (*, metadata=true, javascript=true, attached_files=true, remove_links=false, xml_metadata=true))]
    fn scrub(
        &self,
        py: Python<'_>,
        metadata: bool,
        javascript: bool,
        attached_files: bool,
        remove_links: bool,
        xml_metadata: bool,
    ) -> PyResult<()> {
        let opts = ScrubOptions {
            metadata,
            javascript,
            attached_files,
            remove_links,
            xml_metadata,
        };
        py.detach(|| self.doc.scrub(&opts)).map_err(map_err)
    }

    /// Flattens annotations and/or widgets into page content (PyMuPDF
    /// `Document.bake`). Heavy work runs with the GIL released.
    #[pyo3(signature = (*, annots=true, widgets=true))]
    fn bake(&self, py: Python<'_>, annots: bool, widgets: bool) -> PyResult<()> {
        py.detach(|| self.doc.bake(annots, widgets))
            .map_err(map_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "<oxide_pdf._core.Document page_count={}>",
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

/// Returns the oxide_pdf version string.
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

// === Pixmap (PRD §8.10 / §9.4) ============================================

/// Maps a colorspace component count to its PyMuPDF name string.
fn colorspace_name(cs: Colorspace) -> &'static str {
    cs.name()
}

/// A decoded raster (PyMuPDF `Pixmap`, PRD §8.10). Implements the **buffer
/// protocol** with the enforced copy-on-write lifetime contract (PRD §9.4):
///
/// - the pixel bytes live in an `Arc<[u8]>` inside [`ApiPixmap`];
/// - `__getbuffer__` clones that `Arc` into the `Py_buffer.internal` (a boxed
///   `Arc<[u8]>`) so a `memoryview` / `numpy` view keeps the bytes alive even if
///   the `Pixmap` Python object is GC'd while the view is live, and increments an
///   export count;
/// - `__releasebuffer__` drops that boxed `Arc` clone and decrements the count;
/// - every mutator (`set_pixel`, `clear`, `set_alpha`, `invert_irect`) goes
///   through `ApiPixmap`'s copy-on-write (`Arc::make_mut`): while any external
///   `Arc` clone is alive (a live export, or the boxed clone in a `Py_buffer`),
///   the mutation lands in a fresh allocation, so a view can never observe a
///   mutate-under-view or use-after-free.
#[pyclass(name = "Pixmap", module = "oxide_pdf._core")]
struct PyPixmap {
    pix: ApiPixmap,
    /// The number of live buffer exports (for `readonly` + diagnostics; the COW
    /// itself rides on the `Arc` strong count, which the boxed clone bumps).
    /// Atomic so the `#[pyclass]` stays `Sync` (PyO3 0.29 requirement).
    exports: AtomicUsize,
}

impl PyPixmap {
    fn new(pix: ApiPixmap) -> Self {
        PyPixmap {
            pix,
            exports: AtomicUsize::new(0),
        }
    }
}

#[pymethods]
impl PyPixmap {
    /// `Pixmap(colorspace, irect, alpha)` — a blank pixmap. `colorspace` is a
    /// component count (1=gray, 3=rgb, 4=cmyk) or a name string; `irect` is the
    /// `(x0, y0, x1, y1)` bounds. Matches the common PyMuPDF constructor shape.
    #[new]
    #[pyo3(signature = (colorspace, irect, alpha=false))]
    fn py_new(
        colorspace: &Bound<'_, PyAny>,
        irect: (i64, i64, i64, i64),
        alpha: bool,
    ) -> PyResult<Self> {
        let cs = parse_colorspace(colorspace)?;
        let (x0, y0, x1, y1) = irect;
        let w = u32::try_from((x1 - x0).max(0))
            .map_err(|_| PyValueError::new_err("invalid irect width"))?;
        let h = u32::try_from((y1 - y0).max(0))
            .map_err(|_| PyValueError::new_err("invalid irect height"))?;
        let pix = pdf_api::pixmap_blank(w, h, cs, alpha, 0).map_err(map_err)?;
        Ok(PyPixmap::new(pix))
    }

    /// The pixel width (PyMuPDF `Pixmap.width` / `.w`).
    #[getter]
    fn width(&self) -> u32 {
        self.pix.width
    }

    /// The pixel width alias (PyMuPDF `Pixmap.w`).
    #[getter]
    fn w(&self) -> u32 {
        self.pix.width
    }

    /// The pixel height (PyMuPDF `Pixmap.height` / `.h`).
    #[getter]
    fn height(&self) -> u32 {
        self.pix.height
    }

    /// The pixel height alias (PyMuPDF `Pixmap.h`).
    #[getter]
    fn h(&self) -> u32 {
        self.pix.height
    }

    /// Components per pixel including alpha (PyMuPDF `Pixmap.n`).
    #[getter]
    fn n(&self) -> u8 {
        self.pix.n
    }

    /// Whether the last component is alpha (PyMuPDF `Pixmap.alpha`).
    #[getter]
    fn alpha(&self) -> bool {
        self.pix.alpha
    }

    /// Bytes per row (PyMuPDF `Pixmap.stride`).
    #[getter]
    fn stride(&self) -> usize {
        self.pix.stride
    }

    /// `(x0, y0, x1, y1)` bounding box at the origin (PyMuPDF `Pixmap.irect`).
    #[getter]
    fn irect(&self) -> (i64, i64, i64, i64) {
        (0, 0, self.pix.width as i64, self.pix.height as i64)
    }

    /// The colorspace name string (`"DeviceGray"`/`"DeviceRGB"`/`"DeviceCMYK"`).
    #[getter]
    fn colorspace(&self) -> &'static str {
        colorspace_name(self.pix.colorspace)
    }

    /// The raw sample bytes as an owning `bytes` copy (PyMuPDF `Pixmap.samples`).
    /// Zero lifetime concerns — see also the buffer protocol for zero-copy views.
    #[getter]
    fn samples<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.pix.samples())
    }

    /// `len(samples)` (PyMuPDF `Pixmap.samples_mv` length).
    #[getter]
    fn size(&self) -> usize {
        self.pix.samples().len()
    }

    /// A zero-copy `memoryview` of the pixels (PyMuPDF `Pixmap.samples_mv`).
    /// Goes through the buffer protocol, so it carries the COW lifetime contract.
    #[getter]
    fn samples_mv<'py>(slf: Bound<'py, Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let memoryview = py.import("builtins")?.getattr("memoryview")?;
        memoryview.call1((slf,))
    }

    /// Reads pixel `(x, y)` as a tuple of `n` component ints (PyMuPDF
    /// `Pixmap.pixel`).
    fn pixel<'py>(&self, py: Python<'py>, x: u32, y: u32) -> PyResult<Bound<'py, PyTuple>> {
        let px = self
            .pix
            .pixel(x, y)
            .ok_or_else(|| PyValueError::new_err("pixel coordinate out of range"))?;
        PyTuple::new(py, px.iter().map(|&b| b as u32))
    }

    /// Writes pixel `(x, y)` from a sequence of `n` component bytes (PyMuPDF
    /// `Pixmap.set_pixel`). Copy-on-write if a buffer view is live.
    fn set_pixel(&mut self, x: u32, y: u32, value: Vec<u8>) -> PyResult<()> {
        pdf_api::pixmap_set_pixel(&mut self.pix, x, y, &value).map_err(map_err)
    }

    /// Sets every alpha byte to `value` (PyMuPDF `Pixmap.set_alpha` constant).
    fn set_alpha(&mut self, value: u8) {
        self.pix.set_alpha(value);
    }

    /// Fills the whole buffer with `value` (PyMuPDF `Pixmap.clear_with`).
    #[pyo3(signature = (value=0))]
    fn clear_with(&mut self, value: u8) {
        self.pix.clear(value);
    }

    /// Inverts colors within `irect` (PyMuPDF `Pixmap.invert_irect`); without an
    /// argument inverts the whole pixmap.
    #[pyo3(signature = (irect=None))]
    fn invert_irect(&mut self, irect: Option<(i64, i64, i64, i64)>) {
        let (x0, y0, x1, y1) =
            irect.unwrap_or((0, 0, self.pix.width as i64, self.pix.height as i64));
        self.pix.invert_irect(
            x0.max(0) as u32,
            y0.max(0) as u32,
            x1.max(0) as u32,
            y1.max(0) as u32,
        );
    }

    /// Encodes the pixmap and returns the bytes (PyMuPDF `Pixmap.tobytes`).
    /// `output` is `"png"` (default), `"pam"`, or `"ppm"`/`"pnm"`.
    #[pyo3(signature = (output="png"))]
    fn tobytes<'py>(&self, py: Python<'py>, output: &str) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = py
            .detach(|| pdf_api::pixmap_tobytes(&self.pix, output))
            .map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Saves the pixmap to `filename` (PyMuPDF `Pixmap.save`). The format is the
    /// `output` arg or inferred from the extension (PNG default).
    #[pyo3(signature = (filename, output=None))]
    fn save(&self, py: Python<'_>, filename: &str, output: Option<&str>) -> PyResult<()> {
        let fmt = output
            .map(str::to_string)
            .or_else(|| {
                std::path::Path::new(filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
            })
            .unwrap_or_else(|| "png".to_string());
        let bytes = py
            .detach(|| pdf_api::pixmap_tobytes(&self.pix, &fmt))
            .map_err(map_err)?;
        std::fs::write(filename, bytes).map_err(|e| PyOSError::new_err(e.to_string()))
    }

    fn __len__(&self) -> usize {
        self.pix.samples().len()
    }

    fn __repr__(&self) -> String {
        format!(
            "Pixmap({}, {}x{}, alpha={})",
            colorspace_name(self.pix.colorspace),
            self.pix.width,
            self.pix.height,
            self.pix.alpha
        )
    }

    // --- buffer protocol (PRD §9.4) --------------------------------------

    /// Exposes the samples as a read-only buffer. Clones the backing `Arc<[u8]>`
    /// into `view.internal` so the bytes outlive this object while a view is
    /// alive (the enforced COW lifetime contract, PRD §9.4).
    ///
    /// # Safety
    ///
    /// PyO3 calls this with a valid `view` pointer; we initialize every field.
    unsafe fn __getbuffer__(
        slf: Bound<'_, Self>,
        view: *mut ffi::Py_buffer,
        flags: c_int,
    ) -> PyResult<()> {
        if view.is_null() {
            return Err(PyValueError::new_err("null buffer view"));
        }
        let this = slf.borrow();
        // Clone the Arc; this raises the strong count so any in-place mutator
        // copy-on-writes instead of touching the bytes this view points at.
        let arc: Arc<[u8]> = this.pix.samples_arc();
        let len = arc.len();
        // Stash a heap-owned Arc clone in `internal`; reclaimed in releasebuffer.
        let boxed: *mut Arc<[u8]> = Box::into_raw(Box::new(arc));
        let data_ptr = unsafe { (*boxed).as_ptr() } as *mut c_void;

        this.exports.fetch_add(1, Ordering::SeqCst);

        unsafe {
            (*view).obj = slf.clone().into_any().into_ptr();
            (*view).buf = data_ptr;
            (*view).len = len as isize;
            (*view).readonly = 1;
            (*view).itemsize = 1;
            (*view).format = if (flags & ffi::PyBUF_FORMAT) == ffi::PyBUF_FORMAT {
                CString::new("B").unwrap().into_raw()
            } else {
                ptr::null_mut()
            };
            (*view).ndim = 1;
            (*view).shape = ptr::null_mut();
            (*view).strides = ptr::null_mut();
            (*view).suboffsets = ptr::null_mut();
            (*view).internal = boxed as *mut c_void;
        }
        Ok(())
    }

    /// Releases a buffer export: drops the boxed `Arc` clone (lowering the strong
    /// count) and the format string, and decrements the export count.
    ///
    /// # Safety
    ///
    /// `view` is the same pointer a prior `__getbuffer__` populated.
    unsafe fn __releasebuffer__(&self, view: *mut ffi::Py_buffer) {
        if view.is_null() {
            return;
        }
        unsafe {
            if !(*view).format.is_null() {
                drop(CString::from_raw((*view).format));
                (*view).format = ptr::null_mut();
            }
            if !(*view).internal.is_null() {
                drop(Box::from_raw((*view).internal as *mut Arc<[u8]>));
                (*view).internal = ptr::null_mut();
            }
        }
        let prev = self.exports.load(Ordering::SeqCst);
        if prev > 0 {
            self.exports.store(prev - 1, Ordering::SeqCst);
        }
    }
}

/// A recorded, replayable page render (PyMuPDF `DisplayList`). Built by
/// `page.get_displaylist()`; replay with `dl.get_pixmap(...)`.
#[pyclass(name = "DisplayList", module = "oxide_pdf")]
struct PyDisplayList {
    inner: Arc<ApiDisplayList>,
}

#[pymethods]
impl PyDisplayList {
    /// The source rect (the page CropBox), as a `(x0, y0, x1, y1)` tuple
    /// (PyMuPDF `DisplayList.rect`).
    #[getter]
    fn rect(&self) -> (f64, f64, f64, f64) {
        self.inner.rect()
    }

    /// The number of recorded drawcalls.
    fn __len__(&self) -> usize {
        self.inner.len()
    }

    /// Replays the recorded drawcalls into a [`PyPixmap`] (PyMuPDF
    /// `DisplayList.get_pixmap`). Same kwargs as `Page.get_pixmap`.
    #[pyo3(signature = (*, matrix=None, dpi=None, colorspace=None, alpha=false, clip=None))]
    fn get_pixmap(
        &self,
        py: Python<'_>,
        matrix: Option<(f64, f64, f64, f64, f64, f64)>,
        dpi: Option<f64>,
        colorspace: Option<Bound<'_, PyAny>>,
        alpha: bool,
        clip: Option<(f64, f64, f64, f64)>,
    ) -> PyResult<PyPixmap> {
        let args = build_render_args(matrix, dpi, colorspace, alpha, clip)?;
        let inner = self.inner.clone();
        let pix = py.detach(|| inner.get_pixmap(&args)).map_err(map_err)?;
        Ok(PyPixmap::new(pix))
    }

    fn __repr__(&self) -> String {
        let (x0, y0, x1, y1) = self.inner.rect();
        format!(
            "DisplayList(rect=({x0}, {y0}, {x1}, {y1}), ops={})",
            self.inner.len()
        )
    }
}

/// Builds a [`RenderArgs`] from the Python `get_pixmap` kwargs (matrix tuple,
/// dpi float, colorspace object/int/name, alpha flag, clip tuple).
fn build_render_args(
    matrix: Option<(f64, f64, f64, f64, f64, f64)>,
    dpi: Option<f64>,
    colorspace: Option<Bound<'_, PyAny>>,
    alpha: bool,
    clip: Option<(f64, f64, f64, f64)>,
) -> PyResult<RenderArgs> {
    let m = matrix
        .map(|(a, b, c, d, e, f)| Matrix::new(a, b, c, d, e, f))
        .unwrap_or(Matrix::IDENTITY);
    let cs = match colorspace {
        Some(obj) => parse_colorspace(&obj)?,
        None => Colorspace::Rgb,
    };
    let dpi_u = dpi.map(|d| d.max(1.0).round() as u32);
    let clip_r = clip.map(|(x0, y0, x1, y1)| {
        IRect::new(
            x0.floor() as i32,
            y0.floor() as i32,
            x1.ceil() as i32,
            y1.ceil() as i32,
        )
    });
    Ok(RenderArgs {
        matrix: m,
        dpi: dpi_u,
        colorspace: cs,
        alpha,
        clip: clip_r,
    })
}

/// Parses a Python colorspace argument (a component count int, or a name string
/// like `"rgb"`/`"DeviceRGB"`) into a [`Colorspace`].
fn parse_colorspace(obj: &Bound<'_, PyAny>) -> PyResult<Colorspace> {
    if let Ok(n) = obj.extract::<i64>() {
        return match n {
            1 => Ok(Colorspace::Gray),
            3 => Ok(Colorspace::Rgb),
            4 => Ok(Colorspace::Cmyk),
            _ => Err(PyValueError::new_err(
                "unsupported colorspace component count",
            )),
        };
    }
    // A colorspace object often exposes `.n`; or a plain name string.
    if let Ok(n) = obj.getattr("n").and_then(|v| v.extract::<i64>()) {
        return match n {
            1 => Ok(Colorspace::Gray),
            3 => Ok(Colorspace::Rgb),
            4 => Ok(Colorspace::Cmyk),
            _ => Err(PyValueError::new_err("unsupported colorspace")),
        };
    }
    let s: String = obj.extract().map_err(|_| {
        PyValueError::new_err("colorspace must be an int, name string, or colorspace object")
    })?;
    match s.to_ascii_lowercase().as_str() {
        "gray" | "grey" | "devicegray" | "csgray" | "g" => Ok(Colorspace::Gray),
        "rgb" | "devicergb" | "csrgb" => Ok(Colorspace::Rgb),
        "cmyk" | "devicecmyk" | "cscmyk" => Ok(Colorspace::Cmyk),
        _ => Err(PyValueError::new_err("unrecognized colorspace name")),
    }
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
    m.add_class::<PyAnnot>()?;
    m.add_class::<PyWidget>()?;
    m.add_class::<PyShape>()?;
    m.add_class::<PyPixmap>()?;
    m.add_class::<PyDisplayList>()?;

    // Exception hierarchy (PRD §9.3).
    m.add("PdfError", py.get_type::<PdfError>())?;
    m.add("PdfSyntaxError", py.get_type::<PdfSyntaxError>())?;
    m.add("PdfPasswordError", py.get_type::<PdfPasswordError>())?;
    m.add("PdfUnsupportedError", py.get_type::<PdfUnsupportedError>())?;
    m.add("PdfDecodeError", py.get_type::<PdfDecodeError>())?;
    m.add("PdfLimitError", py.get_type::<PdfLimitError>())?;
    m.add("PdfRedactionError", py.get_type::<PdfRedactionError>())?;

    Ok(())
}
