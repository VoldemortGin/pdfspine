//! `OCG-INTERP-*` — optional-content visibility in the content interpreter
//! (PRD §8.6 / §8.11, ISO 32000-1 §8.11). The shared interpreter (text
//! extraction + render-op stream) must skip glyphs, images, form XObjects,
//! path paints and shadings governed by a hidden OCG / OCMD, while still
//! advancing the text matrix and applying clips.
//!
//! IDs:
//! - OCG-INTERP-TEXT           `/OC … BDC (…) Tj EMC` glyph suppression + toggles
//! - OCG-INTERP-IMAGE          image XObject with a hidden `/OC` is not inventoried
//! - OCG-INTERP-FORM           form XObject `/OC` on vs off
//! - OCG-INTERP-OCMD-POLICY    OCMD `/OCGs … /P /AllOn` hidden section
//! - OCG-INTERP-OCMD-VE        OCMD `/VE [/Not B]` visible while B is off
//! - OCG-INTERP-PATH           hidden `re f` produces no drawing / Fill op
//! - OCG-INTERP-TM             hidden `Tj` still advances the text matrix
//! - OCG-INTERP-NESTED         hidden outer section keeps a visible-tag inner hidden
//! - OCG-INTERP-UNBALANCED-EMC a stray `EMC` in a form does not corrupt visibility
//! - OCG-INTERP-NON-OC-BDC     a non-`/OC` `BDC` hides nothing
//! - OCG-INTERP-RENDER         the ordered render-op stream honors the same rules

mod common;

use common::*;
use pdf_core::ocg::set_layer_ui_config;
use pdf_core::{Dict, DocumentStore, Limits, Name, Object, PdfString, StringKind};
use pdf_text::{interpret_page, interpret_page_render, RenderOp};

/// The two-section text stream used by the TEXT / RENDER tests: `AAAA` under
/// OCG A (`/MC0`), `BBBB` under OCG B (`/MC1`), on one baseline.
const TWO_SECTION_TEXT: &[u8] =
    b"BT /F1 12 Tf 1 0 0 1 72 700 Tm /OC /MC0 BDC (AAAA) Tj EMC /OC /MC1 BDC (BBBB) Tj EMC ET";

/// A literal PDF text string object.
fn pstr(s: &str) -> Object {
    Object::String(PdfString {
        bytes: s.as_bytes().to_vec(),
        kind: StringKind::Literal,
    })
}

/// A Letter-size MediaBox array.
fn media() -> Object {
    Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ])
}

/// A WinAnsi Helvetica with a flat 500-unit width over codes 32..=126 (so at
/// size 12 each glyph advances 6 user-space units).
fn font5() -> Object {
    winansi_type1("Helvetica", 32, &[500i64; 95])
}

/// An `/Type /OCG` dictionary with the given `/Name`.
fn ocg(name: &str) -> Object {
    Object::Dictionary(dict([("Type", name_obj("OCG")), ("Name", pstr(name))]))
}

/// The default `/OCProperties`: OCGs A(7) / B(8); `/D` has A on, B off, ordered.
fn default_ocp() -> Object {
    Object::Dictionary(dict([
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        (
            "D",
            Object::Dictionary(dict([
                ("ON", Object::Array(vec![rref(7, 0)])),
                ("OFF", Object::Array(vec![rref(8, 0)])),
                ("Order", Object::Array(vec![rref(7, 0), rref(8, 0)])),
            ])),
        ),
    ]))
}

/// A dict mapping resource names to indirect references (for `/Properties` /
/// `/XObject`).
fn ref_dict(entries: &[(&str, u32)]) -> Object {
    let mut d = Dict::new();
    for (name, num) in entries {
        d.insert(Name::new(name), rref(*num, 0));
    }
    Object::Dictionary(d)
}

/// Builds a one-page layered doc with the default OCProperties (A on, B off).
///
/// Objects: 1 catalog (+ /OCProperties → 6), 2 pages, 3 page, 4 content,
/// 5 font, 6 /OCProperties, 7 OCG A, 8 OCG B, plus `extra` (numbers ≥ 10).
/// `props` / `xobjs` fill the page `/Resources /Properties` / `/XObject`.
/// Returns `(doc, page_dict)`.
fn build_page(
    content: &[u8],
    props: &[(&str, u32)],
    xobjs: &[(&str, u32)],
    extra: Vec<(u32, Object)>,
) -> (DocumentStore, Dict) {
    let mut resources = Dict::new();
    resources.insert(Name::new("Font"), ref_dict(&[("F1", 5)]));
    if !props.is_empty() {
        resources.insert(Name::new("Properties"), ref_dict(props));
    }
    if !xobjs.is_empty() {
        resources.insert(Name::new("XObject"), ref_dict(xobjs));
    }
    let page = dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", media()),
        ("Contents", rref(4, 0)),
        ("Resources", Object::Dictionary(resources)),
    ]);
    let mut pdf = Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2, 0)),
                ("OCProperties", rref(6, 0)),
            ])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3, 0)])),
                ("Count", Object::Integer(1)),
            ])),
        )
        .obj(3, 0, Object::Dictionary(page.clone()))
        .obj(4, 0, raw_stream([], content))
        .obj(5, 0, font5())
        .obj(6, 0, default_ocp())
        .obj(7, 0, ocg("A"))
        .obj(8, 0, ocg("B"));
    for (num, obj) in extra {
        pdf = pdf.obj(num, 0, obj);
    }
    let bytes = pdf.root(1, 0).build();
    let doc = DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open layered");
    (doc, page)
}

/// A Form XObject stream that draws `text` and carries `/OC` → `oc`.
fn form_with_oc(oc: u32, text: &str) -> Object {
    let body = format!("BT /F1 12 Tf 1 0 0 1 72 700 Tm ({text}) Tj ET").into_bytes();
    raw_stream(
        [
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Form")),
            ("BBox", media()),
            ("OC", rref(oc, 0)),
        ],
        &body,
    )
}

/// The concatenated Unicode of a render-op stream's text runs.
fn render_text(ops: &[RenderOp]) -> String {
    ops.iter()
        .filter_map(|op| match op {
            RenderOp::Text(run) => Some(
                run.glyphs
                    .iter()
                    .map(|g| g.unicode.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

// === OCG-INTERP-TEXT ======================================================

/// OCG-INTERP-TEXT: only the visible layer's glyphs are produced; toggling B on
/// adds `BBBB`; then toggling A off leaves only `BBBB`.
#[test]
fn ocg_interp_text() {
    let (doc, page) = build_page(TWO_SECTION_TEXT, &[("MC0", 7), ("MC1", 8)], &[], vec![]);
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "AAAA");

    // Turn B on (row 1) → both sections show.
    set_layer_ui_config(&doc, 1, 0).expect("B on");
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "AAAABBBB");

    // Turn A off (row 0) → only B shows.
    set_layer_ui_config(&doc, 0, 2).expect("A off");
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "BBBB");
}

// === OCG-INTERP-IMAGE =====================================================

/// OCG-INTERP-IMAGE: an image XObject whose `/OC` is off is not inventoried and
/// emits no `RenderOp::Image`; turning the layer on brings it back.
#[test]
fn ocg_interp_image() {
    let img = raw_stream(
        [
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(2)),
            ("Height", Object::Integer(2)),
            ("ColorSpace", name_obj("DeviceGray")),
            ("BitsPerComponent", Object::Integer(8)),
            ("OC", rref(8, 0)), // B (off)
        ],
        &[0u8, 0, 0, 0],
    );
    let content = b"q 100 0 0 100 0 0 cm /X1 Do Q";
    let (doc, page) = build_page(content, &[], &[("X1", 10)], vec![(10, img)]);

    // B off → the image is skipped everywhere.
    assert!(interpret_page(&doc, &page).images.is_empty());
    let ops = interpret_page_render(&doc, &page);
    assert!(!ops.iter().any(|op| matches!(op, RenderOp::Image(_))));

    // B on → the image is inventoried and emitted.
    set_layer_ui_config(&doc, 1, 0).expect("B on");
    assert_eq!(interpret_page(&doc, &page).images.len(), 1);
    let ops = interpret_page_render(&doc, &page);
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, RenderOp::Image(_)))
            .count(),
        1
    );
}

// === OCG-INTERP-FORM ======================================================

/// OCG-INTERP-FORM: a form XObject with `/OC` on renders its glyphs; one with
/// `/OC` off is skipped entirely.
#[test]
fn ocg_interp_form() {
    let content = b"/XA Do /XB Do";
    let (doc, page) = build_page(
        content,
        &[],
        &[("XA", 10), ("XB", 11)],
        vec![
            (10, form_with_oc(7, "FA")), // A (on)
            (11, form_with_oc(8, "FB")), // B (off)
        ],
    );
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "FA");
}

// === OCG-INTERP-OCMD-POLICY ===============================================

/// OCG-INTERP-OCMD-POLICY: a section governed by an OCMD `/OCGs [A B] /P /AllOn`
/// is hidden while B is off (A on ≠ all on), and shows once B is on.
#[test]
fn ocg_interp_ocmd_policy() {
    let ocmd = Object::Dictionary(dict([
        ("Type", name_obj("OCMD")),
        ("OCGs", Object::Array(vec![rref(7, 0), rref(8, 0)])),
        ("P", name_obj("AllOn")),
    ]));
    let content = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm /OC /MCP BDC (PP) Tj EMC ET";
    let (doc, page) = build_page(content, &[("MCP", 10)], &[], vec![(10, ocmd)]);

    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "", "AllOn hidden");
    set_layer_ui_config(&doc, 1, 0).expect("B on");
    assert_eq!(
        glyph_text(&interpret_page(&doc, &page)),
        "PP",
        "AllOn visible"
    );
}

// === OCG-INTERP-OCMD-VE ===================================================

/// OCG-INTERP-OCMD-VE: an OCMD `/VE [/Not B]` is visible while B is off.
#[test]
fn ocg_interp_ocmd_ve() {
    let ocmd = Object::Dictionary(dict([
        ("Type", name_obj("OCMD")),
        ("VE", Object::Array(vec![name_obj("Not"), rref(8, 0)])),
    ]));
    let content = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm /OC /MCV BDC (VV) Tj EMC ET";
    let (doc, page) = build_page(content, &[("MCV", 10)], &[], vec![(10, ocmd)]);
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "VV");
}

// === OCG-INTERP-PATH ======================================================

/// OCG-INTERP-PATH: a `re f` inside a hidden section yields no drawing and no
/// `RenderOp::Fill`; a visible `re f` outside it is unaffected.
#[test]
fn ocg_interp_path() {
    let content = b"10 10 20 20 re f /OC /MC1 BDC 100 100 50 50 re f EMC";
    let (doc, page) = build_page(content, &[("MC1", 8)], &[], vec![]);

    let res = interpret_page(&doc, &page);
    assert_eq!(res.drawings.len(), 1, "only the visible fill is recorded");

    let ops = interpret_page_render(&doc, &page);
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, RenderOp::Fill { .. }))
            .count(),
        1,
        "only the visible fill emits a Fill op"
    );
}

// === OCG-INTERP-TM ========================================================

/// OCG-INTERP-TM: a hidden `Tj` suppresses its glyphs but still advances the
/// text matrix, so a following visible `Tj` starts past the hidden run's width.
#[test]
fn ocg_interp_tm_continuity() {
    // `AA` (hidden) advances 2 × 6 = 12 units; `B` (visible) starts at x = 12.
    let content = b"BT /F1 12 Tf 1 0 0 1 0 0 Tm /OC /MC1 BDC (AA) Tj EMC (B) Tj ET";
    let (doc, page) = build_page(content, &[("MC1", 8)], &[], vec![]);
    let res = interpret_page(&doc, &page);
    assert_eq!(res.glyphs.len(), 1);
    assert_eq!(res.glyphs[0].unicode.as_str(), "B");
    approx(res.glyphs[0].origin.x, 12.0, 1e-6);
    approx(res.glyphs[0].origin.y, 0.0, 1e-6);
}

// === OCG-INTERP-NESTED ====================================================

/// OCG-INTERP-NESTED: a visible-tag inner section inside a hidden outer section
/// is still hidden; only content after the outer `EMC` shows.
#[test]
fn ocg_interp_nested() {
    let content = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm \
        /OC /MC1 BDC (X) Tj /OC /MC0 BDC (Y) Tj EMC (Z) Tj EMC (W) Tj ET";
    let (doc, page) = build_page(content, &[("MC0", 7), ("MC1", 8)], &[], vec![]);
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "W");
}

// === OCG-INTERP-UNBALANCED-EMC ============================================

/// OCG-INTERP-UNBALANCED-EMC: a stray `EMC` inside a form does not underflow or
/// corrupt the page's own optional-content state (a later hidden page section
/// still hides; the form's own glyphs still show).
#[test]
fn ocg_interp_unbalanced_emc() {
    // The form starts with a stray `EMC` (empty section stack), then draws `F`.
    let form_body = b"EMC BT /F1 12 Tf 1 0 0 1 100 100 Tm (F) Tj ET".to_vec();
    let form = raw_stream(
        [
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Form")),
            ("BBox", media()),
        ],
        &form_body,
    );
    let content = b"/XF Do \
        BT /F1 12 Tf 1 0 0 1 72 700 Tm /OC /MC1 BDC (H) Tj EMC (V) Tj ET";
    let (doc, page) = build_page(content, &[("MC1", 8)], &[("XF", 10)], vec![(10, form)]);
    // `F` from the form (stray EMC harmless), `V` after the page's hidden `H`.
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "FV");
}

// === OCG-INTERP-NON-OC-BDC ================================================

/// OCG-INTERP-NON-OC-BDC: a `BDC` with a non-`/OC` tag (e.g. `/Span`) never
/// hides its content, even while a layer is off.
#[test]
fn ocg_interp_non_oc_bdc() {
    let content = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm /Span <</MCID 0>> BDC (S) Tj EMC ET";
    let (doc, page) = build_page(content, &[], &[], vec![]);
    assert_eq!(glyph_text(&interpret_page(&doc, &page)), "S");
}

// === OCG-INTERP-RENDER ====================================================

/// OCG-INTERP-RENDER: the ordered render-op stream honors optional content —
/// hidden text emits no `RenderOp::Text`; toggling the layer on brings it back.
#[test]
fn ocg_interp_render_stream() {
    let (doc, page) = build_page(TWO_SECTION_TEXT, &[("MC0", 7), ("MC1", 8)], &[], vec![]);
    assert_eq!(render_text(&interpret_page_render(&doc, &page)), "AAAA");

    set_layer_ui_config(&doc, 1, 0).expect("B on");
    assert_eq!(render_text(&interpret_page_render(&doc, &page)), "AAAABBBB");
}
