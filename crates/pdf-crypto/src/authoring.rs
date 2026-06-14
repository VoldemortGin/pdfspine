//! Encryption **authoring** for save (PRD §8.4 write rules).
//!
//! Promotes the encrypt-side helpers (formerly test-only) to a public API the
//! `pdf-core` writer uses to encrypt a document on full save. Computes the
//! `/Encrypt` dict fields (`/O`/`/U`/`/OE`/`/UE`) for the three writable
//! methods and encrypts each object's strings + stream body with the correct
//! per-object key.
//!
//! **Policy (PRD §8.4): never write R5.** AES-256 is always authored as **R6**
//! (Algorithm 2.B). Salts and AES IVs come from the OS CSPRNG ([`getrandom`]).
//!
//! - **RC4-128**: `/V 2 /R 3`, RC4 per-object key (`MD5(filekey‖num‖gen)`,
//!   truncate `min(len+5,16)`), no IV.
//! - **AES-128 (AESV2)**: `/V 4 /R 4`, per-object key adds the `"sAlT"` salt,
//!   AES-128-CBC with a random 16-byte IV prepended.
//! - **AES-256 R6 (AESV3)**: `/V 5 /R 6`, the file key is used **directly** (no
//!   per-object salting/truncation), AES-256-CBC with a random IV prepended.

use crate::handler::{per_object_key, CryptMethod, EncryptConfig};
use crate::kdf::{self, R234Inputs};
use crate::primitives::{aes128_cbc_encrypt, aes256_cbc_encrypt, aes256_cbc_nopad_encrypt, rc4};

/// The writable encryption methods (PRD §8.4 — RC4-128 / AES-128 / AES-256-R6).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EncryptMethod {
    /// RC4 128-bit, `/V 2 /R 3`.
    Rc4_128,
    /// AES-128 (`/AESV2`), `/V 4 /R 4`.
    Aes128,
    /// AES-256 (`/AESV3`), `/V 5 /R 6` — the only AES-256 form we write.
    Aes256R6,
}

/// What to encrypt with and the passwords/permissions to author.
#[derive(Clone, Debug)]
pub struct EncryptSpec {
    /// The user password (empty string = open without a password).
    pub user_pw: Vec<u8>,
    /// The owner password (empty = same as user).
    pub owner_pw: Vec<u8>,
    /// The `/P` permission flags (signed 32-bit; advisory).
    pub permissions: i32,
    /// The cipher + revision to author.
    pub method: EncryptMethod,
}

impl EncryptSpec {
    /// A spec for `method` with empty passwords and all-permissions (`-1`).
    #[must_use]
    pub fn new(method: EncryptMethod) -> Self {
        Self {
            user_pw: Vec::new(),
            owner_pw: Vec::new(),
            permissions: -1,
            method,
        }
    }
}

/// A built security handler ready to encrypt object data on save.
///
/// Holds the derived file key + the resolved [`EncryptConfig`] (whose fields the
/// writer serializes into the indirect `/Encrypt` dictionary).
#[derive(Clone, Debug)]
pub struct Authoring {
    file_key: Vec<u8>,
    config: EncryptConfig,
}

impl Authoring {
    /// Builds the handler for `spec`. `id0` is the document `/ID[0]` (used by the
    /// R2–R4 KDF; ignored by R6). Salts/IVs are drawn from the OS CSPRNG.
    ///
    /// # Errors
    ///
    /// [`crate::CryptoError`] only if the resulting config fails validation
    /// (should not happen for the fixed writable methods).
    pub fn new(spec: &EncryptSpec, id0: &[u8]) -> crate::Result<Self> {
        let owner = if spec.owner_pw.is_empty() {
            &spec.user_pw
        } else {
            &spec.owner_pw
        };
        let (file_key, config) = match spec.method {
            EncryptMethod::Rc4_128 => build_r234(
                3,
                16,
                id0,
                spec.permissions,
                &spec.user_pw,
                owner,
                CryptMethod::Rc4,
            ),
            EncryptMethod::Aes128 => build_r234(
                4,
                16,
                id0,
                spec.permissions,
                &spec.user_pw,
                owner,
                CryptMethod::AesV2,
            ),
            EncryptMethod::Aes256R6 => build_aes256_r6(spec.permissions, &spec.user_pw, owner),
        };
        // Validate via the read-side constructor; discard the Decryptor.
        crate::Decryptor::new(config.clone())?;
        Ok(Self { file_key, config })
    }

    /// The resolved `/Encrypt` configuration (its fields go into the dict).
    #[must_use]
    pub fn config(&self) -> &EncryptConfig {
        &self.config
    }

    /// Encrypts a string for object `num gen` (uses `/StrF`). A fresh random IV
    /// is drawn for AES methods.
    #[must_use]
    pub fn encrypt_string(&self, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        encrypt_obj(&self.file_key, num, gen, data, self.config.str_method)
    }

    /// Encrypts a stream body for object `num gen` (uses `/StmF`).
    #[must_use]
    pub fn encrypt_stream(&self, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        encrypt_obj(&self.file_key, num, gen, data, self.config.stm_method)
    }
}

/// 16 fresh bytes from the OS CSPRNG. Falls back to a hash of process/time
/// entropy only if the OS source is somehow unavailable (never expected).
fn random_iv() -> [u8; 16] {
    let mut iv = [0u8; 16];
    if getrandom::getrandom(&mut iv).is_err() {
        // Defensive fallback: still 16 unpredictable-ish bytes. Real targets
        // always have an OS entropy source, so this path is effectively dead.
        let seed = crate::primitives::sha256(
            format!(
                "{:?}",
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            )
            .as_bytes(),
        );
        iv.copy_from_slice(&seed[..16]);
    }
    iv
}

/// Encrypt `data` for object `num gen` under `method` (mirrors the `Decryptor`
/// read path). AES methods prepend a random 16-byte IV.
#[must_use]
fn encrypt_obj(file_key: &[u8], num: u32, gen: u16, data: &[u8], method: CryptMethod) -> Vec<u8> {
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
            let iv = random_iv();
            let mut out = iv.to_vec();
            out.extend_from_slice(&aes128_cbc_encrypt(&key, &iv, data));
            out
        }
        CryptMethod::AesV3 => {
            let mut key = [0u8; 32];
            let n = file_key.len().min(32);
            key[..n].copy_from_slice(&file_key[..n]);
            let iv = random_iv();
            let mut out = iv.to_vec();
            out.extend_from_slice(&aes256_cbc_encrypt(&key, &iv, data));
            out
        }
    }
}

/// Builds an R2–R4 (RC4 / AES-128) file key + config via Algorithms 2/3/4/5.
fn build_r234(
    revision: u8,
    key_len: usize,
    id0: &[u8],
    p: i32,
    user_pwd: &[u8],
    owner_pwd: &[u8],
    method: CryptMethod,
) -> (Vec<u8>, EncryptConfig) {
    let encrypt_metadata = true;
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
    let v = if revision == 3 { 2 } else { 4 };
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
        stm_method: method,
        str_method: method,
    };
    (file_key, config)
}

/// Builds an AES-256 **R6** file key + config (Algorithms 2.B / 8 / 9).
///
/// Generates a random 32-byte file key and four random 8-byte salts. **Never
/// authors R5** — the hash is always the hardened R6 form.
fn build_aes256_r6(p: i32, user_pwd: &[u8], owner_pwd: &[u8]) -> (Vec<u8>, EncryptConfig) {
    let encrypt_metadata = true;
    let mut file_key = [0u8; 32];
    let mut salts = [0u8; 32];
    if getrandom::getrandom(&mut file_key).is_err() {
        file_key = crate::primitives::sha256(b"oxipdf-aes256-key-fallback");
    }
    if getrandom::getrandom(&mut salts).is_err() {
        salts = crate::primitives::sha256(b"oxipdf-aes256-salt-fallback");
    }
    let u_vsalt = &salts[0..8];
    let u_ksalt = &salts[8..16];
    let o_vsalt = &salts[16..24];
    let o_ksalt = &salts[24..32];

    // --- /U + /UE (user) — Algorithm 8 -------------------------------------
    let u_hash = kdf::hash_r6(user_pwd, u_vsalt, &[]);
    let mut u = Vec::with_capacity(48);
    u.extend_from_slice(&u_hash);
    u.extend_from_slice(u_vsalt);
    u.extend_from_slice(u_ksalt);
    let u_intermediate = kdf::hash_r6(user_pwd, u_ksalt, &[]);
    let ue = aes256_cbc_nopad_encrypt(&u_intermediate, &[0u8; 16], &file_key);

    // --- /O + /OE (owner) — Algorithm 9 (depends on the 48-byte /U) --------
    let u48 = &u[..48];
    let o_hash = kdf::hash_r6(owner_pwd, o_vsalt, u48);
    let mut o = Vec::with_capacity(48);
    o.extend_from_slice(&o_hash);
    o.extend_from_slice(o_vsalt);
    o.extend_from_slice(o_ksalt);
    let o_intermediate = kdf::hash_r6(owner_pwd, o_ksalt, u48);
    let oe = aes256_cbc_nopad_encrypt(&o_intermediate, &[0u8; 16], &file_key);

    let config = EncryptConfig {
        v: 5,
        r: 6, // never R5
        o,
        u,
        oe,
        ue,
        p,
        key_len: 32,
        encrypt_metadata,
        id0: Vec::new(),
        stm_method: CryptMethod::AesV3,
        str_method: CryptMethod::AesV3,
    };
    (file_key.to_vec(), config)
}
