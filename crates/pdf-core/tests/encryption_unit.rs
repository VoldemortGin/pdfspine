//! `CRYPT-DOC-*` / `CRYPT-EXEMPT-*` — `DocumentStore` decryption integration
//! (PRD §8.4 / §9.1). Built only under `--features encryption`; the fixtures are
//! self-generated via `pdf_crypto::testsupport` (no external files).

#![cfg(feature = "encryption")]

mod common;

use common::{dict, name_obj, rref, Pdf};

use pdf_core::object::{Name, Object, PdfString, StreamObj};
use pdf_core::{DocumentStore, Limits, ParseMode};

use pdf_crypto::handler::CryptMethod;
use pdf_crypto::kdf::Aes256Hash;
use pdf_crypto::testsupport::{build_aes256, build_r234, Fixture};
use pdf_crypto::EncryptConfig;

/// Builds an `/Encrypt` dictionary object from a config (R2–R4 / RC4 / AES-128).
fn encrypt_dict_r234(cfg: &EncryptConfig) -> Object {
    let mut d = dict([
        ("Filter", name_obj("Standard")),
        ("V", Object::Integer(i64::from(cfg.v))),
        ("R", Object::Integer(i64::from(cfg.r))),
        ("O", Object::String(PdfString::literal(cfg.o.clone()))),
        ("U", Object::String(PdfString::literal(cfg.u.clone()))),
        ("P", Object::Integer(i64::from(cfg.p))),
        ("Length", Object::Integer((cfg.key_len * 8) as i64)),
    ]);
    if !cfg.encrypt_metadata {
        d.insert(Name::new("EncryptMetadata"), Object::Boolean(false));
    }
    // Crypt filters for V≥4.
    if cfg.v >= 4 {
        let cfm = match cfg.stm_method {
            CryptMethod::Rc4 => "V2",
            CryptMethod::AesV2 => "AESV2",
            CryptMethod::AesV3 => "AESV3",
            CryptMethod::Identity => "Identity",
        };
        let stdcf = dict([("CFM", name_obj(cfm))]);
        let cf = dict([("StdCF", Object::Dictionary(stdcf))]);
        d.insert(Name::new("CF"), Object::Dictionary(cf));
        d.insert(Name::new("StmF"), name_obj("StdCF"));
        d.insert(Name::new("StrF"), name_obj("StdCF"));
    }
    Object::Dictionary(d)
}

/// Builds an `/Encrypt` dictionary object for AES-256 (R5/R6).
fn encrypt_dict_aes256(cfg: &EncryptConfig) -> Object {
    let stdcf = dict([("CFM", name_obj("AESV3"))]);
    let cf = dict([("StdCF", Object::Dictionary(stdcf))]);
    let mut d = dict([
        ("Filter", name_obj("Standard")),
        ("V", Object::Integer(5)),
        ("R", Object::Integer(i64::from(cfg.r))),
        ("O", Object::String(PdfString::literal(cfg.o.clone()))),
        ("U", Object::String(PdfString::literal(cfg.u.clone()))),
        ("OE", Object::String(PdfString::literal(cfg.oe.clone()))),
        ("UE", Object::String(PdfString::literal(cfg.ue.clone()))),
        ("P", Object::Integer(i64::from(cfg.p))),
        ("Length", Object::Integer(256)),
        ("CF", Object::Dictionary(cf)),
        ("StmF", name_obj("StdCF")),
        ("StrF", name_obj("StdCF")),
    ]);
    if !cfg.encrypt_metadata {
        d.insert(Name::new("EncryptMetadata"), Object::Boolean(false));
    }
    Object::Dictionary(d)
}

const SECRET_STR: &[u8] = b"Confidential /Title text";
const SECRET_STM: &[u8] = b"BT /F1 12 Tf (hidden body content) Tj ET";

/// Assembles a minimal encrypted PDF whose object 4 holds an encrypted string
/// and object 5 holds a stream with an encrypted (then FlateDecode-free) body.
/// Returns the file bytes. `encrypt_dict` is the serialized `/Encrypt` object.
fn build_encrypted_pdf(fx: &Fixture, encrypt_dict: Object) -> Vec<u8> {
    // Object 4: a dictionary carrying an encrypted string value.
    let enc_str = fx.encrypt_string(4, 0, SECRET_STR, None);
    let obj4 = Object::Dictionary(dict([
        ("Type", name_obj("Info")),
        ("Title", Object::String(PdfString::literal(enc_str))),
    ]));

    // Object 5: a stream whose body is the encrypted plaintext (no /Filter, so
    // decode is a passthrough — we test decryption, not codecs).
    let enc_stm = fx.encrypt_stream(5, 0, SECRET_STM, None);
    let stm_dict = dict([("Length", Object::Integer(enc_stm.len() as i64))]);
    let obj5 = Object::Stream(StreamObj::new_encoded(stm_dict, enc_stm));

    let pages = Object::Dictionary(dict([
        ("Type", name_obj("Pages")),
        ("Kids", Object::Array(vec![])),
        ("Count", Object::Integer(0)),
    ]));
    let catalog = Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))]));

    Pdf::new()
        .obj(1, 0, catalog)
        .obj(2, 0, pages)
        .obj(4, 0, obj4)
        .obj(5, 0, obj5)
        .root(1, 0)
        .trailer_key("Encrypt", encrypt_dict)
        .trailer_key(
            "ID",
            Object::Array(vec![
                Object::String(PdfString::hex(fx.config.id0.clone())),
                Object::String(PdfString::hex(fx.config.id0.clone())),
            ]),
        )
        .build()
}

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("open encrypted doc")
}

fn resolved_title(doc: &DocumentStore) -> Vec<u8> {
    let obj = doc.resolve(pdf_core::ObjRef::new(4, 0)).unwrap();
    let d = obj.as_dict().unwrap();
    d.get(&Name::new("Title"))
        .and_then(Object::as_string)
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn resolved_stream_body(doc: &DocumentStore) -> Vec<u8> {
    let obj = doc.resolve(pdf_core::ObjRef::new(5, 0)).unwrap();
    let stream = obj.as_stream().unwrap();
    doc.decode_stream(stream).unwrap().into_decoded().unwrap()
}

#[test]
fn crypt_doc_001_encrypted_opens_needs_pass() {
    let fx = build_r234(
        3,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let bytes = build_encrypted_pdf(&fx, encrypt_dict_r234(&fx.config));
    let doc = open(&bytes);
    assert!(doc.is_encrypted());
    assert!(doc.needs_pass());
    assert_eq!(doc.permissions(), Some(-44));
}

#[test]
fn crypt_doc_002_authenticate_then_resolve_decrypts_string() {
    let fx = build_r234(
        3,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let bytes = build_encrypted_pdf(&fx, encrypt_dict_r234(&fx.config));
    let doc = open(&bytes);
    doc.authenticate(b"").unwrap();
    assert!(!doc.needs_pass());
    assert_eq!(resolved_title(&doc), SECRET_STR);
}

#[test]
fn crypt_doc_003_authenticate_then_decode_stream_decrypts_body() {
    let fx = build_r234(
        3,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let bytes = build_encrypted_pdf(&fx, encrypt_dict_r234(&fx.config));
    let doc = open(&bytes);
    doc.authenticate(b"").unwrap();
    assert_eq!(resolved_stream_body(&doc), SECRET_STM);
}

#[test]
fn crypt_doc_002_aesv2_roundtrip() {
    let fx = build_r234(
        4,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::AesV2,
        CryptMethod::AesV2,
    );
    let bytes = build_encrypted_pdf(&fx, encrypt_dict_r234(&fx.config));
    let doc = open(&bytes);
    doc.authenticate(b"").unwrap();
    assert_eq!(resolved_title(&doc), SECRET_STR);
    assert_eq!(resolved_stream_body(&doc), SECRET_STM);
}

#[test]
fn crypt_doc_002_aesv3_r6_roundtrip() {
    let fx = build_aes256(Aes256Hash::R6, [0x5Au8; 32], -4, true, b"", b"owner");
    let bytes = build_encrypted_pdf(&fx, encrypt_dict_aes256(&fx.config));
    let doc = open(&bytes);
    assert!(doc.needs_pass());
    doc.authenticate(b"").unwrap();
    assert_eq!(resolved_title(&doc), SECRET_STR);
    assert_eq!(resolved_stream_body(&doc), SECRET_STM);
}

#[test]
fn crypt_doc_004_unencrypted_doc_needs_no_pass() {
    let catalog = Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))]));
    let pages = Object::Dictionary(dict([
        ("Type", name_obj("Pages")),
        ("Kids", Object::Array(vec![])),
        ("Count", Object::Integer(0)),
    ]));
    let info = Object::Dictionary(dict([(
        "Title",
        Object::String(PdfString::literal(b"plain".to_vec())),
    )]));
    let bytes = Pdf::new()
        .obj(1, 0, catalog)
        .obj(2, 0, pages)
        .obj(4, 0, info)
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert!(!doc.is_encrypted());
    assert!(!doc.needs_pass());
    assert_eq!(doc.permissions(), None);
    // Strings come through untouched.
    assert_eq!(resolved_title(&doc), b"plain");
}

#[test]
fn crypt_exempt_002_encrypt_dict_strings_not_decrypted() {
    // The /Encrypt dict is an indirect object (obj 9). Its /O and /U strings must
    // NOT be decrypted when obj 9 is resolved (PRD §8.4 exemption).
    let fx = build_r234(
        4,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::AesV2,
        CryptMethod::AesV2,
    );
    let enc_str = fx.encrypt_string(4, 0, SECRET_STR, None);
    let obj4 = Object::Dictionary(dict([(
        "Title",
        Object::String(PdfString::literal(enc_str)),
    )]));
    let pages = Object::Dictionary(dict([
        ("Type", name_obj("Pages")),
        ("Kids", Object::Array(vec![])),
        ("Count", Object::Integer(0)),
    ]));
    let catalog = Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))]));
    let bytes = Pdf::new()
        .obj(1, 0, catalog)
        .obj(2, 0, pages)
        .obj(4, 0, obj4)
        .obj(9, 0, encrypt_dict_r234(&fx.config))
        .root(1, 0)
        .trailer_key("Encrypt", rref(9, 0))
        .trailer_key(
            "ID",
            Object::Array(vec![
                Object::String(PdfString::hex(fx.config.id0.clone())),
                Object::String(PdfString::hex(fx.config.id0.clone())),
            ]),
        )
        .build();
    let doc = open(&bytes);
    doc.authenticate(b"").unwrap();
    // The encrypt object resolves with its /O and /U intact (NOT decrypted).
    let enc = doc.resolve(pdf_core::ObjRef::new(9, 0)).unwrap();
    let d = enc.as_dict().unwrap();
    let o = d.get(&Name::new("O")).and_then(Object::as_string).unwrap();
    assert_eq!(o.as_bytes(), fx.config.o.as_slice(), "/O must be untouched");
    // And obj 4's string still decrypts correctly (proves auth worked).
    assert_eq!(resolved_title(&doc), SECRET_STR);
}

#[test]
fn crypt_exempt_004_metadata_clear_when_flag_false() {
    // EncryptMetadata=false: a /Type /Metadata stream is left clear (PRD §8.4).
    let fx = build_r234(
        4,
        16,
        b"id-bytes-0001234",
        -44,
        false,
        b"",
        b"o",
        CryptMethod::AesV2,
        CryptMethod::AesV2,
    );
    let clear_meta = b"<?xpacket plaintext metadata ?>";
    let meta = Object::Stream(StreamObj::new_encoded(
        dict([
            ("Type", name_obj("Metadata")),
            ("Subtype", name_obj("XML")),
            ("Length", Object::Integer(clear_meta.len() as i64)),
        ]),
        clear_meta.to_vec(),
    ));
    // A normal encrypted stream (obj 5) to prove encryption is otherwise active.
    let enc_stm = fx.encrypt_stream(5, 0, SECRET_STM, None);
    let obj5 = Object::Stream(StreamObj::new_encoded(
        dict([("Length", Object::Integer(enc_stm.len() as i64))]),
        enc_stm,
    ));
    let pages = Object::Dictionary(dict([
        ("Type", name_obj("Pages")),
        ("Kids", Object::Array(vec![])),
        ("Count", Object::Integer(0)),
    ]));
    let catalog = Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))]));
    let bytes = Pdf::new()
        .obj(1, 0, catalog)
        .obj(2, 0, pages)
        .obj(5, 0, obj5)
        .obj(6, 0, meta)
        .root(1, 0)
        .trailer_key("Encrypt", encrypt_dict_r234(&fx.config))
        .trailer_key(
            "ID",
            Object::Array(vec![
                Object::String(PdfString::hex(fx.config.id0.clone())),
                Object::String(PdfString::hex(fx.config.id0.clone())),
            ]),
        )
        .build();
    let doc = open(&bytes);
    doc.authenticate(b"").unwrap();
    // Metadata stream body must be the verbatim clear bytes.
    let m = doc.resolve(pdf_core::ObjRef::new(6, 0)).unwrap();
    let mstream = m.as_stream().unwrap();
    assert_eq!(
        doc.decode_stream(mstream).unwrap().into_decoded().unwrap(),
        clear_meta
    );
    // The normal stream still decrypts.
    assert_eq!(resolved_stream_body(&doc), SECRET_STM);
}

#[test]
fn crypt_exempt_005_objstm_member_strings_via_container() {
    // Object 7 is an ObjStm packing object 4 (a dict with a /Title string). The
    // ObjStm *container* body is encrypted (RC4); the member string inside is
    // plaintext. Decryption happens once at the container level — the member is
    // NOT individually re-decrypted (PRD §8.4).
    use common::{objstm_object, write_value};

    let fx = build_r234(
        4,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );

    // Member object 4 with a plaintext title (inside the ObjStm).
    let member = Object::Dictionary(dict([(
        "Title",
        Object::String(PdfString::literal(SECRET_STR.to_vec())),
    )]));
    let objstm = objstm_object(&[(4, write_value(&member))]);
    // Encrypt the container's *raw* (flate-encoded) body for object 7.
    let (raw_body, mut stm_dict) = match objstm {
        Object::Stream(s) => (s.data.owned_bytes().unwrap().to_vec(), s.dict),
        _ => unreachable!(),
    };
    let enc_body = fx.encrypt_stream(7, 0, &raw_body, None);
    stm_dict.insert(Name::new("Length"), Object::Integer(enc_body.len() as i64));
    let enc_objstm = Object::Stream(StreamObj::new_encoded(stm_dict, enc_body));

    // Build with a classic xref but mark object 4 as compressed inside 7 — we
    // need an xref stream for /Type /Compressed entries, so use RawPdf.
    use common::{pack_xref_records, xref_stream_object, RawPdf};
    let catalog = Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))]));
    let pages = Object::Dictionary(dict([
        ("Type", name_obj("Pages")),
        ("Kids", Object::Array(vec![])),
        ("Count", Object::Integer(0)),
    ]));

    let mut p = RawPdf::new();
    p.header();
    let off1 = p.push_object(1, 0, &catalog);
    let off2 = p.push_object(2, 0, &pages);
    let off7 = p.push_object(7, 0, &enc_objstm);

    // XRef stream (object 8) — itself exempt. Records:
    //   0 free; 1,2,7 uncompressed; 4 compressed in 7 idx 0; 8 self.
    let off8 = p.pos();
    let records = vec![
        (0u64, 0u64, 0u64),  // 0 free
        (1, off1 as u64, 0), // 1
        (1, off2 as u64, 0), // 2
        (2, 7u64, 0),        // 4 → compressed in objstm 7, index 0
        (1, off7 as u64, 0), // 7
        (1, off8 as u64, 0), // 8 (self)
    ];
    // Order by object number 0,1,2,4,7,8 — need an /Index since they're sparse.
    let packed = pack_xref_records(&records, [1, 4, 2]);
    let xref = xref_stream_object(
        &packed,
        [1, 4, 2],
        Some(vec![0, 3, 4, 1, 7, 2]), // (0..3),(4..5),(7..9)
        9,
        [
            ("Root", rref(1, 0)),
            ("Encrypt", encrypt_dict_r234(&fx.config)),
            (
                "ID",
                Object::Array(vec![
                    Object::String(PdfString::hex(fx.config.id0.clone())),
                    Object::String(PdfString::hex(fx.config.id0.clone())),
                ]),
            ),
        ],
        Some(7),
    );
    p.push_object(8, 0, &xref);
    p.raw(b"startxref\n");
    p.raw(format!("{off8}\n").as_bytes());
    p.raw(b"%%EOF\n");
    let bytes = p.finish();

    let doc = open(&bytes);
    doc.authenticate(b"").unwrap();
    // Member object 4 resolves with its title intact (decrypted via container 7).
    assert_eq!(resolved_title(&doc), SECRET_STR);
}

#[test]
fn crypt_exempt_003_xref_stream_not_decrypted() {
    // An encrypted doc with an XRef stream: the XRef stream object resolves to a
    // valid /Type /XRef stream whose body decodes (it was never encrypted).
    use common::{pack_xref_records, xref_stream_object, RawPdf};

    let fx = build_r234(
        4,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::AesV2,
        CryptMethod::AesV2,
    );
    let catalog = Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))]));
    let pages = Object::Dictionary(dict([
        ("Type", name_obj("Pages")),
        ("Kids", Object::Array(vec![])),
        ("Count", Object::Integer(0)),
    ]));
    let enc_str = fx.encrypt_string(4, 0, SECRET_STR, None);
    let obj4 = Object::Dictionary(dict([(
        "Title",
        Object::String(PdfString::literal(enc_str)),
    )]));

    let mut p = RawPdf::new();
    p.header();
    let off1 = p.push_object(1, 0, &catalog);
    let off2 = p.push_object(2, 0, &pages);
    let off4 = p.push_object(4, 0, &obj4);
    let off5 = p.pos();
    let records = vec![
        (0u64, 0u64, 0u64),
        (1, off1 as u64, 0),
        (1, off2 as u64, 0),
        (1, off4 as u64, 0),
        (1, off5 as u64, 0),
    ];
    let packed = pack_xref_records(&records, [1, 4, 2]);
    let xref = xref_stream_object(
        &packed,
        [1, 4, 2],
        Some(vec![0, 3, 4, 2]), // (0..3),(4..6)
        6,
        [
            ("Root", rref(1, 0)),
            ("Encrypt", encrypt_dict_r234(&fx.config)),
            (
                "ID",
                Object::Array(vec![
                    Object::String(PdfString::hex(fx.config.id0.clone())),
                    Object::String(PdfString::hex(fx.config.id0.clone())),
                ]),
            ),
        ],
        Some(7),
    );
    p.push_object(5, 0, &xref);
    p.raw(b"startxref\n");
    p.raw(format!("{off5}\n").as_bytes());
    p.raw(b"%%EOF\n");
    let bytes = p.finish();

    let doc = open(&bytes);
    doc.authenticate(b"").unwrap();
    // Resolve the XRef stream object: it must be a /Type /XRef stream whose body
    // decodes cleanly (proves it was not run through the decryptor).
    let xobj = doc.resolve(pdf_core::ObjRef::new(5, 0)).unwrap();
    let xstream = xobj.as_stream().unwrap();
    assert_eq!(
        xstream
            .dict
            .get(&Name::new("Type"))
            .and_then(Object::as_name)
            .unwrap()
            .as_bytes(),
        b"XRef"
    );
    let decoded = doc.decode_stream(xstream).unwrap().into_decoded().unwrap();
    assert_eq!(
        decoded.len(),
        records.len() * 7,
        "XRef body decodes to packed records"
    );
    // And the regular encrypted string still decrypts.
    assert_eq!(resolved_title(&doc), SECRET_STR);
}

#[test]
fn crypt_doc_005_strict_mode_encrypted_opens() {
    // Strict mode opens an encrypted doc too (authentication is a separate step).
    let fx = build_r234(
        3,
        16,
        b"id-bytes-0001234",
        -44,
        true,
        b"",
        b"o",
        CryptMethod::Rc4,
        CryptMethod::Rc4,
    );
    let bytes = build_encrypted_pdf(&fx, encrypt_dict_r234(&fx.config));
    let doc = DocumentStore::from_bytes_with(bytes, ParseMode::Strict, Limits::default()).unwrap();
    doc.authenticate(b"").unwrap();
    assert_eq!(resolved_title(&doc), SECRET_STR);
}
