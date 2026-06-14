//! `CRYPT-{RC4,AESV2,AESV3,R5}-*` / `CRYPT-PEROBJ-004` — full encrypt→authenticate
//! →decrypt round-trips for every scheme, using self-generated fixtures.

use pdf_crypto::handler::CryptMethod;
use pdf_crypto::kdf::Aes256Hash;
use pdf_crypto::testsupport::{build_aes256, build_r234, Fixture};
use pdf_crypto::{AuthRole, Decryptor};

const STR: &[u8] = b"a secret /Title string value";
const STM: &[u8] = b"stream body bytes: lorem ipsum dolor sit amet, the quick brown fox";

/// Encrypt a string and a stream for object (5,0) / (6,0), reopen via a fresh
/// `Decryptor`, authenticate with the empty user password, and assert equality.
fn roundtrip_empty_pwd(fx: &Fixture) {
    let enc_str = fx.encrypt_string(5, 0, STR, None);
    let enc_stm = fx.encrypt_stream(6, 0, STM, None);

    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    assert!(d.needs_pass());
    let role = d.authenticate(b"").unwrap();
    assert_eq!(role, AuthRole::User);
    assert!(!d.needs_pass());

    assert_eq!(d.decrypt_string(5, 0, &enc_str).unwrap(), STR);
    assert_eq!(d.decrypt_stream(6, 0, &enc_stm).unwrap(), STM);
}

#[test]
fn crypt_rc4_40_001() {
    let fx = build_r234(
        2,
        5,
        b"id-bytes-0",
        -1,
        true,
        b"",
        b"owner",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    roundtrip_empty_pwd(&fx);
}

#[test]
fn crypt_rc4_128_001() {
    let fx = build_r234(
        3,
        16,
        b"id-bytes-0",
        -44,
        true,
        b"",
        b"owner",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    roundtrip_empty_pwd(&fx);
}

#[test]
fn crypt_rc4_128_002_crypt_filters() {
    // R4 with V2 (RC4) crypt filters for both streams and strings.
    let fx = build_r234(
        4,
        16,
        b"id-bytes-0",
        -3904,
        true,
        b"",
        b"owner",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    roundtrip_empty_pwd(&fx);
}

#[test]
fn crypt_aesv2_001() {
    let fx = build_r234(
        4,
        16,
        b"id-bytes-0",
        -44,
        true,
        b"",
        b"owner",
        CryptMethod::AesV2,
        CryptMethod::AesV2,
    );
    roundtrip_empty_pwd(&fx);
}

#[test]
fn crypt_aesv2_002_distinct_objects_distinct_keys() {
    let fx = build_r234(
        4,
        16,
        b"id-bytes-0",
        -44,
        true,
        b"",
        b"owner",
        CryptMethod::AesV2,
        CryptMethod::AesV2,
    );
    // Same plaintext, same IV, different object numbers → different ciphertext
    // bodies (per-object key differs).
    let iv = Some([0x11u8; 16]);
    let a = fx.encrypt_stream(10, 0, STM, iv);
    let b = fx.encrypt_stream(11, 0, STM, iv);
    assert_ne!(a, b);
    // And both decrypt correctly under their own object numbers.
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    d.authenticate(b"").unwrap();
    assert_eq!(d.decrypt_stream(10, 0, &a).unwrap(), STM);
    assert_eq!(d.decrypt_stream(11, 0, &b).unwrap(), STM);
}

#[test]
fn crypt_aesv3_r6_001() {
    let fx = build_aes256(Aes256Hash::R6, [0x5Au8; 32], -4, true, b"", b"owner-pw");
    roundtrip_empty_pwd(&fx);
}

#[test]
fn crypt_aesv3_r6_002_nonempty_user_pwd() {
    let fx = build_aes256(
        Aes256Hash::R6,
        [0x77u8; 32],
        -4,
        true,
        b"hunter2",
        b"owner-pw",
    );
    let enc = fx.encrypt_string(5, 0, STR, None);
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    assert!(
        d.authenticate(b"").is_err(),
        "empty pwd must NOT authenticate"
    );
    assert_eq!(d.authenticate(b"hunter2").unwrap(), AuthRole::User);
    assert_eq!(d.decrypt_string(5, 0, &enc).unwrap(), STR);
}

#[test]
fn crypt_perobj_004_aesv3_uses_file_key_directly() {
    // For AESV3 the object number must NOT affect the key — same plaintext + IV
    // under two object numbers gives identical ciphertext.
    let fx = build_aes256(Aes256Hash::R6, [0x33u8; 32], -4, true, b"", b"o");
    let iv = Some([0x22u8; 16]);
    let a = fx.encrypt_stream(10, 0, STM, iv);
    let b = fx.encrypt_stream(99, 7, STM, iv);
    assert_eq!(a, b, "AESV3 ignores num/gen — file key used directly");
    let mut d = Decryptor::new(fx.config.clone()).unwrap();
    d.authenticate(b"").unwrap();
    assert_eq!(d.decrypt_stream(10, 0, &a).unwrap(), STM);
    assert_eq!(d.decrypt_stream(99, 7, &b).unwrap(), STM);
}

#[test]
fn crypt_r5_001_transitional_roundtrip() {
    // R5 read path (never written) still decrypts.
    let fx = build_aes256(Aes256Hash::R5, [0x42u8; 32], -4, true, b"", b"owner");
    roundtrip_empty_pwd(&fx);
}
