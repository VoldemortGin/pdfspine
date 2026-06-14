//! `insert_pdf` (merge) + `extract_pages` (split) — transitive-closure deep copy
//! with reference remapping and shared-object single-copy (PRD §8.7 / §12).
//!
//! The merge copies the object graph reachable from the selected source pages
//! into the destination, allocating **fresh** object numbers and rewriting every
//! reference through a `src ObjRef → dst ObjRef` map. The map is the dedup
//! mechanism: an object is copied **once** the first time it is reached and the
//! map entry is recorded *before* its children are copied, so a shared font (or
//! XObject, or any shared/cyclic node) is copied a single time and referenced by
//! all copied pages — the "shared font deduped single" requirement (PRD §12).
//!
//! Inherited page attributes (`/Resources` `/MediaBox` `/CropBox` `/Rotate`) are
//! materialized onto the copied leaves before copying so the pages render
//! independently in the destination; each copied leaf's `/Parent` is repointed
//! to the destination root `/Pages`; the copied page refs are spliced into the
//! destination page tree at `start_at`.

use std::collections::HashMap;

use pdf_core::error::{Error, Result};
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamData, StreamObj};
use pdf_core::pagetree;
use pdf_core::{DocumentStore, Limits, SaveOptions};

use crate::page_ops::PageEditor;

/// Options for [`insert_pdf`] (PRD §8.7). All fields default to "append every
/// source page at the end, no rotation override".
#[derive(Clone, Copy, Debug, Default)]
pub struct InsertOptions {
    /// First source page index to copy (inclusive). `None` ⇒ 0.
    pub from_page: Option<usize>,
    /// Last source page index to copy (inclusive). `None` ⇒ last page. When
    /// `to_page < from_page` the range is copied in **reverse** order (PyMuPDF).
    pub to_page: Option<usize>,
    /// Destination position to splice the copied pages at. `None` ⇒ append.
    pub start_at: Option<usize>,
    /// A rotation (degrees) to apply to every inserted page. `None` ⇒ keep the
    /// source rotation.
    pub rotate: Option<i64>,
}

/// Deep-copies the selected pages of `src` into `dst` and splices them into the
/// destination page tree (PRD §8.7 `insert_pdf`). Returns the destination leaf
/// references of the inserted pages, in insertion order.
///
/// `dst` is flattened first (so its page tree is a flat `/Kids`). The copy is a
/// transitive-closure graft with fresh destination object numbers and full
/// reference remapping; shared / cyclic source objects are copied once.
///
/// # Errors
///
/// [`Error::Xref`] if `dst` has no resolvable `/Root → /Pages`; ChangeSet /
/// resolution errors propagate. An out-of-range source page range is clamped to
/// the available pages (an empty range is a no-op).
pub fn insert_pdf(
    dst: &DocumentStore,
    src: &DocumentStore,
    opts: &InsertOptions,
) -> Result<Vec<ObjRef>> {
    let mut editor = PageEditor::new(dst)?;

    let src_pages = pagetree::page_refs(src);
    let selected = select_range(src_pages.len(), opts.from_page, opts.to_page);
    if selected.is_empty() {
        return Ok(Vec::new());
    }

    let mut graft = Graft::new(src, dst);
    let mut copied_leaves = Vec::with_capacity(selected.len());
    for &src_idx in &selected {
        let src_leaf = src_pages[src_idx];
        let dst_leaf = graft.copy_page_leaf(src_leaf, editor.pages_ref(), opts.rotate)?;
        copied_leaves.push(dst_leaf);
    }

    // Splice the copied leaves into the destination at `start_at` (append by
    // default). Insert them one at a time so a clamped `start_at` keeps order.
    let start = opts.start_at.unwrap_or_else(|| editor.page_count());
    for (k, &leaf) in copied_leaves.iter().enumerate() {
        editor.insert_page(start + k, leaf)?;
    }
    Ok(copied_leaves)
}

/// Extracts the pages named by `indices` (in that order) into a **fresh**
/// self-contained one-document byte stream (PRD §8.7 split). Builds an empty
/// destination document, grafts the selected `src` pages into it, and full-saves
/// it. The returned bytes reopen as a standalone PDF whose pages equal the
/// selected source pages.
///
/// # Errors
///
/// [`Error::Unsupported`] if any index is out of range; resolution / save errors
/// propagate.
pub fn extract_pages(src: &DocumentStore, indices: &[usize]) -> Result<Vec<u8>> {
    let src_pages = pagetree::page_refs(src);
    for &i in indices {
        if i >= src_pages.len() {
            return Err(Error::Unsupported("extract_pages: index out of range"));
        }
    }
    let dst = DocumentStore::from_bytes(empty_doc(), Limits::default())?;
    let opts = InsertOptions {
        from_page: None,
        to_page: None,
        start_at: None,
        rotate: None,
    };
    // Insert exactly the requested pages, in the requested order, one by one
    // (a per-index insert preserves arbitrary ordering / duplicates).
    let mut editor = PageEditor::new(&dst)?;
    let mut graft = Graft::new(src, &dst);
    let mut leaves = Vec::with_capacity(indices.len());
    for &i in indices {
        let dst_leaf = graft.copy_page_leaf(src_pages[i], editor.pages_ref(), opts.rotate)?;
        leaves.push(dst_leaf);
    }
    for (k, &leaf) in leaves.iter().enumerate() {
        editor.insert_page(k, leaf)?;
    }
    dst.save_to_vec(&SaveOptions::default().with_garbage(1))
}

/// Resolves an inclusive `[from, to]` source page range against `len` pages.
/// Returns the ordered list of indices (reversed when `to < from`). An empty
/// document yields an empty list.
fn select_range(len: usize, from: Option<usize>, to: Option<usize>) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let last = len - 1;
    let from = from.unwrap_or(0).min(last);
    let to = to.unwrap_or(last).min(last);
    if from <= to {
        (from..=to).collect()
    } else {
        (to..=from).rev().collect()
    }
}

/// A single deep-copy ("graft") session: maps each source object number to its
/// freshly allocated destination reference, copying every object **once**.
struct Graft<'a> {
    src: &'a DocumentStore,
    dst: &'a DocumentStore,
    /// `src object number → dst ObjRef`. Recorded *before* a node's children are
    /// copied, so cycles and shared nodes resolve to the single dst copy.
    map: HashMap<u32, ObjRef>,
}

impl<'a> Graft<'a> {
    fn new(src: &'a DocumentStore, dst: &'a DocumentStore) -> Self {
        Graft {
            src,
            dst,
            map: HashMap::new(),
        }
    }

    /// Copies one source page leaf into the destination: materializes inherited
    /// attributes, repoints `/Parent` to `dst_pages`, applies the optional
    /// `rotate` override, then deep-copies the (now self-contained) leaf graph.
    /// Returns the destination leaf reference.
    fn copy_page_leaf(
        &mut self,
        src_leaf: ObjRef,
        dst_pages: ObjRef,
        rotate: Option<i64>,
    ) -> Result<ObjRef> {
        let mut dict = self
            .src
            .resolve(src_leaf)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::xref(0, "insert_pdf: source page is not a dictionary"))?;

        // Materialize inherited attributes onto the copied leaf so it renders
        // independently in the destination (PRD §8.7). `/Parent` is replaced.
        for key in ["Resources", "MediaBox", "CropBox", "Rotate"] {
            let name = Name::new(key);
            let has = dict.get(&name).is_some_and(|v| !v.is_null());
            if !has {
                if let Some(val) = inherited_value(self.src, src_leaf, key) {
                    dict.insert(name, val);
                }
            }
        }
        // Drop the source `/Parent` link before copying (it points into the
        // source tree); it is set to the destination root afterwards.
        dict.remove(&Name::new("Parent"));
        if let Some(deg) = rotate {
            dict.insert(
                Name::new("Rotate"),
                Object::Integer(i64::from(pagetree::normalize_rotation(deg))),
            );
        }

        // Allocate the destination leaf number up-front and record the mapping
        // so any self-reference inside the page graph resolves to it. The leaf
        // is *not* part of `map` keyed by its source number unless it is itself
        // reached as a reference — but page leaves are spliced by ref, never
        // referenced from their own children, so a fresh allocation is correct.
        let placeholder = self.dst.add_object(Object::Dictionary(Dict::new()))?;
        self.map.insert(src_leaf.num, placeholder);

        // Deep-copy each value of the leaf dict (children get fresh numbers).
        let mut copied = Dict::new();
        for (k, v) in &dict {
            copied.insert(k.clone(), self.copy_value(v)?);
        }
        copied.insert(Name::new("Parent"), Object::Reference(dst_pages));
        copied.insert(Name::new("Type"), Object::Name(Name::new("Page")));
        self.dst
            .update_object(placeholder, Object::Dictionary(copied))?;
        Ok(placeholder)
    }

    /// Deep-copies an arbitrary object value, recursively remapping any
    /// references it contains.
    fn copy_value(&mut self, obj: &Object) -> Result<Object> {
        match obj {
            Object::Reference(r) => Ok(Object::Reference(self.copy_indirect(*r)?)),
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.copy_value(it)?);
                }
                Ok(Object::Array(out))
            }
            Object::Dictionary(d) => Ok(Object::Dictionary(self.copy_dict(d)?)),
            Object::Stream(s) => {
                let dict = self.copy_dict(&s.dict)?;
                let body = self.src.stream_raw_bytes(s)?;
                Ok(Object::Stream(StreamObj {
                    dict,
                    data: StreamData::Encoded(body),
                }))
            }
            // Scalars are copied verbatim.
            other => Ok(other.clone()),
        }
    }

    /// Deep-copies a dictionary's values.
    fn copy_dict(&mut self, d: &Dict) -> Result<Dict> {
        let mut out = Dict::new();
        for (k, v) in d {
            out.insert(k.clone(), self.copy_value(v)?);
        }
        Ok(out)
    }

    /// Copies the indirect object `src_ref` into the destination (once),
    /// returning its destination reference. Records the mapping **before**
    /// recursing so cycles and shared objects resolve to the single copy.
    fn copy_indirect(&mut self, src_ref: ObjRef) -> Result<ObjRef> {
        if let Some(&dst_ref) = self.map.get(&src_ref.num) {
            return Ok(dst_ref);
        }
        // Resolve the source object. A dangling reference becomes Null in the
        // destination (Lenient tolerance, PRD §8.1) — never a dangling dst ref.
        let resolved = self.src.resolve(src_ref)?;

        // Allocate the destination number first, record the mapping, THEN fill
        // it in (so a self/forward reference points at this very object).
        let placeholder = self.dst.add_object(Object::Null)?;
        self.map.insert(src_ref.num, placeholder);

        let copied = self.copy_value(resolved.as_ref())?;
        self.dst.update_object(placeholder, copied)?;
        Ok(placeholder)
    }
}

/// Resolves an inheritable attribute for `leaf` by walking `/Parent` to the
/// root (ISO 32000-1 §7.7.3.4). Mirrors the page-tree resolver; used to
/// materialize attributes onto copied leaves.
fn inherited_value(doc: &DocumentStore, leaf: ObjRef, key: &str) -> Option<Object> {
    let name = Name::new(key);
    let mut current = leaf;
    let mut seen = std::collections::HashSet::new();
    let max_depth = doc.limits().max_recursion_depth;
    let mut depth = 0u32;
    loop {
        depth += 1;
        if depth > max_depth || !seen.insert(current.num) {
            return None;
        }
        let node = doc.resolve(current).ok()?;
        let dict = node.as_dict()?;
        if let Ok(Some(v)) = doc.resolve_dict_key(dict, &name) {
            if !v.is_null() {
                return Some((*v).clone());
            }
        }
        match dict.get(&Name::new("Parent")) {
            Some(Object::Reference(r)) => current = *r,
            _ => return None,
        }
    }
}

/// A minimal empty (zero-page) PDF: catalog + an empty `/Pages` node. Used as the
/// destination for [`extract_pages`].
fn empty_doc() -> Vec<u8> {
    // 1: catalog, 2: pages (empty). Classic xref, hand-laid for byte accuracy.
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::new();

    offsets.push((1u32, out.len()));
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets.push((2u32, out.len()));
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");

    let startxref = out.len();
    out.extend_from_slice(b"xref\n0 3\n");
    out.extend_from_slice(b"0000000000 65535 f \n");
    let mut map = std::collections::HashMap::new();
    for (num, off) in &offsets {
        map.insert(*num, *off);
    }
    for num in 1..3u32 {
        let off = map[&num];
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(b"trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
    out
}
