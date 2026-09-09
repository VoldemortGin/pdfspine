//! `oc=` writers — `OCG-WRITE-*` (PRD-NEXT §0 item 6, ISO 32000-1 §8.11.3).
//!
//! PyMuPDF parity for the optional-content parameter of the content writers:
//! `insert_text` / `insert_textbox` / `Shape::finish` wrap their chunk in an
//! `/OC /MCn BDC` … `EMC` marked-content section registered under
//! `/Resources /Properties`, while `insert_image_*` / `show_pdf_page` put `/OC`
//! on the XObject dictionary instead. Byte-level assertions on the emitted
//! content, plus the M2 interpreter as the visibility oracle after a full
//! save → reopen.

mod common;

use common::{
    blank_page, first_xobject_dict, open, page_content_bytes, page_glyphs, page_images, page_text,
    save_reopen,
};
use pdf_core::error::Error;
use pdf_core::geom::{Point, Rect};
use pdf_core::object::{Dict, Name, ObjRef, Object};
use pdf_core::DocumentStore;
use pdf_edit::ocg::{add_ocg, set_ocmd};
use pdf_edit::{
    insert_image_jpeg, insert_image_rgb, insert_text, insert_textbox, show_pdf_page, Color,
    PageContent, Shape, TextOptions,
};

/// A minimal structurally valid JPEG header (SOI, JFIF, SOF0, EOI) — enough for
/// the `/DCTDecode` passthrough (mirrors `insert_image_e2e::synthetic_jpeg`).
fn synthetic_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8];
    v.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
    v.extend_from_slice(b"JFIF\0");
    v.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 8]);
    v.extend_from_slice(&height.to_be_bytes());
    v.extend_from_slice(&width.to_be_bytes());
    v.push(3);
    for c in 1..=3u8 {
        v.extend_from_slice(&[c, 0x11, 0x00]);
    }
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

/// The page-0 `/Resources /Properties` dict (resolved), or empty.
fn properties(doc: &DocumentStore) -> Dict {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let resources = doc
        .resolve_dict_key(&page, &Name::new("Resources"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .unwrap_or_default();
    doc.resolve_dict_key(&resources, &Name::new("Properties"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .unwrap_or_default()
}

/// The object number an `/MC<n>` property references.
fn property_ref(doc: &DocumentStore, key: &str) -> Option<u32> {
    properties(doc)
        .get(&Name::new(key))
        .and_then(Object::as_reference)
        .map(|r| r.num)
}

fn text_opts(oc: u32) -> TextOptions<'static> {
    TextOptions {
        oc,
        ..Default::default()
    }
}

fn content(doc: &DocumentStore) -> String {
    String::from_utf8_lossy(&page_content_bytes(doc, 0)).into_owned()
}

fn is_invalid(err: &Error, msg: &str) -> bool {
    matches!(err, Error::InvalidArgument(m) if *m == msg)
}

// === OCG-WRITE-TEXT-* =====================================================

/// `OCG-WRITE-TEXT-BDC`: `insert_text(oc=)` emits `q / BDC / BT … ET / EMC / Q`
/// with the BDC right after the opening `q` and the EMC right before the
/// closing `Q`; `/Properties /MC0` references the OCG; a hidden OCG hides the
/// glyphs after reopen and an ON one shows them.
#[test]
fn ocg_write_text_bdc() {
    let doc = open(&blank_page(612, 792));
    let off = add_ocg(&doc, "Off", false, &[], None).unwrap();
    insert_text(
        &doc,
        0,
        Point::new(72.0, 72.0),
        "Hidden",
        &text_opts(off.num),
    )
    .unwrap();

    let expected = format!(
        "q\n/OC /MC0 BDC\nBT\n/F0 11 Tf\n{}\n13.2 TL\n1 0 0 1 72 720 Tm\n(Hidden) Tj\nET\nEMC\nQ\n\n",
        Color::BLACK.fill_op()
    );
    assert_eq!(content(&doc), expected);
    assert_eq!(property_ref(&doc, "MC0"), Some(off.num));

    let re = save_reopen(&doc);
    assert_eq!(property_ref(&re, "MC0"), Some(off.num));
    assert!(
        page_glyphs(&re, 0).is_empty(),
        "text under an OFF layer must be hidden"
    );

    // The same writer under an ON layer is visible.
    let doc2 = open(&blank_page(612, 792));
    let on = add_ocg(&doc2, "On", true, &[], None).unwrap();
    insert_text(
        &doc2,
        0,
        Point::new(72.0, 72.0),
        "Shown",
        &text_opts(on.num),
    )
    .unwrap();
    assert_eq!(page_text(&save_reopen(&doc2), 0), "Shown");
}

/// `OCG-WRITE-TEXTBOX-BDC`: `insert_textbox(oc=)` wraps the text object the
/// same way and the OFF layer hides the wrapped lines.
#[test]
fn ocg_write_textbox_bdc() {
    let doc = open(&blank_page(612, 792));
    let off = add_ocg(&doc, "Off", false, &[], None).unwrap();
    let rect = Rect::new(72.0, 72.0, 400.0, 200.0);
    insert_textbox(&doc, 0, rect, "Boxed text", &text_opts(off.num)).unwrap();

    let c = content(&doc);
    assert!(c.starts_with("q\n/OC /MC0 BDC\nBT\n"), "{c}");
    assert!(c.ends_with("ET\nEMC\nQ\n\n"), "{c}");
    assert_eq!(c.matches("BDC").count(), 1);
    assert_eq!(c.matches("EMC").count(), 1);
    assert_eq!(property_ref(&doc, "MC0"), Some(off.num));
    assert!(page_glyphs(&save_reopen(&doc), 0).is_empty());
}

// === OCG-WRITE-PROPS-* ====================================================

/// `OCG-WRITE-PROPS-REUSE`: the same OCG inserted twice reuses `/MC0`; a second
/// OCG gets `/MC1`; `/Properties` holds exactly one entry per OCG.
#[test]
fn ocg_write_props_reuse() {
    let doc = open(&blank_page(612, 792));
    let a = add_ocg(&doc, "A", true, &[], None).unwrap();
    let b = add_ocg(&doc, "B", true, &[], None).unwrap();
    insert_text(&doc, 0, Point::new(72.0, 72.0), "one", &text_opts(a.num)).unwrap();
    insert_text(&doc, 0, Point::new(72.0, 100.0), "two", &text_opts(a.num)).unwrap();
    insert_text(&doc, 0, Point::new(72.0, 130.0), "three", &text_opts(b.num)).unwrap();

    let c = content(&doc);
    assert_eq!(c.matches("/OC /MC0 BDC").count(), 2, "{c}");
    assert_eq!(c.matches("/OC /MC1 BDC").count(), 1, "{c}");
    let props = properties(&doc);
    assert_eq!(props.len(), 2);
    assert_eq!(property_ref(&doc, "MC0"), Some(a.num));
    assert_eq!(property_ref(&doc, "MC1"), Some(b.num));

    let re = save_reopen(&doc);
    assert_eq!(property_ref(&re, "MC0"), Some(a.num));
    assert_eq!(property_ref(&re, "MC1"), Some(b.num));
    assert_eq!(page_text(&re, 0), "onetwothree");
}

/// `OCG-WRITE-PROPS-EXISTING`: a pre-existing, unrelated `/Properties /MC0`
/// entry is skipped — the next OCG lands on `/MC1` (PyMuPDF's smallest-free-
/// index rule) — while an existing entry that already references the OCG is
/// reused under its own key.
#[test]
fn ocg_write_props_existing() {
    let doc = open(&blank_page(612, 792));
    let a = add_ocg(&doc, "A", true, &[], None).unwrap();
    let b = add_ocg(&doc, "B", true, &[], None).unwrap();
    // Seed `/MC0` with an unrelated reference and `/Custom` with the OCG `b`.
    let pc = PageContent::new(&doc, 0).unwrap();
    assert_eq!(
        pc.add_resource("Properties", "MC", Object::Reference(ObjRef::new(1, 0)))
            .unwrap(),
        "MC0"
    );
    pc.add_resource("Properties", "Custom", Object::Reference(b))
        .unwrap();

    insert_text(&doc, 0, Point::new(72.0, 72.0), "a", &text_opts(a.num)).unwrap();
    insert_text(&doc, 0, Point::new(72.0, 100.0), "b", &text_opts(b.num)).unwrap();

    let c = content(&doc);
    assert!(c.contains("/OC /MC1 BDC"), "{c}");
    assert!(c.contains("/OC /Custom0 BDC"), "{c}");
    assert!(!c.contains("/OC /MC0 BDC"), "{c}");
    assert_eq!(property_ref(&doc, "MC1"), Some(a.num));
    assert_eq!(properties(&doc).len(), 3);
}

/// `OCG-WRITE-OCMD`: an OCMD xref is accepted like an OCG and referenced from
/// `/Properties`; its `/OCGs` decide visibility (`AllOn` over an OFF member
/// hides the text).
#[test]
fn ocg_write_ocmd_accepted() {
    let doc = open(&blank_page(612, 792));
    let a = add_ocg(&doc, "A", true, &[], None).unwrap();
    let b = add_ocg(&doc, "B", false, &[], None).unwrap();
    let ocmd = set_ocmd(&doc, 0, Some(&[a.num, b.num]), Some("AllOn"), None).unwrap();
    insert_text(&doc, 0, Point::new(72.0, 72.0), "member", &text_opts(ocmd)).unwrap();

    assert!(content(&doc).contains("/OC /MC0 BDC"));
    assert_eq!(property_ref(&doc, "MC0"), Some(ocmd));
    assert!(page_glyphs(&save_reopen(&doc), 0).is_empty());
}

// === OCG-WRITE-SHAPE-* ====================================================

/// `OCG-WRITE-SHAPE-FINISH`: every `Shape::finish(oc=)` block is wrapped in its
/// own `q / BDC … EMC / Q`; a block finished without `oc` and the default
/// trailing block at `commit` stay unwrapped; a hidden layer hides the fill.
#[test]
fn ocg_write_shape_finish() {
    let doc = open(&blank_page(200, 200));
    let off = add_ocg(&doc, "Off", false, &[], None).unwrap();
    let mut s = Shape::new(&doc, 0).unwrap();
    s.draw_rect(Rect::new(10.0, 10.0, 50.0, 50.0));
    s.finish(None, Some(Color::BLACK), 1.0, None, false, false, off.num)
        .unwrap();
    s.draw_line(Point::new(0.0, 0.0), Point::new(100.0, 100.0));
    s.finish(Some(Color::BLACK), None, 2.0, None, false, false, off.num)
        .unwrap();
    s.draw_line(Point::new(0.0, 100.0), Point::new(100.0, 0.0));
    s.finish(Some(Color::BLACK), None, 1.0, None, false, false, 0)
        .unwrap();
    s.draw_line(Point::new(5.0, 5.0), Point::new(6.0, 6.0));
    s.commit().unwrap();

    let expected = format!(
        "q\n/OC /MC0 BDC\n1 w\n{fill}\n10 150 40 40 re\nf\nEMC\nQ\n\
         q\n/OC /MC0 BDC\n2 w\n{stroke}\n0 200 m\n100 100 l\nS\nEMC\nQ\n\
         q\n1 w\n{stroke}\n0 100 m\n100 200 l\nS\nQ\n\
         q\n1 w\n0 0 0 RG\n5 195 m\n6 194 l\nS\nQ\n\n",
        fill = Color::BLACK.fill_op(),
        stroke = Color::BLACK.stroke_op()
    );
    assert_eq!(content(&doc), expected);
    assert_eq!(property_ref(&doc, "MC0"), Some(off.num));

    let re = save_reopen(&doc);
    let leaf = pdf_core::pagetree::page_refs(&re)[0];
    let page = pdf_core::pagetree::page_dict(&re, leaf).unwrap();
    let ops = pdf_text::interpret_page(&re, &page).drawings;
    // Only the two unwrapped strokes survive; the hidden fill + stroke are gone.
    assert_eq!(ops.len(), 2);
}

// === OCG-WRITE-XOBJECT-* ==================================================

/// `OCG-WRITE-IMAGE-OC`: `insert_image_jpeg(oc=)` / `insert_image_rgb(oc=)`
/// put `/OC` on the image XObject dict, write **no** marked content and leave
/// `/Properties` alone; the hidden image is not inventoried after reopen.
#[test]
fn ocg_write_image_oc() {
    for rgb in [false, true] {
        let doc = open(&blank_page(612, 792));
        let off = add_ocg(&doc, "Off", false, &[], None).unwrap();
        let rect = Rect::new(10.0, 10.0, 110.0, 110.0);
        let name = if rgb {
            insert_image_rgb(&doc, 0, rect, 2, 2, &[0u8; 12], off.num).unwrap()
        } else {
            insert_image_jpeg(&doc, 0, rect, &synthetic_jpeg(8, 8), off.num).unwrap()
        };
        assert_eq!(name, "Img0");
        let c = content(&doc);
        assert!(!c.contains("BDC") && !c.contains("EMC"), "{c}");
        assert!(properties(&doc).is_empty());
        assert_eq!(
            first_xobject_dict(&doc, 0)
                .get(&Name::new("OC"))
                .and_then(Object::as_reference),
            Some(off)
        );

        let re = save_reopen(&doc);
        assert_eq!(
            first_xobject_dict(&re, 0)
                .get(&Name::new("OC"))
                .and_then(Object::as_reference),
            Some(off)
        );
        assert!(
            page_images(&re, 0).is_empty(),
            "hidden image must be skipped"
        );
    }
}

/// `OCG-WRITE-FORM-OC`: `show_pdf_page(oc=)` puts `/OC` on the Form XObject; no
/// marked content; the OFF layer hides the placed page's text after reopen.
#[test]
fn ocg_write_form_oc() {
    let src = open(&blank_page(300, 300));
    insert_text(
        &src,
        0,
        Point::new(20.0, 40.0),
        "Stamp",
        &TextOptions::default(),
    )
    .unwrap();
    let src = save_reopen(&src);

    let dst = open(&blank_page(612, 792));
    let off = add_ocg(&dst, "Off", false, &[], None).unwrap();
    let leaf = pdf_core::pagetree::page_refs(&dst)[0];
    let name = show_pdf_page(
        &dst,
        leaf,
        &src,
        0,
        Rect::new(0.0, 0.0, 300.0, 300.0),
        off.num,
    )
    .unwrap();
    assert_eq!(name, "Fm0");
    let c = content(&dst);
    assert!(!c.contains("BDC"), "{c}");
    assert!(properties(&dst).is_empty());
    let form = first_xobject_dict(&dst, 0);
    assert_eq!(
        form.get(&Name::new("OC")).and_then(Object::as_reference),
        Some(off)
    );

    let re = save_reopen(&dst);
    assert_eq!(page_text(&re, 0), "");
}

// === OCG-WRITE-ERRORS / OCG-WRITE-ZERO ====================================

/// `OCG-WRITE-BAD-OC`: an `oc` that is not an OCG / OCMD (here the page leaf)
/// is `InvalidArgument("bad optional content: 'oc'")` and a nonexistent xref
/// is `InvalidArgument("bad xref")` on every writer; a failed text insert
/// leaves the page untouched (no chunk, no `/Properties`).
#[test]
fn ocg_write_bad_oc() {
    let doc = open(&blank_page(612, 792));
    let page_xref = pdf_core::pagetree::page_refs(&doc)[0].num;
    let bad = [
        (page_xref, "bad optional content: 'oc'"),
        (9999, "bad xref"),
    ];
    let jpeg = synthetic_jpeg(4, 4);
    let src = open(&blank_page(100, 100));
    let leaf = pdf_core::pagetree::page_refs(&doc)[0];
    for (oc, msg) in bad {
        let e = insert_text(&doc, 0, Point::new(1.0, 1.0), "x", &text_opts(oc)).unwrap_err();
        assert!(is_invalid(&e, msg), "insert_text: {e}");
        let e = insert_textbox(
            &doc,
            0,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            "x",
            &text_opts(oc),
        )
        .unwrap_err();
        assert!(is_invalid(&e, msg), "insert_textbox: {e}");
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let e = insert_image_jpeg(&doc, 0, r, &jpeg, oc).unwrap_err();
        assert!(is_invalid(&e, msg), "insert_image_jpeg: {e}");
        let e = insert_image_rgb(&doc, 0, r, 1, 1, &[0u8; 3], oc).unwrap_err();
        assert!(is_invalid(&e, msg), "insert_image_rgb: {e}");
        let e = show_pdf_page(&doc, leaf, &src, 0, r, oc).unwrap_err();
        assert!(is_invalid(&e, msg), "show_pdf_page: {e}");
        let mut s = Shape::new(&doc, 0).unwrap();
        s.draw_rect(r);
        let e = s
            .finish(Some(Color::BLACK), None, 1.0, None, false, false, oc)
            .unwrap_err();
        assert!(is_invalid(&e, msg), "Shape::finish: {e}");
    }
    assert!(page_content_bytes(&doc, 0)
        .iter()
        .all(u8::is_ascii_whitespace));
    assert!(properties(&doc).is_empty());
    assert!(
        !has_xobjects(&doc),
        "a failed image insert must not register an XObject"
    );
}

/// Whether page 0 has an `/XObject` resource dict at all.
fn has_xobjects(doc: &DocumentStore) -> bool {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    doc.resolve_dict_key(&page, &Name::new("Resources"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .is_some_and(|r| r.contains_key(&Name::new("XObject")))
}

/// `OCG-WRITE-ZERO-UNCHANGED`: `oc == 0` adds nothing — the text, drawing and
/// image chunks are byte-identical to the pre-`oc` writers (no `BDC` / `EMC`),
/// and no `/Properties` or XObject `/OC` appears.
#[test]
fn ocg_write_zero_unchanged() {
    let doc = open(&blank_page(200, 200));
    insert_text(&doc, 0, Point::new(72.0, 72.0), "Plain", &text_opts(0)).unwrap();
    assert_eq!(
        content(&doc),
        format!(
            "q\nBT\n/F0 11 Tf\n{}\n13.2 TL\n1 0 0 1 72 128 Tm\n(Plain) Tj\nET\nQ\n\n",
            Color::BLACK.fill_op()
        )
    );
    assert!(properties(&doc).is_empty());

    let doc = open(&blank_page(200, 200));
    let mut s = Shape::new(&doc, 0).unwrap();
    s.draw_rect(Rect::new(10.0, 10.0, 50.0, 50.0));
    s.finish(Some(Color::BLACK), None, 1.0, None, false, false, 0)
        .unwrap();
    s.commit().unwrap();
    assert_eq!(
        content(&doc),
        format!(
            "q\n1 w\n{}\n10 150 40 40 re\nS\nQ\n\n",
            Color::BLACK.stroke_op()
        )
    );
    assert!(properties(&doc).is_empty());

    let doc = open(&blank_page(200, 200));
    insert_image_jpeg(
        &doc,
        0,
        Rect::new(0.0, 0.0, 10.0, 10.0),
        &synthetic_jpeg(4, 4),
        0,
    )
    .unwrap();
    assert_eq!(content(&doc), "q\n10 0 0 10 0 190 cm\n/Img0 Do\nQ\n\n");
    assert!(properties(&doc).is_empty());
    assert!(!first_xobject_dict(&doc, 0).contains_key(&Name::new("OC")));
}
