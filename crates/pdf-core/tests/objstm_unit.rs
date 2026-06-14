//! `OBJSTM-*` — object-stream (`/Type /ObjStm`) decoding and resolution.
//! Compressed objects resolve identically to uncompressed; `/N`/`/First`;
//! max-objects limit; corrupt-table robustness. PRD §8.2 / §9.6.2.

mod common;

use common::*;
use pdf_core::source::Source;
use pdf_core::xref::parse_xref_chain;
use pdf_core::{DocumentStore, Error, LimitKind, Limits, Object};

/// Builds a document whose objects 2 and 3 live inside an object stream
/// (object 4), with a `/Type /XRef` stream (object 5) recording the compressed
/// entries. Object 1 is an uncompressed catalog. `n_override` lets a test forge
/// a too-large `/N`.
fn doc_with_objstm(n_override: Option<i64>) -> Vec<u8> {
    let mut p = RawPdf::new();
    p.header();

    // Object 1: uncompressed catalog.
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Catalog")),
            ("Marker", rref(2, 0)),
        ])),
    );
    let o1 = p.offset_of(1);

    // Objects 2 & 3 will be packed into an ObjStm (object 4).
    let m2 = write_value(&Object::Integer(2002));
    let m3 = write_value(&Object::Dictionary(dict([("Inner", Object::Integer(3))])));
    let mut objstm = objstm_object(&[(2, m2), (3, m3)]);
    if let (Some(nv), Object::Stream(s)) = (n_override, &mut objstm) {
        s.dict.insert(n("N"), Object::Integer(nv));
    }
    let o4 = p.push_object(4, 0, &objstm);

    // Object 5: xref stream covering 0..6.
    let xref_off = p.pos();
    let records = vec![
        (0u64, 0u64, 65535u64),  // 0 free
        (1, o1 as u64, 0),       // 1 uncompressed
        (2, 4u64, 0),            // 2 compressed in objstm 4, index 0
        (2, 4u64, 1),            // 3 compressed in objstm 4, index 1
        (1, o4 as u64, 0),       // 4 uncompressed (the objstm itself)
        (1, xref_off as u64, 0), // 5 uncompressed (the xref stream)
    ];
    let widths = [1usize, 2, 2];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(&data, widths, None, 6, [("Root", rref(1, 0))], None);
    p.push_object(5, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    p.finish()
}

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::unbounded_decode()).expect("open")
}

#[test]
fn objstm_001_compressed_resolves_like_uncompressed() {
    // OBJSTM-001: a compressed object resolves to its value.
    let bytes = doc_with_objstm(None);
    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(2, 0).unwrap().as_ref(),
        &Object::Integer(2002)
    );
}

#[test]
fn objstm_002_n_first_multiple_members() {
    // OBJSTM-002: both members of a 2-object stream resolve.
    let bytes = doc_with_objstm(None);
    let doc = open(&bytes);
    let o2 = doc.get_object(2, 0).unwrap();
    let o3 = doc.get_object(3, 0).unwrap();
    assert_eq!(o2.as_ref(), &Object::Integer(2002));
    assert_eq!(
        o3.as_dict().unwrap().get(&n("Inner")).unwrap(),
        &Object::Integer(3)
    );
}

#[test]
fn objstm_003_second_member_index1() {
    // OBJSTM-003: index 1 resolves to the second member (dict), distinct from
    // index 0.
    let bytes = doc_with_objstm(None);
    let doc = open(&bytes);
    let o3 = doc.get_object(3, 0).unwrap();
    assert!(o3.as_dict().is_some());
    assert!(o3.as_ref() != &Object::Integer(2002));
}

#[test]
fn objstm_004_n_exceeds_limit() {
    // OBJSTM-004: a forged huge /N trips Limits::max_objstm_objects.
    let bytes = doc_with_objstm(Some(5_000_000));
    let limits = Limits::unbounded_decode().with_max_objstm_objects(10);
    let doc = DocumentStore::from_bytes(bytes, limits).unwrap();
    let err = doc.get_object(2, 0).unwrap_err();
    assert!(
        matches!(err, Error::LimitExceeded(LimitKind::ObjstmObjects)),
        "{err:?}"
    );
}

#[test]
fn objstm_005_corrupt_offset_table_typed_error() {
    // OBJSTM-005: a /First that lands mid-number / a garbled header → typed
    // error, no panic.
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    let o1 = p.offset_of(1);

    // A deliberately corrupt ObjStm: /N says 2 but the header has only garbage.
    let decoded = b"not numbers here <<>>".to_vec();
    let enc = flate_encode(&decoded);
    let bad = Object::Stream(pdf_core::StreamObj::new_encoded(
        dict([
            ("Type", name_obj("ObjStm")),
            ("N", Object::Integer(2)),
            ("First", Object::Integer(5)),
            ("Filter", name_obj("FlateDecode")),
            ("Length", Object::Integer(enc.len() as i64)),
        ]),
        enc,
    ));
    let o2 = p.push_object(2, 0, &bad);

    let xref_off = p.pos();
    let records = vec![
        (0u64, 0u64, 65535u64),
        (1, o1 as u64, 0),
        (1, o2 as u64, 0),
        (2, 2u64, 0), // object 3 compressed in objstm 2, index 0
        (1, xref_off as u64, 0),
    ];
    let widths = [1usize, 2, 2];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(&data, widths, None, 5, [("Root", rref(1, 0))], None);
    p.push_object(4, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let doc = open(&bytes);
    let err = doc.get_object(3, 0).unwrap_err();
    assert!(matches!(err, Error::Xref { .. }), "{err:?}");
}

#[test]
fn objstm_xref_table_built() {
    // Sanity: the xref table marks objects 2 & 3 as Compressed.
    let bytes = doc_with_objstm(None);
    let src = Source::from_bytes(bytes);
    let xref = parse_xref_chain(&src, 0, &Limits::unbounded_decode()).unwrap();
    assert!(matches!(
        xref.get(2),
        Some(pdf_core::XrefEntry::Compressed {
            objstm_num: 4,
            index: 0
        })
    ));
    assert!(matches!(
        xref.get(3),
        Some(pdf_core::XrefEntry::Compressed {
            objstm_num: 4,
            index: 1
        })
    ));
}
