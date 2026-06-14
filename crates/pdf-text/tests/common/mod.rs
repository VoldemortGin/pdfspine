//! Self-constructed PDF / font / content fixtures for the M2b interpreter tests.
//!
//! We control every byte: a tiny classic-xref [`Pdf`] builder, font-dict helpers
//! (a WinAnsi Type1 with explicit `/Widths`, an Identity-H Type0), and a
//! `DocStore` convenience that opens a doc with a catalog + a single page so the
//! interpreter has real `/Contents` + `/Resources` to walk. No external /
//! PyMuPDF files (PRD §10).

#![allow(dead_code)] // each test file uses a subset of the helpers

use std::collections::HashMap;

use pdf_core::filters::flate;
use pdf_core::geom::Matrix;
use pdf_core::serialize::write_indirect;
use pdf_core::{Dict, DocumentStore, Limits, Name, ObjRef, Object, StreamObj};
use pdf_text::{ContentInterpreter, InterpretResult, PositionedGlyph};

/// `%PDF-<v>` header + a binary-marker comment line.
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

/// An array of integers.
pub fn int_array(vals: impl IntoIterator<Item = i64>) -> Object {
    Object::Array(vals.into_iter().map(Object::Integer).collect())
}

/// A Flate-compressed stream object (gets `/Filter /FlateDecode` + `/Length`).
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

/// A classic-xref PDF builder.
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

    pub fn obj(mut self, num: u32, gen: u16, obj: Object) -> Self {
        let bytes = write_indirect(ObjRef::new(num, gen), &obj);
        self.objects.push((num, gen, bytes));
        self
    }

    pub fn root(mut self, num: u32, gen: u16) -> Self {
        self.root = Some(ObjRef::new(num, gen));
        self
    }

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

// === font-dict fixtures ===================================================

/// A simple WinAnsi Type1 font dict with an explicit `/Widths` table.
///
/// `first_char` is the code of the first `/Widths` entry; `widths` are advances
/// in 1000-unit glyph space. No `/FontDescriptor`, so vertical metrics fall back
/// to the interpreter's Latin-text defaults (ascent 800, descent -200).
pub fn winansi_type1(base: &str, first_char: i64, widths: &[i64]) -> Object {
    Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj(base)),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FirstChar", Object::Integer(first_char)),
        (
            "LastChar",
            Object::Integer(first_char + widths.len() as i64 - 1),
        ),
        (
            "Widths",
            Object::Array(widths.iter().copied().map(Object::Integer).collect()),
        ),
    ]))
}

/// A simple WinAnsi Type1 font dict that also carries an inline
/// `/FontDescriptor` with explicit `/Ascent`/`/Descent` (so bbox-height tests
/// are deterministic).
pub fn winansi_type1_with_metrics(
    base: &str,
    first_char: i64,
    widths: &[i64],
    ascent: i64,
    descent: i64,
) -> Object {
    let descriptor = Object::Dictionary(dict([
        ("Type", name_obj("FontDescriptor")),
        ("FontName", name_obj(base)),
        ("Ascent", Object::Integer(ascent)),
        ("Descent", Object::Integer(descent)),
        ("Flags", Object::Integer(32)),
    ]));
    Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj(base)),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FirstChar", Object::Integer(first_char)),
        (
            "LastChar",
            Object::Integer(first_char + widths.len() as i64 - 1),
        ),
        (
            "Widths",
            Object::Array(widths.iter().copied().map(Object::Integer).collect()),
        ),
        ("FontDescriptor", descriptor),
    ]))
}

/// A 1-page document with a content stream + a `/Font` resource dict.
///
/// `fonts` maps a resource name (`F1`) to a font-dict object placed inline in
/// `/Resources /Font`. Returns `(DocumentStore, page_dict)`.
pub struct PageDoc {
    objects: Vec<(u32, u16, Object)>,
    next: u32,
    fonts: Dict,
    xobjects: Dict,
    content_obj: Option<u32>,
    content_array: Option<Vec<u32>>,
}

impl PageDoc {
    pub fn new() -> Self {
        PageDoc {
            objects: Vec::new(),
            next: 10,
            fonts: Dict::new(),
            xobjects: Dict::new(),
            content_obj: None,
            content_array: None,
        }
    }

    /// The object number that the next [`Self::add`] call will assign.
    pub fn peek_next(&self) -> u32 {
        self.next
    }

    /// Adds an indirect object, returning its number.
    pub fn add(&mut self, obj: Object) -> u32 {
        let num = self.next;
        self.next += 1;
        self.objects.push((num, 0, obj));
        num
    }

    /// Registers a font under resource `name` (inline in `/Resources /Font`).
    pub fn font(mut self, name: &str, font: Object) -> Self {
        self.fonts.insert(Name::new(name), font);
        self
    }

    /// Registers a font under resource `name` via an indirect reference.
    pub fn font_ref(mut self, name: &str, num: u32) -> Self {
        self.fonts.insert(Name::new(name), rref(num, 0));
        self
    }

    /// Registers an XObject under resource `name` (by reference number).
    pub fn xobject_ref(mut self, name: &str, num: u32) -> Self {
        self.xobjects.insert(Name::new(name), rref(num, 0));
        self
    }

    /// Sets the page content stream (single, uncompressed).
    pub fn content(mut self, body: &[u8]) -> Self {
        let num = self.add(raw_stream([], body));
        self.content_obj = Some(num);
        self
    }

    /// Sets the page content as an array of (uncompressed) streams.
    pub fn content_streams(mut self, bodies: &[&[u8]]) -> Self {
        let nums: Vec<u32> = bodies.iter().map(|b| self.add(raw_stream([], b))).collect();
        self.content_array = Some(nums);
        self
    }

    /// Builds the document and returns `(DocumentStore, page_dict)`.
    pub fn open(mut self) -> (DocumentStore, Dict) {
        // Resources dict.
        let mut resources = Dict::new();
        if !self.fonts.is_empty() {
            resources.insert(Name::new("Font"), Object::Dictionary(self.fonts.clone()));
        }
        if !self.xobjects.is_empty() {
            resources.insert(
                Name::new("XObject"),
                Object::Dictionary(self.xobjects.clone()),
            );
        }

        // Contents reference.
        let contents = if let Some(num) = self.content_obj {
            rref(num, 0)
        } else if let Some(nums) = &self.content_array {
            Object::Array(nums.iter().map(|n| rref(*n, 0)).collect())
        } else {
            Object::Array(vec![])
        };

        // Page (obj 3), Pages (obj 2), Catalog (obj 1).
        let page = dict([
            ("Type", name_obj("Page")),
            ("Parent", rref(2, 0)),
            (
                "MediaBox",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(612),
                    Object::Integer(792),
                ]),
            ),
            ("Resources", Object::Dictionary(resources)),
            ("Contents", contents),
        ]);

        let mut pdf = Pdf::new().root(1, 0);
        pdf = pdf.obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
        );
        pdf = pdf.obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![rref(3, 0)])),
            ])),
        );
        pdf = pdf.obj(3, 0, Object::Dictionary(page.clone()));
        for (num, gen, obj) in self.objects.drain(..) {
            pdf = pdf.obj(num, gen, obj);
        }
        let bytes = pdf.build();
        let doc =
            DocumentStore::from_bytes(bytes, Limits::unbounded_decode()).expect("open page doc");
        (doc, page)
    }
}

impl Default for PageDoc {
    fn default() -> Self {
        PageDoc::new()
    }
}

// === interpreter convenience ==============================================

/// Runs an explicit content buffer against a single inline font (resource `F1`)
/// with an identity base CTM. The simplest interpreter entry for unit tests.
pub fn run_with_font(font: Object, content: &[u8]) -> InterpretResult {
    let (doc, page) = PageDoc::new().font("F1", font).content(content).open();
    ContentInterpreter::new(&doc).run_page(&page)
}

/// Runs an explicit content buffer against a resource dict + base CTM (no page).
pub fn run_content(content: &[u8], resources: Dict, base: Matrix) -> InterpretResult {
    // A throwaway doc is needed for the store; resources resolve direct values.
    let (doc, _page) = PageDoc::new().content(b"").open();
    ContentInterpreter::new(&doc).run_content(content, &resources, base)
}

/// The Unicode strings of all emitted glyphs joined into one string.
pub fn glyph_text(res: &InterpretResult) -> String {
    res.glyphs.iter().map(|g| g.unicode.as_str()).collect()
}

/// Assert two `f64`s are within `eps`.
#[track_caller]
pub fn approx(a: f64, b: f64, eps: f64) {
    assert!(
        (a - b).abs() <= eps,
        "expected {a} ≈ {b} (within {eps}), diff = {}",
        (a - b).abs()
    );
}

/// Assert a glyph's origin is near `(x, y)`.
#[track_caller]
pub fn assert_origin(g: &PositionedGlyph, x: f64, y: f64, eps: f64) {
    approx(g.origin.x, x, eps);
    approx(g.origin.y, y, eps);
}
