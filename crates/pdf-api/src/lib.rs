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

use std::path::Path;
use std::sync::Arc;

use pdf_core::object::{Name, Object};
use pdf_core::source::MmapMode;
use pdf_core::{DocumentStore, Limits, ObjRef};

pub use error::{Error, Result};
pub use pdf_core::page::Page;
pub use pdf_core::repair::ParseMode;

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
    pages: Arc<Vec<ObjRef>>,
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
            pages: Arc::new(pages),
        }
    }

    // --- pages ------------------------------------------------------------

    /// The number of pages (PRD §3.4 `page_count`).
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Loads the page at zero-based `index` (PyMuPDF `load_page`). A negative
    /// PyMuPDF index is the caller's concern; this takes `usize`.
    ///
    /// # Errors
    ///
    /// [`Error::Syntax`] when `index` is out of range.
    pub fn load_page(&self, index: usize) -> Result<Page> {
        let page_ref = self
            .pages
            .get(index)
            .copied()
            .ok_or_else(|| Error::Syntax(format!("page index {index} out of range")))?;
        Ok(Page::new(Arc::clone(&self.store), index, page_ref))
    }

    /// An iterator over every page in order.
    pub fn pages(&self) -> impl Iterator<Item = Page> + '_ {
        let store = Arc::clone(&self.store);
        self.pages
            .iter()
            .enumerate()
            .map(move |(i, r)| Page::new(Arc::clone(&store), i, *r))
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

    /// The resolved `/Info` dictionary, if present in the trailer.
    fn info_dict(&self) -> Option<pdf_core::Dict> {
        let info_ref = self.store.trailer().get(&Name::new("Info"))?;
        let obj = match info_ref {
            Object::Reference(r) => self.store.resolve(*r).ok()?,
            direct => Arc::new(direct.clone()),
        };
        obj.as_dict().cloned()
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
