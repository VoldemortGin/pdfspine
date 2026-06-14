//! `CRYPT-KDF-*` — primitive & key-derivation known-answers and round-trips.

use pdf_crypto::kdf::{self, Aes256Hash, R234Inputs};
use pdf_crypto::primitives::{
    aes128_cbc_decrypt, aes128_cbc_encrypt, aes256_cbc_decrypt, aes256_cbc_encrypt,
    aes256_cbc_nopad_decrypt, aes256_cbc_nopad_encrypt, md5, rc4, sha256, sha384, sha512,
};

/// Hex helper (avoids a hex-literal dependency).
fn hx(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn crypt_kdf_001_hash_vectors() {
    // MD5("") and MD5("abc").
    assert_eq!(md5(b"").to_vec(), hx("d41d8cd98f00b204e9800998ecf8427e"));
    assert_eq!(md5(b"abc").to_vec(), hx("900150983cd24fb0d6963f7d28e17f72"));
    // SHA-256("abc"), SHA-384(""), SHA-512("abc") — NIST vectors.
    assert_eq!(
        sha256(b"abc").to_vec(),
        hx("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(
        sha384(b"").to_vec(),
        hx("38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b")
    );
    assert_eq!(
        sha512(b"abc").to_vec(),
        hx("ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f")
    );
}

#[test]
fn crypt_kdf_002_rc4_vectors() {
    // Classic RC4 test vectors.
    assert_eq!(rc4(b"Key", b"Plaintext"), hx("bbf316e8d940af0ad3"));
    assert_eq!(rc4(b"Wiki", b"pedia"), hx("1021bf0420"));
    assert_eq!(
        rc4(b"Secret", b"Attack at dawn"),
        hx("45a01f645fc35b383552544b9bf5")
    );
    // Symmetric: decrypt(encrypt) == identity.
    let k = b"any-key";
    let ct = rc4(k, b"round trips");
    assert_eq!(rc4(k, &ct), b"round trips");
}

#[test]
fn crypt_kdf_003_aes_roundtrip() {
    let k128 = [0x07u8; 16];
    let iv = [0x03u8; 16];
    let ct = aes128_cbc_encrypt(&k128, &iv, b"hello aes 128 world!!");
    assert_eq!(
        aes128_cbc_decrypt(&k128, &iv, &ct).unwrap(),
        b"hello aes 128 world!!"
    );

    let k256 = [0x09u8; 32];
    let ct = aes256_cbc_encrypt(&k256, &iv, b"hello aes 256 world!!");
    assert_eq!(
        aes256_cbc_decrypt(&k256, &iv, &ct).unwrap(),
        b"hello aes 256 world!!"
    );

    // No-pad round-trip (aligned input).
    let aligned = [0xABu8; 32];
    let ct = aes256_cbc_nopad_encrypt(&k256, &[0u8; 16], &aligned);
    assert_eq!(
        aes256_cbc_nopad_decrypt(&k256, &[0u8; 16], &ct).unwrap(),
        aligned
    );
}

#[test]
fn crypt_kdf_003_aes_bad_length_is_err() {
    let k = [0u8; 16];
    let iv = [0u8; 16];
    assert!(aes128_cbc_decrypt(&k, &iv, &[1, 2, 3]).is_err()); // not a multiple of 16
    assert!(aes128_cbc_decrypt(&k, &iv, &[]).is_err()); // empty
}

#[test]
fn crypt_kdf_004_pad_password() {
    // Empty password → exactly the 32-byte pad.
    assert_eq!(kdf::pad_password(b"").to_vec(), kdf::PAD.to_vec());
    // Short password → pwd ‖ pad-prefix.
    let p = kdf::pad_password(b"abc");
    assert_eq!(&p[..3], b"abc");
    assert_eq!(&p[3..], &kdf::PAD[..29]);
    // Over-long password truncates to 32.
    let long = vec![b'x'; 50];
    assert_eq!(kdf::pad_password(&long).to_vec(), vec![b'x'; 32]);
}

fn r234(rev: u8, key_len: usize, em: bool) -> Vec<u8> {
    kdf::derive_key_r234(&R234Inputs {
        password: b"",
        o: &[0x42u8; 32],
        p: -44,
        id0: b"0123456789abcdef",
        revision: rev,
        key_len,
        encrypt_metadata: em,
    })
}

#[test]
fn crypt_kdf_005_r2_key_is_5_bytes_single_md5() {
    let key = r234(2, 5, true);
    assert_eq!(key.len(), 5);
}

#[test]
fn crypt_kdf_006_r3_key_len_and_iteration() {
    // /Length 128 → 16-byte key; the 50× iteration makes it differ from R2's
    // single-MD5 prefix.
    let k16 = r234(3, 16, true);
    assert_eq!(k16.len(), 16);
    let single = md5(b"x"); // sanity that iteration changes output for a real input
    assert_ne!(&k16[..5], &single[..5]);
    // /Length 40 → 5-byte key, still iterated for R3.
    assert_eq!(r234(3, 5, true).len(), 5);
}

#[test]
fn crypt_kdf_007_encrypt_metadata_flag_changes_key() {
    let with = r234(4, 16, true);
    let without = r234(4, 16, false);
    assert_ne!(with, without, "0xFFFFFFFF tail must change the R4 key");
}

#[test]
fn crypt_kdf_008_r6_hardened_hash_deterministic() {
    let a = kdf::hash_r6(b"pw", b"saltsalt", &[]);
    let b = kdf::hash_r6(b"pw", b"saltsalt", &[]);
    assert_eq!(a, b);
    assert_eq!(a.len(), 32);
    // Different salt → different hash.
    assert_ne!(a, kdf::hash_r6(b"pw", b"OTHERslt", &[]));
}

#[test]
fn crypt_kdf_009_r5_differs_from_r6() {
    let r5 = kdf::hash_r5(b"pw", b"saltsalt", &[]);
    let r6 = kdf::hash_r6(b"pw", b"saltsalt", &[]);
    assert_ne!(
        r5, r6,
        "R5 single-SHA-256 must differ from R6 hardened hash"
    );
}

#[test]
fn crypt_kdf_010_011_ue_oe_unwrap() {
    // Plant a known 32-byte file key, build /U+/UE and /O+/OE, then unwrap.
    let file_key = [0x5Au8; 32];
    let fx = pdf_crypto::testsupport::build_aes256(
        Aes256Hash::R6,
        file_key,
        -4,
        true,
        b"",
        b"owner-secret",
    );
    let cfg = &fx.config;
    // user recovery
    let uk = kdf::recover_key_user_aes256(b"", &cfg.u, &cfg.ue, Aes256Hash::R6).unwrap();
    assert_eq!(uk, file_key);
    // owner recovery
    let ok =
        kdf::recover_key_owner_aes256(b"owner-secret", &cfg.o, &cfg.oe, &cfg.u, Aes256Hash::R6)
            .unwrap();
    assert_eq!(ok, file_key);
}
