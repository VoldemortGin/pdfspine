//! Optional Content Groups (layers) — write side (ISO 32000-1 §8.11, PRD §8.x).
//!
//! These operations create and toggle OCGs and bind objects to them, mutating
//! the catalog `/OCProperties` through the [`DocumentStore`] ChangeSet so a full
//! save → reopen reflects them:
//!
//! - [`add_ocg`] creates an OCG dictionary, registers it in
//!   `/OCProperties /OCGs`, appends it to `/D /Order`, and records its initial
//!   state in `/D /ON` or `/D /OFF`. It creates `/OCProperties` (with an empty
//!   `/D`) when the document has none.
//! - [`set_layer_state`] moves an OCG between `/D /ON` and `/D /OFF`.
//! - [`set_oc`] binds a target object (an XObject, annotation, …) to an OCG by
//!   setting its `/OC` entry.
//! - [`set_layer`] is a bulk ON/OFF toggle over several OCGs at once.

use pdf_core::object::Name;
use pdf_core::{Dict, DocumentStore, ObjRef, Object, PdfString, StringKind};

/// Creates a new Optional Content Group named `name` and registers it in the
/// catalog `/OCProperties` (PyMuPDF `add_ocg()`).
///
/// `on` sets its initial visibility in the default configuration `/D`. `intent`
/// is the `/Intent` (one or more names, e.g. `["View"]` / `["Design"]`); an
/// empty slice omits `/Intent` (the viewer then defaults to `/View`). `config`,
/// when given, is a UI label used as the leading string of a nested `/Order`
/// group so the layer panel can show the OCG under a heading.
///
/// Returns the new OCG's object reference. `/OCProperties` (and an empty `/D`)
/// are created when absent.
///
/// # Errors
///
/// [`pdf_core::Error::InvalidArgument`] when the document has no `/Root` or the
/// catalog is not a dictionary; propagates object-edit errors.
pub fn add_ocg(
    doc: &DocumentStore,
    name: &str,
    on: bool,
    intent: &[&str],
    config: Option<&str>,
) -> pdf_core::Result<ObjRef> {
    let root = doc
        .root()
        .ok_or(pdf_core::Error::InvalidArgument("document has no /Root"))?;
    let mut catalog = catalog_dict(doc)?;

    // Build the OCG dictionary: /Type /OCG, /Name, optional /Intent.
    let mut ocg = Dict::new();
    ocg.insert(Name::new("Type"), Object::Name(Name::new("OCG")));
    ocg.insert(Name::new("Name"), Object::String(encode_text(name)));
    match intent.len() {
        0 => {}
        1 => {
            ocg.insert(Name::new("Intent"), Object::Name(Name::new(intent[0])));
        }
        _ => {
            let arr = intent.iter().map(|s| Object::Name(Name::new(*s))).collect();
            ocg.insert(Name::new("Intent"), Object::Array(arr));
        }
    }
    let ocg_ref = doc.add_object(Object::Dictionary(ocg))?;

    // Load (or create) /OCProperties.
    let mut ocp = oc_properties_dict(doc, &catalog)?;

    // Append to /OCGs.
    let mut ocgs = array_value(doc, &ocp, "OCGs");
    ocgs.push(Object::Reference(ocg_ref));
    ocp.insert(Name::new("OCGs"), Object::Array(ocgs));

    // Load (or create) the default config /D and update it.
    let mut d = config_d_dict(doc, &ocp);

    // /ON or /OFF.
    let state_key = if on { "ON" } else { "OFF" };
    let mut state = array_value_local(&d, state_key);
    state.push(Object::Reference(ocg_ref));
    d.insert(Name::new(state_key), Object::Array(state));

    // /Order: append the OCG (optionally under a label group).
    let mut order = array_value_local(&d, "Order");
    match config {
        Some(label) => {
            order.push(Object::Array(vec![
                Object::String(encode_text(label)),
                Object::Reference(ocg_ref),
            ]));
        }
        None => order.push(Object::Reference(ocg_ref)),
    }
    d.insert(Name::new("Order"), Object::Array(order));

    ocp.insert(Name::new("D"), Object::Dictionary(d));

    // Write /OCProperties back, creating the catalog entry if needed.
    write_oc_properties(doc, &mut catalog, ocp)?;
    doc.update_object(root, Object::Dictionary(catalog))?;
    Ok(ocg_ref)
}

/// Toggles a single OCG between `/D /ON` and `/D /OFF` (PyMuPDF
/// `set_layer(.., on=[..]/off=[..])` for one OCG).
///
/// # Errors
///
/// As [`add_ocg`]; a no-op (still `Ok`) on a document without `/OCProperties`.
pub fn set_layer_state(doc: &DocumentStore, xref: u32, on: bool) -> pdf_core::Result<()> {
    if on {
        set_layer(doc, &[xref], &[])
    } else {
        set_layer(doc, &[], &[xref])
    }
}

/// Bulk ON/OFF toggle of OCGs in the default configuration `/D` (PyMuPDF
/// `set_layer(config=-1, on=[..], off=[..])`). An OCG appearing in `off` is
/// removed from `/ON` and added to `/OFF` (and vice-versa); duplicates are
/// collapsed.
///
/// # Errors
///
/// As [`add_ocg`]. A document with no `/OCProperties` is left unchanged.
pub fn set_layer(doc: &DocumentStore, on: &[u32], off: &[u32]) -> pdf_core::Result<()> {
    let root = doc
        .root()
        .ok_or(pdf_core::Error::InvalidArgument("document has no /Root"))?;
    let mut catalog = catalog_dict(doc)?;
    let Some(mut ocp) = existing_oc_properties(doc, &catalog) else {
        return Ok(()); // nothing to toggle
    };
    let mut d = config_d_dict(doc, &ocp);

    let mut on_set: Vec<u32> = array_value_local(&d, "ON")
        .iter()
        .filter_map(Object::as_reference)
        .map(|r| r.num)
        .collect();
    let mut off_set: Vec<u32> = array_value_local(&d, "OFF")
        .iter()
        .filter_map(Object::as_reference)
        .map(|r| r.num)
        .collect();

    for &num in on {
        off_set.retain(|&x| x != num);
        if !on_set.contains(&num) {
            on_set.push(num);
        }
    }
    for &num in off {
        on_set.retain(|&x| x != num);
        if !off_set.contains(&num) {
            off_set.push(num);
        }
    }

    d.insert(Name::new("ON"), refs_array(&on_set));
    d.insert(Name::new("OFF"), refs_array(&off_set));
    ocp.insert(Name::new("D"), Object::Dictionary(d));

    write_oc_properties(doc, &mut catalog, ocp)?;
    doc.update_object(root, Object::Dictionary(catalog))?;
    Ok(())
}

/// Binds the object `target` to the OCG `ocg` by setting its `/OC` entry
/// (PyMuPDF `set_oc()`). The target must be a dictionary or stream object (an
/// XObject, an annotation, …).
///
/// # Errors
///
/// [`pdf_core::Error::InvalidArgument`] if `target` is not a dictionary/stream;
/// propagates object-edit errors.
pub fn set_oc(doc: &DocumentStore, target: ObjRef, ocg: ObjRef) -> pdf_core::Result<()> {
    let obj = doc.resolve(target)?;
    match obj.as_ref() {
        Object::Stream(s) => {
            let mut stream = s.clone();
            stream.dict.insert(Name::new("OC"), Object::Reference(ocg));
            doc.update_object(target, Object::Stream(stream))
        }
        Object::Dictionary(d) => {
            let mut dict = d.clone();
            dict.insert(Name::new("OC"), Object::Reference(ocg));
            doc.update_object(target, Object::Dictionary(dict))
        }
        _ => Err(pdf_core::Error::InvalidArgument(
            "/OC target is not a dictionary or stream",
        )),
    }
}

// --- internal helpers -----------------------------------------------------

/// The catalog dictionary (cloned), errored when missing / not a dict.
fn catalog_dict(doc: &DocumentStore) -> pdf_core::Result<Dict> {
    let root = doc
        .root()
        .ok_or(pdf_core::Error::InvalidArgument("document has no /Root"))?;
    doc.resolve(root)?
        .as_dict()
        .cloned()
        .ok_or(pdf_core::Error::InvalidArgument(
            "/Root is not a dictionary",
        ))
}

/// The existing `/OCProperties` dict, resolved through any reference, or `None`.
fn existing_oc_properties(doc: &DocumentStore, catalog: &Dict) -> Option<Dict> {
    let ocp = doc
        .resolve_dict_key(catalog, &Name::new("OCProperties"))
        .ok()
        .flatten()?;
    ocp.as_dict().cloned()
}

/// The `/OCProperties` dict (cloned), creating an empty one (with `/OCGs` []
/// and an empty `/D`) when absent.
fn oc_properties_dict(doc: &DocumentStore, catalog: &Dict) -> pdf_core::Result<Dict> {
    if let Some(d) = existing_oc_properties(doc, catalog) {
        return Ok(d);
    }
    let mut ocp = Dict::new();
    ocp.insert(Name::new("OCGs"), Object::Array(Vec::new()));
    ocp.insert(Name::new("D"), Object::Dictionary(Dict::new()));
    Ok(ocp)
}

/// The default config `/D` dict (cloned), resolved through a reference; an empty
/// dict when absent.
fn config_d_dict(doc: &DocumentStore, ocp: &Dict) -> Dict {
    doc.resolve_dict_key(ocp, &Name::new("D"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .unwrap_or_default()
}

/// Writes `ocp` back to the catalog: if `/OCProperties` was an indirect
/// reference, update that object in place; otherwise store it as a fresh
/// indirect object and point the catalog at it (so the dict is not inlined and
/// `/OCGs` references stay valid).
fn write_oc_properties(doc: &DocumentStore, catalog: &mut Dict, ocp: Dict) -> pdf_core::Result<()> {
    match catalog.get(&Name::new("OCProperties")) {
        Some(Object::Reference(r)) => {
            let r = *r;
            doc.update_object(r, Object::Dictionary(ocp))?;
        }
        _ => {
            let r = doc.add_object(Object::Dictionary(ocp))?;
            catalog.insert(Name::new("OCProperties"), Object::Reference(r));
        }
    }
    Ok(())
}

/// Resolves an array-valued key on a dict via the document (following a
/// reference), returning a fresh owned `Vec` (empty when absent / wrong type).
fn array_value(doc: &DocumentStore, d: &Dict, key: &str) -> Vec<Object> {
    doc.resolve_dict_key(d, &Name::new(key))
        .ok()
        .flatten()
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default()
}

/// A direct array-valued key on a dict (no reference resolution), as an owned
/// `Vec` (empty when absent / wrong type). Used for `/D` sub-arrays, which the
/// writer keeps inline.
fn array_value_local(d: &Dict, key: &str) -> Vec<Object> {
    match d.get(&Name::new(key)) {
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

/// An array of indirect references from object numbers (gen 0).
fn refs_array(nums: &[u32]) -> Object {
    Object::Array(
        nums.iter()
            .map(|&num| Object::Reference(ObjRef::new(num, 0)))
            .collect(),
    )
}

/// Encodes a layer name / label as a PDF text string (ASCII literal, else
/// UTF-16BE hex), mirroring `toc::encode_text`.
fn encode_text(s: &str) -> PdfString {
    if s.is_ascii() {
        PdfString {
            bytes: s.as_bytes().to_vec(),
            kind: StringKind::Literal,
        }
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        PdfString {
            bytes,
            kind: StringKind::Hex,
        }
    }
}
