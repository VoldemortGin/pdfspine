//! `pdf-render` error type (PRD §8.11 / §8.1).
//!
//! Mirrors the typed-error discipline of [`pdf_core::Error`] and
//! [`pdf_image::Error`]: arbitrary, truncated or corrupt input yields a typed
//! [`Error`], **never** a panic. Every scaffolding stub returns
//! [`Error::Unsupported`]; nothing in this crate uses
//! `todo!()`/`unimplemented!()`/`panic!`.

/// The `pdf-render` error type.
///
/// `#[non_exhaustive]` so the parallel M6 implementers may add variants (e.g. a
/// font-program or shading-pattern error) without a breaking change to
/// downstream `match`es.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// A construct that is valid but not implemented in this unit yet. The field
    /// is a stable, machine-greppable discriminant (e.g. `"render_page"`,
    /// `"draw_glyph"`, `"shading"`). All scaffolding stubs return this — it is
    /// the panic-free placeholder the parallel implementers replace.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// A caller-supplied argument violates a documented contract (e.g. a zero or
    /// absurdly large render dimension, a non-invertible matrix).
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),

    /// A resource ceiling (PRD §9.6.2) was exceeded while rasterizing — the
    /// never-OOM guard for render targets (e.g. a page that would allocate an
    /// enormous pixmap). The field is a stable English description.
    #[error("limit exceeded: {0}")]
    LimitExceeded(&'static str),

    /// An error propagated from `pdf-core` while resolving the page / its
    /// resources.
    #[error(transparent)]
    Core(#[from] pdf_core::Error),

    /// An error propagated from `pdf-image` while decoding an image XObject or
    /// constructing the output [`pdf_image::Pixmap`].
    #[error(transparent)]
    Image(#[from] pdf_image::Error),
}

impl Error {
    /// A short, stable discriminant string (machine-greppable, never localized),
    /// matching the `pdf-core` / `pdf-image` convention.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Unsupported(_) => "unsupported",
            Error::InvalidArgument(_) => "invalid-argument",
            Error::LimitExceeded(_) => "limit-exceeded",
            Error::Core(_) => "core",
            Error::Image(_) => "image",
        }
    }
}

/// Convenience alias used throughout `pdf-render`.
pub type Result<T> = std::result::Result<T, Error>;
