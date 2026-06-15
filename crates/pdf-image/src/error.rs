//! `pdf-image` error type (PRD §8.4.1 degradation contract / §8.10).
//!
//! Mirrors the typed-error discipline of [`pdf_core::Error`]: arbitrary,
//! truncated or corrupt image input yields a typed [`Error`], **never** a panic
//! (PRD §8.1 / §8.4.1). The codec stubs return [`Error::Unsupported`] until M5
//! implementation lands; nothing in this crate uses `todo!()`/`unimplemented!()`.

/// The `pdf-image` error type.
///
/// `#[non_exhaustive]` so the parallel M5 implementers may add variants (e.g. a
/// resource-cap variant) without a breaking change to downstream `match`es.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// A construct that is valid but not implemented in this unit yet. The field
    /// is a stable, machine-greppable discriminant (e.g. `"JBIG2Decode"`,
    /// `"imagedoc"`). All scaffolding stubs return this — it is the panic-free
    /// placeholder the parallel implementers replace.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// A codec failed to decode its input (truncated/corrupt stream, unsupported
    /// region type per the §8.4.1 documented subset, …). The `codec` field names
    /// the failing codec (stable string), `msg` is a stable English reason.
    /// This is the typed "decode failed for THIS image" signal of the §8.4.1
    /// degradation contract — text extraction continues independently.
    #[error("decode error in {codec}: {msg}")]
    Decode {
        /// The codec that failed (e.g. `"DCTDecode"`).
        codec: &'static str,
        /// Stable English description of what went wrong.
        msg: &'static str,
    },

    /// A caller-supplied argument violates a documented contract (e.g. a
    /// non-image input to `convert_to_pdf`, an out-of-range component count).
    /// Surfaced to Python as the base error / `PdfUnsupportedError` per context.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),

    /// A resource ceiling (PRD §9.6.2 / §8.4.1) was exceeded while decoding —
    /// the decompression-bomb / never-OOM guard for image codecs. The field is a
    /// stable English description of the limit hit.
    #[error("limit exceeded: {0}")]
    LimitExceeded(&'static str),

    /// An error propagated from `pdf-core` while resolving a stream / dict (e.g.
    /// resolving an image XObject's `/DecodeParms` or its underlying object).
    #[error(transparent)]
    Core(#[from] pdf_core::Error),
}

impl Error {
    /// Builds an [`Error::Decode`] for `codec` with a stable message.
    #[must_use]
    pub fn decode(codec: &'static str, msg: &'static str) -> Self {
        Error::Decode { codec, msg }
    }

    /// A short, stable discriminant string (machine-greppable, never localized),
    /// matching the `pdf-core` convention.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Unsupported(_) => "unsupported",
            Error::Decode { .. } => "decode",
            Error::InvalidArgument(_) => "invalid-argument",
            Error::LimitExceeded(_) => "limit-exceeded",
            Error::Core(_) => "core",
        }
    }
}

/// Convenience alias used throughout `pdf-image`.
pub type Result<T> = std::result::Result<T, Error>;
