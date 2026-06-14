//! Typed errors for the security handler (PRD §8.4 / §9.3). Decryption failures
//! must always be typed errors — never panics.

/// A decryption / security-handler error. Messages are stable English prose; the
/// discriminant is the machine-greppable [`CryptoError::kind`].
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CryptoError {
    /// The `/Encrypt` dictionary is structurally invalid or self-inconsistent
    /// (missing `/V`/`/R`, bad `/O`/`/U` length, unknown crypt filter, …).
    #[error("malformed /Encrypt: {0}")]
    Malformed(&'static str),

    /// The security handler / revision / crypt-filter is recognized but not
    /// implemented in this version (e.g. a non-Standard `/Filter`, AES-GCM /
    /// ISO 32003 — *read-tracked, not decrypted* per PRD §8.4).
    #[error("unsupported security handler: {0}")]
    Unsupported(&'static str),

    /// The document is encrypted and not yet authenticated, or the supplied
    /// password is wrong for every role. The clean "wrong password" outcome
    /// (PRD §8.4 — never a panic).
    #[error("password required or incorrect")]
    NeedsPassword,

    /// A cipher operation failed at decrypt time (bad block length, PKCS#7
    /// padding, truncated AES IV). Surfaced per-object; never a panic.
    #[error("decrypt failed: {0}")]
    DecryptFailed(&'static str),
}

impl CryptoError {
    /// A short, stable discriminant string (machine-greppable, never localized).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            CryptoError::Malformed(_) => "crypto-malformed",
            CryptoError::Unsupported(_) => "crypto-unsupported",
            CryptoError::NeedsPassword => "crypto-needs-password",
            CryptoError::DecryptFailed(_) => "crypto-decrypt-failed",
        }
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, CryptoError>;
