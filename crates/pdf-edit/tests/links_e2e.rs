//! `LINK-*` — link annotations read / insert / update / delete (PRD §8.9).

mod common;

use common::{assemble_classic, dict, name_obj, open, rref, save_reopen};

use pdf_core::geom::Rect;
use pdf_core::{ObjRef, Object, PdfString};
use pdf_edit::links::{delete_link, get_links, insert_link, update_link, LinkKind};

/// A 2-page doc (leaves 4, 6) where page 0 carries `annots` in `/Annots`.
fn doc_with_annots(annots: Vec<(u32, Object)>, annot_refs: Vec<u32>) -> Vec<u8> {
    let media = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ])
    };
    let mut page0 = dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2)),
        ("MediaBox", media()),
    ]);
    if !annot_refs.is_empty() {
        page0.insert(
            pdf_core::Name::new("Annots"),
            Object::Array(annot_refs.into_iter().map(rref).collect()),
        );
    }
    let page1 = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2)),
        ("MediaBox", media()),
    ]));

    let mut objects = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(4), rref(6)])),
                ("Count", Object::Integer(2)),
            ])),
        ),
        (4, Object::Dictionary(page0)),
        (6, page1),
    ];
    objects.extend(annots);
    objects.sort_by_key(|(num, _)| *num);
    assemble_classic(&objects, ObjRef::new(1, 0))
}

fn uri_annot(uri: &str) -> Object {
    Object::Dictionary(dict([
        ("Type", name_obj("Annot")),
        ("Subtype", name_obj("Link")),
        (
            "Rect",
            Object::Array(vec![
                Object::Integer(10),
                Object::Integer(20),
                Object::Integer(100),
                Object::Integer(40),
            ]),
        ),
        (
            "A",
            Object::Dictionary(dict([
                ("S", name_obj("URI")),
                (
                    "URI",
                    Object::String(PdfString::literal(uri.as_bytes().to_vec())),
                ),
            ])),
        ),
    ]))
}

fn goto_annot(page: u32) -> Object {
    Object::Dictionary(dict([
        ("Type", name_obj("Annot")),
        ("Subtype", name_obj("Link")),
        (
            "Rect",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(50),
                Object::Integer(50),
            ]),
        ),
        (
            "Dest",
            Object::Array(vec![
                rref(page),
                name_obj("XYZ"),
                Object::Null,
                Object::Null,
                Object::Null,
            ]),
        ),
    ]))
}

/// `LINK-GET-001`: a URI link is read with rect + uri.
#[test]
fn link_get_001_uri() {
    let doc = open(&doc_with_annots(
        vec![(20, uri_annot("https://example.com"))],
        vec![20],
    ));
    let links = get_links(&doc, 0);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Uri("https://example.com".into()));
    assert_eq!(links[0].from, Rect::new(10.0, 20.0, 100.0, 40.0));
    assert_eq!(links[0].xref, 20);
}

/// `LINK-GET-002`: a GoTo link reads its target page.
#[test]
fn link_get_002_goto() {
    let doc = open(&doc_with_annots(vec![(20, goto_annot(6))], vec![20]));
    let links = get_links(&doc, 0);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Goto(1));
}

/// `LINK-GET-003`: a page with no `/Annots` → empty.
#[test]
fn link_get_003_no_annots() {
    let doc = open(&doc_with_annots(vec![], vec![]));
    assert!(get_links(&doc, 0).is_empty());
    assert!(get_links(&doc, 1).is_empty());
}

/// `LINK-INSERT-001`: insert a URI link; reopen shows it.
#[test]
fn link_insert_001_uri() {
    let doc = open(&doc_with_annots(vec![], vec![]));
    insert_link(
        &doc,
        0,
        &Rect::new(5.0, 5.0, 55.0, 25.0),
        &LinkKind::Uri("https://oxide-pdf.dev".into()),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let links = get_links(&re, 0);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Uri("https://oxide-pdf.dev".into()));
    assert_eq!(links[0].from, Rect::new(5.0, 5.0, 55.0, 25.0));
}

/// `LINK-INSERT-002`: insert a GoTo link; reopen target correct.
#[test]
fn link_insert_002_goto() {
    let doc = open(&doc_with_annots(vec![], vec![]));
    insert_link(
        &doc,
        0,
        &Rect::new(0.0, 0.0, 10.0, 10.0),
        &LinkKind::Goto(1),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let links = get_links(&re, 0);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Goto(1));
}

/// `LINK-INSERT-003`: inserting on a page with an existing /Annots appends.
#[test]
fn link_insert_003_appends() {
    let doc = open(&doc_with_annots(
        vec![(20, uri_annot("https://a.test"))],
        vec![20],
    ));
    insert_link(
        &doc,
        0,
        &Rect::new(1.0, 1.0, 2.0, 2.0),
        &LinkKind::Uri("https://b.test".into()),
    )
    .unwrap();
    let re = save_reopen(&doc);
    assert_eq!(get_links(&re, 0).len(), 2);
}

/// `LINK-UPDATE-001`: update a link's rect + uri.
#[test]
fn link_update_001() {
    let doc = open(&doc_with_annots(
        vec![(20, uri_annot("https://old.test"))],
        vec![20],
    ));
    update_link(
        &doc,
        ObjRef::new(20, 0),
        &Rect::new(9.0, 9.0, 99.0, 99.0),
        &LinkKind::Uri("https://new.test".into()),
    )
    .unwrap();
    let re = save_reopen(&doc);
    let links = get_links(&re, 0);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Uri("https://new.test".into()));
    assert_eq!(links[0].from, Rect::new(9.0, 9.0, 99.0, 99.0));
}

/// `LINK-DELETE-001`: delete a link.
#[test]
fn link_delete_001() {
    let doc = open(&doc_with_annots(
        vec![(20, uri_annot("https://x.test")), (21, goto_annot(6))],
        vec![20, 21],
    ));
    assert_eq!(get_links(&doc, 0).len(), 2);
    delete_link(&doc, 0, ObjRef::new(20, 0)).unwrap();
    let re = save_reopen(&doc);
    let links = get_links(&re, 0);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Goto(1));
}
