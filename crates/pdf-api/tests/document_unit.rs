//! `DOC-*` — the `pdf-api::Document`/`Page` read facade (PRD §7 / §9.2). Self-
//! built fixtures (a tiny classic-xref writer over the `pdf-core` serializer).
//! The encrypted-flow tests (`DOC-CRYPT-*`) build fixtures via `pdf-crypto`'s
//! test-support encrypt path and run only under `--features encryption`.

use pdf_api::Document;
use pdf_core::object::{Dict, Name, ObjRef, Object, PdfString};
use pdf_core::serialize::{write_indirect, write_object};

// --- minimal classic-xref PDF writer (test-only) --------------------------

fn dict(pairs: &[(&str, Object)]) -> Dict {
    let mut d = Dict::new();
    for (k, v) in pairs {
        d.insert(Name::new(*k), v.clone());
    }
    d
}

fn name_obj(s: &str) -> Object {
    Object::Name(Name::new(s))
}

fn rref(num: u32, gen: u16) -> Object {
    Object::Reference(ObjRef::new(num, gen))
}

fn int_array(vals: &[i64]) -> Object {
    Object::Array(vals.iter().copied().map(Object::Integer).collect())
}

/// Builds a complete classic-xref PDF from `(num, object)` pairs + trailer keys.
fn build_pdf(objects: &[(u32, Object)], root: u32, extra_trailer: &[(&str, Object)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");

    let mut max_num = 0u32;
    let mut offsets: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (num, obj) in objects {
        offsets.insert(*num, out.len());
        out.extend_from_slice(&write_indirect(ObjRef::new(*num, 0), obj));
        max_num = max_num.max(*num);
    }

    let size = max_num + 1;
    let startxref = out.len();
    out.extend_from_slice(b"xref\n");
    out.extend_from_slice(format!("0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..size {
        match offsets.get(&num) {
            Some(off) => out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }

    let mut trailer = dict(extra_trailer);
    trailer.insert(Name::new("Size"), Object::Integer(i64::from(size)));
    trailer.insert(Name::new("Root"), rref(root, 0));
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(&write_object(&Object::Dictionary(trailer)));
    out.extend_from_slice(b"\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
    out
}

/// A two-page document with an `/Info` dict (obj 5) and a `/Contents` stream.
fn two_page_doc() -> Vec<u8> {
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(2)),
        ("Kids", Object::Array(vec![rref(3, 0), rref(4, 0)])),
        ("MediaBox", int_array(&[0, 0, 200, 300])),
    ]));
    let page1 = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("Rotate", Object::Integer(90)),
    ]));
    let page2 = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", int_array(&[0, 0, 400, 400])),
    ]));
    let info = Object::Dictionary(dict(&[
        (
            "Title",
            Object::String(PdfString::literal(b"Hello Title".to_vec())),
        ),
        (
            "Author",
            Object::String(PdfString::literal(b"Jane Doe".to_vec())),
        ),
        (
            "Producer",
            Object::String(PdfString::literal(b"oxipdf-test".to_vec())),
        ),
        (
            "CreationDate",
            Object::String(PdfString::literal(b"D:20240101000000Z".to_vec())),
        ),
    ]));
    build_pdf(
        &[(1, catalog), (2, pages), (3, page1), (4, page2), (5, info)],
        1,
        &[("Info", rref(5, 0))],
    )
}

// --- DOC-OPEN-* / DOC-PAGE-* ---------------------------------------------

#[test]
fn doc_open_001_open_bytes_pages() {
    // DOC-OPEN-001
    let doc = Document::open_bytes(two_page_doc()).unwrap();
    assert_eq!(doc.page_count(), 2);
    assert!(doc.is_pdf());
    assert_eq!(doc.version(), (1, 7));

    let p0 = doc.load_page(0).unwrap();
    assert_eq!(p0.number(), 0);
    let p1 = doc.load_page(1).unwrap();
    assert_eq!(p1.number(), 1);
}

#[test]
fn doc_open_002_open_path() {
    // DOC-OPEN-002: write to a temp file and open by path.
    let bytes = two_page_doc();
    let dir = std::env::temp_dir();
    let path = dir.join(format!("oxipdf-docopen-{}.pdf", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let doc = Document::open(&path).unwrap();
    assert_eq!(doc.page_count(), 2);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn doc_page_001_out_of_range_errors() {
    // DOC-PAGE-001: out-of-range load_page is a typed error, not a panic.
    let doc = Document::open_bytes(two_page_doc()).unwrap();
    let err = doc.load_page(99).unwrap_err();
    assert_eq!(err.kind(), "syntax");
}

#[test]
fn doc_page_002_pages_iterator() {
    // DOC-PAGE-002: pages() yields each page with the right number + geometry.
    let doc = Document::open_bytes(two_page_doc()).unwrap();
    let pages: Vec<_> = doc.pages().collect();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].number(), 0);
    assert_eq!(pages[1].number(), 1);

    // Page 0 inherits the pages-node MediaBox (200×300) and has /Rotate 90.
    let r0 = pages[0].rect();
    assert_eq!((r0.x0, r0.y0, r0.x1, r0.y1), (0.0, 0.0, 200.0, 300.0));
    assert_eq!(pages[0].rotation(), 90);

    // Page 1 overrides MediaBox to 400×400 and has no rotation.
    let r1 = pages[1].rect();
    assert_eq!((r1.x0, r1.y0, r1.x1, r1.y1), (0.0, 0.0, 400.0, 400.0));
    assert_eq!(pages[1].rotation(), 0);
}

// --- DOC-META-* -----------------------------------------------------------

#[test]
fn doc_meta_001_info_parsed() {
    // DOC-META-001
    let doc = Document::open_bytes(two_page_doc()).unwrap();
    let md = doc.metadata();
    assert_eq!(md.title.as_deref(), Some("Hello Title"));
    assert_eq!(md.author.as_deref(), Some("Jane Doe"));
    assert_eq!(md.producer.as_deref(), Some("oxipdf-test"));
    assert_eq!(md.creation_date.as_deref(), Some("D:20240101000000Z"));
}

#[test]
fn doc_meta_002_format_and_empty_fields() {
    // DOC-META-002
    let doc = Document::open_bytes(two_page_doc()).unwrap();
    let md = doc.metadata();
    assert_eq!(md.format, "PDF 1.7");
    // Absent /Subject etc. → the dict pairs report empty strings (PyMuPDF).
    let pairs = md.as_pairs();
    let subject = pairs.iter().find(|(k, _)| *k == "subject").unwrap();
    assert_eq!(subject.1, "");
    // encryption is empty for a plain doc.
    let enc = pairs.iter().find(|(k, _)| *k == "encryption").unwrap();
    assert_eq!(enc.1, "");
}

#[test]
fn doc_meta_003_utf16be_bom() {
    // DOC-META-003: a UTF-16BE BOM /Title decodes to text.
    // "Hi" = FE FF 00 48 00 69.
    let title_bytes = vec![0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69];
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(0)),
        ("Kids", Object::Array(vec![])),
    ]));
    let info = Object::Dictionary(dict(&[(
        "Title",
        Object::String(PdfString::literal(title_bytes)),
    )]));
    let bytes = build_pdf(
        &[(1, catalog), (2, pages), (5, info)],
        1,
        &[("Info", rref(5, 0))],
    );
    let doc = Document::open_bytes(bytes).unwrap();
    assert_eq!(doc.metadata().title.as_deref(), Some("Hi"));
}

// --- DOC-REPAIR-* ---------------------------------------------------------

#[test]
fn doc_repair_001_broken_file_is_repaired() {
    // DOC-REPAIR-001: strip the startxref so the file must be repaired on open.
    let mut bytes = two_page_doc();
    if let Some(pos) = bytes
        .windows(b"startxref".len())
        .rposition(|w| w == b"startxref")
    {
        bytes.truncate(pos);
    }
    let doc = Document::open_bytes(bytes).unwrap();
    assert!(doc.is_repaired());
    // ...and the repaired document still has its two pages.
    assert_eq!(doc.page_count(), 2);
}

// --- DOC-XREF-* -----------------------------------------------------------

#[test]
fn doc_xref_001_length_and_object() {
    // DOC-XREF-001
    let doc = Document::open_bytes(two_page_doc()).unwrap();
    assert_eq!(doc.xref_length(), 6); // max obj 5 + 1
    let src = doc.xref_object(1).unwrap();
    assert!(src.contains("/Catalog"));
}

#[test]
fn doc_xref_002_key_stream() {
    // DOC-XREF-002: a content-stream object exercises get_key/is_stream/stream.
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(1)),
        ("Kids", Object::Array(vec![rref(3, 0)])),
    ]));
    let page = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("Contents", rref(4, 0)),
    ]));
    // A plain (unfiltered) content stream so the decoded body is verbatim.
    let body = b"BT (hi) Tj ET";
    let stream = Object::Stream(pdf_core::object::StreamObj::new_encoded(
        dict(&[("Length", Object::Integer(body.len() as i64))]),
        body.to_vec(),
    ));
    let bytes = build_pdf(&[(1, catalog), (2, pages), (3, page), (4, stream)], 1, &[]);
    let doc = Document::open_bytes(bytes).unwrap();
    assert_eq!(
        doc.xref_get_key(3, "Type").unwrap().as_deref(),
        Some("/Page")
    );
    assert!(doc.xref_is_stream(4).unwrap());
    assert!(!doc.xref_is_stream(3).unwrap());
    assert_eq!(doc.xref_stream(4).unwrap(), body);
}

// --- DOC-CRYPT-* (encryption feature) ------------------------------------

#[cfg(feature = "encryption")]
mod crypto {
    use super::*;
    use pdf_crypto::handler::CryptMethod;
    use pdf_crypto::testsupport::{build_r234, Fixture};
    use pdf_crypto::EncryptConfig;

    fn encrypt_dict_r234(cfg: &EncryptConfig) -> Object {
        Object::Dictionary(dict(&[
            ("Filter", name_obj("Standard")),
            ("V", Object::Integer(i64::from(cfg.v))),
            ("R", Object::Integer(i64::from(cfg.r))),
            ("O", Object::String(PdfString::literal(cfg.o.clone()))),
            ("U", Object::String(PdfString::literal(cfg.u.clone()))),
            ("P", Object::Integer(i64::from(cfg.p))),
            ("Length", Object::Integer((cfg.key_len * 8) as i64)),
        ]))
    }

    /// A one-page encrypted document whose /Info /Title is an encrypted string.
    fn encrypted_doc(fx: &Fixture) -> Vec<u8> {
        let catalog = Object::Dictionary(dict(&[
            ("Type", name_obj("Catalog")),
            ("Pages", rref(2, 0)),
        ]));
        let pages = Object::Dictionary(dict(&[
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(1)),
            ("Kids", Object::Array(vec![rref(3, 0)])),
            ("MediaBox", int_array(&[0, 0, 100, 100])),
        ]));
        let page = Object::Dictionary(dict(&[("Type", name_obj("Page")), ("Parent", rref(2, 0))]));
        let enc_title = fx.encrypt_string(5, 0, b"Secret Title", None);
        let info = Object::Dictionary(dict(&[(
            "Title",
            Object::String(PdfString::literal(enc_title)),
        )]));
        build_pdf(
            &[(1, catalog), (2, pages), (3, page), (5, info)],
            1,
            &[
                ("Encrypt", encrypt_dict_r234(&fx.config)),
                ("Info", rref(5, 0)),
                (
                    "ID",
                    Object::Array(vec![
                        Object::String(PdfString::hex(fx.config.id0.clone())),
                        Object::String(PdfString::hex(fx.config.id0.clone())),
                    ]),
                ),
            ],
        )
    }

    fn fixture() -> Fixture {
        build_r234(
            3,
            16,
            b"id-bytes-0001234",
            -44,
            true,
            b"",
            b"owner",
            CryptMethod::Rc4,
            CryptMethod::Rc4,
        )
    }

    #[test]
    fn doc_crypt_001_encrypted_facts() {
        // DOC-CRYPT-001
        let fx = fixture();
        let doc = Document::open_bytes(encrypted_doc(&fx)).unwrap();
        assert!(doc.is_encrypted());
        assert!(doc.needs_pass());
        assert_eq!(doc.permissions(), -44);
        let md = doc.metadata();
        assert!(md.encryption.starts_with("Standard"));
    }

    #[test]
    fn doc_crypt_002_authenticate_then_pages() {
        // DOC-CRYPT-002
        let fx = fixture();
        let doc = Document::open_bytes(encrypted_doc(&fx)).unwrap();
        assert!(doc.authenticate(b""));
        assert!(!doc.needs_pass());
        assert_eq!(doc.page_count(), 1);
        let page = doc.load_page(0).unwrap();
        let r = page.rect();
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (0.0, 0.0, 100.0, 100.0));
        // The /Info title now decrypts.
        assert_eq!(doc.metadata().title.as_deref(), Some("Secret Title"));
    }

    #[test]
    fn doc_crypt_003_wrong_password() {
        // DOC-CRYPT-003
        let fx = fixture();
        let doc = Document::open_bytes(encrypted_doc(&fx)).unwrap();
        assert!(!doc.authenticate(b"wrong-password"));
        assert!(doc.needs_pass());
    }
}
