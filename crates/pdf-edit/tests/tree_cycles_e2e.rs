//! Outline / name-tree / number-tree walks over shared or cyclic nodes.
//!
//! The walkers are depth-capped, but a node reachable twice per level (a DAG)
//! is otherwise re-walked once per path — 2^depth work and output. Each node is
//! now walked once, so these trees resolve to their distinct nodes.

mod common;

use common::{assemble_classic, dict, name_obj, open, rref};

use pdf_core::{ObjRef, Object, PdfString};
use pdf_edit::dest::resolve_names;
use pdf_edit::embfile::embfile_names;
use pdf_edit::pagelabel::get_label_rules;
use pdf_edit::toc::{get_outline, get_toc, OutlineNode};

/// Levels of sharing: a pre-fix walk visits 2^LEVELS paths.
const LEVELS: u32 = 16;

fn s(text: &str) -> Object {
    Object::String(PdfString::literal(text.as_bytes().to_vec()))
}

/// Builds a one-page document whose outline, `/Dests` and `/EmbeddedFiles`
/// name trees and `/PageLabels` number tree each share one node per level.
fn shared_trees_doc() -> pdf_core::DocumentStore {
    let mut objs: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2)),
                ("Outlines", rref(10)),
                (
                    "Names",
                    Object::Dictionary(dict([("Dests", rref(100)), ("EmbeddedFiles", rref(200))])),
                ),
                ("PageLabels", rref(300)),
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
                    Object::Array(
                        vec![0, 0, 612, 792]
                            .into_iter()
                            .map(Object::Integer)
                            .collect(),
                    ),
                ),
            ])),
        ),
        (
            10,
            Object::Dictionary(dict([("Type", name_obj("Outlines")), ("First", rref(11))])),
        ),
        (
            400,
            Object::Dictionary(dict([("Type", name_obj("Filespec")), ("F", s("a.txt"))])),
        ),
    ];
    for l in 0..LEVELS {
        // Outline item whose first child and next sibling are the same item.
        let mut item = dict([("Title", s(&format!("item {l}")))]);
        if l + 1 < LEVELS {
            item.insert(pdf_core::Name::new("First"), rref(12 + l));
            item.insert(pdf_core::Name::new("Next"), rref(12 + l));
        }
        objs.push((11 + l, Object::Dictionary(item)));
        // Tree branches listing the same kid twice.
        let kids = |base: u32| Object::Array(vec![rref(base + l + 1), rref(base + l + 1)]);
        objs.push((100 + l, Object::Dictionary(dict([("Kids", kids(100))]))));
        objs.push((200 + l, Object::Dictionary(dict([("Kids", kids(200))]))));
        objs.push((300 + l, Object::Dictionary(dict([("Kids", kids(300))]))));
    }
    let dest = Object::Array(vec![rref(3), name_obj("Fit")]);
    objs.push((
        100 + LEVELS,
        Object::Dictionary(dict([("Names", Object::Array(vec![s("dest"), dest]))])),
    ));
    objs.push((
        200 + LEVELS,
        Object::Dictionary(dict([(
            "Names",
            Object::Array(vec![s("a.txt"), rref(400)]),
        )])),
    ));
    objs.push((
        300 + LEVELS,
        Object::Dictionary(dict([(
            "Nums",
            Object::Array(vec![
                Object::Integer(0),
                Object::Dictionary(dict([("S", name_obj("D"))])),
            ]),
        )])),
    ));
    objs.sort_by_key(|(n, _)| *n);
    open(&assemble_classic(&objs, ObjRef::new(1, 0)))
}

fn outline_nodes(n: &OutlineNode) -> usize {
    1 + n.down.as_deref().map_or(0, outline_nodes) + n.next.as_deref().map_or(0, outline_nodes)
}

#[test]
fn shared_outline_items_are_walked_once() {
    let doc = shared_trees_doc();
    let toc = get_toc(&doc);
    assert_eq!(toc.len(), LEVELS as usize, "{}", toc.len());
    let outline = get_outline(&doc).expect("outline");
    assert_eq!(outline_nodes(&outline), LEVELS as usize);
}

#[test]
fn shared_name_and_number_tree_nodes_are_walked_once() {
    let doc = shared_trees_doc();
    assert_eq!(resolve_names(&doc).len(), 1);
    assert_eq!(embfile_names(&doc), vec!["a.txt".to_string()]);
    assert_eq!(get_label_rules(&doc).len(), 1);
    let pages = pdf_edit::dest::page_index_map(&doc);
    assert_eq!(
        pdf_edit::dest::resolve_named(&doc, b"dest", &pages),
        Some(0)
    );
}
