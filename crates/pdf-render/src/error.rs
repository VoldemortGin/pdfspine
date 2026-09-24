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

/// Contains a panic from third-party rendering code (tiny-skia rasterization,
/// ttf-parser font parsing, ...) at a `pdf-render` public entry point, turning
/// it into [`Error::Unsupported`]`(ctx)` instead of letting it unwind across the
/// PyO3 boundary as `pyo3_runtime.PanicException` (a `BaseException`, not caught
/// by `except Exception`). See ADR 0006. The panic payload is discarded, as in
/// the `pdf-image` codec wrappers; the default panic hook still reports it on
/// stderr.
///
/// # Unwind safety
///
/// `AssertUnwindSafe` is sound here: `pdf-render` holds no shared mutable state
/// (its `FontCache` / `GlyphMaskCache` are locals of each call and are dropped
/// with the unwound frame), the `pdf-fonts` tables are `OnceLock`s that stay
/// unset if their init panics, and the `DocumentStore` `RwLock` guards are held
/// only across a map lookup/insert inside `pdf-core`, never across rendering
/// code; a poisoned lock there degrades to a typed error or a cache miss.
pub(crate) fn contain_render_panic<T>(
    ctx: &'static str,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => Err(Error::Unsupported(ctx)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_render_panic_converts_panic_to_error() {
        let result: Result<()> = contain_render_panic("test: injected panic", || panic!("boom"));
        assert!(matches!(
            result,
            Err(Error::Unsupported("test: injected panic"))
        ));
    }

    #[test]
    fn contain_render_panic_passes_through_results() {
        assert_eq!(contain_render_panic("unused", || Ok(7)).unwrap(), 7);
        let err = contain_render_panic::<()>("unused", || Err(Error::InvalidArgument("bad")));
        assert!(matches!(err, Err(Error::InvalidArgument("bad"))));
    }
}
