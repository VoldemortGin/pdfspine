//! `DOC-EDIT-*` / `DOC-ANNOT-*` / `DOC-FONT-*` — the `pdf-api` edit / draw /
//! annotation / handle surface (PRD §8.7–§8.9 / §9.4). Self-built classic-xref
//! fixtures (the same tiny writer used by `document_unit.rs`), exercising the
//! page-level draw/annotation free functions, the owned `AnnotHandle` /
//! `WidgetHandle` / `ShapeHandle` handles, the font-extraction path, the
//! low-level xref write plumbing, embedded files, links, OCG binding and the
//! undo/redo journal.

use pdf_api::{
    page_add_circle_annot, page_add_file_annot, page_add_freetext_annot, page_add_ink_annot,
    page_add_line_annot, page_add_polygon_annot, page_add_polyline_annot, page_add_rect_annot,
    page_add_squiggly_annot, page_add_stamp_annot, page_add_strikeout_annot, page_add_text_annot,
    page_add_underline_annot, page_add_widget, page_annot_count, page_annot_names, page_annots,
    page_delete_link, page_draw_bezier, page_draw_circle, page_draw_curve, page_draw_line,
    page_draw_oval, page_draw_polyline, page_draw_rect, page_first_annot, page_first_widget,
    page_get_cdrawings, page_get_contents, page_get_image_bbox, page_get_image_info,
    page_get_image_rects, page_get_xobjects, page_insert_link_goto, page_insert_link_uri,
    page_new_shape, page_read_contents, page_widgets, Document, FinishParams, Point, Quad, Rect,
    WidgetSpec,
};
use pdf_core::object::{Dict, Name, ObjRef, Object, PdfString, StreamObj};
use pdf_core::serialize::{write_indirect, write_object};

// --- minimal classic-xref PDF writer (test-only) --------------------------

fn dict(pairs: &[(&str, Object)]) -> Dict {
    let mut d = Dict::new();
    for (k, v) in pairs {
        d.insert(Name::new(*k), v.clone());
    }
    d
}

fn name_obj(s: &str) -> Object {
    Object::Name(Name::new(s))
}

fn rref(num: u32, gen: u16) -> Object {
    Object::Reference(ObjRef::new(num, gen))
}

fn int_array(vals: &[i64]) -> Object {
    Object::Array(vals.iter().copied().map(Object::Integer).collect())
}

fn raw_stream(extra: &[(&str, Object)], body: &[u8]) -> Object {
    let mut d = dict(extra);
    d.insert(Name::new("Length"), Object::Integer(body.len() as i64));
    Object::Stream(StreamObj::new_encoded(d, body.to_vec()))
}

/// Builds a complete classic-xref PDF from `(num, object)` pairs + trailer keys.
fn build_pdf(objects: &[(u32, Object)], root: u32, extra_trailer: &[(&str, Object)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");

    let mut max_num = 0u32;
    let mut offsets: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (num, obj) in objects {
        offsets.insert(*num, out.len());
        out.extend_from_slice(&write_indirect(ObjRef::new(*num, 0), obj));
        max_num = max_num.max(*num);
    }

    let size = max_num + 1;
    let startxref = out.len();
    out.extend_from_slice(b"xref\n");
    out.extend_from_slice(format!("0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..size {
        match offsets.get(&num) {
            Some(off) => out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }

    let mut trailer = dict(extra_trailer);
    trailer.insert(Name::new("Size"), Object::Integer(i64::from(size)));
    trailer.insert(Name::new("Root"), rref(root, 0));
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(&write_object(&Object::Dictionary(trailer)));
    out.extend_from_slice(b"\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
    out
}

/// A two-page doc: page 0 (obj 3) has a `/Contents` stream (obj 4) and a `/Font`
/// resource (obj 5); page 1 (obj 6) has a trivial `/Contents` stream (obj 7).
fn edit_doc() -> Vec<u8> {
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(2)),
        ("Kids", Object::Array(vec![rref(3, 0), rref(6, 0)])),
        ("MediaBox", int_array(&[0, 0, 300, 400])),
    ]));
    let page0 = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("Contents", rref(4, 0)),
        (
            "Resources",
            Object::Dictionary(dict(&[(
                "Font",
                Object::Dictionary(dict(&[("F1", rref(5, 0))])),
            )])),
        ),
    ]));
    let content0 = raw_stream(&[], b"BT /F1 12 Tf 40 350 Td (Hello World) Tj ET\n");
    let font = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
    ]));
    let page1 = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("Contents", rref(7, 0)),
    ]));
    let content1 = raw_stream(&[], b"q Q\n");
    build_pdf(
        &[
            (1, catalog),
            (2, pages),
            (3, page0),
            (4, content0),
            (5, font),
            (6, page1),
            (7, content1),
        ],
        1,
        &[],
    )
}

/// A blank one-page doc (obj 3), a `/Contents` stub (obj 4), 200×200.
fn blank_doc() -> Vec<u8> {
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(1)),
        ("Kids", Object::Array(vec![rref(3, 0)])),
        ("MediaBox", int_array(&[0, 0, 200, 200])),
    ]));
    let page = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("Contents", rref(4, 0)),
    ]));
    let content = raw_stream(&[], b"q Q\n");
    build_pdf(&[(1, catalog), (2, pages), (3, page), (4, content)], 1, &[])
}

// === DOC-FONT-* — font extraction / descriptor / program =================

/// A one-page doc carrying a spread of font objects for the extraction path:
/// embedded TrueType (`FontFile2`), Type0 composite (`FontFile3
/// /CIDFontType0C`), Type1 (`FontFile`), OpenType (`FontFile3 /OpenType`), a
/// bare non-embedded font, a font whose descriptor lacks any `/FontFile*`, and
/// a font with an indirect `/Widths` array.
fn font_doc() -> Vec<u8> {
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(1)),
        ("Kids", Object::Array(vec![rref(3, 0)])),
        ("MediaBox", int_array(&[0, 0, 100, 100])),
    ]));
    let page = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("Contents", rref(4, 0)),
    ]));
    let content = raw_stream(&[], b"q Q\n");

    // 5: TrueType with embedded FontFile2 (obj 6 → 7).
    let ttf = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("ABCDEF+Arial")),
        ("FontDescriptor", rref(6, 0)),
    ]));
    let ttf_desc = Object::Dictionary(dict(&[
        ("Type", name_obj("FontDescriptor")),
        ("FontName", name_obj("ABCDEF+Arial")),
        ("FontFile2", rref(7, 0)),
    ]));
    let ttf_prog = raw_stream(&[], b"TTF-PROGRAM-BYTES");

    // 8: Type0 composite → descendant CIDFont (9) → descriptor (10) → FontFile3
    // /CIDFontType0C (11). Top-level dict has no /FontDescriptor.
    let type0 = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("BaseFont", name_obj("XYZAAA+Song")),
        ("DescendantFonts", Object::Array(vec![rref(9, 0)])),
    ]));
    let cidfont = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType0")),
        ("BaseFont", name_obj("XYZAAA+Song")),
        ("FontDescriptor", rref(10, 0)),
    ]));
    let cid_desc = Object::Dictionary(dict(&[
        ("Type", name_obj("FontDescriptor")),
        ("FontFile3", rref(11, 0)),
    ]));
    let cid_prog = raw_stream(&[("Subtype", name_obj("CIDFontType0C"))], b"CID-PROG");

    // 12: Type1 with FontFile (13 → 14).
    let type1 = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("PfaFont")),
        ("FontDescriptor", rref(13, 0)),
    ]));
    let type1_desc = Object::Dictionary(dict(&[
        ("Type", name_obj("FontDescriptor")),
        ("FontFile", rref(14, 0)),
    ]));
    let type1_prog = raw_stream(&[], b"PFA-PROGRAM");

    // 15: TrueType with OpenType FontFile3 (16 → 17).
    let otf = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("OtfFont")),
        ("FontDescriptor", rref(16, 0)),
    ]));
    let otf_desc = Object::Dictionary(dict(&[
        ("Type", name_obj("FontDescriptor")),
        ("FontFile3", rref(17, 0)),
    ]));
    let otf_prog = raw_stream(&[("Subtype", name_obj("OpenType"))], b"OTF-PROG");

    // 18: font with no descriptor at all.
    let no_desc = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("NoEmbed")),
    ]));

    // 19: font whose descriptor (20) carries no /FontFile* program.
    let bare = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("BareDesc")),
        ("FontDescriptor", rref(20, 0)),
    ]));
    let bare_desc = Object::Dictionary(dict(&[
        ("Type", name_obj("FontDescriptor")),
        ("Flags", Object::Integer(32)),
    ]));

    // 21: font with an indirect /Widths array (22) and /FirstChar.
    let widref = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("WidRef")),
        ("FirstChar", Object::Integer(32)),
        ("Widths", rref(22, 0)),
    ]));
    let widths = int_array(&[250, 333, 408]);

    build_pdf(
        &[
            (1, catalog),
            (2, pages),
            (3, page),
            (4, content),
            (5, ttf),
            (6, ttf_desc),
            (7, ttf_prog),
            (8, type0),
            (9, cidfont),
            (10, cid_desc),
            (11, cid_prog),
            (12, type1),
            (13, type1_desc),
            (14, type1_prog),
            (15, otf),
            (16, otf_desc),
            (17, otf_prog),
            (18, no_desc),
            (19, bare),
            (20, bare_desc),
            (21, widref),
            (22, widths),
        ],
        1,
        &[],
    )
}

#[test]
fn doc_font_001_extract_embedded_variants() {
    // DOC-FONT-001: extract_font returns (basefont, ext, type, buffer) for each
    // embedded-program flavour; `ext` follows fitz's format tags.
    let doc = Document::open_bytes(font_doc()).unwrap();

    let (base, ext, ty, buf) = doc.extract_font(5, false);
    assert_eq!(base, "ABCDEF+Arial");
    assert_eq!(ext, "ttf");
    assert_eq!(ty, "TrueType");
    assert_eq!(buf, b"TTF-PROGRAM-BYTES");

    // info_only suppresses the buffer.
    let (_, _, _, buf_info) = doc.extract_font(5, true);
    assert!(buf_info.is_empty());

    // Type0 composite → descendant descriptor → CIDFontType0C.
    let (_, ext0, ty0, buf0) = doc.extract_font(8, false);
    assert_eq!(ext0, "cid");
    assert_eq!(ty0, "Type0");
    assert_eq!(buf0, b"CID-PROG");

    // OpenType and Type1.
    assert_eq!(doc.extract_font(15, false).1, "otf");
    assert_eq!(doc.extract_font(12, false).1, "pfa");
}

#[test]
fn doc_font_002_extract_non_embedded_and_missing() {
    // DOC-FONT-002: a known font with no descriptor → ("n/a", empty buffer); a
    // descriptor without /FontFile* → also "n/a"; a non-font / missing xref →
    // all-empty.
    let doc = Document::open_bytes(font_doc()).unwrap();

    let (base, ext, ty, buf) = doc.extract_font(18, false);
    assert_eq!(base, "NoEmbed");
    assert_eq!(ext, "n/a");
    assert_eq!(ty, "Type1");
    assert!(buf.is_empty());

    // Descriptor present but no embedded program → still "n/a".
    assert_eq!(doc.extract_font(19, false).1, "n/a");

    // A stream object (the content) is not a font dict → all-empty.
    assert_eq!(
        doc.extract_font(4, false),
        (String::new(), String::new(), String::new(), Vec::new())
    );
    // The /Pages node is a dict but not /Type /Font → all-empty.
    assert!(doc.extract_font(2, false).0.is_empty());
    // A missing xref → all-empty.
    assert!(doc.extract_font(999, false).3.is_empty());
}

#[test]
fn doc_font_003_subset_fonts_counts_embedded() {
    // DOC-FONT-003: subset_fonts counts every font object carrying an embedded
    // program (top-level or descendant), never mutating the document.
    let doc = Document::open_bytes(font_doc()).unwrap();
    // Embedded: 5 (ttf), 8 (Type0), 9 (CIDFont), 12 (Type1), 15 (otf) = 5.
    assert_eq!(doc.subset_fonts(), 5);
    // Idempotent / non-mutating.
    assert_eq!(doc.subset_fonts(), 5);
    assert!(!doc.is_dirty());
}

#[test]
fn doc_font_004_get_char_widths() {
    // DOC-FONT-004: get_char_widths resolves an indirect /Widths array and
    // scales by 1/1000, keyed from /FirstChar.
    let doc = Document::open_bytes(font_doc()).unwrap();
    let widths = doc.get_char_widths(21);
    assert_eq!(widths, vec![(32, 0.25), (33, 0.333), (34, 0.408)]);

    // A missing xref and a non-dict (stream) xref both yield an empty list.
    assert!(doc.get_char_widths(999).is_empty());
    assert!(doc.get_char_widths(4).is_empty());
    // The plain Helvetica in edit_doc has no /Widths.
    let plain = Document::open_bytes(edit_doc()).unwrap();
    assert!(plain.get_char_widths(5).is_empty());
}

// === DOC-EDIT-* — page draw free functions ================================

#[test]
fn doc_edit_001_draw_primitives_grow_content() {
    // DOC-EDIT-001: every draw_* free function appends to the page content.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    let before = page_read_contents(&page).len();

    let black = (0.0, 0.0, 0.0);
    let red = Some((1.0, 0.0, 0.0));
    let blue = Some((0.0, 0.0, 1.0));
    page_draw_line(
        &page,
        Point::new(10.0, 10.0),
        Point::new(90.0, 90.0),
        black,
        1.5,
    )
    .unwrap();
    page_draw_rect(&page, Rect::new(10.0, 10.0, 60.0, 40.0), red, blue, 1.0).unwrap();
    page_draw_circle(&page, Point::new(50.0, 50.0), 20.0, red, None, 1.0).unwrap();
    page_draw_oval(&page, Rect::new(10.0, 60.0, 80.0, 100.0), None, blue, 1.0).unwrap();
    page_draw_bezier(
        &page,
        Point::new(0.0, 0.0),
        Point::new(10.0, 30.0),
        Point::new(30.0, 30.0),
        Point::new(40.0, 0.0),
        black,
        2.0,
    )
    .unwrap();
    page_draw_polyline(
        &page,
        &[
            Point::new(10.0, 10.0),
            Point::new(40.0, 80.0),
            Point::new(90.0, 20.0),
        ],
        black,
        1.0,
    )
    .unwrap();
    page_draw_curve(
        &page,
        &[
            Point::new(10.0, 10.0),
            Point::new(30.0, 60.0),
            Point::new(70.0, 60.0),
            Point::new(90.0, 10.0),
        ],
        black,
        1.0,
    )
    .unwrap();

    let raw = page_read_contents(&page);
    assert!(raw.len() > before, "content should grow after drawing");
    // A rectangle emits the `re` operator.
    assert!(raw.windows(3).any(|w| w == b" re"));

    // The drawings survive a full save + reopen.
    let bytes = doc
        .save_to_bytes(&pdf_core::SaveOptions::default().with_garbage(1))
        .unwrap();
    let re = Document::open_bytes(bytes).unwrap();
    let drawings = pdf_api::page_get_drawings(&re.load_page(0).unwrap());
    assert!(!drawings.is_empty());
}

#[test]
fn doc_edit_002_shape_handle_commit() {
    // DOC-EDIT-002: ShapeHandle records every primitive, `finish` flushes a
    // styled block, and a drawn-but-unfinished trailing block is committed with
    // the default black stroke.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    let before = page_read_contents(&page).len();

    let mut shape = page_new_shape(&page);
    shape.draw_line(Point::new(0.0, 0.0), Point::new(20.0, 20.0));
    shape.draw_rect(Rect::new(5.0, 5.0, 25.0, 25.0));
    shape.draw_circle(Point::new(50.0, 50.0), 10.0);
    shape.draw_oval(Rect::new(10.0, 10.0, 40.0, 30.0));
    shape.draw_bezier(
        Point::new(0.0, 0.0),
        Point::new(5.0, 15.0),
        Point::new(15.0, 15.0),
        Point::new(20.0, 0.0),
    );
    shape.draw_polyline(vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)]);
    shape.draw_curve(vec![
        Point::new(0.0, 0.0),
        Point::new(5.0, 10.0),
        Point::new(15.0, 10.0),
        Point::new(20.0, 0.0),
    ]);
    shape.close_path();
    shape.finish(FinishParams {
        color: Some((0.0, 0.0, 0.0)),
        fill: Some((0.5, 0.5, 0.5)),
        width: 1.0,
        dashes: Some("[3] 0".to_string()),
        even_odd: false,
        close_path: true,
    });
    // A second, unfinished block picks up the default black stroke at commit.
    shape.draw_line(Point::new(30.0, 30.0), Point::new(60.0, 60.0));
    shape.commit().unwrap();

    let raw = page_read_contents(&page);
    assert!(raw.len() > before);
    assert!(raw.windows(3).any(|w| w == b" re"));
}

#[test]
fn doc_edit_003_empty_shape_commit_is_noop() {
    // DOC-EDIT-003: committing a shape with no primitives is a clean no-op.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    let before = page_read_contents(&page).len();
    page_new_shape(&page).commit().unwrap();
    assert_eq!(page_read_contents(&page).len(), before);
}

// === DOC-ANNOT-* — annotation free functions + AnnotHandle ================

#[test]
fn doc_annot_001_markup_and_shape_annots() {
    // DOC-ANNOT-001: the annotation free functions each create an annotation of
    // the right /Subtype and hand back a live AnnotHandle.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    let quads = [Quad::from_rect(&Rect::new(10.0, 10.0, 90.0, 24.0))];

    assert_eq!(
        page_add_text_annot(&page, Point::new(20.0, 20.0), "note", "Note")
            .unwrap()
            .type_string(),
        "Text"
    );
    assert_eq!(
        page_add_underline_annot(&page, &quads)
            .unwrap()
            .type_string(),
        "Underline"
    );
    assert_eq!(
        page_add_strikeout_annot(&page, &quads)
            .unwrap()
            .type_string(),
        "StrikeOut"
    );
    assert_eq!(
        page_add_squiggly_annot(&page, &quads)
            .unwrap()
            .type_string(),
        "Squiggly"
    );
    assert_eq!(
        page_add_rect_annot(
            &page,
            Rect::new(10.0, 30.0, 60.0, 60.0),
            Some((1.0, 0.0, 0.0)),
            None
        )
        .unwrap()
        .type_string(),
        "Square"
    );
    assert_eq!(
        page_add_circle_annot(
            &page,
            Rect::new(10.0, 70.0, 60.0, 110.0),
            Some((0.0, 0.0, 1.0)),
            Some((0.9, 0.9, 0.9)),
        )
        .unwrap()
        .type_string(),
        "Circle"
    );
    assert_eq!(
        page_add_line_annot(&page, Point::new(0.0, 0.0), Point::new(50.0, 50.0), None)
            .unwrap()
            .type_string(),
        "Line"
    );

    // Every added annotation is discoverable on the page.
    assert!(page_annot_count(&page) >= 7);
    assert!(page_first_annot(&page).unwrap().is_some());
}

#[test]
fn doc_annot_002_freetext_polygon_ink_stamp_file() {
    // DOC-ANNOT-002: the remaining annotation constructors (FreeText, Polygon,
    // PolyLine, Ink, Stamp, FileAttachment).
    let doc = Document::open_bytes(edit_doc()).unwrap();
    let page = doc.load_page(0).unwrap();

    let ft = page_add_freetext_annot(
        &page,
        Rect::new(10.0, 10.0, 120.0, 40.0),
        "free text",
        11.0,
        (0.0, 0.0, 0.0),
        Some((1.0, 1.0, 0.8)),
        1,
    )
    .unwrap();
    assert_eq!(ft.type_string(), "FreeText");

    let poly_pts = [
        Point::new(10.0, 10.0),
        Point::new(40.0, 80.0),
        Point::new(90.0, 20.0),
    ];
    let polygon = page_add_polygon_annot(&page, &poly_pts, Some((0.0, 0.0, 0.0)), None).unwrap();
    assert_eq!(polygon.type_string(), "Polygon");
    // A polygon carries its vertices.
    assert_eq!(polygon.vertices().len(), 3);

    let polyline = page_add_polyline_annot(&page, &poly_pts, Some((0.0, 0.0, 0.0))).unwrap();
    assert_eq!(polyline.type_string(), "PolyLine");

    let ink = page_add_ink_annot(
        &page,
        &[vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)]],
        Some((0.0, 0.0, 0.0)),
    )
    .unwrap();
    assert_eq!(ink.type_string(), "Ink");

    let stamp = page_add_stamp_annot(&page, Rect::new(10.0, 10.0, 80.0, 40.0), "Approved").unwrap();
    assert_eq!(stamp.type_string(), "Stamp");

    let file = page_add_file_annot(
        &page,
        Point::new(20.0, 20.0),
        b"payload",
        "attach.bin",
        Some("desc"),
    )
    .unwrap();
    assert_eq!(file.type_string(), "FileAttachment");
    // The attachment reads back its bytes and metadata.
    assert_eq!(file.get_file().unwrap(), b"payload");
    let (fname, fdesc, flen) = file.file_info().unwrap();
    assert_eq!(fname, "attach.bin");
    assert_eq!(fdesc, "desc");
    assert_eq!(flen, 7);
}

#[test]
fn doc_annot_003_handle_getters_and_setters() {
    // DOC-ANNOT-003: AnnotHandle exposes /Rect, colors, opacity, border, flags,
    // info; the setters round-trip through a re-read handle.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    let handle = page_add_rect_annot(
        &page,
        Rect::new(10.0, 10.0, 60.0, 40.0),
        Some((1.0, 0.0, 0.0)),
        None,
    )
    .unwrap();

    // Getters.
    let r = handle.rect();
    assert!((r.x1 - r.x0 - 50.0).abs() < 1.0);
    assert_eq!(handle.annot_type().pdf_name(), "Square");
    let _ = handle.opacity();
    let _ = handle.border_width();
    let _ = handle.flags();

    // Setters + info round-trip.
    handle.set_opacity(0.5).unwrap();
    handle.set_flags(4).unwrap();
    handle
        .set_info(Some("body text"), Some("author"), Some("nm-1"))
        .unwrap();
    handle.update().unwrap();

    // Re-read the same annotation through a fresh handle and confirm the info.
    let xref = handle.xref();
    let again = page_annots(&page)
        .unwrap()
        .into_iter()
        .find(|a| a.xref() == xref)
        .unwrap();
    let info = again.info();
    assert_eq!(info.content, "body text");
    assert_eq!(info.title, "author");
    assert_eq!(info.name, "nm-1");
    assert_eq!(again.flags(), 4);
    assert!((again.opacity() - 0.5).abs() < 1e-9);

    // annot_names surfaces the /NM we set.
    assert!(page_annot_names(&page).contains(&"nm-1".to_string()));
}

// === DOC-EDIT-* — links ===================================================

#[test]
fn doc_edit_004_links_insert_get_delete() {
    // DOC-EDIT-004: Document + page link insertion, listing and deletion.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    doc.insert_link_uri(0, Rect::new(10.0, 10.0, 90.0, 24.0), "https://example.com")
        .unwrap();
    doc.insert_link_goto(0, Rect::new(10.0, 30.0, 90.0, 44.0), 1)
        .unwrap();
    let links = doc.get_links(0);
    assert_eq!(links.len(), 2);
    let goto_xref = links
        .iter()
        .find(|l| matches!(l.kind, pdf_api::LinkKind::Goto(_)))
        .map(|l| l.xref)
        .unwrap();
    doc.delete_link(0, goto_xref).unwrap();
    assert_eq!(doc.get_links(0).len(), 1);

    // The page-level free functions operate on a Page handle.
    let page = doc.load_page(0).unwrap();
    page_insert_link_uri(
        &page,
        Rect::new(10.0, 50.0, 90.0, 64.0),
        "https://rust-lang.org",
    )
    .unwrap();
    page_insert_link_goto(&page, Rect::new(10.0, 70.0, 90.0, 84.0), 1).unwrap();
    let count = pdf_api::page_get_links(&page).len();
    assert_eq!(count, 3);
    let uri_xref = pdf_api::page_get_links(&page)
        .iter()
        .find(|l| matches!(l.kind, pdf_api::LinkKind::Uri(_)))
        .map(|l| l.xref)
        .unwrap();
    page_delete_link(&page, uri_xref).unwrap();
    assert_eq!(pdf_api::page_get_links(&page).len(), 2);
}

// === DOC-EDIT-* — forms / widgets =========================================

#[test]
fn doc_edit_005_widgets_and_form_fill() {
    // DOC-EDIT-005: add_widget builds an AcroForm; form_field_names lists the
    // fields, form_fill sets a value, and WidgetHandle reads the field facts.
    let doc = Document::open_bytes(blank_doc()).unwrap();
    let page = doc.load_page(0).unwrap();

    page_add_widget(
        &page,
        &WidgetSpec {
            rect: Rect::new(10.0, 10.0, 120.0, 30.0),
            field_name: "fname".to_string(),
            field_type: 7, // text
            field_value: String::new(),
            field_flags: 0,
            choice_values: Vec::new(),
            text_color: (0.0, 0.0, 0.0),
            text_font: "Helv".to_string(),
            text_fontsize: 0.0,
        },
    )
    .unwrap();
    page_add_widget(
        &page,
        &WidgetSpec {
            rect: Rect::new(10.0, 40.0, 30.0, 60.0),
            field_name: "agree".to_string(),
            field_type: 2, // checkbox
            field_value: String::new(),
            field_flags: 0,
            choice_values: Vec::new(),
            text_color: (0.0, 0.0, 0.0),
            text_font: "Helv".to_string(),
            text_fontsize: 0.0,
        },
    )
    .unwrap();

    assert!(doc.is_form_pdf());
    let names = doc.form_field_names();
    assert!(names.contains(&"fname".to_string()));
    assert!(names.contains(&"agree".to_string()));

    // Widget handles on the page.
    let widgets = page_widgets(&page);
    assert_eq!(widgets.len(), 2);
    assert!(page_first_widget(&page).is_some());
    let text_w = widgets.iter().find(|w| w.field_name() == "fname").unwrap();
    assert!(text_w.xref() > 0);
    let wr = text_w.rect();
    assert!(wr.x1 > wr.x0);
    assert!(!text_w.field_type_string().is_empty());
    let _ = text_w.field_type();
    let _ = text_w.field_label();
    let _ = text_w.field_flags();
    let check_w = widgets.iter().find(|w| w.field_name() == "agree").unwrap();
    let _ = check_w.button_states();

    // Fill the text field and confirm the stored value.
    doc.form_fill("fname", "Ada").unwrap();
    let filled = page_widgets(&page)
        .into_iter()
        .find(|w| w.field_name() == "fname")
        .unwrap();
    assert_eq!(filled.field_value().as_deref(), Some("Ada"));
}

#[test]
fn doc_edit_006_form_flatten() {
    // DOC-EDIT-006: form_flatten bakes the widgets into page content and clears
    // the interactive form.
    let doc = Document::open_bytes(blank_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    page_add_widget(
        &page,
        &WidgetSpec {
            rect: Rect::new(10.0, 10.0, 120.0, 30.0),
            field_name: "flat".to_string(),
            field_type: 7,
            field_value: "baked".to_string(),
            field_flags: 0,
            choice_values: Vec::new(),
            text_color: (0.0, 0.0, 0.0),
            text_font: "Helv".to_string(),
            text_fontsize: 0.0,
        },
    )
    .unwrap();
    assert!(doc.is_form_pdf());
    doc.form_flatten().unwrap();
    // After flattening there are no interactive widgets left on the page.
    assert!(page_widgets(&doc.load_page(0).unwrap()).is_empty());
}

// === DOC-EDIT-* — xref write plumbing =====================================

#[test]
fn doc_edit_007_xref_read_helpers() {
    // DOC-EDIT-007: the companion xref read helpers (pdf_trailer, is_stream,
    // xref_stream_raw, xref_get_keys, xref_is_xobject).
    let doc = Document::open_bytes(edit_doc()).unwrap();
    assert!(doc.pdf_trailer().contains("/Root"));

    assert!(doc.is_stream(4).unwrap());
    assert!(!doc.is_stream(1).unwrap());
    // The raw (still-encoded) body of the unfiltered content stream.
    assert!(doc.xref_stream_raw(4).unwrap().starts_with(b"BT "));

    let keys = doc.xref_get_keys(1).unwrap();
    assert!(keys.iter().any(|k| k == "Type"));
    assert!(keys.iter().any(|k| k == "Pages"));
    assert!(doc.xref_get_keys(999).unwrap().is_empty());

    // Add a Form XObject and an Image XObject to distinguish the predicates.
    let form = doc.get_new_xref().unwrap();
    doc.update_object(
        form,
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] >>",
    )
    .unwrap();
    assert!(doc.xref_is_xobject(form).unwrap());
    assert!(!doc.xref_is_image(form).unwrap());
    assert!(!doc.xref_is_xobject(4).unwrap());
}

#[test]
fn doc_edit_008_update_object_and_stream() {
    // DOC-EDIT-008: update_object replaces a dict (stream body preserved) and
    // rejects unparseable text; update_stream covers the create / replace /
    // error branches.
    let doc = Document::open_bytes(edit_doc()).unwrap();

    // Replace a stream object's *dictionary* while keeping its raw body.
    let original_body = doc.xref_stream(4).unwrap();
    doc.update_object(4, "<< /Type /MyStream /Custom 7 >>")
        .unwrap();
    assert_eq!(
        doc.xref_get_key(4, "Type").unwrap().as_deref(),
        Some("/MyStream")
    );
    assert_eq!(doc.xref_stream(4).unwrap(), original_body);

    // Unparseable text is a typed error.
    assert!(doc.update_object(1, ">>").is_err());

    // update_stream on the existing stream (compress off).
    doc.update_stream(4, b"NEW BODY".to_vec(), false, false)
        .unwrap();
    assert_eq!(doc.xref_stream(4).unwrap(), b"NEW BODY");

    // new=true turns a plain dict object (the catalog) into a stream — the
    // dictionary keys survive.
    let dict_obj = doc.get_new_xref().unwrap();
    doc.update_object(dict_obj, "<< /Kind /Holder >>").unwrap();
    doc.update_stream(dict_obj, b"stream data".to_vec(), true, false)
        .unwrap();
    assert!(doc.is_stream(dict_obj).unwrap());
    assert_eq!(doc.xref_stream(dict_obj).unwrap(), b"stream data");

    // new=true on a Null slot creates a fresh stream.
    let null_obj = doc.get_new_xref().unwrap();
    doc.update_stream(null_obj, b"from null".to_vec(), true, true)
        .unwrap();
    assert_eq!(doc.xref_stream(null_obj).unwrap(), b"from null");

    // new=true on a wholly out-of-range xref also creates one.
    let far = doc.xref_length() + 5;
    doc.update_stream(far, b"far".to_vec(), true, false)
        .unwrap();
    assert_eq!(doc.xref_stream(far).unwrap(), b"far");

    // update_stream on a non-stream with new=false is an error.
    assert!(doc.update_stream(1, b"x".to_vec(), false, false).is_err());
}

// === DOC-EDIT-* — embedded files ==========================================

#[test]
fn doc_edit_009_embedded_files_lifecycle() {
    // DOC-EDIT-009: embfile add / get / names / count / info / upd / del, plus
    // the not-found error branches.
    let doc = Document::open_bytes(blank_doc()).unwrap();
    doc.embfile_add(
        "data.txt",
        b"hello",
        Some("data.txt"),
        None,
        Some("greeting"),
    )
    .unwrap();
    assert_eq!(doc.embfile_count(), 1);
    assert_eq!(doc.embfile_names(), vec!["data.txt".to_string()]);
    assert_eq!(doc.embfile_get("data.txt").unwrap(), b"hello");

    let info = doc.embfile_info("data.txt").unwrap();
    assert_eq!(info.name, "data.txt");
    assert_eq!(info.desc, "greeting");
    assert_eq!(info.length, 5);

    // Update the content + description in place.
    doc.embfile_upd(
        "data.txt",
        Some(b"goodbye world"),
        None,
        None,
        Some("farewell"),
        Some("D:20240101000000Z"),
    )
    .unwrap();
    assert_eq!(doc.embfile_get("data.txt").unwrap(), b"goodbye world");
    assert_eq!(doc.embfile_info("data.txt").unwrap().desc, "farewell");

    // Adding a duplicate name is rejected.
    assert!(doc.embfile_add("data.txt", b"x", None, None, None).is_err());
    // Missing-name operations are typed errors.
    assert!(doc.embfile_get("missing").is_err());
    assert!(doc.embfile_info("missing").is_err());

    // Delete removes it.
    doc.embfile_del("data.txt").unwrap();
    assert_eq!(doc.embfile_count(), 0);
    assert!(doc.embfile_del("data.txt").is_err());
}

// === DOC-EDIT-* — OCG binding, journal, misc state ========================

#[test]
fn doc_edit_010_set_oc_binds_object() {
    // DOC-EDIT-010: add_ocg then set_oc writes an /OC entry onto the target.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    let ocg = doc.add_ocg("Layer 1", true, &[], None).unwrap();
    assert!(ocg > 0);
    doc.set_oc(4, ocg).unwrap();
    let oc = doc.xref_get_key(4, "OC").unwrap();
    assert!(oc.is_some());
    assert!(oc.unwrap().contains(&ocg.to_string()));
}

#[test]
fn doc_edit_011_journal_undo_redo_cycle() {
    // DOC-EDIT-011: enable the journal, checkpoint a structural edit, then undo
    // and redo it.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    doc.journal_enable();
    assert!(doc.journal_is_enabled());
    // Nothing recorded yet → undo is impossible and a no-op.
    assert!(!doc.journal_can_undo());
    assert!(!doc.journal_undo());

    doc.new_page(None, 100.0, 100.0).unwrap();
    assert_eq!(doc.page_count(), 3);
    doc.journal_save_state();
    assert!(doc.journal_can_undo());

    assert!(doc.journal_undo());
    assert_eq!(doc.page_count(), 2);
    assert!(doc.journal_can_redo());
    assert!(doc.journal_redo());
    assert_eq!(doc.page_count(), 3);
    // Nothing left to redo.
    assert!(!doc.journal_redo());
}

#[test]
fn doc_edit_012_journal_disabled_is_inert() {
    // DOC-EDIT-012: without journal_enable, undo/redo report false.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    assert!(!doc.journal_is_enabled());
    assert!(!doc.journal_undo());
    assert!(!doc.journal_redo());
    // save_state on a disabled journal is a silent no-op.
    doc.journal_save_state();
    assert!(!doc.journal_can_undo());
}

#[test]
fn doc_edit_013_state_flags_and_page_ops() {
    // DOC-EDIT-013: is_dirty / is_fast_webaccess / can_save_incrementally,
    // authenticate on a plain doc, set_page_rotation, set_mark_info, bake and
    // fullcopy_page_to.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    assert!(!doc.is_dirty());
    assert!(!doc.is_fast_webaccess());
    let _ = doc.can_save_incrementally();
    // An unencrypted document authenticates trivially.
    assert!(doc.authenticate(b"anything"));

    doc.set_page_rotation(0, 90).unwrap();
    assert_eq!(doc.load_page(0).unwrap().rotation(), 90);
    assert!(doc.is_dirty());

    doc.set_mark_info(true, false, true).unwrap();
    assert_eq!(doc.mark_info(), Some((true, false, true)));

    doc.bake(true, true).unwrap();

    // fullcopy_page_to with to >= page_count appends.
    let count = doc.page_count();
    let idx = doc.fullcopy_page_to(0, 999).unwrap();
    assert_eq!(idx, count);
    assert_eq!(doc.page_count(), count + 1);
    // A finite earlier position moves the copy into place.
    let front = doc.fullcopy_page_to(0, 0).unwrap();
    assert_eq!(front, 0);
}

// === DOC-EDIT-* — page content / query free functions =====================

/// A one-page doc whose `/Contents` is an array of two stream refs (obj 4, 5),
/// plus an image + form XObject in the page resources (obj 6, 7).
fn array_contents_doc() -> Vec<u8> {
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(1)),
        ("Kids", Object::Array(vec![rref(3, 0)])),
        ("MediaBox", int_array(&[0, 0, 200, 200])),
    ]));
    let page = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        // A trailing `null` array member exercises the non-reference paths of
        // page_get_contents / page_read_contents (both skip it).
        (
            "Contents",
            Object::Array(vec![rref(4, 0), rref(5, 0), Object::Null]),
        ),
        (
            "Resources",
            Object::Dictionary(dict(&[(
                "XObject",
                Object::Dictionary(dict(&[("Im0", rref(6, 0)), ("Fm0", rref(7, 0))])),
            )])),
        ),
    ]));
    let c0 = raw_stream(&[], b"q 50 0 0 50 20 20 cm /Im0 Do Q");
    let c1 = raw_stream(&[], b"q 30 0 0 30 100 100 cm /Fm0 Do Q");
    let image = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(2)),
            ("Height", Object::Integer(2)),
            ("BitsPerComponent", Object::Integer(8)),
            ("ColorSpace", name_obj("DeviceRGB")),
        ],
        &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
    );
    let form = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Form")),
            ("BBox", int_array(&[0, 0, 100, 100])),
        ],
        b"q Q",
    );
    build_pdf(
        &[
            (1, catalog),
            (2, pages),
            (3, page),
            (4, c0),
            (5, c1),
            (6, image),
            (7, form),
        ],
        1,
        &[],
    )
}

#[test]
fn doc_edit_014_page_contents_array_join() {
    // DOC-EDIT-014: page_get_contents lists each /Contents array member's xref;
    // page_read_contents joins the decoded streams with a newline separator.
    let doc = Document::open_bytes(array_contents_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    assert_eq!(page_get_contents(&page), vec![4, 5]);

    let raw = page_read_contents(&page);
    assert!(raw.windows(7).any(|w| w == b"/Im0 Do"));
    assert!(raw.windows(7).any(|w| w == b"/Fm0 Do"));
    // The newline separator sits between the two streams.
    assert!(raw.contains(&b'\n'));

    // A page with no /Contents yields an empty list and empty bytes.
    let blank = Document::open_bytes(blank_doc()).unwrap();
    let bpage = blank.load_page(0).unwrap();
    assert_eq!(page_get_contents(&bpage), vec![4]);
}

#[test]
fn doc_edit_015_page_xobject_and_image_queries() {
    // DOC-EDIT-015: the page_get_* wrappers for XObjects and image placements.
    let doc = Document::open_bytes(array_contents_doc()).unwrap();
    let page = doc.load_page(0).unwrap();

    let xobjs = page_get_xobjects(&page);
    assert!(xobjs.iter().any(|x| x.name == "Im0"));
    assert!(xobjs.iter().any(|x| x.name == "Fm0"));

    let rects = page_get_image_rects(&page);
    assert!(!rects.is_empty());
    let infos = page_get_image_info(&page);
    assert!(!infos.is_empty());

    // The bbox of the placed image is discoverable by its resource name.
    let name = infos[0].name.clone();
    assert!(page_get_image_bbox(&page, &name).is_some());

    // cdrawings is defined (empty for a doc with only image placements).
    let _ = page_get_cdrawings(&page);

    // delete_image swaps the image for a transparent stub; a missing target is
    // a typed error.
    pdf_api::page_delete_image(&page, "Im0").unwrap();
    assert!(pdf_api::page_delete_image(&page, "NoSuchImage").is_err());
}

#[test]
fn doc_edit_016_page_language_roundtrip() {
    // DOC-EDIT-016: page_set_language writes /Lang; page_language reads it back
    // (normalized to MuPDF's compact form); an empty tag removes it.
    let doc = Document::open_bytes(blank_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    pdf_api::page_set_language(&page, "en-US").unwrap();
    let lang = pdf_api::page_language(&page).unwrap();
    assert!(lang.to_ascii_lowercase().starts_with("en"));

    pdf_api::page_set_language(&page, "").unwrap();
    assert_eq!(pdf_api::page_language(&page), None);
}

// === DOC-EDIT-* — error branches & content edge shapes ====================

/// A two-page doc: page 0 (obj 3) has a `/Contents` that is a bare `/Name`
/// (neither reference nor array); page 1 (obj 4) has no `/Contents` at all.
fn weird_contents_doc() -> Vec<u8> {
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(2)),
        ("Kids", Object::Array(vec![rref(3, 0), rref(4, 0)])),
        ("MediaBox", int_array(&[0, 0, 100, 100])),
    ]));
    let name_page = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("Contents", name_obj("Bogus")),
    ]));
    let no_contents =
        Object::Dictionary(dict(&[("Type", name_obj("Page")), ("Parent", rref(2, 0))]));
    build_pdf(
        &[(1, catalog), (2, pages), (3, name_page), (4, no_contents)],
        1,
        &[],
    )
}

#[test]
fn doc_edit_017_error_branches() {
    // DOC-EDIT-017: link insertion on an out-of-range page, update_stream on a
    // missing object with new=false, and a named-destination miss all return
    // typed errors / None rather than panicking.
    let doc = Document::open_bytes(edit_doc()).unwrap();
    assert!(doc
        .insert_link_uri(99, Rect::new(0.0, 0.0, 10.0, 10.0), "u")
        .is_err());
    assert!(doc
        .insert_link_goto(99, Rect::new(0.0, 0.0, 10.0, 10.0), 0)
        .is_err());

    let missing = doc.xref_length() + 50;
    assert!(doc
        .update_stream(missing, b"x".to_vec(), false, false)
        .is_err());

    // A bare named destination that does not exist resolves to None.
    assert_eq!(doc.resolve_link("NoSuchNamedDest"), None);
}

#[test]
fn doc_edit_018_page_contents_edge_shapes() {
    // DOC-EDIT-018: a `/Name` or absent `/Contents` yields an empty xref list
    // and empty concatenated bytes (no panic).
    let doc = Document::open_bytes(weird_contents_doc()).unwrap();
    let name_page = doc.load_page(0).unwrap();
    assert!(page_get_contents(&name_page).is_empty());
    assert!(page_read_contents(&name_page).is_empty());

    let no_page = doc.load_page(1).unwrap();
    assert!(page_get_contents(&no_page).is_empty());
    assert!(page_read_contents(&no_page).is_empty());
}

#[test]
fn doc_edit_019_utf8_bom_info_decodes() {
    // DOC-EDIT-019: an /Info string carrying a UTF-8 BOM (PDF 2.0) decodes to
    // its text via decode_pdf_text.
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(0)),
        ("Kids", Object::Array(vec![])),
    ]));
    // "Hi" prefixed with the UTF-8 BOM EF BB BF.
    let title = vec![0xEF, 0xBB, 0xBF, b'H', b'i'];
    let info = Object::Dictionary(dict(&[(
        "Title",
        Object::String(PdfString::literal(title)),
    )]));
    let bytes = build_pdf(
        &[(1, catalog), (2, pages), (5, info)],
        1,
        &[("Info", rref(5, 0))],
    );
    let doc = Document::open_bytes(bytes).unwrap();
    assert_eq!(doc.metadata().title.as_deref(), Some("Hi"));
}
