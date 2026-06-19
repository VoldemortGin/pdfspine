//! Cross-reference machinery — the unified [`XrefTable`] and the chain parser
//! that builds it (PRD §8.2).
//!
//! A PDF locates every object through a cross-reference structure that maps an
//! object number to where its bytes live. Two physical forms exist — the classic
//! `xref` **table** (PDF ≤1.4, [`table`]) and the `/Type /XRef` **stream**
//! (PDF 1.5+, [`stream`]) — and a file may chain several of them via `/Prev`
//! (incremental updates) or overlay a stream on a table via `/XRefStm`
//! (hybrid-reference files). This module reads `startxref`, walks that chain
//! **newest → oldest**, and merges the sections into one [`XrefTable`] with
//! newest-wins semantics (PRD §8.2: "build effective xref newest→oldest").
//!
//! Everything here is **total**: malformed input yields a typed
//! [`Error::Xref`] (or another typed error), never a panic. This module reads
//! well-formed cross-reference structures and fails cleanly on the rest; the
//! full repair of a broken xref (object scan / reconstruction) lives in
//! [`crate::repair`], which the document open path uses as a fallback.

pub mod stream;
pub mod table;

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::limits::Limits;
use crate::object::{Dict, Name, Object};
use crate::source::Source;

/// One resolved cross-reference entry (PRD §8.2: free / uncompressed /
/// compressed).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum XrefEntry {
    /// A free (deleted) object — not present in the file.
    Free,
    /// An object stored directly in the file at `offset`, with generation `gen`.
    Uncompressed {
        /// Absolute byte offset of the `N G obj` header.
        offset: usize,
        /// Generation number.
        gen: u16,
    },
    /// An object stored inside an object stream (`/Type /ObjStm`).
    Compressed {
        /// Object number of the containing object stream.
        objstm_num: u32,
        /// Zero-based index of this object within that stream.
        index: u32,
    },
}

/// The unified cross-reference table: object number → [`XrefEntry`], plus the
/// merged trailer dictionary (PRD §8.2).
#[derive(Clone, Debug, Default)]
pub struct XrefTable {
    entries: HashMap<u32, XrefEntry>,
    trailer: Dict,
}

impl XrefTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        XrefTable::default()
    }

    /// Builds a table directly from entries and a trailer. Used by the repair
    /// subsystem ([`crate::repair`]) to install a **synthetic** cross-reference
    /// after a full-file object scan (PRD §8.2). Later insertions of the same
    /// object number overwrite earlier ones (the caller scans front-to-back, so
    /// **last definition wins**).
    #[must_use]
    pub fn from_synthetic(entries: HashMap<u32, XrefEntry>, trailer: Dict) -> Self {
        XrefTable { entries, trailer }
    }

    /// The merged trailer dictionary (`/Root`, `/Size`, `/Info`, `/ID`, …).
    #[must_use]
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// The entry for `num`, if any.
    #[must_use]
    pub fn get(&self, num: u32) -> Option<XrefEntry> {
        self.entries.get(&num).copied()
    }

    /// The number of recorded entries (free + in-use).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no entries are recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The recorded object numbers, **sorted ascending**. The page-tree fallback
    /// scan ([`crate::pagetree`]) uses this to enumerate candidate `/Type /Page`
    /// objects in object-number order when the `/Pages` tree is unreachable
    /// (PRD §8.2 step 3). The order is deterministic so the recovered page list
    /// is stable.
    #[must_use]
    pub fn object_numbers(&self) -> Vec<u32> {
        let mut nums: Vec<u32> = self.entries.keys().copied().collect();
        nums.sort_unstable();
        nums
    }

    /// Inserts an entry **only if absent** — the newest-wins merge primitive.
    /// When walking newest → oldest, the first writer of an object number is the
    /// newest revision and must not be overwritten by an older section.
    fn insert_if_absent(&mut self, num: u32, entry: XrefEntry) {
        self.entries.entry(num).or_insert(entry);
    }

    /// Merges a single parsed [`XrefSection`] into `self` (newest-wins). The
    /// trailer is filled key-by-key, again only where absent, so the newest
    /// section's trailer keys take precedence.
    fn merge_section(&mut self, section: XrefSection) {
        for (num, entry) in section.entries {
            self.insert_if_absent(num, entry);
        }
        for (k, v) in section.trailer {
            self.trailer.entry(k).or_insert(v);
        }
    }
}

/// A single parsed cross-reference section (one table or one xref stream) before
/// it is merged into the unified [`XrefTable`]. Internal to the chain walker.
#[derive(Clone, Debug, Default)]
pub(crate) struct XrefSection {
    /// Entries declared by this section.
    pub entries: Vec<(u32, XrefEntry)>,
    /// This section's trailer dictionary.
    pub trailer: Dict,
}

impl XrefSection {
    /// The `/Prev` offset (previous cross-reference section), if declared.
    fn prev(&self) -> Option<usize> {
        self.trailer
            .get(&Name::new("Prev"))
            .and_then(Object::as_i64)
            .and_then(|v| usize::try_from(v).ok())
    }

    /// The `/XRefStm` offset (hybrid-reference overlay), if declared.
    fn xref_stm(&self) -> Option<usize> {
        self.trailer
            .get(&Name::new("XRefStm"))
            .and_then(Object::as_i64)
            .and_then(|v| usize::try_from(v).ok())
    }

    /// Biases every uncompressed-entry offset by `header_offset`, turning the
    /// header-relative stored offsets into absolute file positions (PRD §8.2).
    fn bias_offsets(&mut self, header_offset: usize) {
        if header_offset == 0 {
            return;
        }
        for (_num, entry) in &mut self.entries {
            if let XrefEntry::Uncompressed { offset, .. } = entry {
                *offset = offset.saturating_add(header_offset);
            }
        }
    }
}

/// Maximum number of cross-reference sections followed via `/Prev` / `/XRefStm`.
/// Bounds a pathological / cyclic chain (PRD §8.2: "cycle detection mandatory").
const MAX_XREF_SECTIONS: usize = 4096;

/// Reads `startxref` near the end of `source` and walks the whole
/// cross-reference chain, returning the merged [`XrefTable`] (PRD §8.2).
///
/// `header_offset` is the byte bias of the `%PDF-` header (nonzero when junk
/// precedes it); stored xref offsets are interpreted relative to it and resolved
/// to absolute file positions immediately (PRD §8.2).
///
/// # Errors
///
/// [`Error::Xref`] when `startxref` is missing/garbage or a section is
/// malformed; other typed errors propagate from the stream/predictor decoders.
pub fn parse_xref_chain(
    source: &Source,
    header_offset: usize,
    limits: &Limits,
) -> Result<XrefTable> {
    let start = find_startxref(source)?;
    let mut table = XrefTable::new();

    // Visited offsets guard against `/Prev` cycles (PRD §8.2).
    let mut visited = std::collections::HashSet::new();
    let mut next = Some(absolute(start, header_offset));
    let mut sections = 0usize;

    while let Some(off) = next {
        if !visited.insert(off) {
            // Already-seen offset — a `/Prev` cycle. Stop cleanly (newest
            // sections already merged); never loop forever.
            break;
        }
        sections += 1;
        if sections > MAX_XREF_SECTIONS {
            return Err(Error::xref(off, "cross-reference chain too long"));
        }

        let mut section = parse_section_at(source, off, header_offset, limits)?;
        // Stored offsets are header-relative (PRD §8.2): bias uncompressed-entry
        // offsets to absolute file positions before merging.
        section.bias_offsets(header_offset);

        // Hybrid-reference: overlay the `/XRefStm` stream *before* this section's
        // own `/Prev`, but *after* the classic entries already merged for this
        // revision (PRD §8.2: "read table first, overlay xref-stream objects").
        let xref_stm = section.xref_stm();
        let prev = section.prev();
        table.merge_section(section);

        if let Some(stm_off) = xref_stm {
            let abs = absolute(stm_off, header_offset);
            if visited.insert(abs) {
                let mut stm = stream::parse_xref_stream_at(source, abs, limits)?;
                stm.bias_offsets(header_offset);
                table.merge_section(stm);
            }
        }

        next = prev.map(|p| absolute(p, header_offset));
    }

    if table.is_empty() && table.trailer.is_empty() {
        return Err(Error::xref(start, "no cross-reference entries found"));
    }
    Ok(table)
}

/// Counts the cross-reference **revisions** in `source` — the number of
/// generations chained from the last `startxref` via `/Prev` (PyMuPDF
/// `Document.version_count`).
///
/// A freshly authored / fully rewritten PDF has a single cross-reference section
/// → `1`; each incremental update appends another section whose `/Prev` points
/// at the prior one → `+1` per update. A hybrid-reference `/XRefStm` overlay is
/// part of the *same* revision as its classic table, so it does **not** advance
/// the count (only `/Prev` does), mirroring fitz's revision count.
///
/// Returns `1` (a single, current revision) when the chain cannot be read — a
/// repaired / synthetic document has no walkable `/Prev` history.
#[must_use]
pub fn count_xref_sections(source: &Source, header_offset: usize, limits: &Limits) -> usize {
    let Ok(start) = find_startxref(source) else {
        return 1;
    };
    let mut visited = std::collections::HashSet::new();
    let mut next = Some(absolute(start, header_offset));
    let mut sections = 0usize;
    while let Some(off) = next {
        if !visited.insert(off) {
            break; // `/Prev` cycle — stop cleanly.
        }
        sections += 1;
        if sections >= MAX_XREF_SECTIONS {
            break;
        }
        let Ok(section) = parse_section_at(source, off, header_offset, limits) else {
            break;
        };
        next = section.prev().map(|p| absolute(p, header_offset));
    }
    sections.max(1)
}

/// Resolves a stored (header-relative, per spec convention) offset to an
/// absolute file position. Saturating add keeps the result in range without
/// overflow (PRD §9.6: checked/saturating arithmetic).
fn absolute(stored: usize, header_offset: usize) -> usize {
    stored.saturating_add(header_offset)
}

/// Parses whichever cross-reference form starts at absolute offset `off`: a
/// classic `xref` table (+ `trailer`) or a `/Type /XRef` stream.
fn parse_section_at(
    source: &Source,
    off: usize,
    header_offset: usize,
    limits: &Limits,
) -> Result<XrefSection> {
    let buf = source.bytes();
    // Skip leading whitespace to peek the first keyword/token.
    let mut p = off;
    while matches!(buf.get(p), Some(&b) if crate::lexer::is_whitespace(b)) {
        p += 1;
    }
    if buf.get(p..p + 4) == Some(b"xref") {
        table::parse_table_at(source, p, header_offset, limits)
    } else {
        // Otherwise it must be a cross-reference stream (`N G obj << … >>`).
        stream::parse_xref_stream_at(source, off, limits)
    }
}

/// Locates the **last** `startxref` keyword and reads the integer offset that
/// follows it (PRD §8.2: "multiple `%%EOF` → use last `startxref`").
///
/// # Errors
///
/// [`Error::Xref`] when no `startxref` exists or its offset is unparsable —
/// repair (full object scan) is M1d; this reader fails cleanly.
pub fn find_startxref(source: &Source) -> Result<usize> {
    let buf = source.bytes();
    let needle = b"startxref";
    // Scan the tail (a generous window covers the spec's "near the end").
    let window_start = buf.len().saturating_sub(2048);
    let hay = &buf[window_start..];

    // Find the *last* occurrence within the window.
    let rel = hay
        .windows(needle.len())
        .rposition(|w| w == needle)
        .ok_or_else(|| Error::xref(buf.len(), "no startxref in file tail"))?;
    let mut p = window_start + rel + needle.len();

    // Skip whitespace, then read the decimal offset.
    while matches!(buf.get(p), Some(&b) if crate::lexer::is_whitespace(b)) {
        p += 1;
    }
    let digits_start = p;
    while matches!(buf.get(p), Some(b'0'..=b'9')) {
        p += 1;
    }
    if p == digits_start {
        return Err(Error::xref(
            digits_start,
            "startxref offset is not a number",
        ));
    }
    // Safe: the slice is pure ASCII digits.
    let s = std::str::from_utf8(&buf[digits_start..p])
        .map_err(|_| Error::xref(digits_start, "startxref offset not ASCII"))?;
    s.parse::<usize>()
        .map_err(|_| Error::xref(digits_start, "startxref offset out of range"))
}
