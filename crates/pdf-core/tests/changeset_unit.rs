//! M3a object-edit API tests — `EDIT-*` (PRD §8.7 / §9.2).
//!
//! The primary oracle is our own reparse: open → edit → save → reopen → assert.

mod common;

use common::{dict, name_obj, simple_doc, SIMPLE_CONTENT};

use pdf_core::changeset::Change;
use pdf_core::object::{Name, Object, StreamObj};
use pdf_core::{DocumentStore, Limits, SaveOptions};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("opens")
}

fn reopen(doc: &DocumentStore, opts: &SaveOptions) -> DocumentStore {
    let bytes = doc.save_to_vec(opts).expect("save");
    open(&bytes)
}

/// `EDIT-001`: `add_object` allocates a fresh number past the current max and
/// flips `is_dirty`.
#[test]
fn edit_001_add_object_allocates_fresh_number() {
    let doc = open(&simple_doc());
    assert!(!doc.is_dirty(), "freshly opened doc is not dirty");
    let before_size = doc.xref_length();

    let r = doc.add_object(Object::Integer(42)).unwrap();
    assert!(
        r.num >= before_size,
        "new object number {} must be past current /Size {}",
        r.num,
        before_size
    );
    assert_eq!(r.gen, 0);
    assert!(doc.is_dirty(), "after add_object the doc is dirty");
}

/// `EDIT-002`: `add_object` then `resolve` returns the new value (no save).
#[test]
fn edit_002_add_object_visible_via_resolve() {
    let doc = open(&simple_doc());
    let r = doc.add_object(Object::Integer(7)).unwrap();
    let got = doc.resolve(r).unwrap();
    assert_eq!(*got, Object::Integer(7));
}

/// `EDIT-003`: `add_object` then save → reopen → object present + equal.
#[test]
fn edit_003_add_object_survives_save_reopen() {
    let doc = open(&simple_doc());
    let r = doc
        .add_object(Object::String(pdf_core::PdfString::literal(
            b"new".to_vec(),
        )))
        .unwrap();

    let re = reopen(&doc, &SaveOptions::default());
    let got = re.get_object(r.num, 0).unwrap();
    assert_eq!(
        *got,
        Object::String(pdf_core::PdfString::literal(b"new".to_vec()))
    );
}

/// `EDIT-004`: `update_object` reflected by an immediate `resolve`.
#[test]
fn edit_004_update_object_visible_immediately() {
    let doc = open(&simple_doc());
    // Replace the font object (5 0) with a different dictionary.
    let new_font = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Courier")),
    ]));
    doc.update_object(pdf_core::ObjRef::new(5, 0), new_font.clone())
        .unwrap();

    let got = doc.resolve(pdf_core::ObjRef::new(5, 0)).unwrap();
    assert_eq!(*got, new_font);
}

/// `EDIT-005`: `update_object` reflected after save → reopen.
#[test]
fn edit_005_update_object_survives_save_reopen() {
    let doc = open(&simple_doc());
    let new_font = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Courier")),
    ]));
    doc.update_object(pdf_core::ObjRef::new(5, 0), new_font.clone())
        .unwrap();

    let re = reopen(&doc, &SaveOptions::default());
    let got = re.get_object(5, 0).unwrap();
    assert_eq!(
        got.as_dict().unwrap().get(&Name::new("BaseFont")),
        Some(&name_obj("Courier"))
    );
}

/// `EDIT-006`: `update_stream` (deflate off) body round-trips after save→reopen.
#[test]
fn edit_006_update_stream_roundtrips_no_deflate() {
    let doc = open(&simple_doc());
    let new_body = b"BT /F1 24 Tf 100 700 Td (Edited) Tj ET".to_vec();
    doc.update_stream(
        pdf_core::ObjRef::new(4, 0),
        dict([]),
        new_body.clone(),
        false,
    )
    .unwrap();

    let re = reopen(&doc, &SaveOptions::default().with_deflate(false));
    let decoded = re.xref_stream(4).unwrap();
    assert_eq!(decoded, new_body);
}

/// `EDIT-007`: `update_stream` (deflate on) body decodes to original after
/// reopen.
#[test]
fn edit_007_update_stream_roundtrips_with_deflate() {
    let doc = open(&simple_doc());
    let new_body = b"BT /F1 24 Tf 100 700 Td (Deflated body here) Tj ET".to_vec();
    doc.update_stream(
        pdf_core::ObjRef::new(4, 0),
        dict([]),
        new_body.clone(),
        false,
    )
    .unwrap();

    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_deflate(true))
        .unwrap();
    let re = open(&bytes);
    // The saved stream must be FlateDecode-filtered and decode to the original.
    assert!(re.xref_is_stream(4).unwrap());
    let decoded = re.xref_stream(4).unwrap();
    assert_eq!(decoded, new_body);
    // And it must actually be compressed (filter present in the saved dict).
    let raw = re.xref_stream_raw(4).unwrap();
    assert_ne!(raw, new_body, "deflate=true must encode the body");
}

/// `EDIT-008`: `delete_object` → `resolve` yields Null; gone after save→reopen.
#[test]
fn edit_008_delete_object() {
    let doc = open(&simple_doc());
    // Delete the font object (not referenced from the page tree validation path).
    doc.delete_object(pdf_core::ObjRef::new(5, 0)).unwrap();
    assert_eq!(
        *doc.resolve(pdf_core::ObjRef::new(5, 0)).unwrap(),
        Object::Null
    );

    let re = reopen(&doc, &SaveOptions::default());
    // After save the object is free; resolving it yields Null (lenient).
    assert_eq!(
        *re.resolve(pdf_core::ObjRef::new(5, 0)).unwrap(),
        Object::Null
    );
}

/// `EDIT-009`: an unmodified doc is not dirty and has no changes.
#[test]
fn edit_009_unmodified_is_clean() {
    let doc = open(&simple_doc());
    assert!(!doc.is_dirty());
    assert!(doc.changes_snapshot().is_empty());
}

/// `EDIT-010`: `update_object` on a never-resolved original number overlays
/// correctly (the overlay does not require a prior resolve).
#[test]
fn edit_010_update_without_prior_resolve() {
    let doc = open(&simple_doc());
    // Never resolve object 5 first; update it directly.
    let replacement = Object::Integer(999);
    doc.update_object(pdf_core::ObjRef::new(5, 0), replacement.clone())
        .unwrap();
    assert_eq!(*doc.get_object(5, 0).unwrap(), replacement);
}

/// `EDIT-011`: add/update/delete are reflected in `changes()` (the M3b basis).
#[test]
fn edit_011_changes_list_reflects_edits() {
    let doc = open(&simple_doc());
    let added = doc.add_object(Object::Integer(1)).unwrap();
    doc.update_object(pdf_core::ObjRef::new(5, 0), Object::Integer(2))
        .unwrap();
    doc.delete_object(pdf_core::ObjRef::new(4, 0)).unwrap();

    let changes = doc.changes_snapshot();
    let map: std::collections::BTreeMap<u32, Change> = changes.into_iter().collect();
    assert!(matches!(map.get(&added.num), Some(Change::Set(_))));
    assert!(matches!(map.get(&5), Some(Change::Set(_))));
    assert!(matches!(map.get(&4), Some(Change::Deleted)));
}

/// Sanity: the unedited simple-doc content stream is what we expect (guards the
/// fixture against drift before the edit tests rely on it).
#[test]
fn edit_fixture_content_baseline() {
    let doc = open(&simple_doc());
    let body = doc.xref_stream(4).unwrap();
    assert_eq!(body, SIMPLE_CONTENT);
    // A newly created stream object is also serializable.
    let r = doc
        .add_object(Object::Stream(StreamObj::new_encoded(
            dict([]),
            b"hi".to_vec(),
        )))
        .unwrap();
    assert!(doc.get_object(r.num, 0).unwrap().as_stream().is_some());
}
