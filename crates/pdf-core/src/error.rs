//! Core error type for `pdf-core` (PRD §9.3).
//!
//! The full `Error` enum sketched in PRD §9.3 grows unit-by-unit. M1a seeded the
//! variants needed by the lexer, object parser and serializer: [`Error::Io`],
//! [`Error::Syntax`], [`Error::UnexpectedEof`] and [`Error::Unsupported`]. M1b
//! adds [`Error::Filter`], [`Error::Decode`] and [`Error::LimitExceeded`] for
//! the stream-filter / codec layer (PRD §8.3, §9.6).
//!
//! Messages are **English-only, stable and machine-greppable** (PRD §9.3): the
//! variant discriminant is the stable `kind`, the `msg` is human prose only.

/// Which resource ceiling was tripped (PRD §9.6.2). The discriminant is stable
/// and machine-greppable; it lets callers branch without parsing prose.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum LimitKind {
    /// A single stream's decoded size exceeded `Limits::max_decompressed_stream`.
    DecompressedStream,
    /// The incremental decode ratio exceeded `Limits::max_decode_ratio`
    /// (decompression-bomb guard, PRD §9.6.2).
    DecodeRatio,
    /// An object stream declared more members than `Limits::max_objstm_objects`
    /// (PRD §9.6.2).
    ObjstmObjects,
    /// A document declared/contained more objects than `Limits::max_objects`
    /// (xref/object-count bomb bound, PRD §9.6.2).
    Objects,
    /// Indirect-object resolution nested deeper than
    /// `Limits::max_recursion_depth` (PRD §9.6.2).
    RecursionDepth,
}

impl LimitKind {
    /// A short, stable discriminant string (machine-greppable, never localized).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LimitKind::DecompressedStream => "decompressed-stream",
            LimitKind::DecodeRatio => "decode-ratio",
            LimitKind::ObjstmObjects => "objstm-objects",
            LimitKind::Objects => "objects",
            LimitKind::RecursionDepth => "recursion-depth",
        }
    }
}

impl std::fmt::Display for LimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `pdf-core` error type. Additional variants (`Xref`, `Crypto`, …) land in
/// later M1 units per PRD §9.3.
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

    /// A stream `/Filter` chain was malformed or referenced an impossible
    /// configuration (e.g. a predicate's `/Columns` mismatch, a bad parms
    /// dict). The `filter` field names the offending filter (stable string).
    #[error("filter error in {filter}: {msg}")]
    Filter {
        /// The filter being applied when the fault occurred (e.g. `"FlateDecode"`).
        filter: &'static str,
        /// Stable English description of what went wrong.
        msg: &'static str,
    },

    /// A codec failed to decode its input (truncated/corrupt deflate, bad LZW
    /// code stream, malformed ASCII85 group, …). The `filter` field names the
    /// codec (stable string).
    #[error("decode error in {filter}: {msg}")]
    Decode {
        /// The codec that failed (e.g. `"FlateDecode"`).
        filter: &'static str,
        /// Stable English description of what went wrong.
        msg: &'static str,
    },

    /// A resource ceiling (PRD §9.6.2) was exceeded — the decompression-bomb /
    /// never-OOM guard. Carries the stable [`LimitKind`] discriminant.
    #[error("limit exceeded: {0}")]
    LimitExceeded(LimitKind),

    /// A bounds-checked read into the backing [`crate::source::Source`] fell
    /// outside the captured length (PRD §9.6.1: every offset/len validated
    /// before slicing; a later truncation can never make us read past the
    /// length captured at open time).
    #[error("source out of bounds: {msg}")]
    Source {
        /// Stable English description (e.g. `"slice past end of source"`).
        msg: &'static str,
    },

    /// The cross-reference machinery (classic table, xref stream, `startxref`
    /// discovery, `/Prev` chain, object-stream container) was malformed in a way
    /// that the non-repairing reader cannot recover from (repair is M1d).
    #[error("xref error at offset {offset}: {msg}")]
    Xref {
        /// Byte offset of the fault (best effort).
        offset: usize,
        /// Stable English description of what went wrong.
        msg: &'static str,
    },

    /// A referenced object has no usable cross-reference entry (free, absent, or
    /// a dangling reference). The "object not found" case (PRD §9.3).
    #[error("missing object {num} {gen}")]
    MissingObject {
        /// Object number that could not be resolved.
        num: u32,
        /// Generation number requested.
        gen: u16,
    },

    /// A reference cycle was detected while resolving an indirect object — the
    /// graph-walk guard (PRD §9.3 / §9.6). `num`/`gen` name the object at which
    /// the cycle closed.
    #[error("reference cycle at object {num} {gen}")]
    ReferenceCycle {
        /// Object number at which the cycle closed.
        num: u32,
        /// Generation number at which the cycle closed.
        gen: u16,
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

    /// Builds an [`Error::Filter`] for `filter` with a stable message.
    #[must_use]
    pub fn filter(filter: &'static str, msg: &'static str) -> Self {
        Error::Filter { filter, msg }
    }

    /// Builds an [`Error::Decode`] for `filter` with a stable message.
    #[must_use]
    pub fn decode(filter: &'static str, msg: &'static str) -> Self {
        Error::Decode { filter, msg }
    }

    /// Builds an [`Error::Source`] with a stable message.
    #[must_use]
    pub fn source(msg: &'static str) -> Self {
        Error::Source { msg }
    }

    /// Builds an [`Error::Xref`] at `offset` with a stable message.
    #[must_use]
    pub fn xref(offset: usize, msg: &'static str) -> Self {
        Error::Xref { offset, msg }
    }

    /// A short, stable discriminant string (machine-greppable, never localized).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::Syntax { .. } => "syntax",
            Error::UnexpectedEof { .. } => "unexpected-eof",
            Error::Filter { .. } => "filter",
            Error::Decode { .. } => "decode",
            Error::LimitExceeded(_) => "limit-exceeded",
            Error::Source { .. } => "source",
            Error::Xref { .. } => "xref",
            Error::MissingObject { .. } => "missing-object",
            Error::ReferenceCycle { .. } => "reference-cycle",
            Error::Unsupported(_) => "unsupported",
        }
    }
}

/// Convenience alias used throughout `pdf-core`.
pub type Result<T> = std::result::Result<T, Error>;
