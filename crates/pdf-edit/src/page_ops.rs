//! Page-tree editing — `new_page` / `insert_page` / `delete_page` /
//! `copy_page` / `move_page` / `select` + box / rotation setters (PRD §8.7).
//!
//! All edits go through the [`DocumentStore`]'s ChangeSet object-edit API
//! (`add_object` / `update_object`), so a subsequent `save` (full or
//! incremental) reflects them. The page tree is **normalized to a single-level
//! flat `/Kids` list under the root `/Pages`** on the first edit (PRD §8.7:
//! flatten is the v1 default; it is round-trip-safe because the four inheritable
//! attributes — `/Resources` `/MediaBox` `/CropBox` `/Rotate` — are materialized
//! onto each leaf before the intermediate nodes are discarded). After that the
//! invariant maintained at every step is:
//!
//! - the root `/Pages` `/Kids` is a flat array of leaf references, in page order;
//! - the root `/Pages` `/Count` equals `/Kids.len()`;
//! - every leaf's `/Parent` points at the root `/Pages` reference.
//!
//! There is no persistent in-memory page list to keep in sync: the live page
//! order is *always* re-derived from the document via [`pagetree::page_refs`],
//! which reads through the ChangeSet overlay, so every query after an edit sees
//! the new order.

use pdf_core::error::{Error, Result};
use pdf_core::geom::Rect;
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::pagetree;
use pdf_core::DocumentStore;

/// An editor over one document's page tree (PRD §8.7). Borrows the
/// [`DocumentStore`]; edits land in its ChangeSet overlay (interior mutability).
pub struct PageEditor<'a> {
    doc: &'a DocumentStore,
    /// The root `/Pages` reference, established by [`PageEditor::new`] (flatten).
    pages_ref: ObjRef,
}

impl<'a> PageEditor<'a> {
    /// Opens an editor on `doc`, **flattening** the page tree to a single-level
    /// flat `/Kids` list under the root `/Pages` (PRD §8.7). Idempotent: a
    /// document already flat is left semantically unchanged.
    ///
    /// # Errors
    ///
    /// [`Error::Xref`] if the document has no resolvable `/Root → /Pages`.
    pub fn new(doc: &'a DocumentStore) -> Result<Self> {
        let pages_ref = flatten(doc)?;
        Ok(PageEditor { doc, pages_ref })
    }

    /// The root `/Pages` reference for this editor.
    #[must_use]
    pub fn pages_ref(&self) -> ObjRef {
        self.pages_ref
    }

    /// The current ordered list of page-leaf references (re-derived live).
    #[must_use]
    pub fn page_refs(&self) -> Vec<ObjRef> {
        kids(self.doc, self.pages_ref)
    }

    /// The current page count (PyMuPDF `page_count`).
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.page_refs().len()
    }

    // --- new / insert -----------------------------------------------------

    /// Creates a blank page (MediaBox `[0 0 w h]` + an empty `/Contents` stream)
    /// and inserts it at zero-based `index` (PRD §8.7 `new_page`). `index` is
    /// clamped to `[0, page_count]` (an index past the end appends). Returns the
    /// new leaf reference.
    ///
    /// # Errors
    ///
    /// Propagates ChangeSet-allocation errors.
    pub fn new_page(&mut self, index: usize, width: f64, height: f64) -> Result<ObjRef> {
        // Empty content stream (a valid zero-length stream body).
        let content = self.doc.add_object(Object::Stream(StreamObj::new_encoded(
            Dict::from_iter([(Name::new("Length"), Object::Integer(0))]),
            Vec::new(),
        )))?;
        let mut leaf = Dict::new();
        leaf.insert(Name::new("Type"), Object::Name(Name::new("Page")));
        leaf.insert(Name::new("Parent"), Object::Reference(self.pages_ref));
        leaf.insert(
            Name::new("MediaBox"),
            rect_array(&Rect::new(0.0, 0.0, width, height)),
        );
        leaf.insert(Name::new("Contents"), Object::Reference(content));
        leaf.insert(Name::new("Resources"), Object::Dictionary(Dict::new()));
        let leaf_ref = self.doc.add_object(Object::Dictionary(leaf))?;
        self.splice(&[leaf_ref], index)?;
        Ok(leaf_ref)
    }

    /// Inserts an existing leaf reference `leaf` at zero-based `index`, repointing
    /// its `/Parent` to the root `/Pages` (PRD §8.7 `insert_page`). `index` is
    /// clamped to `[0, page_count]`.
    ///
    /// # Errors
    ///
    /// [`Error::Xref`] if `leaf` does not resolve to a dictionary.
    pub fn insert_page(&mut self, index: usize, leaf: ObjRef) -> Result<()> {
        self.repoint_parent(leaf)?;
        self.splice(&[leaf], index)
    }

    // --- delete -----------------------------------------------------------

    /// Deletes the page at zero-based `index` (PRD §8.7 `delete_page`). The leaf
    /// object itself is left in place (it may be shared via `copy_page`); GC on a
    /// later full save reclaims it if it became unreachable.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `index` is out of range.
    pub fn delete_page(&mut self, index: usize) -> Result<()> {
        let mut refs = self.page_refs();
        if index >= refs.len() {
            return Err(Error::Unsupported("delete_page: index out of range"));
        }
        refs.remove(index);
        self.write_kids(&refs)
    }

    // --- copy / move ------------------------------------------------------

    /// Copies the page at `from` to position `to` (PRD §8.7 `copy_page`). A fresh
    /// leaf object is allocated whose dictionary equals the source leaf's (a
    /// shallow copy: child objects — Contents, Resources, fonts — are **shared**
    /// by reference, matching PyMuPDF). A distinct leaf is required because the
    /// page-tree walk dedups `/Kids` by object number, so reusing the same ref
    /// would not yield a second page. `to` is clamped to `[0, page_count]`.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `from` is out of range.
    pub fn copy_page(&mut self, from: usize, to: usize) -> Result<()> {
        let refs = self.page_refs();
        let leaf = *refs
            .get(from)
            .ok_or(Error::Unsupported("copy_page: from index out of range"))?;
        let dup = self.shallow_copy_leaf(leaf)?;
        self.splice(&[dup], to)
    }

    /// Moves the page at `from` to position `to` (PRD §8.7 `move_page`). Indices
    /// are interpreted against the *current* order; `to` is the desired final
    /// position. A `from == to` move is a no-op.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `from` is out of range.
    pub fn move_page(&mut self, from: usize, to: usize) -> Result<()> {
        let mut refs = self.page_refs();
        if from >= refs.len() {
            return Err(Error::Unsupported("move_page: from index out of range"));
        }
        let leaf = refs.remove(from);
        let to = to.min(refs.len());
        refs.insert(to, leaf);
        self.write_kids(&refs)
    }

    // --- select -----------------------------------------------------------

    /// Reorders / subsets the document to exactly the pages named by `indices`,
    /// in that order (PyMuPDF `select`). Duplicates duplicate the page; an empty
    /// slice yields a zero-page document.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if any index is out of range.
    pub fn select(&mut self, indices: &[usize]) -> Result<()> {
        let refs = self.page_refs();
        // Validate all indices first (no partial mutation on error).
        for &i in indices {
            if i >= refs.len() {
                return Err(Error::Unsupported("select: index out of range"));
            }
        }
        // A repeated index needs a *distinct* leaf object (the page-tree walk
        // dedups /Kids by object number), so allocate a shallow copy for each
        // occurrence after the first.
        let mut seen = std::collections::HashSet::new();
        let mut chosen = Vec::with_capacity(indices.len());
        for &i in indices {
            let leaf = refs[i];
            if seen.insert(leaf.num) {
                chosen.push(leaf);
            } else {
                chosen.push(self.shallow_copy_leaf(leaf)?);
            }
        }
        self.write_kids(&chosen)
    }

    // --- box / rotation setters ------------------------------------------

    /// Sets the page's `/MediaBox` to `rect` (PRD §8.7 box setter).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `index` is out of range.
    pub fn set_mediabox(&mut self, index: usize, rect: &Rect) -> Result<()> {
        self.update_leaf_key(index, "MediaBox", rect_array(&rect.normalize()))
    }

    /// Sets the page's `/CropBox` to `rect`, clipped to the media box
    /// (cropbox ⊆ mediabox, PRD §8.7).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `index` is out of range.
    pub fn set_cropbox(&mut self, index: usize, rect: &Rect) -> Result<()> {
        let leaf = self.leaf_at(index)?;
        let mb = pagetree::mediabox(self.doc, leaf);
        let clipped = rect.normalize().intersect(&mb);
        self.update_leaf_key(index, "CropBox", rect_array(&clipped))
    }

    /// Sets the page's `/Rotate`, normalized to {0, 90, 180, 270} (PRD §8.6.1).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `index` is out of range.
    pub fn set_rotation(&mut self, index: usize, degrees: i64) -> Result<()> {
        let r = pagetree::normalize_rotation(degrees);
        self.update_leaf_key(index, "Rotate", Object::Integer(i64::from(r)))
    }

    // --- internals --------------------------------------------------------

    /// Inserts `new` leaf refs into the flat `/Kids` at clamped `index`.
    fn splice(&mut self, new: &[ObjRef], index: usize) -> Result<()> {
        let mut refs = self.page_refs();
        let at = index.min(refs.len());
        for (k, leaf) in new.iter().enumerate() {
            refs.insert(at + k, *leaf);
        }
        self.write_kids(&refs)
    }

    /// Rewrites the root `/Pages` `/Kids` + `/Count` to exactly `refs`.
    fn write_kids(&mut self, refs: &[ObjRef]) -> Result<()> {
        let mut pages = self
            .doc
            .resolve(self.pages_ref)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::xref(0, "root /Pages is not a dictionary"))?;
        pages.insert(
            Name::new("Kids"),
            Object::Array(refs.iter().map(|r| Object::Reference(*r)).collect()),
        );
        pages.insert(Name::new("Count"), Object::Integer(refs.len() as i64));
        self.doc
            .update_object(self.pages_ref, Object::Dictionary(pages))
    }

    /// The leaf ref at `index`, or an out-of-range error.
    fn leaf_at(&self, index: usize) -> Result<ObjRef> {
        self.page_refs()
            .get(index)
            .copied()
            .ok_or(Error::Unsupported("page index out of range"))
    }

    /// Updates a single key on the leaf at `index`.
    fn update_leaf_key(&mut self, index: usize, key: &str, val: Object) -> Result<()> {
        let leaf = self.leaf_at(index)?;
        let mut dict = self
            .doc
            .resolve(leaf)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::xref(0, "page leaf is not a dictionary"))?;
        dict.insert(Name::new(key), val);
        self.doc.update_object(leaf, Object::Dictionary(dict))
    }

    /// Allocates a fresh leaf object whose dictionary equals `leaf`'s, with
    /// `/Parent` repointed to the root `/Pages`. Child objects (Contents,
    /// Resources, fonts) are **shared** by reference (a shallow copy). Returns
    /// the new leaf reference.
    fn shallow_copy_leaf(&self, leaf: ObjRef) -> Result<ObjRef> {
        let mut dict = self
            .doc
            .resolve(leaf)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::xref(0, "copy_page: leaf is not a dictionary"))?;
        dict.insert(Name::new("Parent"), Object::Reference(self.pages_ref));
        self.doc.add_object(Object::Dictionary(dict))
    }

    /// Repoints a leaf's `/Parent` to the root `/Pages`.
    fn repoint_parent(&self, leaf: ObjRef) -> Result<()> {
        let mut dict = self
            .doc
            .resolve(leaf)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::xref(0, "insert_page: leaf is not a dictionary"))?;
        dict.insert(Name::new("Parent"), Object::Reference(self.pages_ref));
        self.doc.update_object(leaf, Object::Dictionary(dict))
    }
}

/// A `[x0 y0 x1 y1]` array object for `rect`.
pub(crate) fn rect_array(rect: &Rect) -> Object {
    Object::Array(vec![
        Object::Real(rect.x0),
        Object::Real(rect.y0),
        Object::Real(rect.x1),
        Object::Real(rect.y1),
    ])
}

/// The flat `/Kids` leaf list of the node `pages_ref` (references only, in
/// order). Reads through the ChangeSet overlay.
fn kids(doc: &DocumentStore, pages_ref: ObjRef) -> Vec<ObjRef> {
    let Ok(node) = doc.resolve(pages_ref) else {
        return Vec::new();
    };
    let Some(dict) = node.as_dict() else {
        return Vec::new();
    };
    let Some(Object::Array(arr)) = dict.get(&Name::new("Kids")) else {
        return Vec::new();
    };
    arr.iter().filter_map(Object::as_reference).collect()
}

/// Normalizes the page tree to a flat single-level `/Kids` under the root
/// `/Pages` (PRD §8.7). Materializes the four inheritable attributes onto each
/// leaf, repoints every leaf's `/Parent` to the root, rewrites `/Kids` to the
/// ordered leaf list and `/Count` to its length. Returns the root `/Pages` ref.
///
/// Idempotent for an already-flat tree (the leaves keep whatever explicit values
/// they already carry; inheritance from the root is a no-op when the root has no
/// inheritable keys).
fn flatten(doc: &DocumentStore) -> Result<ObjRef> {
    let pages_ref = pages_root_ref(doc)?;
    let leaves = pagetree::page_refs(doc);

    // Materialize inherited attributes onto each leaf BEFORE we drop the
    // intermediate nodes, then repoint `/Parent` to the root.
    for &leaf in &leaves {
        let Ok(node) = doc.resolve(leaf) else {
            continue;
        };
        let Some(mut dict) = node.as_dict().cloned() else {
            continue;
        };
        // Materialize each inheritable attr the leaf does not already carry.
        for key in ["Resources", "MediaBox", "CropBox", "Rotate"] {
            let name = Name::new(key);
            let has = dict.get(&name).is_some_and(|v| !v.is_null());
            if !has {
                if let Some(val) = inherited_value(doc, leaf, key) {
                    dict.insert(name, val);
                }
            }
        }
        dict.insert(Name::new("Parent"), Object::Reference(pages_ref));
        doc.update_object(leaf, Object::Dictionary(dict))?;
    }

    // Rewrite the root `/Pages` to a flat `/Kids` + `/Count`, dropping any
    // inheritable keys it carried (now materialized onto the leaves).
    let mut root = doc
        .resolve(pages_ref)?
        .as_dict()
        .cloned()
        .ok_or_else(|| Error::xref(0, "root /Pages is not a dictionary"))?;
    root.insert(Name::new("Type"), Object::Name(Name::new("Pages")));
    root.insert(
        Name::new("Kids"),
        Object::Array(leaves.iter().map(|r| Object::Reference(*r)).collect()),
    );
    root.insert(Name::new("Count"), Object::Integer(leaves.len() as i64));
    for key in ["Resources", "MediaBox", "CropBox", "Rotate"] {
        root.remove(&Name::new(key));
    }
    root.remove(&Name::new("Parent"));
    doc.update_object(pages_ref, Object::Dictionary(root))?;
    Ok(pages_ref)
}

/// The catalog `/Pages` reference (it must be an indirect reference so the tree
/// has object identity — a direct `/Pages` dict is rejected, PRD §8.2).
fn pages_root_ref(doc: &DocumentStore) -> Result<ObjRef> {
    let root = doc.root().ok_or_else(|| Error::xref(0, "no /Root"))?;
    let catalog = doc.resolve(root)?;
    let dict = catalog
        .as_dict()
        .ok_or_else(|| Error::xref(0, "/Root is not a dictionary"))?;
    match dict.get(&Name::new("Pages")) {
        Some(Object::Reference(r)) => Ok(*r),
        _ => Err(Error::xref(
            0,
            "catalog /Pages is not an indirect reference",
        )),
    }
}

/// Resolves an inheritable attribute for `leaf` by walking `/Parent` to the
/// root, returning the nearest ancestor's value (ISO 32000-1 §7.7.3.4). Used by
/// [`flatten`] to materialize attributes onto leaves.
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
