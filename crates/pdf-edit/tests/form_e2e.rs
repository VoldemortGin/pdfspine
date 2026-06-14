//! M4c — AcroForm forms (read / fill / flatten) + `Widget` API end-to-end tests
//! (PRD §8.8, §12 M4). Self-built AcroForm fixtures; reparse + `qpdf --check`
//! oracle. Catalog IDs `FORM-*` / `WIDGET-*`.

mod common;

use common::*;
use pdf_core::object::{Name, Object};
use pdf_edit::form::{self, FieldType};

// === FORM-READ-* ==========================================================

#[test]
fn form_read_001_enumerates_all_fields() {
    // FORM-READ-001: all terminal fields enumerated (incl. radio kids' parent).
    let doc = open(&acroform_doc());
    let fields = form::form_fields(&doc);
    let names: Vec<String> = fields.iter().map(form::Field::field_name).collect();
    // tx1, cb1, rg1 (the radio group is terminal — its kids are widgets), ch1.
    assert!(names.contains(&"tx1".to_string()), "names={names:?}");
    assert!(names.contains(&"cb1".to_string()), "names={names:?}");
    assert!(names.contains(&"rg1".to_string()), "names={names:?}");
    assert!(names.contains(&"ch1".to_string()), "names={names:?}");
    assert_eq!(fields.len(), 4, "names={names:?}");
}

#[test]
fn form_read_002_fully_qualified_name() {
    // FORM-READ-002: FQN joins /T up the /Parent chain with `.`.
    let doc = open(&acroform_hierarchical_doc());
    let fields = form::form_fields(&doc);
    let names: Vec<String> = fields.iter().map(form::Field::field_name).collect();
    assert_eq!(names, vec!["addr.city".to_string()], "names={names:?}");
}

#[test]
fn form_read_003_field_type_detection() {
    let doc = open(&acroform_doc());
    let by_name = |n: &str| {
        form::form_fields(&doc)
            .into_iter()
            .find(|f| f.field_name() == n)
            .unwrap()
            .field_type()
    };
    assert_eq!(by_name("tx1"), FieldType::Text);
    assert_eq!(by_name("cb1"), FieldType::CheckBox);
    assert_eq!(by_name("rg1"), FieldType::RadioButton);
    assert_eq!(by_name("ch1"), FieldType::ComboBox);
}

#[test]
fn form_read_004_button_subtypes() {
    // FORM-READ-004: checkbox vs radio (32768) vs pushbutton (65536).
    assert_eq!(FieldType::classify(Some(b"Btn"), 0), FieldType::CheckBox);
    assert_eq!(
        FieldType::classify(Some(b"Btn"), 32768),
        FieldType::RadioButton
    );
    assert_eq!(
        FieldType::classify(Some(b"Btn"), 65536),
        FieldType::PushButton
    );
}

#[test]
fn form_read_005_choice_subtypes() {
    assert_eq!(FieldType::classify(Some(b"Ch"), 0), FieldType::ListBox);
    assert_eq!(
        FieldType::classify(Some(b"Ch"), 131072),
        FieldType::ComboBox
    );
    assert_eq!(FieldType::classify(Some(b"Sig"), 0), FieldType::Signature);
    assert_eq!(FieldType::classify(None, 0), FieldType::Unknown);
}

#[test]
fn form_read_006_value_default_flags() {
    let doc = open(&acroform_doc());
    let tx = form::form_fields(&doc)
        .into_iter()
        .find(|f| f.field_name() == "tx1")
        .unwrap();
    assert_eq!(tx.field_value().as_deref(), Some("init"));
    assert_eq!(tx.field_flags(), 0);
    let rg = form::form_fields(&doc)
        .into_iter()
        .find(|f| f.field_name() == "rg1")
        .unwrap();
    assert_eq!(rg.field_flags(), 32768);
}

#[test]
fn form_read_007_acroform_level_attrs() {
    let doc = open(&acroform_doc());
    assert!(!form::need_appearances(&doc));
    assert_eq!(
        form::default_appearance(&doc).as_deref(),
        Some("0 0 0 rg /Helv 12 Tf")
    );
    let af = form::acroform_dict(&doc).unwrap();
    assert!(af.contains_key(&Name::new("DR")));
}

// === WIDGET-* =============================================================

#[test]
fn widget_001_page_widgets_iterator() {
    // WIDGET-001: page.widgets() returns only /Widget annots; first_widget works.
    let doc = open(&acroform_doc());
    let ws = form::widgets(&doc, 0);
    // 11(tx1), 12(cb1), 16(radio A), 17(radio B), 18(combo).
    assert_eq!(ws.len(), 5);
    assert!(form::first_widget(&doc, 0).is_some());
}

#[test]
fn widget_002_basic_accessors() {
    let doc = open(&acroform_doc());
    let tx = form::widgets(&doc, 0)
        .into_iter()
        .find(|w| w.field_name() == "tx1")
        .unwrap();
    assert_eq!(tx.field_type(), FieldType::Text);
    assert_eq!(tx.field_type_string(), "Text");
    assert_eq!(tx.field_value().as_deref(), Some("init"));
    assert_eq!(tx.field_flags(), 0);
    let r = tx.rect();
    assert!((r.x0 - 72.0).abs() < 1e-6 && (r.x1 - 272.0).abs() < 1e-6);
    assert_eq!(tx.xref().num, 11);
}

#[test]
fn widget_003_label_choices_button_states() {
    let doc = open(&acroform_doc());
    // Label /TU.
    let tx = form::widgets(&doc, 0)
        .into_iter()
        .find(|w| w.field_name() == "tx1")
        .unwrap();
    assert_eq!(tx.field_label().as_deref(), Some("Text One"));
    // Choice values from the combo's /Opt.
    let combo = form::widgets(&doc, 0)
        .into_iter()
        .find(|w| w.field_name() == "ch1")
        .unwrap();
    assert_eq!(combo.choice_values(), vec!["alpha", "beta", "gamma"]);
    // Button states from the checkbox /AP /N keys (on-state discovered, not Yes).
    let cb = form::widgets(&doc, 0)
        .into_iter()
        .find(|w| w.field_name() == "cb1")
        .unwrap();
    assert_eq!(cb.button_states(), vec!["On".to_string()]);
}

#[test]
fn widget_004_is_form_pdf() {
    let form_doc = open(&acroform_doc());
    assert!(form::is_form_pdf(&form_doc));
    let plain = open(&blank_page(612, 792));
    assert!(!form::is_form_pdf(&plain));
}

// === FORM-TEXT-* ==========================================================

#[test]
fn form_text_001_set_value_persists() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "tx1", "Hello World").unwrap();
    // Reopen → /V persists.
    let re = save_reopen(&doc);
    let tx = form::form_fields(&re)
        .into_iter()
        .find(|f| f.field_name() == "tx1")
        .unwrap();
    assert_eq!(tx.field_value().as_deref(), Some("Hello World"));
}

#[test]
fn form_text_002_ap_regenerated_with_text() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "tx1", "Bonjour").unwrap();
    let re = save_reopen(&doc);
    let wd = obj_dict(&re, 11);
    let ap = widget_ap_n_bytes(&re, &wd);
    let s = String::from_utf8_lossy(&ap);
    assert!(s.contains("(Bonjour) Tj"), "AP body: {s}");
    assert!(s.contains("/Helv"), "AP body should select /Helv: {s}");
}

#[test]
fn form_text_003_alignment_in_ap() {
    // FORM-TEXT-003: /Q alignment changes the Tm x. Right-align (2) places the
    // text further right than left-align (0).
    let left_x = ap_text_x(0);
    let right_x = ap_text_x(2);
    let center_x = ap_text_x(1);
    assert!(right_x > center_x, "right {right_x} > center {center_x}");
    assert!(center_x > left_x, "center {center_x} > left {left_x}");
}

/// Helper: fill tx1 with `/Q == q` and return the `Tm` x of the drawn text.
fn ap_text_x(q: i64) -> f64 {
    let doc = open(&acroform_doc());
    // Patch the text field's /Q.
    let mut d = obj_dict(&doc, 11);
    d.insert(Name::new("Q"), Object::Integer(q));
    doc.update_object(pdf_core::object::ObjRef::new(11, 0), Object::Dictionary(d))
        .unwrap();
    form::fill(&doc, "tx1", "RR").unwrap();
    let wd = obj_dict(&doc, 11);
    let ap = widget_ap_n_bytes(&doc, &wd);
    let s = String::from_utf8_lossy(&ap);
    // Find "1 0 0 1 <x> <y> Tm".
    for line in s.lines() {
        if line.ends_with("Tm") {
            let toks: Vec<&str> = line.split_whitespace().collect();
            if toks.len() >= 6 {
                return toks[4].parse::<f64>().unwrap_or(0.0);
            }
        }
    }
    panic!("no Tm in AP: {s}");
}

#[test]
fn form_text_004_multiline() {
    // FORM-TEXT-004: multiline flag (4096) → multiple Tj lines.
    let doc = open(&acroform_doc());
    let mut d = obj_dict(&doc, 11);
    d.insert(Name::new("Ff"), Object::Integer(4096));
    // Give the field some height so multiline lines fit.
    d.insert(
        Name::new("Rect"),
        Object::Array(vec![
            Object::Real(72.0),
            Object::Real(640.0),
            Object::Real(272.0),
            Object::Real(720.0),
        ]),
    );
    doc.update_object(pdf_core::object::ObjRef::new(11, 0), Object::Dictionary(d))
        .unwrap();
    form::fill(&doc, "tx1", "line1\nline2\nline3").unwrap();
    let wd = obj_dict(&doc, 11);
    let ap = widget_ap_n_bytes(&doc, &wd);
    let s = String::from_utf8_lossy(&ap);
    let tj = s.matches(" Tj").count();
    assert_eq!(tj, 3, "expected 3 Tj lines: {s}");
}

// === FORM-CHECK-* =========================================================

#[test]
fn form_check_001_check_uses_discovered_on_state() {
    // FORM-CHECK-001: checking sets /V + /AS to the on-state from /AP /N (On),
    // not the assumed /Yes.
    let doc = open(&acroform_doc());
    form::fill(&doc, "cb1", "On").unwrap();
    let re = save_reopen(&doc);
    assert_eq!(name_value(&re, 12, "V"), "On");
    assert_eq!(name_value(&re, 12, "AS"), "On");
}

#[test]
fn form_check_001b_generic_truthy_resolves_to_on() {
    // A generic truthy token also resolves to the discovered on-state.
    let doc = open(&acroform_doc());
    form::fill(&doc, "cb1", "true").unwrap();
    assert_eq!(name_value(&doc, 12, "AS"), "On");
}

#[test]
fn form_check_002_uncheck_off() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "cb1", "On").unwrap();
    form::fill(&doc, "cb1", "Off").unwrap();
    let re = save_reopen(&doc);
    assert_eq!(name_value(&re, 12, "V"), "Off");
    assert_eq!(name_value(&re, 12, "AS"), "Off");
}

// === FORM-RADIO-* =========================================================

#[test]
fn form_radio_001_select_one_kid() {
    // FORM-RADIO-001: select on-state "A" → group /V == A; kid 16 /AS == A;
    // kid 17 /AS == Off. Then switch to B and verify the flip.
    let doc = open(&acroform_doc());
    form::fill(&doc, "rg1", "A").unwrap();
    let re = save_reopen(&doc);
    assert_eq!(name_value(&re, 15, "V"), "A");
    assert_eq!(name_value(&re, 16, "AS"), "A");
    assert_eq!(name_value(&re, 17, "AS"), "Off");

    form::fill(&doc, "rg1", "B").unwrap();
    let re2 = save_reopen(&doc);
    assert_eq!(name_value(&re2, 15, "V"), "B");
    assert_eq!(name_value(&re2, 16, "AS"), "Off");
    assert_eq!(name_value(&re2, 17, "AS"), "B");
}

// === FORM-CHOICE-* ========================================================

#[test]
fn form_choice_001_combo_value_and_ap() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "ch1", "beta").unwrap();
    let re = save_reopen(&doc);
    let combo = form::form_fields(&re)
        .into_iter()
        .find(|f| f.field_name() == "ch1")
        .unwrap();
    assert_eq!(combo.field_value().as_deref(), Some("beta"));
    assert_eq!(combo.choice_values(), vec!["alpha", "beta", "gamma"]);
    let wd = obj_dict(&re, 18);
    let ap = widget_ap_n_bytes(&re, &wd);
    assert!(String::from_utf8_lossy(&ap).contains("(beta) Tj"));
}

#[test]
fn form_choice_001b_value_out_of_domain_rejected() {
    let doc = open(&acroform_doc());
    let err = form::fill(&doc, "ch1", "delta");
    assert!(err.is_err(), "out-of-domain choice value must be rejected");
    // /V unchanged (still absent/None).
    let combo = form::form_fields(&doc)
        .into_iter()
        .find(|f| f.field_name() == "ch1")
        .unwrap();
    assert_eq!(combo.field_value(), None);
}

#[test]
fn form_choice_002_listbox_persists() {
    // FORM-CHOICE-002: a list box (no combo flag) also accepts /V + persists.
    let doc = open(&acroform_doc());
    // Turn the combo into a list box by clearing the combo flag.
    let mut d = obj_dict(&doc, 18);
    d.insert(Name::new("Ff"), Object::Integer(0));
    doc.update_object(pdf_core::object::ObjRef::new(18, 0), Object::Dictionary(d))
        .unwrap();
    form::fill(&doc, "ch1", "gamma").unwrap();
    let re = save_reopen(&doc);
    let combo = form::form_fields(&re)
        .into_iter()
        .find(|f| f.field_name() == "ch1")
        .unwrap();
    assert_eq!(combo.field_type(), FieldType::ListBox);
    assert_eq!(combo.field_value().as_deref(), Some("gamma"));
}

// === FORM-FLATTEN-* =======================================================

#[test]
fn form_flatten_001_removes_acroform_and_widgets() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "tx1", "Flat Value").unwrap();
    form::flatten(&doc).unwrap();
    let re = save_reopen(&doc);
    assert!(!catalog_has_acroform(&re), "/AcroForm must be gone");
    assert_eq!(count_widgets(&re), 0, "no /Widget annots remain");
    // Page /Annots is empty / absent.
    assert!(annot_dicts(&re, 0).is_empty());
}

#[test]
fn form_flatten_002_value_baked_into_content() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "tx1", "BakedText").unwrap();
    form::flatten(&doc).unwrap();
    // The page content must reference the appearance form via `Do`.
    let content = String::from_utf8_lossy(&page_content_bytes(&doc, 0)).into_owned();
    assert!(
        content.contains("Do"),
        "flattened content has no Do: {content}"
    );
    // And the baked appearance text is reachable in the decompressed corpus.
    let bytes = save_bytes(&doc);
    let corpus = decompress_corpus(&bytes);
    assert!(
        corpus
            .windows(b"BakedText".len())
            .any(|w| w == b"BakedText"),
        "baked value not found in decompressed corpus"
    );
}

#[test]
fn form_flatten_003_qpdf_clean_and_reopens() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "tx1", "QV").unwrap();
    form::fill(&doc, "cb1", "On").unwrap();
    form::fill(&doc, "rg1", "A").unwrap();
    form::flatten(&doc).unwrap();
    let bytes = save_bytes(&doc);
    // Reopens valid.
    let re = open(&bytes);
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
    // qpdf clean (skipped if absent).
    if let Some(ok) = qpdf_check(&bytes) {
        assert!(ok, "qpdf --check failed on flattened form");
    }
}

// === FORM-PROP-* ==========================================================

#[test]
fn form_prop_001_non_form_pdf() {
    let doc = open(&blank_page(612, 792));
    assert!(!form::is_form_pdf(&doc));
    assert!(form::form_fields(&doc).is_empty());
    assert!(form::widgets(&doc, 0).is_empty());
    assert!(form::first_widget(&doc, 0).is_none());
    // flatten on a non-form is a clean no-op.
    form::flatten(&doc).unwrap();
}

#[test]
fn form_prop_002_read_only_rejected() {
    let doc = open(&acroform_doc());
    // Mark the text field read-only.
    let mut d = obj_dict(&doc, 11);
    d.insert(Name::new("Ff"), Object::Integer(1));
    doc.update_object(pdf_core::object::ObjRef::new(11, 0), Object::Dictionary(d))
        .unwrap();
    let err = form::fill(&doc, "tx1", "nope");
    assert!(err.is_err(), "read-only set must error");
    // Value unchanged.
    let tx = form::form_fields(&doc)
        .into_iter()
        .find(|f| f.field_name() == "tx1")
        .unwrap();
    assert_eq!(tx.field_value().as_deref(), Some("init"));
}

#[test]
fn form_prop_003_degenerate_dicts_no_panic() {
    // A field dict missing /FT, /Rect, /AP must not panic on read or set.
    let doc = open(&acroform_doc());
    // Strip the text field down to almost nothing but keep it in /Fields.
    let mut d = pdf_core::object::Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("Annot")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new("Widget")));
    d.insert(Name::new("T"), Object::Name(Name::new("broken")));
    doc.update_object(pdf_core::object::ObjRef::new(11, 0), Object::Dictionary(d))
        .unwrap();
    // Reads must not panic.
    let _ = form::form_fields(&doc)
        .iter()
        .map(|f| (f.field_type(), f.field_name(), f.field_value()))
        .collect::<Vec<_>>();
    // Setting a value on a field with no /FT errors cleanly (Unknown type).
    let f = form::form_fields(&doc)
        .into_iter()
        .find(|f| f.xref().num == 11)
        .unwrap();
    assert!(f.set_field_value("x").is_err());
}

#[test]
fn form_prop_004_qpdf_filled() {
    let doc = open(&acroform_doc());
    form::fill(&doc, "tx1", "Filled").unwrap();
    form::fill(&doc, "ch1", "alpha").unwrap();
    let bytes = save_bytes(&doc);
    let re = open(&bytes);
    // Filled form still a form, value present.
    assert!(form::is_form_pdf(&re));
    let tx = form::form_fields(&re)
        .into_iter()
        .find(|f| f.field_name() == "tx1")
        .unwrap();
    assert_eq!(tx.field_value().as_deref(), Some("Filled"));
    if let Some(ok) = qpdf_check(&bytes) {
        assert!(ok, "qpdf --check failed on filled form");
    }
}
