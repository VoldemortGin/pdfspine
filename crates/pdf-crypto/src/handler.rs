//! The Standard Security Handler façade (PRD §8.4): a parsed `/Encrypt`
//! configuration ([`EncryptConfig`]) plus a stateful [`Decryptor`] that
//! authenticates a password and decrypts per-object strings / stream bodies.
//!
//! `pdf-crypto` does **not** depend on `pdf-core`. The caller (`pdf-core`,
//! behind its `encryption` feature) extracts the raw `/Encrypt` fields and the
//! document `/ID[0]` into plain bytes/ints and builds an [`EncryptConfig`] here.

use crate::error::CryptoError;
use crate::kdf::{self, Aes256Hash, R234Inputs};
use crate::primitives::{aes128_cbc_decrypt, aes256_cbc_decrypt, md5, rc4};

/// The cipher a crypt filter applies (PRD §8.4: `/V2`/`AESV2`/`AESV3`/`Identity`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CryptMethod {
    /// No-op (`/Identity`): the data is not encrypted.
    Identity,
    /// RC4 (`/V2`, or implicit V≤2).
    Rc4,
    /// AES-128-CBC (`/AESV2`).
    AesV2,
    /// AES-256-CBC (`/AESV3`).
    AesV3,
}

/// The parsed `/Encrypt` configuration (PRD §8.4). Built by `pdf-core` from the
/// `/Encrypt` dict + `/ID[0]`.
#[derive(Clone, Debug)]
pub struct EncryptConfig {
    /// `/V` algorithm version (1, 2, 4, 5).
    pub v: u8,
    /// `/R` handler revision (2, 3, 4, 5, 6).
    pub r: u8,
    /// `/O` owner entry.
    pub o: Vec<u8>,
    /// `/U` user entry.
    pub u: Vec<u8>,
    /// `/OE` owner-key entry (R5/R6 only; empty otherwise).
    pub oe: Vec<u8>,
    /// `/UE` user-key entry (R5/R6 only; empty otherwise).
    pub ue: Vec<u8>,
    /// `/P` permission flags (signed 32-bit).
    pub p: i32,
    /// File-key length in **bytes** (`/Length`/8; default 5 for R2, 16 for AES-256).
    pub key_len: usize,
    /// `/EncryptMetadata` (default true).
    pub encrypt_metadata: bool,
    /// `/ID[0]` (empty slice when `/ID` is absent — the documented fallback).
    pub id0: Vec<u8>,
    /// The crypt method for stream bodies (`/StmF` resolved through `/CF`).
    pub stm_method: CryptMethod,
    /// The crypt method for strings (`/StrF` resolved through `/CF`).
    pub str_method: CryptMethod,
}

impl EncryptConfig {
    /// Whether this is an AES-256 handler (`/V 5`, R5/R6).
    #[must_use]
    pub fn is_aes256(&self) -> bool {
        self.v == 5 || self.r >= 5
    }

    fn aes256_hash(&self) -> Aes256Hash {
        if self.r == 5 {
            Aes256Hash::R5
        } else {
            Aes256Hash::R6
        }
    }

    /// Validates the configuration's self-consistency before any auth attempt.
    fn validate(&self) -> Result<(), CryptoError> {
        match self.r {
            2..=4 => {
                if self.o.len() < 32 || self.u.len() < 32 {
                    return Err(CryptoError::Malformed("/O or /U shorter than 32 bytes"));
                }
                if self.key_len == 0 || self.key_len > 16 {
                    return Err(CryptoError::Malformed("invalid R2-R4 key length"));
                }
            }
            5 | 6 => {
                if self.o.len() < 48 || self.u.len() < 48 {
                    return Err(CryptoError::Malformed(
                        "/O or /U shorter than 48 bytes (AES-256)",
                    ));
                }
                if self.oe.len() < 32 || self.ue.len() < 32 {
                    return Err(CryptoError::Malformed("/OE or /UE shorter than 32 bytes"));
                }
            }
            _ => return Err(CryptoError::Unsupported("unknown /R revision")),
        }
        Ok(())
    }
}

/// Which role a successful authentication matched.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AuthRole {
    /// The user (open) password matched. The empty password is the common case.
    User,
    /// The owner password matched (full permissions).
    Owner,
}

/// The stateful security handler: a parsed config plus, once authenticated, the
/// derived file key and the role that authenticated (PRD §8.4).
#[derive(Clone, Debug)]
pub struct Decryptor {
    config: EncryptConfig,
    file_key: Option<Vec<u8>>,
    role: Option<AuthRole>,
}

impl Decryptor {
    /// Builds a decryptor from a parsed config, validating self-consistency.
    ///
    /// # Errors
    /// [`CryptoError::Malformed`] / [`CryptoError::Unsupported`] when the config
    /// is structurally invalid or the revision is unknown.
    pub fn new(config: EncryptConfig) -> Result<Self, CryptoError> {
        config.validate()?;
        Ok(Decryptor {
            config,
            file_key: None,
            role: None,
        })
    }

    /// The permission flags (`/P`, advisory — exposed, not enforced; PRD §8.4).
    #[must_use]
    pub fn permissions(&self) -> i32 {
        self.config.p
    }

    /// `true` until [`Decryptor::authenticate`] has succeeded.
    #[must_use]
    pub fn needs_pass(&self) -> bool {
        self.file_key.is_none()
    }

    /// The authenticated role, if any.
    #[must_use]
    pub fn role(&self) -> Option<AuthRole> {
        self.role
    }

    /// The parsed config (read-only).
    #[must_use]
    pub fn config(&self) -> &EncryptConfig {
        &self.config
    }

    /// Attempts to authenticate `password`, trying the **user** role first then
    /// the **owner** role (PRD §8.4). On success the file key is stashed and the
    /// decryptor becomes usable; the matched [`AuthRole`] is returned.
    ///
    /// Pass an empty slice for the common empty-user-password case.
    ///
    /// # Errors
    /// [`CryptoError::NeedsPassword`] when neither role matches;
    /// [`CryptoError`] for a malformed config / cipher failure.
    pub fn authenticate(&mut self, password: &[u8]) -> Result<AuthRole, CryptoError> {
        if self.config.is_aes256() {
            self.authenticate_aes256(password)
        } else {
            self.authenticate_r234(password)
        }
    }

    fn authenticate_r234(&mut self, password: &[u8]) -> Result<AuthRole, CryptoError> {
        let c = &self.config;
        // --- user role -----------------------------------------------------
        let user_key = kdf::derive_key_r234(&R234Inputs {
            password,
            o: &c.o,
            p: c.p,
            id0: &c.id0,
            revision: c.r,
            key_len: c.key_len,
            encrypt_metadata: c.encrypt_metadata,
        });
        if kdf::check_user_r234(&user_key, &c.id0, c.r, &c.u) {
            self.file_key = Some(user_key);
            self.role = Some(AuthRole::User);
            return Ok(AuthRole::User);
        }
        // --- owner role: recover the user password, then derive the key ----
        let recovered_user = kdf::owner_user_pwd_r234(password, &c.o, c.r, c.key_len);
        let owner_key = kdf::derive_key_r234(&R234Inputs {
            password: &recovered_user,
            o: &c.o,
            p: c.p,
            id0: &c.id0,
            revision: c.r,
            key_len: c.key_len,
            encrypt_metadata: c.encrypt_metadata,
        });
        if kdf::check_user_r234(&owner_key, &c.id0, c.r, &c.u) {
            self.file_key = Some(owner_key);
            self.role = Some(AuthRole::Owner);
            return Ok(AuthRole::Owner);
        }
        Err(CryptoError::NeedsPassword)
    }

    fn authenticate_aes256(&mut self, password: &[u8]) -> Result<AuthRole, CryptoError> {
        let c = &self.config;
        let hash = c.aes256_hash();
        // --- user role -----------------------------------------------------
        if kdf::check_user_aes256(password, &c.u, hash) {
            let key = kdf::recover_key_user_aes256(password, &c.u, &c.ue, hash)?;
            self.file_key = Some(key.to_vec());
            self.role = Some(AuthRole::User);
            return Ok(AuthRole::User);
        }
        // --- owner role ----------------------------------------------------
        if kdf::check_owner_aes256(password, &c.o, &c.u, hash) {
            let key = kdf::recover_key_owner_aes256(password, &c.o, &c.oe, &c.u, hash)?;
            self.file_key = Some(key.to_vec());
            self.role = Some(AuthRole::Owner);
            return Ok(AuthRole::Owner);
        }
        Err(CryptoError::NeedsPassword)
    }

    /// The derived file encryption key, if authenticated (test / introspection).
    #[must_use]
    pub fn file_key(&self) -> Option<&[u8]> {
        self.file_key.as_deref()
    }

    /// Decrypts a **string** for object `num gen` using `/StrF` (PRD §8.4).
    ///
    /// # Errors
    /// [`CryptoError::NeedsPassword`] if not authenticated; cipher failures.
    pub fn decrypt_string(&self, num: u32, gen: u16, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.decrypt(num, gen, data, self.config.str_method)
    }

    /// Decrypts a **stream body** for object `num gen` using `/StmF` (PRD §8.4).
    ///
    /// # Errors
    /// As [`Decryptor::decrypt_string`].
    pub fn decrypt_stream(&self, num: u32, gen: u16, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.decrypt(num, gen, data, self.config.stm_method)
    }

    /// Core decrypt: derive the per-object key (or use the file key for AESV3),
    /// then apply `method`. `Identity` is a verbatim copy (PRD §8.4 exemption).
    fn decrypt(
        &self,
        num: u32,
        gen: u16,
        data: &[u8],
        method: CryptMethod,
    ) -> Result<Vec<u8>, CryptoError> {
        if method == CryptMethod::Identity {
            return Ok(data.to_vec());
        }
        let file_key = self.file_key.as_deref().ok_or(CryptoError::NeedsPassword)?;

        match method {
            CryptMethod::Identity => unreachable!(),
            CryptMethod::AesV3 => {
                // AES-256: file key used directly, no per-object derivation.
                if file_key.len() != 32 {
                    return Err(CryptoError::Malformed("AESV3 file key not 32 bytes"));
                }
                let mut key = [0u8; 32];
                key.copy_from_slice(file_key);
                aes_decrypt_obj(&AesKey::K256(key), data)
            }
            CryptMethod::Rc4 => {
                let obj_key = per_object_key(file_key, num, gen, false);
                Ok(rc4(&obj_key, data))
            }
            CryptMethod::AesV2 => {
                let obj_key = per_object_key(file_key, num, gen, true);
                if obj_key.len() != 16 {
                    // AESV2 always uses a 16-byte object key (min(len+5,16) with
                    // a 16-byte file key gives 16; shorter file keys still cap at
                    // 16). Pad/clamp defensively to 16.
                    let mut k = [0u8; 16];
                    let n = obj_key.len().min(16);
                    k[..n].copy_from_slice(&obj_key[..n]);
                    aes_decrypt_obj(&AesKey::K128(k), data)
                } else {
                    let mut k = [0u8; 16];
                    k.copy_from_slice(&obj_key);
                    aes_decrypt_obj(&AesKey::K128(k), data)
                }
            }
        }
    }
}

/// An AES object key (128 or 256 bit) for the per-object decrypt path.
enum AesKey {
    K128([u8; 16]),
    K256([u8; 32]),
}

/// AES-CBC decrypt of object data: the **first 16 bytes are the IV** (stripped),
/// the remainder is PKCS#7-padded ciphertext (PRD §8.4).
fn aes_decrypt_obj(key: &AesKey, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < 16 {
        return Err(CryptoError::DecryptFailed(
            "AES object data shorter than the 16-byte IV",
        ));
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&data[..16]);
    let ct = &data[16..];
    if ct.is_empty() {
        // An empty (IV-only) body decrypts to nothing.
        return Ok(Vec::new());
    }
    match key {
        AesKey::K128(k) => aes128_cbc_decrypt(k, &iv, ct),
        AesKey::K256(k) => aes256_cbc_decrypt(k, &iv, ct),
    }
}

/// Per-object key derivation for RC4 / AESV2 (PRD §8.4): the first
/// `min(filekey_len + 5, 16)` bytes of `MD5(filekey ‖ objnum[3 LE] ‖ gen[2 LE]
/// [‖ "sAlT" iff AESV2])`.
#[must_use]
pub fn per_object_key(file_key: &[u8], num: u32, gen: u16, aesv2: bool) -> Vec<u8> {
    let mut buf = Vec::with_capacity(file_key.len() + 5 + 4);
    buf.extend_from_slice(file_key);
    buf.push((num & 0xff) as u8);
    buf.push(((num >> 8) & 0xff) as u8);
    buf.push(((num >> 16) & 0xff) as u8);
    buf.push((gen & 0xff) as u8);
    buf.push(((gen >> 8) & 0xff) as u8);
    if aesv2 {
        buf.extend_from_slice(&[0x73, 0x41, 0x6C, 0x54]); // "sAlT"
    }
    let digest = md5(&buf);
    let out_len = (file_key.len() + 5).min(16);
    digest[..out_len].to_vec()
}
