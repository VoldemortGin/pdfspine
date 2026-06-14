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
pub mod text;

use std::path::Path;
use std::sync::{Arc, RwLock};

use pdf_core::object::{Name, Object};
use pdf_core::source::MmapMode;
use pdf_core::{DocumentStore, Limits, ObjRef};

pub use error::{Error, Result};
pub use pdf_core::page::Page;
pub use pdf_core::repair::ParseMode;
pub use pdf_core::{OnRepaired, SaveOptions, XrefStyle};

// Editing types surfaced to the bindings (PRD §8.9).
pub use pdf_edit::{Link, LinkKind, TocEntry};

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
