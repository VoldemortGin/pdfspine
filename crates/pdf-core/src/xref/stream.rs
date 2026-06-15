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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::flate;
    use crate::object::{Dict, Name, Object, StreamData, StreamObj};

    /// Builds a `Dict` from `(key, value)` pairs.
    fn dict(pairs: impl IntoIterator<Item = (&'static str, Object)>) -> Dict {
        let mut d = Dict::new();
        for (k, v) in pairs {
            d.insert(Name::new(k), v);
        }
        d
    }

    /// Packs `(f1,f2,f3)` records into big-endian fixed-width bytes per `widths`.
    fn pack(records: &[(u64, u64, u64)], widths: [usize; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        for &(f1, f2, f3) in records {
            for (val, w) in [(f1, widths[0]), (f2, widths[1]), (f3, widths[2])] {
                let bytes = val.to_be_bytes();
                out.extend_from_slice(&bytes[8 - w..]);
            }
        }
        out
    }

    /// Builds a `/Type /XRef` `StreamObj` from packed records, Flate-encoded,
    /// with `/W`, `/Size` and an optional `/Index`.
    fn xref_stream(
        records: &[(u64, u64, u64)],
        widths: [usize; 3],
        size: i64,
        index: Option<Vec<i64>>,
    ) -> StreamObj {
        let data = pack(records, widths);
        let enc = flate::encode(&data);
        let mut d = dict([
            ("Type", Object::Name(Name::new("XRef"))),
            ("Filter", Object::Name(Name::new("FlateDecode"))),
            ("Length", Object::Integer(enc.len() as i64)),
            ("Size", Object::Integer(size)),
            (
                "W",
                Object::Array(widths.iter().map(|&w| Object::Integer(w as i64)).collect()),
            ),
        ]);
        if let Some(idx) = index {
            d.insert(
                Name::new("Index"),
                Object::Array(idx.into_iter().map(Object::Integer).collect()),
            );
        }
        StreamObj::new_encoded(d, enc)
    }

    fn parse(stream: &StreamObj) -> Result<XrefSection> {
        parse_xref_stream_obj(stream, &Limits::unbounded_decode(), 0)
    }

    #[test]
    fn xrefstm_read_index_default() {
        let d = dict([]);
        assert_eq!(read_index(&d, 5).unwrap(), vec![(0, 5)]);
    }

    #[test]
    fn xrefstm_read_index_explicit_pairs() {
        let d = dict([(
            "Index",
            Object::Array(vec![
                Object::Integer(2),
                Object::Integer(3),
                Object::Integer(10),
                Object::Integer(1),
            ]),
        )]);
        assert_eq!(read_index(&d, 0).unwrap(), vec![(2, 3), (10, 1)]);
    }

    #[test]
    fn xrefstm_read_index_odd_length_err() {
        let d = dict([(
            "Index",
            Object::Array(vec![
                Object::Integer(1),
                Object::Integer(2),
                Object::Integer(3),
            ]),
        )]);
        let e = read_index(&d, 0).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_read_index_not_array_err() {
        let d = dict([("Index", Object::Integer(5))]);
        let e = read_index(&d, 0).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_read_index_start_not_u32_err() {
        let d = dict([(
            "Index",
            Object::Array(vec![Object::Integer(-1), Object::Integer(2)]),
        )]);
        let e = read_index(&d, 0).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_read_w_variants() {
        let ok = dict([(
            "W",
            Object::Array(vec![
                Object::Integer(1),
                Object::Integer(2),
                Object::Integer(1),
            ]),
        )]);
        assert_eq!(read_w(&ok), Some(vec![1, 2, 1]));

        let non_int = dict([(
            "W",
            Object::Array(vec![
                Object::Integer(1),
                Object::Name(Name::new("x")),
                Object::Integer(1),
            ]),
        )]);
        assert_eq!(read_w(&non_int), None);

        let missing = dict([]);
        assert_eq!(read_w(&missing), None);
    }

    #[test]
    fn xrefstm_split_fields() {
        assert_eq!(
            split_fields(&[0x01, 0x00, 0x10, 0x05], &[1, 2, 1]),
            (1, 0x0010, 5)
        );
        // Zero-width field-1 reads as 0.
        assert_eq!(
            split_fields(&[0x00, 0x10, 0x05], &[0, 2, 1]),
            (0, 0x0010, 5)
        );
    }

    #[test]
    fn xrefstm_missing_w_err() {
        let enc = flate::encode(b"");
        let d = dict([
            ("Type", Object::Name(Name::new("XRef"))),
            ("Size", Object::Integer(1)),
            ("Length", Object::Integer(enc.len() as i64)),
        ]);
        let stream = StreamObj::new_encoded(d, enc);
        let e = parse(&stream).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_w_wrong_length_err() {
        let stream = xref_stream(&[(1, 0, 0)], [1, 2, 0], 1, None);
        // Re-stamp /W to length 2.
        let mut stream = stream;
        stream.dict.insert(
            Name::new("W"),
            Object::Array(vec![Object::Integer(1), Object::Integer(2)]),
        );
        let e = parse(&stream).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_w_all_zero_err() {
        let stream = xref_stream(&[], [0, 0, 0], 1, None);
        let e = parse(&stream).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_missing_size_err() {
        let data = pack(&[(1, 0, 0)], [1, 2, 1]);
        let enc = flate::encode(&data);
        let d = dict([
            ("Type", Object::Name(Name::new("XRef"))),
            ("Filter", Object::Name(Name::new("FlateDecode"))),
            ("Length", Object::Integer(enc.len() as i64)),
            (
                "W",
                Object::Array(vec![
                    Object::Integer(1),
                    Object::Integer(2),
                    Object::Integer(1),
                ]),
            ),
        ]);
        let stream = StreamObj::new_encoded(d, enc);
        let e = parse(&stream).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_all_three_entry_types() {
        let widths = [1usize, 2, 2];
        let records = vec![
            (0u64, 0u64, 0u64), // type 0 free
            (1, 1234, 7),       // type 1 uncompressed offset=1234 gen=7
            (2, 42, 3),         // type 2 compressed objstm_num=42 index=3
        ];
        let stream = xref_stream(&records, widths, 3, None);
        let section = parse(&stream).unwrap();

        assert_eq!(section.entries.len(), 3);
        assert_eq!(section.entries[0], (0, XrefEntry::Free));
        assert_eq!(
            section.entries[1],
            (
                1,
                XrefEntry::Uncompressed {
                    offset: 1234,
                    gen: 7
                }
            )
        );
        assert_eq!(
            section.entries[2],
            (
                2,
                XrefEntry::Compressed {
                    objstm_num: 42,
                    index: 3
                }
            )
        );
    }

    #[test]
    fn xrefstm_field1_width_zero_defaults_to_uncompressed() {
        let widths = [0usize, 2, 1];
        // One record encoding offset=512 gen=0; field-1 absent.
        let records = vec![(0u64, 512u64, 0u64)];
        let stream = xref_stream(&records, widths, 1, None);
        let section = parse(&stream).unwrap();
        assert_eq!(section.entries.len(), 1);
        assert_eq!(
            section.entries[0],
            (
                0,
                XrefEntry::Uncompressed {
                    offset: 512,
                    gen: 0
                }
            )
        );
    }

    #[test]
    fn xrefstm_unknown_type_treated_as_free() {
        let widths = [1usize, 2, 1];
        let records = vec![(7u64, 99u64, 0u64)]; // type 7 unknown
        let stream = xref_stream(&records, widths, 1, None);
        let section = parse(&stream).unwrap();
        assert_eq!(section.entries[0], (0, XrefEntry::Free));
    }

    #[test]
    fn xrefstm_data_shorter_than_index_err() {
        let widths = [1usize, 2, 2];
        // Size 5 but only one record packed.
        let stream = xref_stream(&[(1, 0, 0)], widths, 5, None);
        let e = parse(&stream).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn xrefstm_raw_body_not_materialized_err() {
        let d = dict([
            ("Type", Object::Name(Name::new("XRef"))),
            ("Size", Object::Integer(1)),
            (
                "W",
                Object::Array(vec![
                    Object::Integer(1),
                    Object::Integer(2),
                    Object::Integer(1),
                ]),
            ),
        ]);
        let stream = StreamObj {
            dict: d,
            data: StreamData::Raw { offset: 0, len: 0 },
        };
        let e = parse(&stream).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }
}
