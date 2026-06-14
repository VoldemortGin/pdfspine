//! PDF stream objects — ISO 32000-1 §7.3.8.
//!
//! A stream is a dictionary followed by raw bytes between the `stream` and
//! `endstream` keywords. The payload is held **out-of-line** in [`bytes::Bytes`]
//! (PRD §8.1 / §9.2) for O(1) clone.
//!
//! PRD §9.2 sketches three payload variants `Raw{off,len}|Encoded|Decoded`. M1a
//! shipped the two owned variants ([`StreamData::Encoded`],
//! [`StreamData::Decoded`]); M1c adds the source-backed
//! [`StreamData::Raw`]`{ offset, len }` variant now that the `DocumentStore` /
//! `Source` exist to back it (see §9.2 memory model). The enum is
//! `#[non_exhaustive]` so adding variants is not a breaking change.

use bytes::Bytes;

use super::Dict;

/// The payload of a [`StreamObj`].
///
/// - [`StreamData::Raw`] is the lazy, source-backed variant: it records only the
///   `offset`/`len` of the still-filter-encoded body within the document's
///   [`crate::source::Source`]; the bytes are sliced on demand (PRD §9.2). This
///   is what the object parser produces when reading from a `DocumentStore`.
/// - [`StreamData::Encoded`] holds owned bytes exactly as they appear in the
///   file (still run through the filters named in `/Filter`).
/// - [`StreamData::Decoded`] holds owned bytes after those filters were applied.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum StreamData {
    /// A still-encoded body, recorded as an `(offset, len)` slice into the
    /// document `Source` and materialized lazily (PRD §9.2 memory model).
    Raw {
        /// Absolute byte offset of the stream body within the `Source`.
        offset: usize,
        /// Byte length of the stream body.
        len: usize,
    },
    /// Raw, still-filter-encoded bytes (verbatim from the file).
    Encoded(Bytes),
    /// Bytes after the stream's filters have been applied.
    Decoded(Bytes),
}

impl StreamData {
    /// The underlying owned bytes, if this payload is materialized
    /// ([`StreamData::Encoded`] / [`StreamData::Decoded`]). A
    /// [`StreamData::Raw`] payload has no owned bytes (it must be sliced from the
    /// `Source` first) and returns `None`.
    #[must_use]
    pub fn owned_bytes(&self) -> Option<&Bytes> {
        match self {
            StreamData::Encoded(b) | StreamData::Decoded(b) => Some(b),
            StreamData::Raw { .. } => None,
        }
    }

    /// The underlying owned bytes regardless of variant.
    ///
    /// # Panics
    ///
    /// Panics on a [`StreamData::Raw`] payload — callers that may hold a `Raw`
    /// stream must go through the `DocumentStore` (which slices the body from the
    /// `Source`) or use [`StreamData::owned_bytes`]. Kept for the owned-only
    /// call sites that predate the `Raw` variant.
    #[must_use]
    pub fn bytes(&self) -> &Bytes {
        match self {
            StreamData::Encoded(b) | StreamData::Decoded(b) => b,
            StreamData::Raw { .. } => {
                panic!("StreamData::bytes() called on a source-backed Raw payload")
            }
        }
    }

    /// Length in bytes of the payload (the recorded `len` for [`StreamData::Raw`]).
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            StreamData::Raw { len, .. } => *len,
            StreamData::Encoded(b) | StreamData::Decoded(b) => b.len(),
        }
    }

    /// `true` when the payload is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

    /// The raw payload bytes for a materialized ([`StreamData::Encoded`] /
    /// [`StreamData::Decoded`]) stream.
    ///
    /// # Panics
    ///
    /// Panics on a source-backed [`StreamData::Raw`] payload — resolve such
    /// streams through the `DocumentStore` (which slices the body from the
    /// `Source`) first. Use [`StreamData::owned_bytes`] for a non-panicking path.
    #[must_use]
    pub fn raw_bytes(&self) -> &Bytes {
        self.data.bytes()
    }

    /// Decodes this stream's payload through its `/Filter` chain (PRD §8.3),
    /// returning a [`DecodeOutcome`]. Decoding is **lazy / opt-in**: the stream
    /// itself still holds the original (`Encoded`) bytes — call
    /// [`StreamObj::decoded`] to obtain a copy with a `Decoded` payload.
    ///
    /// An [`StreamData::Decoded`] payload is treated as already-decoded and
    /// returned verbatim.
    ///
    /// # Errors
    ///
    /// Propagates any [`crate::Error`] from the filter chain (unknown filter,
    /// bad parms, decode failure, limit exceeded).
    pub fn decode(
        &self,
        limits: &crate::limits::Limits,
    ) -> crate::Result<crate::filters::DecodeOutcome> {
        match &self.data {
            StreamData::Decoded(b) => Ok(crate::filters::DecodeOutcome::Decoded(b.to_vec())),
            StreamData::Encoded(b) => crate::filters::decode_stream(&self.dict, b, limits),
            // A source-backed body must be materialized via the `DocumentStore`
            // before it can be decoded; `decode` operates on owned payloads only.
            StreamData::Raw { .. } => Err(crate::Error::Unsupported(
                "decode on a source-backed Raw stream; resolve via DocumentStore",
            )),
        }
    }

    /// Returns a clone of this stream whose payload has been replaced with its
    /// decoded bytes ([`StreamData::Decoded`]) — the lazy `Decoded`-production
    /// path (PRD §9.2). If the chain ends at an image-only filter
    /// ([`crate::filters::DecodeOutcome::ImageEncoded`]) the stream is returned
    /// **unchanged** (still `Encoded`), since those codecs land in M5.
    ///
    /// # Errors
    ///
    /// Propagates any decode error from [`StreamObj::decode`].
    pub fn decoded(&self, limits: &crate::limits::Limits) -> crate::Result<StreamObj> {
        match self.decode(limits)? {
            crate::filters::DecodeOutcome::Decoded(bytes) => Ok(StreamObj {
                dict: self.dict.clone(),
                data: StreamData::Decoded(Bytes::from(bytes)),
            }),
            crate::filters::DecodeOutcome::ImageEncoded { .. } => Ok(self.clone()),
        }
    }
}
