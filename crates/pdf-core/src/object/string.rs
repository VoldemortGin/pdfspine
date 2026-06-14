//! PDF string objects — ISO 32000-1 §7.3.4.
//!
//! A PDF string is a raw byte sequence (never text). Two syntactic forms exist:
//! literal `( … )` and hexadecimal `< … >`. We retain the original [`StringKind`]
//! so the serializer can prefer a sensible canonical form, but the *value* is
//! always the decoded bytes regardless of kind.

/// Which syntactic form a [`PdfString`] was written in (ISO 32000-1 §7.3.4).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StringKind {
    /// Literal form `( … )` with escapes.
    Literal,
    /// Hexadecimal form `< … >`.
    Hex,
}

/// A PDF string: decoded raw bytes plus the form it was parsed from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfString {
    /// Decoded raw bytes (escapes / hex already resolved).
    pub bytes: Vec<u8>,
    /// The syntactic form (literal vs hex).
    pub kind: StringKind,
}

impl PdfString {
    /// A literal-form string from decoded bytes.
    #[must_use]
    pub fn literal(bytes: impl Into<Vec<u8>>) -> Self {
        PdfString {
            bytes: bytes.into(),
            kind: StringKind::Literal,
        }
    }

    /// A hex-form string from decoded bytes.
    #[must_use]
    pub fn hex(bytes: impl Into<Vec<u8>>) -> Self {
        PdfString {
            bytes: bytes.into(),
            kind: StringKind::Hex,
        }
    }

    /// The decoded bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}
