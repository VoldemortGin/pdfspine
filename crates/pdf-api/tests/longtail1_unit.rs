//! `LT1-*` — long-tail PyMuPDF parity batch 1 over the `pdf-api` facade:
//! `get_xobjects` / `get_image_rects` / `page_get_contents` / `page_read_contents`
//! / `page_show_pdf_page` and the Document navigation / chapter getters
//! (`reload_page`, `page_xref`, `resolve_link`, `fullcopy_page`,
//! `chapter_count`, `chapter_page_count`, `last_location`,
//! `get_page_xobjects`). Self-built classic-xref fixtures (the same tiny writer
//! used by `inventory_unit.rs`).

use pdf_api::{
    get_xobjects, page_get_contents, page_read_contents, page_show_pdf_page, Document, Rect,
};
use pdf_core::object::{Dict, Name, ObjRef, Object};
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
    Object::Stream(pdf_core::object::StreamObj::new_encoded(d, body.to_vec()))
}

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

/// A one-page doc whose page paints a Form XObject and an Image XObject and has
/// its own content stream. Objects: 1 catalog, 2 pages, 3 page, 4 content,
/// 5 form, 6 image.
fn xobject_doc() -> Vec<u8> {
    let page_content =
        b"q 80 0 0 60 20 20 cm /Fm0 Do Q\nq 50 0 0 50 100 100 cm /Im0 Do Q\n".to_vec();
    let form = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Form")),
            ("BBox", int_array(&[0, 0, 100, 100])),
        ],
        b"q Q",
    );
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
    let content = raw_stream(&[], &page_content);
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
        (
            "Resources",
            Object::Dictionary(dict(&[(
                "XObject",
                Object::Dictionary(dict(&[("Fm0", rref(5, 0)), ("Im0", rref(6, 0))])),
            )])),
        ),
    ]));
    build_pdf(
        &[
            (1, catalog),
            (2, pages),
            (3, page),
            (4, content),
            (5, form),
            (6, image),
        ],
        1,
        &[],
    )
}

/// An N-page doc, each page with its own content stream painting its marker.
fn multi_page_doc(markers: &[&str]) -> Vec<u8> {
    let mut objects: Vec<(u32, Object)> = Vec::new();
    let mut kids = Vec::new();
    for (i, marker) in markers.iter().enumerate() {
        let leaf = 4 + (i as u32) * 2;
        let content = leaf + 1;
        kids.push(rref(leaf, 0));
        let body = format!("BT /F1 12 Tf 20 100 Td ({marker}) Tj ET").into_bytes();
        let page = Object::Dictionary(dict(&[
            ("Type", name_obj("Page")),
            ("Parent", rref(2, 0)),
            ("MediaBox", int_array(&[0, 0, 200, 200])),
            ("Contents", rref(content, 0)),
            (
                "Resources",
                Object::Dictionary(dict(&[(
                    "Font",
                    Object::Dictionary(dict(&[("F1", rref(3, 0))])),
                )])),
            ),
        ]));
        objects.push((leaf, page));
        objects.push((content, raw_stream(&[], &body)));
    }
    let font = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
    ]));
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(markers.len() as i64)),
        ("Kids", Object::Array(kids)),
    ]));
    objects.push((1, catalog));
    objects.push((2, pages));
    objects.push((3, font));
    build_pdf(&objects, 1, &[])
}

/// A two-page doc with a catalog `/Dests` named destination `Chapter2` → page 2.
fn named_dest_doc() -> Vec<u8> {
    let catalog = Object::Dictionary(dict(&[
        ("Type", name_obj("Catalog")),
        ("Pages", rref(2, 0)),
        ("Dests", rref(5, 0)),
    ]));
    let pages = Object::Dictionary(dict(&[
        ("Type", name_obj("Pages")),
        ("Count", Object::Integer(2)),
        ("Kids", Object::Array(vec![rref(3, 0), rref(4, 0)])),
        ("MediaBox", int_array(&[0, 0, 200, 200])),
    ]));
    let p1 = Object::Dictionary(dict(&[("Type", name_obj("Page")), ("Parent", rref(2, 0))]));
    let p2 = Object::Dictionary(dict(&[("Type", name_obj("Page")), ("Parent", rref(2, 0))]));
    let dests = Object::Dictionary(dict(&[(
        "Chapter2",
        Object::Array(vec![
            rref(4, 0),
            name_obj("XYZ"),
            Object::Null,
            Object::Null,
        ]),
    )]));
    build_pdf(
        &[(1, catalog), (2, pages), (3, p1), (4, p2), (5, dests)],
        1,
        &[],
    )
}

// === LT1 get_xobjects =====================================================

#[test]
fn lt1_001_get_xobjects_form_and_image() {
    let doc = Document::open_bytes(xobject_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    let xobjs = get_xobjects(&page);
    assert_eq!(xobjs.len(), 2);
    let fm = xobjs.iter().find(|x| x.name == "Fm0").unwrap();
    assert_eq!(fm.kind, "Form");
    assert_eq!(fm.xref, 5);
    assert_eq!(fm.bbox, (0.0, 0.0, 100.0, 100.0));
    assert_eq!(fm.referencer, page.obj_ref().num as i32);
    let im = xobjs.iter().find(|x| x.name == "Im0").unwrap();
    assert_eq!(im.kind, "Image");
    // Image XObjects have no /BBox → unit square.
    assert_eq!(im.bbox, (0.0, 0.0, 1.0, 1.0));
}

#[test]
fn lt1_002_document_get_page_xobjects() {
    let doc = Document::open_bytes(xobject_doc()).unwrap();
    let names: Vec<String> = doc
        .get_page_xobjects(0)
        .into_iter()
        .map(|x| x.name)
        .collect();
    assert!(names.contains(&"Fm0".to_string()));
    assert!(names.contains(&"Im0".to_string()));
    // Out-of-range page → empty.
    assert!(doc.get_page_xobjects(9).is_empty());
}

// === LT1 contents =========================================================

#[test]
fn lt1_003_page_get_contents_xref() {
    let doc = Document::open_bytes(xobject_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    assert_eq!(page_get_contents(&page), vec![4]);
}

#[test]
fn lt1_004_page_read_contents_bytes() {
    let doc = Document::open_bytes(xobject_doc()).unwrap();
    let page = doc.load_page(0).unwrap();
    let raw = page_read_contents(&page);
    assert!(raw.windows(7).any(|w| w == b"/Fm0 Do"));
    assert!(raw.windows(7).any(|w| w == b"/Im0 Do"));
}

// === LT1 Document navigation / identity ===================================

#[test]
fn lt1_005_page_xref_and_reload() {
    let doc = Document::open_bytes(multi_page_doc(&["A", "B"])).unwrap();
    assert_eq!(doc.page_xref(0), 4);
    assert_eq!(doc.page_xref(1), 6);
    assert_eq!(doc.page_xref(9), 0); // out of range
    let reloaded = doc.reload_page(1).unwrap();
    assert_eq!(reloaded.number(), 1);
    assert!(doc.reload_page(9).is_err());
}

#[test]
fn lt1_006_resolve_link_named_and_fragment() {
    let doc = Document::open_bytes(named_dest_doc()).unwrap();
    assert_eq!(doc.resolve_link("Chapter2"), Some(1));
    assert_eq!(doc.resolve_link("Missing"), None);

    let doc2 = Document::open_bytes(multi_page_doc(&["A", "B", "C"])).unwrap();
    assert_eq!(doc2.resolve_link("x.pdf#page=2"), Some(1));
    assert_eq!(doc2.resolve_link("#3"), Some(2));
    assert_eq!(doc2.resolve_link("#page=99"), None);
}

#[test]
fn lt1_007_fullcopy_page_appends() {
    let doc = Document::open_bytes(multi_page_doc(&["AAA", "BBB"])).unwrap();
    assert_eq!(doc.page_count(), 2);
    let new_idx = doc.fullcopy_page(0).unwrap();
    assert_eq!(new_idx, 2);
    assert_eq!(doc.page_count(), 3);
    // The copy is independent and survives a roundtrip.
    let bytes = doc
        .save_to_bytes(&pdf_core::SaveOptions::default().with_garbage(1))
        .unwrap();
    let re = Document::open_bytes(bytes).unwrap();
    assert_eq!(re.page_count(), 3);
    let text = match pdf_api::get_text(&re.load_page(2).unwrap(), "text", None, None) {
        pdf_api::TextOutput::Text(s) => s,
        _ => String::new(),
    };
    assert!(text.contains("AAA"));
    assert!(doc.fullcopy_page(9).is_err());
}

#[test]
fn lt1_008_chapter_location_model() {
    let doc = Document::open_bytes(multi_page_doc(&["A", "B", "C"])).unwrap();
    assert_eq!(doc.chapter_count(), 1);
    assert_eq!(doc.chapter_page_count(0), 3);
    assert_eq!(doc.chapter_page_count(1), 0);
    assert_eq!(doc.last_location(), (0, 2));
}

// === LT1 show_pdf_page ====================================================

#[test]
fn lt1_009_show_pdf_page_places_form() {
    let dst = Document::open_bytes(multi_page_doc(&["DST"])).unwrap();
    let src = Document::open_bytes(multi_page_doc(&["SRC"])).unwrap();
    let dst_page = dst.load_page(0).unwrap();
    let name = page_show_pdf_page(&dst_page, Rect::new(10.0, 10.0, 110.0, 110.0), &src, 0).unwrap();
    assert!(name.starts_with("Fm"));
    // The destination page now references the form.
    let xobjs = get_xobjects(&dst_page);
    let placed = xobjs.iter().find(|x| x.name == name).unwrap();
    assert_eq!(placed.kind, "Form");
    // The placement Do is in the content.
    let raw = page_read_contents(&dst_page);
    let needle = format!("/{name} Do");
    assert!(raw.windows(needle.len()).any(|w| w == needle.as_bytes()));
}

#[test]
fn lt1_010_show_pdf_page_roundtrips() {
    let dst = Document::open_bytes(multi_page_doc(&["DST"])).unwrap();
    let src = Document::open_bytes(multi_page_doc(&["SRC"])).unwrap();
    let dst_page = dst.load_page(0).unwrap();
    page_show_pdf_page(&dst_page, Rect::new(10.0, 10.0, 110.0, 110.0), &src, 0).unwrap();
    let bytes = dst
        .save_to_bytes(&pdf_core::SaveOptions::default().with_garbage(1))
        .unwrap();
    let re = Document::open_bytes(bytes).unwrap();
    assert_eq!(re.page_count(), 1);
    let xobjs = get_xobjects(&re.load_page(0).unwrap());
    assert!(xobjs.iter().any(|x| x.kind == "Form"));
}

#[test]
fn lt1_011_show_pdf_page_out_of_range() {
    let dst = Document::open_bytes(multi_page_doc(&["DST"])).unwrap();
    let src = Document::open_bytes(multi_page_doc(&["SRC"])).unwrap();
    let dst_page = dst.load_page(0).unwrap();
    assert!(page_show_pdf_page(&dst_page, Rect::new(0.0, 0.0, 50.0, 50.0), &src, 9).is_err());
}
