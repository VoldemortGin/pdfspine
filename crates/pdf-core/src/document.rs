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

use crate::error::{Error, LimitKind, Result};
use crate::interner::NameInterner;
use crate::lexer::Lexer;
use crate::limits::Limits;
use crate::object::parse::Parser;
use crate::object::{Dict, Name, ObjRef, Object, StreamData, StreamObj};
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
    limits: Limits,
    interner: RwLock<NameInterner>,
    /// Lazy object arena: object number → cached resolved object (PRD §9.2).
    arena: RwLock<HashMap<u32, Arc<Object>>>,
}

impl DocumentStore {
    // --- opening ----------------------------------------------------------

    /// Opens a document from in-memory bytes with the given [`Limits`].
    ///
    /// Parses the header, locates and walks the cross-reference chain and reads
    /// the trailer; **does not** load object bodies (PRD §9.2).
    ///
    /// # Errors
    ///
    /// [`Error::Source`] if the buffer is empty / too small,
    /// [`Error::Unsupported`] for a missing header, or [`Error::Xref`] for an
    /// unrecoverable cross-reference (repair is M1d).
    pub fn from_bytes(bytes: impl Into<Bytes>, limits: Limits) -> Result<Self> {
        let source = Source::from_bytes(bytes);
        Self::open_source(source, limits)
    }

    /// Opens a document from a path. `mode` selects the backing strategy; the
    /// hard-safe [`MmapMode::Never`] is recommended for untrusted inputs (PRD
    /// §9.6.1).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] on read failure, plus the [`DocumentStore::from_bytes`]
    /// errors.
    pub fn open(path: impl AsRef<Path>, mode: MmapMode, limits: Limits) -> Result<Self> {
        let source = Source::open(path, mode)?;
        Self::open_source(source, limits)
    }

    /// Shared open path over an already-built [`Source`].
    fn open_source(source: Source, limits: Limits) -> Result<Self> {
        if source.is_empty() {
            return Err(Error::source("empty document"));
        }
        let (version, header_offset) = parse_header(&source)?;
        let xref = parse_xref_chain(&source, header_offset, &limits)?;
        let trailer = xref.trailer().clone();

        let mut store = DocumentStore {
            source,
            xref,
            trailer,
            version,
            header_offset,
            parse_was_repaired: header_offset != 0,
            limits,
            interner: RwLock::new(NameInterner::new()),
            arena: RwLock::new(HashMap::new()),
        };

        // The catalog `/Version`, if present, overrides the header (PRD §8.2).
        store.apply_catalog_version();
        Ok(store)
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

    /// Whether the parse was repair-tainted (PRD §8.2). M1c only sets this for a
    /// nonzero `header_offset`; full repair is M1d.
    #[must_use]
    pub fn parse_was_repaired(&self) -> bool {
        self.parse_was_repaired
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
            let obj = self.get_object(current.num, current.gen)?;
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
    fn load_uncompressed(&self, num: u32, offset: usize) -> Result<Object> {
        let tail = self.source.slice_from(offset)?;
        let mut parser = Parser::from_lexer(Lexer::new(tail));
        let (r, obj) = parser
            .parse_indirect_object()
            .map_err(|_| Error::xref(offset, "malformed object at xref offset"))?;
        // Object number sanity (lenient: tolerate a mismatch, real files lie).
        let _ = r.num == num;

        // Convert an owned stream body into a source-backed `Raw` payload so the
        // bytes are sliced lazily and not duplicated in the arena (PRD §9.2).
        if let Object::Stream(stream) = obj {
            if let Some((body_off, body_len)) = parser.last_stream_body() {
                let abs = offset.saturating_add(body_off);
                // Validate the range against the source up-front (never trust it).
                let _ = self.source.slice(abs, body_len)?;
                return Ok(Object::Stream(StreamObj {
                    dict: stream.dict,
                    data: StreamData::Raw {
                        offset: abs,
                        len: body_len,
                    },
                }));
            }
            return Ok(Object::Stream(stream));
        }
        Ok(obj)
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
