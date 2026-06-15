//! OCG (layers) write-side e2e tests (PRD §8.x, ISO §8.11).
//!
//! Each test mutates a self-built document, saves (full, garbage=1) and reopens
//! it, then asserts the layer state survives the round-trip via the
//! `pdf_core::ocg` read API and direct `/OCProperties` inspection.

mod common;

use common::{blank_page, dict, name_obj, open, save_reopen};
use pdf_core::object::Name;
use pdf_core::ocg::{get_ocgs, layer_ui_configs, ocg_state};
use pdf_core::{DocumentStore, Object, StreamObj};
use pdf_edit::ocg::{add_ocg, set_layer, set_layer_state, set_oc};

/// The catalog dict of `doc`, resolved through the overlay (reopen-safe).
fn catalog(doc: &DocumentStore) -> pdf_core::Dict {
    let root = doc.root().expect("root");
    doc.resolve(root)
        .expect("catalog")
        .as_dict()
        .cloned()
        .unwrap()
}

/// The `/OCProperties` dict, resolved through any reference.
fn oc_properties(doc: &DocumentStore) -> pdf_core::Dict {
    let cat = catalog(doc);
    let ocp = doc
        .resolve_dict_key(&cat, &Name::new("OCProperties"))
        .expect("resolve")
        .expect("OCProperties present");
    ocp.as_dict().cloned().expect("OCProperties dict")
}

/// The object numbers in a `/D` array sub-key (ON/OFF/Locked/Order leaves).
fn d_array_nums(doc: &DocumentStore, key: &str) -> Vec<u32> {
    let ocp = oc_properties(doc);
    let d = doc
        .resolve_dict_key(&ocp, &Name::new("D"))
        .unwrap()
        .unwrap()
        .as_dict()
        .cloned()
        .unwrap();
    match d.get(&Name::new(key)) {
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(Object::as_reference)
            .map(|r| r.num)
            .collect(),
        _ => Vec::new(),
    }
}

// === OCG-ADD-* ============================================================

/// OCG-ADD-FRESH: add an OCG to a document that has no `/OCProperties`; after
/// save→reopen it is present in `/OCGs` and ON in `/D /ON`.
#[test]
fn ocg_add_fresh() {
    let doc = open(&blank_page(612, 792));
    // No layers initially.
    assert!(get_ocgs(&doc).is_empty());

    let xref = add_ocg(&doc, "Layer 1", true, &[], None).expect("add_ocg");

    let re = save_reopen(&doc);
    let ocgs = get_ocgs(&re);
    assert_eq!(ocgs.len(), 1);
    let info = ocgs.get(&xref.num).expect("ocg present after reopen");
    assert_eq!(info.name, "Layer 1");
    assert!(info.on);
    // It is registered in /D /ON.
    assert!(d_array_nums(&re, "ON").contains(&xref.num));
    assert!(!d_array_nums(&re, "OFF").contains(&xref.num));
    // And in /OCGs (via the read API count).
    assert!(ocg_state(&re, xref.num));
}

/// OCG-ADD-OFF: an OCG added with `on=false` lands in `/D /OFF`.
#[test]
fn ocg_add_off() {
    let doc = open(&blank_page(612, 792));
    let xref = add_ocg(&doc, "Hidden", false, &[], None).expect("add_ocg");
    let re = save_reopen(&doc);
    assert!(!ocg_state(&re, xref.num));
    assert!(d_array_nums(&re, "OFF").contains(&xref.num));
    assert!(!d_array_nums(&re, "ON").contains(&xref.num));
}

/// OCG-ADD-MULTI: adding two OCGs leaves both registered and ordered.
#[test]
fn ocg_add_multi() {
    let doc = open(&blank_page(612, 792));
    let a = add_ocg(&doc, "Alpha", true, &[], None).expect("a");
    let b = add_ocg(&doc, "Beta", false, &[], None).expect("b");

    let re = save_reopen(&doc);
    let ocgs = get_ocgs(&re);
    assert_eq!(ocgs.len(), 2);
    assert_eq!(ocgs[&a.num].name, "Alpha");
    assert_eq!(ocgs[&b.num].name, "Beta");
    assert!(ocgs[&a.num].on);
    assert!(!ocgs[&b.num].on);

    // /Order carries both, in add order.
    let order = d_array_nums(&re, "Order");
    assert_eq!(order, vec![a.num, b.num]);
}

/// OCG-ADD-INTENT: a non-default `/Intent` survives the round-trip.
#[test]
fn ocg_add_intent() {
    let doc = open(&blank_page(612, 792));
    let xref = add_ocg(&doc, "Designy", true, &["Design"], None).expect("add");
    let re = save_reopen(&doc);
    let ocgs = get_ocgs(&re);
    assert_eq!(ocgs[&xref.num].intent, vec!["Design".to_string()]);
}

/// OCG-ADD-CONFIG-LABEL: a `config` label nests the OCG under a `/Order` group,
/// surfacing as a label row in the UI config.
#[test]
fn ocg_add_config_label() {
    let doc = open(&blank_page(612, 792));
    let xref = add_ocg(&doc, "Inside", true, &[], Some("My Group")).expect("add");
    let re = save_reopen(&doc);
    let cfgs = layer_ui_configs(&re);
    assert_eq!(cfgs.len(), 2, "label row + checkbox row");
    assert_eq!(cfgs[0].kind, "label");
    assert_eq!(cfgs[0].text, "My Group");
    assert_eq!(cfgs[0].depth, 0);
    assert_eq!(cfgs[1].kind, "checkbox");
    assert_eq!(cfgs[1].number, xref.num);
    assert_eq!(cfgs[1].depth, 1);
    assert_eq!(cfgs[1].text, "Inside");
}

// === OCG-TOGGLE-* =========================================================

/// OCG-TOGGLE-OFF: toggling an ON layer to off moves it from `/ON` to `/OFF`.
#[test]
fn ocg_toggle_off() {
    let doc = open(&blank_page(612, 792));
    let xref = add_ocg(&doc, "Toggle", true, &[], None).expect("add");
    assert!(ocg_state(&doc, xref.num));

    set_layer_state(&doc, xref.num, false).expect("toggle off");

    let re = save_reopen(&doc);
    assert!(!ocg_state(&re, xref.num));
    assert!(d_array_nums(&re, "OFF").contains(&xref.num));
    assert!(!d_array_nums(&re, "ON").contains(&xref.num));
}

/// OCG-TOGGLE-ON: toggling an OFF layer to on moves it from `/OFF` to `/ON`.
#[test]
fn ocg_toggle_on() {
    let doc = open(&blank_page(612, 792));
    let xref = add_ocg(&doc, "Toggle", false, &[], None).expect("add");
    assert!(!ocg_state(&doc, xref.num));

    set_layer_state(&doc, xref.num, true).expect("toggle on");

    let re = save_reopen(&doc);
    assert!(ocg_state(&re, xref.num));
    assert!(d_array_nums(&re, "ON").contains(&xref.num));
    assert!(!d_array_nums(&re, "OFF").contains(&xref.num));
}

/// OCG-TOGGLE-BULK: `set_layer` bulk-toggles several OCGs at once.
#[test]
fn ocg_toggle_bulk() {
    let doc = open(&blank_page(612, 792));
    let a = add_ocg(&doc, "A", true, &[], None).expect("a");
    let b = add_ocg(&doc, "B", true, &[], None).expect("b");
    let c = add_ocg(&doc, "C", false, &[], None).expect("c");

    // Turn A,B off; turn C on.
    set_layer(&doc, &[c.num], &[a.num, b.num]).expect("bulk");

    let re = save_reopen(&doc);
    assert!(!ocg_state(&re, a.num));
    assert!(!ocg_state(&re, b.num));
    assert!(ocg_state(&re, c.num));
    let on = d_array_nums(&re, "ON");
    let off = d_array_nums(&re, "OFF");
    assert!(on.contains(&c.num));
    assert!(off.contains(&a.num) && off.contains(&b.num));
    // No duplicates introduced.
    assert_eq!(on.iter().filter(|&&x| x == c.num).count(), 1);
}

// === OCG-BIND-* ===========================================================

/// OCG-BIND-XOBJECT: `set_oc` puts an `/OC` reference on an XObject stream
/// reachable from the page's `/Resources /XObject` (so GC keeps it).
#[test]
fn ocg_bind_xobject() {
    let doc = open(&blank_page(612, 792));
    let xref = add_ocg(&doc, "Layer", true, &[], None).expect("add");

    // Create a Form XObject stream and wire it into the page resources.
    let body = b"q Q";
    let xobj = doc
        .add_object(Object::Stream(StreamObj::new_encoded(
            dict([
                ("Type", name_obj("XObject")),
                ("Subtype", name_obj("Form")),
                ("Length", Object::Integer(body.len() as i64)),
                (
                    "BBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Integer(100),
                        Object::Integer(100),
                    ]),
                ),
            ]),
            body.to_vec(),
        )))
        .expect("xobj");
    attach_to_page_resources_xobject(&doc, "Fm0", xobj);

    set_oc(&doc, xobj, xref).expect("set_oc");

    let re = save_reopen(&doc);
    // Find the XObject through the (reopened) page resources.
    let dict = first_xobject_dict(&re);
    let oc = dict.get(&Name::new("OC")).and_then(Object::as_reference);
    assert!(oc.is_some(), "/OC entry present on the bound XObject");
}

/// OCG-BIND-DICT: `set_oc` puts an `/OC` reference on a dictionary object
/// reachable from the page `/Annots` (so GC keeps it).
#[test]
fn ocg_bind_dict() {
    let doc = open(&blank_page(612, 792));
    let xref = add_ocg(&doc, "Layer", true, &[], None).expect("add");
    let target = doc
        .add_object(Object::Dictionary(dict([
            ("Type", name_obj("Annot")),
            ("Subtype", name_obj("Square")),
        ])))
        .expect("target");
    attach_to_page_annots(&doc, target);

    set_oc(&doc, target, xref).expect("set_oc");

    let _ = xref;
    let re = save_reopen(&doc);
    let annot = first_annot_dict(&re);
    let oc = annot
        .get(&Name::new("OC"))
        .and_then(Object::as_reference)
        .expect("/OC present on the bound annotation");
    // The /OC reference resolves to an OCG dictionary.
    let oc_obj = re.resolve(oc).expect("resolve /OC");
    let oc_dict = oc_obj.as_dict().expect("OCG dict");
    assert_eq!(
        oc_dict.get(&Name::new("Type")).and_then(Object::as_name),
        Some(&Name::new("OCG"))
    );
}

// --- bind-test graph helpers ---------------------------------------------

/// Adds `target` to the page-0 leaf's `/Resources /XObject` under `name`.
fn attach_to_page_resources_xobject(doc: &DocumentStore, name: &str, target: pdf_core::ObjRef) {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let mut page = doc.resolve(leaf).unwrap().as_dict().cloned().unwrap();
    let mut res = match page.get(&Name::new("Resources")) {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => pdf_core::Dict::new(),
    };
    let mut xo = match res.get(&Name::new("XObject")) {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => pdf_core::Dict::new(),
    };
    xo.insert(Name::new(name), Object::Reference(target));
    res.insert(Name::new("XObject"), Object::Dictionary(xo));
    page.insert(Name::new("Resources"), Object::Dictionary(res));
    doc.update_object(leaf, Object::Dictionary(page)).unwrap();
}

/// Appends `target` to the page-0 leaf's `/Annots` array.
fn attach_to_page_annots(doc: &DocumentStore, target: pdf_core::ObjRef) {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let mut page = doc.resolve(leaf).unwrap().as_dict().cloned().unwrap();
    let mut annots = match page.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    annots.push(Object::Reference(target));
    page.insert(Name::new("Annots"), Object::Array(annots));
    doc.update_object(leaf, Object::Dictionary(page)).unwrap();
}

/// The first XObject dictionary reachable from page-0 resources.
fn first_xobject_dict(doc: &DocumentStore) -> pdf_core::Dict {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let page = doc.resolve(leaf).unwrap().as_dict().cloned().unwrap();
    let res = doc
        .resolve_dict_key(&page, &Name::new("Resources"))
        .unwrap()
        .unwrap();
    let res = res.as_dict().unwrap();
    let xo = doc
        .resolve_dict_key(res, &Name::new("XObject"))
        .unwrap()
        .unwrap();
    let xo = xo.as_dict().unwrap();
    let first = xo.values().next().and_then(Object::as_reference).unwrap();
    doc.resolve(first).unwrap().as_dict().cloned().unwrap()
}

/// The first annotation dictionary on page 0.
fn first_annot_dict(doc: &DocumentStore) -> pdf_core::Dict {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let page = doc.resolve(leaf).unwrap().as_dict().cloned().unwrap();
    let annots = doc
        .resolve_dict_key(&page, &Name::new("Annots"))
        .unwrap()
        .unwrap();
    let arr = annots.as_array().unwrap();
    let first = arr.first().and_then(Object::as_reference).unwrap();
    doc.resolve(first).unwrap().as_dict().cloned().unwrap()
}

// === robustness ===========================================================

/// set_layer on a doc without /OCProperties is a harmless no-op.
#[test]
fn ocg_set_layer_no_layers_noop() {
    let doc = open(&blank_page(612, 792));
    set_layer(&doc, &[1], &[2]).expect("noop");
    let re = save_reopen(&doc);
    assert!(get_ocgs(&re).is_empty());
}

/// Adding to a document that already has an indirect `/OCProperties` updates the
/// existing object in place rather than orphaning it.
#[test]
fn ocg_add_existing_ocproperties() {
    let doc = open(&blank_page(612, 792));
    let first = add_ocg(&doc, "First", true, &[], None).expect("first");
    // Second add must extend the same /OCProperties.
    let second = add_ocg(&doc, "Second", true, &[], None).expect("second");
    let re = save_reopen(&doc);
    let ocgs = get_ocgs(&re);
    assert_eq!(ocgs.len(), 2);
    assert!(ocgs.contains_key(&first.num));
    assert!(ocgs.contains_key(&second.num));
}
