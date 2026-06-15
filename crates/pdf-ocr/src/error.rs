//! `pdf-ocr` error type (PRD §8.1 discipline).
//!
//! Mirrors the typed-error convention of the sibling crates: arbitrary input,
//! a missing engine, or a failed recognition yields a typed [`Error`], **never**
//! a panic. The stable [`Error::kind`] discriminant drives the `pdf-api` ->
//! Python exception mapping; a missing/unusable OCR engine maps to `unsupported`
//! (PyMuPDF raises on the same condition).

/// The `pdf-ocr` error type.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// The OCR engine is unavailable or unusable: the `tesseract` binary could
    /// not be located, failed to launch, or reported a fatal error (e.g. a
    /// missing language pack). The field is a stable English description. Maps to
    /// `PdfUnsupportedError`, matching PyMuPDF, which raises when Tesseract is
    /// absent.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// A caller-supplied argument violates a documented contract (e.g. a
    /// non-positive DPI).
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),

    /// An I/O failure writing the temporary input image or reading the engine
    /// output.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// An error propagated from `pdf-core` (page resolution / save).
    #[error(transparent)]
    Core(#[from] pdf_core::Error),

    /// An error propagated from `pdf-render` while rasterizing the page.
    #[error(transparent)]
    Render(#[from] pdf_render::Error),

    /// An error propagated from `pdf-image` (Pixmap -> PNG, image-document ->
    /// PDF).
    #[error(transparent)]
    Image(#[from] pdf_image::Error),
}

impl Error {
    /// A short, stable discriminant string (machine-greppable, never localized),
    /// matching the sibling-crate convention.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Unsupported(_) => "unsupported",
            Error::InvalidArgument(_) => "invalid-argument",
            Error::Io(_) => "io",
            Error::Core(_) => "core",
            Error::Render(_) => "render",
            Error::Image(_) => "image",
        }
    }
}

/// Convenience alias used throughout `pdf-ocr`.
pub type Result<T> = std::result::Result<T, Error>;
