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

/// How [`crate::DocumentStore::save_incremental`] reacts to a repair-tainted
/// parse (PRD §8.7). Incremental save is valid only on a clean parse.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum OnRepaired {
    /// Reject with [`crate::Error::IncrementalRequiresCleanParse`] (the default —
    /// chosen so a signature-preservation expectation is never silently broken,
    /// PRD §8.7).
    #[default]
    Reject,
    /// Silently fall back to a **full** save (no append, fresh `%PDF-` header).
    Upgrade,
}

/// Options controlling a save (PRD §8.7 `SaveOptions`).
#[derive(Clone, Debug)]
pub struct SaveOptions {
    /// The cross-reference form to emit.
    pub xref_style: XrefStyle,
    /// Whether to Flate-deflate non-image, non-already-filtered stream bodies.
    pub deflate: bool,
    /// Garbage-collection level for a **full** save, PyMuPDF `garbage` (PRD §8.7):
    /// `0` none, `1` mark-sweep, `2` +renumber, `3` +dedup objects, `4` +dedup
    /// streams. Ignored by incremental save (which appends only changed objects).
    pub garbage: u8,
    /// How incremental save reacts to a repair-tainted parse (PRD §8.7).
    pub on_repaired: OnRepaired,
}

impl Default for SaveOptions {
    /// `Table` xref, no deflation, no garbage collection, reject-on-repaired —
    /// the deterministic, round-trip baseline used by the M3a determinism tests
    /// (PRD §8.7).
    fn default() -> Self {
        SaveOptions {
            xref_style: XrefStyle::Table,
            deflate: false,
            garbage: 0,
            on_repaired: OnRepaired::Reject,
        }
    }
}

impl SaveOptions {
    /// The deterministic baseline (`Table`, no deflate, no GC).
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

    /// Sets the garbage-collection level (clamped to `0..=4`).
    #[must_use]
    pub fn with_garbage(mut self, level: u8) -> Self {
        self.garbage = level.min(4);
        self
    }

    /// Sets the repaired-document policy for incremental save.
    #[must_use]
    pub fn with_on_repaired(mut self, on_repaired: OnRepaired) -> Self {
        self.on_repaired = on_repaired;
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
    // 1. Compute the effective in-use object set as a materialized snapshot
    //    (object number → value, stream bodies owned) and the trailer roots.
    let mut objects = collect_effective_objects(doc)?;
    let root_num = doc
        .root()
        .map(|r| r.num)
        .ok_or_else(|| crate::Error::xref(0, "cannot save: trailer has no /Root"))?;
    let mut roots = crate::gc::Roots {
        root: Some(root_num),
        info: trailer_ref(doc, "Info").map(|r| r.num),
        encrypt: trailer_ref(doc, "Encrypt").map(|r| r.num),
    };

    // 2. Garbage collection on the snapshot (PRD §8.7). The live `DocumentStore`
    //    is never touched, so a dedup merge is save-time only (COW by design).
    if opts.garbage >= 1 {
        let result = crate::gc::collect(objects, roots, opts.garbage);
        objects = result.objects;
        roots = result.roots;
    }

    // 3. Serialize each surviving object's body bytes (deflate policy applied).
    let bodies = serialize_objects(&objects, opts)?;

    // 4. Lay out the body: header + each object, recording offsets.
    let mut out = Vec::new();
    write_header(&mut out, doc);

    let max_num = bodies.keys().copied().max().unwrap_or(0);
    let size = max_num + 1;

    let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
    for (&num, body) in &bodies {
        let off = out.len();
        offsets.insert(num, off);
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    // 5. Trailer keys (roots possibly remapped by GC) + `/ID`.
    let root = ObjRef::new(roots.root.unwrap_or(root_num), 0);
    let info = roots.info.map(|n| ObjRef::new(n, 0));
    let encrypt = roots.encrypt.map(|n| ObjRef::new(n, 0));
    let id_first = stable_first_id(doc);
    let id_second = hash16(&out);

    // 6. Emit the cross-reference structure + trailer + startxref + %%EOF.
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

/// Serializes an incremental update: **appends** to the document's original
/// source bytes only the changed objects (added / updated) plus deletion-marker
/// free entries, then a new cross-reference section whose `/Prev` points at the
/// document's prior `startxref`, a new trailer, `startxref` and `%%EOF`
/// (PRD §8.7).
///
/// Byte-exactness invariant: `out[..orig.len()] == orig` holds by construction —
/// `out` *starts* as a copy of the original source and is only ever appended to.
///
/// # Errors
///
/// [`crate::Error::IncrementalRequiresCleanParse`] when the parse was
/// repair-tainted and `opts.on_repaired == OnRepaired::Reject`; with
/// `OnRepaired::Upgrade` a full [`save_to_vec`] is returned instead.
/// [`crate::Error::Xref`] if the document has no `/Root`.
pub fn save_incremental(doc: &DocumentStore, opts: &SaveOptions) -> crate::Result<Vec<u8>> {
    if doc.parse_was_repaired() {
        match opts.on_repaired {
            OnRepaired::Reject => return Err(crate::Error::IncrementalRequiresCleanParse),
            OnRepaired::Upgrade => return save_to_vec(doc, opts),
        }
    }

    // Start from a byte-exact copy of the original — the only way to guarantee
    // `out[..orig.len()] == orig` and keep any signature byte range intact.
    let orig = doc.source().bytes();
    let mut out = orig.to_vec();
    let orig_len = out.len();

    // The previous cross-reference offset becomes the new section's `/Prev`.
    let prev = crate::xref::find_startxref(doc.source())?;

    // Enumerate the change set: `Set` → an appended body; `Deleted` → a free
    // entry in the new section (no body).
    let mut new_offsets: BTreeMap<u32, usize> = BTreeMap::new();
    let mut freed: Vec<u32> = Vec::new();

    // A leading separator so an object header never abuts the prior `%%EOF`.
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    for (num, change) in doc.changes_snapshot() {
        match change {
            Change::Set(obj) => {
                let body = serialize_body(doc, obj.as_ref(), opts)?;
                let off = out.len();
                new_offsets.insert(num, off);
                out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
                out.extend_from_slice(&body);
                out.extend_from_slice(b"\nendobj\n");
            }
            Change::Deleted => freed.push(num),
        }
    }

    // `/Size` is one past the highest object number now in play.
    let prev_size = doc.xref_length();
    let max_changed = new_offsets
        .keys()
        .chain(freed.iter())
        .copied()
        .max()
        .unwrap_or(0);
    let size = prev_size.max(max_changed + 1);

    let root = doc
        .root()
        .ok_or_else(|| crate::Error::xref(0, "cannot save: trailer has no /Root"))?;
    let info = trailer_ref(doc, "Info");
    let encrypt = trailer_ref(doc, "Encrypt");
    let id_first = stable_first_id(doc);
    let id_second = hash16(&out);

    let keys = TrailerKeys {
        size,
        root,
        info,
        encrypt,
        id_first,
        id_second,
    };
    match opts.xref_style {
        XrefStyle::Table => write_incremental_table(&mut out, &new_offsets, &freed, &keys, prev),
        XrefStyle::Stream => write_incremental_stream(&mut out, &new_offsets, &freed, &keys, prev),
    }

    debug_assert_eq!(&out[..orig_len], orig, "incremental prefix byte-exact");
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

/// Collects the effective in-use object set as a **materialized snapshot**:
/// every original object number with a usable xref entry, overlaid by the change
/// set (created/updated win, deleted removed). Stream bodies are materialized to
/// owned [`StreamData::Encoded`] payloads so the snapshot is self-contained
/// (needed by GC, which compares/renumbers off the live `Source`).
fn collect_effective_objects(doc: &DocumentStore) -> crate::Result<BTreeMap<u32, Object>> {
    let mut out: BTreeMap<u32, Object> = BTreeMap::new();

    // Original in-use objects (object number > 0; object 0 is the free head and
    // is never emitted as a body).
    for num in doc.xref().object_numbers() {
        if num == 0 {
            continue;
        }
        match doc.change_get(num) {
            Some(Change::Deleted) => continue,
            Some(Change::Set(obj)) => {
                out.insert(num, materialize_object(doc, obj.as_ref())?);
            }
            None => match doc.get_object(num, 0) {
                Ok(obj) => {
                    out.insert(num, materialize_object(doc, obj.as_ref())?);
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
            out.insert(num, materialize_object(doc, obj.as_ref())?);
        }
    }

    Ok(out)
}

/// Materializes one object for the save-time snapshot: a stream's source-backed
/// [`StreamData::Raw`] body is sliced into owned bytes; every other object is
/// cloned as-is.
fn materialize_object(doc: &DocumentStore, obj: &Object) -> crate::Result<Object> {
    match obj {
        Object::Stream(stream) => {
            let (dict, body, encoded) = materialize_stream_body(doc, stream)?;
            let data = if encoded {
                StreamData::Encoded(body.into())
            } else {
                StreamData::Decoded(body.into())
            };
            Ok(Object::Stream(StreamObj { dict, data }))
        }
        other => Ok(other.clone()),
    }
}

/// Serializes a snapshot's object map to body bytes (no `N G obj` wrapper),
/// applying the deflate policy per object.
fn serialize_objects(
    objects: &BTreeMap<u32, Object>,
    opts: &SaveOptions,
) -> crate::Result<BTreeMap<u32, Vec<u8>>> {
    let mut out = BTreeMap::new();
    for (&num, obj) in objects {
        out.insert(num, serialize_snapshot_body(obj, opts)?);
    }
    Ok(out)
}

/// Serializes a snapshot object's body. A stream's body is already owned (the
/// snapshot materialized it), so the deflate policy is applied directly.
fn serialize_snapshot_body(obj: &Object, opts: &SaveOptions) -> crate::Result<Vec<u8>> {
    match obj {
        Object::Stream(stream) => {
            let mut dict = stream.dict.clone();
            let already_encoded = matches!(stream.data, StreamData::Encoded(_));
            let mut body = match &stream.data {
                StreamData::Decoded(b) | StreamData::Encoded(b) => b.to_vec(),
                StreamData::Raw { .. } => Vec::new(), // never present post-materialize
            };
            if opts.deflate && !already_encoded {
                body = crate::filters::flate::encode(&body);
                set_flate_filter(&mut dict);
            }
            let mut out = write_stream_dict_with_length(&dict, body.len());
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&body);
            out.extend_from_slice(b"\nendstream");
            Ok(out)
        }
        other => Ok(write_object(other)),
    }
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

/// Emits an **incremental** classic `xref` table covering only the changed
/// objects (`new_offsets`) and freed objects (`freed`), plus a trailer carrying
/// `/Prev = prev` and `startxref`/`%%EOF` (PRD §8.7). The table is written in
/// minimal subsections: a fresh `0 1` free-head subsection followed by one
/// subsection per contiguous run of touched object numbers.
fn write_incremental_table(
    out: &mut Vec<u8>,
    new_offsets: &BTreeMap<u32, usize>,
    freed: &[u32],
    keys: &TrailerKeys,
    prev: usize,
) {
    let startxref = out.len();
    out.extend_from_slice(b"xref\n");

    // The object-0 free-list head is always present in a classic section.
    out.extend_from_slice(b"0 1\n");
    out.extend_from_slice(b"0000000000 65535 f \n");

    // Touched numbers (updated/added → in-use; deleted → free), sorted.
    let mut touched: Vec<u32> = new_offsets
        .keys()
        .copied()
        .chain(freed.iter().copied())
        .collect();
    touched.sort_unstable();
    touched.dedup();

    let freed_set: std::collections::HashSet<u32> = freed.iter().copied().collect();
    for run in contiguous_runs(&touched) {
        let first = run[0];
        out.extend_from_slice(format!("{first} {}\n", run.len()).as_bytes());
        for num in run {
            if let Some(&off) = new_offsets.get(&num) {
                out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
            } else {
                debug_assert!(freed_set.contains(&num));
                // Freed entry: gen bumped to 1, next-free 0 (single-revision free).
                out.extend_from_slice(b"0000000000 00001 f \n");
            }
        }
    }

    let mut trailer = build_trailer_dict(keys);
    trailer.insert(Name::new("Prev"), Object::Integer(prev as i64));
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(&write_object(&Object::Dictionary(trailer)));
    out.extend_from_slice(b"\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
}

/// Emits an **incremental** `/Type /XRef` cross-reference stream covering only
/// object 0, the changed objects, the freed objects and the xref stream object
/// itself, with `/Prev = prev` and an explicit `/Index` (PRD §8.7).
fn write_incremental_stream(
    out: &mut Vec<u8>,
    new_offsets: &BTreeMap<u32, usize>,
    freed: &[u32],
    keys: &TrailerKeys,
    prev: usize,
) {
    let xref_obj_num = keys.size;
    let xref_offset = out.len();
    let new_size = keys.size + 1;

    // The subset of object numbers this section declares: 0, every touched
    // number, and the xref stream object itself.
    let mut nums: Vec<u32> = vec![0];
    nums.extend(new_offsets.keys().copied());
    nums.extend(freed.iter().copied());
    nums.push(xref_obj_num);
    nums.sort_unstable();
    nums.dedup();

    let max_off = xref_offset.max(new_offsets.values().copied().max().unwrap_or(0));
    let off_w = byte_width(max_off as u64).max(1);
    let w = [1usize, off_w, 2usize];

    let freed_set: std::collections::HashSet<u32> = freed.iter().copied().collect();
    let mut records: Vec<u8> = Vec::with_capacity(nums.len() * (1 + off_w + 2));
    let mut index: Vec<i64> = Vec::new();
    for run in contiguous_runs(&nums) {
        index.push(i64::from(run[0]));
        index.push(run.len() as i64);
        for num in run {
            let (f1, f2, f3): (u64, u64, u64) = if num == 0 {
                (0, 0, 65535)
            } else if num == xref_obj_num {
                (1, xref_offset as u64, 0)
            } else if let Some(&off) = new_offsets.get(&num) {
                (1, off as u64, 0)
            } else {
                debug_assert!(freed_set.contains(&num));
                (0, 0, 0)
            };
            push_field(&mut records, f1, 1);
            push_field(&mut records, f2, off_w);
            push_field(&mut records, f3, 2);
        }
    }

    let mut dict = build_trailer_dict(&TrailerKeys {
        size: new_size,
        ..copy_keys(keys)
    });
    dict.insert(Name::new("Type"), Object::Name(Name::new("XRef")));
    dict.insert(
        Name::new("W"),
        Object::Array(w.iter().map(|&x| Object::Integer(x as i64)).collect()),
    );
    dict.insert(
        Name::new("Index"),
        Object::Array(index.into_iter().map(Object::Integer).collect()),
    );
    dict.insert(Name::new("Prev"), Object::Integer(prev as i64));

    out.extend_from_slice(format!("{xref_obj_num} 0 obj\n").as_bytes());
    out.extend_from_slice(&write_stream_dict_with_length(&dict, records.len()));
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(&records);
    out.extend_from_slice(b"\nendstream\nendobj\n");

    out.extend_from_slice(b"startxref\n");
    out.extend_from_slice(format!("{xref_offset}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
}

/// Splits a **sorted, deduped** number list into maximal contiguous runs.
fn contiguous_runs(nums: &[u32]) -> Vec<Vec<u32>> {
    let mut runs: Vec<Vec<u32>> = Vec::new();
    for &n in nums {
        match runs.last_mut() {
            Some(run) if *run.last().unwrap() + 1 == n => run.push(n),
            _ => runs.push(vec![n]),
        }
    }
    runs
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
