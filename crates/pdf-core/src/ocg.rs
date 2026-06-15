//! Optional Content Groups (layers) — read side (ISO 32000-1 §8.11, PRD §8.x).
//!
//! A layered PDF declares its optional content in the catalog's `/OCProperties`
//! dictionary, which has two parts:
//!
//! - `/OCGs`: an array of indirect references to every Optional Content Group
//!   (OCG) dictionary in the document. Each OCG has a `/Name`, an optional
//!   `/Intent` (a name or array of names, default `/View`) and an optional
//!   `/Usage` dictionary.
//! - `/D`: the **default viewing configuration** (`/Type /OCConfig`). Its
//!   `/ON` / `/OFF` arrays list the OCGs that are initially visible / hidden,
//!   `/Locked` lists the ones the UI must not let the user toggle, `/Order`
//!   gives the (possibly nested) presentation tree shown in a layer panel, and
//!   `/BaseState` (`/ON` default, or `/OFF`) decides the visibility of any OCG
//!   not named in `/ON` or `/OFF`.
//!
//! This module parses all of that into plain value types. A non-layered PDF
//! (no `/OCProperties`) yields empty results and never panics (PRD robustness).

use std::collections::BTreeMap;

use crate::object::Name;
use crate::{Dict, DocumentStore, Object};

/// One Optional Content Group, as read from `/OCProperties`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct OcgInfo {
    /// The human-readable layer name (`/Name`). Empty if absent.
    pub name: String,
    /// The `/Intent` names (default `["View"]` when absent).
    pub intent: Vec<String>,
    /// Whether the layer is ON in the default configuration `/D`.
    pub on: bool,
    /// Whether the layer is locked in `/D /Locked` (UI must not toggle it).
    pub locked: bool,
}

/// One row of the layer-panel UI, mirroring PyMuPDF `layer_ui_configs()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerUiConfig {
    /// The OCG object number (xref).
    pub number: u32,
    /// The display text (the OCG `/Name`, or a label string for a nested group).
    pub text: String,
    /// Nesting depth in `/Order` (0 for a top-level entry).
    pub depth: i32,
    /// The entry kind: `"label"` for a nesting label string, else `"checkbox"`.
    pub kind: &'static str,
    /// Whether the layer is ON.
    pub on: bool,
    /// Whether the layer is locked.
    pub locked: bool,
}

/// Reads every OCG declared in the catalog `/OCProperties /OCGs`, keyed by the
/// OCG object number, resolving its name/intent and ON/locked state from the
/// default configuration `/D` (PyMuPDF `get_ocgs()`).
///
/// A non-layered document yields an empty map.
#[must_use]
pub fn get_ocgs(doc: &DocumentStore) -> BTreeMap<u32, OcgInfo> {
    let mut out = BTreeMap::new();
    let Some(ocp) = oc_properties(doc) else {
        return out;
    };
    let ocg_nums = ocg_object_numbers(doc, &ocp);
    if ocg_nums.is_empty() {
        return out;
    }
    let cfg = DefaultConfig::read(doc, &ocp);
    for num in ocg_nums {
        let info = read_ocg(doc, num, &cfg);
        out.insert(num, info);
    }
    out
}

/// The ON/OFF state of a single OCG in the default configuration `/D` (PyMuPDF
/// per-layer state lookup). Returns `false` for an unknown OCG / non-layered
/// document.
#[must_use]
pub fn ocg_state(doc: &DocumentStore, xref: u32) -> bool {
    let Some(ocp) = oc_properties(doc) else {
        return false;
    };
    let cfg = DefaultConfig::read(doc, &ocp);
    cfg.is_on(xref)
}

/// The layer-panel UI configuration list (PyMuPDF `layer_ui_configs()`),
/// flattening `/D /Order` into depth-tagged rows. When `/Order` is absent, the
/// rows follow `/OCGs` order at depth 0.
#[must_use]
pub fn layer_ui_configs(doc: &DocumentStore) -> Vec<LayerUiConfig> {
    let mut out = Vec::new();
    let Some(ocp) = oc_properties(doc) else {
        return out;
    };
    let ocg_nums = ocg_object_numbers(doc, &ocp);
    if ocg_nums.is_empty() {
        return out;
    }
    let cfg = DefaultConfig::read(doc, &ocp);

    // Prefer `/Order`: it gives both nesting and the panel order.
    if let Some(order) = cfg.order.as_ref() {
        walk_order(doc, order, 0, &cfg, &mut out);
        if !out.is_empty() {
            return out;
        }
    }

    // Fallback: a flat list in `/OCGs` order.
    for num in ocg_nums {
        let info = read_ocg(doc, num, &cfg);
        out.push(LayerUiConfig {
            number: num,
            text: info.name,
            depth: 0,
            kind: "checkbox",
            on: info.on,
            locked: info.locked,
        });
    }
    out
}

// --- internal helpers -----------------------------------------------------

/// The catalog `/OCProperties` dictionary, resolved through any reference.
fn oc_properties(doc: &DocumentStore) -> Option<Dict> {
    let root = doc.root()?;
    let catalog = doc.resolve(root).ok()?;
    let cat = catalog.as_dict()?;
    let ocp = doc
        .resolve_dict_key(cat, &Name::new("OCProperties"))
        .ok()??;
    ocp.as_dict().cloned()
}

/// The object numbers of every OCG in `/OCProperties /OCGs` (in array order).
fn ocg_object_numbers(doc: &DocumentStore, ocp: &Dict) -> Vec<u32> {
    let Ok(Some(arr)) = doc.resolve_dict_key(ocp, &Name::new("OCGs")) else {
        return Vec::new();
    };
    let Some(items) = arr.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(Object::as_reference)
        .map(|r| r.num)
        .collect()
}

/// Reads a single OCG dict (`/Name`, `/Intent`) and resolves its ON/locked
/// state from `cfg`.
fn read_ocg(doc: &DocumentStore, num: u32, cfg: &DefaultConfig) -> OcgInfo {
    let Ok(obj) = doc.get_object(num, 0) else {
        return OcgInfo::default();
    };
    let Some(d) = obj.as_dict() else {
        return OcgInfo::default();
    };
    let name = d
        .get(&Name::new("Name"))
        .and_then(Object::as_string)
        .map(|s| decode_text(s.as_bytes()))
        .unwrap_or_default();
    let intent = read_intent(d);
    OcgInfo {
        name,
        intent,
        on: cfg.is_on(num),
        locked: cfg.locked.contains(&num),
    }
}

/// Reads `/Intent` — a single name or an array of names — defaulting to
/// `["View"]` when absent (ISO 32000-1 §8.11.2).
fn read_intent(d: &Dict) -> Vec<String> {
    match d.get(&Name::new("Intent")) {
        Some(Object::Name(n)) => vec![name_string(n)],
        Some(Object::Array(items)) => {
            let v: Vec<String> = items
                .iter()
                .filter_map(Object::as_name)
                .map(name_string)
                .collect();
            if v.is_empty() {
                vec!["View".to_string()]
            } else {
                v
            }
        }
        _ => vec!["View".to_string()],
    }
}

/// Flattens `/D /Order` into depth-tagged UI rows. An `/Order` array entry is
/// either an OCG reference (a checkbox row) or a nested array whose optional
/// leading string is a non-toggling label for the entries that follow.
fn walk_order(
    doc: &DocumentStore,
    order: &[Object],
    depth: i32,
    cfg: &DefaultConfig,
    out: &mut Vec<LayerUiConfig>,
) {
    if depth > 64 {
        return; // defensive nesting cap
    }
    let mut i = 0;
    while i < order.len() {
        match &order[i] {
            Object::Reference(r) => {
                let info = read_ocg(doc, r.num, cfg);
                out.push(LayerUiConfig {
                    number: r.num,
                    text: info.name,
                    depth,
                    kind: "checkbox",
                    on: info.on,
                    locked: info.locked,
                });
            }
            Object::Array(nested) => {
                // A leading string is the group's label (a non-toggle row).
                let mut start = 0;
                if let Some(Object::String(s)) = nested.first() {
                    out.push(LayerUiConfig {
                        number: 0,
                        text: decode_text(s.as_bytes()),
                        depth,
                        kind: "label",
                        on: false,
                        locked: false,
                    });
                    start = 1;
                }
                walk_order(doc, &nested[start..], depth + 1, cfg, out);
            }
            _ => {}
        }
        i += 1;
    }
}

/// The parsed default configuration `/D` — the ON/OFF/Locked sets, the base
/// state and the raw `/Order` array (resolved one level).
struct DefaultConfig {
    on: Vec<u32>,
    off: Vec<u32>,
    locked: Vec<u32>,
    /// `true` when `/BaseState` is `/OFF` (default is `/ON`).
    base_off: bool,
    order: Option<Vec<Object>>,
}

impl DefaultConfig {
    /// Reads `/OCProperties /D` (the default `/OCConfig`). A missing `/D` yields
    /// an all-ON base state with empty sets.
    fn read(doc: &DocumentStore, ocp: &Dict) -> Self {
        let d = doc
            .resolve_dict_key(ocp, &Name::new("D"))
            .ok()
            .flatten()
            .and_then(|o| o.as_dict().cloned())
            .unwrap_or_default();
        let on = ref_nums(doc, &d, "ON");
        let off = ref_nums(doc, &d, "OFF");
        let locked = ref_nums(doc, &d, "Locked");
        let base_off = matches!(
            d.get(&Name::new("BaseState")),
            Some(Object::Name(n)) if n.as_bytes() == b"OFF"
        );
        let order = doc
            .resolve_dict_key(&d, &Name::new("Order"))
            .ok()
            .flatten()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec));
        DefaultConfig {
            on,
            off,
            locked,
            base_off,
            order,
        }
    }

    /// Whether OCG `num` is visible in this configuration: `/ON` wins, then
    /// `/OFF`, otherwise the `/BaseState` default.
    fn is_on(&self, num: u32) -> bool {
        if self.on.contains(&num) {
            return true;
        }
        if self.off.contains(&num) {
            return false;
        }
        !self.base_off
    }
}

/// Resolves a `/D` array key (`ON`/`OFF`/`Locked`) into a list of OCG object
/// numbers.
fn ref_nums(doc: &DocumentStore, d: &Dict, key: &str) -> Vec<u32> {
    let Ok(Some(arr)) = doc.resolve_dict_key(d, &Name::new(key)) else {
        return Vec::new();
    };
    let Some(items) = arr.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(Object::as_reference)
        .map(|r| r.num)
        .collect()
}

/// A `/Name`'s value as a UTF-8 string (lossy for non-UTF-8 names).
fn name_string(n: &Name) -> String {
    String::from_utf8_lossy(n.as_bytes()).into_owned()
}

/// Decodes a PDF text string: UTF-16BE when it carries the BOM, else PDFDoc /
/// ASCII (mirrors `toc::decode_text`).
fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}
