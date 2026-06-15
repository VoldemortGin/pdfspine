#![forbid(unsafe_code)]
//! `pdf-api` — the unified ergonomic facade over the oxipdf core crates and the
//! only crate `py-bindings` depends on (PRD §9.1).
//!
//! M1f adds the [`Document`] / [`Page`] read surface (PRD §7): `open`/`open_bytes`,
//! `page_count`/`load_page`/`pages`, `metadata`, the encryption read API
//! (`needs_pass`/`authenticate`/`permissions`/`is_encrypted`), `is_repaired`, and
//! the low-level xref read API (`xref_length`/`xref_object`/…). Geometry value
//! types are re-exported from [`pdf_core::geom`].

pub mod error;
pub mod image;
pub mod text;

use std::path::Path;
use std::sync::{Arc, RwLock};

use pdf_core::object::{Name, Object};
use pdf_core::pagetree;
use pdf_core::source::MmapMode;
use pdf_core::{DocumentStore, Limits, ObjRef};

pub use error::{Error, Result};
pub use pdf_core::page::Page;
pub use pdf_core::repair::ParseMode;
pub use pdf_core::{OnRepaired, SaveOptions, XrefStyle};

// Editing types surfaced to the bindings (PRD §8.9).
pub use pdf_edit::{Link, LinkKind, TocEntry};

// M4e value/enum types surfaced so `py-bindings` depends only on `pdf-api`
// (PRD §9.1). `Color` is constructed via [`Color::new`] at the tuple-color
// boundary; the rest are returned/consumed by the M4 surface below.
pub use pdf_edit::{
    Align, AnnotType, Color, DrawItem, Drawing, EmbfileInfo, FieldType, ScrubOptions,
};

/// Encryption-authoring types for `save(encryption=…)` (PRD §8.4). Available only
/// under the `encryption` feature (the default for the Python build).
#[cfg(feature = "encryption")]
pub use pdf_crypto::{EncryptMethod, EncryptSpec};

// Page inventory + reusable text-extraction surface (M2e). The PyO3 layer calls
// these free functions (the orphan rule forbids inherent `impl Page` here, since
// `Page` is defined in `pdf-core`).
pub use text::{
    get_fonts, get_images, get_text, search, textpage, FontInfo, ImageInfo, TextOutput,
};

// Image path (M5): `Pixmap`, `get_pixmap`, `extract_image`, image documents
// (PRD §3.3 / §8.10). The bindings depend only on `pdf-api`.
pub use image::{
    document_extract_image, image_document_page_pixmap, image_to_pdf, open_image_document,
    page_get_pixmap, page_is_image_only, pixmap_blank, pixmap_set_pixel, pixmap_tobytes,
    Colorspace, ExtractedImage, ImageDocument, ImageFormat, Pixmap,
};

// `pdf-text` types the bindings need so they only depend on `pdf-api` (PRD §9.1).
pub use pdf_text::{
    defaults, BlockTuple, DictBlock, DictChar, DictImageBlock, DictLine, DictSpan, DictTextBlock,
    SearchOptions, TextDict, TextPage, WordTuple,
};

/// The crate version string (the workspace version), surfaced to Python as
/// `oxipdf.__version__`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Geometry value types, re-exported from `pdf-core` (PRD §7 / M0).
pub mod geom {
    pub use pdf_core::geom::*;
}

// Convenience flat re-exports of the most-used geometry types.
pub use pdf_core::geom::{IRect, Matrix, Point, Quad, Rect};

/// A parsed document — the ergonomic Rust API the bindings build on (PRD §9.2).
///
/// Holds the shared [`DocumentStore`] behind an `Arc` (so [`Page`]s carry their
/// own clone, never a borrow — PRD §9.4) plus the precomputed ordered page list.
#[derive(Clone)]
pub struct Document {
    store: Arc<DocumentStore>,
    /// The ordered page list, cached for the fast read path and refreshed by
    /// [`Document::refresh_pages`] after any structural edit (PRD §8.7).
    pages: Arc<RwLock<Vec<ObjRef>>>,
}

impl Document {
    // --- opening ----------------------------------------------------------

    /// Opens a document from a filesystem path in the default Lenient mode with
    /// the hard-safe `mmap: Never` source (recommended for untrusted inputs,
    /// PRD §9.6.1).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] on read failure, or a typed parse error.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, ParseMode::Lenient)
    }

    /// Opens from a path with an explicit [`ParseMode`] (PRD §8.2).
    ///
    /// # Errors
    ///
    /// See [`Document::open`].
    pub fn open_with(path: impl AsRef<Path>, mode: ParseMode) -> Result<Self> {
        let store = DocumentStore::open_with(path, MmapMode::Never, mode, Limits::default())?;
        Ok(Self::from_store(store))
    }

    /// Opens a document from in-memory bytes in the default Lenient mode.
    ///
    /// # Errors
    ///
    /// A typed parse error if even repair cannot recover a usable document.
    pub fn open_bytes(bytes: impl Into<bytes::Bytes>) -> Result<Self> {
        Self::open_bytes_with(bytes, ParseMode::Lenient)
    }

    /// Opens from bytes with an explicit [`ParseMode`].
    ///
    /// # Errors
    ///
    /// See [`Document::open_bytes`].
    pub fn open_bytes_with(bytes: impl Into<bytes::Bytes>, mode: ParseMode) -> Result<Self> {
        let store = DocumentStore::from_bytes_with(bytes, mode, Limits::default())?;
        Ok(Self::from_store(store))
    }

    /// Wraps an already-opened store, computing the ordered page list once.
    fn from_store(store: DocumentStore) -> Self {
        let store = Arc::new(store);
        let pages = pdf_core::pagetree::page_refs(&store);
        Document {
            store,
            pages: Arc::new(RwLock::new(pages)),
        }
    }

    /// Re-derives the cached page list from the live store. Called after any
    /// structural edit (page op / merge) so `page_count`/`load_page` stay correct.
    fn refresh_pages(&self) {
        let fresh = pdf_core::pagetree::page_refs(&self.store);
        if let Ok(mut guard) = self.pages.write() {
            *guard = fresh;
        }
    }

    /// A snapshot of the current ordered page refs.
    fn page_refs(&self) -> Vec<ObjRef> {
        self.pages.read().map(|p| p.clone()).unwrap_or_default()
    }

    // --- pages ------------------------------------------------------------

    /// The number of pages (PRD §3.4 `page_count`).
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.read().map(|p| p.len()).unwrap_or(0)
    }

    /// Loads the page at zero-based `index` (PyMuPDF `load_page`). A negative
    /// PyMuPDF index is the caller's concern; this takes `usize`.
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] when `index` is out of range.
    pub fn load_page(&self, index: usize) -> Result<Page> {
        let page_ref = self
            .page_refs()
            .get(index)
            .copied()
            .ok_or_else(|| Error::Syntax(format!("page index {index} out of range")))?;
        Ok(Page::new(Arc::clone(&self.store), index, page_ref))
    }

    /// An iterator over every page in order.
    pub fn pages(&self) -> impl Iterator<Item = Page> + '_ {
        let store = Arc::clone(&self.store);
        self.page_refs()
            .into_iter()
            .enumerate()
            .map(move |(i, r)| Page::new(Arc::clone(&store), i, r))
    }

    /// The shared document store (escape hatch for advanced callers).
    #[must_use]
    pub fn store(&self) -> &Arc<DocumentStore> {
        &self.store
    }

    // --- document facts ---------------------------------------------------

    /// Whether this is a PDF (always `true` here; image-doc support is M5).
    #[must_use]
    pub fn is_pdf(&self) -> bool {
        true
    }

    /// Whether the parse needed repair (PyMuPDF `is_repaired`; PRD §8.2).
    #[must_use]
    pub fn is_repaired(&self) -> bool {
        self.store.parse_was_repaired()
    }

    /// The PDF version as `(major, minor)`.
    #[must_use]
    pub fn version(&self) -> (u8, u8) {
        let v = self.store.version();
        (v.major, v.minor)
    }

    // --- encryption (PRD §8.4) -------------------------------------------

    /// Whether the document is encrypted (PyMuPDF `is_encrypted`).
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        #[cfg(feature = "encryption")]
        {
            self.store.is_encrypted()
        }
        #[cfg(not(feature = "encryption"))]
        {
            false
        }
    }

    /// Whether a password is still required (PyMuPDF `needs_pass`).
    #[must_use]
    pub fn needs_pass(&self) -> bool {
        #[cfg(feature = "encryption")]
        {
            self.store.needs_pass()
        }
        #[cfg(not(feature = "encryption"))]
        {
            false
        }
    }

    /// Authenticates `password` (PyMuPDF `authenticate`). Returns `true` on a
    /// successful match (any role), `false` on a wrong password. For an
    /// unencrypted document returns `true` (nothing to do, matching PyMuPDF).
    #[must_use]
    pub fn authenticate(&self, password: &[u8]) -> bool {
        #[cfg(feature = "encryption")]
        {
            if !self.store.is_encrypted() {
                return true;
            }
            self.store.authenticate(password).is_ok()
        }
        #[cfg(not(feature = "encryption"))]
        {
            let _ = password;
            true
        }
    }

    /// The advisory `/P` permission flags, if encrypted (PyMuPDF `permissions`).
    /// Returns the all-permissions sentinel for an unencrypted document, matching
    /// PyMuPDF (which reports `-1` / all bits set).
    #[must_use]
    pub fn permissions(&self) -> i32 {
        #[cfg(feature = "encryption")]
        {
            self.store.permissions().unwrap_or(-1)
        }
        #[cfg(not(feature = "encryption"))]
        {
            -1
        }
    }

    // --- save (PRD §8.7 / §8.4) ------------------------------------------

    /// Full-saves the document to a byte vector with the given options
    /// (`garbage`, `deflate`, `xref_style`, optional encryption). PyMuPDF
    /// `tobytes`/`write`.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] on a write failure.
    pub fn save_to_bytes(&self, opts: &pdf_core::SaveOptions) -> Result<Vec<u8>> {
        Ok(self.store.save_to_vec(opts)?)
    }

    /// Full-saves to a filesystem path.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] on a write failure, or a typed save error.
    pub fn save_to_path(&self, path: impl AsRef<Path>, opts: &pdf_core::SaveOptions) -> Result<()> {
        Ok(self.store.save(path, opts)?)
    }

    /// Incremental-saves (append-only) to a byte vector (PyMuPDF `saveIncr`).
    ///
    /// # Errors
    ///
    /// [`Error`] when the parse was repair-tainted (clean-parse precondition) or
    /// on a write failure.
    pub fn save_incremental(&self, opts: &pdf_core::SaveOptions) -> Result<Vec<u8>> {
        Ok(self.store.save_incremental(opts)?)
    }

    // --- metadata write (PRD §8.9) ---------------------------------------

    /// Writes the `/Info` dictionary from PyMuPDF `(key, value)` pairs.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] from the object-edit path.
    pub fn set_metadata(&self, fields: &[(String, String)]) -> Result<()> {
        Ok(pdf_edit::set_metadata(&self.store, fields)?)
    }

    /// The catalog `/Metadata` XMP stream as a string, or `None` when absent.
    #[must_use]
    pub fn get_xml_metadata(&self) -> Option<String> {
        pdf_edit::get_xml_metadata(&self.store)
    }

    /// Creates or replaces the catalog `/Metadata` XMP stream.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] from the object-edit path.
    pub fn set_xml_metadata(&self, xml: &str) -> Result<()> {
        Ok(pdf_edit::set_xml_metadata(&self.store, xml)?)
    }

    // --- TOC (PRD §8.9) ---------------------------------------------------

    /// The document outline as `(level, title, page)` rows (PyMuPDF `get_toc`).
    #[must_use]
    pub fn get_toc(&self) -> Vec<(i32, String, i32)> {
        pdf_edit::get_toc(&self.store)
            .into_iter()
            .map(|e| (e.level, e.title, e.page))
            .collect()
    }

    /// Builds the `/Outlines` tree from a flat level list (PyMuPDF `set_toc`).
    ///
    /// # Errors
    ///
    /// [`Error`] (invalid-argument) on a level jump; document left unmutated.
    pub fn set_toc(&self, entries: &[(i32, String, i32)]) -> Result<()> {
        let toc: Vec<pdf_edit::TocEntry> = entries
            .iter()
            .map(|(level, title, page)| pdf_edit::TocEntry {
                level: *level,
                title: title.clone(),
                page: *page,
            })
            .collect();
        pdf_edit::set_toc(&self.store, &toc)?;
        Ok(())
    }

    // --- page ops + merge (PRD §8.7) -------------------------------------

    /// Inserts pages from `src` into this document (PyMuPDF `insert_pdf`).
    ///
    /// # Errors
    ///
    /// A typed [`Error`] from the merge path.
    pub fn insert_pdf(
        &self,
        src: &Document,
        from_page: Option<usize>,
        to_page: Option<usize>,
        start_at: Option<usize>,
    ) -> Result<()> {
        let opts = pdf_edit::InsertOptions {
            from_page,
            to_page,
            start_at,
            rotate: None,
        };
        pdf_edit::insert_pdf(&self.store, &src.store, &opts)?;
        self.refresh_pages();
        Ok(())
    }

    /// Inserts a blank page (PyMuPDF `new_page`). `index` is the 0-based position
    /// (`None`/large = append).
    ///
    /// # Errors
    ///
    /// A typed [`Error`] from the page-op path.
    pub fn new_page(&self, index: Option<usize>, width: f64, height: f64) -> Result<()> {
        let mut ed = pdf_edit::PageEditor::new(&self.store)?;
        let idx = index.unwrap_or(ed.page_count());
        ed.new_page(idx.min(ed.page_count()), width, height)?;
        self.refresh_pages();
        Ok(())
    }

    /// Deletes the page at `index` (PyMuPDF `delete_page`).
    ///
    /// # Errors
    ///
    /// A typed [`Error`] for an out-of-range index.
    pub fn delete_page(&self, index: usize) -> Result<()> {
        let mut ed = pdf_edit::PageEditor::new(&self.store)?;
        ed.delete_page(index)?;
        self.refresh_pages();
        Ok(())
    }

    /// Keeps only `indices` (in order, with duplication) (PyMuPDF `select`).
    ///
    /// # Errors
    ///
    /// A typed [`Error`] for an out-of-range index.
    pub fn select(&self, indices: &[usize]) -> Result<()> {
        let mut ed = pdf_edit::PageEditor::new(&self.store)?;
        ed.select(indices)?;
        self.refresh_pages();
        Ok(())
    }

    /// Sets a page's rotation (PyMuPDF `Page.set_rotation`).
    ///
    /// # Errors
    ///
    /// A typed [`Error`] for an out-of-range index.
    pub fn set_page_rotation(&self, index: usize, degrees: i64) -> Result<()> {
        let mut ed = pdf_edit::PageEditor::new(&self.store)?;
        ed.set_rotation(index, degrees)?;
        Ok(())
    }

    // --- links (PRD §8.9) -------------------------------------------------

    /// The link annotations on page `index` (PyMuPDF `Page.get_links`).
    #[must_use]
    pub fn get_links(&self, index: usize) -> Vec<pdf_edit::Link> {
        pdf_edit::get_links(&self.store, index)
    }

    /// Inserts a URI link on page `index`.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] for an out-of-range page.
    pub fn insert_link_uri(&self, index: usize, rect: Rect, uri: &str) -> Result<()> {
        pdf_edit::insert_link(
            &self.store,
            index,
            &rect,
            &pdf_edit::LinkKind::Uri(uri.to_string()),
        )?;
        Ok(())
    }

    /// Inserts a GoTo link on page `index` targeting `target_page`.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] for an out-of-range page.
    pub fn insert_link_goto(&self, index: usize, rect: Rect, target_page: i32) -> Result<()> {
        pdf_edit::insert_link(
            &self.store,
            index,
            &rect,
            &pdf_edit::LinkKind::Goto(target_page),
        )?;
        Ok(())
    }

    /// Deletes the link annotation `xref` on page `index`.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] for an out-of-range page.
    pub fn delete_link(&self, index: usize, xref: u32) -> Result<()> {
        pdf_edit::delete_link(&self.store, index, ObjRef::new(xref, 0))?;
        Ok(())
    }

    /// The page label of physical page `index` (PyMuPDF `Page.get_label`).
    #[must_use]
    pub fn get_label(&self, index: usize) -> String {
        pdf_edit::get_label(&self.store, index)
    }

    // --- metadata (PRD §7) ------------------------------------------------

    /// The document metadata as a [`Metadata`] struct, parsed from the trailer
    /// `/Info` dict plus the format/encryption facts (PyMuPDF `metadata`).
    #[must_use]
    pub fn metadata(&self) -> Metadata {
        let mut md = Metadata::default();
        let (major, minor) = self.version();
        md.format = format!("PDF {major}.{minor}");
        md.encryption = self.encryption_name();

        if let Some(info) = self.info_dict() {
            md.title = info_string(&info, "Title");
            md.author = info_string(&info, "Author");
            md.subject = info_string(&info, "Subject");
            md.keywords = info_string(&info, "Keywords");
            md.creator = info_string(&info, "Creator");
            md.producer = info_string(&info, "Producer");
            md.creation_date = info_string(&info, "CreationDate");
            md.mod_date = info_string(&info, "ModDate");
            md.trapped = info_string(&info, "Trapped");
        }
        md
    }

    /// The resolved `/Info` dictionary, if present in the trailer (honoring an
    /// edit-time override so a freshly-set `/Info` is reflected by `metadata`).
    fn info_dict(&self) -> Option<pdf_core::Dict> {
        if let Some(r) = self.store.effective_trailer_ref("Info") {
            return self.store.resolve(r).ok()?.as_dict().cloned();
        }
        // Fall back to a direct (non-indirect) /Info dict in the original trailer.
        match self.store.trailer().get(&Name::new("Info"))? {
            Object::Reference(r) => self.store.resolve(*r).ok()?.as_dict().cloned(),
            direct => direct.as_dict().cloned(),
        }
    }

    /// The PyMuPDF-style encryption descriptor (e.g. `"Standard V2 R3 128-bit"`),
    /// or empty when unencrypted.
    fn encryption_name(&self) -> String {
        #[cfg(feature = "encryption")]
        {
            if !self.store.is_encrypted() {
                return String::new();
            }
            if let Some(enc) = self.encrypt_dict() {
                let v = enc
                    .get(&Name::new("V"))
                    .and_then(Object::as_i64)
                    .unwrap_or(0);
                let r = enc
                    .get(&Name::new("R"))
                    .and_then(Object::as_i64)
                    .unwrap_or(0);
                let len = enc
                    .get(&Name::new("Length"))
                    .and_then(Object::as_i64)
                    .unwrap_or(40);
                return format!("Standard V{v} R{r} {len}-bit");
            }
            "Standard".to_string()
        }
        #[cfg(not(feature = "encryption"))]
        {
            String::new()
        }
    }

    /// The `/Encrypt` dictionary, if present.
    #[cfg(feature = "encryption")]
    fn encrypt_dict(&self) -> Option<pdf_core::Dict> {
        let enc = self.store.trailer().get(&Name::new("Encrypt"))?;
        let obj = match enc {
            Object::Reference(r) => self.store.resolve(*r).ok()?,
            direct => Arc::new(direct.clone()),
        };
        obj.as_dict().cloned()
    }

    // --- low-level xref read API (PRD §7 P1) ------------------------------

    /// The cross-reference length (PyMuPDF `xref_length`).
    #[must_use]
    pub fn xref_length(&self) -> u32 {
        self.store.xref_length()
    }

    /// The serialized source of object `num` (PyMuPDF `xref_object`).
    ///
    /// # Errors
    ///
    /// Resolution / decode errors propagate.
    pub fn xref_object(&self, num: u32) -> Result<String> {
        Ok(self.store.xref_object(num)?)
    }

    /// The serialized value of key `key` on object `num`, or `None`.
    ///
    /// # Errors
    ///
    /// Resolution errors propagate.
    pub fn xref_get_key(&self, num: u32, key: &str) -> Result<Option<String>> {
        Ok(self.store.xref_get_key(num, key)?)
    }

    /// Whether object `num` is a stream (PyMuPDF `xref_is_stream`).
    ///
    /// # Errors
    ///
    /// Resolution errors propagate.
    pub fn xref_is_stream(&self, num: u32) -> Result<bool> {
        Ok(self.store.xref_is_stream(num)?)
    }

    /// The decoded stream body of object `num` (PyMuPDF `xref_stream`).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if not a stream; decode errors propagate.
    pub fn xref_stream(&self, num: u32) -> Result<Vec<u8>> {
        Ok(self.store.xref_stream(num)?)
    }

    // --- forms (PRD §8.8, PyMuPDF `Document`) -----------------------------

    /// Whether the document has an interactive form (PyMuPDF `is_form_pdf`).
    #[must_use]
    pub fn is_form_pdf(&self) -> bool {
        pdf_edit::is_form_pdf(&self.store)
    }

    /// The fully-qualified names of every terminal form field, in document
    /// order (the keys accepted by [`Document::form_fill`]).
    #[must_use]
    pub fn form_field_names(&self) -> Vec<String> {
        pdf_edit::form_fields(&self.store)
            .iter()
            .map(pdf_edit::Field::field_name)
            .collect()
    }

    /// Sets a form field's value by fully-qualified name (PyMuPDF form fill),
    /// regenerating the widget appearance.
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] when no field matches, the field is read-only, or the
    /// value is out of domain for the field type.
    pub fn form_fill(&self, name: &str, value: &str) -> Result<()> {
        pdf_edit::fill(&self.store, name, value)?;
        Ok(())
    }

    /// Flattens the interactive form into static page content (PyMuPDF
    /// `Document.bake` for widgets / Acrobat "flatten"). Refreshes the page list.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] from the flatten path.
    pub fn form_flatten(&self) -> Result<()> {
        pdf_edit::flatten(&self.store)?;
        self.refresh_pages();
        Ok(())
    }

    // --- embedded files (PRD §8.8, PyMuPDF `embfile_*`) -------------------

    /// Adds an embedded file under name-tree key `name` (PyMuPDF
    /// `embfile_add`). `filename` defaults to `name`; `ufilename` to `filename`.
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] when `name` already exists; propagates object-edit
    /// errors.
    pub fn embfile_add(
        &self,
        name: &str,
        bytes: &[u8],
        filename: Option<&str>,
        ufilename: Option<&str>,
        desc: Option<&str>,
    ) -> Result<()> {
        pdf_edit::embfile_add(&self.store, name, bytes, filename, ufilename, desc)?;
        Ok(())
    }

    /// Reads back the bytes of the embedded file under `name` (PyMuPDF
    /// `embfile_get`).
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] when no such file exists; propagates decode errors.
    pub fn embfile_get(&self, name: &str) -> Result<Vec<u8>> {
        Ok(pdf_edit::embfile_get(&self.store, name)?)
    }

    /// Deletes the embedded file under `name` (PyMuPDF `embfile_del`).
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] when no such file exists.
    pub fn embfile_del(&self, name: &str) -> Result<()> {
        pdf_edit::embfile_del(&self.store, name)?;
        Ok(())
    }

    /// All embedded-file names, in name-tree (byte-sorted) order (PyMuPDF
    /// `embfile_names`).
    #[must_use]
    pub fn embfile_names(&self) -> Vec<String> {
        pdf_edit::embfile_names(&self.store)
    }

    /// The number of embedded files (PyMuPDF `embfile_count`).
    #[must_use]
    pub fn embfile_count(&self) -> usize {
        pdf_edit::embfile_count(&self.store)
    }

    /// Metadata for the embedded file under `name` (PyMuPDF `embfile_info`).
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] when no such file exists.
    pub fn embfile_info(&self, name: &str) -> Result<EmbfileInfo> {
        Ok(pdf_edit::embfile_info(&self.store, name)?)
    }

    // --- sanitization (PRD §8.8, PyMuPDF `scrub` / `bake`) ---------------

    /// Removes sensitive data per the enabled [`ScrubOptions`] (PyMuPDF
    /// `scrub`). Refreshes the page list (links may be removed).
    ///
    /// # Errors
    ///
    /// A typed [`Error`] from the object-edit path.
    pub fn scrub(&self, opts: &ScrubOptions) -> Result<()> {
        pdf_edit::scrub(&self.store, opts)?;
        self.refresh_pages();
        Ok(())
    }

    /// Flattens interactive content into the page content streams (PyMuPDF
    /// `Document.bake`): `annots` bakes non-widget annotations, `widgets` bakes
    /// form fields. Refreshes the page list.
    ///
    /// # Errors
    ///
    /// A typed [`Error`] from the flatten / content-append path.
    pub fn bake(&self, annots: bool, widgets: bool) -> Result<()> {
        pdf_edit::bake(&self.store, annots, widgets)?;
        self.refresh_pages();
        Ok(())
    }
}

// --- page-level edit free functions (PRD §8.9) ---------------------------
//
// The orphan rule forbids inherent `impl Page` here (`Page` is `pdf-core`'s), so
// page links / labels / rotation are free functions the bindings call on a
// `Page` handle (which carries its own store `Arc`).

/// The link annotations on `page` (PyMuPDF `Page.get_links`).
#[must_use]
pub fn page_get_links(page: &Page) -> Vec<Link> {
    pdf_edit::get_links(page.document(), page.number())
}

/// Inserts a URI link on `page`.
///
/// # Errors
///
/// A typed [`Error`] for an out-of-range page or object-edit failure.
pub fn page_insert_link_uri(page: &Page, rect: Rect, uri: &str) -> Result<()> {
    pdf_edit::insert_link(
        page.document(),
        page.number(),
        &rect,
        &pdf_edit::LinkKind::Uri(uri.to_string()),
    )?;
    Ok(())
}

/// Inserts a GoTo link on `page` targeting `target_page`.
///
/// # Errors
///
/// A typed [`Error`] for an out-of-range page or object-edit failure.
pub fn page_insert_link_goto(page: &Page, rect: Rect, target_page: i32) -> Result<()> {
    pdf_edit::insert_link(
        page.document(),
        page.number(),
        &rect,
        &pdf_edit::LinkKind::Goto(target_page),
    )?;
    Ok(())
}

/// Deletes the link annotation `xref` on `page`.
///
/// # Errors
///
/// A typed [`Error`] for an out-of-range page or object-edit failure.
pub fn page_delete_link(page: &Page, xref: u32) -> Result<()> {
    pdf_edit::delete_link(page.document(), page.number(), ObjRef::new(xref, 0))?;
    Ok(())
}

/// The page label of `page` (PyMuPDF `Page.get_label`).
#[must_use]
pub fn page_get_label(page: &Page) -> String {
    pdf_edit::get_label(page.document(), page.number())
}

/// Sets the rotation of `page` (PyMuPDF `Page.set_rotation`).
///
/// # Errors
///
/// A typed [`Error`] from the page-op path.
pub fn page_set_rotation(page: &Page, degrees: i64) -> Result<()> {
    let mut ed = pdf_edit::PageEditor::new(page.document())?;
    ed.set_rotation(page.number(), degrees)?;
    Ok(())
}

// === M4e owned handle types (PRD §9.1 / §9.4) =============================
//
// `pdf_edit::{Annot, Widget, Shape}` all BORROW `&DocumentStore`, so they cannot
// cross the FFI boundary. The facade exposes owned handles that carry an
// `Arc<DocumentStore>` plus the relevant `ObjRef`/page index and reconstruct the
// borrowed type on demand inside each method.

/// An RGB color at the facade boundary, the PyMuPDF `(r, g, b)` tuple
/// convention (each component in `0.0..=1.0`). Mapped to [`Color`] internally.
pub type RgbColor = (f64, f64, f64);

/// The page-leaf reference for `page`'s index, or an error when the page list
/// no longer contains it (after a structural edit).
fn page_leaf(page: &Page) -> Result<ObjRef> {
    pagetree::page_refs(page.document())
        .get(page.number())
        .copied()
        .ok_or_else(|| Error::Syntax(format!("page index {} out of range", page.number())))
}

/// Maps an optional tuple color `(r, g, b)` to a [`Color`].
fn opt_color(c: Option<(f64, f64, f64)>) -> Option<Color> {
    c.map(|(r, g, b)| Color::new(r, g, b))
}

/// A handle to one annotation (PyMuPDF `Annot`). Owns an `Arc<DocumentStore>`
/// plus the page leaf and the annotation's object reference; the borrowed
/// [`pdf_edit::Annot`] is reconstructed on demand for each call.
pub struct AnnotHandle {
    store: Arc<DocumentStore>,
    leaf: ObjRef,
    xref: ObjRef,
}

/// The PyMuPDF `Annot.info` fields (`content` `/Contents`, `name` `/NM`,
/// `title` `/T`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnnotInfo {
    /// `/Contents` text.
    pub content: String,
    /// `/NM` annotation name.
    pub name: String,
    /// `/T` title (author).
    pub title: String,
}

impl AnnotHandle {
    /// Reconstructs the borrowed [`pdf_edit::Annot`] for a method call.
    fn annot(&self) -> pdf_edit::Annot<'_> {
        pdf_edit::Annot::from_ref(&self.store, self.leaf, self.xref)
    }

    /// The annotation object number (PyMuPDF `Annot.xref`).
    #[must_use]
    pub fn xref(&self) -> u32 {
        self.xref.num
    }

    /// The annotation `/Rect` (PyMuPDF `Annot.rect`).
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.annot().rect()
    }

    /// The annotation subtype (PyMuPDF `Annot.type`).
    #[must_use]
    pub fn annot_type(&self) -> AnnotType {
        self.annot().annot_type()
    }

    /// The annotation subtype as its PDF `/Subtype` name string.
    #[must_use]
    pub fn type_string(&self) -> String {
        self.annot().annot_type().pdf_name().to_string()
    }

    /// The annotation info fields (PyMuPDF `Annot.info`).
    #[must_use]
    pub fn info(&self) -> AnnotInfo {
        let a = self.annot();
        AnnotInfo {
            content: a.contents(),
            name: a.name(),
            title: a.title(),
        }
    }

    /// The `(stroke /C, fill /IC)` colors as RGB tuples (PyMuPDF `Annot.colors`).
    #[must_use]
    pub fn colors(&self) -> (Option<RgbColor>, Option<RgbColor>) {
        let a = self.annot();
        let to_tuple = |c: Color| (c.r, c.g, c.b);
        (a.color().map(to_tuple), a.fill_color().map(to_tuple))
    }

    /// The constant opacity `/CA` (PyMuPDF `Annot.opacity`).
    #[must_use]
    pub fn opacity(&self) -> f64 {
        self.annot().opacity()
    }

    /// The border width (PyMuPDF `Annot.border`).
    #[must_use]
    pub fn border_width(&self) -> f64 {
        self.annot().border_width()
    }

    /// The annotation flags `/F` (PyMuPDF `Annot.flags`).
    #[must_use]
    pub fn flags(&self) -> i64 {
        self.annot().flags()
    }

    /// The `/Vertices` (Polygon / PolyLine), as user-space points (PyMuPDF
    /// `Annot.vertices`).
    #[must_use]
    pub fn vertices(&self) -> Vec<Point> {
        self.annot().vertices()
    }

    /// Whether an `/AP /N` appearance stream is present and non-empty.
    #[must_use]
    pub fn has_appearance(&self) -> bool {
        self.annot().has_appearance()
    }

    /// Sets the annotation `/Rect` (PyMuPDF `Annot.set_rect`). Call
    /// [`AnnotHandle::update`] to regenerate the appearance.
    ///
    /// # Errors
    ///
    /// Propagates object-edit errors.
    pub fn set_rect(&self, rect: Rect) -> Result<()> {
        self.annot().set_rect(rect)?;
        Ok(())
    }

    /// Sets the stroke `/C` and/or fill `/IC` colors (PyMuPDF
    /// `Annot.set_colors`). Each `None` leaves the key untouched.
    ///
    /// # Errors
    ///
    /// Propagates object-edit errors.
    pub fn set_colors(
        &self,
        stroke: Option<(f64, f64, f64)>,
        fill: Option<(f64, f64, f64)>,
    ) -> Result<()> {
        self.annot()
            .set_colors(opt_color(stroke), opt_color(fill))?;
        Ok(())
    }

    /// Sets the constant opacity `/CA` (PyMuPDF `Annot.set_opacity`).
    ///
    /// # Errors
    ///
    /// Propagates object-edit errors.
    pub fn set_opacity(&self, opacity: f64) -> Result<()> {
        self.annot().set_opacity(opacity)?;
        Ok(())
    }

    /// Sets the border width (PyMuPDF `Annot.set_border`).
    ///
    /// # Errors
    ///
    /// Propagates object-edit errors.
    pub fn set_border(&self, width: f64) -> Result<()> {
        self.annot().set_border(width)?;
        Ok(())
    }

    /// Sets the annotation flags `/F` (PyMuPDF `Annot.set_flags`).
    ///
    /// # Errors
    ///
    /// Propagates object-edit errors.
    pub fn set_flags(&self, flags: i64) -> Result<()> {
        self.annot().set_flags(flags)?;
        Ok(())
    }

    /// Sets the info fields `/Contents`, `/T` (title), `/NM` (name) (PyMuPDF
    /// `Annot.set_info`). Each `None` leaves the key untouched.
    ///
    /// # Errors
    ///
    /// Propagates object-edit errors.
    pub fn set_info(
        &self,
        content: Option<&str>,
        title: Option<&str>,
        name: Option<&str>,
    ) -> Result<()> {
        self.annot().set_info(content, title, name)?;
        Ok(())
    }

    /// Regenerates the `/AP /N` appearance stream from current properties
    /// (PyMuPDF `Annot.update`).
    ///
    /// # Errors
    ///
    /// Propagates resolve / object-edit errors.
    pub fn update(&self) -> Result<()> {
        self.annot().update()?;
        Ok(())
    }
}

/// A handle to one form widget (PyMuPDF `Widget`). Owns an `Arc<DocumentStore>`
/// plus the widget's object reference; the borrowed [`pdf_edit::Widget`] is
/// reconstructed on demand.
pub struct WidgetHandle {
    store: Arc<DocumentStore>,
    xref: ObjRef,
}

impl WidgetHandle {
    /// Reconstructs the borrowed [`pdf_edit::Widget`] for a method call.
    fn widget(&self) -> pdf_edit::Widget<'_> {
        pdf_edit::Widget::from_ref(&self.store, self.xref)
    }

    /// The widget annotation object number (PyMuPDF `Widget.xref`).
    #[must_use]
    pub fn xref(&self) -> u32 {
        self.xref.num
    }

    /// The widget `/Rect` (PyMuPDF `Widget.rect`).
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.widget().rect()
    }

    /// The field type (PyMuPDF `Widget.field_type`).
    #[must_use]
    pub fn field_type(&self) -> FieldType {
        self.widget().field_type()
    }

    /// The field-type string (PyMuPDF `Widget.field_type_string`).
    #[must_use]
    pub fn field_type_string(&self) -> String {
        self.widget().field_type_string().to_string()
    }

    /// The fully-qualified field name (PyMuPDF `Widget.field_name`).
    #[must_use]
    pub fn field_name(&self) -> String {
        self.widget().field_name()
    }

    /// The field label `/TU` (PyMuPDF `Widget.field_label`).
    #[must_use]
    pub fn field_label(&self) -> Option<String> {
        self.widget().field_label()
    }

    /// The current field value (PyMuPDF `Widget.field_value`).
    #[must_use]
    pub fn field_value(&self) -> Option<String> {
        self.widget().field_value()
    }

    /// The field flags `/Ff` (PyMuPDF `Widget.field_flags`).
    #[must_use]
    pub fn field_flags(&self) -> i64 {
        self.widget().field_flags()
    }

    /// The choice option values (PyMuPDF `Widget.choice_values`).
    #[must_use]
    pub fn choice_values(&self) -> Vec<String> {
        self.widget().choice_values()
    }

    /// The widget on-state names for checkbox / radio buttons (PyMuPDF
    /// `Widget.button_states`).
    #[must_use]
    pub fn button_states(&self) -> Vec<String> {
        self.widget().button_states()
    }

    /// Sets the field value through the owning field (PyMuPDF
    /// `Widget.field_value = …`), regenerating appearances.
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] for a read-only / signature / pushbutton field or an
    /// out-of-domain value; propagates object-edit errors.
    pub fn set_field_value(&self, value: &str) -> Result<()> {
        self.widget().set_field_value(value)?;
        Ok(())
    }
}

/// One recorded `Shape` draw primitive (replayed at commit time). Coordinates
/// are in PyMuPDF top-left page space, exactly as the `draw_*` methods receive
/// them.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeOp {
    /// `draw_line(p1, p2)`.
    Line(Point, Point),
    /// `draw_rect(rect)`.
    Rect(Rect),
    /// `draw_circle(center, radius)`.
    Circle(Point, f64),
    /// `draw_oval(rect)`.
    Oval(Rect),
    /// `draw_bezier(p1, p2, p3, p4)`.
    Bezier(Point, Point, Point, Point),
    /// `draw_polyline(points)`.
    Polyline(Vec<Point>),
    /// `draw_curve(points)`.
    Curve(Vec<Point>),
}

/// The paint parameters for one finished `Shape` block (PyMuPDF
/// `Shape.finish`). Tuple colors map to [`Color`] at replay time.
#[derive(Clone, Debug, PartialEq)]
pub struct FinishParams {
    /// Stroke color `(r, g, b)`, or `None` for no stroke.
    pub color: Option<(f64, f64, f64)>,
    /// Fill color `(r, g, b)`, or `None` for no fill.
    pub fill: Option<(f64, f64, f64)>,
    /// Stroke line width.
    pub width: f64,
    /// Dash-pattern string (PDF `d` operand body, e.g. `"[3] 0"`), or `None`.
    pub dashes: Option<String>,
    /// Use the even-odd fill rule.
    pub even_odd: bool,
    /// Close the current sub-path before painting.
    pub close_path: bool,
}

/// A path/paint builder over one page (PyMuPDF `Shape`). Because
/// [`pdf_edit::Shape`] borrows the store and accumulates a buffer, it cannot be
/// stored across the FFI boundary; this handle records the draw primitives and
/// finished blocks and replays them into a fresh [`pdf_edit::Shape`] at
/// [`ShapeHandle::commit`]. Multiple [`ShapeHandle::finish`] calls before one
/// `commit` are supported (each `finish` flushes a styled block, `commit` writes
/// them all at once — see `pdf_edit::Shape::finish` / `commit`).
pub struct ShapeHandle {
    store: Arc<DocumentStore>,
    index: usize,
    /// Primitives drawn since the last `finish`.
    current: Vec<ShapeOp>,
    /// Finished styled blocks, in order.
    committed_blocks: Vec<(Vec<ShapeOp>, FinishParams)>,
}

impl ShapeHandle {
    /// Records a straight segment (PyMuPDF `Shape.draw_line`).
    pub fn draw_line(&mut self, p1: Point, p2: Point) {
        self.current.push(ShapeOp::Line(p1, p2));
    }

    /// Records a rectangle (PyMuPDF `Shape.draw_rect`).
    pub fn draw_rect(&mut self, rect: Rect) {
        self.current.push(ShapeOp::Rect(rect));
    }

    /// Records a circle (PyMuPDF `Shape.draw_circle`).
    pub fn draw_circle(&mut self, center: Point, radius: f64) {
        self.current.push(ShapeOp::Circle(center, radius));
    }

    /// Records an ellipse fitting `rect` (PyMuPDF `Shape.draw_oval`).
    pub fn draw_oval(&mut self, rect: Rect) {
        self.current.push(ShapeOp::Oval(rect));
    }

    /// Records a cubic Bézier (PyMuPDF `Shape.draw_bezier`).
    pub fn draw_bezier(&mut self, p1: Point, p2: Point, p3: Point, p4: Point) {
        self.current.push(ShapeOp::Bezier(p1, p2, p3, p4));
    }

    /// Records a polyline (PyMuPDF `Shape.draw_polyline`).
    pub fn draw_polyline(&mut self, points: Vec<Point>) {
        self.current.push(ShapeOp::Polyline(points));
    }

    /// Records a smooth curve (PyMuPDF `Shape.draw_curve`).
    pub fn draw_curve(&mut self, points: Vec<Point>) {
        self.current.push(ShapeOp::Curve(points));
    }

    /// Finishes the current styled block with the given paint parameters
    /// (PyMuPDF `Shape.finish`). Subsequent draws begin a new block.
    pub fn finish(&mut self, params: FinishParams) {
        let ops = std::mem::take(&mut self.current);
        self.committed_blocks.push((ops, params));
    }

    /// Replays the recorded blocks into a fresh [`pdf_edit::Shape`] and writes
    /// them to the page (PyMuPDF `Shape.commit`). An unfinished trailing block
    /// gets a default black stroke at width 1.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for an out-of-range page; propagates content-edit
    /// errors.
    pub fn commit(mut self) -> Result<()> {
        // A drawn-but-unfinished trailing block defaults to a black stroke.
        if !self.current.is_empty() {
            let ops = std::mem::take(&mut self.current);
            self.committed_blocks.push((
                ops,
                FinishParams {
                    color: Some((0.0, 0.0, 0.0)),
                    fill: None,
                    width: 1.0,
                    dashes: None,
                    even_odd: false,
                    close_path: false,
                },
            ));
        }
        if self.committed_blocks.is_empty() {
            return Ok(());
        }
        let mut shape = pdf_edit::Shape::new(&self.store, self.index)?;
        for (ops, params) in &self.committed_blocks {
            for op in ops {
                match op {
                    ShapeOp::Line(p1, p2) => {
                        shape.draw_line(*p1, *p2);
                    }
                    ShapeOp::Rect(r) => shape.draw_rect(*r),
                    ShapeOp::Circle(c, r) => shape.draw_circle(*c, *r),
                    ShapeOp::Oval(r) => shape.draw_oval(*r),
                    ShapeOp::Bezier(p1, p2, p3, p4) => shape.draw_bezier(*p1, *p2, *p3, *p4),
                    ShapeOp::Polyline(pts) => shape.draw_polyline(pts),
                    ShapeOp::Curve(pts) => shape.draw_curve(pts),
                }
            }
            shape.finish(
                opt_color(params.color),
                opt_color(params.fill),
                params.width,
                params.dashes.as_deref(),
                params.even_odd,
                params.close_path,
            );
        }
        shape.commit()?;
        Ok(())
    }
}

// === M4e page-level content / draw free functions (PRD §8.8) =============
//
// The orphan rule forbids inherent `impl Page` here; these mirror the existing
// `page_*` free functions, taking `&Page` and tuple colors and delegating to
// `pdf_edit::*` with `page.document()` + `page.number()`.

/// Inserts `text` at `point` (PyMuPDF `Page.insert_text`), returning the number
/// of lines written.
///
/// # Errors
///
/// A typed [`Error`] from the content-insert path (e.g. an unparseable
/// `fontfile`).
#[allow(clippy::too_many_arguments)]
pub fn page_insert_text(
    page: &Page,
    point: Point,
    text: &str,
    fontname: &str,
    fontsize: f64,
    color: (f64, f64, f64),
    fontfile: Option<&[u8]>,
) -> Result<usize> {
    let opts = pdf_edit::TextOptions {
        fontname,
        fontsize,
        color: Color::new(color.0, color.1, color.2),
        fontfile,
        align: Align::Left,
    };
    Ok(pdf_edit::insert_text(
        page.document(),
        page.number(),
        point,
        text,
        &opts,
    )?)
}

/// Inserts wrapped, aligned `text` into `rect` (PyMuPDF `Page.insert_textbox`),
/// returning the unused height (positive) or overflow (negative).
///
/// # Errors
///
/// A typed [`Error`] from the content-insert path.
#[allow(clippy::too_many_arguments)]
pub fn page_insert_textbox(
    page: &Page,
    rect: Rect,
    text: &str,
    fontname: &str,
    fontsize: f64,
    color: (f64, f64, f64),
    align: Align,
    fontfile: Option<&[u8]>,
) -> Result<f64> {
    let opts = pdf_edit::TextOptions {
        fontname,
        fontsize,
        color: Color::new(color.0, color.1, color.2),
        fontfile,
        align,
    };
    Ok(pdf_edit::insert_textbox(
        page.document(),
        page.number(),
        rect,
        text,
        &opts,
    )?)
}

/// Inserts a JPEG image filling `rect` (PyMuPDF `Page.insert_image`), returning
/// the chosen XObject resource name.
///
/// # Errors
///
/// [`Error::Unsupported`] when `jpeg` is not a parseable JPEG.
pub fn page_insert_image_jpeg(page: &Page, rect: Rect, jpeg: &[u8]) -> Result<String> {
    Ok(pdf_edit::insert_image_jpeg(
        page.document(),
        page.number(),
        rect,
        jpeg,
    )?)
}

/// Inserts a raw 8-bit RGB image filling `rect` (PyMuPDF `Page.insert_image`),
/// returning the chosen XObject resource name.
///
/// # Errors
///
/// [`Error::Unsupported`] when `pixels.len() != width * height * 3`.
pub fn page_insert_image_rgb(
    page: &Page,
    rect: Rect,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<String> {
    Ok(pdf_edit::insert_image_rgb(
        page.document(),
        page.number(),
        rect,
        width,
        height,
        pixels,
    )?)
}

/// Draws a stroked line (PyMuPDF `Page.draw_line`).
///
/// # Errors
///
/// A typed [`Error`] from the content-edit path.
pub fn page_draw_line(
    page: &Page,
    p1: Point,
    p2: Point,
    color: (f64, f64, f64),
    width: f64,
) -> Result<()> {
    pdf_edit::draw_line(
        page.document(),
        page.number(),
        p1,
        p2,
        Color::new(color.0, color.1, color.2),
        width,
    )?;
    Ok(())
}

/// Draws a rectangle (PyMuPDF `Page.draw_rect`).
///
/// # Errors
///
/// A typed [`Error`] from the content-edit path.
pub fn page_draw_rect(
    page: &Page,
    rect: Rect,
    color: Option<(f64, f64, f64)>,
    fill: Option<(f64, f64, f64)>,
    width: f64,
) -> Result<()> {
    pdf_edit::draw_rect(
        page.document(),
        page.number(),
        rect,
        opt_color(color),
        opt_color(fill),
        width,
    )?;
    Ok(())
}

/// Draws a circle (PyMuPDF `Page.draw_circle`).
///
/// # Errors
///
/// A typed [`Error`] from the content-edit path.
pub fn page_draw_circle(
    page: &Page,
    center: Point,
    radius: f64,
    color: Option<(f64, f64, f64)>,
    fill: Option<(f64, f64, f64)>,
    width: f64,
) -> Result<()> {
    pdf_edit::draw_circle(
        page.document(),
        page.number(),
        center,
        radius,
        opt_color(color),
        opt_color(fill),
        width,
    )?;
    Ok(())
}

/// Draws an ellipse fitting `rect` (PyMuPDF `Page.draw_oval`).
///
/// # Errors
///
/// A typed [`Error`] from the content-edit path.
pub fn page_draw_oval(
    page: &Page,
    rect: Rect,
    color: Option<(f64, f64, f64)>,
    fill: Option<(f64, f64, f64)>,
    width: f64,
) -> Result<()> {
    pdf_edit::draw_oval(
        page.document(),
        page.number(),
        rect,
        opt_color(color),
        opt_color(fill),
        width,
    )?;
    Ok(())
}

/// Draws a single cubic Bézier (PyMuPDF `Page.draw_bezier`).
///
/// # Errors
///
/// A typed [`Error`] from the content-edit path.
#[allow(clippy::too_many_arguments)]
pub fn page_draw_bezier(
    page: &Page,
    p1: Point,
    p2: Point,
    p3: Point,
    p4: Point,
    color: (f64, f64, f64),
    width: f64,
) -> Result<()> {
    pdf_edit::draw_bezier(
        page.document(),
        page.number(),
        p1,
        p2,
        p3,
        p4,
        Color::new(color.0, color.1, color.2),
        width,
    )?;
    Ok(())
}

/// Draws a polyline (PyMuPDF `Page.draw_polyline`).
///
/// # Errors
///
/// A typed [`Error`] from the content-edit path.
pub fn page_draw_polyline(
    page: &Page,
    points: &[Point],
    color: (f64, f64, f64),
    width: f64,
) -> Result<()> {
    pdf_edit::draw_polyline(
        page.document(),
        page.number(),
        points,
        Color::new(color.0, color.1, color.2),
        width,
    )?;
    Ok(())
}

/// Draws a smooth curve (PyMuPDF `Page.draw_curve`).
///
/// # Errors
///
/// A typed [`Error`] from the content-edit path.
pub fn page_draw_curve(
    page: &Page,
    points: &[Point],
    color: (f64, f64, f64),
    width: f64,
) -> Result<()> {
    pdf_edit::draw_curve(
        page.document(),
        page.number(),
        points,
        Color::new(color.0, color.1, color.2),
        width,
    )?;
    Ok(())
}

/// Opens a new [`ShapeHandle`] on `page` (PyMuPDF `Page.new_shape`). The
/// underlying [`pdf_edit::Shape`] is built at [`ShapeHandle::commit`] time.
#[must_use]
pub fn page_new_shape(page: &Page) -> ShapeHandle {
    ShapeHandle {
        store: Arc::clone(page.document()),
        index: page.number(),
        current: Vec::new(),
        committed_blocks: Vec::new(),
    }
}

// === M4e page-level annotation free functions (PRD §8.8) =================

/// Builds an [`AnnotHandle`] for the annotation `xref` on `page`.
///
/// # Errors
///
/// [`Error::Syntax`] when `page`'s index is out of range.
fn annot_handle(page: &Page, xref: ObjRef) -> Result<AnnotHandle> {
    Ok(AnnotHandle {
        store: Arc::clone(page.document()),
        leaf: page_leaf(page)?,
        xref,
    })
}

/// `page.add_text_annot` — a sticky-note `/Text` annotation.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_text_annot(
    page: &Page,
    point: Point,
    text: &str,
    icon: &str,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_text_annot(page.document(), page.number(), point, text, icon)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_freetext_annot` — a `/FreeText` box. `align` is the `/Q` value
/// (0 left, 1 center, 2 right).
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
#[allow(clippy::too_many_arguments)]
pub fn page_add_freetext_annot(
    page: &Page,
    rect: Rect,
    text: &str,
    fontsize: f64,
    color: (f64, f64, f64),
    fill: Option<(f64, f64, f64)>,
    align: i64,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_freetext_annot(
        page.document(),
        page.number(),
        rect,
        text,
        fontsize,
        Color::new(color.0, color.1, color.2),
        opt_color(fill),
        align,
    )?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_highlight_annot` — a `/Highlight` over `quads`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_highlight_annot(page: &Page, quads: &[Quad]) -> Result<AnnotHandle> {
    let a = pdf_edit::add_highlight_annot(page.document(), page.number(), quads)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_underline_annot` — an `/Underline` over `quads`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_underline_annot(page: &Page, quads: &[Quad]) -> Result<AnnotHandle> {
    let a = pdf_edit::add_underline_annot(page.document(), page.number(), quads)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_strikeout_annot` — a `/StrikeOut` over `quads`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_strikeout_annot(page: &Page, quads: &[Quad]) -> Result<AnnotHandle> {
    let a = pdf_edit::add_strikeout_annot(page.document(), page.number(), quads)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_squiggly_annot` — a `/Squiggly` over `quads`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_squiggly_annot(page: &Page, quads: &[Quad]) -> Result<AnnotHandle> {
    let a = pdf_edit::add_squiggly_annot(page.document(), page.number(), quads)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_rect_annot` — a `/Square` annotation fitting `rect`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_rect_annot(
    page: &Page,
    rect: Rect,
    color: Option<(f64, f64, f64)>,
    fill: Option<(f64, f64, f64)>,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_rect_annot(
        page.document(),
        page.number(),
        rect,
        opt_color(color),
        opt_color(fill),
    )?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_circle_annot` — a `/Circle` annotation fitting `rect`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_circle_annot(
    page: &Page,
    rect: Rect,
    color: Option<(f64, f64, f64)>,
    fill: Option<(f64, f64, f64)>,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_circle_annot(
        page.document(),
        page.number(),
        rect,
        opt_color(color),
        opt_color(fill),
    )?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_line_annot` — a `/Line` from `p1` to `p2`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_line_annot(
    page: &Page,
    p1: Point,
    p2: Point,
    color: Option<(f64, f64, f64)>,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_line_annot(page.document(), page.number(), p1, p2, opt_color(color))?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_polygon_annot` — a closed `/Polygon` through `points`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_polygon_annot(
    page: &Page,
    points: &[Point],
    color: Option<(f64, f64, f64)>,
    fill: Option<(f64, f64, f64)>,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_polygon_annot(
        page.document(),
        page.number(),
        points,
        opt_color(color),
        opt_color(fill),
    )?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_polyline_annot` — an open `/PolyLine` through `points`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_polyline_annot(
    page: &Page,
    points: &[Point],
    color: Option<(f64, f64, f64)>,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_polyline_annot(page.document(), page.number(), points, opt_color(color))?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_ink_annot` — an `/Ink` annotation of free-form `strokes` (each a
/// polyline).
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_ink_annot(
    page: &Page,
    strokes: &[Vec<Point>],
    color: Option<(f64, f64, f64)>,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_ink_annot(page.document(), page.number(), strokes, opt_color(color))?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_stamp_annot` — a `/Stamp` annotation fitting `rect`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_stamp_annot(page: &Page, rect: Rect, stamp: &str) -> Result<AnnotHandle> {
    let a = pdf_edit::add_stamp_annot(page.document(), page.number(), rect, stamp)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_file_annot` — a `/FileAttachment` at `point` embedding `bytes`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_file_annot(
    page: &Page,
    point: Point,
    bytes: &[u8],
    filename: &str,
) -> Result<AnnotHandle> {
    let a = pdf_edit::add_file_annot(page.document(), page.number(), point, bytes, filename)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// `page.add_redact_annot` — a `/Redact` marker over `rect`.
///
/// # Errors
///
/// A typed [`Error`] from the annotation path.
pub fn page_add_redact_annot(
    page: &Page,
    rect: Rect,
    fill: Option<(f64, f64, f64)>,
    text: Option<&str>,
) -> Result<AnnotHandle> {
    let a =
        pdf_edit::add_redact_annot(page.document(), page.number(), rect, opt_color(fill), text)?;
    let xref = a.xref();
    annot_handle(page, xref)
}

/// The annotation handles on `page`, in `/Annots` order (PyMuPDF
/// `Page.annots()`).
///
/// # Errors
///
/// [`Error::Syntax`] when `page`'s index is out of range.
pub fn page_annots(page: &Page) -> Result<Vec<AnnotHandle>> {
    let leaf = page_leaf(page)?;
    let store = Arc::clone(page.document());
    Ok(pdf_edit::annot_refs(page.document(), page.number())
        .into_iter()
        .map(|xref| AnnotHandle {
            store: Arc::clone(&store),
            leaf,
            xref,
        })
        .collect())
}

/// The first annotation on `page`, if any (PyMuPDF `Page.first_annot`).
///
/// # Errors
///
/// [`Error::Syntax`] when `page`'s index is out of range.
pub fn page_first_annot(page: &Page) -> Result<Option<AnnotHandle>> {
    Ok(page_annots(page)?.into_iter().next())
}

/// The object numbers of every annotation on `page`, in `/Annots` order.
#[must_use]
pub fn page_annot_xrefs(page: &Page) -> Vec<u32> {
    pdf_edit::annot_refs(page.document(), page.number())
        .iter()
        .map(|r| r.num)
        .collect()
}

/// The number of annotations on `page` (PyMuPDF `Page.annot_count`).
#[must_use]
pub fn page_annot_count(page: &Page) -> usize {
    pdf_edit::annot_count(page.document(), page.number())
}

/// The `/NM` names of every annotation on `page` (PyMuPDF `Page.annot_names`).
#[must_use]
pub fn page_annot_names(page: &Page) -> Vec<String> {
    pdf_edit::annot_names(page.document(), page.number())
}

/// Deletes the annotation `xref` from `page` (PyMuPDF `Page.delete_annot`).
///
/// # Errors
///
/// A typed [`Error`] for an out-of-range page; propagates object-edit errors.
pub fn page_delete_annot(page: &Page, xref: u32) -> Result<()> {
    pdf_edit::delete_annot(page.document(), page.number(), ObjRef::new(xref, 0))?;
    Ok(())
}

// === M4e page-level redaction / drawings free functions (PRD §8.8) =======

/// Applies the page's `/Redact` annotations destructively (PyMuPDF
/// `Page.apply_redactions`), returning the number applied.
///
/// # Errors
///
/// [`Error::Redaction`] when a redaction cannot be applied safely; propagates
/// object-edit errors.
pub fn page_apply_redactions(page: &Page) -> Result<usize> {
    Ok(pdf_edit::apply_redactions(page.document(), page.number())?)
}

/// The vector paths of `page` in PyMuPDF device space (PyMuPDF
/// `Page.get_drawings`).
#[must_use]
pub fn page_get_drawings(page: &Page) -> Vec<Drawing> {
    pdf_edit::get_drawings(page.document(), page.number())
}

/// The raw vector paths of `page` in PDF user space (PyMuPDF
/// `Page.get_cdrawings`).
#[must_use]
pub fn page_get_cdrawings(page: &Page) -> Vec<Drawing> {
    pdf_edit::get_cdrawings(page.document(), page.number())
}

// === M4e page-level form free functions (PRD §8.8) =======================

/// The form widget handles on `page` (PyMuPDF `Page.widgets()`).
#[must_use]
pub fn page_widgets(page: &Page) -> Vec<WidgetHandle> {
    let store = Arc::clone(page.document());
    pdf_edit::widget_refs(page.document(), page.number())
        .into_iter()
        .map(|xref| WidgetHandle {
            store: Arc::clone(&store),
            xref,
        })
        .collect()
}

/// The first form widget on `page`, if any (PyMuPDF `Page.first_widget`).
#[must_use]
pub fn page_first_widget(page: &Page) -> Option<WidgetHandle> {
    page_widgets(page).into_iter().next()
}

/// Reads `/Info` key `key` as a UTF-decoded string (PDFDocEncoding / UTF-16BE
/// BOM auto-detected — the common cases), or `None` if absent.
fn info_string(info: &pdf_core::Dict, key: &str) -> Option<String> {
    let s = info.get(&Name::new(key)).and_then(Object::as_string)?;
    Some(decode_pdf_text(s.as_bytes()))
}

/// Decodes a PDF text string: UTF-16BE when it carries the `FE FF` BOM, else
/// PDFDocEncoding approximated by Latin-1 (PRD §8.7 string encodings; full
/// PDFDocEncoding tables land with the writer in M3).
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        // UTF-8 BOM (PDF 2.0).
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        // PDFDocEncoding ≈ Latin-1 for the ASCII/Latin range.
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// PyMuPDF-compatible document metadata (PRD §7). All textual fields default to
/// the empty string when absent (matching PyMuPDF, which never returns `None`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    /// `"PDF M.N"`.
    pub format: String,
    /// The encryption descriptor, or empty if unencrypted.
    pub encryption: String,
    /// `/Info /Title`.
    pub title: Option<String>,
    /// `/Info /Author`.
    pub author: Option<String>,
    /// `/Info /Subject`.
    pub subject: Option<String>,
    /// `/Info /Keywords`.
    pub keywords: Option<String>,
    /// `/Info /Creator`.
    pub creator: Option<String>,
    /// `/Info /Producer`.
    pub producer: Option<String>,
    /// `/Info /CreationDate` (`D:YYYYMMDDHHmmSS±HH'mm'`, verbatim).
    pub creation_date: Option<String>,
    /// `/Info /ModDate`.
    pub mod_date: Option<String>,
    /// `/Info /Trapped`.
    pub trapped: Option<String>,
}

impl Metadata {
    /// The PyMuPDF `metadata` dict as ordered `(key, value)` pairs. Absent text
    /// fields are the empty string, exactly as PyMuPDF reports them.
    #[must_use]
    pub fn as_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("format", self.format.clone()),
            ("title", self.title.clone().unwrap_or_default()),
            ("author", self.author.clone().unwrap_or_default()),
            ("subject", self.subject.clone().unwrap_or_default()),
            ("keywords", self.keywords.clone().unwrap_or_default()),
            ("creator", self.creator.clone().unwrap_or_default()),
            ("producer", self.producer.clone().unwrap_or_default()),
            (
                "creationDate",
                self.creation_date.clone().unwrap_or_default(),
            ),
            ("modDate", self.mod_date.clone().unwrap_or_default()),
            ("trapped", self.trapped.clone().unwrap_or_default()),
            ("encryption", self.encryption.clone()),
        ]
    }
}
