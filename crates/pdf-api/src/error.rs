//! The unified `pdf-api` error type (PRD §9.3).
//!
//! `pdf-api` is the single façade the bindings depend on; it **flattens** the
//! focused sub-crate error enums into one `Error` whose stable `kind()`
//! discriminant drives the Rust→Python exception mapping (PRD §9.3). The
//! discriminant strings match `pdf_core::Error::kind` so downstream code (and the
//! PyO3 layer) can branch without parsing prose.

use std::fmt;

/// The unified oxide-pdf error (PRD §9.3). Wraps the core error plus the I/O and
/// password cases the document-open surface needs.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A file-system failure opening the document path.
    Io(std::io::Error),
    /// A lexical / grammatical / xref / structural violation (maps to
    /// `PdfSyntaxError`).
    Syntax(String),
    /// The document is encrypted and needs a password, or a supplied password
    /// was wrong (maps to `PdfPasswordError`).
    Password(String),
    /// A valid-PDF construct not yet implemented, or a deferred surface (maps to
    /// `PdfUnsupportedError`).
    Unsupported(String),
    /// A filter / codec failed to decode (maps to `PdfDecodeError`).
    Decode(String),
    /// A resource ceiling was exceeded — the never-OOM guard (maps to
    /// `PdfLimitError`).
    Limit(String),
    /// A redaction could not be applied safely (e.g. an undecodable image under
    /// the rect) — fail-closed (maps to `PdfRedactionError`, PRD §8.8 / §9.3).
    Redaction(String),
}

impl Error {
    /// A short, stable discriminant string (machine-greppable, never localized).
    /// Mirrors [`pdf_core::Error::kind`] so the PyO3 mapping is one switch.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::Syntax(_) => "syntax",
            Error::Password(_) => "password",
            Error::Unsupported(_) => "unsupported",
            Error::Decode(_) => "decode",
            Error::Limit(_) => "limit",
            Error::Redaction(_) => "redaction",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Syntax(m) => write!(f, "{m}"),
            Error::Password(m) => write!(f, "{m}"),
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
            Error::Decode(m) => write!(f, "{m}"),
            Error::Limit(m) => write!(f, "limit exceeded: {m}"),
            Error::Redaction(m) => write!(f, "redaction failed: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<pdf_core::Error> for Error {
    fn from(e: pdf_core::Error) -> Self {
        use pdf_core::Error as C;
        match e {
            C::Io(io) => Error::Io(io),
            C::Decode { .. } | C::Filter { .. } => Error::Decode(e.to_string()),
            C::LimitExceeded(_) => Error::Limit(e.to_string()),
            C::Unsupported(m) => Error::Unsupported(m.to_string()),
            C::Redaction(m) => Error::Redaction(m.to_string()),
            #[cfg(feature = "encryption")]
            C::NeedsPassword(_) | C::Crypto(_) => Error::Password(e.to_string()),
            // Lexical / structural / xref / object faults all surface as syntax.
            _ => Error::Syntax(e.to_string()),
        }
    }
}

impl From<pdf_image::Error> for Error {
    fn from(e: pdf_image::Error) -> Self {
        use pdf_image::Error as I;
        let msg = e.to_string();
        match e {
            I::Unsupported(_) => Error::Unsupported(msg),
            I::Decode { .. } => Error::Decode(msg),
            // A non-image / bad-argument input maps to `PdfUnsupportedError`
            // (PRD §3.2 #2 / §8.10): an unsupported input, not a syntax fault.
            I::InvalidArgument(_) => Error::Unsupported(msg),
            I::LimitExceeded(_) => Error::Limit(msg),
            I::Core(c) => Error::from(c),
            // `pdf_image::Error` is `#[non_exhaustive]`; any future variant maps
            // to a decode failure (the conservative §8.4.1 default).
            _ => Error::Decode(msg),
        }
    }
}

/// Convenience alias used throughout `pdf-api`.
pub type Result<T> = std::result::Result<T, Error>;
