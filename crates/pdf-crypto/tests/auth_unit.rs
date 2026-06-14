//! `CRYPT-OWNER-*`, `CRYPT-WRONGPW-*`, `CRYPT-ID-ABSENT-*`, `CRYPT-EXEMPT-001`.

use pdf_crypto::handler::CryptMethod;
use pdf_crypto::kdf::Aes256Hash;
use pdf_crypto::testsupport::{build_aes256, build_r234};
use pdf_crypto::{AuthRole, CryptoError, Decryptor};

#[test]
fn crypt_owner_001_r4_owner_authenticates() {
    let fx = build_r234(
        4,
        16,
        b"id0",
        -44,
        true,
        b"userpw",
        b"ownerpw",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    let role = d.authenticate(b"ownerpw").unwrap();
    assert_eq!(role, AuthRole::Owner);
    // The owner-derived key must decrypt object data identically to the user key.
    let enc = fx.encrypt_string(5, 0, b"payload", None);
    assert_eq!(d.decrypt_string(5, 0, &enc).unwrap(), b"payload");
}

#[test]
fn crypt_owner_002_r6_owner_authenticates() {
    let fx = build_aes256(
        Aes256Hash::R6,
        [0x5Au8; 32],
        -4,
        true,
        b"userpw",
        b"ownerpw",
    );
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    assert_eq!(d.authenticate(b"ownerpw").unwrap(), AuthRole::Owner);
    assert_eq!(d.file_key().unwrap(), &[0x5Au8; 32]);
}

#[test]
fn crypt_owner_003_user_role_reported() {
    let fx = build_r234(
        3,
        16,
        b"id0",
        -44,
        true,
        b"userpw",
        b"ownerpw",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    assert_eq!(d.authenticate(b"userpw").unwrap(), AuthRole::User);
    assert_eq!(d.role(), Some(AuthRole::User));
}

#[test]
fn crypt_wrongpw_001_r4_wrong_pw_typed_error() {
    let fx = build_r234(
        4,
        16,
        b"id0",
        -44,
        true,
        b"userpw",
        b"ownerpw",
        CryptMethod::AesV2,
        CryptMethod::AesV2,
    );
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    assert_eq!(
        d.authenticate(b"not-the-password"),
        Err(CryptoError::NeedsPassword)
    );
    assert!(d.needs_pass());
}

#[test]
fn crypt_wrongpw_002_r6_wrong_pw_typed_error() {
    let fx = build_aes256(
        Aes256Hash::R6,
        [0x5Au8; 32],
        -4,
        true,
        b"userpw",
        b"ownerpw",
    );
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    assert_eq!(d.authenticate(b"wrong"), Err(CryptoError::NeedsPassword));
}

#[test]
fn crypt_wrongpw_003_decrypt_before_auth_errors() {
    let fx = build_r234(
        3,
        16,
        b"id0",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let d = Decryptor::new(fx.config.clone()).unwrap();
    assert_eq!(
        d.decrypt_string(1, 0, b"anything"),
        Err(CryptoError::NeedsPassword)
    );
    assert_eq!(
        d.decrypt_stream(1, 0, b"anything"),
        Err(CryptoError::NeedsPassword)
    );
}

#[test]
fn crypt_id_absent_001_empty_id_roundtrips() {
    // /ID[0] absent → empty byte string fallback; key derivation & decrypt still
    // work (PRD §8.4 /ID-absent fallback).
    let fx = build_r234(
        3,
        16,
        b"",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let enc = fx.encrypt_string(5, 0, b"no-id-doc", None);
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    d.authenticate(b"").unwrap();
    assert_eq!(d.decrypt_string(5, 0, &enc).unwrap(), b"no-id-doc");
}

#[test]
fn crypt_exempt_001_identity_is_noop() {
    // An Identity crypt method returns data verbatim, even authenticated.
    let mut fx = build_r234(
        4,
        16,
        b"id0",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Identity,
        CryptMethod::Identity,
    );
    fx.config.stm_method = CryptMethod::Identity;
    fx.config.str_method = CryptMethod::Identity;
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    d.authenticate(b"").unwrap();
    assert_eq!(
        d.decrypt_string(5, 0, b"verbatim"),
        Ok(b"verbatim".to_vec())
    );
    assert_eq!(
        d.decrypt_stream(5, 0, b"verbatim"),
        Ok(b"verbatim".to_vec())
    );
}
