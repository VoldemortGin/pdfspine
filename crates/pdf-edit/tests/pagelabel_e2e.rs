//! `PAGELABEL-*` — `/PageLabels` number-tree read → per-page label (PRD §8.9).

mod common;

use common::{assemble_classic, dict, name_obj, open, rref};

use pdf_core::{ObjRef, Object, PdfString};
use pdf_edit::pagelabel::get_label;

/// Builds an N-page doc whose catalog `/PageLabels` is `nums` (a flat number-tree
/// `/Nums` array, alternating `int dict`), or no `/PageLabels` if `nums` is None.
fn doc_with_labels(n: u32, page_labels: Option<Object>) -> Vec<u8> {
    let media = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ])
    };
    let mut catalog_pairs: Vec<(&'static str, Object)> =
        vec![("Type", name_obj("Catalog")), ("Pages", rref(2))];
    let mut objects: Vec<(u32, Object)> = Vec::new();
    let mut kids = Vec::new();
    for i in 0..n {
        let leaf = 10 + i;
        kids.push(rref(leaf));
        objects.push((
            leaf,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media()),
            ])),
        ));
    }
    objects.push((
        2,
        Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Kids", Object::Array(kids)),
            ("Count", Object::Integer(n as i64)),
        ])),
    ));
    if let Some(pl) = page_labels {
        objects.push((3, pl));
        catalog_pairs.push(("PageLabels", rref(3)));
    }
    objects.push((1, Object::Dictionary(dict(catalog_pairs))));
    objects.sort_by_key(|(num, _)| *num);
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// A `/PageLabels` number tree from `(start_page, label_dict)` pairs.
fn page_labels(ranges: &[(i64, Object)]) -> Object {
    let mut nums = Vec::new();
    for (start, d) in ranges {
        nums.push(Object::Integer(*start));
        nums.push(d.clone());
    }
    Object::Dictionary(dict([("Nums", Object::Array(nums))]))
}

fn style(s: &str) -> (&'static str, Object) {
    ("S", name_obj(s))
}

/// `PAGELABEL-001`: decimal `D` with prefix.
#[test]
fn pagelabel_001_decimal_prefix() {
    let pl = page_labels(&[(
        0,
        Object::Dictionary(dict([
            style("D"),
            ("P", Object::String(PdfString::literal(b"A-".to_vec()))),
        ])),
    )]);
    let doc = open(&doc_with_labels(3, Some(pl)));
    assert_eq!(get_label(&doc, 0), "A-1");
    assert_eq!(get_label(&doc, 1), "A-2");
    assert_eq!(get_label(&doc, 2), "A-3");
}

/// `PAGELABEL-002`: lowercase roman.
#[test]
fn pagelabel_002_lower_roman() {
    let pl = page_labels(&[(0, Object::Dictionary(dict([style("r")])))]);
    let doc = open(&doc_with_labels(3, Some(pl)));
    assert_eq!(get_label(&doc, 0), "i");
    assert_eq!(get_label(&doc, 1), "ii");
    assert_eq!(get_label(&doc, 2), "iii");
}

/// `PAGELABEL-003`: uppercase roman + alpha styles.
#[test]
fn pagelabel_003_roman_alpha() {
    let pl = page_labels(&[
        (0, Object::Dictionary(dict([style("R")]))),
        (2, Object::Dictionary(dict([style("A")]))),
        (4, Object::Dictionary(dict([style("a")]))),
    ]);
    let doc = open(&doc_with_labels(6, Some(pl)));
    assert_eq!(get_label(&doc, 0), "I");
    assert_eq!(get_label(&doc, 1), "II");
    assert_eq!(get_label(&doc, 2), "A");
    assert_eq!(get_label(&doc, 3), "B");
    assert_eq!(get_label(&doc, 4), "a");
    assert_eq!(get_label(&doc, 5), "b");
}

/// `PAGELABEL-004`: multiple ranges apply to correct spans.
#[test]
fn pagelabel_004_multiple_ranges() {
    let pl = page_labels(&[
        (0, Object::Dictionary(dict([style("r")]))), // i, ii
        (
            2,
            Object::Dictionary(dict([
                style("D"),
                ("P", Object::String(PdfString::literal(b"p".to_vec()))),
            ])),
        ), // p1, p2
    ]);
    let doc = open(&doc_with_labels(4, Some(pl)));
    assert_eq!(get_label(&doc, 0), "i");
    assert_eq!(get_label(&doc, 1), "ii");
    assert_eq!(get_label(&doc, 2), "p1");
    assert_eq!(get_label(&doc, 3), "p2");
}

/// `PAGELABEL-005`: no `/PageLabels` → empty string.
#[test]
fn pagelabel_005_absent() {
    let doc = open(&doc_with_labels(2, None));
    assert_eq!(get_label(&doc, 0), "");
    assert_eq!(get_label(&doc, 1), "");
}

/// `PAGELABEL-006`: `/St` start value honored.
#[test]
fn pagelabel_006_start_value() {
    let pl = page_labels(&[(
        0,
        Object::Dictionary(dict([style("D"), ("St", Object::Integer(5))])),
    )]);
    let doc = open(&doc_with_labels(3, Some(pl)));
    assert_eq!(get_label(&doc, 0), "5");
    assert_eq!(get_label(&doc, 1), "6");
    assert_eq!(get_label(&doc, 2), "7");
}
