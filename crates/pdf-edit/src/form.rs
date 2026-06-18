//! Interactive forms (AcroForm) — read the field tree, fill values per field
//! type with appearance regeneration, expose the [`Widget`] handle, and
//! [`flatten`] the form into static page content (PRD §8.8 / §12 M4 exit).
//!
//! Model: a form field is a dictionary in the catalog `/AcroForm /Fields` tree
//! (with `/Kids` for hierarchy and radio groups). A **widget** is the on-page
//! appearance of a field — a `/Subtype /Widget` annotation in a page's
//! `/Annots`. In the common single-widget case the field and widget dictionaries
//! are **merged** into one object (it carries both `/FT` and `/Subtype /Widget`).
//!
//! Field type (`/FT`, inheritable up the `/Parent` chain): `Tx` text, `Btn`
//! button (checkbox / radio / pushbutton, distinguished by `/Ff` bits 32768
//! radio / 65536 pushbutton), `Ch` choice (combo vs list by `/Ff` bit 131072),
//! `Sig` signature.
//!
//! Filling regenerates the widget `/AP /N` appearance so viewers without
//! `/NeedAppearances` still show the value (the preferred portability path):
//! text/choice draw the value per `/DA` into a Form XObject; checkbox/radio set
//! `/AS` to the on-state name discovered from the widget's own `/AP /N` keys (not
//! assumed `/Yes`). Read-only fields (`/Ff` bit 1) reject a set.
//!
//! Coordinate model: a widget's `/Rect` is already PDF user space (y up); the
//! generated `/AP /N` draws in the rect's translated frame with identity
//! `/Matrix` and `/BBox` == the rect, exactly as [`crate::annot`] does.

use pdf_core::error::{Error, Result};
use pdf_core::geom::Rect;
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::pagetree;
use pdf_core::{DocumentStore, PdfString, StringKind};
use pdf_fonts::std_widths;

use crate::color::Color;
use crate::content::{escape_pdf_literal, fmt_num, PageContent};

// === field-flag bit masks (ISO 32000-1 Table 226/227/228) =================

/// `/Ff` bit 1 — Read-only.
pub const FF_READ_ONLY: i64 = 1 << 0;
/// `/Ff` bit 13 (value 4096) — Multiline (text fields).
pub const FF_MULTILINE: i64 = 1 << 12;
/// `/Ff` bit 16 (value 32768) — Radio (button fields).
pub const FF_RADIO: i64 = 1 << 15;
/// `/Ff` bit 17 (value 65536) — Pushbutton (button fields).
pub const FF_PUSHBUTTON: i64 = 1 << 16;
/// `/Ff` bit 18 (value 131072) — Combo (choice fields).
pub const FF_COMBO: i64 = 1 << 17;

// === field type ===========================================================

/// A form field's logical type, classified from `/FT` + `/Ff`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FieldType {
    /// `/FT /Tx` — a text field.
    Text,
    /// `/FT /Btn` with neither radio nor pushbutton flag — a checkbox.
    CheckBox,
    /// `/FT /Btn` with the radio flag (`/Ff` 32768) — one button of a radio group.
    RadioButton,
    /// `/FT /Btn` with the pushbutton flag (`/Ff` 65536) — a pushbutton (no value).
    PushButton,
    /// `/FT /Ch` with the combo flag (`/Ff` 131072) — a dropdown combo box.
    ComboBox,
    /// `/FT /Ch` without the combo flag — a scrollable list box.
    ListBox,
    /// `/FT /Sig` — a signature field (read-only here).
    Signature,
    /// An unknown / missing `/FT`.
    Unknown,
}

impl FieldType {
    /// Classifies a field from its (inherited) `/FT` name and `/Ff` flags.
    #[must_use]
    pub fn classify(ft: Option<&[u8]>, ff: i64) -> FieldType {
        match ft {
            Some(b"Tx") => FieldType::Text,
            Some(b"Btn") => {
                if ff & FF_PUSHBUTTON != 0 {
                    FieldType::PushButton
                } else if ff & FF_RADIO != 0 {
                    FieldType::RadioButton
                } else {
                    FieldType::CheckBox
                }
            }
            Some(b"Ch") => {
                if ff & FF_COMBO != 0 {
                    FieldType::ComboBox
                } else {
                    FieldType::ListBox
                }
            }
            Some(b"Sig") => FieldType::Signature,
            _ => FieldType::Unknown,
        }
    }

    /// The PyMuPDF-style string label for this field type.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FieldType::Text => "Text",
            FieldType::CheckBox => "CheckBox",
            FieldType::RadioButton => "RadioButton",
            FieldType::PushButton => "PushButton",
            FieldType::ComboBox => "ComboBox",
            FieldType::ListBox => "ListBox",
            FieldType::Signature => "Signature",
            FieldType::Unknown => "Unknown",
        }
    }
}

// === AcroForm-level read ==================================================

/// Whether the document has an interactive form (a catalog `/AcroForm` with a
/// non-empty `/Fields` array).
#[must_use]
pub fn is_form_pdf(doc: &DocumentStore) -> bool {
    acroform_dict(doc)
        .and_then(|af| field_refs(doc, &af))
        .map(|f| !f.is_empty())
        .unwrap_or(false)
}

/// The catalog `/AcroForm` dictionary (resolved), if present.
#[must_use]
pub fn acroform_dict(doc: &DocumentStore) -> Option<Dict> {
    let root = doc.root()?;
    let catalog = doc.resolve(root).ok()?.as_dict().cloned()?;
    match catalog.get(&Name::new("AcroForm"))? {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(r) => doc.resolve(*r).ok()?.as_dict().cloned(),
        _ => None,
    }
}

/// The catalog `/AcroForm` object reference (when stored indirectly), if present.
fn acroform_ref(doc: &DocumentStore) -> Option<ObjRef> {
    let root = doc.root()?;
    let catalog = doc.resolve(root).ok()?.as_dict().cloned()?;
    catalog
        .get(&Name::new("AcroForm"))
        .and_then(Object::as_reference)
}

/// Whether the document has an `/AcroForm` at all (PyMuPDF reports
/// `need_appearances()` as `None` when there is no form).
#[must_use]
pub fn has_acroform(doc: &DocumentStore) -> bool {
    acroform_dict(doc).is_some()
}

/// Whether the form requests `/NeedAppearances true`.
#[must_use]
pub fn need_appearances(doc: &DocumentStore) -> bool {
    acroform_dict(doc)
        .and_then(|af| {
            af.get(&Name::new("NeedAppearances"))
                .and_then(Object::as_bool)
        })
        .unwrap_or(false)
}

/// Sets the form `/NeedAppearances` flag (PyMuPDF `Document.need_appearances`
/// with a value). A no-op when the document has no `/AcroForm`.
///
/// # Errors
///
/// Propagates object-edit errors.
pub fn set_need_appearances(doc: &DocumentStore, value: bool) -> Result<()> {
    let key = Name::new("NeedAppearances");
    if let Some(r) = acroform_ref(doc) {
        // Indirect AcroForm: rewrite the referenced dict in place.
        let mut af = doc
            .resolve(r)?
            .as_dict()
            .cloned()
            .ok_or(Error::InvalidArgument("/AcroForm is not a dictionary"))?;
        af.insert(key, Object::Boolean(value));
        doc.update_object(r, Object::Dictionary(af))?;
        return Ok(());
    }
    // Inline AcroForm dict in the catalog (or none): rewrite the catalog.
    let root = doc
        .root()
        .ok_or(Error::InvalidArgument("document has no /Root"))?;
    let mut catalog = doc
        .resolve(root)?
        .as_dict()
        .cloned()
        .ok_or(Error::InvalidArgument("/Root is not a dictionary"))?;
    if let Some(Object::Dictionary(af)) = catalog.get(&Name::new("AcroForm")) {
        let mut af = af.clone();
        af.insert(key, Object::Boolean(value));
        catalog.insert(Name::new("AcroForm"), Object::Dictionary(af));
        doc.update_object(root, Object::Dictionary(catalog))?;
    }
    Ok(())
}

/// The form `/SigFlags` integer (PyMuPDF `Document.get_sigflags`), or `-1` when
/// the document has no `/AcroForm` — fitz's "no form" sentinel. A present
/// `/AcroForm` with no `/SigFlags` reports `0`.
#[must_use]
pub fn sigflags(doc: &DocumentStore) -> i32 {
    match acroform_dict(doc) {
        Some(af) => af
            .get(&Name::new("SigFlags"))
            .and_then(Object::as_i64)
            .map(|v| v as i32)
            .unwrap_or(0),
        None => -1,
    }
}

/// The form default appearance string `/DA`, if present.
#[must_use]
pub fn default_appearance(doc: &DocumentStore) -> Option<String> {
    acroform_dict(doc).and_then(|af| {
        af.get(&Name::new("DA"))
            .and_then(Object::as_string)
            .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
    })
}

/// The `/Fields` references of `acroform` (top-level fields).
fn field_refs(doc: &DocumentStore, acroform: &Dict) -> Option<Vec<ObjRef>> {
    let arr = match acroform.get(&Name::new("Fields"))? {
        Object::Array(a) => a.clone(),
        Object::Reference(r) => doc.resolve(*r).ok()?.as_array().map(<[Object]>::to_vec)?,
        _ => return None,
    };
    Some(arr.iter().filter_map(Object::as_reference).collect())
}

/// Walks the `/AcroForm /Fields` tree and returns every **terminal field**
/// reference (a field with no field-`/Kids`, i.e. a leaf — its `/Kids` if any are
/// pure widgets). Intermediate field nodes are not returned; their FQN prefix is
/// composed into the leaves. Returns refs in document order.
#[must_use]
pub fn terminal_field_refs(doc: &DocumentStore) -> Vec<ObjRef> {
    let Some(af) = acroform_dict(doc) else {
        return Vec::new();
    };
    let Some(roots) = field_refs(doc, &af) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in roots {
        collect_terminals(doc, r, &mut out, &mut seen, 0);
    }
    out
}

/// Recursively classifies a node as terminal (a real field) or intermediate.
/// A node is **intermediate** only if at least one `/Kids` entry is itself a
/// field (has `/T` or `/FT`); kids that are pure widgets keep the node terminal.
fn collect_terminals(
    doc: &DocumentStore,
    node: ObjRef,
    out: &mut Vec<ObjRef>,
    seen: &mut std::collections::HashSet<u32>,
    depth: usize,
) {
    if depth > 50 || !seen.insert(node.num) {
        return;
    }
    let Ok(obj) = doc.resolve(node) else { return };
    let Some(d) = obj.as_dict() else { return };
    let kids = kid_refs(doc, d);
    let has_field_kid = kids.iter().any(|&k| ref_is_field_node(doc, k));
    if has_field_kid {
        for k in kids {
            collect_terminals(doc, k, out, seen, depth + 1);
        }
    } else {
        out.push(node);
    }
}

/// The `/Kids` references of a dict.
fn kid_refs(doc: &DocumentStore, d: &Dict) -> Vec<ObjRef> {
    match d.get(&Name::new("Kids")) {
        Some(Object::Array(a)) => a.iter().filter_map(Object::as_reference).collect(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec))
            .unwrap_or_default()
            .iter()
            .filter_map(Object::as_reference)
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether `r` resolves to a *field* node (carries `/T` or `/FT`) rather than a
/// pure widget annotation.
fn ref_is_field_node(doc: &DocumentStore, r: ObjRef) -> bool {
    let Ok(obj) = doc.resolve(r) else {
        return false;
    };
    let Some(d) = obj.as_dict() else {
        return false;
    };
    d.contains_key(&Name::new("T")) || d.contains_key(&Name::new("FT"))
}

/// All form field handles (terminal fields) in document order.
#[must_use]
pub fn form_fields(doc: &DocumentStore) -> Vec<Field<'_>> {
    terminal_field_refs(doc)
        .into_iter()
        .map(|obj| Field { doc, obj })
        .collect()
}

// === Field handle =========================================================

/// A handle to one terminal form field (the dictionary in the `/AcroForm` tree).
/// In the merged single-widget case this is the same object as its widget.
pub struct Field<'a> {
    doc: &'a DocumentStore,
    obj: ObjRef,
}

impl<'a> Field<'a> {
    /// Wraps a known terminal-field reference.
    #[must_use]
    pub fn from_ref(doc: &'a DocumentStore, obj: ObjRef) -> Self {
        Field { doc, obj }
    }

    /// The field object reference.
    #[must_use]
    pub fn xref(&self) -> ObjRef {
        self.obj
    }

    fn dict(&self) -> Option<Dict> {
        self.doc.resolve(self.obj).ok()?.as_dict().cloned()
    }

    /// The inherited `/FT` field-type name, walking `/Parent` if absent locally.
    fn ft(&self) -> Option<Vec<u8>> {
        inherited_name(self.doc, self.obj, "FT")
    }

    /// The inherited `/Ff` flags (OR is not done; the nearest definition wins,
    /// matching ISO 32000-1 inheritance).
    #[must_use]
    pub fn field_flags(&self) -> i64 {
        inherited_i64(self.doc, self.obj, "Ff").unwrap_or(0)
    }

    /// The field type.
    #[must_use]
    pub fn field_type(&self) -> FieldType {
        FieldType::classify(self.ft().as_deref(), self.field_flags())
    }

    /// The PyMuPDF-style field-type string.
    #[must_use]
    pub fn field_type_string(&self) -> &'static str {
        self.field_type().as_str()
    }

    /// The fully-qualified field name: the partial names `/T` joined by `.` from
    /// the root of the field tree down to this field.
    #[must_use]
    pub fn field_name(&self) -> String {
        fully_qualified_name(self.doc, self.obj)
    }

    /// The user-facing field label `/TU` (the "alternate" / tooltip name).
    #[must_use]
    pub fn field_label(&self) -> Option<String> {
        self.dict().and_then(|d| {
            d.get(&Name::new("TU"))
                .and_then(Object::as_string)
                .map(decode_text_string)
        })
    }

    /// The current value `/V` as a string (inherited). Names and strings both
    /// decode to text; arrays return their first element.
    #[must_use]
    pub fn field_value(&self) -> Option<String> {
        value_to_string(inherited_value(self.doc, self.obj, "V")?)
    }

    /// The default value `/DV` as a string (inherited).
    #[must_use]
    pub fn default_value(&self) -> Option<String> {
        value_to_string(inherited_value(self.doc, self.obj, "DV")?)
    }

    /// The choice option values `/Opt` (export values). For `[export, display]`
    /// pairs the export value is returned.
    #[must_use]
    pub fn choice_values(&self) -> Vec<String> {
        let Some(d) = self.dict() else {
            return Vec::new();
        };
        read_opt(self.doc, &d)
    }

    /// Whether the field is read-only (`/Ff` bit 1).
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.field_flags() & FF_READ_ONLY != 0
    }

    /// The field's widget references: its own (if merged) plus any widget kids.
    #[must_use]
    pub fn widget_refs(&self) -> Vec<ObjRef> {
        let Some(d) = self.dict() else {
            return Vec::new();
        };
        let kids = kid_refs(self.doc, &d);
        if kids.is_empty() {
            // Merged field+widget.
            vec![self.obj]
        } else {
            kids
        }
    }

    /// Sets this field's value, regenerating widget appearances. Dispatches on
    /// field type; read-only fields and signatures are rejected.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for read-only / signature / pushbutton fields
    /// or an out-of-domain choice/checkbox value; propagates object-edit errors.
    pub fn set_field_value(&self, value: &str) -> Result<()> {
        if self.is_read_only() {
            return Err(Error::InvalidArgument("form field is read-only"));
        }
        match self.field_type() {
            FieldType::Text => self.set_text_value(value),
            FieldType::CheckBox => self.set_checkbox_value(value),
            FieldType::RadioButton => self.set_radio_value(value),
            FieldType::ComboBox | FieldType::ListBox => self.set_choice_value(value),
            FieldType::PushButton => Err(Error::InvalidArgument("pushbutton fields have no value")),
            FieldType::Signature => Err(Error::InvalidArgument("signature fields are read-only")),
            FieldType::Unknown => Err(Error::InvalidArgument("field has no /FT")),
        }
    }

    // --- per-type fill --------------------------------------------------

    fn set_text_value(&self, value: &str) -> Result<()> {
        let mut d = self.dict_or_err()?;
        d.insert(Name::new("V"), text_string(value));
        self.doc.update_object(self.obj, Object::Dictionary(d))?;
        // Regenerate each widget appearance.
        for w in self.widget_refs() {
            self.regen_text_ap(w, value)?;
        }
        Ok(())
    }

    fn set_choice_value(&self, value: &str) -> Result<()> {
        // Validate the value is among the options (when options are declared).
        let opts = self.choice_values();
        if !opts.is_empty() && !opts.iter().any(|o| o == value) {
            return Err(Error::InvalidArgument("value not in choice options"));
        }
        let mut d = self.dict_or_err()?;
        d.insert(Name::new("V"), text_string(value));
        self.doc.update_object(self.obj, Object::Dictionary(d))?;
        for w in self.widget_refs() {
            self.regen_text_ap(w, value)?;
        }
        Ok(())
    }

    fn set_checkbox_value(&self, value: &str) -> Result<()> {
        // The on-state of a checkbox is the single non-`Off` key of the widget's
        // `/AP /N`. "Checking" means matching that name; everything else is Off.
        let widget = self
            .widget_refs()
            .into_iter()
            .next()
            .ok_or(Error::InvalidArgument("checkbox has no widget"))?;
        let on = on_state_name(self.doc, widget);
        let truthy = is_truthy(value);
        // Resolve the requested state name: an explicit on-state name, a generic
        // truthy token, or Off.
        let state = if let Some(on) = &on {
            if value == on.as_str() || (truthy && !value.eq_ignore_ascii_case("off")) {
                on.clone()
            } else {
                "Off".to_string()
            }
        } else if truthy {
            // No discoverable on-state — fall back to the conventional `/Yes`.
            "Yes".to_string()
        } else {
            "Off".to_string()
        };
        // Field `/V` and widget `/AS` both carry the state name.
        let mut d = self.dict_or_err()?;
        d.insert(Name::new("V"), Object::Name(Name::new(&state)));
        // If field and widget are merged, set /AS here too.
        if self.widget_refs() == vec![self.obj] {
            d.insert(Name::new("AS"), Object::Name(Name::new(&state)));
        }
        self.doc.update_object(self.obj, Object::Dictionary(d))?;
        // Separate widget object case.
        if widget.num != self.obj.num {
            self.set_widget_as(widget, &state)?;
        }
        Ok(())
    }

    fn set_radio_value(&self, value: &str) -> Result<()> {
        // The group `/V` becomes the chosen kid's on-state; each kid's `/AS` is
        // its own on-state if it matches `value`, else `/Off`.
        let kids = self.widget_refs();
        let mut matched: Option<String> = None;
        for w in &kids {
            let on = on_state_name(self.doc, *w);
            let is_match = match &on {
                Some(name) => name.as_str() == value,
                None => false,
            };
            if is_match {
                matched = on.clone();
                self.set_widget_as(*w, on.as_deref().unwrap_or("Off"))?;
            } else {
                self.set_widget_as(*w, "Off")?;
            }
        }
        let state = matched.ok_or(Error::InvalidArgument(
            "value matches no radio kid on-state",
        ))?;
        let mut d = self.dict_or_err()?;
        d.insert(Name::new("V"), Object::Name(Name::new(&state)));
        // The group dict may itself be a widget (rare); clear its /AS to Off.
        if d.contains_key(&Name::new("Subtype")) {
            d.insert(Name::new("AS"), Object::Name(Name::new("Off")));
        }
        self.doc.update_object(self.obj, Object::Dictionary(d))
    }

    /// Sets `/AS` on a widget object (used for checkboxes/radios with a separate
    /// widget object).
    fn set_widget_as(&self, widget: ObjRef, state: &str) -> Result<()> {
        let mut wd = self
            .doc
            .resolve(widget)?
            .as_dict()
            .cloned()
            .ok_or(Error::InvalidArgument("widget is not a dictionary"))?;
        wd.insert(Name::new("AS"), Object::Name(Name::new(state)));
        self.doc.update_object(widget, Object::Dictionary(wd))
    }

    /// Regenerates a text/choice widget's `/AP /N` showing `value` per the
    /// effective `/DA` (font / size / color), honoring `/Q` alignment and the
    /// multiline flag.
    fn regen_text_ap(&self, widget: ObjRef, value: &str) -> Result<()> {
        let wd = self
            .doc
            .resolve(widget)?
            .as_dict()
            .cloned()
            .ok_or(Error::InvalidArgument("widget is not a dictionary"))?;
        let rect = read_rect(&wd).normalize();
        let da = self.effective_da(&wd);
        let (fontsize, color) = parse_da(&da);
        let multiline = self.field_flags() & FF_MULTILINE != 0;
        let q = wd
            .get(&Name::new("Q"))
            .and_then(Object::as_i64)
            .unwrap_or(0);
        let ap = build_text_field_ap(rect, value, fontsize, color, q, multiline);
        let n_ref = match existing_ap_n(self.doc, &wd) {
            Some(r) => {
                self.doc.update_object(r, Object::Stream(ap))?;
                r
            }
            None => self.doc.add_object(Object::Stream(ap))?,
        };
        let mut wd = self
            .doc
            .resolve(widget)?
            .as_dict()
            .cloned()
            .ok_or(Error::InvalidArgument("widget is not a dictionary"))?;
        let mut ap_dict = Dict::new();
        ap_dict.insert(Name::new("N"), Object::Reference(n_ref));
        wd.insert(Name::new("AP"), Object::Dictionary(ap_dict));
        self.doc.update_object(widget, Object::Dictionary(wd))
    }

    /// The effective `/DA`: the widget's own `/DA`, else the field's (inherited),
    /// else the AcroForm `/DA`, else a Helvetica default.
    fn effective_da(&self, wd: &Dict) -> String {
        if let Some(da) = wd
            .get(&Name::new("DA"))
            .and_then(Object::as_string)
            .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
        {
            return da;
        }
        if let Some(v) = inherited_value(self.doc, self.obj, "DA") {
            if let Some(s) = v.as_string() {
                return String::from_utf8_lossy(s.as_bytes()).into_owned();
            }
        }
        default_appearance(self.doc).unwrap_or_else(|| "0 0 0 rg /Helv 0 Tf".to_string())
    }

    fn dict_or_err(&self) -> Result<Dict> {
        self.dict()
            .ok_or(Error::InvalidArgument("field is not a dictionary"))
    }

    /// Clears the field's value: removes the `/V` key and regenerates each
    /// widget's appearance from an empty value. Used by `reset` when the field
    /// has no `/DV` default (matches PyMuPDF clearing `/V` rather than writing a
    /// literal value).
    fn clear_value(&self) -> Result<()> {
        let mut d = self.dict_or_err()?;
        d.remove(&Name::new("V"));
        self.doc.update_object(self.obj, Object::Dictionary(d))?;
        if matches!(
            self.field_type(),
            FieldType::Text | FieldType::ComboBox | FieldType::ListBox
        ) {
            for w in self.widget_refs() {
                self.regen_text_ap(w, "")?;
            }
        }
        Ok(())
    }
}

// === Widget handle ========================================================

/// A handle to one on-page form widget (`/Subtype /Widget` annotation), exposing
/// the PyMuPDF `Widget` surface. A widget is read through its owning field for
/// inherited attributes (`/FT`, `/Ff`, `/V`, FQN).
pub struct Widget<'a> {
    doc: &'a DocumentStore,
    obj: ObjRef,
}

impl<'a> Widget<'a> {
    /// Wraps a known widget-annotation reference.
    #[must_use]
    pub fn from_ref(doc: &'a DocumentStore, obj: ObjRef) -> Self {
        Widget { doc, obj }
    }

    /// The widget annotation object reference.
    #[must_use]
    pub fn xref(&self) -> ObjRef {
        self.obj
    }

    fn dict(&self) -> Option<Dict> {
        self.doc.resolve(self.obj).ok()?.as_dict().cloned()
    }

    /// The field this widget belongs to: itself when merged, else its `/Parent`.
    #[must_use]
    pub fn field(&self) -> Field<'a> {
        let field_ref = self
            .dict()
            .and_then(|d| {
                // A merged widget carries /FT (or /T); a pure widget points up
                // via /Parent to its field.
                if d.contains_key(&Name::new("FT")) {
                    Some(self.obj)
                } else {
                    d.get(&Name::new("Parent")).and_then(Object::as_reference)
                }
            })
            .unwrap_or(self.obj);
        Field {
            doc: self.doc,
            obj: field_ref,
        }
    }

    /// The widget's `/Rect` (PDF user space, normalized).
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.dict()
            .map(|d| read_rect(&d).normalize())
            .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0))
    }

    /// The field type (via the owning field).
    #[must_use]
    pub fn field_type(&self) -> FieldType {
        self.field().field_type()
    }

    /// The PyMuPDF-style field-type string.
    #[must_use]
    pub fn field_type_string(&self) -> &'static str {
        self.field_type().as_str()
    }

    /// The fully-qualified field name (via the owning field).
    #[must_use]
    pub fn field_name(&self) -> String {
        self.field().field_name()
    }

    /// The field label `/TU` (own, else field).
    #[must_use]
    pub fn field_label(&self) -> Option<String> {
        if let Some(l) = self.dict().and_then(|d| {
            d.get(&Name::new("TU"))
                .and_then(Object::as_string)
                .map(decode_text_string)
        }) {
            return Some(l);
        }
        self.field().field_label()
    }

    /// The current value (via the owning field).
    #[must_use]
    pub fn field_value(&self) -> Option<String> {
        self.field().field_value()
    }

    /// The field flags (via the owning field).
    #[must_use]
    pub fn field_flags(&self) -> i64 {
        self.field().field_flags()
    }

    /// The choice option values (via the owning field).
    #[must_use]
    pub fn choice_values(&self) -> Vec<String> {
        self.field().choice_values()
    }

    /// The widget's on-state names: the non-`Off` keys of its `/AP /N` (for
    /// checkbox / radio buttons). Empty for non-button widgets.
    #[must_use]
    pub fn button_states(&self) -> Vec<String> {
        let Some(d) = self.dict() else {
            return Vec::new();
        };
        ap_n_state_names(self.doc, &d)
    }

    /// The `/MK /BC` border color, as raw colorspace components (1=gray,
    /// 3=rgb, 4=cmyk), or `None` if absent (PyMuPDF `Widget.border_color`).
    #[must_use]
    pub fn border_color(&self) -> Option<Vec<f64>> {
        self.mk_color("BC")
    }

    /// The `/MK /BG` fill (background) color, raw components, or `None`
    /// (PyMuPDF `Widget.fill_color`).
    #[must_use]
    pub fn fill_color(&self) -> Option<Vec<f64>> {
        self.mk_color("BG")
    }

    /// Reads a raw color array nested under `/MK`.
    fn mk_color(&self, key: &str) -> Option<Vec<f64>> {
        let d = self.dict()?;
        let mk = resolve_dict(self.doc, d.get(&Name::new("MK"))?)?;
        let arr = mk.get(&Name::new(key)).and_then(Object::as_array)?;
        Some(arr.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect())
    }

    /// The border style full name ("Solid" / "Dashed" / "Beveled" / "Inset" /
    /// "Underline"), from `/BS /S` (default "Solid") (PyMuPDF
    /// `Widget.border_style`).
    #[must_use]
    pub fn border_style(&self) -> String {
        let code = self
            .bs_dict()
            .and_then(|bs| {
                bs.get(&Name::new("S"))
                    .and_then(Object::as_name)
                    .map(|n| n.as_bytes().first().copied().unwrap_or(b'S'))
            })
            .unwrap_or(b'S');
        match code {
            b'D' => "Dashed",
            b'B' => "Beveled",
            b'I' => "Inset",
            b'U' => "Underline",
            _ => "Solid",
        }
        .to_string()
    }

    /// The border width from `/BS /W`, defaulting to `1.0` when zero/absent
    /// (matches PyMuPDF `Widget.border_width`).
    #[must_use]
    pub fn border_width(&self) -> f64 {
        let w = self
            .bs_dict()
            .and_then(|bs| bs.get(&Name::new("W")).and_then(Object::as_f64))
            .unwrap_or(0.0);
        if w == 0.0 {
            1.0
        } else {
            w
        }
    }

    /// The border dash pattern from `/BS /D` as integers, or `None`
    /// (PyMuPDF `Widget.border_dashes`). Real entries are rounded to the nearest
    /// integer, mirroring MuPDF's `pdf_to_int` (e.g. `[2.5, 1.5]` -> `[3, 2]`).
    #[must_use]
    pub fn border_dashes(&self) -> Option<Vec<i64>> {
        let bs = self.bs_dict()?;
        let arr = bs.get(&Name::new("D")).and_then(Object::as_array)?;
        Some(
            arr.iter()
                .map(|o| match o {
                    Object::Integer(i) => *i,
                    _ => o.as_f64().map(|f| f.round() as i64).unwrap_or(0),
                })
                .collect(),
        )
    }

    /// Resolves the `/BS` border-style dict.
    fn bs_dict(&self) -> Option<Dict> {
        let d = self.dict()?;
        resolve_dict(self.doc, d.get(&Name::new("BS"))?)
    }

    /// The effective `/DA` default-appearance string (own, inherited, or
    /// AcroForm), or empty.
    fn da_string(&self) -> String {
        if let Some(da) = self.dict().and_then(|d| {
            d.get(&Name::new("DA"))
                .and_then(Object::as_string)
                .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
        }) {
            return da;
        }
        if let Some(o) = inherited_value(self.doc, self.obj, "DA") {
            if let Some(s) = o.as_string() {
                return String::from_utf8_lossy(s.as_bytes()).into_owned();
            }
        }
        default_appearance(self.doc).unwrap_or_default()
    }

    /// The text color parsed from `/DA`, as raw components, defaulting to
    /// `[0,0,0]` (PyMuPDF `Widget.text_color`).
    ///
    /// DEVIATION (oxide is more correct): for a CMYK `/DA` (the `k` operator)
    /// oxide returns the 4 CMYK components, whereas MuPDF/PyMuPDF's `/DA` parser
    /// only handles `g`/`rg` and returns `(0,0,0)` for `k`. Kept intentionally.
    #[must_use]
    pub fn text_color(&self) -> Vec<f64> {
        parse_da_full(&self.da_string()).2
    }

    /// The text font name parsed from `/DA`, defaulting to "Helv" (PyMuPDF
    /// `Widget.text_font`).
    #[must_use]
    pub fn text_font(&self) -> String {
        parse_da_full(&self.da_string()).0
    }

    /// The text font size parsed from `/DA`, defaulting to `0.0` (PyMuPDF
    /// `Widget.text_fontsize`).
    #[must_use]
    pub fn text_fontsize(&self) -> f64 {
        parse_da_full(&self.da_string()).1
    }

    /// The maximum text length `/MaxLen` (inherited), `0` if absent (PyMuPDF
    /// `Widget.text_maxlen`).
    #[must_use]
    pub fn text_maxlen(&self) -> i64 {
        inherited_i64(self.doc, self.obj, "MaxLen").unwrap_or(0)
    }

    /// The text quadding `/Q` (inherited): 0 left, 1 center, 2 right (PyMuPDF
    /// `Widget.text_format`).
    ///
    /// DEVIATION (oxide is more correct): oxide reads the spec-correct `/Q`,
    /// whereas PyMuPDF 1.27's getter is broken — its `pdf_text_widget_format`
    /// never reads `/Q`, so it always returns 0. Kept intentionally.
    #[must_use]
    pub fn text_format(&self) -> i64 {
        inherited_i64(self.doc, self.obj, "Q").unwrap_or(0)
    }

    /// The pushbutton caption `/MK /CA`, or `None` (PyMuPDF
    /// `Widget.button_caption`).
    #[must_use]
    pub fn button_caption(&self) -> Option<String> {
        let d = self.dict()?;
        let mk = resolve_dict(self.doc, d.get(&Name::new("MK"))?)?;
        mk.get(&Name::new("CA"))
            .and_then(Object::as_string)
            .map(decode_text_string)
    }

    /// The field display code derived from the annotation `/F` flags, matching
    /// MuPDF `pdf_field_display` (verified against PyMuPDF 1.27 for `/F` 0..=63):
    /// - Hidden (bit 2) set -> `1` (overrides NoView/Print);
    /// - else NoView (bit 6) set -> `3` when Print (bit 3) also set, else `1`;
    /// - else (neither Hidden nor NoView) -> `0` when Print set, else `2`.
    #[must_use]
    pub fn field_display(&self) -> i64 {
        let f = self
            .dict()
            .and_then(|d| d.get(&Name::new("F")).and_then(Object::as_i64))
            .unwrap_or(0);
        const HIDDEN: i64 = 1 << 1; // 2
        const PRINT: i64 = 1 << 2; // 4
        const NO_VIEW: i64 = 1 << 5; // 32
        if f & HIDDEN != 0 {
            1
        } else if f & NO_VIEW != 0 {
            if f & PRINT != 0 {
                3
            } else {
                1
            }
        } else if f & PRINT != 0 {
            0
        } else {
            2
        }
    }

    /// For signature fields: whether the signature is signed, `None` for
    /// non-signature fields (PyMuPDF `Widget.is_signed`). A signed field has a
    /// `/V` signature *dictionary*; the string getter
    /// [`Field::field_value`] returns `None` for a dict, so we test for the `/V`
    /// key's presence directly (inherited).
    #[must_use]
    pub fn is_signed(&self) -> Option<bool> {
        if self.field_type() != FieldType::Signature {
            return None;
        }
        Some(inherited_value(self.doc, self.obj, "V").is_some())
    }

    /// The current on-state name `/AS` for a button widget (checkbox/radio),
    /// or `None` (PyMuPDF `Widget.on_state` reads the non-`Off` button state).
    #[must_use]
    pub fn on_state(&self) -> Option<String> {
        if !matches!(
            self.field_type(),
            FieldType::CheckBox | FieldType::RadioButton
        ) {
            return None;
        }
        self.button_states().into_iter().find(|s| s != "Off")
    }

    /// The radio-group parent object number `/Parent` (radio buttons only),
    /// or `None` (PyMuPDF `Widget.rb_parent`).
    #[must_use]
    pub fn rb_parent(&self) -> Option<u32> {
        if self.field_type() != FieldType::RadioButton {
            return None;
        }
        self.dict()
            .and_then(|d| d.get(&Name::new("Parent")).and_then(Object::as_reference))
            .map(|r| r.num)
    }

    /// Resets the field to its default value `/DV` (PyMuPDF `Widget.reset`).
    ///
    /// With a `/DV`, `/V` is set to it; without a `/DV`, fitz *clears* `/V`
    /// (removes the key, leaving an empty field value) rather than writing a
    /// literal sentinel.
    ///
    /// # Errors
    /// As [`Field::set_field_value`] / [`Field::clear_value`].
    pub fn reset(&self) -> Result<()> {
        let field = self.field();
        match field.default_value() {
            Some(dv) => field.set_field_value(&dv),
            None => field.clear_value(),
        }
    }

    /// Sets the field value through the owning field (regenerates appearances).
    ///
    /// # Errors
    /// As [`Field::set_field_value`].
    pub fn set_field_value(&self, value: &str) -> Result<()> {
        self.field().set_field_value(value)
    }
}

/// Resolves an object (possibly an indirect reference) into a dict clone.
fn resolve_dict(doc: &DocumentStore, o: &Object) -> Option<Dict> {
    match o {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(r) => doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned()),
        _ => None,
    }
}

/// Parses a `/DA` string into `(font, fontsize, color)` matching PyMuPDF's
/// `Widget._parse_da`: font defaults "Helv", size `0.0`, color `[0,0,0]`; the
/// color follows the `g` (gray) / `rg` (rgb) / `k` (cmyk) operator.
fn parse_da_full(da: &str) -> (String, f64, Vec<f64>) {
    let mut font = "Helv".to_string();
    let mut size = 0.0;
    let mut color = vec![0.0, 0.0, 0.0];
    let toks: Vec<&str> = da.split_whitespace().collect();
    for (i, t) in toks.iter().enumerate() {
        match *t {
            "Tf" if i >= 2 => {
                font = toks[i - 2].trim_start_matches('/').to_string();
                if let Ok(v) = toks[i - 1].parse::<f64>() {
                    size = v;
                }
            }
            "g" if i >= 1 => {
                if let Ok(v) = toks[i - 1].parse::<f64>() {
                    color = vec![v];
                }
            }
            "rg" if i >= 3 => {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    toks[i - 3].parse::<f64>(),
                    toks[i - 2].parse::<f64>(),
                    toks[i - 1].parse::<f64>(),
                ) {
                    color = vec![r, g, b];
                }
            }
            "k" if i >= 4 => {
                if let (Ok(c), Ok(m), Ok(y), Ok(kk)) = (
                    toks[i - 4].parse::<f64>(),
                    toks[i - 3].parse::<f64>(),
                    toks[i - 2].parse::<f64>(),
                    toks[i - 1].parse::<f64>(),
                ) {
                    color = vec![c, m, y, kk];
                }
            }
            _ => {}
        }
    }
    (font, size, color)
}

/// The `/Widget` annotation references on the page at `index` (in `/Annots`
/// order).
#[must_use]
pub fn widget_refs(doc: &DocumentStore, index: usize) -> Vec<ObjRef> {
    let Some(leaf) = pagetree::page_refs(doc).get(index).copied() else {
        return Vec::new();
    };
    annot_refs_on_leaf(doc, leaf)
        .into_iter()
        .filter(|&r| is_widget(doc, r))
        .collect()
}

/// The widget handles on the page at `index`.
#[must_use]
pub fn widgets(doc: &DocumentStore, index: usize) -> Vec<Widget<'_>> {
    widget_refs(doc, index)
        .into_iter()
        .map(|obj| Widget { doc, obj })
        .collect()
}

/// The first widget on the page at `index`, if any.
#[must_use]
pub fn first_widget(doc: &DocumentStore, index: usize) -> Option<Widget<'_>> {
    widgets(doc, index).into_iter().next()
}

fn is_widget(doc: &DocumentStore, r: ObjRef) -> bool {
    doc.resolve(r)
        .ok()
        .and_then(|o| o.as_dict().cloned())
        .and_then(|d| {
            d.get(&Name::new("Subtype"))
                .and_then(Object::as_name)
                .map(|n| n.as_bytes() == b"Widget")
        })
        .unwrap_or(false)
}

fn annot_refs_on_leaf(doc: &DocumentStore, leaf: ObjRef) -> Vec<ObjRef> {
    let Ok(pd) = doc.resolve(leaf) else {
        return Vec::new();
    };
    let Some(pd) = pd.as_dict() else {
        return Vec::new();
    };
    let arr = match pd.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    arr.iter().filter_map(Object::as_reference).collect()
}

// === fill() convenience ===================================================

/// Sets a field's value by fully-qualified name. Returns
/// [`Error::InvalidArgument`] if no such field exists.
///
/// # Errors
/// No matching field, or a per-type set error (read-only, bad value, …).
pub fn fill(doc: &DocumentStore, name: &str, value: &str) -> Result<()> {
    let field = form_fields(doc)
        .into_iter()
        .find(|f| f.field_name() == name)
        .ok_or(Error::InvalidArgument("no form field with that name"))?;
    field.set_field_value(value)
}

// === flatten() ============================================================

/// Flattens the interactive form: draws each widget's current `/AP /N` into its
/// page content as a Form XObject (`Do`) at the widget rect, removes every
/// `/Widget` annotation from each page's `/Annots`, and deletes the catalog
/// `/AcroForm`. The result is a static PDF with filled values baked in and no
/// interactive fields.
///
/// # Errors
/// Propagates resolve / object-edit errors.
pub fn flatten(doc: &DocumentStore) -> Result<()> {
    let pages = pagetree::page_refs(doc);
    for (index, &leaf) in pages.iter().enumerate() {
        let widget_list: Vec<ObjRef> = annot_refs_on_leaf(doc, leaf)
            .into_iter()
            .filter(|&r| is_widget(doc, r))
            .collect();
        if widget_list.is_empty() {
            continue;
        }
        let pc = PageContent::new(doc, index)?;
        // The appearance streams we bake into page content must survive the
        // widget cleanup (the page now references them via `Do`).
        let mut baked: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for w in &widget_list {
            if let Some(n_ref) = bake_widget(doc, &pc, *w)? {
                baked.insert(n_ref.num);
            }
        }
        remove_widgets_from_annots(doc, leaf, &widget_list)?;
        // Free the widget objects and their *unused* appearance streams, keeping
        // the baked ones alive.
        for w in &widget_list {
            free_widget(doc, *w, &baked);
        }
    }
    delete_acroform(doc)
}

/// Draws one widget's `/AP /N` appearance into the page content at its rect, as
/// a Form XObject `Do`. Returns the baked appearance reference (so the caller
/// keeps it alive). `Ok(None)` when the widget has no appearance.
fn bake_widget(doc: &DocumentStore, pc: &PageContent, widget: ObjRef) -> Result<Option<ObjRef>> {
    let wd = doc
        .resolve(widget)?
        .as_dict()
        .cloned()
        .ok_or(Error::InvalidArgument("widget is not a dictionary"))?;
    let rect = read_rect(&wd).normalize();
    let Some(n_ref) = current_ap_n(doc, &wd) else {
        return Ok(None);
    };
    // The AP /N form has its own /BBox and /Matrix; placing it via `Do` requires
    // a CTM mapping the form's BBox into the widget rect (ISO 32000-1 §12.5.5
    // appearance-stream algorithm). Our generated appearances use BBox == rect
    // with identity Matrix, so the mapping is a pure identity — but third-party
    // appearances may differ, so we compute the BBox→rect transform generally.
    let n_obj = doc.resolve(n_ref)?;
    let n_dict = n_obj
        .as_stream()
        .map(|s| s.dict.clone())
        .or_else(|| n_obj.as_dict().cloned())
        .unwrap_or_default();
    let bbox = read_rect_key(&n_dict, "BBox");
    let cm = bbox_to_rect_cm(bbox, rect);
    // Register the form XObject under the page resources.
    let name = pc.add_resource("XObject", "Fm", Object::Reference(n_ref))?;
    let chunk = format!(
        "q\n{} {} {} {} {} {} cm\n/{name} Do\nQ\n",
        fmt_num(cm[0]),
        fmt_num(cm[1]),
        fmt_num(cm[2]),
        fmt_num(cm[3]),
        fmt_num(cm[4]),
        fmt_num(cm[5]),
    );
    pc.append_content(chunk.as_bytes())?;
    Ok(Some(n_ref))
}

/// Computes the CTM placing a form whose `/BBox` is `bbox` (after its own
/// `/Matrix`, which our appearances leave identity) into the target `rect`,
/// scaling/translating to fit. Degenerate BBoxes fall back to identity.
fn bbox_to_rect_cm(bbox: Rect, rect: Rect) -> [f64; 6] {
    let bw = bbox.width();
    let bh = bbox.height();
    if bw.abs() < 1e-6 || bh.abs() < 1e-6 {
        // No usable BBox: translate so the form origin lands at the rect corner.
        return [1.0, 0.0, 0.0, 1.0, rect.x0, rect.y0];
    }
    let sx = rect.width() / bw;
    let sy = rect.height() / bh;
    // Map bbox.x0→rect.x0, bbox.y0→rect.y0 with scale (sx, sy).
    let e = rect.x0 - sx * bbox.x0;
    let f = rect.y0 - sy * bbox.y0;
    [sx, 0.0, 0.0, sy, e, f]
}

/// Removes the given widget references from a page leaf's `/Annots`.
fn remove_widgets_from_annots(doc: &DocumentStore, leaf: ObjRef, widgets: &[ObjRef]) -> Result<()> {
    let mut pd = doc
        .resolve(leaf)?
        .as_dict()
        .cloned()
        .ok_or(Error::InvalidArgument("page is not a dictionary"))?;
    let arr = match pd.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)?
            .as_array()
            .map(<[Object]>::to_vec)
            .unwrap_or_default(),
        _ => return Ok(()),
    };
    let drop: std::collections::HashSet<u32> = widgets.iter().map(|r| r.num).collect();
    let filtered: Vec<Object> = arr
        .into_iter()
        .filter(|o| !matches!(o, Object::Reference(r) if drop.contains(&r.num)))
        .collect();
    if filtered.is_empty() {
        pd.remove(&Name::new("Annots"));
    } else {
        pd.insert(Name::new("Annots"), Object::Array(filtered));
    }
    doc.update_object(leaf, Object::Dictionary(pd))
}

/// Frees a widget object and its `/AP` appearance streams (best-effort), keeping
/// any appearance reference whose object number is in `keep` (baked into content).
fn free_widget(doc: &DocumentStore, widget: ObjRef, keep: &std::collections::HashSet<u32>) {
    if let Ok(obj) = doc.resolve(widget) {
        if let Some(d) = obj.as_dict() {
            free_ap_streams(doc, d, keep);
        }
    }
    let _ = doc.delete_object(widget);
}

/// Frees every appearance stream referenced from a dict's `/AP` (`/N`, `/D`,
/// `/R`, including sub-dictionary on-state entries), except references whose
/// object number is in `keep` (baked into page content and still live).
fn free_ap_streams(doc: &DocumentStore, d: &Dict, keep: &std::collections::HashSet<u32>) {
    let ap = match d.get(&Name::new("AP")) {
        Some(Object::Dictionary(ap)) => ap.clone(),
        Some(Object::Reference(r)) => match doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned())
        {
            Some(ap) => ap,
            None => return,
        },
        _ => return,
    };
    for v in ap.values() {
        match v {
            Object::Reference(r) if !keep.contains(&r.num) => {
                let _ = doc.delete_object(*r);
            }
            Object::Dictionary(states) => {
                for sv in states.values() {
                    if let Some(r) = sv.as_reference() {
                        if !keep.contains(&r.num) {
                            let _ = doc.delete_object(r);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Deletes the catalog `/AcroForm` (and frees an indirect AcroForm object).
fn delete_acroform(doc: &DocumentStore) -> Result<()> {
    let Some(root) = doc.root() else {
        return Ok(());
    };
    let mut catalog = doc
        .resolve(root)?
        .as_dict()
        .cloned()
        .ok_or(Error::InvalidArgument("catalog is not a dictionary"))?;
    let af_ref = acroform_ref(doc);
    if catalog.remove(&Name::new("AcroForm")).is_some() {
        doc.update_object(root, Object::Dictionary(catalog))?;
    }
    if let Some(r) = af_ref {
        let _ = doc.delete_object(r);
    }
    Ok(())
}

// === shared read helpers ==================================================

/// The `/AP /N` reference of a dict, whether `/AP` is direct or indirect, and
/// whether `/N` is a stream reference (text/choice) — for checkbox/radio the
/// `/N` is a sub-dict of on-state streams (handled separately).
fn existing_ap_n(doc: &DocumentStore, d: &Dict) -> Option<ObjRef> {
    let ap = match d.get(&Name::new("AP"))? {
        Object::Dictionary(ap) => ap.clone(),
        Object::Reference(r) => doc.resolve(*r).ok()?.as_dict().cloned()?,
        _ => return None,
    };
    ap.get(&Name::new("N")).and_then(Object::as_reference)
}

/// The `/AP /N` form-XObject reference to *bake* for flatten: for text/choice
/// it's the `/N` stream; for checkbox/radio it's the on-state sub-dict entry
/// matching the widget's current `/AS`.
fn current_ap_n(doc: &DocumentStore, d: &Dict) -> Option<ObjRef> {
    let ap = match d.get(&Name::new("AP"))? {
        Object::Dictionary(ap) => ap.clone(),
        Object::Reference(r) => doc.resolve(*r).ok()?.as_dict().cloned()?,
        _ => return None,
    };
    match ap.get(&Name::new("N"))? {
        // Text / choice: a single appearance stream.
        Object::Reference(r) => Some(*r),
        // Checkbox / radio: pick the stream for the current /AS state.
        Object::Dictionary(states) => {
            let as_name = d
                .get(&Name::new("AS"))
                .and_then(Object::as_name)
                .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
                .unwrap_or_else(|| "Off".to_string());
            states
                .get(&Name::new(&as_name))
                .and_then(Object::as_reference)
                .or_else(|| {
                    // Fall back to the first available state stream.
                    states.values().find_map(Object::as_reference)
                })
        }
        _ => None,
    }
}

/// The single on-state name (a non-`Off` key) of a widget's `/AP /N` state
/// sub-dictionary, if discoverable.
fn on_state_name(doc: &DocumentStore, widget: ObjRef) -> Option<String> {
    let d = doc.resolve(widget).ok()?.as_dict().cloned()?;
    ap_n_state_names(doc, &d).into_iter().next()
}

/// All non-`Off` state names of a dict's `/AP /N` (checkbox/radio on-states).
fn ap_n_state_names(doc: &DocumentStore, d: &Dict) -> Vec<String> {
    let ap = match d.get(&Name::new("AP")) {
        Some(Object::Dictionary(ap)) => ap.clone(),
        Some(Object::Reference(r)) => match doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned())
        {
            Some(ap) => ap,
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let states = match ap.get(&Name::new("N")) {
        Some(Object::Dictionary(s)) => s.clone(),
        Some(Object::Reference(r)) => match doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned())
        {
            Some(s) => s,
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    states
        .keys()
        .filter(|k| k.as_bytes() != b"Off")
        .map(|k| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect()
}

/// Reads `/Opt` (choice options) into export-value strings.
fn read_opt(doc: &DocumentStore, d: &Dict) -> Vec<String> {
    let arr = match d.get(&Name::new("Opt")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec))
            .unwrap_or_default(),
        _ => return Vec::new(),
    };
    arr.iter()
        .map(|item| match item {
            Object::String(s) => decode_text_string(s),
            // `[export display]` pair → export value (element 0).
            Object::Array(pair) => pair
                .first()
                .and_then(Object::as_string)
                .map(decode_text_string)
                .unwrap_or_default(),
            _ => String::new(),
        })
        .collect()
}

/// Inherited `/Name` value by key, walking `/Parent` (depth-capped).
fn inherited_name(doc: &DocumentStore, start: ObjRef, key: &str) -> Option<Vec<u8>> {
    inherited_value(doc, start, key).and_then(|o| o.as_name().map(|n| n.as_bytes().to_vec()))
}

/// Inherited integer value by key, walking `/Parent`.
fn inherited_i64(doc: &DocumentStore, start: ObjRef, key: &str) -> Option<i64> {
    inherited_value(doc, start, key).and_then(|o| o.as_i64())
}

/// The nearest definition of `key` on `start` or up its `/Parent` chain
/// (depth-capped, cycle-safe). Returns a cloned [`Object`].
fn inherited_value(doc: &DocumentStore, start: ObjRef, key: &str) -> Option<Object> {
    let mut cur = Some(start);
    let mut seen = std::collections::HashSet::new();
    let mut depth = 0;
    while let Some(r) = cur {
        if depth > 50 || !seen.insert(r.num) {
            break;
        }
        depth += 1;
        let d = doc.resolve(r).ok()?.as_dict().cloned()?;
        if let Some(v) = d.get(&Name::new(key)) {
            // Resolve a one-level indirect value for convenience.
            return match v {
                Object::Reference(rr) => doc.resolve(*rr).ok().map(|o| (*o).clone()),
                other => Some(other.clone()),
            };
        }
        cur = d.get(&Name::new("Parent")).and_then(Object::as_reference);
    }
    None
}

/// The fully-qualified field name: `/T` values joined by `.` from root → field.
fn fully_qualified_name(doc: &DocumentStore, start: ObjRef) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = Some(start);
    let mut seen = std::collections::HashSet::new();
    let mut depth = 0;
    while let Some(r) = cur {
        if depth > 50 || !seen.insert(r.num) {
            break;
        }
        depth += 1;
        let Some(d) = doc.resolve(r).ok().and_then(|o| o.as_dict().cloned()) else {
            break;
        };
        if let Some(t) = d.get(&Name::new("T")).and_then(Object::as_string) {
            parts.push(decode_text_string(t));
        }
        cur = d.get(&Name::new("Parent")).and_then(Object::as_reference);
    }
    parts.reverse();
    parts.join(".")
}

/// Converts a `/V` / `/DV` value object to a display string.
fn value_to_string(v: Object) -> Option<String> {
    match v {
        Object::String(s) => Some(decode_text_string(&s)),
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        Object::Array(a) => a.first().and_then(|o| value_to_string(o.clone())),
        _ => None,
    }
}

/// Whether a checkbox set-value string means "checked".
fn is_truthy(value: &str) -> bool {
    !matches!(
        value.to_ascii_lowercase().as_str(),
        "" | "off" | "false" | "no" | "0"
    )
}

fn read_rect(d: &Dict) -> Rect {
    read_rect_key(d, "Rect")
}

fn read_rect_key(d: &Dict, key: &str) -> Rect {
    match d.get(&Name::new(key)).and_then(Object::as_array) {
        Some(a) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
            Rect::new(v[0], v[1], v[2], v[3])
        }
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

/// Parses a `/DA` string into `(fontsize, color)`. Font size `0` (auto) is left
/// as-is for the caller to resolve; color defaults to black.
fn parse_da(da: &str) -> (f64, Color) {
    let toks: Vec<&str> = da.split_whitespace().collect();
    let mut size = 0.0;
    let mut color = Color::BLACK;
    for w in toks.windows(2) {
        if w[1] == "Tf" {
            if let Ok(v) = w[0].parse::<f64>() {
                size = v;
            }
        }
    }
    // Color: "g" (gray, 1 operand) or "rg" (rgb, 3 operands).
    for (i, t) in toks.iter().enumerate() {
        match *t {
            "g" if i >= 1 => {
                if let Ok(v) = toks[i - 1].parse::<f64>() {
                    color = Color::new(v, v, v);
                }
            }
            "rg" if i >= 3 => {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    toks[i - 3].parse::<f64>(),
                    toks[i - 2].parse::<f64>(),
                    toks[i - 1].parse::<f64>(),
                ) {
                    color = Color::new(r, g, b);
                }
            }
            _ => {}
        }
    }
    (size, color)
}

// === text-field appearance generation =====================================

/// Builds a text-/choice-field `/AP /N` Form XObject drawing `value` inside
/// `rect`, honoring `/Q` alignment (0 left, 1 center, 2 right) and the multiline
/// flag. Font size `0` (auto) is resolved to a size that fits the rect height.
fn build_text_field_ap(
    rect: Rect,
    value: &str,
    fontsize: f64,
    color: Color,
    q: i64,
    multiline: bool,
) -> StreamObj {
    let w = rect.width();
    let h = rect.height();
    let pad = 2.0;
    // Resolve auto font size.
    let size = if fontsize > 0.0 {
        fontsize
    } else if multiline {
        12.0
    } else {
        (h - 2.0 * pad).clamp(4.0, 12.0)
    };

    let mut body = Vec::new();
    // /Tx BMC marked content (Acrobat convention) + clip to the rect.
    body.extend_from_slice(b"/Tx BMC\nq\n");
    body.extend_from_slice(
        format!(
            "0 0 {} {} re\nW\nn\n",
            fmt_num(w.max(0.0)),
            fmt_num(h.max(0.0))
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"BT\n");
    body.extend_from_slice(format!("{}\n", color.fill_op()).as_bytes());
    body.extend_from_slice(format!("/Helv {} Tf\n", fmt_num(size)).as_bytes());

    let lines: Vec<&str> = if multiline {
        value.split('\n').collect()
    } else {
        vec![value]
    };
    let leading = size * 1.15;
    // First baseline: multiline starts near the top; single-line is vertically
    // centered.
    let mut y = if multiline {
        h - pad - size
    } else {
        (h - size) / 2.0 + size * 0.18
    };
    for line in &lines {
        let text_w = std_widths::string_advance("Helvetica", line, size);
        let x = match q {
            1 => ((w - text_w) / 2.0).max(pad), // center
            2 => (w - text_w - pad).max(pad),   // right
            _ => pad,                           // left (default)
        };
        body.extend_from_slice(format!("1 0 0 1 {} {} Tm\n", fmt_num(x), fmt_num(y)).as_bytes());
        body.extend_from_slice(b"(");
        body.extend_from_slice(&escape_pdf_literal(winansi_bytes(line).as_slice()));
        body.extend_from_slice(b") Tj\n");
        y -= leading;
    }
    body.extend_from_slice(b"ET\nQ\nEMC\n");

    // The form's coordinate frame is the rect translated to the origin, so /BBox
    // is `[0 0 w h]` and the placement CTM (flatten / viewer) maps it into the
    // rect. /Matrix identity.
    let mut dict = Dict::new();
    dict.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    dict.insert(Name::new("Subtype"), Object::Name(Name::new("Form")));
    dict.insert(Name::new("FormType"), Object::Integer(1));
    dict.insert(
        Name::new("BBox"),
        Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(w),
            Object::Real(h),
        ]),
    );
    dict.insert(
        Name::new("Matrix"),
        Object::Array(vec![
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    // Helvetica resource for the /DA font.
    let mut font = Dict::new();
    font.insert(Name::new("Type"), Object::Name(Name::new("Font")));
    font.insert(Name::new("Subtype"), Object::Name(Name::new("Type1")));
    font.insert(Name::new("BaseFont"), Object::Name(Name::new("Helvetica")));
    font.insert(
        Name::new("Encoding"),
        Object::Name(Name::new("WinAnsiEncoding")),
    );
    let mut fonts = Dict::new();
    fonts.insert(Name::new("Helv"), Object::Dictionary(font));
    let mut resources = Dict::new();
    resources.insert(Name::new("Font"), Object::Dictionary(fonts));
    dict.insert(Name::new("Resources"), Object::Dictionary(resources));
    dict.insert(Name::new("Length"), Object::Integer(body.len() as i64));
    StreamObj::new_encoded(dict, body)
}

/// Encodes `text` to WinAnsi bytes for a Base-14 `Tj` operand (mirrors the
/// `text` module's mapping; non-representable chars degrade to `?`).
fn winansi_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|ch| {
            let cp = ch as u32;
            if (0x20..=0x7e).contains(&cp) || (0xa0..=0xff).contains(&cp) {
                cp as u8
            } else {
                b'?'
            }
        })
        .collect()
}

// === string codec (shared shape with annot.rs) ============================

/// Builds a PDF text string object (ASCII → literal; non-ASCII → UTF-16BE BOM).
fn text_string(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(PdfString {
            bytes: s.as_bytes().to_vec(),
            kind: StringKind::Literal,
        })
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(PdfString {
            bytes,
            kind: StringKind::Literal,
        })
    }
}

/// Decodes a PDF text string (UTF-16BE with BOM, else PDFDocEncoding/Latin-1).
fn decode_text_string(s: &PdfString) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let units: Vec<u16> = b[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        b.iter().map(|&c| c as char).collect()
    }
}
