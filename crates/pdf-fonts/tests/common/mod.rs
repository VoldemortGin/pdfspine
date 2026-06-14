//! Self-built-PDF fixture builders for the M2a font-mapping tests.
//!
//! A focused subset of the M1c helpers: a classic-xref [`Pdf`] builder plus
//! `dict`/`name_obj`/`rref`/`flate_stream` so each test can assemble a tiny
//! document containing self-constructed font objects (we control every byte).
//! No external / PyMuPDF files (PRD §10).

#![allow(dead_code)] // each test file uses a subset of the helpers

use std::collections::HashMap;

use pdf_core::filters::flate;
use pdf_core::serialize::write_indirect;
use pdf_core::{Dict, DocumentStore, Limits, Name, ObjRef, Object, StreamObj};

/// Builds a `%PDF-<v>\n` header followed by a binary-marker comment line.
fn header_bytes(v: &str) -> Vec<u8> {
    let mut h = format!("%PDF-{v}\n%").into_bytes();
    h.extend_from_slice(&[0xE2, 0xE3, 0xCF, 0xD3, b'\n']);
    h
}

/// A `/Name` object.
pub fn name_obj(s: &str) -> Object {
    Object::Name(Name::new(s))
}

/// An indirect reference object.
pub fn rref(num: u32, gen: u16) -> Object {
    Object::Reference(ObjRef::new(num, gen))
}

/// Builds a `Dict` from `(key, value)` pairs.
pub fn dict(pairs: impl IntoIterator<Item = (&'static str, Object)>) -> Dict {
    let mut d = Dict::new();
    for (k, v) in pairs {
        d.insert(Name::new(k), v);
    }
    d
}

/// A Flate-compressed stream object (dict gets `/Filter /FlateDecode` and a
/// correct `/Length`). Used for `/ToUnicode`, embedded CMap and CIDToGIDMap
/// stream fixtures.
pub fn flate_stream(
    extra: impl IntoIterator<Item = (&'static str, Object)>,
    body: &[u8],
) -> Object {
    let enc = flate::encode(body);
    let mut d = dict(extra);
    d.insert(Name::new("Filter"), name_obj("FlateDecode"));
    d.insert(Name::new("Length"), Object::Integer(enc.len() as i64));
    Object::Stream(StreamObj::new_encoded(d, enc))
}

/// An uncompressed stream object carrying `body` verbatim.
pub fn raw_stream(extra: impl IntoIterator<Item = (&'static str, Object)>, body: &[u8]) -> Object {
    let mut d = dict(extra);
    d.insert(Name::new("Length"), Object::Integer(body.len() as i64));
    Object::Stream(StreamObj::new_encoded(d, body.to_vec()))
}

/// A classic-xref PDF builder (see the M1c `common` for the full version).
pub struct Pdf {
    header: Vec<u8>,
    objects: Vec<(u32, u16, Vec<u8>)>,
    root: Option<ObjRef>,
}

impl Pdf {
    pub fn new() -> Self {
        Pdf {
            header: header_bytes("1.7"),
            objects: Vec::new(),
            root: None,
        }
    }

    /// Records an indirect object `num gen obj … endobj`.
    pub fn obj(mut self, num: u32, gen: u16, obj: Object) -> Self {
        let bytes = write_indirect(ObjRef::new(num, gen), &obj);
        self.objects.push((num, gen, bytes));
        self
    }

    /// Sets the trailer `/Root` catalog reference.
    pub fn root(mut self, num: u32, gen: u16) -> Self {
        self.root = Some(ObjRef::new(num, gen));
        self
    }

    /// Emits the complete PDF bytes with a classic xref table.
    pub fn build(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.header);

        let mut max_num = 0u32;
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, _gen, bytes) in &self.objects {
            offsets.push((*num, out.len()));
            out.extend_from_slice(bytes);
            max_num = max_num.max(*num);
        }

        let startxref = out.len();
        let size = max_num + 1;

        out.extend_from_slice(b"xref\n");
        out.extend_from_slice(format!("0 {size}\n").as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        let mut map = HashMap::new();
        for (num, off) in &offsets {
            map.insert(*num, *off);
        }
        for num in 1..size {
            if let Some(off) = map.get(&num) {
                out.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
            } else {
                out.extend_from_slice(b"0000000000 65535 f \n");
            }
        }

        let mut trailer = Dict::new();
        trailer.insert(Name::new("Size"), Object::Integer(i64::from(size)));
        if let Some(r) = self.root {
            trailer.insert(Name::new("Root"), Object::Reference(r));
        }
        out.extend_from_slice(b"trailer\n");
        out.extend_from_slice(&pdf_core::serialize::write_object(&Object::Dictionary(
            trailer,
        )));
        out.extend_from_slice(b"\nstartxref\n");
        out.extend_from_slice(format!("{startxref}\n").as_bytes());
        out.extend_from_slice(b"%%EOF\n");
        out
    }
}

impl Default for Pdf {
    fn default() -> Self {
        Pdf::new()
    }
}

/// A minimal catalog (obj 1) + empty pages (obj 2). Tests append their font
/// objects starting at `start_num` and reference them directly.
pub struct FontDoc {
    pdf_objects: Vec<(u32, u16, Object)>,
    next: u32,
}

impl FontDoc {
    /// A new doc with catalog (1) + pages (2); the first free object number is 3.
    pub fn new() -> Self {
        FontDoc {
            pdf_objects: vec![
                (
                    1,
                    0,
                    Object::Dictionary(dict([
                        ("Type", name_obj("Catalog")),
                        ("Pages", rref(2, 0)),
                    ])),
                ),
                (
                    2,
                    0,
                    Object::Dictionary(dict([
                        ("Type", name_obj("Pages")),
                        ("Count", Object::Integer(0)),
                        ("Kids", Object::Array(vec![])),
                    ])),
                ),
            ],
            next: 3,
        }
    }

    /// Adds an object, returning its assigned number (a reference to it).
    pub fn add(&mut self, obj: Object) -> u32 {
        let num = self.next;
        self.next += 1;
        self.pdf_objects.push((num, 0, obj));
        num
    }

    /// Builds the document and returns `(DocumentStore, ())`. Use [`Self::open`].
    pub fn open(self) -> DocumentStore {
        let mut pdf = Pdf::new().root(1, 0);
        for (num, gen, obj) in self.pdf_objects {
            pdf = pdf.obj(num, gen, obj);
        }
        let bytes = pdf.build();
        DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open font doc")
    }
}

impl Default for FontDoc {
    fn default() -> Self {
        FontDoc::new()
    }
}
