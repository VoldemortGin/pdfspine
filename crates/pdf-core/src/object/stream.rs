//! PDF stream objects — ISO 32000-1 §7.3.8.
//!
//! A stream is a dictionary followed by raw bytes between the `stream` and
//! `endstream` keywords. The payload is held **out-of-line** in [`bytes::Bytes`]
//! (PRD §8.1 / §9.2) for O(1) clone.
//!
//! PRD §9.2 sketches three payload variants `Raw{off,len}|Encoded|Decoded`. M1a
//! ships the two owned variants ([`StreamData::Encoded`], [`StreamData::Decoded`]);
//! the source-backed `Raw { offset, len }` variant is deferred to M1c, when the
//! `DocumentStore` / `Source` exist to back it (see §9.2 memory model). The enum
//! is `#[non_exhaustive]` so adding `Raw` later is not a breaking change.

use bytes::Bytes;

use super::Dict;

/// The payload of a [`StreamObj`].
///
/// `Encoded` holds bytes exactly as they appear in the file (still run through
/// the filters named in the stream dict's `/Filter`); `Decoded` holds bytes
/// after those filters have been applied. M1a only ever produces `Encoded`
/// (filters are M1b); `Decoded` exists so the variant set is stable.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum StreamData {
    /// Raw, still-filter-encoded bytes (verbatim from the file).
    Encoded(Bytes),
    /// Bytes after the stream's filters have been applied.
    Decoded(Bytes),
}

impl StreamData {
    /// The underlying bytes regardless of variant.
    #[must_use]
    pub fn bytes(&self) -> &Bytes {
        match self {
            StreamData::Encoded(b) | StreamData::Decoded(b) => b,
        }
    }

    /// Length in bytes of the payload.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    /// `true` when the payload is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes().is_empty()
    }
}

/// A PDF stream object: a [`Dict`] plus its out-of-line [`StreamData`].
#[derive(Clone, Debug, PartialEq)]
pub struct StreamObj {
    /// The stream dictionary (`/Length`, `/Filter`, …).
    pub dict: Dict,
    /// The stream payload.
    pub data: StreamData,
}

impl StreamObj {
    /// Builds a stream from a dict and still-encoded bytes.
    #[must_use]
    pub fn new_encoded(dict: Dict, data: impl Into<Bytes>) -> Self {
        StreamObj {
            dict,
            data: StreamData::Encoded(data.into()),
        }
    }

    /// The raw payload bytes.
    #[must_use]
    pub fn raw_bytes(&self) -> &Bytes {
        self.data.bytes()
    }
}
