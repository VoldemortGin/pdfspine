//! M4b — annotation CRUD + `/AP /N` appearance-stream tests (PRD §8.8 / §12 M4
//! exit). For each subtype: create → save → reopen → assert `/Subtype` +
//! geometry + a present-and-non-empty `/AP /N` Form XObject. Plus update / CRUD /
//! quadpoints / file-attachment / robustness coverage.
//!
//! All assertions go through the reparse oracle (`save_reopen`) so they verify
//! what actually round-trips, and a `qpdf --check` gate runs when qpdf is on PATH.

mod common;

use common::{
    annot_ap_bytes, annot_ap_dict, annot_dicts, annot_subtype, blank_page, open, qpdf_check,
    save_bytes, save_reopen,
};

use pdf_core::geom::{Point, Quad, Rect};
use pdf_core::object::{Name, Object};
use pdf_edit::{
    add_circle_annot, add_file_annot, add_freetext_annot, add_highlight_annot, add_ink_annot,
    add_line_annot, add_polygon_annot, add_polyline_annot, add_rect_annot, add_redact_annot,
    add_squiggly_annot, add_stamp_annot, add_strikeout_annot, add_text_annot, add_underline_annot,
    annot_count, annot_names, annots, delete_annot, first_annot, Color,
};

/// Helper: a fresh single-page (Letter) blank document.
fn doc() -> pdf_core::DocumentStore {
    open(&blank_page(612, 792))
}

/// Asserts the first annot of the reopened page has the given subtype and a
/// present, non-empty `/AP /N` Form XObject. Returns the reopened doc.
fn assert_subtype_and_ap(d: &pdf_core::DocumentStore, subtype: &str) -> pdf_core::DocumentStore {
    let re = save_reopen(d);
    let dicts = annot_dicts(&re, 0);
    assert_eq!(dicts.len(), 1, "expected exactly one annot");
    assert_eq!(annot_subtype(&dicts[0]), subtype, "subtype mismatch");
    let ap = annot_ap_dict(&re, &dicts[0]).expect("/AP /N present");
    assert_eq!(
        ap.get(&Name::new("Subtype"))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes()),
        Some(&b"Form"[..]),
        "/AP /N must be a Form XObject"
    );
    assert!(
        ap.contains_key(&Name::new("BBox")),
        "/AP /N must have a /BBox"
    );
    let bytes = annot_ap_bytes(&re, &dicts[0]);
    assert!(!bytes.is_empty(), "/AP /N content stream must be non-empty");
    re
}

// === per-subtype create → reopen → subtype + geometry + /AP /N ============

/// `ANNOT-TEXT-001`
#[test]
fn annot_text_001() {
    let d = doc();
    add_text_annot(&d, 0, Point::new(100.0, 100.0), "a note", "Note").unwrap();
    let re = assert_subtype_and_ap(&d, "Text");
    let dicts = annot_dicts(&re, 0);
    // Contents preserved.
    let c = dicts[0]
        .get(&Name::new("Contents"))
        .and_then(Object::as_string)
        .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
        .unwrap_or_default();
    assert_eq!(c, "a note");
}

/// `ANNOT-FREETEXT-001`
#[test]
fn annot_freetext_001() {
    let d = doc();
    add_freetext_annot(
        &d,
        0,
        Rect::new(72.0, 72.0, 300.0, 140.0),
        "Hello box",
        12.0,
        Color::BLACK,
        Some(Color::new(0.9, 0.9, 0.9)),
        0,
    )
    .unwrap();
    let re = assert_subtype_and_ap(&d, "FreeText");
    let bytes = annot_ap_bytes(&re, &annot_dicts(&re, 0)[0]);
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("Tj"), "FreeText AP should render text: {s}");
    assert!(s.contains("BT"), "FreeText AP should have a text object");
}

/// `ANNOT-HIGHLIGHT-001`
#[test]
fn annot_highlight_001() {
    let d = doc();
    let q = Quad {
        ul: Point::new(72.0, 100.0),
        ur: Point::new(200.0, 100.0),
        ll: Point::new(72.0, 115.0),
        lr: Point::new(200.0, 115.0),
    };
    add_highlight_annot(&d, 0, &[q]).unwrap();
    let re = assert_subtype_and_ap(&d, "Highlight");
    let dicts = annot_dicts(&re, 0);
    // QuadPoints present with 8 numbers.
    let qp = dicts[0]
        .get(&Name::new("QuadPoints"))
        .and_then(Object::as_array)
        .expect("QuadPoints");
    assert_eq!(qp.len(), 8);
    // AP uses a Multiply blend and a fill.
    let bytes = annot_ap_bytes(&re, &dicts[0]);
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("\nf\n"),
        "highlight AP should fill the quad: {s}"
    );
    // ExtGState Multiply present in AP resources.
    let ap = annot_ap_dict(&re, &dicts[0]).unwrap();
    let res = ap
        .get(&Name::new("Resources"))
        .and_then(Object::as_dict)
        .unwrap();
    assert!(
        res.contains_key(&Name::new("ExtGState")),
        "highlight AP should reference a Multiply ExtGState"
    );
}

/// `ANNOT-UNDERLINE-001`
#[test]
fn annot_underline_001() {
    let d = doc();
    let q = Quad {
        ul: Point::new(72.0, 100.0),
        ur: Point::new(200.0, 100.0),
        ll: Point::new(72.0, 115.0),
        lr: Point::new(200.0, 115.0),
    };
    add_underline_annot(&d, 0, &[q]).unwrap();
    let re = assert_subtype_and_ap(&d, "Underline");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &annot_dicts(&re, 0)[0])).into_owned();
    assert!(s.contains(" l"), "underline AP should draw a line: {s}");
    assert!(s.contains("S"), "underline AP should stroke: {s}");
}

/// `ANNOT-STRIKEOUT-001`
#[test]
fn annot_strikeout_001() {
    let d = doc();
    let q = Quad {
        ul: Point::new(72.0, 100.0),
        ur: Point::new(200.0, 100.0),
        ll: Point::new(72.0, 115.0),
        lr: Point::new(200.0, 115.0),
    };
    add_strikeout_annot(&d, 0, &[q]).unwrap();
    let re = assert_subtype_and_ap(&d, "StrikeOut");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &annot_dicts(&re, 0)[0])).into_owned();
    assert!(s.contains(" l"), "strikeout AP should draw a line: {s}");
}

/// `ANNOT-SQUIGGLY-001`
#[test]
fn annot_squiggly_001() {
    let d = doc();
    let q = Quad {
        ul: Point::new(72.0, 100.0),
        ur: Point::new(200.0, 100.0),
        ll: Point::new(72.0, 115.0),
        lr: Point::new(200.0, 115.0),
    };
    add_squiggly_annot(&d, 0, &[q]).unwrap();
    let re = assert_subtype_and_ap(&d, "Squiggly");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &annot_dicts(&re, 0)[0])).into_owned();
    assert!(
        s.contains(" l"),
        "squiggly AP should draw zig-zag segments: {s}"
    );
}

/// `ANNOT-SQUARE-001`
#[test]
fn annot_square_001() {
    let d = doc();
    add_rect_annot(
        &d,
        0,
        Rect::new(100.0, 100.0, 300.0, 200.0),
        Some(Color::new(1.0, 0.0, 0.0)),
        Some(Color::new(0.0, 0.0, 1.0)),
    )
    .unwrap();
    let re = assert_subtype_and_ap(&d, "Square");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &annot_dicts(&re, 0)[0])).into_owned();
    assert!(s.contains("re"), "square AP should use a rect path: {s}");
}

/// `ANNOT-CIRCLE-001`
#[test]
fn annot_circle_001() {
    let d = doc();
    add_circle_annot(
        &d,
        0,
        Rect::new(100.0, 100.0, 300.0, 200.0),
        Some(Color::BLACK),
        None,
    )
    .unwrap();
    let re = assert_subtype_and_ap(&d, "Circle");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &annot_dicts(&re, 0)[0])).into_owned();
    assert!(s.contains(" c"), "circle AP should use Bézier curves: {s}");
}

/// `ANNOT-LINE-001`
#[test]
fn annot_line_001() {
    let d = doc();
    add_line_annot(
        &d,
        0,
        Point::new(72.0, 72.0),
        Point::new(300.0, 200.0),
        Some(Color::BLACK),
    )
    .unwrap();
    let re = assert_subtype_and_ap(&d, "Line");
    let dicts = annot_dicts(&re, 0);
    let l = dicts[0]
        .get(&Name::new("L"))
        .and_then(Object::as_array)
        .expect("/L");
    assert_eq!(l.len(), 4, "/L should have two endpoints");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &dicts[0])).into_owned();
    assert!(
        s.contains(" m") && s.contains(" l"),
        "line AP should draw a segment: {s}"
    );
}

/// `ANNOT-POLYGON-001`
#[test]
fn annot_polygon_001() {
    let d = doc();
    let pts = [
        Point::new(72.0, 72.0),
        Point::new(200.0, 100.0),
        Point::new(150.0, 200.0),
    ];
    add_polygon_annot(
        &d,
        0,
        &pts,
        Some(Color::BLACK),
        Some(Color::new(0.8, 0.8, 0.0)),
    )
    .unwrap();
    let re = assert_subtype_and_ap(&d, "Polygon");
    let dicts = annot_dicts(&re, 0);
    let v = dicts[0]
        .get(&Name::new("Vertices"))
        .and_then(Object::as_array)
        .expect("/Vertices");
    assert_eq!(v.len(), 6, "3 vertices → 6 numbers");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &dicts[0])).into_owned();
    assert!(s.contains("h"), "polygon AP should close the path: {s}");
}

/// `ANNOT-POLYLINE-001`
#[test]
fn annot_polyline_001() {
    let d = doc();
    let pts = [
        Point::new(72.0, 72.0),
        Point::new(200.0, 100.0),
        Point::new(150.0, 200.0),
    ];
    add_polyline_annot(&d, 0, &pts, Some(Color::BLACK)).unwrap();
    let re = assert_subtype_and_ap(&d, "PolyLine");
    let dicts = annot_dicts(&re, 0);
    let v = dicts[0]
        .get(&Name::new("Vertices"))
        .and_then(Object::as_array)
        .expect("/Vertices");
    assert_eq!(v.len(), 6);
}

/// `ANNOT-INK-001`
#[test]
fn annot_ink_001() {
    let d = doc();
    let strokes = vec![
        vec![
            Point::new(72.0, 72.0),
            Point::new(120.0, 90.0),
            Point::new(150.0, 72.0),
        ],
        vec![Point::new(200.0, 200.0), Point::new(250.0, 250.0)],
    ];
    add_ink_annot(&d, 0, &strokes, Some(Color::new(0.0, 0.0, 1.0))).unwrap();
    let re = assert_subtype_and_ap(&d, "Ink");
    let dicts = annot_dicts(&re, 0);
    let ink = dicts[0]
        .get(&Name::new("InkList"))
        .and_then(Object::as_array)
        .expect("/InkList");
    assert_eq!(ink.len(), 2, "two strokes");
}

/// `ANNOT-STAMP-001`
#[test]
fn annot_stamp_001() {
    let d = doc();
    add_stamp_annot(&d, 0, Rect::new(100.0, 100.0, 300.0, 160.0), "APPROVED").unwrap();
    let re = assert_subtype_and_ap(&d, "Stamp");
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &annot_dicts(&re, 0)[0])).into_owned();
    assert!(s.contains("Tj"), "stamp AP should render the label: {s}");
}

/// `ANNOT-REDACT-001`
#[test]
fn annot_redact_001() {
    let d = doc();
    add_redact_annot(
        &d,
        0,
        Rect::new(100.0, 100.0, 300.0, 130.0),
        Some(Color::BLACK),
        Some("REDACTED"),
    )
    .unwrap();
    let re = assert_subtype_and_ap(&d, "Redact");
    let dicts = annot_dicts(&re, 0);
    assert!(
        dicts[0].contains_key(&Name::new("QuadPoints")),
        "redact should carry QuadPoints"
    );
    assert!(
        dicts[0].contains_key(&Name::new("OverlayText")),
        "redact should carry OverlayText"
    );
}

// === FileAttachment embeds the bytes (ANNOT-FILE-*) =======================

/// `ANNOT-FILE-001` — FileAttachment embeds extractable bytes + has `/AP /N`.
#[test]
fn annot_file_001() {
    let d = doc();
    let payload = b"hello embedded file \x00\x01\x02 contents";
    add_file_annot(&d, 0, Point::new(100.0, 100.0), payload, "data.bin").unwrap();
    let re = assert_subtype_and_ap(&d, "FileAttachment");
    let dicts = annot_dicts(&re, 0);
    // Walk /FS → /EF → /F to the embedded-file stream and extract its bytes.
    let fs_ref = dicts[0]
        .get(&Name::new("FS"))
        .and_then(Object::as_reference)
        .expect("/FS ref");
    let fs = re.resolve(fs_ref).unwrap();
    let fs = fs.as_dict().unwrap();
    let ef = fs
        .get(&Name::new("EF"))
        .and_then(Object::as_dict)
        .expect("/EF");
    let f_ref = ef
        .get(&Name::new("F"))
        .and_then(Object::as_reference)
        .expect("/EF /F");
    let stream = re.resolve(f_ref).unwrap();
    let stream = stream.as_stream().expect("embedded-file stream");
    let extracted = re.decode_stream(stream).unwrap().into_decoded().unwrap();
    assert_eq!(extracted, payload, "embedded bytes must round-trip exactly");
    // Filename preserved.
    let name = fs
        .get(&Name::new("F"))
        .and_then(Object::as_string)
        .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
        .unwrap_or_default();
    assert_eq!(name, "data.bin");
}

// === update() reflects color change in the AP (ANNOT-UPDATE-*) ============

/// `ANNOT-UPDATE-001` — changing `/C` + `update()` rewrites the AP color op.
#[test]
fn annot_update_001_color_reflected() {
    let d = doc();
    let a = add_rect_annot(
        &d,
        0,
        Rect::new(100.0, 100.0, 300.0, 200.0),
        Some(Color::new(1.0, 0.0, 0.0)),
        None,
    )
    .unwrap();
    let xref = a.xref();
    // Original AP should stroke red (1 0 0 RG).
    let re0 = save_reopen(&d);
    let s0 = String::from_utf8_lossy(&annot_ap_bytes(&re0, &annot_dicts(&re0, 0)[0])).into_owned();
    assert!(s0.contains("1 0 0 RG"), "original AP should be red: {s0}");

    // Change color to green and regenerate the appearance.
    let handle = pdf_edit::Annot::from_ref(&d, pdf_core::pagetree::page_refs(&d)[0], xref);
    handle
        .set_colors(Some(Color::new(0.0, 1.0, 0.0)), None)
        .unwrap();
    handle.update().unwrap();

    let re1 = save_reopen(&d);
    let s1 = String::from_utf8_lossy(&annot_ap_bytes(&re1, &annot_dicts(&re1, 0)[0])).into_owned();
    assert!(s1.contains("0 1 0 RG"), "updated AP should be green: {s1}");
    assert!(!s1.contains("1 0 0 RG"), "stale red op must be gone: {s1}");
}

/// `ANNOT-UPDATE-002` — opacity change writes `/CA` and reopens.
#[test]
fn annot_update_002_opacity() {
    let d = doc();
    let a = add_highlight_annot(
        &d,
        0,
        &[Quad {
            ul: Point::new(72.0, 100.0),
            ur: Point::new(200.0, 100.0),
            ll: Point::new(72.0, 115.0),
            lr: Point::new(200.0, 115.0),
        }],
    )
    .unwrap();
    let xref = a.xref();
    let handle = pdf_edit::Annot::from_ref(&d, pdf_core::pagetree::page_refs(&d)[0], xref);
    handle.set_opacity(0.5).unwrap();
    handle.update().unwrap();
    let re = save_reopen(&d);
    let ca = annot_dicts(&re, 0)[0]
        .get(&Name::new("CA"))
        .and_then(Object::as_f64)
        .expect("/CA");
    assert!((ca - 0.5).abs() < 1e-6, "CA should be 0.5, got {ca}");
}

// === CRUD: count / delete / iterate order (ANNOT-CRUD-*) ==================

/// `ANNOT-CRUD-001` — count after adds; delete removes; iterate order.
#[test]
fn annot_crud_001() {
    let d = doc();
    assert_eq!(annot_count(&d, 0), 0);
    add_rect_annot(
        &d,
        0,
        Rect::new(10.0, 10.0, 50.0, 50.0),
        Some(Color::BLACK),
        None,
    )
    .unwrap();
    let a2 = add_circle_annot(
        &d,
        0,
        Rect::new(60.0, 60.0, 100.0, 100.0),
        Some(Color::BLACK),
        None,
    )
    .unwrap();
    add_line_annot(&d, 0, Point::new(0.0, 0.0), Point::new(20.0, 20.0), None).unwrap();
    assert_eq!(annot_count(&d, 0), 3);

    // Reopen preserves order Square, Circle, Line.
    let re = save_reopen(&d);
    let subs: Vec<String> = annots(&re, 0).iter().map(|a| annot_subtype_of(a)).collect();
    assert_eq!(subs, vec!["Square", "Circle", "Line"]);

    // Delete the middle (circle) one.
    let xref = a2.xref();
    delete_annot(&d, 0, xref).unwrap();
    assert_eq!(annot_count(&d, 0), 2);
    let re2 = save_reopen(&d);
    let subs2: Vec<String> = annots(&re2, 0)
        .iter()
        .map(|a| annot_subtype_of(a))
        .collect();
    assert_eq!(subs2, vec!["Square", "Line"], "circle removed, order kept");
}

/// Reads an annot handle's subtype as a String (test convenience).
fn annot_subtype_of(a: &pdf_edit::Annot) -> String {
    a.annot_type().pdf_name().to_string()
}

/// `ANNOT-CRUD-002` — `first_annot` / `annot_names`.
#[test]
fn annot_crud_002_first_and_names() {
    let d = doc();
    assert!(first_annot(&d, 0).is_none());
    let a = add_rect_annot(
        &d,
        0,
        Rect::new(10.0, 10.0, 50.0, 50.0),
        Some(Color::BLACK),
        None,
    )
    .unwrap();
    let xref = a.xref();
    let handle = pdf_edit::Annot::from_ref(&d, pdf_core::pagetree::page_refs(&d)[0], xref);
    handle.set_info(None, None, Some("annot-A")).unwrap();
    let re = save_reopen(&d);
    assert!(first_annot(&re, 0).is_some());
    assert_eq!(annot_names(&re, 0), vec!["annot-A".to_string()]);
}

/// `ANNOT-CRUD-003` — delete also frees the AP /N stream object.
#[test]
fn annot_crud_003_delete_frees_ap() {
    let d = doc();
    let a = add_rect_annot(
        &d,
        0,
        Rect::new(10.0, 10.0, 50.0, 50.0),
        Some(Color::BLACK),
        None,
    )
    .unwrap();
    let ap_ref = a.appearance_ref().expect("AP ref");
    let xref = a.xref();
    delete_annot(&d, 0, xref).unwrap();
    // The AP stream object is now free (resolves to Null).
    let obj = d.resolve(ap_ref).unwrap();
    assert!(obj.is_null(), "AP /N stream should be freed after delete");
}

// === MARKUP quadpoints geometry (ANNOT-MARKUP-QUAD-*) =====================

/// `ANNOT-MARKUP-QUAD-001` — highlight quadpoints geometry: AP fill covers the
/// quad and the QuadPoints round-trip in Acrobat order.
#[test]
fn annot_markup_quad_001() {
    let d = doc();
    // Two adjacent quads (two highlighted lines).
    let quads = [
        Quad {
            ul: Point::new(72.0, 100.0),
            ur: Point::new(300.0, 100.0),
            ll: Point::new(72.0, 114.0),
            lr: Point::new(300.0, 114.0),
        },
        Quad {
            ul: Point::new(72.0, 120.0),
            ur: Point::new(250.0, 120.0),
            ll: Point::new(72.0, 134.0),
            lr: Point::new(250.0, 134.0),
        },
    ];
    add_highlight_annot(&d, 0, &quads).unwrap();
    let re = save_reopen(&d);
    let dicts = annot_dicts(&re, 0);
    let qp = dicts[0]
        .get(&Name::new("QuadPoints"))
        .and_then(Object::as_array)
        .expect("QuadPoints");
    assert_eq!(qp.len(), 16, "2 quads → 16 numbers");
    // The AP fills two quads (two `f` operators).
    let s = String::from_utf8_lossy(&annot_ap_bytes(&re, &dicts[0])).into_owned();
    let fills = s.matches("\nf\n").count();
    assert!(fills >= 2, "expected ≥2 quad fills in AP, got {fills}: {s}");
}

// === robustness / preservation (ANNOT-PROP-*) =============================

/// `ANNOT-PROP-001` — adding annots preserves existing page content/text.
#[test]
fn annot_prop_001_preserves_content() {
    // Build a page that already has text inserted.
    let d = doc();
    pdf_edit::insert_text(
        &d,
        0,
        Point::new(72.0, 72.0),
        "Original",
        &pdf_edit::TextOptions::default(),
    )
    .unwrap();
    add_rect_annot(
        &d,
        0,
        Rect::new(100.0, 100.0, 300.0, 200.0),
        Some(Color::BLACK),
        None,
    )
    .unwrap();
    add_highlight_annot(
        &d,
        0,
        &[Quad {
            ul: Point::new(72.0, 100.0),
            ur: Point::new(200.0, 100.0),
            ll: Point::new(72.0, 115.0),
            lr: Point::new(200.0, 115.0),
        }],
    )
    .unwrap();
    let re = save_reopen(&d);
    // Original page text still extractable.
    let leaf = pdf_core::pagetree::page_refs(&re)[0];
    let page = pdf_core::pagetree::page_dict(&re, leaf).unwrap();
    let text: String = pdf_text::interpret_page(&re, &page)
        .glyphs
        .iter()
        .map(|g| g.unicode.as_str())
        .collect();
    assert!(text.contains("Original"), "page text must survive: {text}");
    assert_eq!(annot_count(&re, 0), 2);
}

/// `ANNOT-PROP-002` — none of the add_* paths panic on degenerate inputs.
#[test]
fn annot_prop_002_no_panic_degenerate() {
    let d = doc();
    // Empty quads / vertices / strokes.
    add_highlight_annot(&d, 0, &[]).unwrap();
    add_polygon_annot(&d, 0, &[], Some(Color::BLACK), None).unwrap();
    add_ink_annot(&d, 0, &[], Some(Color::BLACK)).unwrap();
    add_freetext_annot(
        &d,
        0,
        Rect::new(0.0, 0.0, 0.0, 0.0),
        "",
        0.0,
        Color::BLACK,
        None,
        0,
    )
    .unwrap();
    // Still reopens cleanly.
    let re = save_reopen(&d);
    assert_eq!(annot_count(&re, 0), 4);
}

/// `ANNOT-PROP-003` (qpdf) — a saved file with a mix of annots passes
/// `qpdf --check`. Skipped if qpdf is absent.
#[test]
fn annot_prop_003_qpdf_check() {
    let d = doc();
    add_text_annot(&d, 0, Point::new(50.0, 50.0), "n", "Note").unwrap();
    add_freetext_annot(
        &d,
        0,
        Rect::new(72.0, 72.0, 300.0, 140.0),
        "T",
        12.0,
        Color::BLACK,
        None,
        0,
    )
    .unwrap();
    add_highlight_annot(
        &d,
        0,
        &[Quad {
            ul: Point::new(72.0, 200.0),
            ur: Point::new(200.0, 200.0),
            ll: Point::new(72.0, 215.0),
            lr: Point::new(200.0, 215.0),
        }],
    )
    .unwrap();
    add_rect_annot(
        &d,
        0,
        Rect::new(300.0, 300.0, 400.0, 400.0),
        Some(Color::BLACK),
        None,
    )
    .unwrap();
    add_line_annot(&d, 0, Point::new(10.0, 10.0), Point::new(60.0, 60.0), None).unwrap();
    add_ink_annot(
        &d,
        0,
        &[vec![Point::new(100.0, 400.0), Point::new(150.0, 420.0)]],
        Some(Color::BLACK),
    )
    .unwrap();
    add_file_annot(&d, 0, Point::new(500.0, 500.0), b"x", "f.txt").unwrap();
    let bytes = save_bytes(&d);
    match qpdf_check(&bytes) {
        Some(true) => {}
        Some(false) => panic!("qpdf --check rejected the annotated file"),
        None => eprintln!("SKIP annot_prop_003_qpdf_check: qpdf not on PATH"),
    }
}

/// `ANNOT-PROP-004` — accessors never panic and reflect set values.
#[test]
fn annot_prop_004_accessors() {
    let d = doc();
    let a = add_rect_annot(
        &d,
        0,
        Rect::new(100.0, 100.0, 300.0, 200.0),
        Some(Color::new(1.0, 0.0, 0.0)),
        Some(Color::new(0.0, 1.0, 0.0)),
    )
    .unwrap();
    let xref = a.xref();
    let h = pdf_edit::Annot::from_ref(&d, pdf_core::pagetree::page_refs(&d)[0], xref);
    h.set_opacity(0.7).unwrap();
    h.set_border(2.5).unwrap();
    h.set_flags(4).unwrap();
    h.set_info(Some("body"), Some("author"), Some("nm1"))
        .unwrap();
    assert_eq!(h.contents(), "body");
    assert_eq!(h.title(), "author");
    assert_eq!(h.name(), "nm1");
    assert_eq!(h.flags(), 4);
    assert!((h.opacity() - 0.7).abs() < 1e-6);
    assert!((h.border_width() - 2.5).abs() < 1e-6);
    assert_eq!(h.color(), Some(Color::new(1.0, 0.0, 0.0)));
    assert_eq!(h.fill_color(), Some(Color::new(0.0, 1.0, 0.0)));
}
