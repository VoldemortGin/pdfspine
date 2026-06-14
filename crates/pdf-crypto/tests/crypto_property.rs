//! `CRYPT-PANIC-*` — garbage `/Encrypt` configs / random key material / random
//! object data must yield a typed error (or correct bytes), never a panic.
//!
//! These properties exercise the **cheap** RC4/AES-128 (R2–R4) paths so the
//! suite stays fast; the expensive R6 Algorithm 2.B hardened hash is a
//! deliberate password-hardening cost and is covered for correctness by the
//! `roundtrip_unit` / `auth_unit` tests (one R6 derivation each, not per-case).

use proptest::prelude::*;

use pdf_crypto::handler::{CryptMethod, EncryptConfig};
use pdf_crypto::testsupport::build_r234;
use pdf_crypto::Decryptor;

fn method() -> impl Strategy<Value = CryptMethod> {
    prop_oneof![
        Just(CryptMethod::Identity),
        Just(CryptMethod::Rc4),
        Just(CryptMethod::AesV2),
        Just(CryptMethod::AesV3),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]

    /// CRYPT-PANIC-001: arbitrary /Encrypt field bytes never panic; `Decryptor::new`
    /// and `authenticate`/`decrypt` return `Ok`/typed-`Err`.
    #[test]
    fn crypt_panic_001_garbage_config(
        v in 0u8..=8,
        r in 0u8..=8,
        o in prop::collection::vec(any::<u8>(), 0..80),
        u in prop::collection::vec(any::<u8>(), 0..80),
        oe in prop::collection::vec(any::<u8>(), 0..64),
        ue in prop::collection::vec(any::<u8>(), 0..64),
        p in any::<i32>(),
        key_len in 0usize..40,
        em in any::<bool>(),
        id0 in prop::collection::vec(any::<u8>(), 0..40),
        stm in method(),
        strm in method(),
        pwd in prop::collection::vec(any::<u8>(), 0..40),
    ) {
        let cfg = EncryptConfig {
            v, r, o, u, oe, ue, p, key_len,
            encrypt_metadata: em, id0,
            stm_method: stm, str_method: strm,
        };
        if let Ok(mut d) = Decryptor::new(cfg) {
            // Whatever the verdict, it must be a clean Result, never a panic.
            let _ = d.authenticate(&pwd);
            let _ = d.decrypt_string(1, 0, b"data-data-data16");
            let _ = d.decrypt_stream(2, 0, b"data-data-data16");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]

    /// CRYPT-PANIC-002/003: a valid R4 AES-128 fixture + arbitrary object data
    /// never panics on decrypt — returns bytes or a typed error (short data /
    /// bad PKCS#7 padding are the interesting cases).
    #[test]
    fn crypt_panic_002_003_random_object_data(
        data in prop::collection::vec(any::<u8>(), 0..200),
        num in any::<u32>(),
        gen in any::<u16>(),
    ) {
        let fx = build_r234(4, 16, b"id0", -4, true, b"", b"o", CryptMethod::AesV2, CryptMethod::AesV2);
        let mut d = Decryptor::new(fx.config.clone()).unwrap();
        d.authenticate(b"").unwrap();
        let _ = d.decrypt_stream(num, gen, &data);
        let _ = d.decrypt_string(num, gen, &data);
    }

    /// CRYPT-PANIC-004: arbitrary passwords against a valid fixture authenticate
    /// cleanly (empty matches, others usually NeedsPassword) — never a panic.
    #[test]
    fn crypt_panic_004_arbitrary_password(
        pwd in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let fx = build_r234(4, 16, b"id0", -4, true, b"", b"owner", CryptMethod::Rc4, CryptMethod::Rc4);
        let mut d = Decryptor::new(fx.config.clone()).unwrap();
        let _ = d.authenticate(&pwd);
    }
}
