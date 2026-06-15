//! OCG (Optional Content Groups / layers) read-side tests (PRD §8.x, ISO §8.11).
//!
//! Self-built layered fixtures: a catalog with `/OCProperties` declaring two
//! OCGs in `/OCGs` and a default config `/D` that puts one ON, one OFF, marks
//! one Locked, and orders them in `/Order`. Asserts `get_ocgs`,
//! `layer_ui_configs`, `ocg_state`, and the non-layered / robustness cases.

mod common;

use common::{dict, name_obj, rref, Pdf};
use pdf_core::object::Name;
use pdf_core::ocg::{get_ocgs, layer_ui_configs, ocg_state};
use pdf_core::{DocumentStore, Limits, Object, PdfString, StringKind};

/// A literal PDF text string object.
fn pdf_str(s: &str) -> Object {
    Object::String(PdfString {
        bytes: s.as_bytes().to_vec(),
        kind: StringKind::Literal,
    })
}

/// A layered single-page document.
///
/// Objects: 1 catalog (+ /OCProperties ref 6), 2 pages, 3 page, 4 content,
/// 5 font, 6 /OCProperties, 7 OCG "Layer ON", 8 OCG "Layer OFF" (locked).
/// `/D`: ON=[7], OFF=[8], Locked=[8], Order=[7, 8].
fn layered_doc() -> Vec<u8> {
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ]);
    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", media),
        ("Contents", rref(4, 0)),
        (
            "Resources",
            Object::Dictionary(dict([(
                "Font",
                Object::Dictionary(dict([("F1", rref(5, 0))])),
            )])),
        ),
    ]));
    let content_body = b"BT /F1 12 Tf (hi) Tj ET";
    let content = Object::Stream(pdf_core::StreamObj::new_encoded(
        dict([("Length", Object::Integer(content_body.len() as i64))]),
        content_body.to_vec(),
    ));
    let font = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
    ]));

    // /D default config.
    let d = Object::Dictionary(dict([
        ("ON", Object::Array(vec![rref(7, 0)])),
        ("OFF", Object::Array(vec![rref(8, 0)])),
        ("Locked", Object::Array(vec![rref(8, 0)])),
        ("Order", Object::Array(vec![rref(7, 0), rref(8, 0)])),
    ]));
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        ("D", d),
    ]));
    let ocg_on = Object::Dictionary(dict([
        ("Type", name_obj("OCG")),
        ("Name", pdf_str("Layer ON")),
    ]));
    let ocg_off = Object::Dictionary(dict([
        ("Type", name_obj("OCG")),
        ("Name", pdf_str("Layer OFF")),
        ("Intent", name_obj("Design")),
    ]));

    Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2, 0)),
                ("OCProperties", rref(6, 0)),
            ])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("Count", Object::Integer(1)),
            ])),
        )
        .obj(3, 0, page)
        .obj(4, 0, content)
        .obj(5, 0, font)
        .obj(6, 0, ocp)
        .obj(7, 0, ocg_on)
        .obj(8, 0, ocg_off)
        .root(1, 0)
        .build()
}

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("opens")
}

// === OCG-READ-* ===========================================================

/// OCG-READ-COUNT: both declared OCGs are returned, keyed by object number.
#[test]
fn ocg_read_count() {
    let doc = open(&layered_doc());
    let ocgs = get_ocgs(&doc);
    assert_eq!(ocgs.len(), 2);
    assert!(ocgs.contains_key(&7));
    assert!(ocgs.contains_key(&8));
}

/// OCG-READ-NAME: the `/Name` of each OCG is decoded.
#[test]
fn ocg_read_name() {
    let doc = open(&layered_doc());
    let ocgs = get_ocgs(&doc);
    assert_eq!(ocgs[&7].name, "Layer ON");
    assert_eq!(ocgs[&8].name, "Layer OFF");
}

/// OCG-READ-STATE: ON layer reads on=true, OFF layer reads on=false.
#[test]
fn ocg_read_state() {
    let doc = open(&layered_doc());
    let ocgs = get_ocgs(&doc);
    assert!(ocgs[&7].on, "layer 7 should be ON");
    assert!(!ocgs[&8].on, "layer 8 should be OFF");
    // The standalone state query agrees.
    assert!(ocg_state(&doc, 7));
    assert!(!ocg_state(&doc, 8));
}

/// OCG-READ-LOCKED: the locked OCG reports locked=true, the other false.
#[test]
fn ocg_read_locked() {
    let doc = open(&layered_doc());
    let ocgs = get_ocgs(&doc);
    assert!(!ocgs[&7].locked);
    assert!(ocgs[&8].locked);
}

/// OCG-READ-INTENT: default `/View` intent vs an explicit `/Design`.
#[test]
fn ocg_read_intent() {
    let doc = open(&layered_doc());
    let ocgs = get_ocgs(&doc);
    assert_eq!(ocgs[&7].intent, vec!["View".to_string()]);
    assert_eq!(ocgs[&8].intent, vec!["Design".to_string()]);
}

/// OCG-READ-UICONFIG: `layer_ui_configs` flattens `/Order` to depth-tagged rows
/// carrying the per-layer state.
#[test]
fn ocg_read_ui_config() {
    let doc = open(&layered_doc());
    let cfgs = layer_ui_configs(&doc);
    assert_eq!(cfgs.len(), 2);
    assert_eq!(cfgs[0].number, 7);
    assert_eq!(cfgs[0].text, "Layer ON");
    assert_eq!(cfgs[0].depth, 0);
    assert_eq!(cfgs[0].kind, "checkbox");
    assert!(cfgs[0].on);
    assert!(!cfgs[0].locked);

    assert_eq!(cfgs[1].number, 8);
    assert!(!cfgs[1].on);
    assert!(cfgs[1].locked);
}

/// OCG-READ-ORDER-LABEL: a nested `/Order` group with a leading label string is
/// flattened into a label row followed by its (deeper) children.
#[test]
fn ocg_read_order_label() {
    // Reuse layered_doc but replace /Order with a labelled nested group.
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ]);
    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", media),
    ]));
    let d = Object::Dictionary(dict([
        ("ON", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "Order",
            Object::Array(vec![Object::Array(vec![
                pdf_str("Group A"),
                rref(7, 0),
                rref(8, 0),
            ])]),
        ),
    ]));
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        ("D", d),
    ]));
    let bytes = Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2, 0)),
                ("OCProperties", rref(6, 0)),
            ])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("Count", Object::Integer(1)),
            ])),
        )
        .obj(3, 0, page)
        .obj(6, 0, ocp)
        .obj(
            7,
            0,
            Object::Dictionary(dict([("Type", name_obj("OCG")), ("Name", pdf_str("L7"))])),
        )
        .obj(
            8,
            0,
            Object::Dictionary(dict([("Type", name_obj("OCG")), ("Name", pdf_str("L8"))])),
        )
        .root(1, 0)
        .build();

    let doc = open(&bytes);
    let cfgs = layer_ui_configs(&doc);
    // label row + 2 checkbox rows.
    assert_eq!(cfgs.len(), 3);
    assert_eq!(cfgs[0].kind, "label");
    assert_eq!(cfgs[0].text, "Group A");
    assert_eq!(cfgs[0].depth, 0);
    assert_eq!(cfgs[1].kind, "checkbox");
    assert_eq!(cfgs[1].number, 7);
    assert_eq!(cfgs[1].depth, 1);
    assert_eq!(cfgs[2].number, 8);
    assert_eq!(cfgs[2].depth, 1);
}

/// OCG-READ-BASESTATE-OFF: with `/BaseState /OFF`, an OCG not in `/ON` reads as
/// off; one explicitly in `/ON` reads on.
#[test]
fn ocg_read_basestate_off() {
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ]);
    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", media),
    ]));
    let d = Object::Dictionary(dict([
        ("BaseState", name_obj("OFF")),
        ("ON", Object::Array(vec![rref(7, 0)])),
    ]));
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        ("D", d),
    ]));
    let bytes = Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2, 0)),
                ("OCProperties", rref(6, 0)),
            ])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("Count", Object::Integer(1)),
            ])),
        )
        .obj(3, 0, page)
        .obj(6, 0, ocp)
        .obj(
            7,
            0,
            Object::Dictionary(dict([("Type", name_obj("OCG")), ("Name", pdf_str("L7"))])),
        )
        .obj(
            8,
            0,
            Object::Dictionary(dict([("Type", name_obj("OCG")), ("Name", pdf_str("L8"))])),
        )
        .root(1, 0)
        .build();

    let doc = open(&bytes);
    assert!(ocg_state(&doc, 7), "explicit ON wins over BaseState OFF");
    assert!(!ocg_state(&doc, 8), "unlisted OCG follows BaseState OFF");
}

// === OCG-NONE-* (non-layered / robustness) ================================

/// OCG-NONE-EMPTY: a document with no `/OCProperties` yields empty results and
/// never panics.
#[test]
fn ocg_none_empty() {
    let doc = open(&common::simple_doc());
    assert!(get_ocgs(&doc).is_empty());
    assert!(layer_ui_configs(&doc).is_empty());
    assert!(!ocg_state(&doc, 99));
}

/// OCG-NONE-MALFORMED: `/OCProperties` present but `/OCGs` is the wrong type —
/// no panic, empty result.
#[test]
fn ocg_none_malformed() {
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ]);
    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", media),
    ]));
    // /OCGs is an integer, /D is missing — degenerate but must not panic.
    let ocp = Object::Dictionary(dict([("OCGs", Object::Integer(0))]));
    let bytes = Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2, 0)),
                ("OCProperties", rref(6, 0)),
            ])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("Count", Object::Integer(1)),
            ])),
        )
        .obj(3, 0, page)
        .obj(6, 0, ocp)
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    assert!(get_ocgs(&doc).is_empty());
    assert!(layer_ui_configs(&doc).is_empty());
    // `ocg_state` must not panic on a malformed fixture (the value is the
    // BaseState default for an unlisted OCG; only "no panic" is contractual).
    let _ = ocg_state(&doc, 7);
}

/// Sanity: the OCG dicts carry the expected `/Type /OCG` (guards the fixture).
#[test]
fn ocg_fixture_type_ocg() {
    let doc = open(&layered_doc());
    let obj = doc.get_object(7, 0).expect("ocg 7");
    let d = obj.as_dict().expect("dict");
    assert_eq!(
        d.get(&Name::new("Type")).and_then(Object::as_name),
        Some(&Name::new("OCG"))
    );
}
