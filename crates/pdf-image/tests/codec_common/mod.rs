//! Shared helpers for the M5 codec integration tests.
//!
//! The per-codec `decode` functions take a `&DocumentStore` only to resolve
//! indirect references inside the params dict. The codec tests build their
//! params with **inline** values, so the store is never actually queried — we
//! just need a valid one to satisfy the signature. A minimal in-memory catalog
//! PDF is assembled here with pdf-core's own object model + a tiny hand-written
//! classic xref (no external/PyMuPDF fixtures, per PRD §10).

#![allow(dead_code)] // each test file uses a subset of the helpers

use std::collections::BTreeMap;

use pdf_core::{Dict, DocumentStore, Limits, Name, Object};

/// Builds a `Dict` from `(key, value)` pairs.
pub fn dict(pairs: impl IntoIterator<Item = (&'static str, Object)>) -> Dict {
    let mut d: Dict = BTreeMap::new();
    for (k, v) in pairs {
        d.insert(Name::new(k), v);
    }
    d
}

/// A `/Name` object.
pub fn name_obj(s: &str) -> Object {
    Object::Name(Name::new(s))
}

/// An integer object.
pub fn int(v: i64) -> Object {
    Object::Integer(v)
}

/// A bool object.
pub fn boolean(v: bool) -> Object {
    Object::Boolean(v)
}

/// An array object.
pub fn array(items: impl IntoIterator<Item = Object>) -> Object {
    Object::Array(items.into_iter().collect())
}

/// A minimal, valid `DocumentStore` (catalog + empty page tree) for use as the
/// `doc` argument when the params carry no indirect references.
pub fn empty_doc() -> DocumentStore {
    DocumentStore::from_bytes(minimal_pdf(), Limits::unbounded_decode()).expect("open minimal pdf")
}

fn minimal_pdf() -> Vec<u8> {
    // Hand-written classic-xref PDF: catalog (obj 1) + pages (obj 2).
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");

    let mut offsets: Vec<usize> = vec![0; 3];

    offsets[1] = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets[2] = out.len();
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n");

    let startxref = out.len();
    out.extend_from_slice(b"xref\n0 3\n");
    out.extend_from_slice(b"0000000000 65535 f \n");
    out.extend_from_slice(format!("{:010} 00000 n \n", offsets[1]).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", offsets[2]).as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 3 /Root 1 0 R >>\n");
    out.extend_from_slice(format!("startxref\n{startxref}\n%%EOF\n").as_bytes());
    out
}
