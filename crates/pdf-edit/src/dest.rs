//! Destination + named-destination resolution (PRD §8.9).
//!
//! Resolves a `/Dest` value (explicit array, named string, or via a `/GoTo`
//! action) to a **physical page index** (0-based). Named destinations are looked
//! up first in the catalog `/Dests` dictionary, then in the `/Names /Dests`
//! name-tree (which may be a multi-level `/Kids`/`/Limits` structure).

use std::collections::HashMap;

use pdf_core::object::Name;
use pdf_core::{DocumentStore, ObjRef, Object};

/// A page-index lookup table built once per resolution batch: page-object number
/// → 0-based physical index (honoring the live, possibly-flattened page tree).
#[must_use]
pub fn page_index_map(doc: &DocumentStore) -> HashMap<u32, usize> {
    pdf_core::pagetree::page_refs(doc)
        .into_iter()
        .enumerate()
        .map(|(i, r)| (r.num, i))
        .collect()
}

/// Resolves a `/Dest` **value** (already resolved through any indirection) to a
/// 0-based page index. Handles the explicit array form `[pageref /XYZ …]`, an
/// integer page-number dest, and a named dest (name or string) via the catalog.
#[must_use]
pub fn dest_to_page(
    doc: &DocumentStore,
    dest: &Object,
    pages: &HashMap<u32, usize>,
) -> Option<usize> {
    match dest {
        Object::Array(items) => first_pageref_index(doc, items, pages),
        Object::Name(n) => resolve_named(doc, n.as_bytes(), pages),
        Object::String(s) => resolve_named(doc, s.as_bytes(), pages),
        Object::Reference(r) => {
            // A dest may itself be an indirect array/dict; resolve once.
            let resolved = doc.resolve(*r).ok()?;
            dest_to_page(doc, &resolved, pages)
        }
        Object::Dictionary(d) => {
            // Some dests are `<< /D [pageref …] >>` (PDF 2.0 structure dests).
            let inner = d.get(&Name::new("D"))?;
            dest_to_page(doc, inner, pages)
        }
        _ => None,
    }
}

/// Resolves a link's action/dest object (an annotation or outline value) to a
/// page index: `/Dest` value or `/A << /S /GoTo /D … >>`.
#[must_use]
pub fn resolve_link(
    doc: &DocumentStore,
    dict: &pdf_core::Dict,
    pages: &HashMap<u32, usize>,
) -> Option<usize> {
    if let Some(dest) = dict.get(&Name::new("Dest")) {
        let dest = deref(doc, dest);
        if let Some(p) = dest_to_page(doc, &dest, pages) {
            return Some(p);
        }
    }
    if let Some(a) = dict.get(&Name::new("A")) {
        let a = deref(doc, a);
        if let Some(adict) = a.as_dict() {
            if let Some(d) = adict.get(&Name::new("D")) {
                let d = deref(doc, d);
                return dest_to_page(doc, &d, pages);
            }
        }
    }
    None
}

/// Looks up a named destination (by raw name/string bytes) and returns its page
/// index. Checks the catalog `/Dests` dict first, then `/Names /Dests`.
#[must_use]
pub fn resolve_named(
    doc: &DocumentStore,
    name: &[u8],
    pages: &HashMap<u32, usize>,
) -> Option<usize> {
    let catalog = catalog_dict(doc)?;

    // 1. Catalog `/Dests` dictionary (PDF 1.1 form): keys are names.
    if let Some(dests) = catalog.get(&Name::new("Dests")) {
        let dests = deref(doc, dests);
        if let Some(dd) = dests.as_dict() {
            if let Some(v) = dd.get(&Name::new(std::str::from_utf8(name).unwrap_or(""))) {
                let v = deref(doc, v);
                if let Some(p) = dest_value_to_page(doc, &v, pages) {
                    return Some(p);
                }
            }
        }
    }

    // 2. `/Names /Dests` name-tree (PDF 1.2+): keys are strings.
    if let Some(names) = catalog.get(&Name::new("Names")) {
        let names = deref(doc, names);
        if let Some(nd) = names.as_dict() {
            if let Some(dests_tree) = nd.get(&Name::new("Dests")) {
                let root = deref(doc, dests_tree);
                if let Some(v) = name_tree_lookup(doc, &root, name, 0) {
                    return dest_value_to_page(doc, &v, pages);
                }
            }
        }
    }
    None
}

/// One resolved named destination (PyMuPDF `Document.resolve_names` value).
///
/// `to`/`zoom` are populated only for explicit `/XYZ` destinations; otherwise the
/// raw serialized destination is carried in `dest` (matching fitz, which only
/// fills `page`/`to`/`zoom` for `/XYZ` and falls back to a `dest` string).
#[derive(Debug, Clone, Default)]
pub struct ResolvedName {
    /// Target page index (0-based), or `-1` when no page could be resolved.
    pub page: i64,
    /// Target point `(x, y)` in PDF coordinates, when the dest is `/XYZ`.
    pub to: Option<(f64, f64)>,
    /// Zoom factor, when the dest is `/XYZ`.
    pub zoom: Option<f64>,
    /// The raw serialized destination, when not an `/XYZ` (or page unresolved).
    pub dest: Option<String>,
}

/// Resolves **every** named destination in the catalog to a [`ResolvedName`]
/// (PyMuPDF `Document.resolve_names`): the `/Dests` dictionary plus the
/// `/Names /Dests` name-tree, keyed by destination name.
#[must_use]
pub fn resolve_names(doc: &DocumentStore) -> Vec<(String, ResolvedName)> {
    let pages = page_index_map(doc);
    let mut out: Vec<(String, ResolvedName)> = Vec::new();
    let Some(catalog) = catalog_dict(doc) else {
        return out;
    };

    // 1. Catalog `/Dests` dictionary (PDF 1.1 form): keys are names.
    if let Some(dests) = catalog.get(&Name::new("Dests")) {
        let dests = deref(doc, dests);
        if let Some(dd) = dests.as_dict() {
            for (k, v) in dd {
                let name = String::from_utf8_lossy(k.as_bytes()).into_owned();
                let v = deref(doc, v);
                out.push((name, resolve_one_name(doc, &v, &pages)));
            }
        }
    }

    // 2. `/Names /Dests` name-tree (PDF 1.2+): keys are strings.
    if let Some(names) = catalog.get(&Name::new("Names")) {
        let names = deref(doc, names);
        if let Some(nd) = names.as_dict() {
            if let Some(dests_tree) = nd.get(&Name::new("Dests")) {
                let root = deref(doc, dests_tree);
                name_tree_collect(doc, &root, &pages, 0, &mut out);
            }
        }
    }
    out
}

/// Walks a name-tree, pushing every `(key, resolved)` pair into `out`.
fn name_tree_collect(
    doc: &DocumentStore,
    node: &Object,
    pages: &HashMap<u32, usize>,
    depth: usize,
    out: &mut Vec<(String, ResolvedName)>,
) {
    if depth > 50 {
        return;
    }
    let Some(d) = node.as_dict() else {
        return;
    };
    if let Some(names) = d.get(&Name::new("Names")) {
        let names = deref(doc, names);
        if let Some(arr) = names.as_array() {
            let mut i = 0;
            while i + 1 < arr.len() {
                if let Some(k) = arr[i].as_string() {
                    let name = String::from_utf8_lossy(k.as_bytes()).into_owned();
                    let v = deref(doc, &arr[i + 1]);
                    out.push((name, resolve_one_name(doc, &v, pages)));
                }
                i += 2;
            }
        }
    }
    if let Some(kids) = d.get(&Name::new("Kids")) {
        let kids = deref(doc, kids);
        if let Some(arr) = kids.as_array() {
            for kid in arr {
                let kid = deref(doc, kid);
                name_tree_collect(doc, &kid, pages, depth + 1, out);
            }
        }
    }
}

/// Resolves a single named-dest value into a [`ResolvedName`], extracting the
/// `/XYZ` point + zoom when present (fitz's exact shape).
fn resolve_one_name(doc: &DocumentStore, v: &Object, pages: &HashMap<u32, usize>) -> ResolvedName {
    // Unwrap a `<< /D [...] >>` wrapper.
    let arr_obj = match v {
        Object::Dictionary(d) => d.get(&Name::new("D")).map(|inner| deref(doc, inner)),
        other => Some(other.clone()),
    };
    let page = dest_value_to_page(doc, v, pages)
        .and_then(|p| i64::try_from(p).ok())
        .unwrap_or(-1);

    if let Some(Object::Array(items)) = arr_obj.as_ref() {
        // [pageref /XYZ x y zoom] — extract the point + zoom for /XYZ.
        if items.len() >= 2 {
            if let Some(kind) = items[1].as_name() {
                if kind.as_bytes() == b"XYZ" {
                    let x = items.get(2).and_then(num_or_null).unwrap_or(0.0);
                    let y = items.get(3).and_then(num_or_null).unwrap_or(0.0);
                    let z = items.get(4).and_then(num_or_null).unwrap_or(0.0);
                    if page >= 0 {
                        return ResolvedName {
                            page,
                            to: Some((x, y)),
                            zoom: Some(z),
                            dest: None,
                        };
                    }
                }
            }
        }
    }

    // Non-/XYZ or unresolved page: carry the serialized dest like fitz, which
    // drops the leading page reference and keeps only the dest-type tail
    // (e.g. `/FitH 222`). For a non-array dest, serialize it whole.
    let dest_str = match arr_obj.as_ref() {
        Some(Object::Array(items)) if items.len() >= 2 => {
            let parts: Vec<String> = items[1..]
                .iter()
                .map(|o| {
                    String::from_utf8_lossy(&pdf_core::serialize::write_object(o)).into_owned()
                })
                .collect();
            Some(parts.join(" "))
        }
        Some(o) => {
            Some(String::from_utf8_lossy(&pdf_core::serialize::write_object(o)).into_owned())
        }
        None => None,
    };
    ResolvedName {
        page,
        to: None,
        zoom: None,
        dest: dest_str,
    }
}

/// A numeric dest coordinate, treating `null` (a "keep current" placeholder) as
/// `0.0` per fitz.
fn num_or_null(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(n) => Some(*n as f64),
        Object::Real(r) => Some(*r),
        Object::Null => Some(0.0),
        _ => None,
    }
}

/// A named-dest value may be `[pageref …]` directly or `<< /D [pageref …] >>`.
fn dest_value_to_page(
    doc: &DocumentStore,
    v: &Object,
    pages: &HashMap<u32, usize>,
) -> Option<usize> {
    match v {
        Object::Dictionary(d) => {
            let inner = d.get(&Name::new("D"))?;
            let inner = deref(doc, inner);
            dest_to_page(doc, &inner, pages)
        }
        other => dest_to_page(doc, other, pages),
    }
}

/// Walks a name-tree (`/Names` leaf pairs or `/Kids` + `/Limits` branches) for
/// `key`, returning the (resolved) value. Depth-guarded.
fn name_tree_lookup(
    doc: &DocumentStore,
    node: &Object,
    key: &[u8],
    depth: usize,
) -> Option<Object> {
    if depth > 50 {
        return None;
    }
    let d = node.as_dict()?;

    // Leaf: /Names is a flat [k1 v1 k2 v2 …] array, sorted by key.
    if let Some(names) = d.get(&Name::new("Names")) {
        let names = deref(doc, names);
        if let Some(arr) = names.as_array() {
            let mut i = 0;
            while i + 1 < arr.len() {
                if let Some(k) = arr[i].as_string() {
                    if k.as_bytes() == key {
                        return Some(deref(doc, &arr[i + 1]));
                    }
                }
                i += 2;
            }
        }
    }

    // Branch: /Kids, each with /Limits [lo hi]. Descend into the matching range.
    if let Some(kids) = d.get(&Name::new("Kids")) {
        let kids = deref(doc, kids);
        if let Some(arr) = kids.as_array() {
            for kid in arr {
                let kid = deref(doc, kid);
                if let Some(kd) = kid.as_dict() {
                    let in_range = match kd.get(&Name::new("Limits")) {
                        Some(lim) => {
                            let lim = deref(doc, lim);
                            limits_contain(&lim, key)
                        }
                        None => true,
                    };
                    if in_range {
                        if let Some(v) = name_tree_lookup(doc, &kid, key, depth + 1) {
                            return Some(v);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Whether `key` falls within the inclusive `[lo, hi]` string limits (byte-wise,
/// matching PDF name-tree ordering).
fn limits_contain(limits: &Object, key: &[u8]) -> bool {
    let Some(arr) = limits.as_array() else {
        return true;
    };
    if arr.len() != 2 {
        return true;
    }
    let lo = arr[0].as_string().map(|s| s.as_bytes());
    let hi = arr[1].as_string().map(|s| s.as_bytes());
    match (lo, hi) {
        (Some(lo), Some(hi)) => key >= lo && key <= hi,
        _ => true,
    }
}

/// Finds the first `Object::Reference` in `items` that is a known page object,
/// returning its physical index.
fn first_pageref_index(
    doc: &DocumentStore,
    items: &[Object],
    pages: &HashMap<u32, usize>,
) -> Option<usize> {
    for it in items {
        if let Object::Reference(r) = it {
            if let Some(&idx) = pages.get(&r.num) {
                return Some(idx);
            }
        }
        if let Object::Integer(n) = it {
            // A bare integer dest is a 0-based page number.
            let idx = usize::try_from(*n).ok()?;
            if idx < pages.len() {
                return Some(idx);
            }
        }
    }
    let _ = doc;
    None
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
fn catalog_dict(doc: &DocumentStore) -> Option<pdf_core::Dict> {
    let root: ObjRef = doc.root()?;
    doc.resolve(root).ok()?.as_dict().cloned()
}
