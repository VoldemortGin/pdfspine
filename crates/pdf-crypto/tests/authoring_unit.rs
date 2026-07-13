//! `CRYPT-AUTH-*` — the public encryption-authoring API (PRD §8.4 write side).
//!
//! Each authored handler must produce an `/Encrypt` config that the read-side
//! `Decryptor` authenticates with the empty user password and round-trips
//! string/stream data through. Salts/IVs come from a real CSPRNG; AES-256 is
//! authored as R6 only (never R5).

use pdf_crypto::handler::CryptMethod;
use pdf_crypto::{AuthRole, Authoring, Decryptor, EncryptMethod, EncryptSpec};

const STR: &[u8] = b"a secret /Title string value";
const STM: &[u8] = b"stream body bytes: lorem ipsum dolor sit amet";

fn roundtrip(auth: &Authoring) {
    let enc_str = auth.encrypt_string(5, 0, STR);
    let enc_stm = auth.encrypt_stream(6, 0, STM);
    let mut d = Decryptor::new(auth.config().clone()).unwrap();
    assert!(d.needs_pass());
    assert_eq!(d.authenticate(b"").unwrap(), AuthRole::User);
    assert_eq!(d.decrypt_string(5, 0, &enc_str).unwrap(), STR);
    assert_eq!(d.decrypt_stream(6, 0, &enc_stm).unwrap(), STM);
}

#[test]
fn crypt_auth_001_rc4_128() {
    let spec = EncryptSpec::new(EncryptMethod::Rc4_128);
    let auth = Authoring::new(&spec, b"some-id-bytes-01").unwrap();
    assert_eq!(auth.config().v, 2);
    assert_eq!(auth.config().r, 3);
    assert_eq!(auth.config().str_method, CryptMethod::Rc4);
    roundtrip(&auth);
}

#[test]
fn crypt_auth_002_aes128() {
    let spec = EncryptSpec::new(EncryptMethod::Aes128);
    let auth = Authoring::new(&spec, b"some-id-bytes-01").unwrap();
    assert_eq!(auth.config().v, 4);
    assert_eq!(auth.config().r, 4);
    assert_eq!(auth.config().str_method, CryptMethod::AesV2);
    roundtrip(&auth);
}

#[test]
fn crypt_auth_003_aes256_r6() {
    let spec = EncryptSpec::new(EncryptMethod::Aes256R6);
    let auth = Authoring::new(&spec, b"").unwrap();
    assert_eq!(auth.config().v, 5);
    assert_eq!(auth.config().r, 6);
    assert_eq!(auth.config().str_method, CryptMethod::AesV3);
    roundtrip(&auth);
}

#[test]
fn crypt_auth_004_owner_only() {
    // Owner password set, empty user password — authenticating with "" yields
    // User (the empty user password opens the doc).
    let spec = EncryptSpec {
        user_pw: Vec::new(),
        owner_pw: b"theowner".to_vec(),
        permissions: -1,
        method: EncryptMethod::Aes256R6,
    };
    let auth = Authoring::new(&spec, b"").unwrap();
    let mut d = Decryptor::new(auth.config().clone()).unwrap();
    assert_eq!(d.authenticate(b"").unwrap(), AuthRole::User);

    // A doc with a non-empty user password: empty "" fails, owner pw is Owner.
    let spec2 = EncryptSpec {
        user_pw: b"theuser".to_vec(),
        owner_pw: b"theowner".to_vec(),
        permissions: -1,
        method: EncryptMethod::Aes256R6,
    };
    let auth2 = Authoring::new(&spec2, b"").unwrap();
    let mut d2 = Decryptor::new(auth2.config().clone()).unwrap();
    assert!(d2.authenticate(b"").is_err());
    let mut d3 = Decryptor::new(auth2.config().clone()).unwrap();
    assert_eq!(d3.authenticate(b"theowner").unwrap(), AuthRole::Owner);
}

#[test]
fn crypt_auth_009_encryption_descriptor_matches_fitz() {
    // Fitz `metadata["encryption"]` format: `Standard V{V} R{R} {bits}-bit {cipher}`
    // (oracle: PyMuPDF 1.24.14). Bits = key length × 8; cipher from the stream filter.
    for (method, want) in [
        (EncryptMethod::Rc4_128, "Standard V2 R3 128-bit RC4"),
        (EncryptMethod::Aes128, "Standard V4 R4 128-bit AES"),
        (EncryptMethod::Aes256R6, "Standard V5 R6 256-bit AES"),
    ] {
        let spec = EncryptSpec::new(method);
        let auth = Authoring::new(&spec, b"some-id-bytes-01").unwrap();
        let d = Decryptor::new(auth.config().clone()).unwrap();
        assert_eq!(d.encryption_descriptor(), want, "{method:?}");
    }
}

#[test]
fn crypt_auth_010_empty_password_authenticates_is_stateless() {
    // Empty user password → probe is true and stays true (no live-state mutation).
    let auth = Authoring::new(&EncryptSpec::new(EncryptMethod::Rc4_128), b"id0").unwrap();
    let d = Decryptor::new(auth.config().clone()).unwrap();
    assert!(d.empty_password_authenticates());
    // Non-empty user password → probe is false even after a real owner auth
    // (fitz `needs_pass` stays truthy post-auth).
    let spec2 = EncryptSpec {
        user_pw: b"theuser".to_vec(),
        owner_pw: b"theowner".to_vec(),
        permissions: -1,
        method: EncryptMethod::Rc4_128,
    };
    let auth2 = Authoring::new(&spec2, b"id0").unwrap();
    let mut d2 = Decryptor::new(auth2.config().clone()).unwrap();
    assert!(!d2.empty_password_authenticates());
    assert_eq!(d2.authenticate(b"theowner").unwrap(), AuthRole::Owner);
    assert!(
        !d2.empty_password_authenticates(),
        "stateless after real auth"
    );
}

#[test]
fn crypt_auth_005_wrong_password() {
    let spec = EncryptSpec {
        user_pw: b"correct".to_vec(),
        owner_pw: b"owner".to_vec(),
        permissions: -1,
        method: EncryptMethod::Rc4_128,
    };
    let auth = Authoring::new(&spec, b"id0").unwrap();
    let mut d = Decryptor::new(auth.config().clone()).unwrap();
    assert!(d.authenticate(b"definitely-wrong").is_err());
}

#[test]
fn crypt_auth_006_random_salts() {
    // Two authorings of the same AES-256 spec must differ in their /U salt
    // (proves real RNG, not deterministic seeding).
    let spec = EncryptSpec::new(EncryptMethod::Aes256R6);
    let a = Authoring::new(&spec, b"").unwrap();
    let b = Authoring::new(&spec, b"").unwrap();
    assert_ne!(a.config().u, b.config().u);
}

#[test]
fn crypt_auth_007_never_r5() {
    // The only AES-256 method is R6; assert the authored revision is 6, not 5.
    let spec = EncryptSpec::new(EncryptMethod::Aes256R6);
    let auth = Authoring::new(&spec, b"").unwrap();
    assert_eq!(auth.config().r, 6);
    assert_ne!(auth.config().r, 5);
}

#[test]
fn crypt_auth_008_method_mapping() {
    let rc4 = Authoring::new(&EncryptSpec::new(EncryptMethod::Rc4_128), b"id0").unwrap();
    assert_eq!((rc4.config().v, rc4.config().r), (2, 3));
    let aes128 = Authoring::new(&EncryptSpec::new(EncryptMethod::Aes128), b"id0").unwrap();
    assert_eq!((aes128.config().v, aes128.config().r), (4, 4));
    let aes256 = Authoring::new(&EncryptSpec::new(EncryptMethod::Aes256R6), b"").unwrap();
    assert_eq!((aes256.config().v, aes256.config().r), (5, 6));
}
