//! OCG (Optional Content Groups / layers) read-side tests (PRD §8.x, ISO §8.11).
//!
//! Self-built layered fixtures: a catalog with `/OCProperties` declaring two
//! OCGs in `/OCGs` and a default config `/D` that puts one ON, one OFF, marks
//! one Locked, and orders them in `/Order`. Asserts `get_ocgs`,
//! `layer_ui_configs`, `ocg_state`, and the non-layered / robustness cases.

mod common;

use common::{dict, name_obj, rref, Pdf};
use pdf_core::object::Name;
use pdf_core::ocg::{
    get_layers, get_oc, get_ocgs, get_ocmd, layer_config_count, layer_ui_configs, ocg_state,
    select_layer_config, set_layer_ui_config, OcVisibility, VeExpr,
};
use pdf_core::{DocumentStore, Error, Limits, Object, PdfString, StringKind};

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
    assert_eq!(cfgs[0].ocg, 7);
    assert_eq!(cfgs[0].text, "Layer ON");
    assert_eq!(cfgs[0].depth, 0);
    assert_eq!(cfgs[0].kind, "checkbox");
    assert!(cfgs[0].on);
    assert!(!cfgs[0].locked);

    assert_eq!(cfgs[1].ocg, 8);
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
    assert_eq!(cfgs[1].ocg, 7);
    assert_eq!(cfgs[1].depth, 1);
    assert_eq!(cfgs[2].ocg, 8);
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

// === OCG-READ-{LAYERS,CONFIG,UI,OC,OCMD} + OCG-VIS-* ======================
//
// Fixtures below extend the `layered_doc()` byte-builder style: `build_doc`
// assembles a one-page catalog whose `/OCProperties` is object 6, plus caller-
// supplied OCG / OCMD / XObject objects. No fonts are needed (read side only).

/// An `/Type /OCG` dictionary with the given `/Name`.
fn ocg_dict(name: &str) -> Object {
    Object::Dictionary(dict([("Type", name_obj("OCG")), ("Name", pdf_str(name))]))
}

/// Builds a one-page doc: catalog (obj 1, `/OCProperties` → 6), pages (obj 2),
/// a blank page (obj 3), the given `/OCProperties` dict (obj 6), plus `extra`
/// objects (OCG dicts at 7,8,…, OCMDs, XObjects…).
fn build_doc(ocp: Object, extra: Vec<(u32, Object)>) -> Vec<u8> {
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ]);
    let mut pdf = Pdf::new()
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
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("MediaBox", media),
            ])),
        )
        .obj(6, 0, ocp);
    for (num, obj) in extra {
        pdf = pdf.obj(num, 0, obj);
    }
    pdf.root(1, 0).build()
}

/// A doc with two OCGs and one alternate config `{/BaseState /OFF /ON [8]}`
/// (no `/Order` of its own). `/D`: ON=[7], OFF=[8], Order=[7, 8].
fn config_doc() -> Vec<u8> {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([
                ("ON", Object::Array(vec![rref(7, 0)])),
                ("OFF", Object::Array(vec![rref(8, 0)])),
                ("Order", Object::Array(vec![rref(7, 0), rref(8, 0)])),
            ])),
        ),
        (
            "Configs",
            Object::Array(vec![Object::Dictionary(dict([
                ("Name", pdf_str("Alt")),
                ("BaseState", name_obj("OFF")),
                ("ON", Object::Array(vec![rref(8, 0)])),
            ]))]),
        ),
    ]));
    build_doc(ocp, vec![(7, ocg_dict("A")), (8, ocg_dict("B"))])
}

/// OCG-READ-LAYERS: `/Configs` entries are listed by array index with their
/// `/Name` / `/Creator`; `/D` is never listed.
#[test]
fn ocg_read_layers() {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([("ON", Object::Array(vec![rref(7, 0), rref(8, 0)]))])),
        ),
        (
            "Configs",
            Object::Array(vec![
                Object::Dictionary(dict([
                    ("Name", pdf_str("Config A")),
                    ("Creator", pdf_str("pdfspine")),
                ])),
                Object::Dictionary(dict([("Name", pdf_str("Config B"))])),
            ]),
        ),
    ]));
    let doc = open(&build_doc(
        ocp,
        vec![(7, ocg_dict("A")), (8, ocg_dict("B"))],
    ));
    let layers = get_layers(&doc);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].number, 0);
    assert_eq!(layers[0].name.as_deref(), Some("Config A"));
    assert_eq!(layers[0].creator.as_deref(), Some("pdfspine"));
    assert_eq!(layers[1].number, 1);
    assert_eq!(layers[1].name.as_deref(), Some("Config B"));
    assert_eq!(layers[1].creator, None);
    assert_eq!(layer_config_count(&doc), 2);
}

/// OCG-READ-LAYERS-NONE: no `/OCProperties`, or `/OCProperties` without
/// `/Configs`, yields an empty layer list / zero count.
#[test]
fn ocg_read_layers_none() {
    let doc = open(&common::simple_doc());
    assert!(get_layers(&doc).is_empty());
    assert_eq!(layer_config_count(&doc), 0);

    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0)])),
        (
            "D",
            Object::Dictionary(dict([("ON", Object::Array(vec![rref(7, 0)]))])),
        ),
    ]));
    let doc2 = open(&build_doc(ocp, vec![(7, ocg_dict("A"))]));
    assert!(get_layers(&doc2).is_empty());
    assert_eq!(layer_config_count(&doc2), 0);
}

/// OCG-READ-LAYERS-NAMES: a config missing `/Creator` reads `creator = None`;
/// one missing `/Name` reads `name = None`.
#[test]
fn ocg_read_layers_names() {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0)])),
        (
            "D",
            Object::Dictionary(dict([("ON", Object::Array(vec![rref(7, 0)]))])),
        ),
        (
            "Configs",
            Object::Array(vec![
                Object::Dictionary(dict([("Name", pdf_str("Named"))])),
                Object::Dictionary(dict([("BaseState", name_obj("OFF"))])),
            ]),
        ),
    ]));
    let doc = open(&build_doc(ocp, vec![(7, ocg_dict("A"))]));
    let layers = get_layers(&doc);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].name.as_deref(), Some("Named"));
    assert_eq!(layers[0].creator, None);
    assert_eq!(layers[1].name, None);
    assert_eq!(layers[1].creator, None);
}

/// OCG-READ-CONFIG-SELECT: selecting an alternate `{/BaseState /OFF /ON [b]}`
/// flips the read state (get_ocgs / ocg_state / OcVisibility) without touching
/// the document (`is_dirty` stays false); selecting `None` restores `/D`.
#[test]
fn ocg_read_config_select() {
    let doc = open(&config_doc());
    // Default `/D`: A on, B off.
    assert!(ocg_state(&doc, 7));
    assert!(!ocg_state(&doc, 8));
    assert!(!doc.is_dirty());

    select_layer_config(&doc, Some(0)).expect("select alt");
    let ocgs = get_ocgs(&doc);
    assert!(!ocgs[&7].on, "A off under the alternate base-OFF config");
    assert!(ocgs[&8].on, "B explicitly ON in the alternate config");
    assert!(!ocg_state(&doc, 7));
    assert!(ocg_state(&doc, 8));

    let vis = OcVisibility::read(&doc);
    assert!(vis.is_ocg_hidden(7));
    assert!(!vis.is_ocg_hidden(8));
    assert!(vis.is_hidden(&doc, &rref(7, 0)));
    assert!(!vis.is_hidden(&doc, &rref(8, 0)));
    assert!(!doc.is_dirty(), "selecting a config is in-memory only");

    select_layer_config(&doc, None).expect("select default");
    assert!(ocg_state(&doc, 7));
    assert!(!ocg_state(&doc, 8));
}

/// OCG-READ-CONFIG-BAD: `Some(n)` with `n >= count` is an error; `None` is ok.
#[test]
fn ocg_read_config_bad() {
    let doc = open(&config_doc());
    assert_eq!(layer_config_count(&doc), 1);
    let err = select_layer_config(&doc, Some(1)).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidArgument("Illegal Layer config")
    ));
    assert!(select_layer_config(&doc, Some(5)).is_err());
    select_layer_config(&doc, None).expect("None is always ok");
}

/// OCG-READ-CONFIG-FALLBACK: an alternate config without `/Order` inherits
/// `/D`'s `/Order` for `layer_ui_configs`, but reports its own ON states.
#[test]
fn ocg_read_config_fallback() {
    let doc = open(&config_doc());
    select_layer_config(&doc, Some(0)).expect("select");
    let rows = layer_ui_configs(&doc);
    assert_eq!(rows.len(), 2, "rows follow /D /Order [7, 8]");
    assert_eq!(rows[0].ocg, 7);
    assert_eq!(rows[1].ocg, 8);
    assert!(!rows[0].on, "A off in the alternate config");
    assert!(rows[1].on, "B on in the alternate config");
}

/// OCG-READ-UI-NUMBER: rows carry a 0-based `number` (row index) and the `ocg`
/// xref field.
#[test]
fn ocg_read_ui_number() {
    let doc = open(&layered_doc());
    let rows = layer_ui_configs(&doc);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].number, 0);
    assert_eq!(rows[0].ocg, 7);
    assert_eq!(rows[1].number, 1);
    assert_eq!(rows[1].ocg, 8);
}

/// A doc whose `/Order` nests both OCGs under a label string; OCG 8 is
/// `/D /Locked`.
fn label_locked_doc() -> Vec<u8> {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([
                ("ON", Object::Array(vec![rref(7, 0), rref(8, 0)])),
                ("Locked", Object::Array(vec![rref(8, 0)])),
                (
                    "Order",
                    Object::Array(vec![Object::Array(vec![
                        pdf_str("Group"),
                        rref(7, 0),
                        rref(8, 0),
                    ])]),
                ),
            ])),
        ),
    ]));
    build_doc(ocp, vec![(7, ocg_dict("A")), (8, ocg_dict("B"))])
}

/// OCG-READ-UI-LABEL-LOCKED: a nested-group label row reports `kind == "label"`,
/// `ocg == 0`, `locked == true`; a `/D /Locked` OCG row reports `locked`.
#[test]
fn ocg_read_ui_label_locked() {
    let doc = open(&label_locked_doc());
    let rows = layer_ui_configs(&doc);
    assert_eq!(rows.len(), 3, "label row + 2 checkbox rows");
    assert_eq!(rows[0].kind, "label");
    assert_eq!(rows[0].text, "Group");
    assert_eq!(rows[0].ocg, 0);
    assert_eq!(rows[0].depth, 0);
    assert!(rows[0].locked, "label rows report locked");
    assert_eq!(rows[1].ocg, 7);
    assert_eq!(rows[1].depth, 1);
    assert!(!rows[1].locked);
    assert_eq!(rows[2].ocg, 8);
    assert!(rows[2].locked, "OCG 8 is in /D /Locked");
}

/// A doc whose `/D /RBGroups` places both OCGs in one radio group.
fn radio_doc() -> Vec<u8> {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([
                ("ON", Object::Array(vec![rref(7, 0)])),
                ("OFF", Object::Array(vec![rref(8, 0)])),
                ("Order", Object::Array(vec![rref(7, 0), rref(8, 0)])),
                (
                    "RBGroups",
                    Object::Array(vec![Object::Array(vec![rref(7, 0), rref(8, 0)])]),
                ),
            ])),
        ),
    ]));
    build_doc(ocp, vec![(7, ocg_dict("A")), (8, ocg_dict("B"))])
}

/// OCG-READ-UI-RADIO: OCGs in an `/RBGroups` group report `kind == "radiobox"`.
#[test]
fn ocg_read_ui_radio() {
    let doc = open(&radio_doc());
    let rows = layer_ui_configs(&doc);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].kind, "radiobox");
    assert_eq!(rows[1].kind, "radiobox");
}

/// OCG-READ-UI-SELECT: action 0=ON / 1=toggle / 2=OFF applied by row index;
/// out-of-range errors; label / locked rows are no-ops; a radio ON clears the
/// other group members. All in-memory (`is_dirty` stays false).
#[test]
fn ocg_read_ui_select() {
    let doc = open(&layered_doc()); // rows: 0->OCG7(on), 1->OCG8(off, locked)
    set_layer_ui_config(&doc, 0, 2).expect("OFF ok");
    assert!(!ocg_state(&doc, 7));
    set_layer_ui_config(&doc, 0, 1).expect("toggle ok");
    assert!(ocg_state(&doc, 7));
    set_layer_ui_config(&doc, 0, 0).expect("ON ok");
    assert!(ocg_state(&doc, 7));
    assert!(!doc.is_dirty(), "layer-UI ops are in-memory only");

    let err = set_layer_ui_config(&doc, 99, 0).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidArgument("Out of range UI entry selected")
    ));

    // Locked row (OCG 8) is a no-op even when asked ON.
    assert!(!ocg_state(&doc, 8));
    set_layer_ui_config(&doc, 1, 0).expect("locked no-op ok");
    assert!(!ocg_state(&doc, 8), "locked row stays OFF");

    // Label row is a no-op (still Ok).
    let labeled = open(&label_locked_doc());
    set_layer_ui_config(&labeled, 0, 0).expect("label no-op ok");

    // Radio ON clears the other member of the group.
    let radio = open(&radio_doc());
    set_layer_ui_config(&radio, 1, 0).expect("radio ON ok");
    assert!(ocg_state(&radio, 8));
    assert!(
        !ocg_state(&radio, 7),
        "radio group cleared the other member"
    );
    assert!(!radio.is_dirty());
}

/// A doc with two OCGs and image/form XObjects (with and without `/OC`) plus a
/// font, for the `get_oc` tests.
fn oc_targets_doc() -> Vec<u8> {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([("ON", Object::Array(vec![rref(7, 0), rref(8, 0)]))])),
        ),
    ]));
    let img_with_oc = Object::Dictionary(dict([
        ("Type", name_obj("XObject")),
        ("Subtype", name_obj("Image")),
        ("Width", Object::Integer(1)),
        ("Height", Object::Integer(1)),
        ("OC", rref(7, 0)),
    ]));
    let form_with_oc = Object::Dictionary(dict([
        ("Type", name_obj("XObject")),
        ("Subtype", name_obj("Form")),
        ("OC", rref(8, 0)),
    ]));
    let img_no_oc = Object::Dictionary(dict([
        ("Type", name_obj("XObject")),
        ("Subtype", name_obj("Image")),
        ("Width", Object::Integer(1)),
        ("Height", Object::Integer(1)),
    ]));
    let font = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
    ]));
    build_doc(
        ocp,
        vec![
            (7, ocg_dict("A")),
            (8, ocg_dict("B")),
            (10, img_with_oc),
            (11, form_with_oc),
            (12, img_no_oc),
            (13, font),
        ],
    )
}

/// OCG-READ-OC: `get_oc` returns the `/OC` reference number of an image / form
/// XObject, or 0 when `/OC` is absent.
#[test]
fn ocg_read_oc() {
    let doc = open(&oc_targets_doc());
    assert_eq!(get_oc(&doc, 10).expect("image /OC"), 7);
    assert_eq!(get_oc(&doc, 11).expect("form /OC"), 8);
    assert_eq!(get_oc(&doc, 12).expect("no /OC"), 0);
}

/// OCG-READ-OC-BADTYPE: `get_oc` on a non-XObject (catalog / page / OCG / font)
/// is an `InvalidArgument` error.
#[test]
fn ocg_read_oc_badtype() {
    let doc = open(&oc_targets_doc());
    assert!(get_oc(&doc, 1).is_err(), "catalog");
    assert!(get_oc(&doc, 3).is_err(), "page");
    assert!(get_oc(&doc, 7).is_err(), "OCG");
    assert!(get_oc(&doc, 13).is_err(), "font");
    assert!(matches!(
        get_oc(&doc, 1).unwrap_err(),
        Error::InvalidArgument(_)
    ));
}

/// A doc carrying OCMDs: array `/OCGs`, single-ref `/OCGs`, absent `/OCGs`, and
/// a nested `/VE`.
fn ocmd_doc() -> Vec<u8> {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([
                ("ON", Object::Array(vec![rref(7, 0)])),
                ("OFF", Object::Array(vec![rref(8, 0)])),
            ])),
        ),
    ]));
    let ocmd_array = Object::Dictionary(dict([
        ("Type", name_obj("OCMD")),
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        ("P", name_obj("AnyOn")),
    ]));
    let ocmd_single = Object::Dictionary(dict([("Type", name_obj("OCMD")), ("OCGs", rref(7, 0))]));
    let ocmd_absent = Object::Dictionary(dict([("Type", name_obj("OCMD"))]));
    let ocmd_ve = Object::Dictionary(dict([
        ("Type", name_obj("OCMD")),
        (
            "VE",
            Object::Array(vec![
                name_obj("And"),
                rref(7, 0),
                Object::Array(vec![name_obj("Not"), rref(8, 0)]),
            ]),
        ),
    ]));
    build_doc(
        ocp,
        vec![
            (7, ocg_dict("A")),
            (8, ocg_dict("B")),
            (20, ocmd_array),
            (21, ocmd_single),
            (22, ocmd_absent),
            (23, ocmd_ve),
        ],
    )
}

/// OCG-READ-OCMD: `get_ocmd` reads `/OCGs` (array or single ref), `/P` policy,
/// and reports `None` when `/OCGs` is absent.
#[test]
fn ocg_read_ocmd() {
    let doc = open(&ocmd_doc());
    let a = get_ocmd(&doc, 20).expect("array ocmd");
    assert_eq!(a.xref, 20);
    assert_eq!(a.ocgs, Some(vec![7, 8]));
    assert_eq!(a.policy.as_deref(), Some("AnyOn"));
    assert_eq!(a.ve, None);

    let s = get_ocmd(&doc, 21).expect("single-ref ocmd");
    assert_eq!(s.ocgs, Some(vec![7]));
    assert_eq!(s.policy, None);

    let none = get_ocmd(&doc, 22).expect("absent /OCGs ocmd");
    assert_eq!(none.ocgs, None);
    assert_eq!(none.policy, None);
    assert_eq!(none.ve, None);
}

/// OCG-READ-OCMD-VE: a nested `/VE` parses into a `VeExpr` tree with `Ocg`
/// leaves.
#[test]
fn ocg_read_ocmd_ve() {
    let doc = open(&ocmd_doc());
    let m = get_ocmd(&doc, 23).expect("ve ocmd");
    let ve = m.ve.expect("has /VE");
    match ve {
        VeExpr::Op { op, args } => {
            assert_eq!(op, "And");
            assert_eq!(args.len(), 2);
            assert_eq!(args[0], VeExpr::Ocg(7));
            match &args[1] {
                VeExpr::Op { op, args } => {
                    assert_eq!(op, "Not");
                    assert_eq!(args, &vec![VeExpr::Ocg(8)]);
                }
                other => panic!("nested Not expected, got {other:?}"),
            }
        }
        other => panic!("Op expected, got {other:?}"),
    }
}

/// OCG-READ-OCMD-BADTYPE: `get_ocmd` on a non-OCMD object is an error.
#[test]
fn ocg_read_ocmd_badtype() {
    let doc = open(&ocmd_doc());
    assert!(get_ocmd(&doc, 7).is_err(), "OCG is not an OCMD");
    assert!(get_ocmd(&doc, 3).is_err(), "page is not an OCMD");
    assert!(matches!(
        get_ocmd(&doc, 7).unwrap_err(),
        Error::InvalidArgument(_)
    ));
}

/// A doc with A (7) ON, B (8) OFF, plus OCMDs over them exercising every policy
/// and `/VE` shape (numbers 30..=40).
fn vis_doc() -> Vec<u8> {
    let ocp = Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([
                ("ON", Object::Array(vec![rref(7, 0)])),
                ("OFF", Object::Array(vec![rref(8, 0)])),
            ])),
        ),
    ]));
    let ocmd = |extra: Vec<(&'static str, Object)>| -> Object {
        let mut pairs = vec![("Type", name_obj("OCMD"))];
        pairs.extend(extra);
        Object::Dictionary(dict(pairs))
    };
    let both = || Object::Array(vec![rref(7, 0), rref(8, 0)]);
    build_doc(
        ocp,
        vec![
            (7, ocg_dict("A")),
            (8, ocg_dict("B")),
            (30, ocmd(vec![("OCGs", both())])),
            (31, ocmd(vec![("OCGs", both()), ("P", name_obj("AllOn"))])),
            (32, ocmd(vec![("OCGs", both()), ("P", name_obj("AnyOff"))])),
            (33, ocmd(vec![("OCGs", both()), ("P", name_obj("AllOff"))])),
            (34, ocmd(vec![("OCGs", Object::Array(vec![]))])),
            (
                35,
                ocmd(vec![(
                    "VE",
                    Object::Array(vec![name_obj("Not"), rref(8, 0)]),
                )]),
            ),
            (
                36,
                ocmd(vec![(
                    "VE",
                    Object::Array(vec![name_obj("Not"), rref(7, 0)]),
                )]),
            ),
            (
                37,
                ocmd(vec![(
                    "VE",
                    Object::Array(vec![name_obj("And"), rref(7, 0), rref(8, 0)]),
                )]),
            ),
            (
                38,
                ocmd(vec![(
                    "VE",
                    Object::Array(vec![name_obj("Or"), rref(7, 0), rref(8, 0)]),
                )]),
            ),
            (
                39,
                ocmd(vec![(
                    "VE",
                    Object::Array(vec![
                        name_obj("And"),
                        rref(7, 0),
                        Object::Array(vec![name_obj("Not"), rref(8, 0)]),
                    ]),
                )]),
            ),
            (
                40,
                ocmd(vec![
                    ("OCGs", Object::Array(vec![rref(8, 0)])),
                    ("P", name_obj("AllOn")),
                    (
                        "VE",
                        Object::Array(vec![name_obj("Or"), rref(7, 0), rref(8, 0)]),
                    ),
                ]),
            ),
        ],
    )
}

/// OCG-VIS-OCG: an OCG reference is hidden iff that OCG is OFF.
#[test]
fn ocg_vis_ocg() {
    let doc = open(&vis_doc());
    let vis = OcVisibility::read(&doc);
    assert!(vis.hides_anything());
    assert!(!vis.is_hidden(&doc, &rref(7, 0)), "A on -> visible");
    assert!(vis.is_hidden(&doc, &rref(8, 0)), "B off -> hidden");
    assert!(vis.is_ocg_hidden(8));
    assert!(!vis.is_ocg_hidden(7));
}

/// OCG-VIS-UNKNOWN: a reference not declared in `/OCGs` (or dangling) is
/// visible and never panics.
#[test]
fn ocg_vis_unknown() {
    let doc = open(&vis_doc());
    let vis = OcVisibility::read(&doc);
    assert!(!vis.is_hidden(&doc, &rref(3, 0)), "page ref -> visible");
    assert!(
        !vis.is_hidden(&doc, &rref(999, 0)),
        "dangling ref -> visible"
    );
    assert!(!vis.is_ocg_hidden(999));
}

/// OCG-VIS-OCMD-POLICIES: AnyOn / AllOn / AnyOff / AllOff over `[A(on) B(off)]`
/// plus an empty `/OCGs`, evaluated per ISO 32000-1 (spec, not MuPDF's bugs).
#[test]
fn ocg_vis_ocmd_policies() {
    let doc = open(&vis_doc());
    let vis = OcVisibility::read(&doc);
    assert!(
        !vis.is_hidden(&doc, &rref(30, 0)),
        "AnyOn: any on -> visible"
    );
    assert!(
        vis.is_hidden(&doc, &rref(31, 0)),
        "AllOn: not all on -> hidden"
    );
    assert!(
        !vis.is_hidden(&doc, &rref(32, 0)),
        "AnyOff: any off -> visible"
    );
    assert!(
        vis.is_hidden(&doc, &rref(33, 0)),
        "AllOff: not all off -> hidden"
    );
    assert!(!vis.is_hidden(&doc, &rref(34, 0)), "empty /OCGs -> visible");
}

/// OCG-VIS-OCMD-VE: And / Or / Not (nested) over OCG refs, and `/VE` taking
/// precedence over `/OCGs` + `/P`.
#[test]
fn ocg_vis_ocmd_ve() {
    let doc = open(&vis_doc());
    let vis = OcVisibility::read(&doc);
    assert!(
        !vis.is_hidden(&doc, &rref(35, 0)),
        "/VE [Not B], B off -> visible"
    );
    assert!(
        vis.is_hidden(&doc, &rref(36, 0)),
        "/VE [Not A], A on -> hidden"
    );
    assert!(
        vis.is_hidden(&doc, &rref(37, 0)),
        "/VE [And A B], B off -> hidden"
    );
    assert!(
        !vis.is_hidden(&doc, &rref(38, 0)),
        "/VE [Or A B], A on -> visible"
    );
    assert!(
        !vis.is_hidden(&doc, &rref(39, 0)),
        "/VE [And A [Not B]] -> visible"
    );
    assert!(
        !vis.is_hidden(&doc, &rref(40, 0)),
        "/VE wins over /OCGs /AllOn -> visible"
    );
}

/// OCG-VIS-VIEW: a layer-panel override and a switched configuration are both
/// reflected by `OcVisibility` (no document write).
#[test]
fn ocg_vis_view() {
    let doc = open(&config_doc());
    let vis0 = OcVisibility::read(&doc);
    assert!(!vis0.is_ocg_hidden(7), "A on by default");
    assert!(vis0.is_ocg_hidden(8), "B off by default");

    // Override row 0 (OCG 7) OFF via the layer panel.
    set_layer_ui_config(&doc, 0, 2).expect("override A off");
    let vis1 = OcVisibility::read(&doc);
    assert!(vis1.is_ocg_hidden(7), "override took effect");
    assert!(vis1.is_ocg_hidden(8));

    // Switching a config clears overrides and reflects the alternate states.
    select_layer_config(&doc, Some(0)).expect("select alt");
    let vis2 = OcVisibility::read(&doc);
    assert!(vis2.is_ocg_hidden(7), "A off in the alternate config");
    assert!(!vis2.is_ocg_hidden(8), "B on in the alternate config");
    assert!(!doc.is_dirty());
}

// === OCG-VIS-USAGE-* ======================================================
//
// View usage fixtures (ISO 32000-1 §8.11.4.4): OCG 7 ("U") carries the
// `/Usage` under test, OCG 8 ("B") is a plain control, OCMD 30 is
// `/OCGs [7]`. `/D` is built from caller-supplied pairs (`ON` / `OFF` /
// `BaseState` / `AS`); with no `/Order`, `layer_ui_configs` row 0 is OCG 7.

/// `<< /View << /ViewState /<state> >> >>`.
fn view_usage(state: &str) -> Object {
    Object::Dictionary(dict([(
        "View",
        Object::Dictionary(dict([("ViewState", name_obj(state))])),
    )]))
}

/// An OCG "U" with the given `/Usage` dictionary (or reference).
fn ocg_usage(usage: Object) -> Object {
    Object::Dictionary(dict([
        ("Type", name_obj("OCG")),
        ("Name", pdf_str("U")),
        ("Usage", usage),
    ]))
}

/// A usage-application dictionary for `/AS`.
fn as_entry(event: &str, categories: &[&str], ocgs: Vec<Object>) -> Object {
    Object::Dictionary(dict([
        ("Event", name_obj(event)),
        (
            "Category",
            Object::Array(categories.iter().map(|c| name_obj(c)).collect()),
        ),
        ("OCGs", Object::Array(ocgs)),
    ]))
}

/// `/AS [<< /Event /View /Category [/View] /OCGs [7] >>]`.
fn as_view7() -> (&'static str, Object) {
    (
        "AS",
        Object::Array(vec![as_entry("View", &["View"], vec![rref(7, 0)])]),
    )
}

fn on7() -> (&'static str, Object) {
    ("ON", Object::Array(vec![rref(7, 0)]))
}

fn off7() -> (&'static str, Object) {
    ("OFF", Object::Array(vec![rref(7, 0)]))
}

/// A doc with OCG 7 = `ocg7` (under test), OCG 8 = plain "B", `/D` = `d`,
/// optional `/Configs`, OCMD 30 over `[7]`, plus `extra` objects.
fn usage_doc(
    ocg7: Object,
    d: Vec<(&'static str, Object)>,
    configs: Option<Vec<Object>>,
    extra: Vec<(u32, Object)>,
) -> Vec<u8> {
    let mut ocp = vec![
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        ("D", Object::Dictionary(dict(d))),
    ];
    if let Some(configs) = configs {
        ocp.push(("Configs", Object::Array(configs)));
    }
    let mut objects = vec![
        (7, ocg7),
        (8, ocg_dict("B")),
        (
            30,
            Object::Dictionary(dict([
                ("Type", name_obj("OCMD")),
                ("OCGs", Object::Array(vec![rref(7, 0)])),
            ])),
        ),
    ];
    objects.extend(extra);
    build_doc(Object::Dictionary(dict(ocp)), objects)
}

/// Whether OCG 7 is hidden in a freshly opened `bytes`.
fn hidden7(bytes: &[u8]) -> bool {
    OcVisibility::read(&open(bytes)).is_ocg_hidden(7)
}

/// OCG-VIS-USAGE-NONE: an ON OCG without `/Usage` (or with an empty one) is
/// visible — the usage rules add nothing.
#[test]
fn ocg_vis_usage_none() {
    assert!(!hidden7(&usage_doc(
        ocg_dict("U"),
        vec![on7()],
        None,
        vec![]
    )));
    let empty = Object::Dictionary(dict([]));
    assert!(!hidden7(&usage_doc(
        ocg_usage(empty),
        vec![on7()],
        None,
        vec![]
    )));
}

/// OCG-VIS-USAGE-VIEWSTATE-OFF: config ON + `/View << /ViewState /OFF >>`,
/// no `/AS` → hidden (MuPDF parity: `/ViewState /OFF` always hides).
#[test]
fn ocg_vis_usage_viewstate_off() {
    let doc = open(&usage_doc(
        ocg_usage(view_usage("OFF")),
        vec![on7()],
        None,
        vec![],
    ));
    let vis = OcVisibility::read(&doc);
    assert!(vis.hides_anything());
    assert!(vis.is_ocg_hidden(7), "ViewState OFF hides an ON layer");
    assert!(vis.is_hidden(&doc, &rref(7, 0)));
    assert!(!vis.is_ocg_hidden(8), "control layer untouched");
}

/// OCG-VIS-USAGE-VIEWSTATE-OFF-AS: config ON + ViewState OFF, and an `/AS`
/// View entry listing the OCG → still hidden (`/AS` never overrides
/// `/ViewState /OFF`).
#[test]
fn ocg_vis_usage_viewstate_off_as() {
    assert!(hidden7(&usage_doc(
        ocg_usage(view_usage("OFF")),
        vec![on7(), as_view7()],
        None,
        vec![],
    )));
}

/// OCG-VIS-USAGE-VIEWSTATE-OFF-AS-MISS: config ON + ViewState OFF with an
/// `/AS` that does not apply (`/Event /Print` only, or `/OCGs []`) → hidden.
#[test]
fn ocg_vis_usage_viewstate_off_as_miss() {
    let print_only = (
        "AS",
        Object::Array(vec![as_entry("Print", &["Print"], vec![rref(7, 0)])]),
    );
    assert!(hidden7(&usage_doc(
        ocg_usage(view_usage("OFF")),
        vec![on7(), print_only],
        None,
        vec![],
    )));
    let empty_ocgs = (
        "AS",
        Object::Array(vec![as_entry("View", &["View"], vec![])]),
    );
    assert!(hidden7(&usage_doc(
        ocg_usage(view_usage("OFF")),
        vec![on7(), empty_ocgs],
        None,
        vec![],
    )));
}

/// OCG-VIS-USAGE-VIEWSTATE-ON: config ON + `/ViewState /ON` → visible.
#[test]
fn ocg_vis_usage_viewstate_on() {
    assert!(!hidden7(&usage_doc(
        ocg_usage(view_usage("ON")),
        vec![on7()],
        None,
        vec![],
    )));
}

/// OCG-VIS-USAGE-AS-ABSENT: config OFF + `/ViewState /ON` stays hidden when
/// no `/AS` View entry lists the OCG — absent `/AS`, a `/Print` event, a
/// `/View` event whose `/Category` lacks `/View`, or an entry listing only
/// another OCG.
#[test]
fn ocg_vis_usage_as_absent() {
    let ocg = || ocg_usage(view_usage("ON"));
    assert!(hidden7(&usage_doc(ocg(), vec![off7()], None, vec![])));
    let cases = [
        as_entry("Print", &["View"], vec![rref(7, 0)]),
        as_entry("View", &["Print"], vec![rref(7, 0)]),
        as_entry("View", &["View"], vec![rref(8, 0)]),
    ];
    for entry in cases {
        let bytes = usage_doc(
            ocg(),
            vec![off7(), ("AS", Object::Array(vec![entry.clone()]))],
            None,
            vec![],
        );
        assert!(hidden7(&bytes), "no applicable /AS entry: {entry:?}");
    }
}

/// OCG-VIS-USAGE-AS-PROMOTE: config OFF + `/ViewState /ON` + an `/AS` View
/// entry listing the OCG → **visible** (ISO 32000-1 §8.11.4.4).
///
/// Deliberate divergence from MuPDF / PyMuPDF, which ignore `/AS` and keep
/// this OCG hidden; a live-oracle comparison must not assert equality here.
/// The OCMD over the OCG and a `/BaseState /OFF` configuration follow suit.
#[test]
fn ocg_vis_usage_as_promote() {
    let doc = open(&usage_doc(
        ocg_usage(view_usage("ON")),
        vec![off7(), as_view7()],
        None,
        vec![],
    ));
    let vis = OcVisibility::read(&doc);
    assert!(
        !vis.is_ocg_hidden(7),
        "/AS View promotes an OFF layer (spec)"
    );
    assert!(!vis.is_hidden(&doc, &rref(7, 0)));
    assert!(
        !vis.is_hidden(&doc, &rref(30, 0)),
        "OCMD over it is visible too"
    );

    let base_off = ("BaseState", name_obj("OFF"));
    assert!(!hidden7(&usage_doc(
        ocg_usage(view_usage("ON")),
        vec![base_off, as_view7()],
        None,
        vec![],
    )));
}

/// OCG-VIS-USAGE-PRINT-EXPORT: only the View usage counts — `/Print
/// /PrintState /OFF` and `/Export /ExportState /OFF` leave an ON layer
/// visible, and `/PrintState /ON` + an `/AS` Print entry do not promote an
/// OFF layer.
#[test]
fn ocg_vis_usage_print_export() {
    let usage = |cat: &'static str, key: &'static str, state: &str| {
        Object::Dictionary(dict([(
            cat,
            Object::Dictionary(dict([(key, name_obj(state))])),
        )]))
    };
    assert!(!hidden7(&usage_doc(
        ocg_usage(usage("Print", "PrintState", "OFF")),
        vec![on7()],
        None,
        vec![],
    )));
    assert!(!hidden7(&usage_doc(
        ocg_usage(usage("Export", "ExportState", "OFF")),
        vec![on7()],
        None,
        vec![],
    )));
    let as_print = (
        "AS",
        Object::Array(vec![as_entry("Print", &["Print"], vec![rref(7, 0)])]),
    );
    assert!(hidden7(&usage_doc(
        ocg_usage(usage("Print", "PrintState", "ON")),
        vec![off7(), as_print],
        None,
        vec![],
    )));
}

/// OCG-VIS-USAGE-OCMD: `/ViewState /OFF` reached through an OCMD `/OCGs [7]`
/// (and a `/VE [/Not 7]`) is evaluated per OCG; the OCMD inherits it.
#[test]
fn ocg_vis_usage_ocmd() {
    let not7 = Object::Dictionary(dict([
        ("Type", name_obj("OCMD")),
        ("VE", Object::Array(vec![name_obj("Not"), rref(7, 0)])),
    ]));
    let doc = open(&usage_doc(
        ocg_usage(view_usage("OFF")),
        vec![on7()],
        None,
        vec![(31, not7)],
    ));
    let vis = OcVisibility::read(&doc);
    assert!(vis.is_hidden(&doc, &rref(30, 0)), "OCMD /OCGs [7] hidden");
    assert!(!vis.is_hidden(&doc, &rref(31, 0)), "/VE [/Not 7] visible");
}

/// OCG-VIS-USAGE-OVERRIDE: `/ViewState /OFF` beats a layer-panel override
/// that turns the layer ON; an override OFF beats an `/AS` promotion.
#[test]
fn ocg_vis_usage_override() {
    let doc = open(&usage_doc(
        ocg_usage(view_usage("OFF")),
        vec![on7()],
        None,
        vec![],
    ));
    set_layer_ui_config(&doc, 0, 0).expect("override U on");
    assert!(layer_ui_configs(&doc)[0].on, "panel reports the override");
    assert!(
        OcVisibility::read(&doc).is_ocg_hidden(7),
        "ViewState OFF still hides over the override"
    );

    let doc = open(&usage_doc(
        ocg_usage(view_usage("ON")),
        vec![off7(), as_view7()],
        None,
        vec![],
    ));
    assert!(!OcVisibility::read(&doc).is_ocg_hidden(7), "promoted");
    set_layer_ui_config(&doc, 0, 2).expect("override U off");
    assert!(
        OcVisibility::read(&doc).is_ocg_hidden(7),
        "override OFF beats the /AS promotion"
    );
    assert!(!doc.is_dirty());
}

/// OCG-VIS-USAGE-BOGUS: an unrecognised `/ViewState` name, a string instead
/// of a name, an empty `/View <<>>` or a `/Usage` without `/View` all fall
/// through to the configuration state (ON → visible, OFF → hidden).
#[test]
fn ocg_vis_usage_bogus() {
    let bogus: Vec<Object> = vec![
        view_usage("Maybe"),
        Object::Dictionary(dict([(
            "View",
            Object::Dictionary(dict([("ViewState", pdf_str("OFF"))])),
        )])),
        Object::Dictionary(dict([("View", Object::Dictionary(dict([])))])),
        Object::Dictionary(dict([(
            "Print",
            Object::Dictionary(dict([("PrintState", name_obj("OFF"))])),
        )])),
    ];
    for usage in bogus {
        assert!(
            !hidden7(&usage_doc(
                ocg_usage(usage.clone()),
                vec![on7()],
                None,
                vec![]
            )),
            "ON + {usage:?} -> visible"
        );
        assert!(
            hidden7(&usage_doc(
                ocg_usage(usage.clone()),
                vec![off7()],
                None,
                vec![]
            )),
            "OFF + {usage:?} -> hidden"
        );
        // A bogus state never enables the /AS promotion either.
        assert!(
            hidden7(&usage_doc(
                ocg_usage(usage.clone()),
                vec![off7(), as_view7()],
                None,
                vec![]
            )),
            "OFF + /AS + {usage:?} -> hidden (no ViewState ON)"
        );
    }
}

/// OCG-VIS-USAGE-AS-CONFIG: `/AS` is read from the active configuration
/// only — `/D`'s `/AS` does not leak into a selected `/Configs[n]` without
/// one, and an alternate configuration's own `/AS` applies when selected.
#[test]
fn ocg_vis_usage_as_config() {
    let alt_plain = Object::Dictionary(dict([("Name", pdf_str("plain")), off7()]));
    let alt_as = Object::Dictionary(dict([("Name", pdf_str("as")), off7(), as_view7()]));
    let doc = open(&usage_doc(
        ocg_usage(view_usage("ON")),
        vec![off7(), as_view7()],
        Some(vec![alt_plain, alt_as]),
        vec![],
    ));
    assert!(!OcVisibility::read(&doc).is_ocg_hidden(7), "/D promotes");

    select_layer_config(&doc, Some(0)).expect("select plain alt");
    assert!(
        OcVisibility::read(&doc).is_ocg_hidden(7),
        "/D's /AS must not leak into /Configs[0]"
    );

    select_layer_config(&doc, Some(1)).expect("select alt with /AS");
    assert!(
        !OcVisibility::read(&doc).is_ocg_hidden(7),
        "/Configs[1]'s own /AS promotes"
    );

    select_layer_config(&doc, None).expect("back to /D");
    assert!(!OcVisibility::read(&doc).is_ocg_hidden(7));
    assert!(!doc.is_dirty());
}

/// OCG-VIS-USAGE-INDIRECT: `/Usage`, `/View`, `/AS`, its entries, their
/// `/Category` and `/OCGs` may all be indirect; every level is resolved on
/// both the hide (`/ViewState /OFF`) and the promote paths.
#[test]
fn ocg_vis_usage_indirect() {
    // 40 = /Usage dict, 41 = /View dict, 42 = /AS array, 43 = the entry,
    // 44 = its /Category, 45 = its /OCGs.
    let indirect_usage = |state: &str| -> Vec<(u32, Object)> {
        vec![
            (40, Object::Dictionary(dict([("View", rref(41, 0))]))),
            (
                41,
                Object::Dictionary(dict([("ViewState", name_obj(state))])),
            ),
            (42, Object::Array(vec![rref(43, 0)])),
            (
                43,
                Object::Dictionary(dict([
                    ("Event", name_obj("View")),
                    ("Category", rref(44, 0)),
                    ("OCGs", rref(45, 0)),
                ])),
            ),
            (44, Object::Array(vec![name_obj("View")])),
            (45, Object::Array(vec![rref(7, 0)])),
        ]
    };
    assert!(
        hidden7(&usage_doc(
            ocg_usage(rref(40, 0)),
            vec![on7()],
            None,
            indirect_usage("OFF"),
        )),
        "indirect /Usage -> /View -> /ViewState /OFF hides"
    );
    assert!(
        !hidden7(&usage_doc(
            ocg_usage(rref(40, 0)),
            vec![off7(), ("AS", rref(42, 0))],
            None,
            indirect_usage("ON"),
        )),
        "indirect /AS chain promotes"
    );
}

/// OCG-VIS-USAGE-REPORT: `ocg_state` / `get_ocgs` / `layer_ui_configs`
/// reflect only the configuration ON/OFF state — neither `/ViewState /OFF`
/// nor an `/AS` promotion changes what they report (PyMuPDF parity).
#[test]
fn ocg_vis_usage_report() {
    let doc = open(&usage_doc(
        ocg_usage(view_usage("OFF")),
        vec![on7()],
        None,
        vec![],
    ));
    assert!(ocg_state(&doc, 7));
    assert!(get_ocgs(&doc)[&7].on);
    assert!(layer_ui_configs(&doc)[0].on);
    assert!(OcVisibility::read(&doc).is_ocg_hidden(7));

    let doc = open(&usage_doc(
        ocg_usage(view_usage("ON")),
        vec![off7(), as_view7()],
        None,
        vec![],
    ));
    assert!(!ocg_state(&doc, 7));
    assert!(!get_ocgs(&doc)[&7].on);
    assert!(!layer_ui_configs(&doc)[0].on);
    assert!(!OcVisibility::read(&doc).is_ocg_hidden(7));
}
