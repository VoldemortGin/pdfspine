//! `/Encrypt`-dictionary parsing and the [`DocumentStore`](crate::DocumentStore)
//! decryption glue (PRD §8.4 / §9.1). Compiled only with the `encryption`
//! feature; the default read-only build does not pull in `pdf-crypto`.
//!
//! This module turns the parsed `/Encrypt` dict + trailer `/ID[0]` into a
//! [`pdf_crypto::EncryptConfig`], then hands it to a [`pdf_crypto::Decryptor`].
//! The store decrypts strings / stream bodies transparently in `resolve()`,
//! honoring the spec's exemptions (PRD §8.4: the `/Encrypt` dict, `/ID`, XRef
//! streams, and `EncryptMetadata=false` metadata are never decrypted; objects
//! inside an ObjStm are decrypted via their container, not individually).

use pdf_crypto::handler::CryptMethod;
use pdf_crypto::EncryptConfig;

use crate::error::Error;
use crate::object::{Dict, Name, Object};

/// Reads `/ID[0]` from the trailer, returning an empty vec when `/ID` is absent
/// (the documented fallback — PRD §8.4). `/ID` strings are *not* decrypted.
#[must_use]
pub fn id0_from_trailer(trailer: &Dict) -> Vec<u8> {
    match trailer.get(&Name::new("ID")) {
        Some(Object::Array(a)) => match a.first() {
            Some(Object::String(s)) => s.as_bytes().to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Maps a crypt-filter `/CFM` name to a [`CryptMethod`].
fn cfm_to_method(name: &[u8]) -> Option<CryptMethod> {
    match name {
        b"V2" => Some(CryptMethod::Rc4),
        b"AESV2" => Some(CryptMethod::AesV2),
        b"AESV3" => Some(CryptMethod::AesV3),
        b"Identity" => Some(CryptMethod::Identity),
        _ => None,
    }
}

/// Resolves a `/StmF` or `/StrF` filter name through the `/CF` dictionary into a
/// concrete [`CryptMethod`]. `/Identity` is the no-op filter; an unknown named
/// filter is an error (PRD §8.4).
fn resolve_filter(cf: Option<&Dict>, filter_name: &[u8]) -> Result<CryptMethod, Error> {
    if filter_name == b"Identity" {
        return Ok(CryptMethod::Identity);
    }
    let cf = cf.ok_or(Error::Unsupported(
        "crypt filter named but no /CF dictionary",
    ))?;
    let entry = cf
        .get(&Name::from_decoded(filter_name.to_vec()))
        .and_then(Object::as_dict)
        .ok_or(Error::Unsupported("named crypt filter not found in /CF"))?;
    let cfm = entry
        .get(&Name::new("CFM"))
        .and_then(Object::as_name)
        .ok_or(Error::Unsupported("crypt filter has no /CFM"))?;
    cfm_to_method(cfm.as_bytes()).ok_or(Error::Unsupported("unsupported /CFM crypt method"))
}

/// Parses the `/Encrypt` dictionary (plus trailer `/ID[0]`) into an
/// [`EncryptConfig`]. Only `/Filter /Standard` is supported; AES-GCM / ISO 32003
/// and non-Standard handlers are *read-tracked* but rejected here with a typed
/// [`Error::Unsupported`] (PRD §8.4).
///
/// # Errors
/// [`Error::Unsupported`] for a non-Standard handler / unknown crypt method;
/// [`Error::Xref`] for a structurally invalid dict (missing `/V`/`/R`/`/O`/`/U`).
pub fn parse_encrypt_dict(enc: &Dict, id0: Vec<u8>) -> Result<EncryptConfig, Error> {
    // /Filter must be /Standard.
    match enc.get(&Name::new("Filter")).and_then(Object::as_name) {
        Some(f) if f.as_bytes() == b"Standard" => {}
        Some(_) => return Err(Error::Unsupported("non-Standard security handler")),
        None => return Err(Error::xref(0, "/Encrypt has no /Filter")),
    }

    let v = enc
        .get(&Name::new("V"))
        .and_then(Object::as_i64)
        .unwrap_or(0) as u8;
    let r = enc
        .get(&Name::new("R"))
        .and_then(Object::as_i64)
        .ok_or(Error::xref(0, "/Encrypt has no /R"))? as u8;

    let get_str = |k: &str| -> Vec<u8> {
        enc.get(&Name::new(k))
            .and_then(Object::as_string)
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default()
    };
    let o = get_str("O");
    let u = get_str("U");
    let oe = get_str("OE");
    let ue = get_str("UE");

    let p = enc
        .get(&Name::new("P"))
        .and_then(Object::as_i64)
        .ok_or(Error::xref(0, "/Encrypt has no /P"))? as i32;

    // /Length is in bits; default 40. For AES-256 the key is always 32 bytes.
    let length_bits = enc
        .get(&Name::new("Length"))
        .and_then(Object::as_i64)
        .unwrap_or(40);
    let key_len = if v >= 5 || r >= 5 {
        32
    } else if r == 2 {
        5
    } else {
        ((length_bits / 8).clamp(5, 16)) as usize
    };

    let encrypt_metadata = enc
        .get(&Name::new("EncryptMetadata"))
        .and_then(Object::as_bool)
        .unwrap_or(true);

    // Crypt-filter resolution (V≥4). For V<4 the cipher is RC4 implicitly.
    let (stm_method, str_method) = if v >= 4 {
        let cf = enc.get(&Name::new("CF")).and_then(Object::as_dict);
        let stmf = enc
            .get(&Name::new("StmF"))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_else(|| b"Identity".to_vec());
        let strf = enc
            .get(&Name::new("StrF"))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_else(|| b"Identity".to_vec());
        (resolve_filter(cf, &stmf)?, resolve_filter(cf, &strf)?)
    } else {
        (CryptMethod::Rc4, CryptMethod::Rc4)
    };

    Ok(EncryptConfig {
        v,
        r,
        o,
        u,
        oe,
        ue,
        p,
        key_len,
        encrypt_metadata,
        id0,
        stm_method,
        str_method,
    })
}

/// Recursively decrypts every string inside `obj` for object `num gen`, leaving
/// non-string leaves untouched. Used after parsing an uncompressed indirect
/// object (PRD §8.4: strings are decrypted; references/names/numbers are not).
///
/// Decryption failures degrade gracefully: a string that fails to decrypt is
/// left as-is (never a panic) — the typed error path is reserved for stream
/// bodies where the caller can react.
pub fn decrypt_strings_in_place(
    obj: &mut Object,
    decryptor: &pdf_crypto::Decryptor,
    num: u32,
    gen: u16,
) {
    match obj {
        Object::String(s) => {
            if let Ok(plain) = decryptor.decrypt_string(num, gen, s.as_bytes()) {
                s.bytes = plain;
            }
        }
        Object::Array(items) => {
            for it in items.iter_mut() {
                decrypt_strings_in_place(it, decryptor, num, gen);
            }
        }
        Object::Dictionary(d) => {
            for v in d.values_mut() {
                decrypt_strings_in_place(v, decryptor, num, gen);
            }
        }
        Object::Stream(s) => {
            for v in s.dict.values_mut() {
                decrypt_strings_in_place(v, decryptor, num, gen);
            }
        }
        _ => {}
    }
}

/// Whether the object `num` is exempt from string/stream decryption (PRD §8.4):
/// the `/Encrypt` dict object itself is never decrypted.
#[must_use]
pub fn is_encrypt_object(num: u32, encrypt_ref_num: Option<u32>) -> bool {
    encrypt_ref_num == Some(num)
}

/// Whether a stream dict denotes an exempt stream body (PRD §8.4): XRef streams
/// (`/Type /XRef`) are never encrypted, and the `/Metadata` stream is left clear
/// when the handler's `EncryptMetadata` is false.
#[must_use]
pub fn is_exempt_stream(dict: &Dict, encrypt_metadata: bool) -> bool {
    let ty = dict.get(&Name::new("Type")).and_then(Object::as_name);
    if let Some(t) = ty {
        if t.as_bytes() == b"XRef" {
            return true;
        }
        if !encrypt_metadata && t.as_bytes() == b"Metadata" {
            return true;
        }
    }
    false
}
