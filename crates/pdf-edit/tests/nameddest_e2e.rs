//! `NAMEDDEST-*` — named-destination resolution → physical page (PRD §8.9/§12).

mod common;

use common::{assemble_classic, dict, name_obj, open, rref};

use pdf_core::{ObjRef, Object, PdfString};
use pdf_edit::dest::{page_index_map, resolve_link, resolve_named};

/// A 3-page doc (leaves 4,6,8) plus extra catalog keys supplied by `extra`.
fn doc_with_catalog_keys(
    extra: &[(&'static str, Object)],
    extra_objs: &[(u32, Object)],
) -> Vec<u8> {
    let media = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ])
    };
    let mk_page = || {
        Object::Dictionary(dict([
            ("Type", name_obj("Page")),
            ("Parent", rref(2)),
            ("MediaBox", media()),
        ]))
    };
    let mut catalog_pairs: Vec<(&'static str, Object)> =
        vec![("Type", name_obj("Catalog")), ("Pages", rref(2))];
    catalog_pairs.extend_from_slice(extra);

    let mut objects = vec![
        (1, Object::Dictionary(dict(catalog_pairs))),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(4), rref(6), rref(8)])),
                ("Count", Object::Integer(3)),
            ])),
        ),
        (4, mk_page()),
        (6, mk_page()),
        (8, mk_page()),
    ];
    objects.extend_from_slice(extra_objs);
    objects.sort_by_key(|(num, _)| *num);
    assemble_classic(&objects, ObjRef::new(1, 0))
}

fn dest_array(page: u32) -> Object {
    Object::Array(vec![
        rref(page),
        name_obj("XYZ"),
        Object::Null,
        Object::Null,
        Object::Null,
    ])
}

/// `NAMEDDEST-001`: a name in the catalog `/Dests` dict resolves.
#[test]
fn nameddest_001_catalog_dests() {
    let dests = Object::Dictionary(dict([("intro", dest_array(6))]));
    let doc = open(&doc_with_catalog_keys(
        &[("Dests", rref(20))],
        &[(20, dests)],
    ));
    let pages = page_index_map(&doc);
    assert_eq!(resolve_named(&doc, b"intro", &pages), Some(1));
}

/// `NAMEDDEST-002`: a name in the `/Names /Dests` flat name-tree resolves.
#[test]
fn nameddest_002_names_tree_flat() {
    let names_leaf = Object::Dictionary(dict([(
        "Names",
        Object::Array(vec![
            Object::String(PdfString::literal(b"chapter2".to_vec())),
            dest_array(8),
        ]),
    )]));
    let names = Object::Dictionary(dict([("Dests", rref(21))]));
    let doc = open(&doc_with_catalog_keys(
        &[("Names", rref(20))],
        &[(20, names), (21, names_leaf)],
    ));
    let pages = page_index_map(&doc);
    assert_eq!(resolve_named(&doc, b"chapter2", &pages), Some(2));
}

/// `NAMEDDEST-003`: a multi-level name-tree (`/Kids` + `/Limits`) traverses to a leaf.
#[test]
fn nameddest_003_names_tree_kids() {
    let leaf = Object::Dictionary(dict([
        (
            "Limits",
            Object::Array(vec![
                Object::String(PdfString::literal(b"m".to_vec())),
                Object::String(PdfString::literal(b"z".to_vec())),
            ]),
        ),
        (
            "Names",
            Object::Array(vec![
                Object::String(PdfString::literal(b"target".to_vec())),
                dest_array(6),
            ]),
        ),
    ]));
    let root = Object::Dictionary(dict([("Kids", Object::Array(vec![rref(22)]))]));
    let names = Object::Dictionary(dict([("Dests", rref(21))]));
    let doc = open(&doc_with_catalog_keys(
        &[("Names", rref(20))],
        &[(20, names), (21, root), (22, leaf)],
    ));
    let pages = page_index_map(&doc);
    assert_eq!(resolve_named(&doc, b"target", &pages), Some(1));
}

/// `NAMEDDEST-004`: a named dest still maps to the right **physical** page even
/// when the doc has a non-trivial `/PageLabels` (labels don't affect physical
/// page resolution).
#[test]
fn nameddest_004_under_pagelabels() {
    let dests = Object::Dictionary(dict([("end", dest_array(8))]));
    let labels = Object::Dictionary(dict([(
        "Nums",
        Object::Array(vec![
            Object::Integer(0),
            Object::Dictionary(dict([("S", name_obj("r"))])),
            Object::Integer(1),
            Object::Dictionary(dict([("S", name_obj("D"))])),
        ]),
    )]));
    let doc = open(&doc_with_catalog_keys(
        &[("Dests", rref(20)), ("PageLabels", rref(23))],
        &[(20, dests), (23, labels)],
    ));
    let pages = page_index_map(&doc);
    // Physical page index is 2 regardless of the label scheme.
    assert_eq!(resolve_named(&doc, b"end", &pages), Some(2));
    // Sanity: the label at that page is the decimal range, not roman.
    assert_eq!(pdf_edit::get_label(&doc, 2), "2");
}

/// `NAMEDDEST-005`: an unknown name → None.
#[test]
fn nameddest_005_unknown() {
    let dests = Object::Dictionary(dict([("intro", dest_array(6))]));
    let doc = open(&doc_with_catalog_keys(
        &[("Dests", rref(20))],
        &[(20, dests)],
    ));
    let pages = page_index_map(&doc);
    assert_eq!(resolve_named(&doc, b"nope", &pages), None);
}

/// `NAMEDDEST-006`: `resolve_link` on a `/GoTo` action with a **named** `/D`.
#[test]
fn nameddest_006_resolve_link_named() {
    let dests = Object::Dictionary(dict([("spot", dest_array(8))]));
    let doc = open(&doc_with_catalog_keys(
        &[("Dests", rref(20))],
        &[(20, dests)],
    ));
    let pages = page_index_map(&doc);
    // A link dict with /A /GoTo /D (spot).
    let link = dict([(
        "A",
        Object::Dictionary(dict([
            ("S", name_obj("GoTo")),
            ("D", Object::String(PdfString::literal(b"spot".to_vec()))),
        ])),
    )]);
    assert_eq!(resolve_link(&doc, &link, &pages), Some(2));
}
