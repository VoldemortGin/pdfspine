//! CRYPTO-ERR-* — CryptoError Display + kind() discriminants. PRD §8.4.

use pdf_crypto::CryptoError;

/// CRYPTO-ERR-001 — `Malformed` Display interpolates the inner message.
#[test]
fn crypto_err_malformed_display() {
    assert_eq!(
        CryptoError::Malformed("missing /V").to_string(),
        "malformed /Encrypt: missing /V"
    );
}

/// CRYPTO-ERR-002 — `Unsupported` Display interpolates the inner message.
#[test]
fn crypto_err_unsupported_display() {
    assert_eq!(
        CryptoError::Unsupported("AES-GCM").to_string(),
        "unsupported security handler: AES-GCM"
    );
}

/// CRYPTO-ERR-003 — `NeedsPassword` Display is the fixed prose string.
#[test]
fn crypto_err_needs_password_display() {
    assert_eq!(
        CryptoError::NeedsPassword.to_string(),
        "password required or incorrect"
    );
}

/// CRYPTO-ERR-004 — `DecryptFailed` Display interpolates the inner message.
#[test]
fn crypto_err_decrypt_failed_display() {
    assert_eq!(
        CryptoError::DecryptFailed("bad padding").to_string(),
        "decrypt failed: bad padding"
    );
}

/// CRYPTO-ERR-005 — `kind()` returns the exact discriminant for every variant.
#[test]
fn crypto_err_kind_discriminants() {
    assert_eq!(CryptoError::Malformed("x").kind(), "crypto-malformed");
    assert_eq!(CryptoError::Unsupported("x").kind(), "crypto-unsupported");
    assert_eq!(CryptoError::NeedsPassword.kind(), "crypto-needs-password");
    assert_eq!(
        CryptoError::DecryptFailed("x").kind(),
        "crypto-decrypt-failed"
    );
}

/// CRYPTO-ERR-006 — the `std::error::Error` trait is implemented (thiserror);
/// Display routes through the trait object.
#[test]
fn crypto_err_error_trait_object() {
    let e: &dyn std::error::Error = &CryptoError::NeedsPassword;
    assert_eq!(e.to_string(), "password required or incorrect");
}

/// CRYPTO-ERR-007 — `Clone` + `PartialEq`: a clone equals its source; distinct
/// variants are unequal.
#[test]
fn crypto_err_clone_and_eq() {
    let original = CryptoError::Malformed("dup");
    let cloned = original.clone();
    assert_eq!(cloned, original);
    assert_ne!(CryptoError::NeedsPassword, CryptoError::Malformed("dup"));
}

/// CRYPTO-ERR-008 — `Debug` output is non-empty and names the variant.
#[test]
fn crypto_err_debug_contains_variant() {
    let dbg = format!("{:?}", CryptoError::Malformed("x"));
    assert!(!dbg.is_empty());
    assert!(dbg.contains("Malformed"));
}
