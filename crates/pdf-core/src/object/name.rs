//! PDF name objects (`/Type`, `/Pages`, …) — ISO 32000-1 §7.3.5.
//!
//! A name's value is the **decoded** byte sequence (after resolving `#XX` hex
//! escapes). The leading `/` is a syntactic marker, not part of the value, and
//! `/` alone denotes the empty name.
//!
//! M1a uses a simple owned [`Vec<u8>`] representation. The type is deliberately
//! opaque (construct via [`Name::new`] / [`Name::from_decoded`], read via
//! [`Name::as_bytes`]) so that full cross-document interning can slot in behind
//! the same surface in M1c (PRD §9.2 — "interned") without touching callers.

use std::fmt;

/// A decoded PDF name. Ordered/hashable so it can key a [`super::Dict`]
/// (`BTreeMap`) for deterministic output.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(Vec<u8>);

impl Name {
    /// Creates a name from already-decoded bytes (no `#XX` processing).
    #[must_use]
    pub fn from_decoded(bytes: impl Into<Vec<u8>>) -> Self {
        Name(bytes.into())
    }

    /// Creates a name from a `&str` (its UTF-8 bytes, no `#XX` processing).
    #[must_use]
    pub fn new(s: impl AsRef<str>) -> Self {
        Name(s.as_ref().as_bytes().to_vec())
    }

    /// The decoded name bytes (without the leading `/`).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The name as UTF-8 if it is valid UTF-8 (the common case for PDF names).
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// `true` for the empty name (`/`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Name::new(s)
    }
}

impl From<String> for Name {
    fn from(s: String) -> Self {
        Name(s.into_bytes())
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_str() {
            Some(s) => write!(f, "Name(/{s})"),
            None => write!(f, "Name(/{:?})", self.0),
        }
    }
}
