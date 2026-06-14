//! Backing bytes for a parsed document — the `Source` abstraction (PRD §9.2,
//! §9.6.1).
//!
//! A [`Source`] owns (or, in a future build, memory-maps) the raw file bytes and
//! is the single authority for every byte the parser touches. **All** access is
//! through **bounds-checked** helpers: an out-of-range offset/len yields a typed
//! [`Error::Source`] instead of a panic or UB (PRD §9.6.1: "offsets validated
//! before slicing"). The length captured at construction is the authority — the
//! reader never re-queries the OS for a length, so a later truncation can never
//! make us read past the original end.
//!
//! # mmap and `#![forbid(unsafe_code)]` (PRD §9.6.1)
//!
//! `pdf-core` is `#![forbid(unsafe_code)]` *crate-wide* (PRD §9.6). Real
//! `memmap2` mapping is fundamentally `unsafe` (a concurrent truncation can
//! fault — the named "#1-gate" UB vector). A crate-level `forbid` **cannot** be
//! lifted by an inner `#[allow(unsafe_code)]`, so honoring the safety story
//! *and* offering mmap would require either (a) moving the mmap into a separate,
//! `allow(unsafe_code)` crate, or (b) demoting `forbid` to `deny` here. Both are
//! larger changes than M1c warrants.
//!
//! **Decision (correctness first):** the default and only built path is owned
//! [`Bytes`] — the documented hard-safe `mmap: Never` mode (PRD §9.6.1 point 3),
//! which is exactly the recommended mode for untrusted inputs. The
//! [`MmapMode`] knob and an `mmap` cargo feature exist as the seam; with the
//! feature off (the default) *every* mode reads owned bytes, so the crate stays
//! genuinely unsafe-free. Wiring a real mmap behind that feature in its own
//! `allow(unsafe_code)` module is left as a documented follow-up.

use std::path::Path;

use bytes::Bytes;

use crate::error::{Error, Result};

/// How a path-backed [`Source`] should obtain its bytes (PRD §9.6.1).
///
/// `Never` is the **hard-safe** mode: the file is read fully into owned
/// [`Bytes`], so there is no live mapping that a concurrent truncation could
/// fault. It is the recommended mode for untrusted/volatile inputs and the
/// behavior selected by `OpenOptions::untrusted()`. `Auto` *requests* a memory
/// map where available; in the current build (no `mmap` feature) it falls back
/// to the same owned-bytes read, so the two modes are observationally identical.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum MmapMode {
    /// Prefer a memory map if the build supports it; otherwise read owned bytes.
    Auto,
    /// Never memory-map: read the whole file into owned [`Bytes`] (hard-safe).
    #[default]
    Never,
}

/// The raw bytes behind a document.
///
/// Cheaply clonable: [`Source::Owned`] holds a refcounted [`Bytes`], and the
/// empty source is trivial. Every read goes through [`Source::bytes`] +
/// bounds-checked [`Source::slice`] / [`Source::byte_at`].
#[derive(Clone, Debug)]
pub enum Source {
    /// Owned, refcounted bytes (the default / hard-safe path, PRD §9.6.1).
    Owned(Bytes),
    /// An empty source (zero bytes). Reads of any nonzero length error.
    Empty,
}

impl Source {
    /// Builds an owned source from any byte container.
    #[must_use]
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        let b = bytes.into();
        if b.is_empty() {
            Source::Empty
        } else {
            Source::Owned(b)
        }
    }

    /// Reads a file into a [`Source`] using `mode`.
    ///
    /// In the current build both [`MmapMode`] variants read the whole file into
    /// owned [`Bytes`] (the hard-safe path); see the module docs for the mmap
    /// rationale.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the file cannot be read.
    pub fn open(path: impl AsRef<Path>, _mode: MmapMode) -> Result<Self> {
        let bytes = std::fs::read(path.as_ref())?;
        Ok(Source::from_bytes(bytes))
    }

    /// The full backing slice (length captured at construction; never re-queried).
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Source::Owned(b) => b.as_ref(),
            Source::Empty => &[],
        }
    }

    /// Total length in bytes — the authority for every bounds check (PRD §9.6.1).
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    /// `true` when the source has no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bounds-checked subslice `[offset, offset+len)`.
    ///
    /// # Errors
    ///
    /// [`Error::Source`] when `offset + len` overflows or lies past the end —
    /// never a panic (PRD §9.6.1).
    pub fn slice(&self, offset: usize, len: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| Error::source("slice length overflow"))?;
        self.bytes()
            .get(offset..end)
            .ok_or_else(|| Error::source("slice past end of source"))
    }

    /// Bounds-checked subslice as refcounted [`Bytes`] (zero-copy on `Owned`).
    ///
    /// # Errors
    ///
    /// [`Error::Source`] when the range is invalid (see [`Source::slice`]).
    pub fn slice_bytes(&self, offset: usize, len: usize) -> Result<Bytes> {
        // Validate first (also covers `Empty`).
        let _ = self.slice(offset, len)?;
        match self {
            Source::Owned(b) => Ok(b.slice(offset..offset + len)),
            Source::Empty => Ok(Bytes::new()),
        }
    }

    /// Bounds-checked tail from `offset` to the end.
    ///
    /// # Errors
    ///
    /// [`Error::Source`] when `offset` is past the end.
    pub fn slice_from(&self, offset: usize) -> Result<&[u8]> {
        self.bytes()
            .get(offset..)
            .ok_or_else(|| Error::source("offset past end of source"))
    }

    /// Bounds-checked single byte at `offset`.
    ///
    /// # Errors
    ///
    /// [`Error::Source`] when `offset` is past the end.
    pub fn byte_at(&self, offset: usize) -> Result<u8> {
        self.bytes()
            .get(offset)
            .copied()
            .ok_or_else(|| Error::source("byte offset past end of source"))
    }
}
