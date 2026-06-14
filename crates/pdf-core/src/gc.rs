//! Garbage collection over a **save-time snapshot** of the effective object set
//! (PRD §8.7 levels 1–4, §8.7.1 dedup exclusion + COW-unshare).
//!
//! The collector takes the writer's effective object map (object number →
//! materialized [`Object`]) plus the trailer roots, and returns a transformed
//! map together with the (possibly remapped) `/Root`, `/Info` and `/Encrypt`
//! references. Because it works on a snapshot — never the live
//! [`crate::DocumentStore`] arena / change set — a dedup merge can never corrupt
//! the editable model: a later `update_*` mutates the *live* object, which is
//! still un-merged, so the two logical users diverge cleanly (copy-on-write by
//! construction, PRD §8.7.1).
//!
//! Levels (cumulative):
//! - **1** mark-sweep: keep only objects reachable from `/Root` + the trailer
//!   roots (`/Info`, `/Encrypt`); drop the rest.
//! - **2** + compact/renumber survivors to a dense `1..=n` with a consistent
//!   reference remap across the whole graph.
//! - **3** + merge structurally identical **non-stream** objects (with the
//!   [exclusion list](`is_dedup_excluded`)).
//! - **4** + merge identical **streams** (dict + decoded/raw body).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::object::{Dict, Name, ObjRef, Object};

/// The trailer roots that anchor reachability and are carried into the saved
/// trailer (PRD §8.7). Each is an optional object number.
#[derive(Copy, Clone, Debug, Default)]
pub struct Roots {
    /// `/Root` (the document catalog) — always required for a savable document.
    pub root: Option<u32>,
    /// `/Info` (the document information dictionary), if referenced indirectly.
    pub info: Option<u32>,
    /// `/Encrypt` (the security handler dictionary), if referenced indirectly.
    pub encrypt: Option<u32>,
}

/// The result of a GC pass: the surviving (possibly renumbered / deduped) object
/// map plus the remapped trailer roots.
pub struct GcResult {
    /// Surviving objects, object number → value (references already remapped).
    pub objects: BTreeMap<u32, Object>,
    /// The trailer roots after any renumber / dedup remap.
    pub roots: Roots,
}

/// Runs garbage collection at `level` (`1..=4`) over `objects` anchored at
/// `roots`. `level == 0` is handled by the caller (no GC); this function assumes
/// `level >= 1`.
#[must_use]
pub fn collect(objects: BTreeMap<u32, Object>, roots: Roots, level: u8) -> GcResult {
    // Level 1: mark-sweep.
    let kept = mark_sweep(&objects, roots);
    let mut objects: BTreeMap<u32, Object> = objects
        .into_iter()
        .filter(|(num, _)| kept.contains(num))
        .collect();
    let mut roots = roots;

    // Level 3/4: dedup (done before renumber so the remap is a single pass).
    if level >= 3 {
        let remap = dedup(&objects, roots, level >= 4);
        apply_dedup(&mut objects, &mut roots, &remap);
    }

    // Level 2+: compact / renumber to a dense 1..=n.
    if level >= 2 {
        let remap = dense_remap(&objects);
        apply_renumber(&mut objects, &mut roots, &remap);
    }

    GcResult { objects, roots }
}

// --- level 1: mark & sweep -----------------------------------------------

/// The set of object numbers reachable from the trailer roots (PRD §8.7).
fn mark_sweep(objects: &BTreeMap<u32, Object>, roots: Roots) -> HashSet<u32> {
    let mut kept = HashSet::new();
    let mut stack: Vec<u32> = roots
        .root
        .into_iter()
        .chain(roots.info)
        .chain(roots.encrypt)
        .collect();
    while let Some(num) = stack.pop() {
        if !kept.insert(num) {
            continue;
        }
        if let Some(obj) = objects.get(&num) {
            for r in referenced_objects(obj) {
                if !kept.contains(&r) {
                    stack.push(r);
                }
            }
        }
    }
    kept
}

/// Every object number referenced (transitively within this object's *direct*
/// value) by `obj`. References inside streams' dicts are included.
fn referenced_objects(obj: &Object) -> Vec<u32> {
    let mut out = Vec::new();
    collect_refs(obj, &mut out);
    out
}

fn collect_refs(obj: &Object, out: &mut Vec<u32>) {
    match obj {
        Object::Reference(r) => out.push(r.num),
        Object::Array(items) => {
            for it in items {
                collect_refs(it, out);
            }
        }
        Object::Dictionary(d) => {
            for v in d.values() {
                collect_refs(v, out);
            }
        }
        Object::Stream(s) => {
            for v in s.dict.values() {
                collect_refs(v, out);
            }
        }
        _ => {}
    }
}

// --- level 2: compact / renumber -----------------------------------------

/// A dense `old number → new number` remap over the survivors, assigned in
/// ascending old-number order so the output is deterministic. New numbers start
/// at 1 (object 0 stays the free-list head).
fn dense_remap(objects: &BTreeMap<u32, Object>) -> HashMap<u32, u32> {
    let mut remap = HashMap::new();
    for (new, &old) in objects.keys().enumerate() {
        remap.insert(old, new as u32 + 1);
    }
    remap
}

// --- level 3/4: dedup ----------------------------------------------------

/// Merges structurally identical objects, returning an `old → survivor` remap
/// (a survivor maps to itself). Streams are deduped only when `merge_streams` is
/// set (level 4). Excluded objects (PRD §8.7.1) never merge.
///
/// Two objects are "identical" when their **canonical serialized form** is equal
/// — for a stream that is the serialized dict plus the materialized body bytes.
/// Identity is computed on the object *as stored* (references intact), so a fixed
/// point is reached without iterating: if two dicts both reference object 6, they
/// are identical regardless of whether 6 itself was a dedup survivor (the remap
/// is applied afterwards, uniformly).
fn dedup(objects: &BTreeMap<u32, Object>, roots: Roots, merge_streams: bool) -> HashMap<u32, u32> {
    let mut remap: HashMap<u32, u32> = HashMap::new();
    // signature → first (smallest) object number carrying it.
    let mut seen: HashMap<Vec<u8>, u32> = HashMap::new();

    for (&num, obj) in objects {
        // A survivor maps to itself by default.
        remap.insert(num, num);
        if is_dedup_excluded(num, obj, roots) {
            continue;
        }
        if matches!(obj, Object::Stream(_)) && !merge_streams {
            continue;
        }
        let sig = object_signature(obj);
        match seen.get(&sig) {
            Some(&first) => {
                remap.insert(num, first);
            }
            None => {
                seen.insert(sig, num);
            }
        }
    }
    remap
}

/// Whether object `num` is excluded from GC-3/4 dedup (PRD §8.7.1). The list is
/// **normative**: never merge a `/Type /Page`, `/Type /Pages`, `/Annot`,
/// `/Widget`, the Catalog (`/Type /Catalog`), the `/Encrypt` dict, an object
/// stream (`/Type /ObjStm`) or cross-reference stream (`/Type /XRef`), or an
/// object carrying a `/StructParent`/`/StructParents` (tagged-PDF identity).
///
/// (Named-destination / outline-`/Dest` targets and `/AcroForm` field-tree
/// members are additionally excluded once those graphs are authored — M3d/M4;
/// the `/Type`- and role-based rules here cover the M3b corpus and are the part
/// the M3b tests assert.)
fn is_dedup_excluded(num: u32, obj: &Object, roots: Roots) -> bool {
    if roots.encrypt == Some(num) {
        return true;
    }
    let Some(dict) = obj.as_dict() else {
        return false;
    };
    if dict.contains_key(&Name::new("StructParent"))
        || dict.contains_key(&Name::new("StructParents"))
    {
        return true;
    }
    if let Some(Object::Name(ty)) = dict.get(&Name::new("Type")) {
        matches!(
            ty.as_bytes(),
            b"Page" | b"Pages" | b"Catalog" | b"Annot" | b"Widget" | b"ObjStm" | b"XRef"
        )
    } else {
        false
    }
}

/// A structural-identity signature for `obj`: its canonical serialized bytes,
/// plus (for a stream) the materialized body bytes. Two objects with equal
/// signatures are interchangeable.
fn object_signature(obj: &Object) -> Vec<u8> {
    match obj {
        Object::Stream(s) => {
            let mut sig =
                crate::serialize::write_object(&Object::Dictionary(strip_length(&s.dict)));
            sig.extend_from_slice(b"\x00stream\x00");
            if let Some(b) = s.data.owned_bytes() {
                sig.extend_from_slice(b.as_ref());
            }
            sig
        }
        other => crate::serialize::write_object(other),
    }
}

/// A stream dict with `/Length` removed — so two otherwise-identical bodies with
/// stale/differing `/Length` values still compare equal (the writer recomputes
/// it anyway).
fn strip_length(dict: &Dict) -> Dict {
    let mut d = dict.clone();
    d.remove(&Name::new("Length"));
    d
}

// --- shared: apply a remap -----------------------------------------------

/// Applies a **dedup** remap (old → survivor): a survivor maps to itself; a
/// duplicate maps to a *different* (smaller) survivor number and is **dropped**.
/// References and trailer roots are rewritten through the remap; object numbers
/// of survivors are left unchanged (a later renumber densifies them).
fn apply_dedup(objects: &mut BTreeMap<u32, Object>, roots: &mut Roots, remap: &HashMap<u32, u32>) {
    let old = std::mem::take(objects);
    for (num, mut obj) in old {
        let target = remap.get(&num).copied().unwrap_or(num);
        if target != num {
            continue; // a duplicate — merged into `target`, drop it.
        }
        remap_refs(&mut obj, remap);
        objects.insert(num, obj);
    }
    remap_roots(roots, remap);
}

/// Applies a **renumber** remap (old → new): a bijection over the survivors, so
/// nothing is dropped. References and trailer roots are rewritten and every
/// object moves to its new (dense) number.
fn apply_renumber(
    objects: &mut BTreeMap<u32, Object>,
    roots: &mut Roots,
    remap: &HashMap<u32, u32>,
) {
    let old = std::mem::take(objects);
    for (num, mut obj) in old {
        let new = remap.get(&num).copied().unwrap_or(num);
        remap_refs(&mut obj, remap);
        objects.insert(new, obj);
    }
    remap_roots(roots, remap);
}

/// Rewrites the three trailer roots through `remap`.
fn remap_roots(roots: &mut Roots, remap: &HashMap<u32, u32>) {
    roots.root = roots.root.map(|n| remap.get(&n).copied().unwrap_or(n));
    roots.info = roots.info.map(|n| remap.get(&n).copied().unwrap_or(n));
    roots.encrypt = roots.encrypt.map(|n| remap.get(&n).copied().unwrap_or(n));
}

/// Rewrites every [`Object::Reference`] in `obj` through `remap`.
fn remap_refs(obj: &mut Object, remap: &HashMap<u32, u32>) {
    match obj {
        Object::Reference(r) => {
            if let Some(&new) = remap.get(&r.num) {
                *r = ObjRef::new(new, r.gen);
            }
        }
        Object::Array(items) => {
            for it in items {
                remap_refs(it, remap);
            }
        }
        Object::Dictionary(d) => {
            for v in d.values_mut() {
                remap_refs(v, remap);
            }
        }
        Object::Stream(s) => {
            for v in s.dict.values_mut() {
                remap_refs(v, remap);
            }
        }
        _ => {}
    }
}
