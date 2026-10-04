//! The PRD §9.6.2 ceilings that used to be declared but never enforced:
//! `max_file_size` (at open), `max_decode_ratio` (decompression-bomb ratio,
//! past `Limits::decode_ratio_floor`) and `max_total_decompressed` (distinct
//! non-image streams a document decodes).

mod common;

use common::{dict, flate_encode, flate_stream, name_obj, rref, simple_doc, Pdf};
use pdf_core::filters::lzw;
use pdf_core::{DecodeOutcome, DocumentStore, Error, Limits, Name, Object, StreamObj};

/// The stable `LimitKind` discriminant of a limit error (`None` otherwise).
fn limit_kind(e: &Error) -> Option<String> {
    match e {
        Error::LimitExceeded(k) => Some(k.as_str().to_owned()),
        _ => None,
    }
}

fn kind(s: &str) -> Option<String> {
    Some(s.to_owned())
}

#[test]
fn files_larger_than_max_file_size_are_refused_at_open() {
    let bytes = simple_doc();
    let mut limits = Limits::default();
    limits.max_file_size = bytes.len() as u64 - 1;
    let err = DocumentStore::from_bytes(bytes.clone(), limits).expect_err("too large");
    assert_eq!(limit_kind(&err), kind("file-size"));
    limits.max_file_size = bytes.len() as u64;
    assert!(DocumentStore::from_bytes(bytes, limits).is_ok());
}

/// A 16 MiB per-stream cap puts the ratio floor at 4 MiB.
fn small_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_decompressed_stream = 16 * 1024 * 1024;
    limits
}

fn flate_dict(filters: usize) -> pdf_core::Dict {
    let names = vec![name_obj("FlateDecode"); filters];
    dict([("Filter", Object::Array(names))])
}

#[test]
fn a_flate_bomb_trips_the_decode_ratio() {
    let zeros = vec![0u8; 8 * 1024 * 1024];
    let bomb = flate_encode(&zeros);
    assert!(zeros.len() > bomb.len() * 200, "precondition: ratio > 200");
    let err = pdf_core::decode_stream(&flate_dict(1), &bomb, &small_limits()).expect_err("bomb");
    assert_eq!(limit_kind(&err), kind("decode-ratio"));
    // Two stacked Flate layers multiply the ratio; the chain is checked too.
    let double = flate_encode(&bomb);
    let err = pdf_core::decode_stream(&flate_dict(2), &double, &small_limits()).expect_err("x2");
    assert_eq!(limit_kind(&err), kind("decode-ratio"));
}

#[test]
fn an_lzw_bomb_trips_the_decode_ratio() {
    let zeros = vec![0u8; 8 * 1024 * 1024];
    let bomb = lzw::encode(&zeros, true);
    if zeros.len() <= bomb.len() * 200 {
        return; // LZW cannot reach 200:1 on this input; nothing to check.
    }
    let d = dict([("Filter", name_obj("LZWDecode"))]);
    let err = pdf_core::decode_stream(&d, &bomb, &small_limits()).expect_err("bomb");
    assert_eq!(limit_kind(&err), kind("decode-ratio"));
}

#[test]
fn highly_compressible_streams_below_the_floor_still_decode() {
    // 1 MiB of zeros compresses ~1000:1 — legal and common (blank image rows);
    // below the floor the ratio is never checked.
    let zeros = vec![0u8; 1024 * 1024];
    let out = pdf_core::decode_stream(&flate_dict(1), &flate_encode(&zeros), &small_limits())
        .expect("below the floor");
    assert_eq!(out, DecodeOutcome::Decoded(zeros.clone()));
    // With the default limits the floor is 256 MiB.
    let big = vec![0u8; 8 * 1024 * 1024];
    let out = pdf_core::decode_stream(&flate_dict(1), &flate_encode(&big), &Limits::default())
        .expect("default limits");
    assert_eq!(out, DecodeOutcome::Decoded(big));
}

/// A document with three 1 MiB content-like streams (objects 10–12), one
/// 1 MiB image (13) and a Flate stream (14).
fn streams_doc(limits: Limits) -> DocumentStore {
    let body = |c: u8| vec![c; 1024 * 1024];
    let raw = |extra: Vec<(&'static str, Object)>, b: Vec<u8>| {
        let mut d = dict(extra);
        d.insert(Name::new("Length"), Object::Integer(b.len() as i64));
        Object::Stream(StreamObj::new_encoded(d, b))
    };
    let bytes = Pdf::new()
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
                ("Kids", Object::Array(vec![])),
                ("Count", Object::Integer(0)),
            ])),
        )
        .obj(10, 0, raw(vec![], body(b'a')))
        .obj(11, 0, raw(vec![], body(b'b')))
        .obj(12, 0, raw(vec![], body(b'c')))
        .obj(
            13,
            0,
            raw(
                vec![
                    ("Type", name_obj("XObject")),
                    ("Subtype", name_obj("Image")),
                    ("Width", Object::Integer(1024)),
                    ("Height", Object::Integer(1024)),
                    ("BitsPerComponent", Object::Integer(8)),
                    ("ColorSpace", name_obj("DeviceGray")),
                ],
                body(b'd'),
            ),
        )
        .obj(14, 0, flate_stream([], &body(b'e')))
        .root(1, 0)
        .build();
    DocumentStore::from_bytes(bytes, limits).expect("open")
}

fn decode(doc: &DocumentStore, num: u32) -> pdf_core::Result<DecodeOutcome> {
    let obj = doc.get_object(num, 0).expect("object");
    doc.decode_stream(obj.as_stream().expect("stream"))
}

#[test]
fn distinct_streams_are_charged_against_the_document_total() {
    let mut limits = Limits::default();
    limits.max_total_decompressed = 2 * 1024 * 1024 + 512 * 1024;
    let doc = streams_doc(limits);
    assert!(decode(&doc, 10).is_ok());
    assert!(decode(&doc, 11).is_ok());
    let err = decode(&doc, 12).expect_err("third MiB is over budget");
    assert_eq!(limit_kind(&err), kind("total-decompressed"));
    // Streams already charged keep decoding: repeated access is free.
    for _ in 0..10 {
        assert!(decode(&doc, 10).is_ok());
        assert!(decode(&doc, 11).is_ok());
    }
    // Image XObjects are exempt from the document total.
    assert!(decode(&doc, 13).is_ok());
    // A Flate stream past the budget is refused like any other.
    let err = decode(&doc, 14).expect_err("over budget");
    assert_eq!(limit_kind(&err), kind("total-decompressed"));
}

#[test]
fn default_limits_leave_ordinary_documents_alone() {
    let doc = streams_doc(Limits::default());
    for _ in 0..3 {
        for num in 10..=14 {
            assert!(decode(&doc, num).is_ok(), "{num}");
        }
    }
}
