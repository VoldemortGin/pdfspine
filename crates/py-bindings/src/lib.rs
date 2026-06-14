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
