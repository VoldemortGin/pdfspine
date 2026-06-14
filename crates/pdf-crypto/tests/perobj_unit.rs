//! `CRYPT-PEROBJ-*` — per-object key derivation correctness.

use pdf_crypto::handler::per_object_key;
use pdf_crypto::primitives::md5;

#[test]
fn crypt_perobj_001_rc4_key_no_salt_and_truncation() {
    // 5-byte file key → object key length = min(5+5,16) = 10.
    let fk = [0x11u8, 0x22, 0x33, 0x44, 0x55];
    let key = per_object_key(&fk, 7, 0, false);
    assert_eq!(key.len(), 10);
    // Recompute the expected MD5 manually: fk ‖ num[3 LE] ‖ gen[2 LE], no sAlT.
    let mut buf = fk.to_vec();
    buf.extend_from_slice(&[7, 0, 0, 0, 0]); // num=7 (LE 3), gen=0 (LE 2)
    let expected = md5(&buf);
    assert_eq!(key, &expected[..10]);
}

#[test]
fn crypt_perobj_002_aesv2_appends_salt() {
    let fk = [0x11u8; 16];
    let rc4_key = per_object_key(&fk, 3, 0, false);
    let aes_key = per_object_key(&fk, 3, 0, true);
    assert_ne!(
        rc4_key, aes_key,
        "AESV2 must append \"sAlT\" → different key"
    );
    // Verify the literal sAlT bytes (0x73 0x41 0x6C 0x54) feed the AES key.
    let mut buf = fk.to_vec();
    buf.extend_from_slice(&[3, 0, 0, 0, 0]);
    buf.extend_from_slice(&[0x73, 0x41, 0x6C, 0x54]);
    let expected = md5(&buf);
    assert_eq!(aes_key, &expected[..16]);
}

#[test]
fn crypt_perobj_003_16byte_filekey_caps_at_16() {
    let fk = [0xAAu8; 16];
    // min(16+5,16) = 16.
    assert_eq!(per_object_key(&fk, 1, 0, false).len(), 16);
    assert_eq!(per_object_key(&fk, 1, 0, true).len(), 16);
}

#[test]
fn crypt_perobj_005_num_gen_little_endian_sensitivity() {
    let fk = [0x33u8; 16];
    // Object 256 (0x0100) vs 1 must differ (3-byte LE encoding of num).
    assert_ne!(
        per_object_key(&fk, 256, 0, false),
        per_object_key(&fk, 1, 0, false)
    );
    // Different generation changes the key.
    assert_ne!(
        per_object_key(&fk, 5, 0, false),
        per_object_key(&fk, 5, 1, false)
    );
    // num=0x010000 sets the 3rd LE byte.
    let mut buf = fk.to_vec();
    buf.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00]); // num = 0x10000
    let expected = md5(&buf);
    assert_eq!(per_object_key(&fk, 0x1_0000, 0, false), &expected[..16]);
}
