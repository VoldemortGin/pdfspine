//! Key-derivation functions for the Standard Security Handler (PRD §8.4).
//!
//! - **R2–R4** — Algorithm 2 (32-byte password pad + MD5, iterated 50× for R≥3).
//! - **R6** — Algorithm 2.B hardened iterated hash (SHA-256/384/512), plus the
//!   `/UE`/`/OE` AES-256 no-pad key-unwrap (Algorithms 8/9).
//! - **R5** — the transitional single-SHA-256 form (read-only).
//!
//! All inputs are plain byte slices / ints supplied by the caller (`pdf-core`
//! extracts them from the `/Encrypt` dict). This crate has **no** dependency on
//! `pdf-core`, so the dependency arrow points one way only (PRD §9.1).

use crate::primitives::{
    aes128_cbc_nopad_encrypt, aes256_cbc_nopad_decrypt, md5, sha256, sha384, sha512,
};

/// The 32-byte password padding string (ISO 32000-1, Algorithm 2). Appended to
/// (or truncated from) a user/owner password to a fixed 32 bytes.
pub const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Pads (or truncates) a password to the fixed 32-byte form (Algorithm 2 step a).
#[must_use]
pub fn pad_password(pwd: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = pwd.len().min(32);
    out[..n].copy_from_slice(&pwd[..n]);
    out[n..].copy_from_slice(&PAD[..32 - n]);
    out
}

/// Inputs to the R2–R4 file-key derivation (Algorithm 2). `id0` is the document
/// `/ID[0]`; when `/ID` is absent the caller passes an **empty** slice (PRD §8.4
/// `/ID`-absent fallback).
pub struct R234Inputs<'a> {
    /// The (already-padded-or-not) raw password bytes.
    pub password: &'a [u8],
    /// `/O` entry (32 bytes).
    pub o: &'a [u8],
    /// `/P` permission flags as a signed 32-bit int.
    pub p: i32,
    /// `/ID[0]` (empty if `/ID` absent).
    pub id0: &'a [u8],
    /// Security-handler revision (2, 3 or 4).
    pub revision: u8,
    /// File-key length in **bytes** (`/Length`/8, or 5 for R2).
    pub key_len: usize,
    /// `/EncryptMetadata` (R4 only; affects the trailing `0xFFFFFFFF`).
    pub encrypt_metadata: bool,
}

/// Algorithm 2 — derive the file encryption key from the user password (R2–R4).
///
/// MD5 over `padded_pwd ‖ O ‖ P(4 LE) ‖ ID[0] [‖ 0xFFFFFFFF if R≥4 && !EncryptMetadata]`,
/// then for R≥3 the first `key_len` bytes are re-hashed 50×. The result is
/// truncated to `key_len` bytes (PRD §8.4).
#[must_use]
pub fn derive_key_r234(input: &R234Inputs) -> Vec<u8> {
    let padded = pad_password(input.password);
    let mut buf: Vec<u8> = Vec::with_capacity(32 + 32 + 4 + input.id0.len() + 4);
    buf.extend_from_slice(&padded);
    // `/O` is conventionally 32 bytes; use what is present (tolerant).
    buf.extend_from_slice(input.o);
    // `/P` as 4 little-endian bytes (treat the i32 as a u32 bit pattern).
    buf.extend_from_slice(&(input.p as u32).to_le_bytes());
    buf.extend_from_slice(input.id0);
    if input.revision >= 4 && !input.encrypt_metadata {
        buf.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    let key_len = input.key_len.clamp(1, 16);
    let mut hash = md5(&buf);
    if input.revision >= 3 {
        for _ in 0..50 {
            hash = md5(&hash[..key_len]);
        }
    }
    hash[..key_len].to_vec()
}

/// Algorithm 3 — compute the `/O` entry from owner+user passwords (test-support /
/// M3 write). For R≥3 the RC4 is applied 20× with key-XOR mutation.
#[must_use]
pub fn compute_o_r234(owner_pwd: &[u8], user_pwd: &[u8], revision: u8, key_len: usize) -> Vec<u8> {
    use crate::primitives::rc4;
    // Step a–d: MD5 of padded owner pwd (50× for R≥3), take first key_len bytes.
    let padded_owner = pad_password(owner_pwd);
    let mut h = md5(&padded_owner);
    let key_len = key_len.clamp(1, 16);
    if revision >= 3 {
        for _ in 0..50 {
            h = md5(&h[..key_len]);
        }
    }
    let rc4_key = &h[..key_len];
    // Step e–f: RC4-encrypt the padded user password (20× for R≥3 with XOR).
    let padded_user = pad_password(user_pwd);
    let mut data = padded_user.to_vec();
    if revision >= 3 {
        for i in 0u8..20 {
            let xkey: Vec<u8> = rc4_key.iter().map(|b| b ^ i).collect();
            data = rc4(&xkey, &data);
        }
    } else {
        data = rc4(rc4_key, &data);
    }
    data
}

/// Algorithm 4/5 — compute the `/U` entry (test-support / M3 write).
///
/// R2: RC4(filekey, PAD). R3/R4: RC4-iterated MD5(PAD ‖ ID[0]) then pad to 32.
#[must_use]
pub fn compute_u_r234(file_key: &[u8], id0: &[u8], revision: u8) -> Vec<u8> {
    use crate::primitives::rc4;
    if revision == 2 {
        return rc4(file_key, &PAD);
    }
    // R3/R4 (Algorithm 5).
    let mut buf = Vec::with_capacity(32 + id0.len());
    buf.extend_from_slice(&PAD);
    buf.extend_from_slice(id0);
    let h = md5(&buf);
    let mut data = h.to_vec();
    for i in 0u8..20 {
        let xkey: Vec<u8> = file_key.iter().map(|b| b ^ i).collect();
        data = rc4(&xkey, &data);
    }
    // Pad the 16-byte result out to 32 bytes (arbitrary padding per spec).
    data.resize(32, 0);
    data
}

/// Algorithm 6 — validate the **user** password (R2–R4): recompute `/U` and
/// compare. For R≥3 only the first 16 bytes are significant.
#[must_use]
pub fn check_user_r234(file_key: &[u8], id0: &[u8], revision: u8, u_entry: &[u8]) -> bool {
    let computed = compute_u_r234(file_key, id0, revision);
    if revision == 2 {
        computed.len() == u_entry.len() && computed == u_entry
    } else {
        let n = 16.min(computed.len()).min(u_entry.len());
        n == 16 && computed[..16] == u_entry[..16]
    }
}

/// Algorithm 7 — recover the user password from the owner password (R2–R4), so
/// the owner can authenticate. Returns the candidate user password (padded form
/// hidden by the caller; only used as input to the file-key derivation).
#[must_use]
pub fn owner_user_pwd_r234(
    owner_pwd: &[u8],
    o_entry: &[u8],
    revision: u8,
    key_len: usize,
) -> Vec<u8> {
    use crate::primitives::rc4;
    let padded_owner = pad_password(owner_pwd);
    let mut h = md5(&padded_owner);
    let key_len = key_len.clamp(1, 16);
    if revision >= 3 {
        for _ in 0..50 {
            h = md5(&h[..key_len]);
        }
    }
    let rc4_key = &h[..key_len];
    let mut data = o_entry.to_vec();
    if revision >= 3 {
        // Decrypt: iterate i = 19 down to 0.
        for i in (0u8..20).rev() {
            let xkey: Vec<u8> = rc4_key.iter().map(|b| b ^ i).collect();
            data = rc4(&xkey, &data);
        }
    } else {
        data = rc4(rc4_key, &data);
    }
    data
}

// --- R5 / R6 (AES-256) ----------------------------------------------------

/// Algorithm 2.B — the R6 hardened iterated hash over a password, salt and
/// optional `/U` (used when validating/deriving with the **owner** entry).
///
/// `udata` is the empty slice for user-password processing, or the 48-byte `/U`
/// for owner-password processing (PRD §8.4 Algorithm 2.B).
#[must_use]
pub fn hash_r6(password: &[u8], salt: &[u8], udata: &[u8]) -> [u8; 32] {
    // K = SHA-256(password ‖ salt ‖ udata)
    let mut seed = Vec::with_capacity(password.len() + salt.len() + udata.len());
    seed.extend_from_slice(password);
    seed.extend_from_slice(salt);
    seed.extend_from_slice(udata);
    let mut k = sha256(&seed).to_vec();

    let mut round = 0usize;
    loop {
        // K1 = (password ‖ K ‖ udata) repeated 64×
        let mut block = Vec::with_capacity((password.len() + k.len() + udata.len()) * 64);
        let mut one = Vec::with_capacity(password.len() + k.len() + udata.len());
        one.extend_from_slice(password);
        one.extend_from_slice(&k);
        one.extend_from_slice(udata);
        for _ in 0..64 {
            block.extend_from_slice(&one);
        }

        // E = AES-128-CBC-NoPad(key=K[0..16], iv=K[16..32], data=K1)
        let mut key16 = [0u8; 16];
        let mut iv16 = [0u8; 16];
        key16.copy_from_slice(&k[0..16]);
        iv16.copy_from_slice(&k[16..32]);
        let e = aes128_cbc_nopad_encrypt(&key16, &iv16, &block);

        // mod = (sum of first 16 bytes of E) mod 3 → choose SHA-256/384/512
        let modulus: u32 = e[..16].iter().map(|&b| b as u32).sum::<u32>() % 3;
        k = match modulus {
            0 => sha256(&e).to_vec(),
            1 => sha384(&e).to_vec(),
            _ => sha512(&e).to_vec(),
        };

        round += 1;
        // After round 64, if the last byte of E ≤ round-32, stop.
        if round >= 64 {
            let last = *e.last().unwrap_or(&0) as usize;
            if last <= round - 32 {
                break;
            }
        }
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&k[..32]);
    out
}

/// The R5 hash: a single SHA-256 of `password ‖ salt [‖ udata]` (transitional,
/// read-only — never written; PRD §8.4).
#[must_use]
pub fn hash_r5(password: &[u8], salt: &[u8], udata: &[u8]) -> [u8; 32] {
    let mut seed = Vec::with_capacity(password.len() + salt.len() + udata.len());
    seed.extend_from_slice(password);
    seed.extend_from_slice(salt);
    seed.extend_from_slice(udata);
    sha256(&seed)
}

/// Splits a 48-byte `/U` (or `/O`) entry into `(hash[0..32], validation_salt[32..40], key_salt[40..48])`.
/// Returns `None` if the entry is shorter than 48 bytes.
#[must_use]
pub fn split_48(entry: &[u8]) -> Option<([u8; 32], [u8; 8], [u8; 8])> {
    if entry.len() < 48 {
        return None;
    }
    let mut hash = [0u8; 32];
    let mut vsalt = [0u8; 8];
    let mut ksalt = [0u8; 8];
    hash.copy_from_slice(&entry[0..32]);
    vsalt.copy_from_slice(&entry[32..40]);
    ksalt.copy_from_slice(&entry[40..48]);
    Some((hash, vsalt, ksalt))
}

/// The chosen R5-vs-R6 hash function for AES-256 (selected by `/R`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Aes256Hash {
    /// R5 transitional single SHA-256.
    R5,
    /// R6 Algorithm 2.B hardened iterated hash.
    R6,
}

impl Aes256Hash {
    fn run(self, password: &[u8], salt: &[u8], udata: &[u8]) -> [u8; 32] {
        match self {
            Aes256Hash::R5 => hash_r5(password, salt, udata),
            Aes256Hash::R6 => hash_r6(password, salt, udata),
        }
    }
}

/// Validate the **user** password for R5/R6 (Algorithm 11/8 validation step):
/// recompute the hash over `password ‖ validation_salt` and compare to `/U[0..32]`.
#[must_use]
pub fn check_user_aes256(password: &[u8], u_entry: &[u8], hash: Aes256Hash) -> bool {
    let Some((stored, vsalt, _)) = split_48(u_entry) else {
        return false;
    };
    hash.run(password, &vsalt, &[]) == stored
}

/// Validate the **owner** password for R5/R6 (Algorithm 12/9 validation step):
/// hash over `password ‖ validation_salt ‖ U[0..48]` and compare to `/O[0..32]`.
#[must_use]
pub fn check_owner_aes256(
    password: &[u8],
    o_entry: &[u8],
    u_entry: &[u8],
    hash: Aes256Hash,
) -> bool {
    let Some((stored, vsalt, _)) = split_48(o_entry) else {
        return false;
    };
    let u48 = if u_entry.len() >= 48 {
        &u_entry[..48]
    } else {
        u_entry
    };
    hash.run(password, &vsalt, u48) == stored
}

/// Recover the file key from `/UE` using the **user** password (Algorithm 8/11
/// key step): intermediate = hash(password ‖ key_salt); file key =
/// AES-256-CBC-NoPad-decrypt(key=intermediate, iv=0, /UE).
///
/// # Errors
/// [`CryptoError`] if `/U` is malformed or the AES unwrap fails.
pub fn recover_key_user_aes256(
    password: &[u8],
    u_entry: &[u8],
    ue_entry: &[u8],
    hash: Aes256Hash,
) -> Result<[u8; 32], crate::error::CryptoError> {
    use crate::error::CryptoError;
    let (_, _, ksalt) =
        split_48(u_entry).ok_or(CryptoError::Malformed("/U shorter than 48 bytes"))?;
    let intermediate = hash.run(password, &ksalt, &[]);
    let iv = [0u8; 16];
    let pt = aes256_cbc_nopad_decrypt(&intermediate, &iv, ue_entry)?;
    if pt.len() < 32 {
        return Err(CryptoError::Malformed("/UE unwrap produced < 32 bytes"));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&pt[..32]);
    Ok(key)
}

/// Recover the file key from `/OE` using the **owner** password (Algorithm 9/12
/// key step): intermediate = hash(password ‖ key_salt ‖ U[0..48]); file key =
/// AES-256-CBC-NoPad-decrypt(key=intermediate, iv=0, /OE).
///
/// # Errors
/// As [`recover_key_user_aes256`].
pub fn recover_key_owner_aes256(
    password: &[u8],
    o_entry: &[u8],
    oe_entry: &[u8],
    u_entry: &[u8],
    hash: Aes256Hash,
) -> Result<[u8; 32], crate::error::CryptoError> {
    use crate::error::CryptoError;
    let (_, _, ksalt) =
        split_48(o_entry).ok_or(CryptoError::Malformed("/O shorter than 48 bytes"))?;
    let u48 = if u_entry.len() >= 48 {
        &u_entry[..48]
    } else {
        u_entry
    };
    // hash(password ‖ key_salt ‖ U) — U is carried in the udata slot.
    let intermediate = hash.run(password, &ksalt, u48);
    let iv = [0u8; 16];
    let pt = aes256_cbc_nopad_decrypt(&intermediate, &iv, oe_entry)?;
    if pt.len() < 32 {
        return Err(CryptoError::Malformed("/OE unwrap produced < 32 bytes"));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&pt[..32]);
    Ok(key)
}
