//! Crate-wide [`Name`] interning (PRD §9.2 — `NameInterner`).
//!
//! PDF documents reuse a small set of name keys (`/Type`, `/Pages`,
//! `/Contents`, …) thousands of times. The [`DocumentStore`](crate::document)
//! holds one `NameInterner` so that each distinct name's bytes are stored once
//! and repeated lookups are cheap, without changing the public [`Name`] type.
//!
//! # Approach (keeps M1a's `Name` API stable)
//!
//! M1a's [`Name`] is an owned `Vec<u8>` with an opaque API (`from_decoded` /
//! `as_bytes`). The note in PRD §9.2 explicitly permits "keep `Name` owned + add
//! an interner used by the store," which is the path taken here: `Name` is
//! **unchanged** (no caller touched), and `NameInterner` is a thin de-dup pool
//! that returns a shared [`Name`] for equal byte sequences. This avoids churning
//! every M1a/M1b call site while still giving the store a single place that owns
//! the canonical name set (and an obvious upgrade seam to a symbol id later).

use std::collections::HashSet;

use crate::object::Name;

/// A de-duplicating pool of [`Name`]s (PRD §9.2).
///
/// [`NameInterner::intern`] returns a clone of the canonical [`Name`] for a
/// given byte sequence, inserting it on first sight. The pool's `len` is the
/// number of *distinct* names seen — useful for tests asserting de-dup.
#[derive(Clone, Debug, Default)]
pub struct NameInterner {
    pool: HashSet<Name>,
}

impl NameInterner {
    /// A fresh, empty interner.
    #[must_use]
    pub fn new() -> Self {
        NameInterner::default()
    }

    /// Returns the canonical [`Name`] for `name`'s bytes, inserting it if unseen.
    ///
    /// The returned value compares equal to `name`; repeated calls with equal
    /// bytes return clones of the same pooled entry.
    pub fn intern(&mut self, name: &Name) -> Name {
        if let Some(existing) = self.pool.get(name) {
            return existing.clone();
        }
        self.pool.insert(name.clone());
        name.clone()
    }

    /// Interns directly from decoded bytes (convenience).
    pub fn intern_bytes(&mut self, bytes: &[u8]) -> Name {
        let name = Name::from_decoded(bytes.to_vec());
        self.intern(&name)
    }

    /// The number of distinct names pooled.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    /// `true` when nothing has been interned yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }
}
