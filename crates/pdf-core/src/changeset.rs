//! The [`ChangeSet`] — an overlay of pending edits on a [`DocumentStore`]
//! (PRD §8.7 object-edit API, §9.2 memory model).
//!
//! A `ChangeSet` layers created / updated / deleted indirect objects over the
//! original cross-reference table. Reads ([`DocumentStore::get_object`] /
//! [`DocumentStore::resolve`]) consult the overlay first and fall through to the
//! original on a miss, so a `resolve` performed *after* an `update_object`
//! transparently returns the new value (PRD §8.7). The overlay is the basis for
//! both full save (M3a — replay the whole effective object set) and incremental
//! save (M3b — emit only the changed objects).
//!
//! The set lives behind the document's `RwLock` (interior mutability, PRD §9.2):
//! edits take a brief write lock; reads a shared read lock. Object numbers for
//! newly created objects are allocated past the current maximum so they never
//! collide with an original object (PRD §8.7 "dense renumber is a GC concern;
//! authoring just appends").

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::object::{Dict, ObjRef, Object, StreamData, StreamObj};

/// One pending change to a single indirect object number.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// The object was created or its value replaced (a `Stream` is allowed).
    Set(Arc<Object>),
    /// The object was deleted (freed). A `resolve` of a deleted object yields
    /// `Null`; a full save omits it (its slot becomes free).
    Deleted,
}

/// The pending-edit overlay on a [`crate::DocumentStore`] (PRD §9.2).
///
/// Keyed by object number (generation is always 0 for authored objects, PRD
/// §8.7). Empty immediately after open; [`ChangeSet::is_dirty`] is `false` then.
#[derive(Clone, Debug, Default)]
pub struct ChangeSet {
    /// Pending changes, object number → [`Change`]. `BTreeMap` keeps a
    /// deterministic iteration order for the writer.
    changes: BTreeMap<u32, Change>,
    /// The next object number to hand out from [`ChangeSet::allocate`]. Seeded
    /// from the document's original `/Size` (max obj num + 1) on first use.
    next_free: u32,
    /// Whether `next_free` has been seeded from the document yet.
    seeded: bool,
}

impl ChangeSet {
    /// A fresh, empty change set.
    #[must_use]
    pub fn new() -> Self {
        ChangeSet::default()
    }

    /// Whether any edit is pending (PRD §9.2: `is_dirty`).
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.changes.is_empty()
    }

    /// The pending changes in object-number order — the basis for M3b
    /// incremental save (PRD §9.2).
    #[must_use]
    pub fn changes(&self) -> &BTreeMap<u32, Change> {
        &self.changes
    }

    /// Seeds the allocator's high-water mark from the document's current `/Size`
    /// (max object number + 1) the first time an object is allocated. Idempotent.
    pub(crate) fn seed(&mut self, xref_length: u32) {
        if !self.seeded {
            self.next_free = xref_length.max(1);
            self.seeded = true;
        }
    }

    /// Allocates a fresh object number past the current maximum and records the
    /// new object under it. Returns the allocated reference (generation 0).
    ///
    /// The caller must have [`ChangeSet::seed`]ed the allocator first.
    pub(crate) fn allocate(&mut self, obj: Object) -> ObjRef {
        let num = self.next_free;
        self.next_free = self.next_free.saturating_add(1);
        self.changes.insert(num, Change::Set(Arc::new(obj)));
        ObjRef::new(num, 0)
    }

    /// Records a value replacement (or creation) at an existing object number.
    pub(crate) fn set(&mut self, num: u32, obj: Object) {
        self.changes.insert(num, Change::Set(Arc::new(obj)));
        // A later-allocated object must not reuse this number.
        if self.seeded {
            self.next_free = self.next_free.max(num.saturating_add(1));
        }
    }

    /// Records a stream replacement: a new dict + body. `body` is held as an
    /// owned [`StreamData::Encoded`] payload (already-filtered bytes) or a
    /// [`StreamData::Decoded`] payload (to be deflated on save), per `encoded`.
    pub(crate) fn set_stream(&mut self, num: u32, dict: Dict, body: Vec<u8>, encoded: bool) {
        let data = if encoded {
            StreamData::Encoded(body.into())
        } else {
            StreamData::Decoded(body.into())
        };
        let stream = Object::Stream(StreamObj { dict, data });
        self.set(num, stream);
    }

    /// Records a deletion of object `num`.
    pub(crate) fn delete(&mut self, num: u32) {
        self.changes.insert(num, Change::Deleted);
    }

    /// The pending change for object `num`, if any.
    #[must_use]
    pub(crate) fn get(&self, num: u32) -> Option<&Change> {
        self.changes.get(&num)
    }

    /// One past the highest object number this overlay touches (created, updated,
    /// or reserved by [`ChangeSet::allocate`]), or `0` if it is empty. Lets
    /// [`crate::DocumentStore::xref_length`] reflect freshly-allocated slots that
    /// are not yet in the original cross-reference table (PyMuPDF `get_new_xref`
    /// bumps `/Size` immediately).
    #[must_use]
    pub(crate) fn high_water(&self) -> u32 {
        let by_changes = self
            .changes
            .keys()
            .next_back()
            .map(|n| n.saturating_add(1))
            .unwrap_or(0);
        by_changes.max(self.next_free)
    }
}
