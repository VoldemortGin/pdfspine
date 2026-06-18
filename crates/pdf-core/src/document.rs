//! The [`DocumentStore`] — owns the source, cross-reference table, trailer and
//! the lazy object arena (PRD §9.2).
//!
//! Opening a document parses only its skeleton: the `%PDF-` header (recording
//! any `header_offset` bias), the cross-reference chain ([`crate::xref`]) and
//! the trailer. **No object bodies are loaded eagerly** (PRD §9.2). Each object
//! is parsed on first [`DocumentStore::resolve`], cached as an [`Arc<Object>`]
//! in an `RwLock`-guarded arena (read on hit, brief write on fill — PRD §9.7),
//! and thereafter returned from the cache.
//!
//! Graph walks are guarded: [`DocumentStore::resolve`] follows
//! reference→reference→… transparently with **cycle detection**
//! ([`Error::ReferenceCycle`]) and a **recursion-depth limit**
//! ([`Limits::max_recursion_depth`]). Every byte read goes through the
//! bounds-checked [`Source`], so malformed offsets are typed errors, never
//! panics (PRD §9.6).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};

use bytes::Bytes;

use crate::changeset::{Change, ChangeSet};
use crate::error::{Error, LimitKind, Result};
use crate::interner::NameInterner;
use crate::lexer::Lexer;
use crate::limits::Limits;
use crate::object::parse::Parser;
use crate::object::{Dict, Name, ObjRef, Object, StreamData, StreamObj};
use crate::repair::{self, Diagnostics, ParseMode, RepairAction, Warning, WarningKind};
use crate::source::{MmapMode, Source};
use crate::xref::{parse_xref_chain, XrefEntry, XrefTable};

/// The PDF version `(major, minor)` from the header / catalog `/Version`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Version {
    /// Major version (always 1 or 2 for real PDFs).
    pub major: u8,
    /// Minor version.
    pub minor: u8,
}

/// A parsed-but-not-domain-interpreted document: source bytes + cross-reference
/// machinery + lazy object access (PRD §9.2). Page-tree / `Document` façade are
/// M1f; encryption is M1e.
#[derive(Debug)]
pub struct DocumentStore {
    source: Source,
    xref: XrefTable,
    trailer: Dict,
    version: Version,
    header_offset: usize,
    parse_was_repaired: bool,
    /// Set once [`crate::DocumentStore::mark_redaction_applied`] is called: a
    /// redacted document must be **fully rewritten** so no pre-redaction bytes
    /// leak in an appended incremental revision (PRD §8.8 surface 6). Interior
    /// mutability so `apply_redactions(&DocumentStore, …)` can taint the doc.
    redaction_applied: std::sync::atomic::AtomicBool,
    mode: ParseMode,
    diagnostics: Diagnostics,
    limits: Limits,
    interner: RwLock<NameInterner>,
    /// Lazy object arena: object number → cached resolved object (PRD §9.2).
    arena: RwLock<HashMap<u32, Arc<Object>>>,
    /// Pending edits overlaid on the original cross-reference (PRD §8.7/§9.2).
    /// Empty after open; consulted by `get_object`/`resolve` before the arena.
    changes: RwLock<ChangeSet>,
    /// Trailer-key overrides set by edits (e.g. a newly created `/Info` or
    /// `/Encrypt` reference). The writer prefers these over the original trailer
    /// so metadata-write / encryption-write survive a full save (PRD §8.7/§8.9).
    /// `Object::Null` for a key means "remove that trailer key".
    trailer_overrides: RwLock<Dict>,
    /// The Standard Security Handler, present iff the trailer has `/Encrypt`
    /// (PRD §8.4). `None` for an unencrypted document. Behind the `encryption`
    /// feature so the default build does not depend on `pdf-crypto` (PRD §9.1).
    #[cfg(feature = "encryption")]
    decryptor: RwLock<Option<pdf_crypto::Decryptor>>,
    /// The object number of the `/Encrypt` dictionary, if it is an indirect
    /// reference (so its own strings are never decrypted — PRD §8.4 exemption).
    #[cfg(feature = "encryption")]
    encrypt_obj_num: Option<u32>,
}

impl DocumentStore {
    // --- opening ----------------------------------------------------------

    /// Opens a document from in-memory bytes with the given [`Limits`], in the
    /// default [`ParseMode::Lenient`] (best-effort: repair + warnings, PRD §8.2).
    ///
    /// Parses the header, locates and walks the cross-reference chain and reads
    /// the trailer; **does not** load object bodies (PRD §9.2). On any failure of
    /// the normal path — broken xref, failed validation gate — it falls back to a
    /// full-file object scan ([`crate::repair`]).
    ///
    /// # Errors
    ///
    /// [`Error::Source`] if the buffer is empty, [`Error::Unsupported`] for a
    /// missing header, or a typed error if even repair cannot recover a usable
    /// document.
    pub fn from_bytes(bytes: impl Into<Bytes>, limits: Limits) -> Result<Self> {
        Self::from_bytes_with(bytes, ParseMode::Lenient, limits)
    }

    /// Opens from bytes with an explicit [`ParseMode`] (PRD §8.2). `Strict`
    /// surfaces the first typed error where `Lenient` would repair.
    ///
    /// # Errors
    ///
    /// See [`DocumentStore::from_bytes`]; in `Strict` mode a broken
    /// cross-reference is returned as the typed error rather than repaired.
    pub fn from_bytes_with(
        bytes: impl Into<Bytes>,
        mode: ParseMode,
        limits: Limits,
    ) -> Result<Self> {
        let source = Source::from_bytes(bytes);
        Self::open_source(source, mode, limits)
    }

    /// Opens a document from a path in [`ParseMode::Lenient`]. `mmap` selects the
    /// backing strategy; the hard-safe [`MmapMode::Never`] is recommended for
    /// untrusted inputs (PRD §9.6.1).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] on read failure, plus the [`DocumentStore::from_bytes`]
    /// errors.
    pub fn open(path: impl AsRef<Path>, mmap: MmapMode, limits: Limits) -> Result<Self> {
        Self::open_with(path, mmap, ParseMode::Lenient, limits)
    }

    /// Opens a document from a path with an explicit [`ParseMode`] (PRD §8.2).
    ///
    /// # Errors
    ///
    /// See [`DocumentStore::open`] and [`DocumentStore::from_bytes_with`].
    pub fn open_with(
        path: impl AsRef<Path>,
        mmap: MmapMode,
        mode: ParseMode,
        limits: Limits,
    ) -> Result<Self> {
        let source = Source::open(path, mmap)?;
        Self::open_source(source, mode, limits)
    }

    /// Shared open path over an already-built [`Source`] — the central decision
    /// flow (PRD §8.2): **normal parse → validation gate → repair-on-fail**.
    ///
    /// 1. Parse the header (records `header_offset`).
    /// 2. **Strict**: parse the xref chain; any failure is the returned error.
    ///    Then run the validation gate; a failure there is an error too.
    /// 3. **Lenient**: try the normal xref parse. If it fails *or* the resulting
    ///    document fails the validation gate (`/Root`→catalog→`/Pages`
    ///    unreachable), fall back to a full-file object scan and retry the gate.
    fn open_source(source: Source, mode: ParseMode, limits: Limits) -> Result<Self> {
        if source.is_empty() {
            return Err(Error::source("empty document"));
        }
        let (version, header_offset) = parse_header(&source)?;
        let mut diagnostics = Diagnostics::new();
        if header_offset != 0 {
            diagnostics.warn(
                0,
                WarningKind::HeaderOffset,
                format!("{header_offset} junk byte(s) before %PDF- header"),
            );
        }

        // --- Strict: no repair; first violation is the error -----------------
        if mode == ParseMode::Strict {
            let xref = parse_xref_chain(&source, header_offset, &limits)?;
            let trailer = xref.trailer().clone();
            let mut store = Self::assemble(
                source,
                xref,
                trailer,
                version,
                header_offset,
                header_offset != 0,
                mode,
                diagnostics,
                limits,
            );
            store.validate_gate()?;
            return Ok(store);
        }

        // --- Lenient: normal parse → gate → repair-on-fail (best-effort) -----
        //
        // The gate never *fails* the open in Lenient mode; it only decides
        // whether to attempt a reconstruction. If neither the normal parse nor
        // the repair yields a gate-valid document, the best available parse is
        // still returned so low-level object access remains possible (PRD §8
        // intro: tolerate, don't reject).
        let normal = parse_xref_chain(&source, header_offset, &limits);
        let mut normal_store = match normal {
            Ok(xref) => {
                let trailer = xref.trailer().clone();
                let mut store = Self::assemble(
                    source.clone(),
                    xref,
                    trailer,
                    version,
                    header_offset,
                    header_offset != 0,
                    mode,
                    diagnostics.clone(),
                    limits,
                );
                if store.validate_gate().is_ok() {
                    return Ok(store);
                }
                // Clean parse exists but is not gate-valid: try repair, keep this
                // as the fallback.
                diagnostics.warn(
                    0,
                    WarningKind::ValidationFailed,
                    "clean parse failed the catalog/page-tree validation gate",
                );
                Some(store)
            }
            Err(e) => {
                diagnostics.warn(
                    0,
                    WarningKind::XrefUnreadable,
                    format!("cross-reference unreadable ({}); reconstructing", e.kind()),
                );
                None
            }
        };

        // Attempt a full-file object scan (PRD §8.2 repair subsystem).
        match repair::reconstruct(&source, header_offset, &limits, &mut diagnostics) {
            Ok(result) => {
                let trailer = result.xref.trailer().clone();
                let mut repaired = Self::assemble(
                    source,
                    result.xref,
                    trailer,
                    version,
                    header_offset,
                    true, // repair path always taints the parse (PRD §8.2)
                    mode,
                    diagnostics,
                    limits,
                );
                if repaired.validate_gate().is_ok() {
                    repaired.apply_catalog_version();
                    return Ok(repaired);
                }
                // Repair ran but produced no gate-valid catalog. Prefer the
                // normal parse if we have one (it is closer to the file as
                // written); otherwise return the best-effort reconstruction so
                // the recovered objects are still reachable.
                if let Some(mut store) = normal_store.take() {
                    store.diagnostics = repaired.diagnostics.clone();
                    Ok(store)
                } else {
                    Ok(repaired)
                }
            }
            Err(e) => {
                // Reconstruction found nothing usable. Fall back to the normal
                // parse if it succeeded at all; else propagate the failure.
                if let Some(mut store) = normal_store.take() {
                    store.diagnostics = diagnostics;
                    Ok(store)
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Assembles the store struct and applies the catalog `/Version` override.
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        source: Source,
        xref: XrefTable,
        trailer: Dict,
        version: Version,
        header_offset: usize,
        parse_was_repaired: bool,
        mode: ParseMode,
        diagnostics: Diagnostics,
        limits: Limits,
    ) -> Self {
        // Build the security handler from `/Encrypt` (if present) before the
        // store is finalized, so subsequent resolves see it (PRD §8.4).
        #[cfg(feature = "encryption")]
        let (decryptor, encrypt_obj_num) =
            Self::build_decryptor(&source, &trailer, &xref, &limits, mode);

        let mut store = DocumentStore {
            source,
            xref,
            trailer,
            version,
            header_offset,
            parse_was_repaired,
            redaction_applied: std::sync::atomic::AtomicBool::new(false),
            mode,
            diagnostics,
            limits,
            interner: RwLock::new(NameInterner::new()),
            arena: RwLock::new(HashMap::new()),
            changes: RwLock::new(ChangeSet::new()),
            trailer_overrides: RwLock::new(Dict::new()),
            #[cfg(feature = "encryption")]
            decryptor: RwLock::new(decryptor),
            #[cfg(feature = "encryption")]
            encrypt_obj_num,
        };
        // The catalog `/Version`, if present, overrides the header (PRD §8.2).
        store.apply_catalog_version();
        store
    }

    /// Builds the Standard Security Handler from the trailer `/Encrypt` entry
    /// (PRD §8.4). Returns `(Some(decryptor), Some(encrypt_obj_num))` for an
    /// encrypted document, `(None, None)` otherwise. A malformed `/Encrypt` is
    /// non-fatal here — the document still opens (lenient) and a later resolve
    /// surfaces the typed error; in Strict mode resolution will reflect it too.
    ///
    /// The `/Encrypt` dict may be direct or an indirect reference. We load it
    /// from the xref/source directly (the store is not yet finalized).
    #[cfg(feature = "encryption")]
    fn build_decryptor(
        source: &Source,
        trailer: &Dict,
        xref: &XrefTable,
        limits: &Limits,
        _mode: ParseMode,
    ) -> (Option<pdf_crypto::Decryptor>, Option<u32>) {
        let Some(enc_obj) = trailer.get(&Name::new("Encrypt")) else {
            return (None, None);
        };
        // Resolve a possible indirect reference to the encrypt dict.
        let (enc_dict, enc_num) = match enc_obj {
            Object::Reference(r) => match Self::load_encrypt_object(source, xref, limits, r.num) {
                Some(Object::Dictionary(d)) => (d, Some(r.num)),
                Some(Object::Stream(s)) => (s.dict, Some(r.num)),
                _ => return (None, None),
            },
            Object::Dictionary(d) => (d.clone(), None),
            _ => return (None, None),
        };

        let id0 = crate::encrypt::id0_from_trailer(trailer);
        match crate::encrypt::parse_encrypt_dict(&enc_dict, id0) {
            Ok(config) => match pdf_crypto::Decryptor::new(config) {
                Ok(d) => (Some(d), enc_num),
                Err(_) => (None, enc_num),
            },
            Err(_) => (None, enc_num),
        }
    }

    /// Loads the `/Encrypt` object by number, **without** the arena and without
    /// any decryption (the `/Encrypt` dict is exempt — PRD §8.4).
    #[cfg(feature = "encryption")]
    fn load_encrypt_object(
        source: &Source,
        xref: &XrefTable,
        _limits: &Limits,
        num: u32,
    ) -> Option<Object> {
        let XrefEntry::Uncompressed { offset, .. } = xref.get(num)? else {
            return None; // /Encrypt is never inside an ObjStm
        };
        let tail = source.slice_from(offset).ok()?;
        let mut parser = Parser::from_lexer(Lexer::new(tail));
        let (_r, obj) = parser.parse_indirect_object().ok()?;
        Some(obj)
    }

    /// The validation gate (PRD §8.2 step 7): `trailer.Root` must resolve to a
    /// dict with `/Type /Catalog`, and the catalog's `/Pages` must resolve to a
    /// page-tree node (a dict). Returns `Ok(())` when the document is usable.
    ///
    /// This populates the arena for the catalog / pages objects, which is the one
    /// sanctioned exception to "no eager load": it is open-time validation, and
    /// the catalog/pages are needed immediately anyway (PRD §8.2).
    ///
    /// # Errors
    ///
    /// [`Error::Xref`] when the catalog or page-tree root is missing /
    /// unresolvable / the wrong type.
    fn validate_gate(&mut self) -> Result<()> {
        let root = self
            .root()
            .ok_or_else(|| Error::xref(0, "trailer has no /Root"))?;
        let catalog = self
            .resolve(root)
            .map_err(|_| Error::xref(0, "/Root does not resolve"))?;
        let cat_dict = catalog
            .as_dict()
            .ok_or_else(|| Error::xref(0, "/Root is not a dictionary"))?;
        match cat_dict.get(&Name::new("Type")) {
            Some(Object::Name(n)) if n.as_bytes() == b"Catalog" => {}
            _ => return Err(Error::xref(0, "/Root is not /Type /Catalog")),
        }
        // `/Pages` must resolve to a page-tree node (a dict). Absent `/Pages` is
        // tolerated only if there genuinely is no page tree — but a catalog with
        // no `/Pages` is degenerate; require it to be present and a dict.
        let pages = self
            .resolve_dict_key(cat_dict, &Name::new("Pages"))
            .map_err(|_| Error::xref(0, "/Pages does not resolve"))?
            .ok_or_else(|| Error::xref(0, "catalog has no /Pages"))?;
        if pages.as_dict().is_none() {
            return Err(Error::xref(0, "/Pages is not a page-tree node"));
        }
        // The gate touched the arena; clear it so the "no eager load" invariant
        // holds for callers that assert an empty arena right after open.
        if let Ok(mut a) = self.arena.write() {
            a.clear();
        }
        Ok(())
    }

    /// If the catalog declares `/Version /1.x`, adopt it over the header value.
    ///
    /// Reads the catalog **without** populating the arena: this is internal
    /// open-time bookkeeping and must not violate the "no eager load" contract
    /// (PRD §9.2). Failures here are non-fatal — the header version stands.
    fn apply_catalog_version(&mut self) {
        let Some(root) = self.root() else { return };
        // Load (not cache) the catalog, following at most one level of reference.
        let Ok(catalog) = self.load_object(root.num) else {
            return;
        };
        let catalog = match catalog {
            Object::Reference(r) => match self.load_object(r.num) {
                Ok(o) => o,
                Err(_) => return,
            },
            other => other,
        };
        let Some(dict) = catalog.as_dict() else {
            return;
        };
        if let Some(Object::Name(name)) = dict.get(&Name::new("Version")) {
            if let Some(v) = parse_version_name(name.as_bytes()) {
                self.version = v;
            }
        }
    }

    // --- accessors --------------------------------------------------------

    /// The merged trailer dictionary.
    #[must_use]
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// The unified cross-reference table.
    #[must_use]
    pub fn xref(&self) -> &XrefTable {
        &self.xref
    }

    /// The backing source bytes.
    #[must_use]
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The effective PDF version (header, possibly overridden by catalog).
    #[must_use]
    pub fn version(&self) -> Version {
        self.version
    }

    /// The header byte bias (nonzero when junk precedes `%PDF-`, PRD §8.2).
    #[must_use]
    pub fn header_offset(&self) -> usize {
        self.header_offset
    }

    /// Whether the file is linearized ("fast web view"; PyMuPDF
    /// `Document.is_fast_webaccess`). A linearized PDF carries its linearization
    /// parameter dictionary as the **first** indirect object right after the
    /// header, identified by a `/Linearized` key. We scan the leading window of
    /// the source for that marker — cheap, and exact across the corpus (a
    /// `/Linearized` in the header region ⟺ MuPDF's verdict). A repaired/rewritten
    /// file no longer has a valid leading linearization dict, so this is `false`,
    /// matching fitz.
    #[must_use]
    pub fn is_linearized(&self) -> bool {
        // The linearization dict lives at the very start; 4 KiB after the header
        // comfortably covers it without scanning the whole file.
        const WINDOW: usize = 4096;
        let bytes = self.source.bytes();
        let start = self.header_offset.min(bytes.len());
        let end = start.saturating_add(WINDOW).min(bytes.len());
        bytes[start..end]
            .windows(b"/Linearized".len())
            .any(|w| w == b"/Linearized")
    }

    /// Whether the parse was repair-tainted (PRD §8.2): set when a full-file
    /// scan ran, the header had a nonzero bias, or any xref/`/Length` was
    /// corrected. The precondition gate for incremental save (PRD §8.7).
    #[must_use]
    pub fn parse_was_repaired(&self) -> bool {
        self.parse_was_repaired
    }

    /// The [`ParseMode`] this document was opened in (PRD §8.2).
    #[must_use]
    pub fn parse_mode(&self) -> ParseMode {
        self.mode
    }

    /// Whether destructive redaction has been applied to this document (PRD
    /// §8.8). A redacted doc must be **fully rewritten** — an incremental save
    /// would append the redacted objects after the original (pre-redaction)
    /// bytes, leaking the secret in the prior revision — so incremental save is
    /// rejected (or auto-upgraded to full) once this is set.
    #[must_use]
    pub fn redaction_applied(&self) -> bool {
        self.redaction_applied
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Marks the document as redacted (PRD §8.8): tainting it so a subsequent
    /// incremental save is rejected/auto-upgraded. Idempotent; uses interior
    /// mutability so the `&DocumentStore` redaction path can set it.
    pub fn mark_redaction_applied(&self) {
        self.redaction_applied
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The reconstruction actions taken during open — empty after a clean parse
    /// (PRD §8.2: queryable `repair_report`).
    #[must_use]
    pub fn repair_report(&self) -> &[RepairAction] {
        self.diagnostics.actions()
    }

    /// The non-fatal warnings collected during a Lenient open (PRD §8.2:
    /// `Warning { offset, kind, detail }`). Empty in Strict / on a clean parse.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        self.diagnostics.warnings()
    }

    /// The configured resource ceilings.
    #[must_use]
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// The catalog reference from the trailer `/Root`, if present.
    #[must_use]
    pub fn root(&self) -> Option<ObjRef> {
        match self.trailer.get(&Name::new("Root")) {
            Some(Object::Reference(r)) => Some(*r),
            _ => None,
        }
    }

    /// The number of objects currently materialized in the arena (for tests
    /// asserting laziness / cache behavior).
    #[must_use]
    pub fn cached_object_count(&self) -> usize {
        self.arena.read().map(|a| a.len()).unwrap_or(0)
    }

    // --- low-level xref read API (PRD §7 P1; pikepdf-style users) ----------

    /// The cross-reference length: one past the largest recorded object number
    /// (PyMuPDF `xref_length()` == `/Size`). Object numbers `1..xref_length()`
    /// are the addressable range; index 0 is the free-list head.
    #[must_use]
    pub fn xref_length(&self) -> u32 {
        // Prefer the trailer `/Size` (authoritative), but never trust it blindly:
        // take the max of `/Size` and (highest recorded object number + 1) so a
        // too-small `/Size` still exposes every object (repaired files lie).
        let by_size = self
            .trailer
            .get(&Name::new("Size"))
            .and_then(Object::as_i64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        let by_max = self
            .xref
            .object_numbers()
            .last()
            .map(|n| n.saturating_add(1))
            .unwrap_or(0);
        // Also reflect any objects created via the change-set overlay (e.g. a
        // `get_new_xref`/`add_object` slot not yet present in the original table),
        // so the addressable range — and PyMuPDF `/Size` — grows immediately.
        let by_changes = self.changes.read().map(|c| c.high_water()).unwrap_or(0);
        by_size.max(by_max).max(by_changes)
    }

    /// The **serialized** source of object `num` — its resolved value rendered
    /// back to canonical PDF syntax (PyMuPDF `xref_object(num)`). For a stream the
    /// dictionary is serialized (the body is fetched with [`Self::xref_stream`]).
    /// A free / absent object yields `"null"`.
    ///
    /// # Errors
    ///
    /// Parse / decode errors from resolving the object propagate.
    pub fn xref_object(&self, num: u32) -> Result<String> {
        match self.get_object(num, 0) {
            Ok(obj) => {
                let bytes = crate::serialize::write_object(obj.as_ref());
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            }
            Err(Error::MissingObject { .. }) => Ok("null".to_string()),
            Err(e) => Err(e),
        }
    }

    /// The value of dictionary key `key` on object `num`, serialized to PDF
    /// syntax (PyMuPDF `xref_get_key`). Returns `None` if the object is not a
    /// dictionary/stream or the key is absent.
    ///
    /// # Errors
    ///
    /// Resolution errors propagate.
    pub fn xref_get_key(&self, num: u32, key: &str) -> Result<Option<String>> {
        let obj = match self.get_object(num, 0) {
            Ok(o) => o,
            Err(Error::MissingObject { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        let Some(dict) = obj.as_dict() else {
            return Ok(None);
        };
        Ok(dict
            .get(&Name::new(key))
            .map(|v| String::from_utf8_lossy(&crate::serialize::write_object(v)).into_owned()))
    }

    /// Whether object `num` is a stream (PyMuPDF `xref_is_stream`).
    ///
    /// # Errors
    ///
    /// Resolution errors propagate; a missing object is `Ok(false)`.
    pub fn xref_is_stream(&self, num: u32) -> Result<bool> {
        match self.get_object(num, 0) {
            Ok(obj) => Ok(obj.as_stream().is_some()),
            Err(Error::MissingObject { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The **decoded** stream body of object `num` (PyMuPDF `xref_stream`): the
    /// full `/Filter` chain applied (and decrypted, if authenticated).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `num` is not a stream; decode errors propagate.
    pub fn xref_stream(&self, num: u32) -> Result<Vec<u8>> {
        let obj = self.get_object(num, 0)?;
        let stream = obj
            .as_stream()
            .ok_or(Error::Unsupported("object is not a stream"))?;
        self.decode_stream(stream)?.into_decoded()
    }

    /// The **raw** (still filter-encoded) stream body of object `num` (PyMuPDF
    /// `xref_stream_raw`).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `num` is not a stream; source-range errors
    /// propagate.
    pub fn xref_stream_raw(&self, num: u32) -> Result<Vec<u8>> {
        let obj = self.get_object(num, 0)?;
        let stream = obj
            .as_stream()
            .ok_or(Error::Unsupported("object is not a stream"))?;
        Ok(self.stream_raw_bytes(stream)?.to_vec())
    }

    /// Whether object `num` is a font dictionary (PyMuPDF `xref_is_font`):
    /// `/Type /Font`. A missing / non-dict object is `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Resolution errors propagate.
    pub fn xref_is_font(&self, num: u32) -> Result<bool> {
        Ok(self.xref_type_is(num, "Font"))
    }

    /// Whether object `num` is an image XObject (PyMuPDF `xref_is_image`):
    /// a stream with `/Type /XObject` and `/Subtype /Image`. A missing object is
    /// `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Resolution errors propagate.
    pub fn xref_is_image(&self, num: u32) -> Result<bool> {
        let obj = match self.get_object(num, 0) {
            Ok(o) => o,
            Err(Error::MissingObject { .. }) => return Ok(false),
            Err(e) => return Err(e),
        };
        let Some(stream) = obj.as_stream() else {
            return Ok(false);
        };
        let is_xobject = stream
            .dict
            .get(&Name::new("Subtype"))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes() == b"Image")
            .unwrap_or(false);
        Ok(is_xobject)
    }

    /// Whether object `num` is a **Form** XObject (PyMuPDF `xref_is_xobject`):
    /// a dict/stream whose `/Subtype` is `/Form` (matching fitz, which keys only
    /// on `/Subtype /Form` — image XObjects are reported by `xref_is_image`). A
    /// missing object is `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Resolution errors propagate.
    pub fn xref_is_xobject(&self, num: u32) -> Result<bool> {
        let obj = match self.get_object(num, 0) {
            Ok(o) => o,
            Err(Error::MissingObject { .. }) => return Ok(false),
            Err(e) => return Err(e),
        };
        let dict = match obj.as_ref() {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        Ok(dict
            .and_then(|d| d.get(&Name::new("Subtype")))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes() == b"Form")
            .unwrap_or(false))
    }

    /// The dictionary keys of object `num` (names, no leading slash), or an empty
    /// vector for a non-dict / missing object (PyMuPDF `xref_get_keys`). Keys are
    /// returned in the backing dictionary's order (sorted, an oxide model trait).
    ///
    /// # Errors
    ///
    /// Resolution errors propagate.
    pub fn xref_get_keys(&self, num: u32) -> Result<Vec<String>> {
        let obj = match self.get_object(num, 0) {
            Ok(o) => o,
            Err(Error::MissingObject { .. }) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let dict = match obj.as_ref() {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        Ok(dict
            .map(|d| {
                d.keys()
                    .map(|k| String::from_utf8_lossy(k.as_bytes()).into_owned())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Whether object `num` resolves to a dictionary (or stream dict) whose
    /// `/Type` name equals `ty`. Missing / wrong-typed → `false`.
    fn xref_type_is(&self, num: u32, ty: &str) -> bool {
        let Ok(obj) = self.get_object(num, 0) else {
            return false;
        };
        let dict = match obj.as_ref() {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        dict.and_then(|d| d.get(&Name::new("Type")))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes() == ty.as_bytes())
            .unwrap_or(false)
    }

    /// Sets dictionary key `key` of object `num` to the PDF value parsed from
    /// `value` (PyMuPDF `xref_set_key`). `value` is a single PDF object in
    /// surface syntax: a name (`/DeviceRGB`), number, string (`(text)`), boolean,
    /// array (`[1 2 3]`), dictionary (`<< … >>`) or indirect reference (`3 0 R`).
    /// A `value` of `"null"` removes the key.
    ///
    /// Works on dictionaries and stream dictionaries; the stream body is
    /// preserved.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `num` is not a dictionary/stream or `value` is
    /// not parseable; object-edit errors propagate.
    pub fn xref_set_key(&self, num: u32, key: &str, value: &str) -> Result<()> {
        let parsed = crate::object::parse::Parser::new(value.as_bytes())
            .parse_object()
            .map_err(|_| Error::Unsupported("xref_set_key: value is not a parseable PDF object"))?;
        let r = ObjRef::new(num, 0);
        let obj = self.get_object(num, 0)?;
        let name = Name::new(key);
        match obj.as_ref() {
            Object::Dictionary(d) => {
                let mut d = d.clone();
                if matches!(parsed, Object::Null) {
                    d.remove(&name);
                } else {
                    d.insert(name, parsed);
                }
                self.update_object(r, Object::Dictionary(d))
            }
            Object::Stream(s) => {
                let mut s = s.clone();
                if matches!(parsed, Object::Null) {
                    s.dict.remove(&name);
                } else {
                    s.dict.insert(name, parsed);
                }
                self.update_object(r, Object::Stream(s))
            }
            _ => Err(Error::Unsupported(
                "xref_set_key: object is not a dictionary or stream",
            )),
        }
    }

    /// Copies the value of object `source` into object `target` (PyMuPDF
    /// `xref_copy`): `target` becomes a deep clone of `source` (dictionary,
    /// stream with body, or any value). Returns an error if `source` is missing.
    ///
    /// # Errors
    ///
    /// Resolution / object-edit errors propagate.
    pub fn xref_copy(&self, source: u32, target: u32) -> Result<()> {
        let obj = self.get_object(source, 0)?;
        // For a source stream, copy the **raw** (still filter-encoded) body
        // verbatim along with its dictionary — this keeps the copy self-contained
        // (no lazy slice into the original file buffer) and works for any filter,
        // including image filters (`DCTDecode`, `JPXDecode`) the core does not
        // re-encode.
        match obj.as_ref() {
            Object::Stream(s) => {
                let bytes = self.stream_raw_bytes(s)?.to_vec();
                let mut dict = s.dict.clone();
                dict.insert(Name::new("Length"), Object::Integer(bytes.len() as i64));
                self.update_stream(ObjRef::new(target, 0), dict, bytes, true)
            }
            other => self.update_object(ObjRef::new(target, 0), other.clone()),
        }
    }

    // --- encryption (PRD §8.4) -------------------------------------------

    /// Whether the document is encrypted (has a usable `/Encrypt` handler).
    #[cfg(feature = "encryption")]
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.decryptor.read().map(|d| d.is_some()).unwrap_or(false)
    }

    /// Whether a password is still required: the document is encrypted and not
    /// yet authenticated (PRD §8.4). `false` for an unencrypted document.
    #[cfg(feature = "encryption")]
    #[must_use]
    pub fn needs_pass(&self) -> bool {
        self.decryptor
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(pdf_crypto::Decryptor::needs_pass))
            .unwrap_or(false)
    }

    /// The advisory permission flags (`/P`) for an encrypted document, if any
    /// (PRD §8.4 — exposed, never enforced for extraction).
    #[cfg(feature = "encryption")]
    #[must_use]
    pub fn permissions(&self) -> Option<i32> {
        self.decryptor
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(pdf_crypto::Decryptor::permissions))
    }

    /// Authenticates `password` against the security handler, trying the user
    /// role then the owner role (PRD §8.4). On success, subsequent `resolve()` /
    /// `decode_stream()` calls decrypt transparently. Pass `b""` for the common
    /// empty-user-password case.
    ///
    /// Clears the object arena on success so any objects cached before
    /// authentication (none are decrypted yet) are re-read and decrypted.
    ///
    /// # Errors
    ///
    /// [`Error::Crypto`] (wrapping `NeedsPassword`) when the password matches no
    /// role; [`Error::Unsupported`] if the document is not encrypted.
    #[cfg(feature = "encryption")]
    pub fn authenticate(&self, password: &[u8]) -> Result<pdf_crypto::AuthRole> {
        let mut guard = self
            .decryptor
            .write()
            .map_err(|_| Error::Unsupported("decryptor lock poisoned"))?;
        let dec = guard
            .as_mut()
            .ok_or(Error::Unsupported("document is not encrypted"))?;
        let role = dec.authenticate(password)?;
        drop(guard);
        // Drop any cached objects parsed before authentication so they re-load
        // through the decrypting path.
        if let Ok(mut a) = self.arena.write() {
            a.clear();
        }
        Ok(role)
    }

    /// Interns `name` against the document-wide pool (PRD §9.2).
    pub fn intern_name(&self, name: &Name) -> Name {
        match self.interner.write() {
            Ok(mut i) => i.intern(name),
            Err(_) => name.clone(),
        }
    }

    // --- resolution -------------------------------------------------------

    /// Returns the **raw** object `num gen` (a `Reference` is *not* followed),
    /// parsing+caching it on first access. The low-level read API surface (PRD
    /// §7 / §9.2).
    ///
    /// `gen` is currently advisory (matched against the xref entry); a mismatch
    /// is tolerated (real files lie) — the object at the recorded slot is used.
    ///
    /// # Errors
    ///
    /// [`Error::MissingObject`] when there is no usable entry; [`Error::Xref`] /
    /// parse / decode errors propagate.
    pub fn get_object(&self, num: u32, _gen: u16) -> Result<Arc<Object>> {
        // The pending-edit overlay takes precedence over the arena / original
        // (PRD §8.7: a `resolve` after `update_object` returns the new value).
        if let Ok(changes) = self.changes.read() {
            match changes.get(num) {
                Some(Change::Set(obj)) => return Ok(Arc::clone(obj)),
                // A deleted object reads back as Null (its slot is freed on save).
                Some(Change::Deleted) => return Ok(Arc::new(Object::Null)),
                None => {}
            }
        }
        if let Some(cached) = self.arena.read().ok().and_then(|a| a.get(&num).cloned()) {
            return Ok(cached);
        }
        let obj = self.load_object(num)?;
        let arc = Arc::new(obj);
        if let Ok(mut a) = self.arena.write() {
            // Another thread may have filled it; keep the first (identical) Arc.
            let entry = a.entry(num).or_insert_with(|| Arc::clone(&arc));
            return Ok(Arc::clone(entry));
        }
        Ok(arc)
    }

    /// Resolves `r` to its final **non-reference** object, following
    /// reference→reference chains transparently with cycle detection and a
    /// depth limit (PRD §8.1 / §9.3 / §9.6).
    ///
    /// # Errors
    ///
    /// [`Error::ReferenceCycle`] on a cyclic chain,
    /// [`Error::LimitExceeded`]`(RecursionDepth)` past the depth limit,
    /// [`Error::MissingObject`] for a dangling reference.
    pub fn resolve(&self, r: ObjRef) -> Result<Arc<Object>> {
        let mut seen = HashSet::new();
        let mut current = r;
        let mut depth = 0u32;
        loop {
            depth += 1;
            if depth > self.limits.max_recursion_depth {
                return Err(Error::LimitExceeded(LimitKind::RecursionDepth));
            }
            if !seen.insert(current.num) {
                return Err(Error::ReferenceCycle {
                    num: current.num,
                    gen: current.gen,
                });
            }
            let obj = match self.get_object(current.num, current.gen) {
                Ok(o) => o,
                // Dangling reference (no usable xref entry): in Lenient mode a
                // reference to a non-existent object yields Null, not an error
                // (PRD §8.1 / §8.2). In Strict mode it is the typed error.
                Err(Error::MissingObject { .. }) if self.mode.is_lenient() => {
                    return Ok(Arc::new(Object::Null));
                }
                Err(e) => return Err(e),
            };
            match obj.as_ref() {
                Object::Reference(next) => current = *next,
                _ => return Ok(obj),
            }
        }
    }

    /// Resolves a dictionary value that may be a reference: returns the
    /// referenced object resolved to a non-reference, or the value itself if it
    /// is already direct. Missing key → `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Propagates [`DocumentStore::resolve`] errors.
    pub fn resolve_dict_key(&self, dict: &Dict, key: &Name) -> Result<Option<Arc<Object>>> {
        match dict.get(key) {
            None => Ok(None),
            Some(Object::Reference(r)) => self.resolve(*r).map(Some),
            Some(direct) => Ok(Some(Arc::new(direct.clone()))),
        }
    }

    // --- object-edit API (PRD §8.7 / §9.2) --------------------------------

    /// Whether any edit is pending (PRD §9.2: `is_dirty`). `false` immediately
    /// after open.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.changes.read().map(|c| c.is_dirty()).unwrap_or(false)
    }

    /// Creates a new indirect object, allocating a fresh object number past the
    /// current maximum, and returns its reference (PRD §8.7 `add_object`). A
    /// subsequent `resolve` of the returned reference yields the new value.
    pub fn add_object(&self, obj: Object) -> Result<ObjRef> {
        let size = self.xref_length();
        let mut changes = self
            .changes
            .write()
            .map_err(|_| Error::Unsupported("change-set lock poisoned"))?;
        changes.seed(size);
        Ok(changes.allocate(obj))
    }

    /// Replaces the value of an existing (or newly created) object number
    /// (PRD §8.7 `update_object`). Reflected immediately by `resolve`/`get_object`
    /// and after `save`.
    pub fn update_object(&self, r: ObjRef, obj: Object) -> Result<()> {
        let size = self.xref_length();
        let mut changes = self
            .changes
            .write()
            .map_err(|_| Error::Unsupported("change-set lock poisoned"))?;
        changes.seed(size);
        changes.set(r.num, obj);
        Ok(())
    }

    /// Replaces a stream object's dictionary and body (PRD §8.7 `update_stream`).
    ///
    /// `encoded = false` (the common case) treats `body` as **decoded** plain
    /// bytes that the writer will Flate-deflate on save when `deflate` is on (and
    /// `/Filter`/`/Length` are managed by the writer). `encoded = true` treats
    /// `body` as **already filter-encoded** bytes written verbatim; in that case
    /// `dict` must already name the matching `/Filter`.
    pub fn update_stream(
        &self,
        r: ObjRef,
        dict: Dict,
        body: impl Into<Vec<u8>>,
        encoded: bool,
    ) -> Result<()> {
        let size = self.xref_length();
        let mut changes = self
            .changes
            .write()
            .map_err(|_| Error::Unsupported("change-set lock poisoned"))?;
        changes.seed(size);
        changes.set_stream(r.num, dict, body.into(), encoded);
        Ok(())
    }

    /// Deletes (frees) an object (PRD §8.7 `delete_object`). A subsequent
    /// `resolve` yields `Null`; a full save omits the object (its slot is free).
    pub fn delete_object(&self, r: ObjRef) -> Result<()> {
        let mut changes = self
            .changes
            .write()
            .map_err(|_| Error::Unsupported("change-set lock poisoned"))?;
        changes.delete(r.num);
        Ok(())
    }

    /// Overrides a trailer key for the next save (PRD §8.7/§8.9). Used to wire a
    /// newly created `/Info` or `/Encrypt` reference into the trailer when the
    /// original document had none (the writer otherwise only carries pre-existing
    /// indirect trailer refs). Pass `Object::Null` to remove a trailer key.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if the internal lock is poisoned.
    pub fn set_trailer_key(&self, key: &str, value: Object) -> Result<()> {
        let mut over = self
            .trailer_overrides
            .write()
            .map_err(|_| Error::Unsupported("trailer-override lock poisoned"))?;
        over.insert(Name::new(key), value);
        Ok(())
    }

    /// The effective trailer reference for `key`, honoring any
    /// [`set_trailer_key`](Self::set_trailer_key) override over the original
    /// trailer (PRD §8.7). Returns `None` if the key is absent, removed, or not
    /// an indirect reference.
    #[must_use]
    pub fn effective_trailer_ref(&self, key: &str) -> Option<ObjRef> {
        let name = Name::new(key);
        if let Ok(over) = self.trailer_overrides.read() {
            match over.get(&name) {
                Some(Object::Reference(r)) => return Some(*r),
                Some(Object::Null) => return None,
                Some(_) => return None,
                None => {}
            }
        }
        match self.trailer.get(&name) {
            Some(Object::Reference(r)) => Some(*r),
            _ => None,
        }
    }

    /// A snapshot of the pending changes (object number → [`Change`]) in
    /// object-number order — the basis for M3b incremental save (PRD §9.2). The
    /// writer uses this to enumerate newly created objects.
    #[must_use]
    pub fn changes_snapshot(&self) -> Vec<(u32, Change)> {
        self.changes
            .read()
            .map(|c| {
                c.changes()
                    .iter()
                    .map(|(&num, change)| (num, change.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The pending change for object `num`, if any (writer overlay lookup).
    #[must_use]
    pub(crate) fn change_get(&self, num: u32) -> Option<Change> {
        self.changes.read().ok().and_then(|c| c.get(num).cloned())
    }

    /// A deep clone of the pending-edit overlay for journal snapshotting
    /// (PyMuPDF journalling).
    #[must_use]
    pub fn snapshot_changeset(&self) -> ChangeSet {
        self.changes.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Restores a previously snapshotted overlay (journal undo/redo).
    pub fn restore_changeset(&self, snap: ChangeSet) {
        if let Ok(mut c) = self.changes.write() {
            *c = snap;
        }
    }

    // --- full save (PRD §8.7) ---------------------------------------------

    /// Serializes the whole effective document (original live objects overlaid by
    /// the change set) to a fresh, valid PDF byte stream per `opts` (PRD §8.7
    /// "Full save"). PyMuPDF `tobytes` / `write` map onto this.
    ///
    /// # Errors
    ///
    /// [`Error::Xref`] if the document has no `/Root`; resolution/decode errors
    /// propagate for objects the writer must materialize.
    pub fn save_to_vec(&self, opts: &crate::writer::SaveOptions) -> Result<Vec<u8>> {
        crate::writer::save_to_vec(self, opts)
    }

    /// Convenience alias for [`DocumentStore::save_to_vec`] (PyMuPDF `tobytes`).
    ///
    /// # Errors
    ///
    /// See [`DocumentStore::save_to_vec`].
    pub fn tobytes(&self, opts: &crate::writer::SaveOptions) -> Result<Vec<u8>> {
        self.save_to_vec(opts)
    }

    /// Saves the document to `path` (PRD §8.7 `save`). Full save only (M3a).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] on a write failure, plus [`DocumentStore::save_to_vec`]
    /// errors.
    pub fn save(&self, path: impl AsRef<Path>, opts: &crate::writer::SaveOptions) -> Result<()> {
        let bytes = self.save_to_vec(opts)?;
        std::fs::write(path, bytes).map_err(Error::from)
    }

    /// Whether an incremental save is permitted: only on a **clean** parse
    /// (PRD §8.7). A repair-tainted parse has no trustworthy original byte
    /// offsets, so append-only updates would corrupt the `/Prev` chain and
    /// invalidate signatures — incremental save is rejected (PyMuPDF
    /// `can_save_incrementally`).
    #[must_use]
    pub fn can_save_incrementally(&self) -> bool {
        !self.parse_was_repaired && !self.redaction_applied()
    }

    /// Serializes an **incremental** update: appends only the changed objects and
    /// a new cross-reference section (whose `/Prev` chains to the prior
    /// `startxref`) to the original source bytes, guaranteeing
    /// `out[..orig.len()] == orig` (PRD §8.7). PyMuPDF `save(incremental=True)`.
    ///
    /// # Errors
    ///
    /// [`Error::IncrementalRequiresCleanParse`] when the parse was repair-tainted
    /// and `opts.on_repaired == OnRepaired::Reject` (the default); with
    /// `OnRepaired::Upgrade` a full save is returned instead. [`Error::Xref`] if
    /// the document has no `/Root`.
    pub fn save_incremental(&self, opts: &crate::writer::SaveOptions) -> Result<Vec<u8>> {
        crate::writer::save_incremental(self, opts)
    }

    /// Saves an incremental update to `path` (PRD §8.7).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] on a write failure, plus [`DocumentStore::save_incremental`]
    /// errors.
    pub fn save_incremental_to(
        &self,
        path: impl AsRef<Path>,
        opts: &crate::writer::SaveOptions,
    ) -> Result<()> {
        let bytes = self.save_incremental(opts)?;
        std::fs::write(path, bytes).map_err(Error::from)
    }

    // --- stream bodies ----------------------------------------------------

    /// Materializes a stream's body bytes (slicing a [`StreamData::Raw`] payload
    /// from the [`Source`]; an owned payload is returned as-is).
    ///
    /// # Errors
    ///
    /// [`Error::Source`] if a `Raw` range is out of bounds (PRD §9.6.1).
    pub fn stream_raw_bytes(&self, stream: &StreamObj) -> Result<Bytes> {
        match &stream.data {
            StreamData::Raw { offset, len } => self.source.slice_bytes(*offset, *len),
            StreamData::Encoded(b) | StreamData::Decoded(b) => Ok(b.clone()),
        }
    }

    /// Decodes a (possibly source-backed) stream's full `/Filter` chain, slicing
    /// a `Raw` body from the [`Source`] first (PRD §8.3 / §9.2). Ties the M1b
    /// codec layer to the source-backed `Raw` variant.
    ///
    /// # Errors
    ///
    /// [`Error::Source`] on a bad `Raw` range; decode errors propagate.
    pub fn decode_stream(&self, stream: &StreamObj) -> Result<crate::filters::DecodeOutcome> {
        let raw = self.stream_raw_bytes(stream)?;
        match &stream.data {
            StreamData::Decoded(b) => Ok(crate::filters::DecodeOutcome::Decoded(b.to_vec())),
            _ => crate::filters::decode_stream(&stream.dict, &raw, &self.limits),
        }
    }

    // --- internal loading -------------------------------------------------

    /// Loads (parses) object `num` from its cross-reference entry, **without**
    /// consulting or filling the arena.
    fn load_object(&self, num: u32) -> Result<Object> {
        match self.xref.get(num) {
            Some(XrefEntry::Uncompressed { offset, .. }) => self.load_uncompressed(num, offset),
            Some(XrefEntry::Compressed { objstm_num, index }) => {
                self.load_compressed(num, objstm_num, index)
            }
            Some(XrefEntry::Free) | None => Err(Error::MissingObject { num, gen: 0 }),
        }
    }

    /// Parses an uncompressed object at absolute `offset`, converting a stream's
    /// body to a source-backed [`StreamData::Raw`] payload (lazy bytes, PRD §9.2).
    ///
    /// When the document is encrypted **and authenticated**, this also decrypts
    /// the object's strings and (for non-exempt streams) the stream body, once,
    /// at load time (PRD §8.4). An encrypted stream body is materialized to an
    /// owned, still-filter-encoded [`StreamData::Encoded`] payload so the rest of
    /// the decode pipeline is unchanged.
    fn load_uncompressed(&self, num: u32, offset: usize) -> Result<Object> {
        let tail = self.source.slice_from(offset)?;
        let mut parser = Parser::from_lexer(Lexer::new(tail));
        let (r, obj) = parser
            .parse_indirect_object()
            .map_err(|_| Error::xref(offset, "malformed object at xref offset"))?;
        // Object number sanity (lenient: tolerate a mismatch, real files lie).
        let _ = r.num == num;
        let gen = r.gen;

        // Convert an owned stream body into a source-backed `Raw` payload so the
        // bytes are sliced lazily and not duplicated in the arena (PRD §9.2).
        // `mut` is needed by the encryption decrypt-in-place pass below.
        #[cfg_attr(not(feature = "encryption"), allow(unused_mut))]
        let mut obj = if let Object::Stream(stream) = obj {
            if let Some((body_off, body_len)) = parser.last_stream_body() {
                let abs = offset.saturating_add(body_off);
                // Validate the range against the source up-front (never trust it).
                let _ = self.source.slice(abs, body_len)?;
                Object::Stream(StreamObj {
                    dict: stream.dict,
                    data: StreamData::Raw {
                        offset: abs,
                        len: body_len,
                    },
                })
            } else {
                Object::Stream(stream)
            }
        } else {
            obj
        };

        #[cfg(feature = "encryption")]
        self.decrypt_uncompressed(&mut obj, num, gen)?;
        #[cfg(not(feature = "encryption"))]
        let _ = gen;

        Ok(obj)
    }

    /// Applies the security handler to a freshly parsed uncompressed object
    /// (PRD §8.4): decrypts strings in place and, for non-exempt streams,
    /// decrypts the body (materializing it to an owned `Encoded` payload). A
    /// no-op when the document is unencrypted, not yet authenticated, or the
    /// object is exempt (the `/Encrypt` dict, an XRef stream, or `/Metadata`
    /// when `EncryptMetadata=false`).
    #[cfg(feature = "encryption")]
    fn decrypt_uncompressed(&self, obj: &mut Object, num: u32, gen: u16) -> Result<()> {
        // Snapshot the decryptor: if absent or not yet authenticated, do nothing.
        let guard = self
            .decryptor
            .read()
            .map_err(|_| Error::Unsupported("decryptor lock poisoned"))?;
        let Some(dec) = guard.as_ref() else {
            return Ok(());
        };
        if dec.needs_pass() {
            return Ok(());
        }
        // Exemption: the /Encrypt dict object itself is never decrypted.
        if crate::encrypt::is_encrypt_object(num, self.encrypt_obj_num) {
            return Ok(());
        }
        let encrypt_metadata = dec.config().encrypt_metadata;

        // Decrypt the stream body first (so the dict's /Length etc. are intact),
        // unless the stream is exempt.
        if let Object::Stream(stream) = obj {
            if !crate::encrypt::is_exempt_stream(&stream.dict, encrypt_metadata) {
                let raw = self.stream_raw_bytes(stream)?;
                let plain = dec.decrypt_stream(num, gen, &raw)?;
                stream.data = StreamData::Encoded(Bytes::from(plain));
            }
        }
        // Decrypt every string in the object graph (dict values, array members).
        crate::encrypt::decrypt_strings_in_place(obj, dec, num, gen);
        Ok(())
    }

    /// Parses object `num` from its containing object stream `objstm_num` at
    /// directory `index` (PRD §8.2). The container is itself resolved (and
    /// cached) recursively; objects inside an ObjStm are never streams.
    fn load_compressed(&self, num: u32, objstm_num: u32, index: u32) -> Result<Object> {
        // Guard against an ObjStm whose container is itself "compressed" (illegal,
        // and a potential infinite recursion): resolve the container as a stream.
        if objstm_num == num {
            return Err(Error::xref(0, "object stream contains itself"));
        }
        let container = self.get_object(objstm_num, 0)?;
        let stream = match container.as_ref() {
            Object::Stream(s) => s,
            _ => {
                return Err(Error::xref(
                    0,
                    "compressed object's container is not a stream",
                ))
            }
        };

        // Materialize the container body (slice Raw from source) and decode.
        let raw = self.stream_raw_bytes(stream)?;
        let materialized = StreamObj {
            dict: stream.dict.clone(),
            data: StreamData::Encoded(raw),
        };
        let objstm = crate::objstm::ObjStm::decode(&materialized, &self.limits)?;
        objstm.object_at(index as usize)
    }
}

// --- header parsing -------------------------------------------------------

/// Locates the `%PDF-m.n` header (scanning the first ~1 KiB for a junk bias) and
/// returns the version plus the byte offset bias (PRD §8.2).
fn parse_header(source: &Source) -> Result<(Version, usize)> {
    let buf = source.bytes();
    let window = &buf[..buf.len().min(1024)];
    let needle = b"%PDF-";
    let pos = window
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or(Error::Unsupported("missing %PDF- header"))?;

    let after = pos + needle.len();
    let version = parse_version_name(buf.get(after..after + 3).unwrap_or(&[]))
        .ok_or(Error::Unsupported("malformed %PDF- version"))?;
    Ok((version, pos))
}

/// Parses `b"1.7"` / `b"2.0"` (or a name like `1.5`) into a [`Version`].
fn parse_version_name(bytes: &[u8]) -> Option<Version> {
    // Accept exactly `M.N` where M and N are single ASCII digits.
    if bytes.len() < 3 {
        return None;
    }
    let major = bytes[0].checked_sub(b'0').filter(|&d| d <= 9)?;
    if bytes[1] != b'.' {
        return None;
    }
    let minor = bytes[2].checked_sub(b'0').filter(|&d| d <= 9)?;
    Some(Version { major, minor })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal one-page PDF skeleton sufficient for the change-set tests.
    fn minimal_pdf() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::new();

        offsets.push((1u32, out.len()));
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        offsets.push((2u32, out.len()));
        out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        offsets.push((3u32, out.len()));
        out.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
        );

        let startxref = out.len();
        out.extend_from_slice(b"xref\n0 4\n");
        out.extend_from_slice(b"0000000000 65535 f \n");
        let mut map = std::collections::HashMap::new();
        for (num, off) in &offsets {
            map.insert(*num, *off);
        }
        for num in 1..4u32 {
            out.extend_from_slice(format!("{:010} 00000 n \n", map[&num]).as_bytes());
        }
        out.extend_from_slice(b"trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n");
        out.extend_from_slice(format!("{startxref}\n").as_bytes());
        out.extend_from_slice(b"%%EOF\n");
        out
    }

    #[test]
    fn snapshot_restore_round_trips_the_overlay() {
        let doc = DocumentStore::from_bytes(minimal_pdf(), Limits::default()).unwrap();

        // Clean parse: no pending edits.
        assert!(!doc.is_dirty());
        let clean = doc.snapshot_changeset();

        // Mutate: create a new object and update an existing one.
        let new_ref = doc.add_object(Object::Integer(42)).unwrap();
        doc.update_object(ObjRef::new(3, 0), Object::Integer(7))
            .unwrap();
        assert!(doc.is_dirty());
        assert_eq!(doc.changes_snapshot().len(), 2);
        assert_eq!(*doc.resolve(new_ref).unwrap(), Object::Integer(42));

        // A snapshot taken now should preserve both edits when restored later.
        let dirty = doc.snapshot_changeset();

        // Restore the clean overlay: the mutations vanish.
        doc.restore_changeset(clean);
        assert!(!doc.is_dirty());
        assert_eq!(doc.changes_snapshot().len(), 0);
        // The new object is gone (a dangling ref resolves to Null in Lenient
        // mode); the page object reads back its original dict, not Integer(7).
        assert_eq!(*doc.resolve(new_ref).unwrap(), Object::Null);
        assert!(doc.resolve(ObjRef::new(3, 0)).unwrap().as_dict().is_some());

        // Restoring the dirty snapshot brings the mutations back.
        doc.restore_changeset(dirty);
        assert!(doc.is_dirty());
        assert_eq!(doc.changes_snapshot().len(), 2);
        assert_eq!(*doc.resolve(new_ref).unwrap(), Object::Integer(42));
    }
}
