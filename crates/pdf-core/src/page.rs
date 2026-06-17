//! [`Page`] — a fully-owned, `'static`, `Send + Sync` handle onto one page leaf
//! (PRD §9.2: `Page = { doc: Arc<DocumentStore>, index, page: ObjRef }`).
//!
//! A `Page` carries its own `Arc<DocumentStore>` clone (never a borrow), so it
//! crosses the PyO3 FFI boundary with no lifetimes (PRD §9.4 handle/index
//! pattern). All geometry is returned as [`crate::geom`] value types; the
//! traversal / inheritance logic lives in [`crate::pagetree`].

use std::sync::Arc;

use crate::document::DocumentStore;
use crate::geom::{Matrix, Rect};
use crate::object::{Dict, ObjRef};
use crate::pagetree;

/// One page of a document (PRD §9.2). Cheap to clone (an `Arc` bump + two
/// integers); `'static` and thread-safe.
#[derive(Clone, Debug)]
pub struct Page {
    doc: Arc<DocumentStore>,
    index: usize,
    page: ObjRef,
}

impl Page {
    /// Builds a page handle for the leaf `page` at zero-based `index`.
    #[must_use]
    pub fn new(doc: Arc<DocumentStore>, index: usize, page: ObjRef) -> Self {
        Page { doc, index, page }
    }

    /// The owning document store.
    #[must_use]
    pub fn document(&self) -> &Arc<DocumentStore> {
        &self.doc
    }

    /// The zero-based page index (PyMuPDF `page.number`).
    #[must_use]
    pub fn number(&self) -> usize {
        self.index
    }

    /// The page-leaf object reference.
    #[must_use]
    pub fn obj_ref(&self) -> ObjRef {
        self.page
    }

    /// The raw `/Type /Page` dictionary (references followed), if resolvable.
    #[must_use]
    pub fn dict(&self) -> Option<Dict> {
        pagetree::page_dict(&self.doc, self.page)
    }

    /// The page bound — `CropBox ∩ MediaBox` (PyMuPDF `page.rect`). Defaults to
    /// US Letter when no media box is present (PRD §9.2).
    #[must_use]
    pub fn rect(&self) -> Rect {
        pagetree::bound(&self.doc, self.page)
    }

    /// Alias for [`Page::rect`] (PyMuPDF `page.bound()`).
    #[must_use]
    pub fn bound(&self) -> Rect {
        self.rect()
    }

    /// The effective `/MediaBox` (inherited, normalized; Letter default).
    #[must_use]
    pub fn mediabox(&self) -> Rect {
        pagetree::mediabox(&self.doc, self.page)
    }

    /// The effective `/CropBox` (inherited, clipped to the media box).
    #[must_use]
    pub fn cropbox(&self) -> Rect {
        pagetree::cropbox(&self.doc, self.page)
    }

    /// The normalized `/Rotate` ∈ {0, 90, 180, 270} (inherited).
    #[must_use]
    pub fn rotation(&self) -> i32 {
        pagetree::rotation(&self.doc, self.page)
    }

    /// The effective `/ArtBox` (defaults to the crop box when absent; PyMuPDF
    /// `page.artbox`). Normalized; not clipped to the media box.
    #[must_use]
    pub fn artbox(&self) -> Rect {
        pagetree::artbox(&self.doc, self.page)
    }

    /// The effective `/BleedBox` (defaults to the crop box; PyMuPDF
    /// `page.bleedbox`). Normalized; not clipped.
    #[must_use]
    pub fn bleedbox(&self) -> Rect {
        pagetree::bleedbox(&self.doc, self.page)
    }

    /// The effective `/TrimBox` (defaults to the crop box; PyMuPDF
    /// `page.trimbox`). Normalized; not clipped.
    #[must_use]
    pub fn trimbox(&self) -> Rect {
        pagetree::trimbox(&self.doc, self.page)
    }

    /// The page-to-fitz transformation matrix (PyMuPDF
    /// `page.transformation_matrix`).
    #[must_use]
    pub fn transformation_matrix(&self) -> Matrix {
        pagetree::transformation_matrix(&self.doc, self.page)
    }

    /// The page rotation matrix (PyMuPDF `page.rotation_matrix`).
    #[must_use]
    pub fn rotation_matrix(&self) -> Matrix {
        pagetree::rotation_matrix(&self.doc, self.page)
    }

    /// The inverse rotation matrix (PyMuPDF `page.derotation_matrix`).
    #[must_use]
    pub fn derotation_matrix(&self) -> Matrix {
        pagetree::derotation_matrix(&self.doc, self.page)
    }

    /// The page-leaf object number (PyMuPDF `page.xref`).
    #[must_use]
    pub fn xref(&self) -> u32 {
        self.page.num
    }
}
