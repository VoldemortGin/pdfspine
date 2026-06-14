//! `RESOLVE-*`, `STREAM-RAW-*`, `OPEN-*` — `DocumentStore` resolution, lazy
//! arena, source-backed stream decode, header/open behavior. Self-built
//! fixtures (M1a serializer + hand-written xref). PRD §8.2 / §9.2 / §9.6.

mod common;

use common::*;
use pdf_core::{DocumentStore, Error, Limits, Object, StreamData, Version};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::unbounded_decode()).expect("open")
}

/// A minimal valid catalog+pages document; `/Root` is object 1.
fn minimal_doc() -> Vec<u8> {
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
            ])),
        )
        .root(1, 0)
        .build()
}

// --- OPEN-* ---------------------------------------------------------------

#[test]
fn open_001_header_version() {
    // OPEN-001
    let doc = open(&minimal_doc());
    assert_eq!(doc.version(), Version { major: 1, minor: 7 });
    assert_eq!(doc.header_offset(), 0);
}

#[test]
fn open_002_header_offset_bias() {
    // OPEN-002
    let bytes = Pdf::new()
        .prefix(b"\x89GARBAGE-HTTP-HEADER\r\n\r\n")
        .obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert_eq!(doc.header_offset(), 24);
    // And we can still resolve through the biased xref.
    let root = doc.root().unwrap();
    let cat = doc.resolve(root).unwrap();
    assert_eq!(
        cat.as_dict().unwrap().get(&n("Type")).unwrap(),
        &name_obj("Catalog")
    );
}

#[test]
fn open_003_lazy_no_eager_load() {
    // OPEN-003: from_bytes must not pre-load object bodies.
    let doc = open(&minimal_doc());
    assert_eq!(doc.cached_object_count(), 0);
}

#[test]
fn open_004_clean_file_not_repaired() {
    // OPEN-004
    let doc = open(&minimal_doc());
    assert!(!doc.parse_was_repaired());
}

#[test]
fn open_004b_biased_file_is_repaired() {
    // OPEN-004 corollary: header_offset != 0 taints the parse (PRD §8.2).
    let bytes = Pdf::new()
        .prefix(b"junk")
        .obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert!(doc.parse_was_repaired());
}

#[test]
fn open_005_catalog_version_overrides_header() {
    // OPEN-005
    let bytes = Pdf::new()
        .version("1.4")
        .obj(
            1,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Version", name_obj("2.0")),
            ])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert_eq!(doc.version(), Version { major: 2, minor: 0 });
}

#[test]
fn open_006_end_to_end_root_to_catalog() {
    // OPEN-006
    let doc = open(&minimal_doc());
    let root = doc.root().expect("root");
    let cat = doc.resolve(root).unwrap();
    let d = cat.as_dict().unwrap();
    assert_eq!(d.get(&n("Type")).unwrap(), &name_obj("Catalog"));
    // /Pages resolves transparently.
    let pages = doc
        .resolve_dict_key(d, &n("Pages"))
        .unwrap()
        .expect("pages");
    assert_eq!(
        pages.as_dict().unwrap().get(&n("Type")).unwrap(),
        &name_obj("Pages")
    );
}

// --- RESOLVE-* ------------------------------------------------------------

#[test]
fn resolve_001_first_resolve_caches() {
    // RESOLVE-001
    let doc = open(&minimal_doc());
    assert_eq!(doc.cached_object_count(), 0);
    let _ = doc.resolve(doc.root().unwrap()).unwrap();
    assert_eq!(doc.cached_object_count(), 1);
}

#[test]
fn resolve_002_second_resolve_same_arc() {
    // RESOLVE-002
    let doc = open(&minimal_doc());
    let a = doc.get_object(1, 0).unwrap();
    let b = doc.get_object(1, 0).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a, &b));
}

#[test]
fn resolve_003_reference_chain_followed() {
    // RESOLVE-003: 1 -> 2 -> 3 (a real value).
    let bytes = Pdf::new()
        .obj(1, 0, rref(2, 0))
        .obj(2, 0, rref(3, 0))
        .obj(3, 0, Object::Integer(42))
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let v = doc.resolve(pdf_core::ObjRef::new(1, 0)).unwrap();
    assert_eq!(v.as_ref(), &Object::Integer(42));
}

#[test]
fn resolve_004_direct_self_cycle() {
    // RESOLVE-004: object 1 references itself.
    let bytes = Pdf::new().obj(1, 0, rref(1, 0)).root(1, 0).build();
    let doc = open(&bytes);
    let err = doc.resolve(pdf_core::ObjRef::new(1, 0)).unwrap_err();
    assert!(
        matches!(err, Error::ReferenceCycle { num: 1, .. }),
        "{err:?}"
    );
}

#[test]
fn resolve_005_indirect_cycle() {
    // RESOLVE-005: 1 -> 2 -> 1.
    let bytes = Pdf::new()
        .obj(1, 0, rref(2, 0))
        .obj(2, 0, rref(1, 0))
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let err = doc.resolve(pdf_core::ObjRef::new(1, 0)).unwrap_err();
    assert!(matches!(err, Error::ReferenceCycle { .. }), "{err:?}");
}

#[test]
fn resolve_006_depth_limit() {
    // RESOLVE-006: a long ref chain past max_recursion_depth.
    let mut b = Pdf::new();
    let depth = 20u32;
    for i in 1..depth {
        b = b.obj(i, 0, rref(i + 1, 0));
    }
    b = b.obj(depth, 0, Object::Integer(7));
    let bytes = b.root(1, 0).build();
    let limits = Limits::unbounded_decode().with_max_recursion_depth(5);
    let doc = DocumentStore::from_bytes(bytes, limits).unwrap();
    let err = doc.resolve(pdf_core::ObjRef::new(1, 0)).unwrap_err();
    assert!(
        matches!(
            err,
            Error::LimitExceeded(pdf_core::LimitKind::RecursionDepth)
        ),
        "{err:?}"
    );
}

#[test]
fn resolve_007_dangling_reference() {
    // RESOLVE-007: reference to an object with no xref entry. Per PRD §8.2 the
    // *Strict*-mode contract is a typed `Error::MissingObject`; the default
    // Lenient mode yields `Null` (covered by MODE-006 in repair_unit.rs). The
    // fixture carries a valid catalog/page-tree so the Strict validation gate
    // passes, then object 3 (a ref to the absent object 99) is resolved.
    use pdf_core::ParseMode;
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
            ])),
        )
        .obj(3, 0, rref(99, 0))
        .root(1, 0)
        .build();
    let doc = DocumentStore::from_bytes_with(bytes, ParseMode::Strict, Limits::unbounded_decode())
        .expect("open");
    let err = doc.resolve(pdf_core::ObjRef::new(3, 0)).unwrap_err();
    assert!(
        matches!(err, Error::MissingObject { num: 99, .. }),
        "{err:?}"
    );
}

#[test]
fn resolve_008_resolve_dict_key() {
    // RESOLVE-008
    let doc = open(&minimal_doc());
    let cat = doc.resolve(doc.root().unwrap()).unwrap();
    let pages = doc
        .resolve_dict_key(cat.as_dict().unwrap(), &n("Pages"))
        .unwrap()
        .unwrap();
    assert!(pages.as_dict().is_some());
    // Missing key → Ok(None).
    assert!(doc
        .resolve_dict_key(cat.as_dict().unwrap(), &n("Nope"))
        .unwrap()
        .is_none());
}

#[test]
fn resolve_009_root_from_trailer() {
    // RESOLVE-009
    let doc = open(&minimal_doc());
    assert_eq!(doc.root(), Some(pdf_core::ObjRef::new(1, 0)));
}

#[test]
fn resolve_010_get_object_raw_reference() {
    // RESOLVE-010: get_object does NOT follow a reference.
    let bytes = Pdf::new()
        .obj(1, 0, rref(2, 0))
        .obj(2, 0, Object::Integer(5))
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let raw = doc.get_object(1, 0).unwrap();
    assert_eq!(
        raw.as_ref(),
        &Object::Reference(pdf_core::ObjRef::new(2, 0))
    );
}

// --- STREAM-RAW-* ---------------------------------------------------------

#[test]
fn stream_raw_001_body_sliced_from_source() {
    // STREAM-RAW-001: a parsed stream object carries a source-backed Raw body.
    let body = b"hello raw stream body";
    let bytes = Pdf::new()
        .obj(
            1,
            0,
            Object::Stream(pdf_core::StreamObj::new_encoded(
                dict([("Length", Object::Integer(body.len() as i64))]),
                body.to_vec(),
            )),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let obj = doc.get_object(1, 0).unwrap();
    let stream = obj.as_stream().unwrap();
    assert!(matches!(stream.data, StreamData::Raw { .. }));
    let sliced = doc.stream_raw_bytes(stream).unwrap();
    assert_eq!(sliced.as_ref(), body);
}

#[test]
fn stream_raw_002_flate_decodes_from_source() {
    // STREAM-RAW-002: a Flate stream parsed from source decodes to the original.
    let plain = b"The quick brown fox jumps over the lazy dog. 1234567890";
    let bytes = Pdf::new()
        .obj(1, 0, flate_stream([], plain))
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let obj = doc.get_object(1, 0).unwrap();
    let stream = obj.as_stream().unwrap();
    assert!(matches!(stream.data, StreamData::Raw { .. }));
    let decoded = doc.decode_stream(stream).unwrap().into_decoded().unwrap();
    assert_eq!(decoded, plain);
}

#[test]
fn stream_raw_003_raw_bounds_validated() {
    // STREAM-RAW-003: a Raw range past the source end errors, never panics.
    let doc = open(&minimal_doc());
    let bad = pdf_core::StreamObj {
        dict: dict([]),
        data: StreamData::Raw {
            offset: doc.source().len() + 10,
            len: 5,
        },
    };
    let err = doc.stream_raw_bytes(&bad).unwrap_err();
    assert!(matches!(err, Error::Source { .. }), "{err:?}");
}
