//! M3a save robustness / determinism — `SAVE-PROP-*` (PRD §8.7).
//!
//! Includes an optional `qpdf --check` over a saved file when `qpdf` is on
//! `PATH`; it is skipped cleanly otherwise (the primary oracle is our reparse).

mod common;

use common::simple_doc;

use pdf_core::object::Name;
use pdf_core::{DocumentStore, Limits, Object, SaveOptions, XrefStyle};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("opens")
}

/// `SAVE-PROP-001`: `Table, deflate=false` is deterministic for the same input +
/// options (two saves of the same opened doc produce identical bytes).
#[test]
fn save_prop_001_deterministic() {
    let doc = open(&simple_doc());
    let opts = SaveOptions::default();
    let a = doc.save_to_vec(&opts).unwrap();
    let b = doc.save_to_vec(&opts).unwrap();
    assert_eq!(a, b, "same input+options ⇒ identical bytes");
}

/// `SAVE-PROP-002`: save never panics on a freshly-opened simple doc (both
/// styles, with and without deflate).
#[test]
fn save_prop_002_never_panics() {
    let doc = open(&simple_doc());
    for style in [XrefStyle::Table, XrefStyle::Stream] {
        for deflate in [false, true] {
            let opts = SaveOptions::default()
                .with_xref_style(style)
                .with_deflate(deflate);
            let bytes = doc.save_to_vec(&opts).expect("save succeeds");
            // And reopens.
            let re = open(&bytes);
            assert_eq!(pdf_core::pagetree::page_count(&re), 1);
        }
    }
}

/// `SAVE-PROP-003`: the first `/ID` element is stable per doc; the second varies
/// per save (different body ⇒ different second id).
#[test]
fn save_prop_003_id_scheme() {
    let doc = open(&simple_doc());

    // Two saves of the *same* doc: identical bytes ⇒ identical /ID (both ids
    // stable). Mutate between saves to force a different body.
    let first = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let id_a = id_of(&first);

    // A second doc opened from the original keeps the same first id (derived from
    // the source) but, after an edit, gets a different second id.
    let doc2 = open(&simple_doc());
    doc2.update_object(pdf_core::ObjRef::new(5, 0), Object::Integer(123))
        .unwrap();
    let second = doc2.save_to_vec(&SaveOptions::default()).unwrap();
    let id_b = id_of(&second);

    assert_eq!(id_a.0, id_b.0, "first /ID element stable per document");
    assert_ne!(
        id_a.1, id_b.1,
        "second /ID element varies when the body changes"
    );
}

/// `SAVE-PROP-004`: optional `qpdf --check` passes on a saved file. Skipped
/// cleanly when `qpdf` is not installed.
#[test]
fn save_prop_004_optional_qpdf_check() {
    let doc = open(&simple_doc());
    let bytes = doc.save_to_vec(&SaveOptions::default()).unwrap();

    if !qpdf_available() {
        eprintln!("SAVE-PROP-004: qpdf not on PATH — skipping --check oracle");
        return;
    }

    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "oxide-pdf_save_prop_004_{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, &bytes).unwrap();

    let out = std::process::Command::new("qpdf")
        .arg("--check")
        .arg(&path)
        .output()
        .expect("run qpdf");
    let _ = std::fs::remove_file(&path);

    assert!(
        out.status.success(),
        "qpdf --check failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Extracts the two `/ID` elements (as byte vecs) from a saved PDF.
fn id_of(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let doc = open(bytes);
    let id = doc.trailer().get(&Name::new("ID")).unwrap();
    let arr = id.as_array().unwrap();
    (
        arr[0].as_string().unwrap().bytes.clone(),
        arr[1].as_string().unwrap().bytes.clone(),
    )
}

/// Whether `qpdf` is callable.
fn qpdf_available() -> bool {
    std::process::Command::new("qpdf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
