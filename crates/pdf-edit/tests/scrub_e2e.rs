//! M4e scrub / bake — `SCRUB-*` and `BAKE-*` (PRD §8.8, PyMuPDF parity).

mod common;

use common::{
    acroform_doc, catalog_has_acroform, decompress_corpus, open, save_bytes, save_reopen, MultiPage,
};

use pdf_core::object::Name;
use pdf_core::{DocumentStore, Object};
use pdf_edit::{bake, scrub, set_metadata, ScrubOptions};

// === helpers ==============================================================

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The catalog dict of `doc`, resolved through the overlay.
fn catalog(doc: &DocumentStore) -> pdf_core::Dict {
    let root = doc.root().unwrap();
    doc.resolve(root).unwrap().as_dict().cloned().unwrap()
}

/// Whether the catalog has a given key.
fn catalog_has(doc: &DocumentStore, key: &str) -> bool {
    catalog(doc).contains_key(&Name::new(key))
}

/// The resolved catalog `/Names` dict (or empty).
fn names_dict(doc: &DocumentStore) -> pdf_core::Dict {
    match catalog(doc).get(&Name::new("Names")) {
        Some(Object::Dictionary(d)) => Some(d.clone()),
        Some(Object::Reference(r)) => doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned()),
        _ => None,
    }
    .unwrap_or_default()
}

/// Inserts `key -> value` into the catalog dict (object-edit on the root).
fn set_catalog_key(doc: &DocumentStore, key: &str, value: Object) {
    let root = doc.root().unwrap();
    let mut cat = catalog(doc);
    cat.insert(Name::new(key), value);
    doc.update_object(root, Object::Dictionary(cat)).unwrap();
}

/// Builds a `Dict` from `(key, value)` pairs.
fn dict(pairs: &[(&str, Object)]) -> pdf_core::Dict {
    let mut d = pdf_core::Dict::new();
    for (k, v) in pairs {
        d.insert(Name::new(k), v.clone());
    }
    d
}

// === SCRUB-META-001 =======================================================

/// `SCRUB-META-001`: scrub(metadata=true) clears the `/Info` fields and removes
/// the catalog `/Metadata` (XMP); reopen shows neither.
#[test]
fn scrub_meta_001_removes_info_and_xmp() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_metadata(&doc, &fields(&[("title", "secret"), ("author", "spy")])).unwrap();
    pdf_edit::set_xml_metadata(&doc, "<x:xmpmeta>secret-xmp</x:xmpmeta>").unwrap();
    assert!(doc.effective_trailer_ref("Info").is_some());

    let opts = ScrubOptions {
        metadata: true,
        javascript: false,
        attached_files: false,
        remove_links: false,
        xml_metadata: true,
    };
    scrub(&doc, &opts).unwrap();

    let re = save_reopen(&doc);
    // /Info is detached entirely (no Title to read back).
    assert!(
        re.effective_trailer_ref("Info").is_none(),
        "trailer /Info removed"
    );
    // Catalog /Metadata gone.
    assert!(!catalog_has(&re, "Metadata"), "catalog /Metadata removed");
    // The secret bytes appear nowhere in the decompressed corpus.
    let corpus = decompress_corpus(&save_bytes(&re));
    assert!(
        !corpus.windows(6).any(|w| w == b"secret"),
        "secret metadata gone from corpus"
    );
}

// === SCRUB-JS-001 =========================================================

/// `SCRUB-JS-001`: scrub(javascript=true) removes catalog `/OpenAction`, `/AA`,
/// and the `/Names /JavaScript` name-tree.
#[test]
fn scrub_js_001_removes_javascript() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    // Wire an /OpenAction (JS action), an /AA, and a /Names /JavaScript tree.
    let js_action = doc
        .add_object(Object::Dictionary(dict(&[
            ("S", Object::Name(Name::new("JavaScript"))),
            (
                "JS",
                Object::String(pdf_core::PdfString {
                    bytes: b"app.alert('hi')".to_vec(),
                    kind: pdf_core::StringKind::Literal,
                }),
            ),
        ])))
        .unwrap();
    set_catalog_key(&doc, "OpenAction", Object::Reference(js_action));
    set_catalog_key(
        &doc,
        "AA",
        Object::Dictionary(dict(&[("WC", Object::Reference(js_action))])),
    );
    let names = dict(&[(
        "JavaScript",
        Object::Dictionary(dict(&[(
            "Names",
            Object::Array(vec![
                Object::String(pdf_core::PdfString {
                    bytes: b"js0".to_vec(),
                    kind: pdf_core::StringKind::Literal,
                }),
                Object::Reference(js_action),
            ]),
        )])),
    )]);
    set_catalog_key(&doc, "Names", Object::Dictionary(names));

    let opts = ScrubOptions {
        metadata: false,
        javascript: true,
        attached_files: false,
        remove_links: false,
        xml_metadata: false,
    };
    scrub(&doc, &opts).unwrap();

    let re = save_reopen(&doc);
    assert!(!catalog_has(&re, "OpenAction"), "/OpenAction removed");
    assert!(!catalog_has(&re, "AA"), "catalog /AA removed");
    assert!(
        !names_dict(&re).contains_key(&Name::new("JavaScript")),
        "/Names /JavaScript removed"
    );
}

// === SCRUB-EMBFILE-001 ====================================================

/// `SCRUB-EMBFILE-001`: scrub(attached_files=true) removes the `/Names`
/// `/EmbeddedFiles` name-tree.
#[test]
fn scrub_embfile_001_removes_embedded_files() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let names = dict(&[(
        "EmbeddedFiles",
        Object::Dictionary(dict(&[(
            "Names",
            Object::Array(vec![
                Object::String(pdf_core::PdfString {
                    bytes: b"file.bin".to_vec(),
                    kind: pdf_core::StringKind::Literal,
                }),
                Object::Null,
            ]),
        )])),
    )]);
    set_catalog_key(&doc, "Names", Object::Dictionary(names));
    assert!(names_dict(&doc).contains_key(&Name::new("EmbeddedFiles")));

    let opts = ScrubOptions {
        metadata: false,
        javascript: false,
        attached_files: true,
        remove_links: false,
        xml_metadata: false,
    };
    scrub(&doc, &opts).unwrap();

    let re = save_reopen(&doc);
    assert!(
        !names_dict(&re).contains_key(&Name::new("EmbeddedFiles")),
        "/Names /EmbeddedFiles removed"
    );
}

// === SCRUB-LINKS-001 ======================================================

/// `SCRUB-LINKS-001`: scrub(remove_links=true) drops `/Link` annotations from
/// every page; non-link annotations survive.
#[test]
fn scrub_links_001_removes_links_only() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    let leaf = pdf_core::pagetree::page_refs(&doc)[0];

    // A /Link annotation and a non-link /Text annotation on the page.
    let link = doc
        .add_object(Object::Dictionary(dict(&[
            ("Type", Object::Name(Name::new("Annot"))),
            ("Subtype", Object::Name(Name::new("Link"))),
            (
                "Rect",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(10),
                    Object::Integer(10),
                ]),
            ),
        ])))
        .unwrap();
    let text_annot = doc
        .add_object(Object::Dictionary(dict(&[
            ("Type", Object::Name(Name::new("Annot"))),
            ("Subtype", Object::Name(Name::new("Text"))),
            (
                "Rect",
                Object::Array(vec![
                    Object::Integer(20),
                    Object::Integer(20),
                    Object::Integer(30),
                    Object::Integer(30),
                ]),
            ),
        ])))
        .unwrap();
    let mut pd = doc.resolve(leaf).unwrap().as_dict().cloned().unwrap();
    pd.insert(
        Name::new("Annots"),
        Object::Array(vec![Object::Reference(link), Object::Reference(text_annot)]),
    );
    doc.update_object(leaf, Object::Dictionary(pd)).unwrap();

    let opts = ScrubOptions {
        metadata: false,
        javascript: false,
        attached_files: false,
        remove_links: true,
        xml_metadata: false,
    };
    scrub(&doc, &opts).unwrap();

    let re = save_reopen(&doc);
    let dicts = common::annot_dicts(&re, 0);
    assert_eq!(dicts.len(), 1, "only the non-link annot survives");
    assert_eq!(common::annot_subtype(&dicts[0]), "Text");
}

// === SCRUB-IDEMPOTENT-001 =================================================

/// `SCRUB-IDEMPOTENT-001`: running a full scrub twice is safe (no panic, no
/// error) and leaves the same result.
#[test]
fn scrub_idempotent_001_double_run() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    set_metadata(&doc, &fields(&[("title", "secret")])).unwrap();
    pdf_edit::set_xml_metadata(&doc, "<x>secret</x>").unwrap();
    set_catalog_key(
        &doc,
        "OpenAction",
        Object::Dictionary(dict(&[("S", Object::Name(Name::new("JavaScript")))])),
    );
    let names = dict(&[
        ("JavaScript", Object::Dictionary(dict(&[]))),
        ("EmbeddedFiles", Object::Dictionary(dict(&[]))),
    ]);
    set_catalog_key(&doc, "Names", Object::Dictionary(names));

    let opts = ScrubOptions::default();
    scrub(&doc, &opts).unwrap();
    // Second run must not panic or error.
    scrub(&doc, &opts).unwrap();

    let re = save_reopen(&doc);
    assert!(re.effective_trailer_ref("Info").is_none());
    assert!(!catalog_has(&re, "Metadata"));
    assert!(!catalog_has(&re, "OpenAction"));
    assert!(!names_dict(&re).contains_key(&Name::new("JavaScript")));
    assert!(!names_dict(&re).contains_key(&Name::new("EmbeddedFiles")));
}

// === SCRUB-PROP-001 =======================================================

/// `SCRUB-PROP-001`: scrub on a minimal blank doc with none of those features is
/// a no-op and never panics; the page text survives.
#[test]
fn scrub_prop_001_blank_noop() {
    let doc = open(&MultiPage::new(&["AAA"]).build());
    scrub(&doc, &ScrubOptions::default()).unwrap();
    let re = save_reopen(&doc);
    assert_eq!(common::all_page_text(&re), vec!["AAA"]);
    // Still no /Info, /Metadata, etc. — nothing was invented.
    assert!(re.effective_trailer_ref("Info").is_none());
    assert!(!catalog_has(&re, "Metadata"));
}

// === BAKE-WIDGETS-001 =====================================================

/// `BAKE-WIDGETS-001`: bake(widgets=true) on the AcroForm fixture removes the
/// catalog `/AcroForm` and bakes a widget appearance into page content (a
/// `Do` operator appears in the decompressed corpus).
#[test]
fn bake_widgets_001_flattens_form() {
    let doc = open(&acroform_doc());
    assert!(catalog_has_acroform(&doc));

    bake(&doc, false, true).unwrap();

    let re = save_reopen(&doc);
    assert!(!catalog_has_acroform(&re), "/AcroForm removed");
    assert_eq!(common::count_widgets(&re), 0, "no widgets remain");
    let corpus = decompress_corpus(&save_bytes(&re));
    assert!(
        corpus.windows(3).any(|w| w == b"Do\n") || corpus.windows(3).any(|w| w == b"Do "),
        "a Form XObject Do was baked into page content"
    );
}

// === BAKE-ANNOTS-001 ======================================================

/// `BAKE-ANNOTS-001`: bake(annots=true) draws a markup annotation's `/AP /N`
/// appearance into page content as a Form XObject `Do` and removes the
/// annotation from `/Annots`.
#[test]
fn bake_annots_001_flattens_annotation() {
    use pdf_core::geom::Rect;
    use pdf_edit::Color;
    let doc = open(&MultiPage::new(&["AAA"]).build());
    // A square annotation with a real /AP /N appearance stream.
    let annot = pdf_edit::add_rect_annot(
        &doc,
        0,
        Rect::new(72.0, 72.0, 172.0, 122.0),
        Some(Color::new(1.0, 0.0, 0.0)),
        None,
    )
    .unwrap();
    let annot_ref = annot.xref();
    // Confirm it has an /AP /N before baking.
    let pre = common::annot_dicts(&doc, 0);
    assert_eq!(pre.len(), 1);
    assert!(!common::annot_ap_bytes(&doc, &pre[0]).is_empty());

    bake(&doc, true, false).unwrap();

    // The annotation object is freed (no longer resolvable to a dict).
    assert!(
        doc.resolve(annot_ref)
            .ok()
            .and_then(|o| o.as_dict().cloned())
            .is_none(),
        "baked annotation object freed"
    );

    let re = save_reopen(&doc);
    // The annotation is gone from the page.
    assert_eq!(
        common::annot_dicts(&re, 0).len(),
        0,
        "baked annotation removed from /Annots"
    );
    // Its appearance is drawn into page content (a Do op in the corpus).
    let corpus = decompress_corpus(&save_bytes(&re));
    assert!(
        corpus.windows(3).any(|w| w == b"Do\n") || corpus.windows(3).any(|w| w == b"Do "),
        "annotation appearance baked via Do"
    );
}
