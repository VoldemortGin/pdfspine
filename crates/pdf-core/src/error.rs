//! Core error type for `pdf-core` (PRD §9.3).
//!
//! The full `Error` enum sketched in PRD §9.3 grows unit-by-unit. M1a seeds the
//! variants needed by the lexer, object parser and serializer: [`Error::Io`],
//! [`Error::Syntax`], [`Error::UnexpectedEof`] and [`Error::Unsupported`].
//!
//! Messages are **English-only, stable and machine-greppable** (PRD §9.3): the
//! variant discriminant is the stable `kind`, the `msg` is human prose only.

/// The `pdf-core` error type. Additional variants (`Xref`, `Filter`, `Decode`,
/// `Crypto`, …) land in later M1 units per PRD §9.3.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// An underlying I/O failure (file read, etc.).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A lexical or grammatical violation at a known byte offset.
    #[error("syntax error at offset {offset}: {msg}")]
    Syntax {
        /// Absolute byte offset (within the buffer being parsed) of the fault.
        offset: usize,
        /// Stable English description of what went wrong.
        msg: &'static str,
    },

    /// Input ended while a token or object was still being assembled.
    #[error("unexpected end of input at offset {offset}")]
    UnexpectedEof {
        /// Byte offset at which the input ran out.
        offset: usize,
    },

    /// A construct that is valid PDF but not yet implemented in this unit.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
}

impl Error {
    /// Builds a [`Error::Syntax`] at `offset` with a stable message.
    #[must_use]
    pub fn syntax(offset: usize, msg: &'static str) -> Self {
        Error::Syntax { offset, msg }
    }

    /// Builds a [`Error::UnexpectedEof`] at `offset`.
    #[must_use]
    pub fn eof(offset: usize) -> Self {
        Error::UnexpectedEof { offset }
    }

    /// A short, stable discriminant string (machine-greppable, never localized).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::Syntax { .. } => "syntax",
            Error::UnexpectedEof { .. } => "unexpected-eof",
            Error::Unsupported(_) => "unsupported",
        }
    }
}

/// Convenience alias used throughout `pdf-core`.
pub type Result<T> = std::result::Result<T, Error>;
