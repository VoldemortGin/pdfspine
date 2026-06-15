//! Object-stream (`/Type /ObjStm`) decoding — ISO 32000-1 §7.5.7 / PRD §8.2.
//!
//! An object stream packs several indirect objects into one Flate-compressed
//! container so they cross-reference more compactly. Its dict carries `/N` (the
//! number of contained objects) and `/First` (the byte offset, within the
//! decoded body, where the first object's bytes begin). The body is:
//!
//! ```text
//! n1 off1  n2 off2  …  nN offN      % the /First-byte header: N (num offset) pairs
//! <object 1><object 2>…<object N>   % each object's serialized value, no `obj`/`endobj`
//! ```
//!
//! Objects inside an ObjStm are never themselves streams and never encrypted
//! individually (PRD §8.2). The decoder enforces
//! [`Limits::max_objstm_objects`]. Everything is total: malformed headers /
//! offsets are typed [`Error`]s, never panics.

use crate::error::{Error, LimitKind, Result};
use crate::filters::decode_stream;
use crate::lexer::Lexer;
use crate::limits::Limits;
use crate::object::parse::Parser;
use crate::object::{Name, Object, StreamObj};

/// A decoded object stream: the (number, byte-offset) directory plus the decoded
/// body bytes, ready for per-object extraction.
#[derive(Clone, Debug)]
pub struct ObjStm {
    /// `(object number, offset-within-body)` for each contained object, in order.
    entries: Vec<(u32, usize)>,
    /// `/First` — the byte offset where object data begins.
    first: usize,
    /// The fully decoded container body.
    body: Vec<u8>,
}

impl ObjStm {
    /// Decodes an object-stream [`StreamObj`] (already materialized, i.e. its
    /// body is owned) into an [`ObjStm`].
    ///
    /// # Errors
    ///
    /// [`Error::Xref`] on a malformed `/N` / `/First` / offset table;
    /// [`Error::LimitExceeded`]`(MaxObjstmObjects)` when `/N` exceeds the cap;
    /// decode errors propagate from the container filter chain.
    pub fn decode(stream: &StreamObj, limits: &Limits) -> Result<ObjStm> {
        let dict = &stream.dict;
        let n = dict
            .get(&Name::new("N"))
            .and_then(Object::as_i64)
            .and_then(|v| u64::try_from(v).ok())
            .ok_or_else(|| Error::xref(0, "object stream missing /N"))?;
        if n > limits.max_objstm_objects {
            return Err(Error::LimitExceeded(LimitKind::ObjstmObjects));
        }
        let n = usize::try_from(n).map_err(|_| Error::xref(0, "/N out of range"))?;

        let first = dict
            .get(&Name::new("First"))
            .and_then(Object::as_i64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| Error::xref(0, "object stream missing /First"))?;

        // Materialize the container body (owned, Encoded/Decoded).
        let raw = stream
            .data
            .owned_bytes()
            .ok_or_else(|| Error::xref(0, "object stream body not materialized"))?
            .clone();
        let body = decode_stream(dict, &raw, limits)?.into_decoded()?;

        // Parse the `/First`-byte header: N pairs of `num offset`.
        let header = body
            .get(..first.min(body.len()))
            .ok_or_else(|| Error::xref(0, "object stream /First past body"))?;
        let mut parser = Parser::from_lexer(Lexer::new(header));
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let num = match parser.parse_object() {
                Ok(Object::Integer(v)) if v >= 0 => u32::try_from(v)
                    .map_err(|_| Error::xref(0, "objstm object number out of range"))?,
                _ => return Err(Error::xref(0, "objstm header: expected object number")),
            };
            let off = match parser.parse_object() {
                Ok(Object::Integer(v)) if v >= 0 => {
                    usize::try_from(v).map_err(|_| Error::xref(0, "objstm offset out of range"))?
                }
                _ => return Err(Error::xref(0, "objstm header: expected offset")),
            };
            entries.push((num, off));
        }

        Ok(ObjStm {
            entries,
            first,
            body,
        })
    }

    /// The number of contained objects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the stream contains no objects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The object number at directory `index`, if in range.
    #[must_use]
    pub fn object_number_at(&self, index: usize) -> Option<u32> {
        self.entries.get(index).map(|&(num, _)| num)
    }

    /// The contained object numbers in directory order (index = position). Used
    /// by the repair scan to register `Compressed` xref entries (PRD §8.2).
    #[must_use]
    pub fn member_nums(&self) -> Vec<u32> {
        self.entries.iter().map(|&(num, _)| num).collect()
    }

    /// Parses and returns the object at directory `index` (0-based). The object's
    /// bytes run from `first + off` to the next entry's start (or end of body).
    ///
    /// # Errors
    ///
    /// [`Error::Xref`] for an out-of-range index or a body too short for the
    /// declared offset; parse errors propagate.
    pub fn object_at(&self, index: usize) -> Result<Object> {
        let &(_num, off) = self
            .entries
            .get(index)
            .ok_or_else(|| Error::xref(0, "objstm index out of range"))?;
        let abs_start = self
            .first
            .checked_add(off)
            .ok_or_else(|| Error::xref(0, "objstm offset overflow"))?;
        // `abs_start` must itself lie within the body.
        if abs_start > self.body.len() {
            return Err(Error::xref(0, "objstm object slice out of range"));
        }
        // The object runs to the next directory offset (or end of body), clamped
        // into `[abs_start, body.len()]` so the range is always valid.
        let next_end = match self.entries.get(index + 1) {
            Some(&(_, next_off)) => self.first.saturating_add(next_off),
            None => self.body.len(),
        };
        let abs_end = next_end.clamp(abs_start, self.body.len());
        let slice = self
            .body
            .get(abs_start..abs_end)
            .ok_or_else(|| Error::xref(0, "objstm object slice out of range"))?;
        let mut parser = Parser::from_lexer(Lexer::new(slice));
        parser
            .parse_object()
            .map_err(|_| Error::xref(0, "malformed object inside object stream"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LimitKind;
    use crate::filters::flate;
    use crate::object::{Dict, Name, Object, StreamData, StreamObj};

    /// Builds a valid ObjStm `StreamObj` from a decoded body, declared `/N`, and
    /// `/First`. The body is Flate-encoded and wrapped with the standard
    /// `/Type /ObjStm` dict.
    fn objstm_stream(decoded: &[u8], n: i64, first: i64) -> StreamObj {
        let enc = flate::encode(decoded);
        let mut d = Dict::new();
        d.insert(Name::new("Type"), Object::Name(Name::new("ObjStm")));
        d.insert(Name::new("N"), Object::Integer(n));
        d.insert(Name::new("First"), Object::Integer(first));
        d.insert(Name::new("Filter"), Object::Name(Name::new("FlateDecode")));
        d.insert(Name::new("Length"), Object::Integer(enc.len() as i64));
        StreamObj::new_encoded(d, enc)
    }

    /// Builds an ObjStm from `(num, serialized-value)` members, computing the
    /// header and `/First` automatically.
    fn objstm_from_members(members: &[(u32, &[u8])]) -> StreamObj {
        let mut bodies = Vec::new();
        let mut offs = Vec::new();
        for (_num, body) in members {
            offs.push(bodies.len());
            bodies.extend_from_slice(body);
            bodies.push(b' ');
        }
        let mut header = String::new();
        for ((num, _), off) in members.iter().zip(&offs) {
            header.push_str(&format!("{num} {off} "));
        }
        let first = header.len() as i64;
        let mut decoded = header.into_bytes();
        decoded.extend_from_slice(&bodies);
        objstm_stream(&decoded, members.len() as i64, first)
    }

    #[test]
    fn objstm_decode_happy_two_members() {
        let stream = objstm_from_members(&[(7, b"42"), (9, b"<< /K 3 >>")]);
        let objstm = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap();

        assert_eq!(objstm.len(), 2);
        assert!(!objstm.is_empty());
        assert_eq!(objstm.object_number_at(0), Some(7));
        assert_eq!(objstm.object_number_at(1), Some(9));
        assert_eq!(objstm.object_number_at(2), None);
        assert_eq!(objstm.member_nums(), vec![7, 9]);

        assert_eq!(objstm.object_at(0).unwrap(), Object::Integer(42));
        let o1 = objstm.object_at(1).unwrap();
        let d = o1.as_dict().unwrap();
        assert_eq!(d.get(&Name::new("K")).unwrap(), &Object::Integer(3));
    }

    #[test]
    fn objstm_decode_empty_is_empty() {
        // /N 0, /First 0, empty body.
        let stream = objstm_stream(b"", 0, 0);
        let objstm = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap();
        assert_eq!(objstm.len(), 0);
        assert!(objstm.is_empty());
        assert!(objstm.member_nums().is_empty());
    }

    #[test]
    fn objstm_decode_missing_n_is_xref_err() {
        let enc = flate::encode(b"");
        let mut d = Dict::new();
        d.insert(Name::new("Type"), Object::Name(Name::new("ObjStm")));
        d.insert(Name::new("First"), Object::Integer(0));
        d.insert(Name::new("Length"), Object::Integer(enc.len() as i64));
        let stream = StreamObj::new_encoded(d, enc);

        let e = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn objstm_decode_missing_first_is_xref_err() {
        let enc = flate::encode(b"");
        let mut d = Dict::new();
        d.insert(Name::new("Type"), Object::Name(Name::new("ObjStm")));
        d.insert(Name::new("N"), Object::Integer(0));
        d.insert(Name::new("Length"), Object::Integer(enc.len() as i64));
        let stream = StreamObj::new_encoded(d, enc);

        let e = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn objstm_decode_n_exceeds_limit() {
        // Forge a large /N but keep the body benign; the cap trips first.
        let stream = objstm_stream(b"", 100, 0);
        let limits = Limits::unbounded_decode().with_max_objstm_objects(2);
        let e = ObjStm::decode(&stream, &limits).unwrap_err();
        assert!(
            matches!(e, Error::LimitExceeded(LimitKind::ObjstmObjects)),
            "{e:?}"
        );
    }

    #[test]
    fn objstm_decode_raw_body_not_materialized() {
        let mut d = Dict::new();
        d.insert(Name::new("Type"), Object::Name(Name::new("ObjStm")));
        d.insert(Name::new("N"), Object::Integer(0));
        d.insert(Name::new("First"), Object::Integer(0));
        let stream = StreamObj {
            dict: d,
            data: StreamData::Raw { offset: 0, len: 0 },
        };

        let e = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn objstm_decode_header_too_few_pairs() {
        // /N 2 but header carries only one `num off` pair, so the second header
        // pair parse fails.
        let decoded = b"1 0 ".to_vec();
        let stream = objstm_stream(&decoded, 2, decoded.len() as i64);
        let e = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn objstm_decode_object_at_index_out_of_range() {
        let stream = objstm_from_members(&[(7, b"42")]);
        let objstm = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap();
        let e = objstm.object_at(5).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn objstm_decode_object_at_offset_past_body() {
        // Header declares offset 999999 for the single member; with /First past
        // the header the absolute start exceeds the body length.
        let decoded = b"1 999999 X".to_vec();
        // `/First` = index where the member-body region begins (after the pair).
        let first = decoded.iter().position(|&b| b == b'X').unwrap() as i64;
        let stream = objstm_stream(&decoded, 1, first);
        let objstm = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap();
        let e = objstm.object_at(0).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }

    #[test]
    fn objstm_decode_malformed_object_inside() {
        // Valid header, but the member body slice is non-parseable.
        let stream = objstm_from_members(&[(7, b"@@@")]);
        let objstm = ObjStm::decode(&stream, &Limits::unbounded_decode()).unwrap();
        let e = objstm.object_at(0).unwrap_err();
        assert!(matches!(e, Error::Xref { .. }), "{e:?}");
    }
}
