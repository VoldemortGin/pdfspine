//! `FONTS-INV-*` / `IMAGES-INV-*` — the page-inventory facade (`get_fonts` /
//! `get_images`) over `/Resources` (PRD §7, PyMuPDF parity). Self-built
//! classic-xref fixtures (the same tiny writer used by `document_unit.rs`).

use pdf_api::{get_fonts, get_images, Document};
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

/// Catalog (obj 1) + Pages (obj 2) + a single Page (obj 3) whose `/Resources` is
/// the supplied dict. Extra objects (fonts/xobjects/descriptors) are appended.
fn one_page_with_resources(resources: Object, extra: &[(u32, Object)]) -> Vec<u8> {
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
        ("Resources", resources),
    ]));
    let mut objects = vec![(1, catalog), (2, pages), (3, page)];
    objects.extend(extra.iter().cloned());
    build_pdf(&objects, 1, &[])
}

// === FONTS-INV-* ==========================================================

#[test]
fn fonts_inv_001_one_font_one_tuple() {
    // FONTS-INV-001: one /Resources /Font entry → exactly one tuple.
    let font = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ]));
    let resources = Object::Dictionary(dict(&[(
        "Font",
        Object::Dictionary(dict(&[("F1", rref(4, 0))])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, font)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let fonts = get_fonts(&page);
    assert_eq!(fonts.len(), 1);
}

#[test]
fn fonts_inv_002_tuple_shape_and_values() {
    // FONTS-INV-002: (xref, ext, type, basefont, name, encoding, referencer).
    // Font obj 4 has a /FontDescriptor (obj 5) with /FontFile2 → "ttf".
    let descriptor = Object::Dictionary(dict(&[
        ("Type", name_obj("FontDescriptor")),
        ("FontName", name_obj("Arial")),
        ("FontFile2", rref(6, 0)),
    ]));
    let fontfile = raw_stream(&[("Length1", Object::Integer(4))], b"ttf!");
    let font = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("TrueType")),
        ("BaseFont", name_obj("Arial")),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FontDescriptor", rref(5, 0)),
    ]));
    let resources = Object::Dictionary(dict(&[(
        "Font",
        Object::Dictionary(dict(&[("F1", rref(4, 0))])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, font), (5, descriptor), (6, fontfile)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let fonts = get_fonts(&page);
    assert_eq!(fonts.len(), 1);
    let f = &fonts[0];
    assert_eq!(f.xref, 4);
    assert_eq!(f.ext, "ttf");
    assert_eq!(f.type_, "TrueType");
    assert_eq!(f.basefont, "Arial");
    assert_eq!(f.name, "F1");
    assert_eq!(f.encoding, "WinAnsiEncoding");
    // The referencer is the page object number (obj 3).
    assert_eq!(f.referencer, page.obj_ref().num as i32);
}

#[test]
fn fonts_inv_003_subset_tag_retained() {
    // FONTS-INV-003: a subset BaseFont (ABCDEF+Name) keeps the full name.
    let font = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("ABCDEF+Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ]));
    let resources = Object::Dictionary(dict(&[(
        "Font",
        Object::Dictionary(dict(&[("F1", rref(4, 0))])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, font)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let fonts = get_fonts(&page);
    assert_eq!(fonts[0].basefont, "ABCDEF+Helvetica");
    // No embedded font file → ext is "n/a".
    assert_eq!(fonts[0].ext, "n/a");
}

#[test]
fn fonts_inv_004_type0_descendant() {
    // FONTS-INV-004: a Type0 font reports "Type0" + Identity-H, with the
    // descendant CIDFont supplying the descriptor when the parent lacks one.
    let descriptor = Object::Dictionary(dict(&[
        ("Type", name_obj("FontDescriptor")),
        ("FontName", name_obj("MingLiU")),
        ("FontFile2", rref(7, 0)),
    ]));
    let cidfont = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("CIDFontType2")),
        ("BaseFont", name_obj("MingLiU")),
        ("FontDescriptor", rref(6, 0)),
    ]));
    let fontfile = raw_stream(&[("Length1", Object::Integer(4))], b"ttf!");
    let font = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type0")),
        ("BaseFont", name_obj("MingLiU")),
        ("Encoding", name_obj("Identity-H")),
        ("DescendantFonts", Object::Array(vec![rref(5, 0)])),
    ]));
    let resources = Object::Dictionary(dict(&[(
        "Font",
        Object::Dictionary(dict(&[("F0", rref(4, 0))])),
    )]));
    let bytes = one_page_with_resources(
        resources,
        &[(4, font), (5, cidfont), (6, descriptor), (7, fontfile)],
    );
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let fonts = get_fonts(&page);
    assert_eq!(fonts.len(), 1);
    let f = &fonts[0];
    assert_eq!(f.type_, "Type0");
    assert_eq!(f.encoding, "Identity-H");
    assert_eq!(f.basefont, "MingLiU");
    // Descriptor came from the descendant → /FontFile2 → "ttf".
    assert_eq!(f.ext, "ttf");
    assert_eq!(f.name, "F0");
}

#[test]
fn fonts_inv_005_no_fonts_empty() {
    // FONTS-INV-005: a page with no /Font resource → empty Vec.
    let resources = Object::Dictionary(dict(&[]));
    let bytes = one_page_with_resources(resources, &[]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();
    assert!(get_fonts(&page).is_empty());
}

#[test]
fn fonts_inv_006_two_fonts_deduped_by_xref() {
    // FONTS-INV-006: two distinct fonts → two tuples; the same font referenced
    // under two names is deduped to one tuple (keyed by xref).
    let font_a = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ]));
    let font_b = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Times-Roman")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ]));
    // F1, F2 → distinct fonts; F3 → same object as F1 (dedup target).
    let resources = Object::Dictionary(dict(&[(
        "Font",
        Object::Dictionary(dict(&[
            ("F1", rref(4, 0)),
            ("F2", rref(5, 0)),
            ("F3", rref(4, 0)),
        ])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, font_a), (5, font_b)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let fonts = get_fonts(&page);
    assert_eq!(fonts.len(), 2, "F1/F3 share xref 4 → deduped");
    let xrefs: Vec<i32> = fonts.iter().map(|f| f.xref).collect();
    assert!(xrefs.contains(&4));
    assert!(xrefs.contains(&5));
}

// === IMAGES-INV-* =========================================================

#[test]
fn images_inv_001_one_image_one_tuple() {
    // IMAGES-INV-001: one image XObject → exactly one tuple.
    let image = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(8)),
            ("Height", Object::Integer(8)),
            ("BitsPerComponent", Object::Integer(8)),
            ("ColorSpace", name_obj("DeviceRGB")),
        ],
        b"imgbytes",
    );
    let resources = Object::Dictionary(dict(&[(
        "XObject",
        Object::Dictionary(dict(&[("Im0", rref(4, 0))])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, image)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let images = get_images(&page);
    assert_eq!(images.len(), 1);
}

#[test]
fn images_inv_002_tuple_shape_and_values() {
    // IMAGES-INV-002: (xref, smask, w, h, bpc, cs, alt_cs, name, filter, ref).
    let image = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(16)),
            ("Height", Object::Integer(32)),
            ("BitsPerComponent", Object::Integer(8)),
            ("ColorSpace", name_obj("DeviceGray")),
            ("Filter", name_obj("FlateDecode")),
        ],
        b"imgbytes",
    );
    let resources = Object::Dictionary(dict(&[(
        "XObject",
        Object::Dictionary(dict(&[("Im0", rref(4, 0))])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, image)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let images = get_images(&page);
    assert_eq!(images.len(), 1);
    let im = &images[0];
    assert_eq!(im.xref, 4);
    assert_eq!(im.smask, 0);
    assert_eq!(im.width, 16);
    assert_eq!(im.height, 32);
    assert_eq!(im.bpc, 8);
    assert_eq!(im.colorspace, "DeviceGray");
    assert_eq!(im.alt_colorspace, "");
    assert_eq!(im.name, "Im0");
    assert_eq!(im.filter, "FlateDecode");
    assert_eq!(im.referencer, page.obj_ref().num as i32);
}

#[test]
fn images_inv_003_form_xobject_excluded() {
    // IMAGES-INV-003: a Form XObject is not an image and must be skipped.
    let form = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Form")),
            ("BBox", int_array(&[0, 0, 10, 10])),
        ],
        b"q Q",
    );
    let image = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(4)),
            ("Height", Object::Integer(4)),
            ("BitsPerComponent", Object::Integer(8)),
            ("ColorSpace", name_obj("DeviceRGB")),
        ],
        b"img!",
    );
    let resources = Object::Dictionary(dict(&[(
        "XObject",
        Object::Dictionary(dict(&[("Fm0", rref(4, 0)), ("Im0", rref(5, 0))])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, form), (5, image)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let images = get_images(&page);
    assert_eq!(images.len(), 1, "only the image XObject is reported");
    assert_eq!(images[0].xref, 5);
}

#[test]
fn images_inv_004_no_images_empty() {
    // IMAGES-INV-004: no /XObject → empty Vec.
    let resources = Object::Dictionary(dict(&[]));
    let bytes = one_page_with_resources(resources, &[]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();
    assert!(get_images(&page).is_empty());
}

#[test]
fn images_inv_005_smask_xref_reported() {
    // IMAGES-INV-005: an image with /SMask reports the soft-mask xref.
    let smask = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(4)),
            ("Height", Object::Integer(4)),
            ("BitsPerComponent", Object::Integer(8)),
            ("ColorSpace", name_obj("DeviceGray")),
        ],
        b"mask",
    );
    let image = raw_stream(
        &[
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(4)),
            ("Height", Object::Integer(4)),
            ("BitsPerComponent", Object::Integer(8)),
            ("ColorSpace", name_obj("DeviceRGB")),
            ("SMask", rref(5, 0)),
        ],
        b"img!",
    );
    let resources = Object::Dictionary(dict(&[(
        "XObject",
        Object::Dictionary(dict(&[("Im0", rref(4, 0))])),
    )]));
    let bytes = one_page_with_resources(resources, &[(4, image), (5, smask)]);
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();

    let images = get_images(&page);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].xref, 4);
    assert_eq!(images[0].smask, 5);
}
