//! Cryptographic primitives for the Standard Security Handler (PRD §8.4).
//!
//! Hashes (MD5, SHA-256/384/512) and AES-CBC come from RustCrypto (all
//! MIT/Apache-2.0, pure-Rust). RC4 is hand-rolled here because the PDF spec uses
//! **runtime-length** keys (5..=16 bytes per object key, after the
//! `min(len+5,16)` truncation) and the `rc4` crate's API fixes the key length at
//! the type level via typenum — which cannot express a length only known at run
//! time. RC4 is a trivial, well-known stream cipher; the implementation below is
//! safe Rust and validated against the standard test vectors.

use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use cbc::{Decryptor, Encryptor};
use cipher::block_padding::{NoPadding, Pkcs7};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha384, Sha512};

use crate::error::CryptoError;

type Aes128CbcEnc = Encryptor<aes::Aes128>;
type Aes128CbcDec = Decryptor<aes::Aes128>;
type Aes256CbcEnc = Encryptor<aes::Aes256>;
type Aes256CbcDec = Decryptor<aes::Aes256>;

// --- hashes ---------------------------------------------------------------

/// MD5 of `data` (16 bytes). Used by R2–R4 key derivation (Algorithm 2) and
/// per-object key derivation (PRD §8.4).
#[must_use]
pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut h = Md5::new();
    h.update(data);
    h.finalize().into()
}

/// SHA-256 of `data` (32 bytes). R5/R6 validation (PRD §8.4).
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

/// SHA-384 of `data` (48 bytes). R6 Algorithm 2.B hardened hash.
#[must_use]
pub fn sha384(data: &[u8]) -> [u8; 48] {
    let mut h = Sha384::new();
    h.update(data);
    h.finalize().into()
}

/// SHA-512 of `data` (64 bytes). R6 Algorithm 2.B hardened hash.
#[must_use]
pub fn sha512(data: &[u8]) -> [u8; 64] {
    let mut h = Sha512::new();
    h.update(data);
    h.finalize().into()
}

// --- RC4 (hand-rolled, runtime key length) --------------------------------

/// RC4 keystream XOR (symmetric: the same call encrypts and decrypts).
///
/// `key` may be any non-empty length 1..=256; PDF uses 5..=16. Returns a new
/// buffer the same length as `data`.
#[must_use]
pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    debug_assert!(!key.is_empty(), "RC4 key must be non-empty");
    if key.is_empty() {
        // Defensive: an empty key would panic on the modulo below. Treat it as a
        // no-op rather than panicking (callers should never hit this).
        return data.to_vec();
    }
    // Key-scheduling algorithm (KSA).
    let mut s: [u8; 256] = [0; 256];
    for (i, b) in s.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut j: usize = 0;
    for i in 0..256 {
        j = (j + s[i] as usize + key[i % key.len()] as usize) & 0xff;
        s.swap(i, j);
    }
    // Pseudo-random generation algorithm (PRGA).
    let (mut i, mut j) = (0usize, 0usize);
    let mut out = Vec::with_capacity(data.len());
    for &byte in data {
        i = (i + 1) & 0xff;
        j = (j + s[i] as usize) & 0xff;
        s.swap(i, j);
        let k = s[(s[i] as usize + s[j] as usize) & 0xff];
        out.push(byte ^ k);
    }
    out
}

// --- AES-128-CBC (AESV2) --------------------------------------------------

/// AES-128-CBC decrypt with PKCS#7 unpadding. `key` is 16 bytes, `iv` 16 bytes.
///
/// # Errors
/// [`CryptoError::Malformed`] if the ciphertext length is not a positive
/// multiple of 16 or the PKCS#7 padding is invalid.
pub fn aes128_cbc_decrypt(
    key: &[u8; 16],
    iv: &[u8; 16],
    ct: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if ct.is_empty() || !ct.len().is_multiple_of(16) {
        return Err(CryptoError::Malformed(
            "AES-128 ciphertext length not a multiple of 16",
        ));
    }
    let mut buf = ct.to_vec();
    let pt = Aes128CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| CryptoError::Malformed("AES-128 PKCS#7 unpad failed"))?;
    Ok(pt.to_vec())
}

/// AES-128-CBC encrypt with PKCS#7 padding (test-support / M3 write).
#[must_use]
pub fn aes128_cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], pt: &[u8]) -> Vec<u8> {
    let n = pt.len();
    let mut buf = vec![0u8; n + 16];
    buf[..n].copy_from_slice(pt);
    Aes128CbcEnc::new(key.into(), iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, n)
        .expect("AES-128 PKCS#7 pad never overflows a +16 buffer")
        .to_vec()
}

// --- AES-256-CBC (AESV3, R5/R6) -------------------------------------------

/// AES-256-CBC decrypt with PKCS#7 unpadding (object data). `key` is 32 bytes.
///
/// # Errors
/// As [`aes128_cbc_decrypt`].
pub fn aes256_cbc_decrypt(
    key: &[u8; 32],
    iv: &[u8; 16],
    ct: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if ct.is_empty() || !ct.len().is_multiple_of(16) {
        return Err(CryptoError::Malformed(
            "AES-256 ciphertext length not a multiple of 16",
        ));
    }
    let mut buf = ct.to_vec();
    let pt = Aes256CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| CryptoError::Malformed("AES-256 PKCS#7 unpad failed"))?;
    Ok(pt.to_vec())
}

/// AES-256-CBC encrypt with PKCS#7 padding (test-support / M3 write).
#[must_use]
pub fn aes256_cbc_encrypt(key: &[u8; 32], iv: &[u8; 16], pt: &[u8]) -> Vec<u8> {
    let n = pt.len();
    let mut buf = vec![0u8; n + 16];
    buf[..n].copy_from_slice(pt);
    Aes256CbcEnc::new(key.into(), iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, n)
        .expect("AES-256 PKCS#7 pad never overflows a +16 buffer")
        .to_vec()
}

/// AES-256-CBC **no-padding** decrypt — the `/UE`/`/OE` key-unwrap step of R6
/// Algorithm 2.B (IV = 0, no padding; PRD §8.4). `ct` must be a multiple of 16.
///
/// # Errors
/// [`CryptoError::Malformed`] if `ct.len()` is not a positive multiple of 16.
pub fn aes256_cbc_nopad_decrypt(
    key: &[u8; 32],
    iv: &[u8; 16],
    ct: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if ct.is_empty() || !ct.len().is_multiple_of(16) {
        return Err(CryptoError::Malformed(
            "AES-256 no-pad input not a multiple of 16",
        ));
    }
    let mut buf = ct.to_vec();
    let pt = Aes256CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|_| CryptoError::Malformed("AES-256 no-pad decrypt failed"))?;
    Ok(pt.to_vec())
}

/// AES-256-CBC **no-padding** encrypt — produces `/UE`/`/OE` (test-support / M3
/// write). `pt` must be a multiple of 16 (it always is: a 32-byte file key).
#[must_use]
pub fn aes256_cbc_nopad_encrypt(key: &[u8; 32], iv: &[u8; 16], pt: &[u8]) -> Vec<u8> {
    debug_assert!(
        pt.len().is_multiple_of(16),
        "no-pad input must be a multiple of 16"
    );
    let mut buf = pt.to_vec();
    let n = buf.len();
    Aes256CbcEnc::new(key.into(), iv.into())
        .encrypt_padded_mut::<NoPadding>(&mut buf, n)
        .expect("AES-256 no-pad over an aligned buffer never overflows")
        .to_vec()
}

/// AES-256-CBC no-padding encrypt of exactly one 16-byte block, used by the R6
/// Algorithm 2.B inner loop (encrypt 64-byte `K1` with key=K[0..16], iv=K[16..32]).
#[must_use]
pub fn aes128_cbc_nopad_encrypt(key: &[u8; 16], iv: &[u8; 16], pt: &[u8]) -> Vec<u8> {
    debug_assert!(
        pt.len().is_multiple_of(16),
        "no-pad input must be a multiple of 16"
    );
    let mut buf = pt.to_vec();
    let n = buf.len();
    Aes128CbcEnc::new(key.into(), iv.into())
        .encrypt_padded_mut::<NoPadding>(&mut buf, n)
        .expect("AES-128 no-pad over an aligned buffer never overflows")
        .to_vec()
}
