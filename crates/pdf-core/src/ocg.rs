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
//! Beyond `/D`, `/Configs` may hold alternate configurations (`/OCConfig`
//! dictionaries with their own `/Name`, `/Creator`, `/BaseState`, `/ON`,
//! `/OFF`, `/Order`). Following MuPDF, the entries of `/Configs` are addressed
//! by number ([`get_layers`], [`layer_config_count`]) while `/D` is the
//! default (number `None`). The store keeps an in-memory [`LayerView`] (the
//! selected configuration + layer-panel overrides, [`select_layer_config`] /
//! [`set_layer_ui_config`]) that
//! [`OcVisibility`] turns into the hidden-OCG set the content interpreter
//! consults for `/OC` XObjects and `BDC /OC` marked content — including
//! Optional Content Membership Dictionaries (`/OCMD` with `/OCGs` + `/P`, or a
//! `/VE` visibility expression; [`get_ocmd`]).
//!
//! This module parses all of that into plain value types. A non-layered PDF
//! (no `/OCProperties`) yields empty results and never panics (PRD robustness).

use std::collections::{BTreeMap, HashSet};

use crate::object::Name;
use crate::{Dict, DocumentStore, Error, Object, Result};

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
    /// The row index in the panel list (PyMuPDF `number`; the argument of
    /// [`set_layer_ui_config`]).
    pub number: usize,
    /// The OCG object number (xref); 0 for a label row.
    pub ocg: u32,
    /// The display text (the OCG `/Name`, or a label string for a nested group).
    pub text: String,
    /// Nesting depth in `/Order` (0 for a top-level entry).
    pub depth: i32,
    /// The entry kind: `"label"` for a nesting label string, `"radiobox"` for
    /// an OCG that belongs to an `/RBGroups` group, else `"checkbox"`.
    pub kind: &'static str,
    /// Whether the layer is ON.
    pub on: bool,
    /// Whether the layer is locked.
    pub locked: bool,
}

/// The in-memory optional-content view (the analogue of MuPDF's layer
/// descriptor): which layer configuration is selected and the layer-panel
/// overrides applied on top of it. Rendering, text extraction and the layer
/// readers consult it; a save does not persist it (PyMuPDF
/// `switch_layer(as_default=False)` / `set_layer_ui_config`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerView {
    /// The selected configuration: `None` for the default `/D`, `Some(n)` for
    /// `/Configs[n]` (the numbering of [`get_layers`]).
    pub config: Option<usize>,
    /// Per-OCG ON (`true`) / OFF (`false`) overrides set from the layer panel.
    pub overrides: BTreeMap<u32, bool>,
}

/// One layer configuration, as listed by PyMuPDF `get_layers()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerConfig {
    /// The configuration number (the argument of `switch_layer`).
    pub number: usize,
    /// The configuration `/Name`, if any.
    pub name: Option<String>,
    /// The configuration `/Creator`, if any.
    pub creator: Option<String>,
}

/// A visibility expression (`/VE`, ISO 32000-1 §8.11.2.2): a nested
/// `/And` / `/Or` / `/Not` tree over OCG object numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VeExpr {
    /// A reference to an OCG (its object number).
    Ocg(u32),
    /// An operator node: `op` is `"And"`, `"Or"` or `"Not"`.
    Op {
        /// The operator name (`And` / `Or` / `Not`).
        op: String,
        /// The operands (OCGs or nested expressions).
        args: Vec<VeExpr>,
    },
}

/// An Optional Content Membership Dictionary (`/Type /OCMD`), as read by
/// [`get_ocmd`] (PyMuPDF `get_ocmd()`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcmdInfo {
    /// The OCMD object number.
    pub xref: u32,
    /// The OCG object numbers of `/OCGs` (a single reference or an array),
    /// `None` when the entry is absent.
    pub ocgs: Option<Vec<u32>>,
    /// The `/P` visibility policy (`AnyOn` / `AllOn` / `AnyOff` / `AllOff`),
    /// if present.
    pub policy: Option<String>,
    /// The `/VE` visibility expression, if present.
    pub ve: Option<VeExpr>,
}

/// Reads every OCG declared in the catalog `/OCProperties /OCGs`, keyed by the
/// OCG object number, resolving its name/intent and ON/locked state from the
/// active configuration (the store's [`LayerView`]; `/D` unless a
/// configuration was switched) (PyMuPDF `get_ocgs()`).
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
    let cfg = OcConfig::active(doc, &ocp);
    for num in ocg_nums {
        let info = read_ocg(doc, num, &cfg);
        out.insert(num, info);
    }
    out
}

/// The ON/OFF state of a single OCG in the active configuration (PyMuPDF
/// per-layer state lookup). Returns `false` for an unknown OCG / non-layered
/// document.
#[must_use]
pub fn ocg_state(doc: &DocumentStore, xref: u32) -> bool {
    let Some(ocp) = oc_properties(doc) else {
        return false;
    };
    let cfg = OcConfig::active(doc, &ocp);
    cfg.is_on(xref)
}

/// The layer-panel UI configuration list (PyMuPDF `layer_ui_configs()`),
/// flattening the active configuration's `/Order` into depth-tagged rows. When
/// `/Order` is absent, the rows follow `/OCGs` order at depth 0.
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
    let cfg = OcConfig::active(doc, &ocp);

    // Prefer `/Order`: it gives both nesting and the panel order.
    if let Some(order) = cfg.order.as_ref() {
        walk_order(doc, order, 0, &cfg, &mut out);
    }

    // Fallback: a flat list in `/OCGs` order.
    if out.is_empty() {
        for num in ocg_nums {
            let info = read_ocg(doc, num, &cfg);
            out.push(LayerUiConfig {
                number: 0,
                ocg: num,
                text: info.name,
                depth: 0,
                kind: cfg.ui_kind(num),
                on: info.on,
                locked: info.locked,
            });
        }
    }
    for (i, row) in out.iter_mut().enumerate() {
        row.number = i;
    }
    out
}

/// The number of alternate layer configurations (MuPDF
/// `pdf_count_layer_configs`): the length of `/OCProperties /Configs` when it
/// is an array, else 0. The default `/D` is never counted.
#[must_use]
pub fn layer_config_count(doc: &DocumentStore) -> usize {
    let Some(ocp) = oc_properties(doc) else {
        return 0;
    };
    match doc.resolve_dict_key(&ocp, &Name::new("Configs")) {
        Ok(Some(obj)) => obj.as_array().map_or(0, <[Object]>::len),
        _ => 0,
    }
}

/// Lists the alternate layer configurations (PyMuPDF `get_layers()`): one
/// entry per `/Configs` element, numbered by array index, with its `/Name` and
/// `/Creator`. The default `/D` is not listed; a non-layered document yields
/// an empty list.
#[must_use]
pub fn get_layers(doc: &DocumentStore) -> Vec<LayerConfig> {
    let Some(ocp) = oc_properties(doc) else {
        return Vec::new();
    };
    (0..layer_config_count(doc))
        .map(|number| {
            let d = config_dict(doc, &ocp, Some(number)).unwrap_or_default();
            LayerConfig {
                number,
                name: text_entry(&d, "Name"),
                creator: text_entry(&d, "Creator"),
            }
        })
        .collect()
}

/// Selects the layer configuration the in-memory view uses (MuPDF
/// `pdf_select_layer_config`; PyMuPDF `switch_layer` without `as_default`):
/// `None` for the default `/D`, `Some(n)` for `/Configs[n]`. Layer-panel
/// overrides are discarded. Nothing is written to the document.
///
/// # Errors
///
/// [`Error::InvalidArgument`] when `Some(n)` does not name a `/Configs` entry.
pub fn select_layer_config(doc: &DocumentStore, number: Option<usize>) -> Result<()> {
    if let Some(n) = number {
        if n >= layer_config_count(doc) {
            return Err(Error::InvalidArgument("Illegal Layer config"));
        }
    }
    doc.set_layer_view(LayerView {
        config: number,
        overrides: BTreeMap::new(),
    });
    Ok(())
}

/// Sets (`action` 0), toggles (1) or clears (2) the layer-panel row `number`
/// of [`layer_ui_configs`] in the in-memory view (MuPDF
/// `pdf_select_layer_config_ui` / `pdf_toggle_layer_config_ui` /
/// `pdf_deselect_layer_config_ui`; PyMuPDF `set_layer_ui_config`). Label and
/// locked rows are left untouched; switching a radio-box row ON first clears
/// the other members of its `/RBGroups` group(s). Nothing is written to the
/// document.
///
/// # Errors
///
/// [`Error::InvalidArgument`] when `number` is out of range.
pub fn set_layer_ui_config(doc: &DocumentStore, number: usize, action: u8) -> Result<()> {
    let rows = layer_ui_configs(doc);
    let Some(row) = rows.get(number) else {
        return Err(Error::InvalidArgument("Out of range UI entry selected"));
    };
    if row.kind == "label" || row.locked {
        return Ok(());
    }
    let on = match action {
        1 => !row.on,
        2 => false,
        _ => true,
    };
    let mut view = doc.layer_view();
    if on && row.kind == "radiobox" {
        if let Some(ocp) = oc_properties(doc) {
            let cfg = OcConfig::active(doc, &ocp);
            for group in cfg.rbgroups.iter().filter(|g| g.contains(&row.ocg)) {
                for &member in group {
                    view.overrides.insert(member, false);
                }
            }
        }
    }
    view.overrides.insert(row.ocg, on);
    doc.set_layer_view(view);
    Ok(())
}

/// The `/OC` optional-content reference of the image / form XObject `xref`
/// (PyMuPDF `get_oc()`): the OCG or OCMD object number, or 0 when the XObject
/// carries no `/OC` (or a direct one).
///
/// # Errors
///
/// [`Error::InvalidArgument`] when `xref` is not an object with `/Subtype
/// /Image` or `/Form`.
pub fn get_oc(doc: &DocumentStore, xref: u32) -> Result<u32> {
    let obj = doc.get_object(xref, 0)?;
    let is_xobject = obj.as_dict().is_some_and(|d| {
        d.get(&Name::new("Subtype"))
            .and_then(Object::as_name)
            .is_some_and(|n| matches!(n.as_bytes(), b"Image" | b"Form"))
    });
    if !is_xobject {
        return Err(Error::InvalidArgument("not an image or form XObject"));
    }
    Ok(obj
        .as_dict()
        .and_then(|d| d.get(&Name::new("OC")))
        .and_then(Object::as_reference)
        .map_or(0, |r| r.num))
}

/// Reads the Optional Content Membership Dictionary `xref` (PyMuPDF
/// `get_ocmd()`).
///
/// # Errors
///
/// [`Error::InvalidArgument`] when `xref` is not a `/Type /OCMD` dictionary.
pub fn get_ocmd(doc: &DocumentStore, xref: u32) -> Result<OcmdInfo> {
    let obj = doc.get_object(xref, 0)?;
    let Some(d) = obj.as_dict() else {
        return Err(Error::InvalidArgument("not an OCMD"));
    };
    if !is_type(d, b"OCMD") {
        return Err(Error::InvalidArgument("not an OCMD"));
    }
    let policy = d
        .get(&Name::new("P"))
        .and_then(Object::as_name)
        .map(name_string);
    let ve = doc
        .resolve_dict_key(d, &Name::new("VE"))
        .ok()
        .flatten()
        .and_then(|o| o.as_array().map(|a| parse_ve(doc, a, 0)));
    Ok(OcmdInfo {
        xref,
        ocgs: ocmd_ocgs(doc, d),
        policy,
        ve,
    })
}

/// The hidden-OCG oracle for one interpreter run: the set of OCGs that are OFF
/// in the store's active [`LayerView`], plus the `/OCMD` evaluation rules
/// (ISO 32000-1 §8.11.2.2; MuPDF `pdf_is_ocg_hidden`).
#[derive(Clone, Debug, Default)]
pub struct OcVisibility {
    /// The object numbers of the OCGs that are OFF.
    hidden: HashSet<u32>,
}

impl OcVisibility {
    /// Snapshots the active configuration's OFF set. A non-layered document
    /// yields an oracle that hides nothing.
    #[must_use]
    pub fn read(doc: &DocumentStore) -> Self {
        let Some(ocp) = oc_properties(doc) else {
            return Self::default();
        };
        let cfg = OcConfig::active(doc, &ocp);
        let hidden = ocg_object_numbers(doc, &ocp)
            .into_iter()
            .filter(|&num| !cfg.is_on(num))
            .collect();
        OcVisibility { hidden }
    }

    /// Whether any OCG is hidden at all (fast path for the interpreter).
    #[must_use]
    pub fn hides_anything(&self) -> bool {
        !self.hidden.is_empty()
    }

    /// Whether the OCG `num` is OFF.
    #[must_use]
    pub fn is_ocg_hidden(&self, num: u32) -> bool {
        self.hidden.contains(&num)
    }

    /// Whether content governed by the `/OC` value `oc` (an OCG or OCMD, given
    /// as a reference or a direct dictionary) is hidden.
    #[must_use]
    pub fn is_hidden(&self, doc: &DocumentStore, oc: &Object) -> bool {
        if self.hidden.is_empty() {
            return false;
        }
        self.is_hidden_at(doc, oc, 0)
    }

    fn is_hidden_at(&self, doc: &DocumentStore, oc: &Object, depth: u32) -> bool {
        if depth > MAX_VE_DEPTH {
            return false;
        }
        match oc {
            Object::Reference(r) => {
                let Ok(obj) = doc.resolve(*r) else {
                    return false;
                };
                match obj.as_dict() {
                    Some(d) if is_type(d, b"OCMD") => self.ocmd_hidden(doc, d, depth),
                    Some(_) => self.hidden.contains(&r.num),
                    None => false,
                }
            }
            Object::Dictionary(d) if is_type(d, b"OCMD") => self.ocmd_hidden(doc, d, depth),
            _ => false,
        }
    }

    /// Evaluates an OCMD: a `/VE` expression when present, else the `/OCGs`
    /// set under the `/P` policy (`AnyOn` by default).
    fn ocmd_hidden(&self, doc: &DocumentStore, d: &Dict, depth: u32) -> bool {
        if let Ok(Some(ve)) = doc.resolve_dict_key(d, &Name::new("VE")) {
            if let Some(arr) = ve.as_array() {
                return !self.eval_ve(doc, arr, depth + 1);
            }
        }
        let ocgs = ocmd_ocgs(doc, d).unwrap_or_default();
        if ocgs.is_empty() {
            return false;
        }
        let hidden = |num: &u32| self.hidden.contains(num);
        let policy = d
            .get(&Name::new("P"))
            .and_then(Object::as_name)
            .map_or(&b"AnyOn"[..], Name::as_bytes);
        match policy {
            b"AllOn" => ocgs.iter().any(hidden),
            b"AnyOff" => !ocgs.iter().any(hidden),
            b"AllOff" => !ocgs.iter().all(hidden),
            _ => ocgs.iter().all(hidden),
        }
    }

    /// Evaluates a `/VE` array to its visibility (`true` = visible). Malformed
    /// nodes evaluate to visible.
    fn eval_ve(&self, doc: &DocumentStore, arr: &[Object], depth: u32) -> bool {
        if depth > MAX_VE_DEPTH {
            return true;
        }
        let Some(op) = arr.first().and_then(Object::as_name) else {
            return true;
        };
        let operand = |o: &Object| -> bool {
            match o {
                Object::Reference(r) => match doc.resolve(*r).ok() {
                    Some(obj) => match obj.as_array() {
                        Some(nested) => self.eval_ve(doc, nested, depth + 1),
                        None => !self.hidden.contains(&r.num),
                    },
                    None => true,
                },
                Object::Array(nested) => self.eval_ve(doc, nested, depth + 1),
                _ => true,
            }
        };
        match op.as_bytes() {
            b"Not" => arr.get(1).is_none_or(|o| !operand(o)),
            b"And" => arr[1..].iter().all(operand),
            b"Or" => arr[1..].iter().any(operand),
            _ => true,
        }
    }
}

/// Nesting cap for `/VE` / OCMD evaluation and parsing (defensive).
const MAX_VE_DEPTH: u32 = 32;

// --- internal helpers -----------------------------------------------------

/// Whether `d` has `/Type /<name>`.
fn is_type(d: &Dict, name: &[u8]) -> bool {
    d.get(&Name::new("Type"))
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == name)
}

/// The configuration dictionary for `number` (MuPDF `pdf_select_layer_config`
/// numbering): `/D` for `None`, `/Configs[n]` for `Some(n)` (resolved through
/// a reference), or `None` when absent.
pub(crate) fn config_dict(doc: &DocumentStore, ocp: &Dict, number: Option<usize>) -> Option<Dict> {
    let Some(n) = number else {
        return doc
            .resolve_dict_key(ocp, &Name::new("D"))
            .ok()
            .flatten()
            .and_then(|o| o.as_dict().cloned());
    };
    let configs = doc.resolve_dict_key(ocp, &Name::new("Configs")).ok()??;
    match configs.as_array()?.get(n)? {
        Object::Reference(r) => doc.resolve(*r).ok()?.as_dict().cloned(),
        other => other.as_dict().cloned(),
    }
}

/// A text-string entry of `d` (decoded), or `None` when absent.
fn text_entry(d: &Dict, key: &str) -> Option<String> {
    d.get(&Name::new(key))
        .and_then(Object::as_string)
        .map(|s| decode_text(s.as_bytes()))
}

/// The OCG object numbers of an OCMD's `/OCGs` (a single reference or an
/// array of references); `None` when the entry is absent.
fn ocmd_ocgs(doc: &DocumentStore, d: &Dict) -> Option<Vec<u32>> {
    let refs = |items: &[Object]| -> Vec<u32> {
        items
            .iter()
            .filter_map(Object::as_reference)
            .map(|r| r.num)
            .collect()
    };
    match d.get(&Name::new("OCGs"))? {
        Object::Reference(r) => match doc.resolve(*r).ok() {
            Some(obj) => Some(obj.as_array().map_or_else(|| vec![r.num], refs)),
            None => Some(Vec::new()),
        },
        Object::Array(items) => Some(refs(items)),
        _ => Some(Vec::new()),
    }
}

/// Parses a `/VE` array into a [`VeExpr`] (unknown operands are dropped).
fn parse_ve(doc: &DocumentStore, arr: &[Object], depth: u32) -> VeExpr {
    let op = arr
        .first()
        .and_then(Object::as_name)
        .map(name_string)
        .unwrap_or_default();
    let mut args = Vec::new();
    if depth <= MAX_VE_DEPTH {
        for item in arr.iter().skip(1) {
            match item {
                Object::Reference(r) => match doc.resolve(*r).ok() {
                    Some(obj) if obj.as_array().is_some() => {
                        let nested = obj.as_array().unwrap_or(&[]);
                        args.push(parse_ve(doc, nested, depth + 1));
                    }
                    _ => args.push(VeExpr::Ocg(r.num)),
                },
                Object::Array(nested) => args.push(parse_ve(doc, nested, depth + 1)),
                _ => {}
            }
        }
    }
    VeExpr::Op { op, args }
}

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
fn read_ocg(doc: &DocumentStore, num: u32, cfg: &OcConfig) -> OcgInfo {
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
    cfg: &OcConfig,
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
                    number: 0,
                    ocg: r.num,
                    text: info.name,
                    depth,
                    kind: cfg.ui_kind(r.num),
                    on: info.on,
                    locked: info.locked,
                });
            }
            Object::Array(nested) => {
                // A leading string is the group's label (a non-toggle row,
                // reported locked like MuPDF's `PDF_LAYER_UI_LABEL`).
                let mut start = 0;
                if let Some(Object::String(s)) = nested.first() {
                    out.push(LayerUiConfig {
                        number: 0,
                        ocg: 0,
                        text: decode_text(s.as_bytes()),
                        depth,
                        kind: "label",
                        on: false,
                        locked: true,
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

/// A parsed `/OCConfig` — the ON/OFF/Locked sets, the base state, the raw
/// `/Order` array (resolved one level), the `/RBGroups` radio groups and any
/// layer-panel overrides.
pub(crate) struct OcConfig {
    on: Vec<u32>,
    off: Vec<u32>,
    locked: Vec<u32>,
    /// `true` when `/BaseState` is `/OFF` (default is `/ON`).
    base_off: bool,
    order: Option<Vec<Object>>,
    rbgroups: Vec<Vec<u32>>,
    /// Per-OCG overrides from the store's [`LayerView`] (win over the arrays).
    overrides: BTreeMap<u32, bool>,
}

impl OcConfig {
    /// Reads the configuration selected by the store's [`LayerView`] (`/D`, or
    /// `/Configs[n]` falling back to `/D` when it does not resolve) and applies
    /// its overrides. Like MuPDF's `load_ui`, an alternate configuration
    /// without `/Order` / `/RBGroups` inherits them from `/D`.
    pub(crate) fn active(doc: &DocumentStore, ocp: &Dict) -> Self {
        let view = doc.layer_view();
        let default = config_dict(doc, ocp, None).unwrap_or_default();
        let mut cfg = match view.config.and_then(|n| config_dict(doc, ocp, Some(n))) {
            Some(alt) => {
                let mut cfg = Self::from_dict(doc, &alt);
                if cfg.order.is_none() {
                    cfg.order = read_order(doc, &default);
                }
                if cfg.rbgroups.is_empty() {
                    cfg.rbgroups = read_rbgroups(doc, &default);
                }
                cfg
            }
            None => Self::from_dict(doc, &default),
        };
        cfg.overrides = view.overrides;
        cfg
    }

    /// Parses one `/OCConfig` dictionary. An empty dict yields an all-ON base
    /// state with empty sets.
    pub(crate) fn from_dict(doc: &DocumentStore, d: &Dict) -> Self {
        let on = ref_nums(doc, d, "ON");
        let off = ref_nums(doc, d, "OFF");
        let locked = ref_nums(doc, d, "Locked");
        let base_off = matches!(
            d.get(&Name::new("BaseState")),
            Some(Object::Name(n)) if n.as_bytes() == b"OFF"
        );
        OcConfig {
            on,
            off,
            locked,
            base_off,
            order: read_order(doc, d),
            rbgroups: read_rbgroups(doc, d),
            overrides: BTreeMap::new(),
        }
    }

    /// The layer-panel row kind of OCG `num`: `"radiobox"` when it belongs to
    /// an `/RBGroups` group, else `"checkbox"`.
    fn ui_kind(&self, num: u32) -> &'static str {
        if self.rbgroups.iter().any(|g| g.contains(&num)) {
            "radiobox"
        } else {
            "checkbox"
        }
    }

    /// Whether OCG `num` is visible in this configuration: a layer-panel
    /// override wins, then `/ON`, then `/OFF`, otherwise the `/BaseState`
    /// default.
    pub(crate) fn is_on(&self, num: u32) -> bool {
        if let Some(&forced) = self.overrides.get(&num) {
            return forced;
        }
        if self.on.contains(&num) {
            return true;
        }
        if self.off.contains(&num) {
            return false;
        }
        !self.base_off
    }
}

/// The raw `/Order` array of a configuration (resolved one level).
fn read_order(doc: &DocumentStore, d: &Dict) -> Option<Vec<Object>> {
    doc.resolve_dict_key(d, &Name::new("Order"))
        .ok()
        .flatten()
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
}

/// The `/RBGroups` of a configuration as lists of OCG object numbers.
fn read_rbgroups(doc: &DocumentStore, d: &Dict) -> Vec<Vec<u32>> {
    let Ok(Some(groups)) = doc.resolve_dict_key(d, &Name::new("RBGroups")) else {
        return Vec::new();
    };
    let Some(groups) = groups.as_array() else {
        return Vec::new();
    };
    groups
        .iter()
        .filter_map(|g| match g {
            Object::Array(items) => Some(items.clone()),
            Object::Reference(r) => doc.resolve(*r).ok()?.as_array().map(<[Object]>::to_vec),
            _ => None,
        })
        .map(|items| {
            items
                .iter()
                .filter_map(Object::as_reference)
                .map(|r| r.num)
                .collect()
        })
        .collect()
}

/// Resolves a configuration array key (`ON`/`OFF`/`Locked`) into a list of
/// OCG object numbers.
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
