//! Cross-reference **stream** parsing — ISO 32000-1 §7.5.8 / PRD §8.2.
//!
//! A PDF 1.5+ file may store its cross-reference data as a `/Type /XRef` stream
//! (mandatory to support, PRD §8.2). The stream's decoded bytes are a packed
//! table of fixed-width records; the field widths come from `/W [w1 w2 w3]`, the
//! covered object ranges from `/Index` (default `[0 /Size]`), and each record's
//! type-0/1/2 is the `w1` field:
//!
//! | type | field 2 | field 3 | meaning |
//! |---|---|---|---|
//! | 0 | next-free obj num | next gen | free entry |
//! | 1 | byte offset | generation | uncompressed object |
//! | 2 | object-stream num | index within it | compressed object |
//!
//! A `w` of 0 means "field absent, use the default" (type defaults to 1).
//! Decoding reuses the M1b [`crate::filters::decode_stream`] (Flate +
//! predictors). Everything is total: a bad `/W`, short data, or decode failure
//! is a typed [`Error`], never a panic.

use crate::error::{Error, Result};
use crate::filters::decode_stream;
use crate::lexer::Lexer;
use crate::limits::Limits;
use crate::object::parse::Parser;
use crate::object::{Name, Object, StreamData, StreamObj};
use crate::source::Source;

use super::{XrefEntry, XrefSection};

/// Parses a cross-reference stream whose `N G obj` header starts at absolute
/// offset `off`, returning its entries and trailer (the stream dict doubles as
/// the trailer for an xref stream, PRD §8.2).
///
/// # Errors
///
/// [`Error::Xref`] on a malformed object / dict / `/W`; decode errors propagate.
pub(crate) fn parse_xref_stream_at(
    source: &Source,
    off: usize,
    limits: &Limits,
) -> Result<XrefSection> {
    let buf = source.bytes();
    let tail = buf
        .get(off..)
        .ok_or_else(|| Error::xref(off, "xref stream offset past end of file"))?;

    let mut parser = Parser::from_lexer(Lexer::new(tail));
    let (_r, obj) = parser
        .parse_indirect_object()
        .map_err(|_| Error::xref(off, "malformed xref stream object"))?;

    let stream = match obj {
        Object::Stream(s) => s,
        _ => return Err(Error::xref(off, "xref object is not a stream")),
    };

    parse_xref_stream_obj(&stream, limits, off)
}

/// Decodes an already-parsed `/Type /XRef` [`StreamObj`] into an [`XrefSection`].
/// Split out so a `DocumentStore` that has already materialized the stream can
/// reuse it.
pub(crate) fn parse_xref_stream_obj(
    stream: &StreamObj,
    limits: &Limits,
    off: usize,
) -> Result<XrefSection> {
    let dict = &stream.dict;

    // `/W [w1 w2 w3]` — the three field widths (bytes). Mandatory.
    let widths = read_w(dict).ok_or_else(|| Error::xref(off, "xref stream missing/bad /W"))?;
    if widths.len() != 3 {
        return Err(Error::xref(off, "xref stream /W must have 3 entries"));
    }
    let total: usize = widths.iter().sum();
    if total == 0 {
        return Err(Error::xref(off, "xref stream /W widths are all zero"));
    }

    // `/Size` and the optional `/Index` ranges (default `[0 Size]`).
    let size = dict
        .get(&Name::new("Size"))
        .and_then(Object::as_i64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| Error::xref(off, "xref stream missing /Size"))?;
    let index = read_index(dict, size)?;

    // Decode the container bytes (Flate + predictors).
    let raw = match &stream.data {
        StreamData::Encoded(b) | StreamData::Decoded(b) => b.clone(),
        // A `Raw` body would need a `Source`; `parse_xref_stream_at` always
        // produces an owned (`Encoded`) body, so this branch is unreachable in
        // practice but kept total.
        StreamData::Raw { .. } => {
            return Err(Error::xref(off, "xref stream body not materialized"))
        }
    };
    let decoded = decode_stream(dict, &raw, limits)?.into_decoded()?;

    let mut entries = Vec::new();
    let mut cursor = 0usize;
    for (start, count) in index {
        for i in 0..count {
            let rec = decoded
                .get(cursor..cursor + total)
                .ok_or_else(|| Error::xref(off, "xref stream data shorter than /Index implies"))?;
            cursor += total;

            let (f1, f2, f3) = split_fields(rec, &widths);
            let num = start
                .checked_add(i)
                .ok_or_else(|| Error::xref(off, "xref stream object number overflow"))?;

            // Field-1 width 0 → default type 1 (ISO 32000-1 §7.5.8.2).
            let kind = if widths[0] == 0 { 1 } else { f1 };
            let entry = match kind {
                0 => XrefEntry::Free,
                1 => XrefEntry::Uncompressed {
                    offset: usize::try_from(f2)
                        .map_err(|_| Error::xref(off, "xref stream offset out of range"))?,
                    gen: u16::try_from(f3).unwrap_or(0),
                },
                2 => XrefEntry::Compressed {
                    objstm_num: u32::try_from(f2)
                        .map_err(|_| Error::xref(off, "objstm number out of range"))?,
                    index: u32::try_from(f3)
                        .map_err(|_| Error::xref(off, "objstm index out of range"))?,
                },
                // Unknown type → treat as free (spec: ignore/reserved).
                _ => XrefEntry::Free,
            };
            entries.push((num, entry));
        }
    }

    Ok(XrefSection {
        entries,
        trailer: dict.clone(),
    })
}

/// Reads `/W` as exactly the byte widths array.
fn read_w(dict: &crate::object::Dict) -> Option<Vec<usize>> {
    let arr = dict.get(&Name::new("W"))?.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for o in arr {
        let v = o.as_i64()?;
        out.push(usize::try_from(v).ok()?);
    }
    Some(out)
}

/// Reads `/Index` as `(start, count)` pairs; defaults to `[(0, size)]`.
fn read_index(dict: &crate::object::Dict, size: u32) -> Result<Vec<(u32, u32)>> {
    match dict.get(&Name::new("Index")) {
        None => Ok(vec![(0, size)]),
        Some(Object::Array(a)) => {
            if a.len() % 2 != 0 {
                return Err(Error::xref(0, "/Index must have an even length"));
            }
            let mut out = Vec::with_capacity(a.len() / 2);
            let mut it = a.iter();
            while let (Some(s), Some(c)) = (it.next(), it.next()) {
                let s = s
                    .as_i64()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| Error::xref(0, "/Index start not a u32"))?;
                let c = c
                    .as_i64()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| Error::xref(0, "/Index count not a u32"))?;
                out.push((s, c));
            }
            Ok(out)
        }
        Some(_) => Err(Error::xref(0, "/Index is not an array")),
    }
}

/// Splits a record into its three big-endian fields per the `/W` widths. A
/// zero-width field reads as 0 (the caller applies type-default semantics).
fn split_fields(rec: &[u8], widths: &[usize]) -> (u64, u64, u64) {
    let mut p = 0;
    let mut read = |w: usize| -> u64 {
        let mut v = 0u64;
        for _ in 0..w {
            // `rec` length == sum(widths) by construction, so indexing is in
            // range; use `get` anyway to stay panic-free.
            let b = rec.get(p).copied().unwrap_or(0);
            v = (v << 8) | u64::from(b);
            p += 1;
        }
        v
    };
    let f1 = read(widths[0]);
    let f2 = read(widths[1]);
    let f3 = read(widths[2]);
    (f1, f2, f3)
}
