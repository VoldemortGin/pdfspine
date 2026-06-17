//! Page-tree traversal with inherited attributes and a broken-tree fallback
//! (PRD §7 "Page tree + inheritance", §8.2 step 3).
//!
//! The catalog `/Pages` is the root of a tree of intermediate `/Pages` nodes
//! (each with `/Kids` and `/Count`) and `/Page` leaves. Four attributes are
//! **inheritable** — `/Resources`, `/MediaBox`, `/CropBox`, `/Rotate` — meaning a
//! leaf that omits them takes the nearest ancestor's value (ISO 32000-1 §7.7.3.4,
//! PRD §8.2 "top correctness pitfall"). [`page_refs`] walks the tree once,
//! materializing the ordered list of leaf [`ObjRef`]s.
//!
//! Every walk is **guarded**: a visited-set breaks `/Kids` cycles, a depth
//! counter bounds pathological nesting ([`Limits::max_recursion_depth`]), and the
//! total page count is capped by [`Limits::max_objects`] so a `/Count`-bomb or a
//! self-referential tree can never hang or OOM (PRD §9.6).
//!
//! When the tree is unreachable — no `/Root`, no `/Pages`, or a `/Pages` that
//! yields zero leaves — [`page_refs`] **falls back** to scanning every object in
//! the cross-reference table for `/Type /Page` in object-number order (PRD §8.2
//! step 3: "reconstruct page tree by scanning all `/Type /Page`"). This is the
//! M1d-deferred page-tree fallback, wired here.

use std::collections::HashSet;

use crate::document::DocumentStore;
use crate::geom::{paper_rect, Matrix, Rect};
use crate::object::{Dict, Name, ObjRef, Object};

/// The four inheritable page attributes (ISO 32000-1 §7.7.3.4).
const INHERITABLE: [&str; 4] = ["Resources", "MediaBox", "CropBox", "Rotate"];

/// The ordered list of page-leaf references for `doc`, resolving inheritance is
/// *not* done here — only the leaf order. Walks `/Root → /Pages`, falling back to
/// an object scan when the tree is unreachable or empty (PRD §8.2 step 3).
///
/// The result is deterministic and bounded: cycles are skipped, depth and total
/// count are capped by [`crate::Limits`].
#[must_use]
pub fn page_refs(doc: &DocumentStore) -> Vec<ObjRef> {
    if let Some(refs) = page_refs_from_tree(doc) {
        if !refs.is_empty() {
            return refs;
        }
    }
    page_refs_by_scan(doc)
}

/// The number of pages — `page_refs(doc).len()` (PRD §3.4 `page_count`).
#[must_use]
pub fn page_count(doc: &DocumentStore) -> usize {
    page_refs(doc).len()
}

/// Walks the `/Pages` tree, returning the ordered leaf references, or `None` if
/// the catalog / `/Pages` root is unreachable. An empty `Vec` is a *reachable but
/// pageless* tree (the caller then tries the scan fallback).
fn page_refs_from_tree(doc: &DocumentStore) -> Option<Vec<ObjRef>> {
    let root = doc.root()?;
    let catalog = doc.resolve(root).ok()?;
    let cat_dict = catalog.as_dict()?;
    let pages_ref = match cat_dict.get(&Name::new("Pages")) {
        Some(Object::Reference(r)) => *r,
        // A direct `/Pages` dict has no object identity; the tree walk needs a
        // ref for cycle detection. Treat as unreachable → scan fallback.
        _ => return None,
    };

    let mut out = Vec::new();
    let mut visited = HashSet::new();
    let max_pages = doc.limits().max_objects as usize;
    let max_depth = doc.limits().max_recursion_depth;
    walk(
        doc,
        pages_ref,
        0,
        max_depth,
        max_pages,
        &mut visited,
        &mut out,
    );
    Some(out)
}

/// Recursively descends a `/Pages` subtree rooted at `node_ref`, appending leaf
/// refs to `out`. `depth` is the current nesting; `visited` guards `/Kids`
/// cycles; the walk stops once `out` reaches `max_pages`.
fn walk(
    doc: &DocumentStore,
    node_ref: ObjRef,
    depth: u32,
    max_depth: u32,
    max_pages: usize,
    visited: &mut HashSet<u32>,
    out: &mut Vec<ObjRef>,
) {
    if depth > max_depth || out.len() >= max_pages {
        return;
    }
    if !visited.insert(node_ref.num) {
        return; // already-seen node: a /Kids cycle.
    }
    let Ok(node) = doc.resolve(node_ref) else {
        return;
    };
    let Some(dict) = node.as_dict() else {
        return;
    };

    // A leaf is `/Type /Page`; an intermediate node is `/Type /Pages` (or, for
    // tolerance, anything that carries `/Kids`). Prefer the explicit `/Type`.
    let ty = dict.get(&Name::new("Type")).and_then(Object::as_name);
    let is_leaf = match ty {
        Some(n) if n.as_bytes() == b"Page" => true,
        Some(n) if n.as_bytes() == b"Pages" => false,
        // No / unknown `/Type`: a node carrying `/Kids` is an intermediate node;
        // otherwise it is a (mistyped) leaf (real files omit `/Type` on leaves).
        _ => !dict.contains_key(&Name::new("Kids")),
    };

    if is_leaf {
        out.push(node_ref);
        return;
    }

    let Ok(Some(kids)) = doc.resolve_dict_key(dict, &Name::new("Kids")) else {
        return;
    };
    let Some(kids) = kids.as_array() else {
        return;
    };
    for kid in kids {
        if out.len() >= max_pages {
            break;
        }
        if let Object::Reference(r) = kid {
            walk(doc, *r, depth + 1, max_depth, max_pages, visited, out);
        }
    }
}

/// The fallback: every cross-reference object that resolves to a `/Type /Page`
/// dict, in ascending object-number order (PRD §8.2 step 3). Used when the
/// `/Pages` tree is unreachable or yields no leaves. Bounded by `max_objects`.
fn page_refs_by_scan(doc: &DocumentStore) -> Vec<ObjRef> {
    let mut out = Vec::new();
    let max_pages = doc.limits().max_objects as usize;
    for num in doc.xref().object_numbers() {
        if out.len() >= max_pages {
            break;
        }
        // `get_object` does not follow references; a `/Type /Page` object is a
        // dictionary, never itself a reference.
        let Ok(obj) = doc.get_object(num, 0) else {
            continue;
        };
        if let Some(dict) = obj.as_dict() {
            if dict
                .get(&Name::new("Type"))
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"Page")
            {
                out.push(ObjRef::new(num, 0));
            }
        }
    }
    out
}

/// Resolves an inheritable attribute for the page leaf `page_ref`: returns the
/// value found on the leaf, else the nearest ancestor `/Pages` node's value,
/// walking `/Parent` links to the root (ISO 32000-1 §7.7.3.4). Guards against
/// `/Parent` cycles and depth blow-ups.
///
/// `key` must be one of [`INHERITABLE`]; other keys are read from the leaf only.
fn inherited(doc: &DocumentStore, page_ref: ObjRef, key: &str) -> Option<Object> {
    let is_inheritable = INHERITABLE.contains(&key);
    let name = Name::new(key);
    let mut current = page_ref;
    let mut visited = HashSet::new();
    let mut depth = 0u32;
    let max_depth = doc.limits().max_recursion_depth;
    loop {
        depth += 1;
        if depth > max_depth || !visited.insert(current.num) {
            return None;
        }
        let node = doc.resolve(current).ok()?;
        let dict = node.as_dict()?;
        if let Ok(Some(v)) = doc.resolve_dict_key(dict, &name) {
            if !v.is_null() {
                return Some((*v).clone());
            }
        }
        if !is_inheritable {
            return None;
        }
        // Ascend to the parent `/Pages` node.
        match dict.get(&Name::new("Parent")) {
            Some(Object::Reference(r)) => current = *r,
            _ => return None,
        }
    }
}

/// A page's `/Type /Page` dictionary (the leaf itself, references followed).
#[must_use]
pub fn page_dict(doc: &DocumentStore, page_ref: ObjRef) -> Option<Dict> {
    let obj = doc.resolve(page_ref).ok()?;
    obj.as_dict().cloned()
}

/// The page's effective `/MediaBox` as a [`Rect`], walking inheritance. Defaults
/// to US Letter (612 × 792) when absent or malformed, matching PyMuPDF (PRD §9.2
/// "default Letter if absent"). The rectangle is normalized.
#[must_use]
pub fn mediabox(doc: &DocumentStore, page_ref: ObjRef) -> Rect {
    inherited(doc, page_ref, "MediaBox")
        .and_then(|o| rect_from_object(doc, &o))
        .map(|r| r.normalize())
        .unwrap_or_else(default_letter)
}

/// The page's effective `/CropBox`, walking inheritance. Per PyMuPDF the crop box
/// is clipped to the media box; when absent it equals the media box (ISO
/// 32000-1 §14.11.2). Returns the normalized intersection.
#[must_use]
pub fn cropbox(doc: &DocumentStore, page_ref: ObjRef) -> Rect {
    let mb = mediabox(doc, page_ref);
    match inherited(doc, page_ref, "CropBox").and_then(|o| rect_from_object(doc, &o)) {
        Some(cb) => cb.normalize().intersect(&mb),
        None => mb,
    }
}

/// The page bound — `CropBox ∩ MediaBox` (PyMuPDF `page.rect` / `page.bound()`).
/// Equivalent to [`cropbox`] (which already intersects with the media box), but
/// named for the `Page::rect`/`bound` surface (PRD §9.2).
#[must_use]
pub fn bound(doc: &DocumentStore, page_ref: ObjRef) -> Rect {
    cropbox(doc, page_ref)
}

/// The page's effective `/ArtBox`, walking inheritance. Per the PDF spec and
/// PyMuPDF, when absent the art box defaults to the **crop box** (ISO 32000-1
/// §14.11.2). The rectangle is normalized; it is *not* clipped to the media box.
#[must_use]
pub fn artbox(doc: &DocumentStore, page_ref: ObjRef) -> Rect {
    box_or_cropbox(doc, page_ref, "ArtBox")
}

/// The page's effective `/BleedBox`, walking inheritance. Defaults to the crop
/// box when absent (ISO 32000-1 §14.11.2). Normalized; not clipped.
#[must_use]
pub fn bleedbox(doc: &DocumentStore, page_ref: ObjRef) -> Rect {
    box_or_cropbox(doc, page_ref, "BleedBox")
}

/// The page's effective `/TrimBox`, walking inheritance. Defaults to the crop
/// box when absent (ISO 32000-1 §14.11.2). Normalized; not clipped.
#[must_use]
pub fn trimbox(doc: &DocumentStore, page_ref: ObjRef) -> Rect {
    box_or_cropbox(doc, page_ref, "TrimBox")
}

/// The inherited page box named `key` (`ArtBox`/`BleedBox`/`TrimBox`), or the
/// crop box when absent (ISO 32000-1 §14.11.2 default). Normalized, unclipped.
fn box_or_cropbox(doc: &DocumentStore, page_ref: ObjRef, key: &str) -> Rect {
    match inherited(doc, page_ref, key).and_then(|o| rect_from_object(doc, &o)) {
        Some(r) => r.normalize(),
        None => cropbox(doc, page_ref),
    }
}

/// The page's normalized `/Rotate` ∈ {0, 90, 180, 270}, walking inheritance.
/// Negative and `>360` multiples of 90 are normalized into range (PRD §8.6.1);
/// a non-multiple-of-90 or absent value yields 0.
#[must_use]
pub fn rotation(doc: &DocumentStore, page_ref: ObjRef) -> i32 {
    let raw = inherited(doc, page_ref, "Rotate")
        .and_then(|o| o.as_i64())
        .unwrap_or(0);
    normalize_rotation(raw)
}

/// Normalizes a raw `/Rotate` to {0, 90, 180, 270} (PRD §8.6.1). A value that is
/// not a multiple of 90 is treated as 0 (PyMuPDF tolerance).
#[must_use]
pub fn normalize_rotation(raw: i64) -> i32 {
    if raw % 90 != 0 {
        return 0;
    }
    raw.rem_euclid(360) as i32
}

/// PyMuPDF `page.transformation_matrix` — maps PDF coordinate space → fitz page
/// space (a y-flip relative to the crop box). Derived to be bit-identical to
/// PyMuPDF for `/Rotate` ∈ {0, 90, 180, 270} (verified against fitz 1.24.x).
///
/// With the (raw, mediabox-clipped) crop box `cb = (x0, y0, x1, y1)` and
/// `ch = y1 - y0`: at rotation 0 the matrix is `[1, 0, 0, -1, -x0, y1]`; at any
/// non-zero rotation the crop-box offset is carried by the rotation matrix, so
/// the transformation matrix is `[1, 0, 0, -1, 0, ch]`.
#[must_use]
pub fn transformation_matrix(doc: &DocumentStore, page_ref: ObjRef) -> Matrix {
    let cb = cropbox(doc, page_ref);
    if rotation(doc, page_ref) == 0 {
        Matrix::new(1.0, 0.0, 0.0, -1.0, -cb.x0, cb.y1)
    } else {
        Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, cb.y1 - cb.y0)
    }
}

/// PyMuPDF `page.rotation_matrix` — the `/Rotate` component about the crop box.
/// Derived to be bit-identical to PyMuPDF for `/Rotate` ∈ {0, 90, 180, 270}.
///
/// With crop-box width `cw = x1 - x0` and height `ch = y1 - y0`:
/// rot 0 → `[1, 0, 0, 1, 0, 0]`; rot 90 → `[0, 1, -1, 0, ch, 0]`;
/// rot 180 → `[-1, 0, 0, -1, cw, ch]`; rot 270 → `[0, -1, 1, 0, 0, cw]`.
#[must_use]
pub fn rotation_matrix(doc: &DocumentStore, page_ref: ObjRef) -> Matrix {
    let cb = cropbox(doc, page_ref);
    let cw = cb.x1 - cb.x0;
    let ch = cb.y1 - cb.y0;
    match rotation(doc, page_ref) {
        90 => Matrix::new(0.0, 1.0, -1.0, 0.0, ch, 0.0),
        180 => Matrix::new(-1.0, 0.0, 0.0, -1.0, cw, ch),
        270 => Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, cw),
        _ => Matrix::IDENTITY,
    }
}

/// PyMuPDF `page.derotation_matrix` — the inverse of [`rotation_matrix`].
/// Falls back to the identity for the (impossible here) singular case.
#[must_use]
pub fn derotation_matrix(doc: &DocumentStore, page_ref: ObjRef) -> Matrix {
    rotation_matrix(doc, page_ref)
        .invert()
        .unwrap_or(Matrix::IDENTITY)
}

/// US Letter, the PyMuPDF default page size when `/MediaBox` is absent.
fn default_letter() -> Rect {
    paper_rect("letter").unwrap_or_else(|| Rect::new(0.0, 0.0, 612.0, 792.0))
}

/// Parses a 4-element numeric array (each element possibly an indirect ref) into
/// a [`Rect`]. Returns `None` if the shape is wrong.
fn rect_from_object(doc: &DocumentStore, obj: &Object) -> Option<Rect> {
    let arr = obj.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    let mut v = [0.0f64; 4];
    for (i, e) in arr.iter().enumerate() {
        let n = match e {
            Object::Reference(r) => doc.resolve(*r).ok()?.as_f64()?,
            other => other.as_f64()?,
        };
        v[i] = n;
    }
    Some(Rect::new(v[0], v[1], v[2], v[3]))
}
