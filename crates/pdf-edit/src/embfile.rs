//! Embedded files — the catalog `/Names /EmbeddedFiles` name-tree API (PRD §8.8).
//!
//! Implements the PyMuPDF-compatible `embfile_*` family over the document-level
//! embedded-file collection. Each entry is a name-tree key mapping to a
//! `/Filespec` dict whose `/EF /F` points at an `/EmbeddedFile` stream carrying
//! the raw bytes (byte-exact round trip).
//!
//! ## Name-tree handling
//!
//! Reads (names/count/get/info/del) walk the existing tree **generally**: a flat
//! leaf root (`/Names [k v …]`) or a `/Kids` + `/Limits` branch structure
//! (depth-guarded), matching whatever an input PDF already contains. The walker
//! mirrors the private helpers in [`crate::dest`].
//!
//! Writes (add/del) **collapse** the whole collection to a single flat sorted
//! root leaf: read all `(key, filespec-ref)` pairs, apply the mutation, sort
//! byte-wise, and rewrite `/EmbeddedFiles` as `<< /Names [ … ] >>`. A single
//! sorted leaf with no `/Limits` is always a legal name-tree, so this avoids the
//! complexity of in-place multi-level rebalancing while staying spec-correct.

use pdf_core::object::Name;
use pdf_core::{Dict, DocumentStore, ObjRef, Object, PdfString, Result, StreamObj, StringKind};

/// Metadata for one embedded file, mirroring the fields PyMuPDF reports from
/// `Document.embfile_info`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbfileInfo {
    /// The name-tree key under which the file is stored.
    pub name: String,
    /// The filespec `/F` filename.
    pub filename: String,
    /// The filespec `/UF` (unicode) filename.
    pub ufilename: String,
    /// The filespec `/Desc` description (empty when absent).
    pub desc: String,
    /// The decoded byte length (`/Params /Size`).
    pub size: usize,
    /// Alias for [`Self::size`] (PyMuPDF reports this as `length`).
    pub length: usize,
}

/// Adds an embedded file under name-tree key `name`.
///
/// `filename` defaults to `name` when `None`; `ufilename` defaults to the
/// effective `filename`. `desc` is written as `/Desc` only when provided. The
/// bytes are stored verbatim in an `/EmbeddedFile` stream and round-trip exactly.
///
/// # Errors
///
/// Returns [`pdf_core::Error::InvalidArgument`] if `name` already exists, or
/// propagates object-edit / resolve errors.
pub fn embfile_add(
    doc: &DocumentStore,
    name: &str,
    bytes: &[u8],
    filename: Option<&str>,
    ufilename: Option<&str>,
    desc: Option<&str>,
) -> Result<()> {
    let mut pairs = collect_pairs(doc);
    if pairs.iter().any(|(k, _)| k.as_slice() == name.as_bytes()) {
        return Err(pdf_core::Error::InvalidArgument(
            "embedded file name already exists",
        ));
    }

    let filename = filename.unwrap_or(name);
    let ufilename = ufilename.unwrap_or(filename);

    // EmbeddedFile stream: << /Type /EmbeddedFile /Length N /Params << /Size N >> >>.
    let mut ef_dict = Dict::new();
    ef_dict.insert(Name::new("Type"), Object::Name(Name::new("EmbeddedFile")));
    ef_dict.insert(Name::new("Length"), Object::Integer(bytes.len() as i64));
    let mut params = Dict::new();
    params.insert(Name::new("Size"), Object::Integer(bytes.len() as i64));
    ef_dict.insert(Name::new("Params"), Object::Dictionary(params));
    let ef_ref = doc.add_object(Object::Stream(StreamObj::new_encoded(
        ef_dict,
        bytes.to_vec(),
    )))?;

    // Filespec dict pointing at the EmbeddedFile stream via /EF /F and /UF.
    let mut fs = Dict::new();
    fs.insert(Name::new("Type"), Object::Name(Name::new("Filespec")));
    fs.insert(Name::new("F"), Object::String(text_string(filename)));
    fs.insert(Name::new("UF"), Object::String(text_string(ufilename)));
    if let Some(desc) = desc {
        fs.insert(Name::new("Desc"), Object::String(text_string(desc)));
    }
    let mut ef = Dict::new();
    ef.insert(Name::new("F"), Object::Reference(ef_ref));
    ef.insert(Name::new("UF"), Object::Reference(ef_ref));
    fs.insert(Name::new("EF"), Object::Dictionary(ef));
    let fs_ref = doc.add_object(Object::Dictionary(fs))?;

    pairs.push((name.as_bytes().to_vec(), fs_ref));
    write_tree(doc, pairs)
}

/// Reads back the bytes of the embedded file under `name` (byte-exact).
///
/// # Errors
///
/// Returns [`pdf_core::Error::InvalidArgument`] if no such file exists, or
/// propagates resolve / stream-decode errors.
pub fn embfile_get(doc: &DocumentStore, name: &str) -> Result<Vec<u8>> {
    let fs_ref = lookup_ref(doc, name)?;
    let fs = doc.resolve(fs_ref)?;
    let fs = fs
        .as_dict()
        .ok_or(pdf_core::Error::InvalidArgument("filespec is not a dict"))?;
    let ef_stream_ref = embeddedfile_ref(fs).ok_or(pdf_core::Error::InvalidArgument(
        "filespec has no /EF stream",
    ))?;
    let obj = doc.resolve(ef_stream_ref)?;
    let stream = obj.as_stream().ok_or(pdf_core::Error::InvalidArgument(
        "embedded file is not a stream",
    ))?;
    doc.decode_stream(stream)?.into_decoded()
}

/// Deletes the embedded file under `name` (best-effort orphan cleanup).
///
/// The name-tree key is always removed; the filespec and `/EmbeddedFile` stream
/// objects are deleted opportunistically (garbage collection handles any that
/// cannot be resolved or deleted here).
///
/// # Errors
///
/// Returns [`pdf_core::Error::InvalidArgument`] if no such file exists, or
/// propagates the tree-rewrite path's errors.
pub fn embfile_del(doc: &DocumentStore, name: &str) -> Result<()> {
    let mut pairs = collect_pairs(doc);
    let Some(pos) = pairs
        .iter()
        .position(|(k, _)| k.as_slice() == name.as_bytes())
    else {
        return Err(pdf_core::Error::InvalidArgument("no such embedded file"));
    };
    let (_, fs_ref) = pairs.remove(pos);

    // Best-effort: delete the EmbeddedFile stream(s), then the filespec itself.
    if let Ok(fs) = doc.resolve(fs_ref) {
        if let Some(fs) = fs.as_dict() {
            for r in embeddedfile_refs(fs) {
                let _ = doc.delete_object(r);
            }
        }
    }
    let _ = doc.delete_object(fs_ref);

    write_tree(doc, pairs)
}

/// All embedded-file names, sorted byte-wise (name-tree key order).
#[must_use]
pub fn embfile_names(doc: &DocumentStore) -> Vec<String> {
    let mut pairs = collect_pairs(doc);
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
        .into_iter()
        .map(|(k, _)| String::from_utf8_lossy(&k).into_owned())
        .collect()
}

/// The number of embedded files in the document.
#[must_use]
pub fn embfile_count(doc: &DocumentStore) -> usize {
    collect_pairs(doc).len()
}

/// Reports metadata for the embedded file under `name`.
///
/// # Errors
///
/// Returns [`pdf_core::Error::InvalidArgument`] if no such file exists, or
/// propagates resolve errors.
pub fn embfile_info(doc: &DocumentStore, name: &str) -> Result<EmbfileInfo> {
    let fs_ref = lookup_ref(doc, name)?;
    let fs = doc.resolve(fs_ref)?;
    let fs = fs
        .as_dict()
        .ok_or(pdf_core::Error::InvalidArgument("filespec is not a dict"))?;

    let filename = decode_text(doc, fs.get(&Name::new("F")));
    let ufilename = decode_text(doc, fs.get(&Name::new("UF")));
    let desc = decode_text(doc, fs.get(&Name::new("Desc")));

    // Size from /EF /F stream's /Params /Size, falling back to the decoded length.
    let size = embeddedfile_ref(fs)
        .and_then(|r| doc.resolve(r).ok())
        .and_then(|obj| {
            let stream = obj.as_stream()?;
            stream
                .dict
                .get(&Name::new("Params"))
                .and_then(Object::as_dict)
                .and_then(|p| p.get(&Name::new("Size")))
                .and_then(Object::as_i64)
                .and_then(|s| usize::try_from(s).ok())
                .or_else(|| {
                    doc.decode_stream(stream)
                        .and_then(|o| o.into_decoded())
                        .ok()
                        .map(|b| b.len())
                })
        })
        .unwrap_or(0);

    Ok(EmbfileInfo {
        name: name.to_string(),
        filename,
        ufilename,
        desc,
        size,
        length: size,
    })
}

// === Name-tree plumbing ===================================================

/// Resolves `name` to its filespec reference via the general tree walk.
fn lookup_ref(doc: &DocumentStore, name: &str) -> Result<ObjRef> {
    collect_pairs(doc)
        .into_iter()
        .find(|(k, _)| k.as_slice() == name.as_bytes())
        .map(|(_, r)| r)
        .ok_or(pdf_core::Error::InvalidArgument("no such embedded file"))
}

/// Collects every `(key, filespec-ref)` pair across all leaves, in tree order.
fn collect_pairs(doc: &DocumentStore) -> Vec<(Vec<u8>, ObjRef)> {
    let mut out = Vec::new();
    let Some(root) = embeddedfiles_root(doc) else {
        return out;
    };
    collect_node(doc, &root, 0, &mut out);
    out
}

/// The `/EmbeddedFiles` name-tree root object (resolved), if present.
fn embeddedfiles_root(doc: &DocumentStore) -> Option<Object> {
    let catalog = catalog_dict(doc)?;
    let names = deref(doc, catalog.get(&Name::new("Names"))?);
    let nd = names.as_dict()?;
    let tree = nd.get(&Name::new("EmbeddedFiles"))?;
    Some(deref(doc, tree))
}

/// Recursively gathers `(key, value-ref)` pairs from a leaf or branch node.
fn collect_node(
    doc: &DocumentStore,
    node: &Object,
    depth: usize,
    out: &mut Vec<(Vec<u8>, ObjRef)>,
) {
    if depth > 50 {
        return;
    }
    let Some(d) = node.as_dict() else {
        return;
    };

    // Leaf: /Names is a flat [k1 v1 k2 v2 …] array.
    if let Some(names) = d.get(&Name::new("Names")) {
        let names = deref(doc, names);
        if let Some(arr) = names.as_array() {
            let mut i = 0;
            while i + 1 < arr.len() {
                if let (Some(k), Some(r)) = (arr[i].as_string(), arr[i + 1].as_reference()) {
                    out.push((k.as_bytes().to_vec(), r));
                }
                i += 2;
            }
        }
    }

    // Branch: descend into every kid (collecting all keys, not a single lookup).
    if let Some(kids) = d.get(&Name::new("Kids")) {
        let kids = deref(doc, kids);
        if let Some(arr) = kids.as_array() {
            for kid in arr {
                let kid = deref(doc, kid);
                collect_node(doc, &kid, depth + 1, out);
            }
        }
    }
}

/// Rewrites `/EmbeddedFiles` as a single flat sorted leaf from `pairs`.
///
/// Creates the `/Names` dict and the leaf object as needed and wires them into
/// the catalog. When `/Names` / `/EmbeddedFiles` is itself indirect, the update
/// is applied through the existing reference.
fn write_tree(doc: &DocumentStore, mut pairs: Vec<(Vec<u8>, ObjRef)>) -> Result<()> {
    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut names_arr = Vec::with_capacity(pairs.len() * 2);
    for (k, r) in pairs {
        names_arr.push(Object::String(PdfString {
            bytes: k,
            kind: StringKind::Literal,
        }));
        names_arr.push(Object::Reference(r));
    }
    let mut leaf = Dict::new();
    leaf.insert(Name::new("Names"), Object::Array(names_arr));
    let leaf_obj = Object::Dictionary(leaf);

    let root = doc
        .root()
        .ok_or(pdf_core::Error::InvalidArgument("document has no /Root"))?;
    let mut catalog =
        doc.resolve(root)?
            .as_dict()
            .cloned()
            .ok_or(pdf_core::Error::InvalidArgument(
                "/Root is not a dictionary",
            ))?;

    // Resolve (or create) the /Names dict, preserving any other name-trees.
    let (names_ref, mut names_dict) = match catalog.get(&Name::new("Names")) {
        Some(Object::Reference(r)) => {
            let d = doc
                .resolve(*r)
                .ok()
                .and_then(|o| o.as_dict().cloned())
                .unwrap_or_default();
            (Some(*r), d)
        }
        Some(Object::Dictionary(d)) => (None, d.clone()),
        _ => (None, Dict::new()),
    };

    // Place the leaf as the /EmbeddedFiles value, reusing its ref if indirect.
    match names_dict.get(&Name::new("EmbeddedFiles")) {
        Some(Object::Reference(r)) => {
            let r = *r;
            doc.update_object(r, leaf_obj)?;
        }
        _ => {
            let leaf_ref = doc.add_object(leaf_obj)?;
            names_dict.insert(Name::new("EmbeddedFiles"), Object::Reference(leaf_ref));
        }
    }

    // Persist the /Names dict and ensure the catalog points at it.
    match names_ref {
        Some(r) => doc.update_object(r, Object::Dictionary(names_dict))?,
        None => {
            catalog.insert(Name::new("Names"), Object::Dictionary(names_dict));
            doc.update_object(root, Object::Dictionary(catalog))?;
        }
    }
    Ok(())
}

/// The `/EF /F` (fallback `/UF`) EmbeddedFile stream reference of a filespec.
fn embeddedfile_ref(fs: &Dict) -> Option<ObjRef> {
    let ef = fs.get(&Name::new("EF")).and_then(Object::as_dict)?;
    ef.get(&Name::new("F"))
        .and_then(Object::as_reference)
        .or_else(|| ef.get(&Name::new("UF")).and_then(Object::as_reference))
}

/// All distinct EmbeddedFile stream references reachable from a filespec's `/EF`.
fn embeddedfile_refs(fs: &Dict) -> Vec<ObjRef> {
    let mut out = Vec::new();
    if let Some(ef) = fs.get(&Name::new("EF")).and_then(Object::as_dict) {
        for key in ["F", "UF"] {
            if let Some(r) = ef.get(&Name::new(key)).and_then(Object::as_reference) {
                if !out.contains(&r) {
                    out.push(r);
                }
            }
        }
    }
    out
}

/// Decodes a (possibly indirect) text-string value to a Rust `String`.
fn decode_text(doc: &DocumentStore, value: Option<&Object>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let value = deref(doc, value);
    match value.as_string() {
        Some(s) => decode_pdf_text(s.as_bytes()),
        None => String::new(),
    }
}

/// Decodes PDF text-string bytes: UTF-16BE with a BOM, else lossy UTF-8/Latin-1.
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Encodes a text string: ASCII → literal; otherwise UTF-16BE with a `FE FF` BOM.
fn text_string(s: &str) -> PdfString {
    if s.is_ascii() {
        PdfString {
            bytes: s.as_bytes().to_vec(),
            kind: StringKind::Literal,
        }
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        PdfString {
            bytes,
            kind: StringKind::Hex,
        }
    }
}

/// Resolves one level of indirection, returning a cloned owned object.
fn deref(doc: &DocumentStore, obj: &Object) -> Object {
    match obj {
        Object::Reference(r) => doc
            .resolve(*r)
            .map(|a| (*a).clone())
            .unwrap_or(Object::Null),
        other => other.clone(),
    }
}

/// The catalog dictionary, if resolvable.
fn catalog_dict(doc: &DocumentStore) -> Option<Dict> {
    let root: ObjRef = doc.root()?;
    doc.resolve(root).ok()?.as_dict().cloned()
}
