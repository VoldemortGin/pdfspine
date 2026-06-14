//! `TEXTPAGE-REUSE-*` — a pre-built [`pdf_text::TextPage`] can be shared across
//! `get_text` + `search` so the model is built once (PRD §9.4). Self-built
//! classic-xref fixture with a WinAnsi Type1 font + a `BT … Tj … ET` content
//! stream so real glyphs are produced.

use pdf_api::{get_text, textpage, Document, TextOutput};
use pdf_core::object::{Dict, Name, ObjRef, Object};
use pdf_core::serialize::{write_indirect, write_object};

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

fn build_pdf(objects: &[(u32, Object)], root: u32) -> Vec<u8> {
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

    let mut trailer = Dict::new();
    trailer.insert(Name::new("Size"), Object::Integer(i64::from(size)));
    trailer.insert(Name::new("Root"), rref(root, 0));
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(&write_object(&Object::Dictionary(trailer)));
    out.extend_from_slice(b"\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
    out
}

/// A one-page document whose content draws the ASCII text "Hello" with a WinAnsi
/// Helvetica (explicit /Widths so glyph advances are deterministic).
fn text_doc() -> Vec<u8> {
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
    // WinAnsi Type1 with widths for 'H','e','l','o' (codes 72..=111 covered).
    let widths: Vec<i64> = (0..40).map(|_| 500).collect();
    let font = Object::Dictionary(dict(&[
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FirstChar", Object::Integer(72)),
        ("LastChar", Object::Integer(111)),
        (
            "Widths",
            Object::Array(widths.into_iter().map(Object::Integer).collect()),
        ),
    ]));
    let resources = Object::Dictionary(dict(&[(
        "Font",
        Object::Dictionary(dict(&[("F1", rref(5, 0))])),
    )]));
    let page = Object::Dictionary(dict(&[
        ("Type", name_obj("Page")),
        ("Parent", rref(2, 0)),
        ("MediaBox", int_array(&[0, 0, 200, 200])),
        ("Resources", resources),
        ("Contents", rref(4, 0)),
    ]));
    let content = b"BT /F1 12 Tf 20 100 Td (Hello) Tj ET";
    let stream = raw_stream(&[], content);
    build_pdf(
        &[(1, catalog), (2, pages), (3, page), (4, stream), (5, font)],
        1,
    )
}

fn output_text(out: &TextOutput) -> String {
    match out {
        TextOutput::Text(s) => s.clone(),
        other => panic!("expected TextOutput::Text, got {other:?}"),
    }
}

#[test]
fn textpage_reuse_001_build_once_used_by_get_text_and_search() {
    // TEXTPAGE-REUSE-001: build a TextPage once; get_text + search both run off
    // it (no rebuild needed).
    let doc = Document::open_bytes(text_doc()).unwrap();
    let page = doc.load_page(0).unwrap();

    let tp = textpage(&page, 0, None);
    let txt = output_text(&get_text(&page, "text", None, Some(&tp)));
    assert!(
        txt.contains("Hello"),
        "reused TextPage extracts text: {txt:?}"
    );

    let opts = pdf_api::SearchOptions {
        hit_max: 16,
        clip: None,
        quads: false,
    };
    let hits = pdf_api::search(&page, "Hello", opts, Some(&tp));
    assert_eq!(hits.len(), 1, "reused TextPage finds the needle");
}

#[test]
fn textpage_reuse_002_reused_text_equals_fresh() {
    // TEXTPAGE-REUSE-002: text from a reused TextPage == fresh-build text.
    let doc = Document::open_bytes(text_doc()).unwrap();
    let page = doc.load_page(0).unwrap();

    let tp = textpage(&page, 0, None);
    let reused = output_text(&get_text(&page, "text", None, Some(&tp)));
    let fresh = output_text(&get_text(&page, "text", None, None));
    assert_eq!(reused, fresh);
    assert!(reused.contains("Hello"));
}

#[test]
fn textpage_reuse_003_search_reused_equals_fresh() {
    // TEXTPAGE-REUSE-003: search over a reused TextPage == a fresh search.
    let doc = Document::open_bytes(text_doc()).unwrap();
    let page = doc.load_page(0).unwrap();

    let tp = textpage(&page, 0, None);
    let opts = || pdf_api::SearchOptions {
        hit_max: 16,
        clip: None,
        quads: false,
    };
    let reused = pdf_api::search(&page, "Hello", opts(), Some(&tp));
    let fresh = pdf_api::search(&page, "Hello", opts(), None);
    assert_eq!(reused.len(), fresh.len());
    assert_eq!(reused.len(), 1);
}
