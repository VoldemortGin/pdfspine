//! Shared self-built-PDF fixture builders for the M1c xref / document tests.
//!
//! Every fixture is assembled **in-test** from the M1a serializer plus a
//! hand-written cross-reference structure (PRD §10: no external/PyMuPDF files).
//! The builders track byte offsets precisely so the emitted `xref` table /
//! `/XRef` stream points at the right `N G obj` headers.

#![allow(dead_code)] // each test file uses a subset of the helpers

use pdf_core::filters::{flate, predictor};
use pdf_core::object::parse::Parser;
use pdf_core::serialize::write_indirect;
use pdf_core::{Dict, Name, ObjRef, Object, StreamObj};

/// Builds a `%PDF-<v>\n` header followed by a binary-marker comment line.
fn header_bytes(v: &str) -> Vec<u8> {
    let mut h = format!("%PDF-{v}\n%").into_bytes();
    h.extend_from_slice(&[0xE2, 0xE3, 0xCF, 0xD3, b'\n']);
    h
}

/// Convenience: a `Name`.
pub fn n(s: &str) -> Name {
    Name::new(s)
}

/// Convenience: a `/Name` object.
pub fn name_obj(s: &str) -> Object {
    Object::Name(Name::new(s))
}

/// Convenience: an indirect reference object.
pub fn rref(num: u32, gen: u16) -> Object {
    Object::Reference(ObjRef::new(num, gen))
}

/// Builds a `Dict` from `(key, value)` pairs.
pub fn dict(pairs: impl IntoIterator<Item = (&'static str, Object)>) -> Dict {
    let mut d = Dict::new();
    for (k, v) in pairs {
        d.insert(Name::new(k), v);
    }
    d
}

/// A classic-xref PDF builder.
///
/// Add objects with [`Pdf::obj`], then call [`Pdf::build`] to emit a complete
/// file with a `%PDF-` header, body, a single-subsection classic `xref` table
/// (object 0 free), a `trailer` and `startxref`/`%%EOF`.
pub struct Pdf {
    header: Vec<u8>,
    /// `(num, gen, serialized-bytes)` in emission order.
    objects: Vec<(u32, u16, Vec<u8>)>,
    /// Extra trailer keys beyond `/Size` and `/Root`.
    extra_trailer: Dict,
    root: Option<ObjRef>,
    /// Bytes of junk to prepend before the header (for `header_offset` tests).
    prefix: Vec<u8>,
}

impl Pdf {
    /// A new builder with a `%PDF-1.7` header.
    pub fn new() -> Self {
        Pdf {
            header: header_bytes("1.7"),
            objects: Vec::new(),
            extra_trailer: Dict::new(),
            root: None,
            prefix: Vec::new(),
        }
    }

    /// Overrides the header version string (e.g. `"1.4"`, `"2.0"`).
    pub fn version(mut self, v: &str) -> Self {
        self.header = header_bytes(v);
        self
    }

    /// Prepends `junk` before the header (to exercise `header_offset`).
    pub fn prefix(mut self, junk: &[u8]) -> Self {
        self.prefix = junk.to_vec();
        self
    }

    /// Records an indirect object `num gen obj … endobj`.
    pub fn obj(mut self, num: u32, gen: u16, obj: Object) -> Self {
        let bytes = write_indirect(ObjRef::new(num, gen), &obj);
        self.objects.push((num, gen, bytes));
        self
    }

    /// Sets the trailer `/Root` catalog reference.
    pub fn root(mut self, num: u32, gen: u16) -> Self {
        self.root = Some(ObjRef::new(num, gen));
        self
    }

    /// Adds an arbitrary trailer key.
    pub fn trailer_key(mut self, key: &'static str, val: Object) -> Self {
        self.extra_trailer.insert(Name::new(key), val);
        self
    }

    /// Emits the complete PDF bytes with a classic xref table.
    pub fn build(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.prefix);
        // header_offset bias = prefix length.
        let bias = self.prefix.len();
        out.extend_from_slice(&self.header);

        // Emit objects, recording each header's offset (relative to file start)
        // and biasing back by `bias` for the *stored* (header-relative) offset.
        let mut max_num = 0u32;
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, _gen, bytes) in &self.objects {
            let abs = out.len();
            offsets.push((*num, abs - bias));
            out.extend_from_slice(bytes);
            max_num = max_num.max(*num);
        }

        // Classic xref at this position (stored value is also bias-relative).
        let startxref = out.len() - bias;
        let size = max_num + 1;

        out.extend_from_slice(b"xref\n");
        out.extend_from_slice(format!("0 {size}\n").as_bytes());
        // Object 0: free-list head.
        out.extend_from_slice(b"0000000000 65535 f \n");
        // Build a num→offset map for 1..size.
        let mut map = std::collections::HashMap::new();
        for (num, off) in &offsets {
            map.insert(*num, *off);
        }
        for num in 1..size {
            if let Some(off) = map.get(&num) {
                out.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
            } else {
                out.extend_from_slice(b"0000000000 65535 f \n");
            }
        }

        // Trailer.
        let mut trailer = self.extra_trailer.clone();
        trailer.insert(Name::new("Size"), Object::Integer(i64::from(size)));
        if let Some(r) = self.root {
            trailer.insert(Name::new("Root"), Object::Reference(r));
        }
        out.extend_from_slice(b"trailer\n");
        out.extend_from_slice(&pdf_core::serialize::write_object(&Object::Dictionary(
            trailer,
        )));
        out.extend_from_slice(b"\nstartxref\n");
        out.extend_from_slice(format!("{startxref}\n").as_bytes());
        out.extend_from_slice(b"%%EOF\n");
        out
    }
}

impl Default for Pdf {
    fn default() -> Self {
        Pdf::new()
    }
}

/// A low-level byte-assembly builder for fixtures that need exact control over
/// the cross-reference layout (xref streams, `/Prev` chains, hybrid files).
///
/// Append content with [`RawPdf::header`], [`RawPdf::push_object`] (records the
/// header offset), [`RawPdf::raw`] (verbatim bytes), and read the current
/// length with [`RawPdf::pos`]. The caller writes the xref + trailer + startxref
/// itself, using the recorded offsets.
pub struct RawPdf {
    buf: Vec<u8>,
    /// num → recorded absolute offset of its `N G obj` header.
    offsets: std::collections::HashMap<u32, usize>,
}

impl RawPdf {
    /// A new, empty buffer.
    pub fn new() -> Self {
        RawPdf {
            buf: Vec::new(),
            offsets: std::collections::HashMap::new(),
        }
    }

    /// Appends a standard `%PDF-1.7` header + binary marker.
    pub fn header(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&header_bytes("1.7"));
        self
    }

    /// Current byte length (== offset of whatever is appended next).
    pub fn pos(&self) -> usize {
        self.buf.len()
    }

    /// Appends verbatim bytes.
    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    /// Appends an indirect object, recording its header offset for later xref
    /// emission. Returns the recorded offset.
    pub fn push_object(&mut self, num: u32, gen: u16, obj: &Object) -> usize {
        let off = self.buf.len();
        self.offsets.insert(num, off);
        let bytes = write_indirect(ObjRef::new(num, gen), obj);
        self.buf.extend_from_slice(&bytes);
        off
    }

    /// The recorded offset of object `num`.
    pub fn offset_of(&self, num: u32) -> usize {
        self.offsets[&num]
    }

    /// Finishes and returns the assembled bytes.
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    /// Appends a classic `xref` table covering `entries` (`num → (offset, gen,
    /// in_use)`), a `trailer` dict, `startxref <off>` and `%%EOF`. `xref_at` is
    /// the offset to record in `startxref`.
    pub fn classic_xref(
        &mut self,
        size: u32,
        entries: &[(u32, usize, u16, bool)],
        trailer: Dict,
    ) -> &mut Self {
        let xref_at = self.buf.len();
        let mut map = std::collections::HashMap::new();
        for &(num, off, gen, used) in entries {
            map.insert(num, (off, gen, used));
        }
        self.buf.extend_from_slice(b"xref\n");
        self.buf.extend_from_slice(format!("0 {size}\n").as_bytes());
        self.buf.extend_from_slice(b"0000000000 65535 f \n");
        for num in 1..size {
            match map.get(&num) {
                Some(&(off, gen, true)) => self
                    .buf
                    .extend_from_slice(format!("{off:010} {gen:05} n \n").as_bytes()),
                _ => self.buf.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        let mut t = trailer;
        t.insert(Name::new("Size"), Object::Integer(i64::from(size)));
        self.buf.extend_from_slice(b"trailer\n");
        self.buf
            .extend_from_slice(&pdf_core::serialize::write_object(&Object::Dictionary(t)));
        self.buf.extend_from_slice(b"\nstartxref\n");
        self.buf
            .extend_from_slice(format!("{xref_at}\n").as_bytes());
        self.buf.extend_from_slice(b"%%EOF\n");
        self
    }
}

impl Default for RawPdf {
    fn default() -> Self {
        RawPdf::new()
    }
}

/// Builds a `/Type /XRef` stream object from packed, Flate-(optionally
/// predictor-)encoded data plus the `/W`, `/Index`, `/Size`, `/Root` and extra
/// trailer keys. Returns the [`Object::Stream`] (caller assigns it a number).
pub fn xref_stream_object(
    data: &[u8],
    widths: [usize; 3],
    index: Option<Vec<i64>>,
    size: i64,
    extra: impl IntoIterator<Item = (&'static str, Object)>,
    predictor_columns: Option<usize>,
) -> Object {
    let payload = match predictor_columns {
        Some(cols) => flate::encode(&png_up_predict(data, cols)),
        None => flate::encode(data),
    };
    let mut d = dict(extra);
    d.insert(Name::new("Type"), name_obj("XRef"));
    d.insert(Name::new("Filter"), name_obj("FlateDecode"));
    d.insert(Name::new("Length"), Object::Integer(payload.len() as i64));
    d.insert(
        Name::new("W"),
        Object::Array(widths.iter().map(|&w| Object::Integer(w as i64)).collect()),
    );
    d.insert(Name::new("Size"), Object::Integer(size));
    if let Some(idx) = index {
        d.insert(
            Name::new("Index"),
            Object::Array(idx.into_iter().map(Object::Integer).collect()),
        );
    }
    if let Some(cols) = predictor_columns {
        d.insert(
            Name::new("DecodeParms"),
            Object::Dictionary(dict([
                ("Predictor", Object::Integer(12)),
                ("Columns", Object::Integer(cols as i64)),
            ])),
        );
    }
    Object::Stream(StreamObj::new_encoded(d, payload))
}

/// Builds an object-stream (`/Type /ObjStm`) object packing `members`
/// (`(num, serialized-value-bytes)`), Flate-compressed. Returns the
/// [`Object::Stream`].
pub fn objstm_object(members: &[(u32, Vec<u8>)]) -> Object {
    // Header: "num off num off …" then concatenated bodies.
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
    let first = header.len();
    let mut decoded = header.into_bytes();
    decoded.extend_from_slice(&bodies);

    let enc = flate::encode(&decoded);
    let mut d = dict([
        ("Type", name_obj("ObjStm")),
        ("N", Object::Integer(members.len() as i64)),
        ("First", Object::Integer(first as i64)),
        ("Filter", name_obj("FlateDecode")),
        ("Length", Object::Integer(enc.len() as i64)),
    ]);
    let _ = &mut d;
    Object::Stream(StreamObj::new_encoded(d, enc))
}

/// Serializes a bare object value (no indirect wrapper) — for ObjStm members.
pub fn write_value(obj: &Object) -> Vec<u8> {
    pdf_core::serialize::write_object(obj)
}

/// Flate-encodes `data` (M1b `flate::encode`).
pub fn flate_encode(data: &[u8]) -> Vec<u8> {
    flate::encode(data)
}

/// Builds a Flate-compressed stream object (dict gets `/Filter /FlateDecode` and
/// a correct `/Length`).
pub fn flate_stream(
    extra: impl IntoIterator<Item = (&'static str, Object)>,
    body: &[u8],
) -> Object {
    let enc = flate::encode(body);
    let mut d = dict(extra);
    d.insert(Name::new("Filter"), name_obj("FlateDecode"));
    d.insert(Name::new("Length"), Object::Integer(enc.len() as i64));
    Object::Stream(StreamObj::new_encoded(d, enc))
}

/// Encodes a sequence of `(f1,f2,f3)` xref records into packed big-endian bytes
/// using the given `/W` widths.
pub fn pack_xref_records(records: &[(u64, u64, u64)], widths: [usize; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(f1, f2, f3) in records {
        for (val, w) in [(f1, widths[0]), (f2, widths[1]), (f3, widths[2])] {
            let bytes = val.to_be_bytes();
            // Take the low `w` bytes (big-endian).
            out.extend_from_slice(&bytes[8 - w..]);
        }
    }
    out
}

/// PNG-Up predictor encode (M1b `predictor::predict`) for an xref stream's data,
/// using the `/Predictor 12` configuration with `columns` = `sum(W)`.
pub fn png_up_predict(rows: &[u8], columns: usize) -> Vec<u8> {
    let parms = dict([
        ("Predictor", Object::Integer(12)),
        ("Columns", Object::Integer(columns as i64)),
    ]);
    let p = predictor::PredictorParams::from_parms(Some(&parms), "FlateDecode")
        .unwrap()
        .unwrap();
    predictor::predict(rows, &p).unwrap()
}

/// Parses a single indirect object from `bytes` (test convenience).
pub fn parse_one_indirect(bytes: &[u8]) -> (ObjRef, Object) {
    let mut p = Parser::new(bytes);
    p.parse_indirect_object().unwrap()
}

// --- M1d malformed-fixture corruptors (self-built; PRD §10) ---------------
//
// Each takes a *valid* PDF (from `Pdf::build`) and corrupts it in one specific
// way so the repair subsystem must recover it. Offsets are recomputed against
// the byte string so the corruption is realistic.

/// Removes the `startxref <n>` line (and its number), leaving the body + xref
/// table but no pointer to it — the "missing startxref" case (REPAIR-XREF-001).
pub fn corrupt_remove_startxref(bytes: &[u8]) -> Vec<u8> {
    let needle = b"startxref";
    if let Some(pos) = find_last(bytes, needle) {
        bytes[..pos].to_vec()
    } else {
        bytes.to_vec()
    }
}

/// Replaces the `startxref` offset digits with a wildly out-of-range value
/// (REPAIR-XREF-002).
pub fn corrupt_garbage_startxref(bytes: &[u8]) -> Vec<u8> {
    let needle = b"startxref";
    let mut out = bytes.to_vec();
    if let Some(pos) = find_last(&out, needle) {
        // Find the digits after `startxref` (skipping whitespace) and overwrite.
        let mut p = pos + needle.len();
        while p < out.len() && (out[p] == b'\n' || out[p] == b'\r' || out[p] == b' ') {
            p += 1;
        }
        let start = p;
        while p < out.len() && out[p].is_ascii_digit() {
            p += 1;
        }
        // Replace the digit run with a bogus, in-bounds-but-wrong offset of the
        // same-or-different length.
        let replacement = b"999999999";
        out.splice(start..p, replacement.iter().copied());
    }
    out
}

/// Removes the whole classic-xref table + trailer + startxref (everything from
/// the `xref` keyword onward), simulating a body-only / no-trailer file
/// (REPAIR-TRAILER-001).
pub fn corrupt_remove_xref_and_trailer(bytes: &[u8]) -> Vec<u8> {
    if let Some(pos) = find_last(bytes, b"\nxref") {
        bytes[..pos + 1].to_vec()
    } else if let Some(pos) = find_last(bytes, b"xref") {
        bytes[..pos].to_vec()
    } else {
        bytes.to_vec()
    }
}

/// Truncates the file to `keep` bytes (REPAIR-TRUNC-*).
pub fn corrupt_truncate(bytes: &[u8], keep: usize) -> Vec<u8> {
    bytes[..keep.min(bytes.len())].to_vec()
}

/// Returns the byte offset just before the classic-xref table (i.e. the end of
/// the object body), so tests can truncate "after the objects".
pub fn body_end_offset(bytes: &[u8]) -> usize {
    find_last(bytes, b"\nxref")
        .map(|p| p + 1)
        .unwrap_or(bytes.len())
}

/// Finds the last occurrence of `needle` in `hay`.
pub fn find_last(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// Finds the first occurrence of `needle` in `hay`.
pub fn find_first(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
