//! Malformed-PDF repair / reconstruction — the differentiator (PRD §8 intro,
//! §8.2 "Repair subsystem (P0)").
//!
//! Real-world PDFs are routinely broken; the design center is "architect around
//! tolerance, not the happy path" (PRD §8 intro). When the cross-reference
//! chain is missing, garbage, inconsistent, or the catalog / page-tree is
//! unreachable, this module performs a **single bounded pass** over the whole
//! [`Source`], recovering every indirect object's true byte offset by locating
//! `N G obj` headers, and builds a **synthetic [`XrefTable`]** (for duplicate
//! object numbers, *last definition wins*). It also recovers objects packed
//! inside object streams (`/Type /ObjStm`) discovered during the scan, and
//! reconstructs a trailer by locating the `/Type /Catalog` object.
//!
//! Everything here is **total**: the scan is O(n) over the source, never
//! allocates past [`Limits`], never recurses unboundedly, never panics, and
//! always terminates (PRD §9.6). Each method either makes progress or stops.
//!
//! Diagnostics ([`Warning`] / [`RepairAction`]) are collected so the result is
//! queryable (PRD §8.2 / §9.3); `kind` discriminants are stable, English-only,
//! machine-greppable strings.

use std::collections::HashMap;

use crate::error::Result;
use crate::lexer::Lexer;
use crate::limits::Limits;
use crate::object::parse::Parser;
use crate::object::{Dict, Name, ObjRef, Object, StreamData, StreamObj};
use crate::source::Source;
use crate::xref::{XrefEntry, XrefTable};

/// Parse strictness (PRD §8.2). `Lenient` (the default for `open`) attempts
/// repair and continues, recording [`Warning`]s; `Strict` returns the first
/// typed error and never substitutes `Null` for a dangling reference (intended
/// for validators / signing).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ParseMode {
    /// First violation → typed error (validators / signing).
    Strict,
    /// Best-effort: repair, substitute `Null` for dangling refs, collect
    /// warnings. The default.
    #[default]
    Lenient,
}

impl ParseMode {
    /// `true` for [`ParseMode::Lenient`].
    #[must_use]
    pub fn is_lenient(self) -> bool {
        matches!(self, ParseMode::Lenient)
    }
}

/// The stable kind of a repair-time [`Warning`] (PRD §9.3: machine-greppable,
/// never localized). The discriminant string is the contract; the human prose
/// lives in [`Warning::detail`].
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum WarningKind {
    /// `startxref` was missing or its offset was unparseable.
    StartxrefMissing,
    /// The cross-reference chain failed to parse / was inconsistent.
    XrefUnreadable,
    /// The normally-parsed document failed the catalog/page-tree validation gate.
    ValidationFailed,
    /// A stream's declared `/Length` did not match the real body extent.
    StreamLength,
    /// A reference pointed at an object with no recovered definition.
    DanglingReference,
    /// An object scanned at a byte offset could not be parsed and was skipped.
    UnparseableObject,
    /// The trailer was missing / broken and was reconstructed.
    TrailerReconstructed,
    /// Junk preceded the `%PDF-` header (nonzero `header_offset`).
    HeaderOffset,
    /// An object-stream container could not be decoded during the scan.
    ObjStmUndecodable,
}

impl WarningKind {
    /// A short, stable discriminant string (machine-greppable, never localized).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            WarningKind::StartxrefMissing => "startxref-missing",
            WarningKind::XrefUnreadable => "xref-unreadable",
            WarningKind::ValidationFailed => "validation-failed",
            WarningKind::StreamLength => "stream-length",
            WarningKind::DanglingReference => "dangling-reference",
            WarningKind::UnparseableObject => "unparseable-object",
            WarningKind::TrailerReconstructed => "trailer-reconstructed",
            WarningKind::HeaderOffset => "header-offset",
            WarningKind::ObjStmUndecodable => "objstm-undecodable",
        }
    }
}

impl std::fmt::Display for WarningKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A non-fatal diagnostic collected during a Lenient parse (PRD §8.2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Warning {
    /// Best-effort absolute byte offset of the condition.
    pub offset: usize,
    /// Stable discriminant.
    pub kind: WarningKind,
    /// Human-readable English detail (never the stable contract).
    pub detail: String,
}

/// The category of a concrete reconstruction performed by the repair pass
/// (PRD §8.2). Stable discriminants, English-only.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RepairKind {
    /// The whole cross-reference was rebuilt from a full-file object scan.
    XrefRebuilt,
    /// The trailer (`/Root`, `/Size`) was synthesized.
    TrailerSynthesized,
    /// A catalog was located by `/Type /Catalog` to set `/Root`.
    RootRecovered,
    /// A stream's body was re-derived (declared `/Length` ignored).
    StreamLengthRecovered,
    /// Objects were recovered from an object stream during the scan.
    ObjStmRecovered,
}

impl RepairKind {
    /// A short, stable discriminant string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RepairKind::XrefRebuilt => "xref-rebuilt",
            RepairKind::TrailerSynthesized => "trailer-synthesized",
            RepairKind::RootRecovered => "root-recovered",
            RepairKind::StreamLengthRecovered => "stream-length-recovered",
            RepairKind::ObjStmRecovered => "objstm-recovered",
        }
    }
}

impl std::fmt::Display for RepairKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single reconstruction action taken by the repair pass — the queryable
/// `repair_report()` element (PRD §8.2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RepairAction {
    /// Stable discriminant.
    pub kind: RepairKind,
    /// Human-readable English detail.
    pub detail: String,
}

/// Diagnostics gathered during a parse: the warnings collector plus the repair
/// report. Empty after a clean open.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    warnings: Vec<Warning>,
    actions: Vec<RepairAction>,
}

impl Diagnostics {
    /// A fresh, empty collector.
    #[must_use]
    pub fn new() -> Self {
        Diagnostics::default()
    }

    /// Records a warning.
    pub fn warn(&mut self, offset: usize, kind: WarningKind, detail: impl Into<String>) {
        self.warnings.push(Warning {
            offset,
            kind,
            detail: detail.into(),
        });
    }

    /// Records a repair action.
    pub fn action(&mut self, kind: RepairKind, detail: impl Into<String>) {
        self.actions.push(RepairAction {
            kind,
            detail: detail.into(),
        });
    }

    /// The collected warnings (PRD §8.2).
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// The repair report (PRD §8.2).
    #[must_use]
    pub fn actions(&self) -> &[RepairAction] {
        &self.actions
    }
}

/// The product of a full-file object scan: a synthetic [`XrefTable`] (entries +
/// reconstructed trailer) ready to install on a `DocumentStore`.
#[derive(Debug)]
pub struct RepairResult {
    /// The synthetic cross-reference table (with reconstructed trailer).
    pub xref: XrefTable,
}

/// Scans the entire `source` for indirect objects and reconstructs a synthetic
/// cross-reference table + trailer (PRD §8.2 repair subsystem steps 1, 2, 4, 6).
///
/// `header_offset` is the `%PDF-` byte bias; recovered offsets are **absolute**
/// file positions (PRD §8.2). `diag` accumulates warnings / actions.
///
/// The scan is a single O(n) forward pass. It honors [`Limits::max_objects`]:
/// once that many distinct object numbers have been recorded it stops scanning
/// for new ones (never unbounded growth, PRD §9.6.2). It never panics.
///
/// # Errors
///
/// Returns an error only if no usable object could be recovered at all (an empty
/// or object-free buffer); any individual unparseable object is skipped with a
/// warning, not propagated.
pub fn reconstruct(
    source: &Source,
    header_offset: usize,
    limits: &Limits,
    diag: &mut Diagnostics,
) -> Result<RepairResult> {
    let buf = source.bytes();

    // --- pass 1: locate every `N G obj` header by byte offset ---------------
    //
    // We scan for the literal keyword `obj` and, for each hit, walk *backwards*
    // over whitespace + two integer runs to recover `N` and `G`. This tolerates
    // arbitrary garbage between objects (PRD §8.2 step 5) without trusting any
    // declared length. Last definition of a given object number wins, so we
    // overwrite as we go front-to-back.
    let mut entries: HashMap<u32, XrefEntry> = HashMap::new();
    // Header offsets in scan order, used to recover stream/ObjStm bodies and to
    // locate catalogs deterministically.
    let mut objstm_candidates: Vec<u32> = Vec::new();

    let mut i = 0usize;
    // Bound the number of recovered objects (PRD §9.6.2). `usize` cap, saturating.
    let max_objects = usize::try_from(limits.max_objects).unwrap_or(usize::MAX);

    while let Some(pos) = find_obj_keyword(buf, i) {
        // Advance past this keyword regardless of what we recover, guaranteeing
        // forward progress (the keyword is 3 bytes).
        i = pos + 3;
        let Some((num, gen, header_off)) = recover_obj_header(buf, pos) else {
            continue;
        };
        // Record (last-wins). Respect the object-count ceiling: stop *adding new*
        // numbers past the cap, but keep updating numbers we already track.
        if entries.len() >= max_objects && !entries.contains_key(&num) {
            continue;
        }
        entries.insert(
            num,
            XrefEntry::Uncompressed {
                offset: header_off,
                gen,
            },
        );
        // Track potential object streams for a second decoding pass.
        if is_objstm_at(source, header_off, limits) && !objstm_candidates.contains(&num) {
            objstm_candidates.push(num);
        }
    }

    if entries.is_empty() {
        return Err(crate::error::Error::xref(
            0,
            "object scan found no indirect objects",
        ));
    }
    diag.action(
        RepairKind::XrefRebuilt,
        format!("recovered {} object(s) by full-file scan", entries.len()),
    );

    // --- pass 2: recover compressed objects from object streams -------------
    //
    // For each ObjStm candidate, decode its directory and add a `Compressed`
    // entry for every member that is *not already* an uncompressed object
    // (uncompressed/last-wins definitions take precedence — they are the real
    // bytes, PRD §8.2). Bounded by the ObjStm member cap inside `ObjStm::decode`.
    let mut objstm_recovered = 0usize;
    for objstm_num in &objstm_candidates {
        let Some(&XrefEntry::Uncompressed { offset, .. }) = entries.get(objstm_num) else {
            continue;
        };
        match decode_objstm_members(source, offset, limits) {
            Ok(members) => {
                for (idx, member_num) in members.into_iter().enumerate() {
                    if entries.len() >= max_objects && !entries.contains_key(&member_num) {
                        continue;
                    }
                    // Do not clobber a directly-defined (uncompressed) object.
                    if matches!(
                        entries.get(&member_num),
                        Some(XrefEntry::Uncompressed { .. })
                    ) {
                        continue;
                    }
                    entries.insert(
                        member_num,
                        XrefEntry::Compressed {
                            objstm_num: *objstm_num,
                            index: idx as u32,
                        },
                    );
                    objstm_recovered += 1;
                }
            }
            Err(_) => {
                diag.warn(
                    offset,
                    WarningKind::ObjStmUndecodable,
                    format!("object stream {objstm_num} could not be decoded during scan"),
                );
            }
        }
    }
    if objstm_recovered > 0 {
        diag.action(
            RepairKind::ObjStmRecovered,
            format!("recovered {objstm_recovered} object(s) from object stream(s)"),
        );
    }

    // --- reconstruct the trailer (PRD §8.2 step 2) --------------------------
    let trailer = reconstruct_trailer(source, &entries, header_offset, diag);

    Ok(RepairResult {
        xref: XrefTable::from_synthetic(entries, trailer),
    })
}

/// Finds the next `obj` keyword at or after `from`, returning the offset of its
/// first byte. The keyword must be bounded by a non-regular byte on both sides
/// so we do not match inside `endobj`, `Nobj`, names, etc.
fn find_obj_keyword(buf: &[u8], from: usize) -> Option<usize> {
    let needle = b"obj";
    let mut i = from;
    while i + needle.len() <= buf.len() {
        if &buf[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !crate::lexer::is_regular(buf[i - 1]);
            let after = i + needle.len();
            let after_ok = after >= buf.len() || !crate::lexer::is_regular(buf[after]);
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Walks backward from an `obj` keyword at `obj_pos` to recover `(num, gen,
/// header_offset)`. Returns `None` if the two preceding integer runs are absent.
fn recover_obj_header(buf: &[u8], obj_pos: usize) -> Option<(u32, u16, usize)> {
    // Skip whitespace immediately before `obj`.
    let mut p = obj_pos;
    p = skip_ws_back(buf, p);
    // Generation number (digits run).
    let (gen_start, gen_str) = take_digits_back(buf, p)?;
    p = skip_ws_back(buf, gen_start);
    // Object number (digits run).
    let (num_start, num_str) = take_digits_back(buf, p)?;

    let num: u32 = num_str.parse().ok()?;
    let gen: u16 = gen_str.parse().unwrap_or(0);
    Some((num, gen, num_start))
}

/// Returns the index just before the whitespace run ending at `end` (exclusive).
fn skip_ws_back(buf: &[u8], end: usize) -> usize {
    let mut p = end;
    while p > 0 && crate::lexer::is_whitespace(buf[p - 1]) {
        p -= 1;
    }
    p
}

/// Reads a run of ASCII digits ending at `end` (exclusive), returning its start
/// index and the parsed string slice. `None` if there is no digit at `end-1`.
fn take_digits_back(buf: &[u8], end: usize) -> Option<(usize, &str)> {
    if end == 0 || !buf[end - 1].is_ascii_digit() {
        return None;
    }
    let mut start = end;
    while start > 0 && buf[start - 1].is_ascii_digit() {
        start -= 1;
    }
    // Bound the digit run to keep parses sane (object numbers are not 100 digits).
    if end - start > 10 {
        return None;
    }
    std::str::from_utf8(&buf[start..end])
        .ok()
        .map(|s| (start, s))
}

/// Best-effort check: is the object at absolute `offset` a `/Type /ObjStm`
/// stream? Parses just the dict header (never the body) and inspects `/Type`.
fn is_objstm_at(source: &Source, offset: usize, _limits: &Limits) -> bool {
    let Ok(tail) = source.slice_from(offset) else {
        return false;
    };
    let mut parser = Parser::from_lexer(Lexer::new(tail));
    let Ok((_, obj)) = parser.parse_indirect_object() else {
        return false;
    };
    matches!(obj, Object::Stream(ref s)
        if s.dict.get(&Name::new("Type")) == Some(&Object::Name(Name::new("ObjStm"))))
}

/// Decodes the object-stream at absolute `offset`, returning the member object
/// numbers in directory order (their indices are positions in this Vec).
fn decode_objstm_members(source: &Source, offset: usize, limits: &Limits) -> Result<Vec<u32>> {
    let tail = source.slice_from(offset)?;
    let mut parser = Parser::from_lexer(Lexer::new(tail));
    let (_, obj) = parser.parse_indirect_object()?;
    let stream = match obj {
        Object::Stream(s) => s,
        _ => {
            return Err(crate::error::Error::xref(
                offset,
                "ObjStm candidate is not a stream",
            ))
        }
    };
    // Re-derive the body from the source (the parser captured Encoded bytes from
    // the `tail` slice; rebuild it as an Encoded stream over those bytes).
    let body = match &stream.data {
        StreamData::Raw { offset: o, len } => source.slice_bytes(*o, *len)?,
        StreamData::Encoded(b) | StreamData::Decoded(b) => b.clone(),
    };
    let materialized = StreamObj {
        dict: stream.dict.clone(),
        data: StreamData::Encoded(body),
    };
    let objstm = crate::objstm::ObjStm::decode(&materialized, limits)?;
    Ok(objstm.member_nums())
}

/// Reconstructs a trailer dictionary by locating the catalog (PRD §8.2 step 2).
///
/// Strategy: resolve each recovered object, find those whose dict is
/// `/Type /Catalog`, and pick the **last** by object number (prefer the newest
/// revision — PRD §8.2). Synthesize `/Root` (the catalog ref) and `/Size`
/// (max obj num + 1). If a catalog cannot be found, `/Root` is omitted (the
/// validation gate will reject the doc as unrecoverable).
fn reconstruct_trailer(
    source: &Source,
    entries: &HashMap<u32, XrefEntry>,
    _header_offset: usize,
    diag: &mut Diagnostics,
) -> Dict {
    let mut trailer = Dict::new();

    // `/Size` = max recovered object number + 1.
    let max_num = entries.keys().copied().max().unwrap_or(0);
    let size = i64::from(max_num).saturating_add(1);
    trailer.insert(Name::new("Size"), Object::Integer(size));

    // Find catalog candidates among *uncompressed* objects (cheap to parse).
    let mut catalog: Option<u32> = None;
    let mut info: Option<u32> = None;
    let mut nums: Vec<u32> = entries.keys().copied().collect();
    nums.sort_unstable();
    for num in nums {
        if let Some(XrefEntry::Uncompressed { offset, gen }) = entries.get(&num).copied() {
            if let Some(dict) = parse_dict_at(source, offset) {
                match dict.get(&Name::new("Type")) {
                    Some(Object::Name(n)) if n.as_bytes() == b"Catalog" => {
                        // Last (highest obj num) wins — keep overwriting.
                        catalog = Some(num);
                        let _ = gen;
                    }
                    Some(Object::Name(n)) if n.as_bytes() == b"Info" => {
                        info = Some(num);
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(root_num) = catalog {
        trailer.insert(
            Name::new("Root"),
            Object::Reference(ObjRef::new(root_num, 0)),
        );
        diag.action(
            RepairKind::RootRecovered,
            format!("located /Type /Catalog at object {root_num}"),
        );
    }
    if let Some(info_num) = info {
        trailer.insert(
            Name::new("Info"),
            Object::Reference(ObjRef::new(info_num, 0)),
        );
    }

    diag.action(
        RepairKind::TrailerSynthesized,
        format!("synthesized trailer with /Size {size}"),
    );
    trailer
}

/// Parses the dictionary of the (possibly stream) object at absolute `offset`,
/// or `None` if it is not a dict/stream. Total: never panics.
fn parse_dict_at(source: &Source, offset: usize) -> Option<Dict> {
    let tail = source.slice_from(offset).ok()?;
    let mut parser = Parser::from_lexer(Lexer::new(tail));
    let (_, obj) = parser.parse_indirect_object().ok()?;
    match obj {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(s.dict),
        _ => None,
    }
}
