//! Test-support **encrypt** side of the security handler (PRD §8.4 / M1e TDD).
//!
//! These helpers compute `/O`/`/U`/`/OE`/`/UE` and encrypt strings/streams for
//! each scheme, so M1e can generate valid encrypted fixtures **without** any
//! external (potentially AGPL) files. They are the encrypt counterpart of the
//! [`crate::kdf`] / [`crate::handler`] read path and are the seed of the M3
//! write API (compute the encryption dict, then encrypt object data).
//!
//! Gated behind `cfg(any(test, feature = "test-support"))` so it never ships in
//! the default read-only build; M3 will promote the relevant pieces to a public
//! authoring API.
#![allow(clippy::missing_panics_doc)]

use crate::handler::{per_object_key, CryptMethod, EncryptConfig};
use crate::kdf::{self, Aes256Hash, R234Inputs};
use crate::primitives::{
    aes128_cbc_encrypt, aes256_cbc_encrypt, aes256_cbc_nopad_encrypt, rc4, sha256,
};

/// A self-built encrypted document description for tests: the file key plus the
/// fields a `pdf-core` caller would extract into an [`EncryptConfig`].
pub struct Fixture {
    /// The 5..=32-byte file encryption key.
    pub file_key: Vec<u8>,
    /// The full `/Encrypt` config (ready to hand to a [`crate::Decryptor`]).
    pub config: EncryptConfig,
    /// The user password these entries were built for (empty = empty pwd).
    pub user_pwd: Vec<u8>,
    /// The owner password.
    pub owner_pwd: Vec<u8>,
}

impl Fixture {
    /// Encrypts a string for object `num gen` with the fixture's `/StrF`.
    #[must_use]
    pub fn encrypt_string(&self, num: u32, gen: u16, data: &[u8], iv: Option<[u8; 16]>) -> Vec<u8> {
        encrypt_obj(&self.file_key, num, gen, data, self.config.str_method, iv)
    }

    /// Encrypts a stream body for object `num gen` with the fixture's `/StmF`.
    #[must_use]
    pub fn encrypt_stream(&self, num: u32, gen: u16, data: &[u8], iv: Option<[u8; 16]>) -> Vec<u8> {
        encrypt_obj(&self.file_key, num, gen, data, self.config.stm_method, iv)
    }
}

/// Encrypt `data` for object `num gen` under `method` (the inverse of the
/// `Decryptor` read path). `iv` overrides the random AES IV (tests want it
/// deterministic). For AESV3 the file key is used directly.
#[must_use]
pub fn encrypt_obj(
    file_key: &[u8],
    num: u32,
    gen: u16,
    data: &[u8],
    method: CryptMethod,
    iv: Option<[u8; 16]>,
) -> Vec<u8> {
    match method {
        CryptMethod::Identity => data.to_vec(),
        CryptMethod::Rc4 => {
            let k = per_object_key(file_key, num, gen, false);
            rc4(&k, data)
        }
        CryptMethod::AesV2 => {
            let k = per_object_key(file_key, num, gen, true);
            let mut key = [0u8; 16];
            let n = k.len().min(16);
            key[..n].copy_from_slice(&k[..n]);
            let iv = iv.unwrap_or([0x11; 16]);
            let mut out = iv.to_vec();
            out.extend_from_slice(&aes128_cbc_encrypt(&key, &iv, data));
            out
        }
        CryptMethod::AesV3 => {
            let mut key = [0u8; 32];
            let n = file_key.len().min(32);
            key[..n].copy_from_slice(&file_key[..n]);
            let iv = iv.unwrap_or([0x22; 16]);
            let mut out = iv.to_vec();
            out.extend_from_slice(&aes256_cbc_encrypt(&key, &iv, data));
            out
        }
    }
}

/// Builds an R2–R4 fixture (RC4 or AES-128) for the given passwords.
///
/// `stm`/`str_method` choose the crypt method; for R2/R3 they must be `Rc4`. The
/// `/O`/`/U` entries are computed via the standard algorithms (3/4/5) and the
/// file key via Algorithm 2 — so the read path's `authenticate("")` round-trips.
#[must_use]
#[allow(clippy::too_many_arguments)] // mirrors the PDF /Encrypt field set (test-support)
pub fn build_r234(
    revision: u8,
    key_len: usize,
    id0: &[u8],
    p: i32,
    encrypt_metadata: bool,
    user_pwd: &[u8],
    owner_pwd: &[u8],
    stm: CryptMethod,
    str_method: CryptMethod,
) -> Fixture {
    let o = kdf::compute_o_r234(owner_pwd, user_pwd, revision, key_len);
    let file_key = kdf::derive_key_r234(&R234Inputs {
        password: user_pwd,
        o: &o,
        p,
        id0,
        revision,
        key_len,
        encrypt_metadata,
    });
    let u = kdf::compute_u_r234(&file_key, id0, revision);

    let v = if revision <= 2 {
        1
    } else if revision == 3 {
        2
    } else {
        4
    };
    let config = EncryptConfig {
        v,
        r: revision,
        o,
        u,
        oe: Vec::new(),
        ue: Vec::new(),
        p,
        key_len,
        encrypt_metadata,
        id0: id0.to_vec(),
        stm_method: stm,
        str_method,
    };
    Fixture {
        file_key,
        config,
        user_pwd: user_pwd.to_vec(),
        owner_pwd: owner_pwd.to_vec(),
    }
}

/// Builds an AES-256 (R5 or R6) fixture. Generates a random-ish 32-byte file key
/// and computes `/U`/`/UE`/`/O`/`/OE` per Algorithms 8/9 (R6) or the single-hash
/// R5 form. `hash` picks the validation/derivation hash (`R5` vs `R6`).
#[must_use]
pub fn build_aes256(
    hash: Aes256Hash,
    file_key: [u8; 32],
    p: i32,
    encrypt_metadata: bool,
    user_pwd: &[u8],
    owner_pwd: &[u8],
) -> Fixture {
    let run = |pwd: &[u8], salt: &[u8], udata: &[u8]| match hash {
        Aes256Hash::R5 => kdf::hash_r5(pwd, salt, udata),
        Aes256Hash::R6 => kdf::hash_r6(pwd, salt, udata),
    };

    // Deterministic-but-distinct salts derived from a seed (no rng dependency).
    let salts = sha256(b"oxipdf-test-aes256-salt-seed");
    let u_vsalt = &salts[0..8];
    let u_ksalt = &salts[8..16];
    let o_vsalt = &salts[16..24];
    let o_ksalt = &salts[24..32];

    // --- /U + /UE (user) ----------------------------------------------------
    let u_hash = run(user_pwd, u_vsalt, &[]);
    let mut u = Vec::with_capacity(48);
    u.extend_from_slice(&u_hash);
    u.extend_from_slice(u_vsalt);
    u.extend_from_slice(u_ksalt);

    let u_intermediate = run(user_pwd, u_ksalt, &[]);
    let ue = aes256_cbc_nopad_encrypt(&u_intermediate, &[0u8; 16], &file_key);

    // --- /O + /OE (owner) — depends on the 48-byte /U -----------------------
    let u48 = &u[..48];
    let o_hash = run(owner_pwd, o_vsalt, u48);
    let mut o = Vec::with_capacity(48);
    o.extend_from_slice(&o_hash);
    o.extend_from_slice(o_vsalt);
    o.extend_from_slice(o_ksalt);

    let o_intermediate = run(owner_pwd, o_ksalt, u48);
    let oe = aes256_cbc_nopad_encrypt(&o_intermediate, &[0u8; 16], &file_key);

    let r = if hash == Aes256Hash::R5 { 5 } else { 6 };
    let config = EncryptConfig {
        v: 5,
        r,
        o,
        u,
        oe,
        ue,
        p,
        key_len: 32,
        encrypt_metadata,
        id0: Vec::new(), // R5/R6 do not use /ID
        stm_method: CryptMethod::AesV3,
        str_method: CryptMethod::AesV3,
    };
    Fixture {
        file_key: file_key.to_vec(),
        config,
        user_pwd: user_pwd.to_vec(),
        owner_pwd: owner_pwd.to_vec(),
    }
}
