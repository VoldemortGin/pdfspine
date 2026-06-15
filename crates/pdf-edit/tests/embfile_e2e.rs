//! M4e — Embedded files (`pdf-edit`) end-to-end tests (PRD §8.8).
//!
//! Catalog IDs: `EMBFILE-*`. Self-built fixtures only (PRD §10). The core oracle
//! is a **byte-exact** add → get round trip; the persistence test rebuilds the
//! document via the save/reopen reparse oracle.

mod common;

use common::{assemble_classic, blank_page, dict, name_obj, open, rref, save_reopen};

use pdf_core::object::Name;
use pdf_core::{Dict, DocumentStore, Object, PdfString, StreamObj, StringKind};
use pdf_edit::{embfile_add, embfile_count, embfile_del, embfile_get, embfile_info, embfile_names};

/// A fresh single-page document with no `/Names` (the common starting point).
fn fresh_doc() -> DocumentStore {
    open(&blank_page(612, 792))
}

// === EMBFILE-ADD-GET-001 — byte-exact round trip ==========================

#[test]
fn embfile_add_get_001_byte_exact_round_trip() {
    let doc = fresh_doc();
    let payload: Vec<u8> = (0u8..=255).cycle().take(5000).collect();
    embfile_add(&doc, "blob.bin", &payload, None, None, None).unwrap();

    let got = embfile_get(&doc, "blob.bin").unwrap();
    assert_eq!(got, payload, "round-trip must be byte-exact");
}

// === EMBFILE-ADD-002 — filename/ufilename/desc stored + readable ==========

#[test]
fn embfile_add_002_metadata_stored_and_readable() {
    let doc = fresh_doc();
    embfile_add(
        &doc,
        "key1",
        b"hello",
        Some("report.txt"),
        Some("report-u.txt"),
        Some("a description"),
    )
    .unwrap();

    let info = embfile_info(&doc, "key1").unwrap();
    assert_eq!(info.name, "key1");
    assert_eq!(info.filename, "report.txt");
    assert_eq!(info.ufilename, "report-u.txt");
    assert_eq!(info.desc, "a description");
}

// === EMBFILE-NAMES-001 — names() sorted; count() matches ==================

#[test]
fn embfile_names_001_sorted_and_counted() {
    let doc = fresh_doc();
    embfile_add(&doc, "charlie", b"c", None, None, None).unwrap();
    embfile_add(&doc, "alpha", b"a", None, None, None).unwrap();
    embfile_add(&doc, "bravo", b"b", None, None, None).unwrap();

    assert_eq!(embfile_names(&doc), vec!["alpha", "bravo", "charlie"]);
    assert_eq!(embfile_count(&doc), 3);
}

// === EMBFILE-MULTI-001 — 3+ files each round-trip; names sorted ===========

#[test]
fn embfile_multi_001_multiple_files_round_trip() {
    let doc = fresh_doc();
    let files: &[(&str, &[u8])] = &[
        ("zeta.dat", b"ZZZ-payload-bytes"),
        ("alpha.dat", &[0u8, 1, 2, 3, 4, 5, 255, 254]),
        ("mike.dat", b""), // empty payload is valid
        ("delta.dat", b"\x00\x01\x02 mixed \xff\xfe text"),
    ];
    for (name, bytes) in files {
        embfile_add(&doc, name, bytes, None, None, None).unwrap();
    }

    assert_eq!(embfile_count(&doc), 4);
    assert_eq!(
        embfile_names(&doc),
        vec!["alpha.dat", "delta.dat", "mike.dat", "zeta.dat"]
    );
    for (name, bytes) in files {
        assert_eq!(
            &embfile_get(&doc, name).unwrap(),
            bytes,
            "round-trip {name}"
        );
    }
}

// === EMBFILE-DEL-001 — del removes key; get errors; survivors intact ======

#[test]
fn embfile_del_001_delete_removes_only_target() {
    let doc = fresh_doc();
    embfile_add(&doc, "keep1", b"K1", None, None, None).unwrap();
    embfile_add(&doc, "drop", b"DROP", None, None, None).unwrap();
    embfile_add(&doc, "keep2", b"K2", None, None, None).unwrap();
    assert_eq!(embfile_count(&doc), 3);

    embfile_del(&doc, "drop").unwrap();

    assert_eq!(embfile_count(&doc), 2);
    assert!(embfile_get(&doc, "drop").is_err(), "deleted key must error");
    assert_eq!(embfile_names(&doc), vec!["keep1", "keep2"]);
    // Survivors still byte-exact.
    assert_eq!(embfile_get(&doc, "keep1").unwrap(), b"K1");
    assert_eq!(embfile_get(&doc, "keep2").unwrap(), b"K2");
}

// === EMBFILE-INFO-001 — info reports filename/ufilename/desc/size =========

#[test]
fn embfile_info_001_reports_fields_and_size() {
    let doc = fresh_doc();
    let payload = b"twelve bytes";
    embfile_add(
        &doc,
        "doc",
        payload,
        Some("f.txt"),
        Some("uf.txt"),
        Some("desc here"),
    )
    .unwrap();

    let info = embfile_info(&doc, "doc").unwrap();
    assert_eq!(info.filename, "f.txt");
    assert_eq!(info.ufilename, "uf.txt");
    assert_eq!(info.desc, "desc here");
    assert_eq!(info.size, payload.len());
    assert_eq!(info.length, info.size, "length aliases size");
}

#[test]
fn embfile_info_defaults_filename_to_name() {
    let doc = fresh_doc();
    embfile_add(&doc, "myname", b"x", None, None, None).unwrap();
    let info = embfile_info(&doc, "myname").unwrap();
    // filename defaults to name; ufilename defaults to filename.
    assert_eq!(info.filename, "myname");
    assert_eq!(info.ufilename, "myname");
    assert_eq!(info.desc, "");
}

// === EMBFILE-PERSIST-001 — add → save → reopen → get byte-exact ===========

#[test]
fn embfile_persist_001_survives_save_reopen() {
    let doc = fresh_doc();
    let payload: Vec<u8> = (0u8..200).rev().cycle().take(3333).collect();
    embfile_add(
        &doc,
        "persist.bin",
        &payload,
        Some("p.bin"),
        None,
        Some("kept"),
    )
    .unwrap();
    embfile_add(&doc, "second.bin", b"second", None, None, None).unwrap();

    let reopened = save_reopen(&doc);

    assert_eq!(embfile_count(&reopened), 2);
    assert_eq!(embfile_names(&reopened), vec!["persist.bin", "second.bin"]);
    assert_eq!(embfile_get(&reopened, "persist.bin").unwrap(), payload);
    assert_eq!(embfile_get(&reopened, "second.bin").unwrap(), b"second");

    let info = embfile_info(&reopened, "persist.bin").unwrap();
    assert_eq!(info.filename, "p.bin");
    assert_eq!(info.desc, "kept");
    assert_eq!(info.size, payload.len());
}

#[test]
fn embfile_persist_del_then_save_reopen() {
    let doc = fresh_doc();
    embfile_add(&doc, "a", b"AAA", None, None, None).unwrap();
    embfile_add(&doc, "b", b"BBB", None, None, None).unwrap();
    embfile_del(&doc, "a").unwrap();

    let reopened = save_reopen(&doc);
    assert_eq!(embfile_names(&reopened), vec!["b"]);
    assert!(embfile_get(&reopened, "a").is_err());
    assert_eq!(embfile_get(&reopened, "b").unwrap(), b"BBB");
}

// === EMBFILE-PROP-001 — typed errors, never panics; empty when no /Names ===

#[test]
fn embfile_prop_001_missing_name_and_empty_doc() {
    let doc = fresh_doc();

    // No /Names yet: enumeration is empty, count is zero, no panic.
    assert!(embfile_names(&doc).is_empty());
    assert_eq!(embfile_count(&doc), 0);

    // get/del/info on a non-existent name → typed InvalidArgument, never panic.
    let e = embfile_get(&doc, "nope").unwrap_err();
    assert_eq!(e.kind(), "invalid-argument");
    let e = embfile_del(&doc, "nope").unwrap_err();
    assert_eq!(e.kind(), "invalid-argument");
    let e = embfile_info(&doc, "nope").unwrap_err();
    assert_eq!(e.kind(), "invalid-argument");

    // After adding one, get on a *different* missing name still errors typed.
    embfile_add(&doc, "exists", b"x", None, None, None).unwrap();
    let e = embfile_get(&doc, "missing").unwrap_err();
    assert_eq!(e.kind(), "invalid-argument");

    // Adding a duplicate key → typed error; original intact.
    let e = embfile_add(&doc, "exists", b"y", None, None, None).unwrap_err();
    assert_eq!(e.kind(), "invalid-argument");
    assert_eq!(embfile_get(&doc, "exists").unwrap(), b"x");
    assert_eq!(embfile_count(&doc), 1);
}

// === EMBFILE-EXISTING-TREE-001 — read a pre-built multi-pair tree ==========

/// A document whose catalog already carries a `/Names /EmbeddedFiles` name-tree
/// with a `/Kids` branch holding two leaves (keys `a`, `b` and `c`, `d`), each
/// value an indirect `/Filespec` → `/EmbeddedFile` stream. Exercises the general
/// (multi-level) read walker.
fn prebuilt_tree_doc() -> Vec<u8> {
    // Helper: build one (filespec-obj, ef-stream-obj) pair for `body`.
    let filespec = |ef_num: u32, fname: &str| -> Object {
        let mut ef = Dict::new();
        ef.insert(Name::new("F"), rref(ef_num));
        Object::Dictionary(dict([
            ("Type", name_obj("Filespec")),
            (
                "F",
                Object::String(PdfString {
                    bytes: fname.as_bytes().to_vec(),
                    kind: StringKind::Literal,
                }),
            ),
            (
                "UF",
                Object::String(PdfString {
                    bytes: fname.as_bytes().to_vec(),
                    kind: StringKind::Literal,
                }),
            ),
            ("EF", Object::Dictionary(ef)),
        ]))
    };
    let ef_stream = |body: &[u8]| -> Object {
        let mut d = dict([
            ("Type", name_obj("EmbeddedFile")),
            ("Length", Object::Integer(body.len() as i64)),
        ]);
        let mut params = Dict::new();
        params.insert(Name::new("Size"), Object::Integer(body.len() as i64));
        d.insert(Name::new("Params"), Object::Dictionary(params));
        Object::Stream(StreamObj::new_encoded(d, body.to_vec()))
    };
    let stro = |s: &str| {
        Object::String(PdfString {
            bytes: s.as_bytes().to_vec(),
            kind: StringKind::Literal,
        })
    };

    // Object layout:
    // 1 catalog (/Names → 10) | 2 pages | 3 page | 4 content
    // 10 Names dict (/EmbeddedFiles → 11)
    // 11 root branch (/Kids [12 13])
    // 12 leaf (/Names [(a) 20 (b) 21], /Limits [(a)(b)])
    // 13 leaf (/Names [(c) 22 (d) 23], /Limits [(c)(d)])
    // 20..23 filespecs ; 30..33 ef streams
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2)),
                ("Names", rref(10)),
            ])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                (
                    "MediaBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Integer(612),
                        Object::Integer(792),
                    ]),
                ),
                ("Contents", rref(4)),
                ("Resources", Object::Dictionary(Dict::new())),
            ])),
        ),
        (
            4,
            Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(0))]),
                Vec::new(),
            )),
        ),
        (10, Object::Dictionary(dict([("EmbeddedFiles", rref(11))]))),
        (
            11,
            Object::Dictionary(dict([("Kids", Object::Array(vec![rref(12), rref(13)]))])),
        ),
        (
            12,
            Object::Dictionary(dict([
                (
                    "Names",
                    Object::Array(vec![stro("a"), rref(20), stro("b"), rref(21)]),
                ),
                ("Limits", Object::Array(vec![stro("a"), stro("b")])),
            ])),
        ),
        (
            13,
            Object::Dictionary(dict([
                (
                    "Names",
                    Object::Array(vec![stro("c"), rref(22), stro("d"), rref(23)]),
                ),
                ("Limits", Object::Array(vec![stro("c"), stro("d")])),
            ])),
        ),
        (20, filespec(30, "a.txt")),
        (21, filespec(31, "b.txt")),
        (22, filespec(32, "c.txt")),
        (23, filespec(33, "d.txt")),
        (30, ef_stream(b"AAA")),
        (31, ef_stream(b"BBB")),
        (32, ef_stream(b"CCC")),
        (33, ef_stream(b"DDD")),
    ];
    assemble_classic(&objects, pdf_core::ObjRef::new(1, 0))
}

#[test]
fn embfile_existing_tree_001_enumerates_all_keys() {
    let doc = open(&prebuilt_tree_doc());

    assert_eq!(embfile_count(&doc), 4);
    assert_eq!(embfile_names(&doc), vec!["a", "b", "c", "d"]);

    // Each key resolves to its byte-exact payload and metadata.
    assert_eq!(embfile_get(&doc, "a").unwrap(), b"AAA");
    assert_eq!(embfile_get(&doc, "d").unwrap(), b"DDD");
    assert_eq!(embfile_info(&doc, "b").unwrap().filename, "b.txt");
    assert_eq!(embfile_info(&doc, "c").unwrap().size, 3);
}

#[test]
fn embfile_existing_tree_add_then_collapse_keeps_all() {
    // Adding to a pre-built multi-level tree collapses to a flat leaf but must
    // keep every prior key plus the new one, all readable and sorted.
    let doc = open(&prebuilt_tree_doc());
    embfile_add(&doc, "e", b"EEE", None, None, None).unwrap();

    assert_eq!(embfile_count(&doc), 5);
    assert_eq!(embfile_names(&doc), vec!["a", "b", "c", "d", "e"]);
    assert_eq!(embfile_get(&doc, "a").unwrap(), b"AAA");
    assert_eq!(embfile_get(&doc, "e").unwrap(), b"EEE");

    // Survives save/reopen too.
    let reopened = save_reopen(&doc);
    assert_eq!(embfile_names(&reopened), vec!["a", "b", "c", "d", "e"]);
    assert_eq!(embfile_get(&reopened, "c").unwrap(), b"CCC");
}
