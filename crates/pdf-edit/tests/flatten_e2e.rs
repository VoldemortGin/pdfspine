//! M3c page-tree flatten / consistency — `PAGEOPS-FLATTEN-*` (PRD §8.7).

mod common;

use common::{all_page_text, nested_doc, open, save_reopen};

use pdf_core::Name;
use pdf_edit::PageEditor;

/// `PAGEOPS-FLATTEN-001`: a nested two-level tree normalizes to a flat `/Kids`
/// under the root on first edit; page order preserved.
#[test]
fn pageops_flatten_001_flat_kids() {
    let doc = open(&nested_doc());
    let ed = PageEditor::new(&doc).unwrap();
    // The root /Pages now has a flat /Kids of all three leaves.
    let pages = doc.resolve(ed.pages_ref()).unwrap();
    let kids = pages
        .as_dict()
        .unwrap()
        .get(&Name::new("Kids"))
        .and_then(pdf_core::Object::as_array)
        .unwrap();
    assert_eq!(kids.len(), 3, "flat /Kids has all leaves");
    assert_eq!(ed.page_count(), 3);

    let re = save_reopen(&doc);
    assert_eq!(all_page_text(&re), vec!["AAA", "BBB", "CCC"]);
}

/// `PAGEOPS-FLATTEN-002`: inherited MediaBox / Rotate are materialized onto the
/// leaves after flatten.
#[test]
fn pageops_flatten_002_materialized_attrs() {
    let doc = open(&nested_doc());
    let _ed = PageEditor::new(&doc).unwrap();
    let re = save_reopen(&doc);
    let refs = pdf_core::pagetree::page_refs(&re);

    // Leaves 0,1 inherited MediaBox [0 0 400 500] from the root and Rotate 90
    // from the intermediate node.
    for i in [0usize, 1] {
        let leaf = refs[i];
        let d = pdf_core::pagetree::page_dict(&re, leaf).unwrap();
        assert!(
            d.contains_key(&Name::new("MediaBox")),
            "leaf {i} has explicit MediaBox"
        );
        assert!(
            d.contains_key(&Name::new("Rotate")),
            "leaf {i} has explicit Rotate"
        );
        assert_eq!(pdf_core::pagetree::rotation(&re, leaf), 90);
        assert_eq!(
            pdf_core::pagetree::mediabox(&re, leaf),
            pdf_core::geom::Rect::new(0.0, 0.0, 400.0, 500.0)
        );
    }
    // Leaf 2 inherited only the MediaBox (no Rotate anywhere on its chain).
    let leaf2 = refs[2];
    assert_eq!(
        pdf_core::pagetree::mediabox(&re, leaf2),
        pdf_core::geom::Rect::new(0.0, 0.0, 400.0, 500.0)
    );
    assert_eq!(pdf_core::pagetree::rotation(&re, leaf2), 0);
}

/// `PAGEOPS-FLATTEN-003`: every leaf's `/Parent` points at the root `/Pages`
/// after flatten.
#[test]
fn pageops_flatten_003_parent_points_to_root() {
    let doc = open(&nested_doc());
    let ed = PageEditor::new(&doc).unwrap();
    let root = ed.pages_ref();
    for leaf in pdf_core::pagetree::page_refs(&doc) {
        let parent = pdf_core::pagetree::page_dict(&doc, leaf)
            .unwrap()
            .get(&Name::new("Parent"))
            .and_then(pdf_core::Object::as_reference)
            .unwrap();
        assert_eq!(parent, root, "leaf {leaf:?} /Parent == root /Pages");
    }
}
