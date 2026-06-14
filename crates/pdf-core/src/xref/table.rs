//! Classic cross-reference **table** parsing — ISO 32000-1 §7.5.4 / PRD §8.2.
//!
//! Layout:
//!
//! ```text
//! xref
//! 0 6                 % subsection: first-object-number  count
//! 0000000000 65535 f  % 20-byte entry: offset(10) gen(5) type(1) EOL(2)
//! 0000000017 00000 n
//! …
//! trailer
//! << /Size 6 /Root 1 0 R … >>
//! ```
//!
//! Tolerances (PRD §8.2): a section may have several subsections; entries are
//! nominally 20 bytes (`\r\n` terminator) but 19-byte / bare-`\n` variants are
//! accepted; object 0 heads the free list at gen 65535; `/Size` may lie.

use crate::error::{Error, Result};
use crate::lexer::{is_whitespace, Lexer};
use crate::limits::Limits;
use crate::object::parse::Parser;
use crate::object::Object;
use crate::source::Source;

use super::{XrefEntry, XrefSection};

/// Parses a classic `xref` table (the `xref` keyword starts at absolute `off`)
/// followed by its `trailer` dictionary, into an [`XrefSection`].
///
/// `_header_offset` / `_limits` are accepted for signature symmetry with the
/// stream parser; the classic table needs neither (entry offsets are absolute
/// after the chain walker's bias, applied by the caller's `absolute`).
///
/// # Errors
///
/// [`Error::Xref`] on a malformed header, subsection line or trailer.
pub(crate) fn parse_table_at(
    source: &Source,
    off: usize,
    _header_offset: usize,
    _limits: &Limits,
) -> Result<XrefSection> {
    let buf = source.bytes();
    let mut p = off;

    // Consume the `xref` keyword.
    if buf.get(p..p + 4) != Some(b"xref") {
        return Err(Error::xref(p, "expected 'xref' keyword"));
    }
    p += 4;
    p = skip_ws(buf, p);

    let mut entries: Vec<(u32, XrefEntry)> = Vec::new();

    // Read subsections until we hit the `trailer` keyword.
    loop {
        if buf.get(p..p + 7) == Some(b"trailer") {
            p += 7;
            break;
        }
        // A subsection header: `start count`.
        let (start, np) =
            read_uint(buf, p).ok_or_else(|| Error::xref(p, "expected subsection start number"))?;
        p = skip_intra_line_ws(buf, np);
        let (count, np) =
            read_uint(buf, p).ok_or_else(|| Error::xref(p, "expected subsection entry count"))?;
        p = skip_eol(buf, np);

        let start =
            u32::try_from(start).map_err(|_| Error::xref(p, "subsection start out of range"))?;

        for i in 0..count {
            let (entry, np) = read_entry(buf, p)?;
            p = np;
            let num = start
                .checked_add(u32::try_from(i).map_err(|_| Error::xref(p, "entry index overflow"))?)
                .ok_or_else(|| Error::xref(p, "object number overflow"))?;
            entries.push((num, entry));
        }
        p = skip_ws(buf, p);
    }

    // Parse the trailer dictionary that follows the `trailer` keyword.
    p = skip_ws(buf, p);
    let trailer = parse_trailer_dict(buf, p)?;

    Ok(XrefSection { entries, trailer })
}

/// Reads one cross-reference entry starting at `p`. Nominally 20 bytes
/// (`oooooooooo ggggg t\r\n`) but tolerant of 19-byte / bare-`\n` terminators
/// (PRD §8.2). Returns the entry and the position just past it.
fn read_entry(buf: &[u8], p: usize) -> Result<(XrefEntry, usize)> {
    let p = skip_intra_line_ws(buf, p);
    let (offset, np) =
        read_uint(buf, p).ok_or_else(|| Error::xref(p, "entry offset not a number"))?;
    let np = skip_intra_line_ws(buf, np);
    let (gen, np) =
        read_uint(buf, np).ok_or_else(|| Error::xref(np, "entry generation not a number"))?;
    let np = skip_intra_line_ws(buf, np);
    let kind = match buf.get(np) {
        Some(b'n') => XrefEntry::Uncompressed {
            offset: usize::try_from(offset)
                .map_err(|_| Error::xref(np, "entry offset out of range"))?,
            gen: u16::try_from(gen).unwrap_or(0),
        },
        Some(b'f') => XrefEntry::Free,
        _ => return Err(Error::xref(np, "entry type is not 'n' or 'f'")),
    };
    let np = np + 1;
    // Skip the entry's trailing EOL (CRLF, LF, CR, or trailing spaces).
    let np = skip_eol(buf, np);
    Ok((kind, np))
}

/// Parses the trailer dictionary `<< … >>` at `p` using the object parser.
fn parse_trailer_dict(buf: &[u8], p: usize) -> Result<crate::object::Dict> {
    let mut parser = Parser::from_lexer(Lexer::new(&buf[p..]));
    match parser.parse_object() {
        Ok(Object::Dictionary(d)) => Ok(d),
        Ok(_) => Err(Error::xref(p, "trailer is not a dictionary")),
        Err(_) => Err(Error::xref(p, "malformed trailer dictionary")),
    }
}

// --- small byte-level scanners (all bounds-checked) -----------------------

/// Reads a run of ASCII digits as `u64`. Returns `(value, next_pos)` or `None`.
fn read_uint(buf: &[u8], mut p: usize) -> Option<(u64, usize)> {
    let start = p;
    let mut v: u64 = 0;
    while let Some(&b @ b'0'..=b'9') = buf.get(p) {
        v = v.saturating_mul(10).saturating_add(u64::from(b - b'0'));
        p += 1;
    }
    if p == start {
        None
    } else {
        Some((v, p))
    }
}

/// Skips spaces and tabs (intra-line whitespace) only.
fn skip_intra_line_ws(buf: &[u8], mut p: usize) -> usize {
    while matches!(buf.get(p), Some(b' ') | Some(b'\t')) {
        p += 1;
    }
    p
}

/// Skips one end-of-line (and any trailing intra-line spaces around it),
/// tolerating CRLF, lone LF and lone CR (PRD §8.2 entry-variant tolerance).
fn skip_eol(buf: &[u8], mut p: usize) -> usize {
    p = skip_intra_line_ws(buf, p);
    match buf.get(p) {
        Some(b'\r') => {
            p += 1;
            if buf.get(p) == Some(&b'\n') {
                p += 1;
            }
        }
        Some(b'\n') => p += 1,
        _ => {}
    }
    p
}

/// Skips all PDF whitespace.
fn skip_ws(buf: &[u8], mut p: usize) -> usize {
    while matches!(buf.get(p), Some(&b) if is_whitespace(b)) {
        p += 1;
    }
    p
}
