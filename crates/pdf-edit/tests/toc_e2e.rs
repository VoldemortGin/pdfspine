//! `TOC-*` — outline read (`get_toc`) + build (`set_toc`) + level-jump (PRD §8.9/§12).

mod common;

use common::{assemble_classic, dict, name_obj, open, rref, save_reopen, MultiPage};

use pdf_core::object::Name;
use pdf_core::{ObjRef, Object};
use pdf_edit::toc::{get_outline, get_toc, set_toc, TocEntry};

fn entry(level: i32, title: &str, page: i32) -> TocEntry {
    TocEntry {
        level,
        title: title.to_string(),
        page,
    }
}

/// Builds a 3-page doc with a hand-written `/Outlines` tree:
/// - "Chapter 1" (level 1, page 0)
///   - "Section 1.1" (level 2, page 1)
/// - "Chapter 2" (level 1, page 2)
fn doc_with_outlines() -> Vec<u8> {
    // Pages built by MultiPage have leaves at 4,6,8. We reuse those object numbers
    // by appending the outline objects (20+) to a parsed-then-rebuilt layout.
    // Simpler: build objects directly here mirroring MultiPage's 3-page layout.
    let media = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ])
    };
    let font = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
    ]));
    let mk_page = |content: u32| {
        Object::Dictionary(dict([
            ("Type", name_obj("Page")),
            ("Parent", rref(2)),
            ("MediaBox", media()),
            ("Contents", rref(content)),
            (
                "Resources",
                Object::Dictionary(dict([(
                    "Font",
                    Object::Dictionary(dict([("F1", rref(3))])),
                )])),
            ),
        ]))
    };
    let content = |s: &str| {
        let body = format!("BT /F1 12 Tf 72 700 Td ({s}) Tj ET").into_bytes();
        Object::Stream(pdf_core::StreamObj::new_encoded(
            dict([("Length", Object::Integer(body.len() as i64))]),
            body,
        ))
    };
    let dest = |page: u32| {
        Object::Array(vec![
            rref(page),
            name_obj("XYZ"),
            Object::Null,
            Object::Null,
            Object::Null,
        ])
    };

    let objects = vec![
        (
            1,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2)),
                ("Outlines", rref(10)),
            ])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(4), rref(6), rref(8)])),
                ("Count", Object::Integer(3)),
            ])),
        ),
        (3, font),
        (4, mk_page(5)),
        (5, content("AAA")),
        (6, mk_page(7)),
        (7, content("BBB")),
        (8, mk_page(9)),
        (9, content("CCC")),
        // Outline root.
        (
            10,
            Object::Dictionary(dict([
                ("Type", name_obj("Outlines")),
                ("First", rref(11)),
                ("Last", rref(13)),
                ("Count", Object::Integer(3)),
            ])),
        ),
        // Chapter 1 (page 0) → child Section 1.1.
        (
            11,
            Object::Dictionary(dict([
                (
                    "Title",
                    Object::String(pdf_core::PdfString::literal(b"Chapter 1".to_vec())),
                ),
                ("Parent", rref(10)),
                ("Next", rref(13)),
                ("First", rref(12)),
                ("Last", rref(12)),
                ("Count", Object::Integer(1)),
                ("Dest", dest(4)),
            ])),
        ),
        // Section 1.1 (page 1) — uses /A /GoTo instead of /Dest.
        (
            12,
            Object::Dictionary(dict([
                (
                    "Title",
                    Object::String(pdf_core::PdfString::literal(b"Section 1.1".to_vec())),
                ),
                ("Parent", rref(11)),
                (
                    "A",
                    Object::Dictionary(dict([("S", name_obj("GoTo")), ("D", dest(6))])),
                ),
            ])),
        ),
        // Chapter 2 (page 2).
        (
            13,
            Object::Dictionary(dict([
                (
                    "Title",
                    Object::String(pdf_core::PdfString::literal(b"Chapter 2".to_vec())),
                ),
                ("Parent", rref(10)),
                ("Prev", rref(11)),
                ("Dest", dest(8)),
            ])),
        ),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// `TOC-GET-001` / `TOC-GET-002` / `TOC-GET-003`: read a hand-built tree.
#[test]
fn toc_get_001_levels_and_pages() {
    let doc = open(&doc_with_outlines());
    let toc = get_toc(&doc);
    assert_eq!(
        toc,
        vec![
            entry(1, "Chapter 1", 0),
            entry(2, "Section 1.1", 1), // via /A /GoTo
            entry(1, "Chapter 2", 2),
        ]
    );
}

/// `TOC-GET-004`: no `/Outlines` → empty.
#[test]
fn toc_get_004_empty() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    assert!(get_toc(&doc).is_empty());
}

/// `OUTLINE-001`: the outline tree exposes title/page/next/down with the right
/// shape (Chapter 1 → down Section 1.1; Chapter 1 → next Chapter 2).
#[test]
fn outline_001_tree_shape() {
    let doc = open(&doc_with_outlines());
    let root = get_outline(&doc).expect("has outline");
    assert_eq!(root.title, "Chapter 1");
    assert_eq!(root.page, 0);
    assert!(root.is_open);

    let down = root.down.as_ref().expect("Chapter 1 has a child");
    assert_eq!(down.title, "Section 1.1");
    assert_eq!(down.page, 1);
    assert!(down.next.is_none());

    let next = root.next.as_ref().expect("Chapter 1 has a sibling");
    assert_eq!(next.title, "Chapter 2");
    assert_eq!(next.page, 2);
    assert!(next.down.is_none());
}

/// `OUTLINE-002`: no `/Outlines` → `None`.
#[test]
fn outline_002_none() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    assert!(get_outline(&doc).is_none());
}

/// `TOC-SET-001`: flat 1-level list round-trips.
#[test]
fn toc_set_001_flat_roundtrip() {
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let input = vec![entry(1, "One", 0), entry(1, "Two", 1), entry(1, "Three", 2)];
    set_toc(&doc, &input).unwrap();
    assert_eq!(get_toc(&doc), input);
}

/// `TOC-SET-002`: nested levels round-trip.
#[test]
fn toc_set_002_nested_roundtrip() {
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let input = vec![
        entry(1, "A", 0),
        entry(2, "A.1", 1),
        entry(3, "A.1.a", 2),
        entry(2, "A.2", 0),
        entry(1, "B", 1),
    ];
    set_toc(&doc, &input).unwrap();
    assert_eq!(get_toc(&doc), input);
}

/// `TOC-SET-003`: the built tree has correct structural links.
#[test]
fn toc_set_003_structure() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let input = vec![entry(1, "A", 0), entry(2, "A.1", 1), entry(1, "B", 1)];
    set_toc(&doc, &input).unwrap();

    // Locate /Outlines and check Count + First/Last presence.
    let root = doc.root().unwrap();
    let catalog = doc.resolve(root).unwrap();
    let ol_ref = catalog
        .as_dict()
        .unwrap()
        .get(&Name::new("Outlines"))
        .and_then(Object::as_reference)
        .unwrap();
    let ol = doc.resolve(ol_ref).unwrap();
    let old = ol.as_dict().unwrap();
    assert_eq!(
        old.get(&Name::new("Count")).and_then(Object::as_i64),
        Some(3)
    );
    assert!(old.get(&Name::new("First")).is_some());
    assert!(old.get(&Name::new("Last")).is_some());
}

/// `TOC-SET-004` / `TOC-SET-006`: dests resolve to the right page after save+reopen.
#[test]
fn toc_set_006_persists_after_reopen() {
    let doc = open(&MultiPage::new(&["AAA", "BBB", "CCC"]).build());
    let input = vec![entry(1, "One", 0), entry(2, "Sub", 2), entry(1, "Two", 1)];
    set_toc(&doc, &input).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(get_toc(&re), input);
}

/// `TOC-SET-005`: empty list removes `/Outlines`.
#[test]
fn toc_set_005_empty_removes() {
    let doc = open(&doc_with_outlines());
    assert!(!get_toc(&doc).is_empty());
    set_toc(&doc, &[]).unwrap();
    assert!(get_toc(&doc).is_empty());
    let re = save_reopen(&doc);
    assert!(get_toc(&re).is_empty());
}

/// `TOC-JUMP-001`: a level jump (1→3) is rejected; the document is unmutated.
#[test]
fn toc_jump_001_rejected() {
    let doc = open(&MultiPage::new(&["AAA", "BBB"]).build());
    let bad = vec![entry(1, "A", 0), entry(3, "C", 1)];
    let err = set_toc(&doc, &bad).unwrap_err();
    assert_eq!(err.kind(), "invalid-argument");
    // Unmutated: no /Outlines was added.
    assert!(get_toc(&doc).is_empty());
}

/// `TOC-JUMP-002`: a first entry whose level != 1 is rejected.
#[test]
fn toc_jump_002_first_not_one() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let bad = vec![entry(2, "A", 0)];
    let err = set_toc(&doc, &bad).unwrap_err();
    assert_eq!(err.kind(), "invalid-argument");
}
