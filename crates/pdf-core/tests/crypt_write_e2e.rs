//! `CRYPT-WRITE-*` — encryption on full save (PRD §8.4 write rules).
//!
//! Each method (RC4-128 / AES-128 / AES-256-R6): build a plaintext doc with an
//! `/Info /Title` string + a content stream, save encrypted, reopen, authenticate
//! with the empty password, and assert the decrypted title + content equal the
//! plaintext. Plus exemption checks (`/ID`, `/Encrypt` dict, xref stream) and a
//! never-R5 assertion.

#![cfg(feature = "encryption")]

mod common;

use common::{dict, name_obj, rref, Pdf, SIMPLE_CONTENT};

use pdf_core::object::Name;
use pdf_crypto::{EncryptMethod, EncryptSpec};

use pdf_core::{DocumentStore, Limits, Object, SaveOptions, StreamObj, StringKind, XrefStyle};

const TITLE: &[u8] = b"Secret Title";

/// A one-page doc with `/Info 6 0 R` (Title) and content stream object 4.
fn doc_with_info() -> Vec<u8> {
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ]);
    let font = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
    ]));
    let resources = Object::Dictionary(dict([(
        "Font",
        Object::Dictionary(dict([("F1", rref(5, 0))])),
    )]));
    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", media),
        ("Contents", rref(4, 0)),
        ("Resources", resources),
    ]));
    let content = Object::Stream(StreamObj::new_encoded(
        dict([("Length", Object::Integer(SIMPLE_CONTENT.len() as i64))]),
        SIMPLE_CONTENT.to_vec(),
    ));
    let info = Object::Dictionary(dict([(
        "Title",
        Object::String(pdf_core::PdfString {
            bytes: TITLE.to_vec(),
            kind: StringKind::Literal,
        }),
    )]));

    Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("Count", Object::Integer(1)),
            ])),
        )
        .obj(3, 0, page)
        .obj(4, 0, content)
        .obj(5, 0, font)
        .obj(6, 0, info)
        .root(1, 0)
        .trailer_key("Info", rref(6, 0))
        .build()
}

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("opens")
}

/// Read back the decrypted `/Info /Title` from an authenticated document.
fn read_title(doc: &DocumentStore) -> Vec<u8> {
    let info_ref = doc.effective_trailer_ref("Info").expect("/Info present");
    let info = doc.resolve(info_ref).unwrap();
    let title = info
        .as_dict()
        .unwrap()
        .get(&Name::new("Title"))
        .and_then(Object::as_string)
        .unwrap();
    title.bytes.clone()
}

/// Read back the decoded content stream of page object 3 → contents 4.
fn read_content(doc: &DocumentStore) -> Vec<u8> {
    let obj = doc.get_object(4, 0).unwrap();
    let stream = obj.as_stream().unwrap();
    doc.decode_stream(stream).unwrap().into_decoded().unwrap()
}

fn roundtrip(method: EncryptMethod) {
    let doc = open(&doc_with_info());
    let opts = SaveOptions::default().with_encrypt(EncryptSpec::new(method));
    let bytes = doc.save_to_vec(&opts).unwrap();

    let re = open(&bytes);
    assert!(re.is_encrypted(), "reopened doc is encrypted");
    re.authenticate(b"").expect("empty password authenticates");

    assert_eq!(read_title(&re), TITLE, "title decrypts to plaintext");
    assert_eq!(
        read_content(&re),
        SIMPLE_CONTENT,
        "content decrypts to plaintext"
    );
}

#[test]
fn crypt_write_rc4() {
    roundtrip(EncryptMethod::Rc4_128);
}

#[test]
fn crypt_write_aes128() {
    roundtrip(EncryptMethod::Aes128);
}

#[test]
fn crypt_write_aes256() {
    roundtrip(EncryptMethod::Aes256R6);
}

/// `CRYPT-WRITE-STR`: the `/Info /Title` is ciphertext on disk (the plaintext
/// "Secret Title" does not appear in the saved bytes), but decrypts on reopen.
#[test]
fn crypt_write_str_encrypted_on_disk() {
    let doc = open(&doc_with_info());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_encrypt(EncryptSpec::new(EncryptMethod::Aes128)))
        .unwrap();
    let plain = b"Secret Title";
    let hit = bytes.windows(plain.len()).any(|w| w == plain);
    assert!(!hit, "plaintext title must not appear in encrypted bytes");
}

/// `CRYPT-WRITE-OWNER` / `CRYPT-WRITE-WRONGPW`: owner-only password — empty user
/// password authenticates; a wrong password is rejected.
#[test]
fn crypt_write_owner_and_wrong_pw() {
    let doc = open(&doc_with_info());
    let spec = EncryptSpec {
        user_pw: Vec::new(),
        owner_pw: b"theowner".to_vec(),
        permissions: -44,
        method: EncryptMethod::Rc4_128,
    };
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_encrypt(spec))
        .unwrap();

    let re = open(&bytes);
    assert!(re.authenticate(b"wrong-password").is_err());
    let re2 = open(&bytes);
    re2.authenticate(b"").expect("empty user password opens");
    assert_eq!(read_title(&re2), TITLE);
}

/// `CRYPT-WRITE-EXEMPT-ID`: the trailer `/ID` strings are clear hex (not
/// encrypted) — they are readable directly from the reopened trailer.
#[test]
fn crypt_write_exempt_id() {
    let doc = open(&doc_with_info());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_encrypt(EncryptSpec::new(EncryptMethod::Rc4_128)))
        .unwrap();
    let re = open(&bytes);
    let id = re.trailer().get(&Name::new("ID")).expect("/ID present");
    let arr = id.as_array().expect("/ID is an array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_string().unwrap().bytes.len(), 16);
}

/// `CRYPT-WRITE-EXEMPT-ENC`: the `/Encrypt` dict's own `/O` and `/U` strings are
/// NOT object-encrypted — the read path uses them verbatim for authentication,
/// which succeeds (would fail if they had been double-encrypted).
#[test]
fn crypt_write_exempt_encrypt_dict() {
    let doc = open(&doc_with_info());
    let bytes = doc
        .save_to_vec(
            &SaveOptions::default().with_encrypt(EncryptSpec::new(EncryptMethod::Aes256R6)),
        )
        .unwrap();
    let re = open(&bytes);
    // Authentication only works if /O,/U,/OE,/UE survived clear.
    re.authenticate(b"")
        .expect("auth uses clear /Encrypt strings");
}

/// `CRYPT-WRITE-EXEMPT-XREF`: with xref-stream style, the xref stream body is not
/// encrypted — the saved file reparses (an encrypted xref stream would be
/// unreadable, breaking the open).
#[test]
fn crypt_write_exempt_xref_stream() {
    let doc = open(&doc_with_info());
    let opts = SaveOptions::default()
        .with_xref_style(XrefStyle::Stream)
        .with_encrypt(EncryptSpec::new(EncryptMethod::Aes128));
    let bytes = doc.save_to_vec(&opts).unwrap();
    let re = open(&bytes);
    re.authenticate(b"").unwrap();
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
    assert_eq!(read_title(&re), TITLE);
}

/// `CRYPT-WRITE-NEVER-R5`: AES-256 is authored as R6, never R5. Read the saved
/// `/Encrypt /R` directly.
#[test]
fn crypt_write_never_r5() {
    let doc = open(&doc_with_info());
    let bytes = doc
        .save_to_vec(
            &SaveOptions::default().with_encrypt(EncryptSpec::new(EncryptMethod::Aes256R6)),
        )
        .unwrap();
    let re = open(&bytes);
    let enc_ref = re
        .effective_trailer_ref("Encrypt")
        .expect("/Encrypt present");
    let enc = re.resolve(enc_ref).unwrap();
    let r = enc
        .as_dict()
        .unwrap()
        .get(&Name::new("R"))
        .and_then(Object::as_i64)
        .unwrap();
    assert_eq!(r, 6, "AES-256 authored as R6");
    assert_ne!(r, 5, "never R5");
}

/// `CRYPT-WRITE-QPDF`: optional `qpdf --decrypt` on the saved file when present.
#[test]
fn crypt_write_qpdf_decrypt() {
    let doc = open(&doc_with_info());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_encrypt(EncryptSpec::new(EncryptMethod::Rc4_128)))
        .unwrap();

    if std::process::Command::new("qpdf")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("CRYPT-WRITE-QPDF: qpdf not on PATH — skipping");
        return;
    }
    let dir = std::env::temp_dir();
    let inp = dir.join(format!("oxide-pdf_cw_in_{}.pdf", std::process::id()));
    let outp = dir.join(format!("oxide-pdf_cw_out_{}.pdf", std::process::id()));
    std::fs::write(&inp, &bytes).unwrap();
    let out = std::process::Command::new("qpdf")
        .arg("--decrypt")
        .arg("--password=")
        .arg(&inp)
        .arg(&outp)
        .output()
        .expect("run qpdf");
    let _ = std::fs::remove_file(&inp);
    let _ = std::fs::remove_file(&outp);
    assert!(
        out.status.success(),
        "qpdf --decrypt failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
