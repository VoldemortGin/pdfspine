//! `PAGE-*` / `PAGETREE-FALLBACK-*` — page-tree traversal, inherited attributes,
//! box/rotation resolution and the broken-tree object-scan fallback (PRD §7,
//! §8.2 step 3, §8.6.1, §9.2). Self-built fixtures (M1a serializer + classic
//! xref via `common::Pdf`).

mod common;

use common::{dict, name_obj, rref, Pdf};

use pdf_core::object::{ObjRef, Object};
use pdf_core::pagetree;
use pdf_core::{DocumentStore, Limits};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::unbounded_decode()).expect("open")
}

fn int_array(vals: &[i64]) -> Object {
    Object::Array(vals.iter().copied().map(Object::Integer).collect())
}

/// A single-page document: catalog(1) → pages(2) → page(3), with a media box on
/// the page leaf.
fn single_page_doc() -> Vec<u8> {
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("MediaBox", int_array(&[0, 0, 200, 300])),
            ])),
        )
        .root(1, 0)
        .build()
}

// --- PAGE-COUNT-* / order -------------------------------------------------

#[test]
fn page_count_001_nested_tree() {
    // PAGE-COUNT-001: catalog → pages(2) → [pages(3) → [4,5], page(6)].
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
                ("Count", Object::Integer(3)),
                ("Kids", Object::Array(vec![rref(3, 0), rref(6, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Parent", rref(2, 0)),
                ("Count", Object::Integer(2)),
                ("Kids", Object::Array(vec![rref(4, 0), rref(5, 0)])),
            ])),
        )
        .obj(
            4,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(3, 0))])),
        )
        .obj(
            5,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(3, 0))])),
        )
        .obj(
            6,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(2, 0))])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert_eq!(pagetree::page_count(&doc), 3);
}

#[test]
fn page_count_002_document_order() {
    // PAGE-COUNT-002: leaves in left-to-right, depth-first document order.
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
                ("Count", Object::Integer(3)),
                ("Kids", Object::Array(vec![rref(3, 0), rref(6, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Parent", rref(2, 0)),
                ("Count", Object::Integer(2)),
                ("Kids", Object::Array(vec![rref(4, 0), rref(5, 0)])),
            ])),
        )
        .obj(
            4,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(3, 0))])),
        )
        .obj(
            5,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(3, 0))])),
        )
        .obj(
            6,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(2, 0))])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let refs = pagetree::page_refs(&doc);
    assert_eq!(
        refs,
        vec![ObjRef::new(4, 0), ObjRef::new(5, 0), ObjRef::new(6, 0)]
    );
}

// --- PAGE-INHERIT-* -------------------------------------------------------

#[test]
fn page_inherit_001_mediabox_from_ancestor() {
    // PAGE-INHERIT-001: page omits /MediaBox; inherits from /Pages root.
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("MediaBox", int_array(&[0, 0, 400, 500])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(2, 0))])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let mb = pagetree::mediabox(&doc, ObjRef::new(3, 0));
    assert_eq!((mb.x0, mb.y0, mb.x1, mb.y1), (0.0, 0.0, 400.0, 500.0));
}

#[test]
fn page_inherit_002_rotate_inherited_and_overridden() {
    // PAGE-INHERIT-002: /Rotate inherited from ancestor; a leaf's own value wins.
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
                ("Count", Object::Integer(2)),
                ("Kids", Object::Array(vec![rref(3, 0), rref(4, 0)])),
                ("Rotate", Object::Integer(90)),
            ])),
        )
        // Leaf 3 inherits 90.
        .obj(
            3,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(2, 0))])),
        )
        // Leaf 4 overrides with 180.
        .obj(
            4,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("Rotate", Object::Integer(180)),
            ])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert_eq!(pagetree::rotation(&doc, ObjRef::new(3, 0)), 90);
    assert_eq!(pagetree::rotation(&doc, ObjRef::new(4, 0)), 180);
}

#[test]
fn page_inherit_003_leaf_overrides_box() {
    // PAGE-INHERIT-003: leaf /MediaBox overrides the inherited ancestor box.
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("MediaBox", int_array(&[0, 0, 400, 500])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("MediaBox", int_array(&[0, 0, 100, 100])),
            ])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let mb = pagetree::mediabox(&doc, ObjRef::new(3, 0));
    assert_eq!((mb.x0, mb.y0, mb.x1, mb.y1), (0.0, 0.0, 100.0, 100.0));
}

// --- PAGE-BOX-* -----------------------------------------------------------

#[test]
fn page_box_001_rect_is_crop_intersect_media() {
    // PAGE-BOX-001: rect/bound == CropBox ∩ MediaBox.
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("MediaBox", int_array(&[0, 0, 200, 200])),
                // CropBox extends past the media box on the right; clipped to it.
                ("CropBox", int_array(&[10, 10, 500, 150])),
            ])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let r = pagetree::bound(&doc, ObjRef::new(3, 0));
    assert_eq!((r.x0, r.y0, r.x1, r.y1), (10.0, 10.0, 200.0, 150.0));
}

#[test]
fn page_box_002_default_letter() {
    // PAGE-BOX-002: no /MediaBox anywhere → US Letter default.
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(2, 0))])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let mb = pagetree::mediabox(&doc, ObjRef::new(3, 0));
    assert_eq!((mb.x0, mb.y0, mb.x1, mb.y1), (0.0, 0.0, 612.0, 792.0));
}

#[test]
fn page_box_003_cropbox_defaults_to_media() {
    // PAGE-BOX-003: absent /CropBox → equals /MediaBox.
    let doc = open(&single_page_doc());
    let mb = pagetree::mediabox(&doc, ObjRef::new(3, 0));
    let cb = pagetree::cropbox(&doc, ObjRef::new(3, 0));
    assert_eq!(mb, cb);
    assert_eq!((cb.x0, cb.y0, cb.x1, cb.y1), (0.0, 0.0, 200.0, 300.0));
}

// --- PAGE-ROT-* -----------------------------------------------------------

#[test]
fn page_rot_001_normalize() {
    // PAGE-ROT-001: normalization of out-of-range and non-multiple-of-90 values.
    assert_eq!(pagetree::normalize_rotation(0), 0);
    assert_eq!(pagetree::normalize_rotation(90), 90);
    assert_eq!(pagetree::normalize_rotation(-90), 270);
    assert_eq!(pagetree::normalize_rotation(450), 90);
    assert_eq!(pagetree::normalize_rotation(-360), 0);
    assert_eq!(pagetree::normalize_rotation(720), 0);
    // Non-multiple of 90 → 0 (PyMuPDF tolerance).
    assert_eq!(pagetree::normalize_rotation(45), 0);
    assert_eq!(pagetree::normalize_rotation(91), 0);
}

// --- PAGE-LIMITS-* --------------------------------------------------------

#[test]
fn page_limits_001_kids_cycle_broken() {
    // PAGE-LIMITS-001: a /Kids self-cycle must not hang; bounded traversal.
    // pages(2) lists page(3) and itself(2) → the cycle on 2 is skipped.
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0), rref(2, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([("Type", name_obj("Page")), ("Parent", rref(2, 0))])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    // Terminates and counts the single real leaf exactly once.
    assert_eq!(pagetree::page_count(&doc), 1);
}

// --- PAGETREE-FALLBACK-* --------------------------------------------------

#[test]
fn pagetree_fallback_001_scan_recovers_pages() {
    // PAGETREE-FALLBACK-001: catalog /Pages points at a non-existent object, so
    // the tree is unreachable. The scan fallback finds the /Type /Page objects.
    let bytes = Pdf::new()
        .obj(
            1,
            0,
            // /Pages → object 99, which does not exist.
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(99, 0)),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("MediaBox", int_array(&[0, 0, 10, 10])),
            ])),
        )
        .obj(
            4,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("MediaBox", int_array(&[0, 0, 20, 20])),
            ])),
        )
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let refs = pagetree::page_refs(&doc);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs, vec![ObjRef::new(3, 0), ObjRef::new(4, 0)]);
}

#[test]
fn pagetree_fallback_002_scan_object_number_order() {
    // PAGETREE-FALLBACK-002: recovered pages are in ascending object-number
    // order regardless of physical layout.
    let bytes = Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(99, 0)),
            ])),
        )
        // Emit higher object number first physically, lower second.
        .obj(7, 0, Object::Dictionary(dict([("Type", name_obj("Page"))])))
        .obj(5, 0, Object::Dictionary(dict([("Type", name_obj("Page"))])))
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let refs = pagetree::page_refs(&doc);
    assert_eq!(refs, vec![ObjRef::new(5, 0), ObjRef::new(7, 0)]);
}

// --- low-level xref read API on DocumentStore -----------------------------

#[test]
fn doc_store_xref_length_and_object() {
    let doc = open(&single_page_doc());
    // /Size = max obj num (3) + 1 = 4.
    assert_eq!(doc.xref_length(), 4);
    let src = doc.xref_object(3).unwrap();
    assert!(src.contains("/Page"));
    assert!(src.contains("/MediaBox"));
    // A free / absent object is "null".
    assert_eq!(doc.xref_object(99).unwrap(), "null");
}

#[test]
fn doc_store_xref_get_key_and_stream() {
    // A document with a content stream so xref_is_stream / xref_stream apply.
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
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
            ])),
        )
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("Contents", rref(4, 0)),
            ])),
        )
        .obj(4, 0, common::flate_stream([], b"BT (hi) Tj ET"))
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert_eq!(
        doc.xref_get_key(3, "Type").unwrap().as_deref(),
        Some("/Page")
    );
    assert!(!doc.xref_is_stream(3).unwrap());
    assert!(doc.xref_is_stream(4).unwrap());
    assert_eq!(doc.xref_stream(4).unwrap(), b"BT (hi) Tj ET");
}
