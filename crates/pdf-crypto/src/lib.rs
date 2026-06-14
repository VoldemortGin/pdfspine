#![forbid(unsafe_code)]
//! `pdf-crypto` — the PDF Standard Security Handler (PRD §8.4).
//!
//! Implements the **read** path for revisions R2–R6:
//! - **R2** RC4-40, **R3** RC4-40..128 (MD5×50), **R4** crypt filters
//!   (`/CF /StmF /StrF`, RC4 `/V2` or AES-128 `/AESV2`, `/EncryptMetadata`),
//! - **R5** AES-256 *transitional* (single SHA-256 validation — read-only),
//! - **R6** AES-256 `/AESV3 /V 5` (Algorithm 2.B hardened iterated hash).
//!
//! Primitives are RustCrypto (AES/CBC/SHA-2/MD5, all MIT/Apache-2.0) plus a
//! hand-rolled RC4 (PDF needs runtime-length keys; PRD §6.4). No `unsafe`.
//!
//! The crate has **no dependency on `pdf-core`** — the caller extracts the raw
//! `/Encrypt` fields and `/ID[0]` into plain bytes/ints and constructs an
//! [`EncryptConfig`]; `pdf-core` wires the resulting [`Decryptor`] into
//! `resolve()` behind its `encryption` feature (PRD §9.1).
//!
//! Encryption **authoring** (computing `/O`/`/U`/`/OE`/`/UE`, encrypting) is M3;
//! the matching encrypt-side primitives live here as `pub` helpers used by the
//! test fixtures and reusable by M3 (see [`kdf`] / [`primitives`]).

pub mod error;
pub mod handler;
pub mod kdf;
pub mod primitives;

#[cfg(any(test, feature = "test-support"))]
pub mod testsupport;

pub use error::{CryptoError, Result};
pub use handler::{AuthRole, CryptMethod, Decryptor, EncryptConfig};
