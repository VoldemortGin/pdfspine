//! Full-document serializer — open → edit → **save** to a fresh, valid PDF byte
//! stream (PRD §8.7 "Full save").
//!
//! The writer replays the **whole** effective object set (every original in-use
//! object, overlaid by the [`crate::changeset::ChangeSet`]) into a brand-new
//! file: a `%PDF-x.y` header + binary-comment line, each in-use object as
//! `N G obj … endobj`, then a cross-reference structure (classic **table** or
//! `/Type /XRef` **stream**, [`XrefStyle`]) and a trailer with `/Root`, `/Size`,
//! `/ID` and any carried-over `/Info`/`/Encrypt` references, ending in
//! `startxref` + `%%EOF`.
//!
//! This is **full** save only (M3a): incremental append (M3b), garbage
//! collection / renumber (M3b) and object-stream authoring (M3b) are out of
//! scope. Encryption-on-write is M3d — an `/Encrypt` reference is carried over
//! verbatim but bodies are written in clear here (callers that need a genuinely
//! re-encrypted file wait for M3d).
//!
//! # Stream deflate policy (PRD §8.7)
//!
//! With `deflate=true` the writer Flate-compresses any stream body that is not
//! already filtered: a body we hold *decoded* is compressed and gains
//! `/Filter /FlateDecode`; a body already carrying a filter (`/DCTDecode`,
//! `/FlateDecode`, …) is left byte-for-byte as-is. With `deflate=false` bodies
//! are written verbatim. Either way `/Length` is recomputed to match the bytes
//! actually emitted.
//!
//! # `/ID` scheme (PRD §8.7)
//!
//! The trailer `/ID` is a 2-element array of 16-byte strings. The **first**
//! element is *stable per document* — derived from the original `/ID` if present,
//! else a content hash of the original bytes — so successive saves of the same
//! source keep the same first id. The **second** element is *per save* — a
//! content hash of the freshly serialized body — so two saves differ in the
//! second id (and a save is reproducible given identical input + options).
//!
//! Everything here is **total**: malformed pending edits or missing objects
//! degrade to `null`, never a panic (PRD §9.6).

use std::collections::BTreeMap;

use crate::changeset::Change;
use crate::document::DocumentStore;
use crate::object::{Dict, Name, ObjRef, Object, PdfString, StreamData, StreamObj, StringKind};
use crate::serialize::{write_object, write_stream_dict_with_length};

/// The cross-reference form the writer emits (PRD §8.7).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum XrefStyle {
    /// A classic `xref` table + `trailer` dictionary (PDF ≤1.4 form, still valid
    /// in any version). The default.
    #[default]
    Table,
    /// A `/Type /XRef` cross-reference stream (PDF 1.5+).
    Stream,
}

/// Options controlling a full save (PRD §8.7 `SaveOptions`).
#[derive(Clone, Debug)]
pub struct SaveOptions {
    /// The cross-reference form to emit.
    pub xref_style: XrefStyle,
    /// Whether to Flate-deflate non-image, non-already-filtered stream bodies.
    pub deflate: bool,
}

impl Default for SaveOptions {
    /// `Table` xref, no deflation — the deterministic, round-trip baseline used
    /// by the M3a determinism tests (PRD §8.7).
    fn default() -> Self {
        SaveOptions {
            xref_style: XrefStyle::Table,
            deflate: false,
        }
    }
}

impl SaveOptions {
    /// The deterministic baseline (`Table`, no deflate).
    #[must_use]
    pub fn new() -> Self {
        SaveOptions::default()
    }

    /// Sets the cross-reference style.
    #[must_use]
    pub fn with_xref_style(mut self, style: XrefStyle) -> Self {
        self.xref_style = style;
        self
    }

    /// Enables/disables stream deflation.
    #[must_use]
    pub fn with_deflate(mut self, deflate: bool) -> Self {
        self.deflate = deflate;
        self
    }
}

/// The binary-comment line that follows the `%PDF-` header (4 high bytes so
/// transfer tools treat the file as binary — ISO 32000-1 §7.5.2).
const BINARY_MARKER: [u8; 4] = [0xE2, 0xE3, 0xCF, 0xD3];

/// Serializes the whole effective document (original live objects overlaid by
/// `doc`'s change set) to a fresh PDF byte stream per `opts` (PRD §8.7).
///
/// # Errors
///
/// [`crate::Error::Xref`] if the document has no `/Root` to save; resolution /
/// decode errors propagate only for objects the writer must materialize (a
/// missing original object degrades to skipped, not an error).
pub fn save_to_vec(doc: &DocumentStore, opts: &SaveOptions) -> crate::Result<Vec<u8>> {
    // 1. Compute the effective in-use object set: object number → body bytes.
    let effective = collect_effective(doc, opts)?;

    // 2. Lay out the body: header + each object, recording offsets.
    let mut out = Vec::new();
    write_header(&mut out, doc);

    // The xref needs `/Size` = max object number + 1.
    let max_num = effective.keys().copied().max().unwrap_or(0);
    let size = max_num + 1;

    // Object number → byte offset of its `N 0 obj` header.
    let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
    for (&num, body) in &effective {
        let off = out.len();
        offsets.insert(num, off);
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    // 3. Trailer keys carried over from the original (Root/Info/Encrypt).
    let root = doc
        .root()
        .ok_or_else(|| crate::Error::xref(0, "cannot save: trailer has no /Root"))?;
    let info = trailer_ref(doc, "Info");
    let encrypt = trailer_ref(doc, "Encrypt");

    // 4. `/ID`: first id stable per doc, second per save (content hash of body).
    let id_first = stable_first_id(doc);
    let id_second = hash16(&out);

    // 5. Emit the cross-reference structure + trailer + startxref + %%EOF.
    let keys = TrailerKeys {
        size,
        root,
        info,
        encrypt,
        id_first,
        id_second,
    };
    match opts.xref_style {
        XrefStyle::Table => write_xref_table(&mut out, &offsets, &keys),
        XrefStyle::Stream => write_xref_stream(&mut out, &offsets, &keys),
    }

    Ok(out)
}

/// The trailer/xref-stream-dict keys shared by both cross-reference forms.
struct TrailerKeys {
    size: u32,
    root: ObjRef,
    info: Option<ObjRef>,
    encrypt: Option<ObjRef>,
    id_first: [u8; 16],
    id_second: [u8; 16],
}

/// Collects the effective in-use object set: every original object number with a
/// usable xref entry, overlaid by the change set (created/updated win, deleted
/// removed). The value is the **already-serialized** object body bytes (without
/// the `N G obj` / `endobj` wrapper) so the layout pass only appends.
fn collect_effective(
    doc: &DocumentStore,
    opts: &SaveOptions,
) -> crate::Result<BTreeMap<u32, Vec<u8>>> {
    let mut out: BTreeMap<u32, Vec<u8>> = BTreeMap::new();

    // Original in-use objects (object number > 0; object 0 is the free head and
    // is never emitted as a body).
    for num in doc.xref().object_numbers() {
        if num == 0 {
            continue;
        }
        // A change-set entry for this number overrides the original entirely.
        match doc.change_get(num) {
            Some(Change::Deleted) => continue,
            Some(Change::Set(obj)) => {
                out.insert(num, serialize_body(doc, obj.as_ref(), opts)?);
            }
            None => match doc.get_object(num, 0) {
                Ok(obj) => {
                    out.insert(num, serialize_body(doc, obj.as_ref(), opts)?);
                }
                Err(crate::Error::MissingObject { .. }) => continue,
                Err(e) => return Err(e),
            },
        }
    }

    // Newly created objects (change-set numbers with no original xref entry).
    for (num, change) in doc.changes_snapshot() {
        if out.contains_key(&num) {
            continue;
        }
        if let Change::Set(obj) = change {
            out.insert(num, serialize_body(doc, obj.as_ref(), opts)?);
        }
    }

    Ok(out)
}

/// Serializes one object's **body** bytes (no indirect wrapper). For a stream the
/// body is materialized (Raw sliced from the source), the deflate policy applied,
/// and `/Length` recomputed; every other object uses the canonical
/// [`write_object`] serializer directly.
fn serialize_body(doc: &DocumentStore, obj: &Object, opts: &SaveOptions) -> crate::Result<Vec<u8>> {
    match obj {
        Object::Stream(stream) => serialize_stream(doc, stream, opts),
        other => Ok(write_object(other)),
    }
}

/// Serializes a stream object's body, applying the deflate policy (PRD §8.7).
fn serialize_stream(
    doc: &DocumentStore,
    stream: &StreamObj,
    opts: &SaveOptions,
) -> crate::Result<Vec<u8>> {
    let (mut dict, mut body, already_encoded) = materialize_stream_body(doc, stream)?;

    if opts.deflate && !already_encoded {
        // Plain (decoded) bytes with no filter → deflate and add
        // `/Filter /FlateDecode`. An already-encoded body is never re-compressed.
        body = crate::filters::flate::encode(&body);
        set_flate_filter(&mut dict);
    }

    let mut out = write_stream_dict_with_length(&dict, body.len());
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\nendstream");
    Ok(out)
}

/// Materializes a stream's body into owned bytes and reports whether those bytes
/// are still filter-encoded (so the deflate policy can skip them). Returns the
/// dict, the body bytes, and the `already_encoded` flag.
///
/// - A [`StreamData::Decoded`] payload is plain bytes → `already_encoded=false`.
/// - A [`StreamData::Encoded`] / [`StreamData::Raw`] payload is verbatim
///   file/filter bytes → `already_encoded=true` (the dict's `/Filter` describes
///   them).
fn materialize_stream_body(
    doc: &DocumentStore,
    stream: &StreamObj,
) -> crate::Result<(Dict, Vec<u8>, bool)> {
    let dict = stream.dict.clone();
    match &stream.data {
        StreamData::Decoded(b) => Ok((dict, b.to_vec(), false)),
        StreamData::Encoded(b) => Ok((dict, b.to_vec(), true)),
        StreamData::Raw { .. } => {
            let bytes = doc.stream_raw_bytes(stream)?;
            Ok((dict, bytes.to_vec(), true))
        }
    }
}

/// Adds `/Filter /FlateDecode` to a stream dict the writer just deflated. If the
/// dict already names a filter (it should not, since we only deflate unfiltered
/// bodies) the new filter is prepended so the chain stays correct.
fn set_flate_filter(dict: &mut Dict) {
    let key = Name::new("Filter");
    let flate = Object::Name(Name::new("FlateDecode"));
    match dict.get(&key).cloned() {
        None => {
            dict.insert(key, flate);
        }
        Some(Object::Name(existing)) => {
            dict.insert(key, Object::Array(vec![flate, Object::Name(existing)]));
        }
        Some(Object::Array(mut arr)) => {
            arr.insert(0, flate);
            dict.insert(key, Object::Array(arr));
        }
        Some(other) => {
            dict.insert(key, Object::Array(vec![flate, other]));
        }
    }
}

/// Writes the `%PDF-x.y` header and the binary-comment line.
fn write_header(out: &mut Vec<u8>, doc: &DocumentStore) {
    let v = doc.version();
    out.extend_from_slice(format!("%PDF-{}.{}\n", v.major, v.minor).as_bytes());
    out.push(b'%');
    out.extend_from_slice(&BINARY_MARKER);
    out.push(b'\n');
}

/// The trailer reference value for `key` (`/Info`, `/Encrypt`), if it is an
/// indirect reference in the original trailer.
fn trailer_ref(doc: &DocumentStore, key: &str) -> Option<ObjRef> {
    match doc.trailer().get(&Name::new(key)) {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    }
}

/// Builds the common trailer dictionary keys (`/Size`, `/Root`, optional
/// `/Info`/`/Encrypt`, `/ID`). Shared by the table and stream paths.
fn build_trailer_dict(keys: &TrailerKeys) -> Dict {
    let mut t = Dict::new();
    t.insert(Name::new("Size"), Object::Integer(i64::from(keys.size)));
    t.insert(Name::new("Root"), Object::Reference(keys.root));
    if let Some(i) = keys.info {
        t.insert(Name::new("Info"), Object::Reference(i));
    }
    if let Some(e) = keys.encrypt {
        t.insert(Name::new("Encrypt"), Object::Reference(e));
    }
    t.insert(
        Name::new("ID"),
        Object::Array(vec![
            Object::String(PdfString {
                bytes: keys.id_first.to_vec(),
                kind: StringKind::Hex,
            }),
            Object::String(PdfString {
                bytes: keys.id_second.to_vec(),
                kind: StringKind::Hex,
            }),
        ]),
    );
    t
}

/// Emits a classic `xref` table + `trailer` + `startxref` + `%%EOF` (PRD §8.7).
///
/// The table is a single subsection `0 size` with object 0 the free-list head
/// (`0000000000 65535 f`), each present object `<10-digit offset> 00000 n`, and
/// any gap (a deleted / never-present number) a free entry.
fn write_xref_table(out: &mut Vec<u8>, offsets: &BTreeMap<u32, usize>, keys: &TrailerKeys) {
    let startxref = out.len();
    let size = keys.size;
    out.extend_from_slice(b"xref\n");
    out.extend_from_slice(format!("0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..size {
        match offsets.get(&num) {
            Some(&off) => {
                out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
            }
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }

    let trailer = build_trailer_dict(keys);
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(&write_object(&Object::Dictionary(trailer)));
    out.extend_from_slice(b"\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
}

/// Emits a `/Type /XRef` cross-reference stream + `startxref` + `%%EOF`
/// (PRD §8.7). The stream is the last object (number `size`), so the written
/// `/Size` is `size + 1`. Records pack as `/W [1 off_w 2]`:
///
/// - type 0 (free): `0 0 65535` for object 0 and any gap.
/// - type 1 (uncompressed): `1 <offset> 0` for a present object and the xref
///   stream object itself.
fn write_xref_stream(out: &mut Vec<u8>, offsets: &BTreeMap<u32, usize>, keys: &TrailerKeys) {
    let xref_obj_num = keys.size;
    let xref_offset = out.len();
    let new_size = keys.size + 1;

    // Offset-field width: enough bytes for the largest offset (incl. this one).
    let max_off = xref_offset.max(offsets.values().copied().max().unwrap_or(0));
    let off_w = byte_width(max_off as u64).max(1);
    let w = [1usize, off_w, 2usize];

    let mut records: Vec<u8> = Vec::with_capacity(new_size as usize * (1 + off_w + 2));
    for num in 0..new_size {
        let (f1, f2, f3): (u64, u64, u64) = if num == 0 {
            (0, 0, 65535)
        } else if num == xref_obj_num {
            (1, xref_offset as u64, 0)
        } else if let Some(&off) = offsets.get(&num) {
            (1, off as u64, 0)
        } else {
            (0, 0, 65535)
        };
        push_field(&mut records, f1, 1);
        push_field(&mut records, f2, off_w);
        push_field(&mut records, f3, 2);
    }

    // The stream dict doubles as the trailer (PRD §8.2).
    let mut dict = build_trailer_dict(&TrailerKeys {
        size: new_size,
        ..copy_keys(keys)
    });
    dict.insert(Name::new("Type"), Object::Name(Name::new("XRef")));
    dict.insert(
        Name::new("W"),
        Object::Array(w.iter().map(|&x| Object::Integer(x as i64)).collect()),
    );
    // No `/Filter` (records written raw) and default `/Index [0 new_size]` (so
    // it is omitted) — the M1c reader handles both.

    out.extend_from_slice(format!("{xref_obj_num} 0 obj\n").as_bytes());
    out.extend_from_slice(&write_stream_dict_with_length(&dict, records.len()));
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(&records);
    out.extend_from_slice(b"\nendstream\nendobj\n");

    out.extend_from_slice(b"startxref\n");
    out.extend_from_slice(format!("{xref_offset}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
}

/// Shallow copy of [`TrailerKeys`] (it is not `Clone` because `ObjRef` is, but
/// keeping it explicit documents the per-field carry-over).
fn copy_keys(k: &TrailerKeys) -> TrailerKeys {
    TrailerKeys {
        size: k.size,
        root: k.root,
        info: k.info,
        encrypt: k.encrypt,
        id_first: k.id_first,
        id_second: k.id_second,
    }
}

/// Pushes the low `w` big-endian bytes of `val` into `out`.
fn push_field(out: &mut Vec<u8>, val: u64, w: usize) {
    let bytes = val.to_be_bytes();
    out.extend_from_slice(&bytes[8 - w..]);
}

/// The minimum number of bytes to hold `val` big-endian (≥1, for 0). Caps at 8.
fn byte_width(val: u64) -> usize {
    if val == 0 {
        return 1;
    }
    let bits = 64 - val.leading_zeros() as usize;
    bits.div_ceil(8)
}

/// The stable first `/ID` element (16 bytes): the original trailer `/ID[0]` when
/// present and 16 bytes long, else a content hash of the source bytes. Keeps the
/// first id constant across re-saves of the same document (PRD §8.7).
fn stable_first_id(doc: &DocumentStore) -> [u8; 16] {
    if let Some(Object::Array(arr)) = doc.trailer().get(&Name::new("ID")) {
        if let Some(Object::String(s)) = arr.first() {
            if s.bytes.len() == 16 {
                let mut id = [0u8; 16];
                id.copy_from_slice(&s.bytes);
                return id;
            }
        }
    }
    hash16(doc.source().bytes())
}

/// A deterministic 16-byte hash of `data` (two FNV-1a lanes → 128 bits). Used for
/// the `/ID` elements: same bytes → same id (reproducible), different bytes →
/// (almost surely) different id. Not cryptographic — `/ID` needs uniqueness, not
/// security (PRD §8.7).
fn hash16(data: &[u8]) -> [u8; 16] {
    const OFFSET_A: u64 = 0xcbf2_9ce4_8422_2325;
    const OFFSET_B: u64 = 0x1000_0000_0000_01B3;
    const PRIME: u64 = 0x0000_0100_0000_01B3;

    let mut a = OFFSET_A;
    let mut b = OFFSET_B;
    for (i, &byte) in data.iter().enumerate() {
        a ^= u64::from(byte);
        a = a.wrapping_mul(PRIME);
        b ^= u64::from(byte).wrapping_add(i as u64);
        b = b.wrapping_mul(PRIME);
    }
    a ^= data.len() as u64;
    a = a.wrapping_mul(PRIME);

    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&a.to_be_bytes());
    out[8..].copy_from_slice(&b.to_be_bytes());
    out
}
