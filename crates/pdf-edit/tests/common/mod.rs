//! Shared self-built-PDF fixture builders for the M3c page-ops / merge tests
//! (PRD §10: self-built fixtures only).
//!
//! The central builder is [`MultiPage`]: a document with N pages, each carrying
//! an identifiable single-word content stream (e.g. page 0 shows `"AAA"`, page 1
//! `"BBB"`), so a test can assert the page *order* after an edit by extracting
//! each page's text. Fonts use a WinAnsi Type1 with explicit `/Widths` so
//! `pdf_text::interpret_page` extracts the marker reliably.

#![allow(dead_code)] // each test file uses a subset of the helpers

use pdf_core::object::parse::Parser;
use pdf_core::serialize::{write_indirect, write_object};
use pdf_core::{Dict, DocumentStore, Limits, Name, ObjRef, Object, StreamObj};
use pdf_text::interpret_page;

/// Convenience: a `/Name` object.
pub fn name_obj(s: &str) -> Object {
    Object::Name(Name::new(s))
}

/// Convenience: an indirect reference object.
pub fn rref(num: u32) -> Object {
    Object::Reference(ObjRef::new(num, 0))
}

/// Builds a `Dict` from `(key, value)` pairs.
pub fn dict(pairs: impl IntoIterator<Item = (&'static str, Object)>) -> Dict {
    let mut d = Dict::new();
    for (k, v) in pairs {
        d.insert(Name::new(k), v);
    }
    d
}

/// A WinAnsi Type1 Helvetica with a flat 600-unit width table over the printable
/// ASCII range (codes 32..=126), so any ASCII marker text extracts.
fn ascii_font() -> Object {
    let widths: Vec<Object> = (32..=126).map(|_| Object::Integer(600)).collect();
    Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
        ("FirstChar", Object::Integer(32)),
        ("LastChar", Object::Integer(126)),
        ("Widths", Object::Array(widths)),
    ]))
}

/// The content-stream body that shows `marker` at a fixed origin.
fn marker_content(marker: &str) -> Vec<u8> {
    format!("BT /F1 12 Tf 1 0 0 1 72 700 Tm ({marker}) Tj ET").into_bytes()
}

/// A multi-page document builder. Each page gets the same shared font (object 3)
/// and its own content stream carrying an identifiable marker word.
///
/// Object layout (classic xref):
/// - 1: catalog
/// - 2: pages (flat `/Kids`, `/Count`)
/// - 3: shared font
/// - 4, 6, 8, …: page leaves
/// - 5, 7, 9, …: per-page content streams
pub struct MultiPage {
    markers: Vec<String>,
    /// When set, each page references the shared font (object 3); always true
    /// here (the shared font is the dedup oracle for `MERGE-DEDUP-*`).
    shared_font: bool,
}

impl MultiPage {
    /// A builder with the given per-page markers (one page per marker).
    pub fn new(markers: &[&str]) -> Self {
        MultiPage {
            markers: markers.iter().map(|s| s.to_string()).collect(),
            shared_font: true,
        }
    }

    /// Emits the complete PDF bytes.
    pub fn build(&self) -> Vec<u8> {
        let n = self.markers.len();
        let mut objects: Vec<(u32, Object)> = Vec::new();

        // 1: catalog, 2: pages, 3: shared font.
        objects.push((
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ));

        // Page leaves at 4,6,8,…; contents at 5,7,9,…
        let mut kids = Vec::with_capacity(n);
        let media = || {
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ])
        };
        let mut page_objs = Vec::new();
        for (i, marker) in self.markers.iter().enumerate() {
            let leaf_num = 4 + (i as u32) * 2;
            let content_num = leaf_num + 1;
            kids.push(rref(leaf_num));
            let page = Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media()),
                ("Contents", rref(content_num)),
                (
                    "Resources",
                    Object::Dictionary(dict([(
                        "Font",
                        Object::Dictionary(dict([("F1", rref(3))])),
                    )])),
                ),
            ]));
            let body = marker_content(marker);
            let content = Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(body.len() as i64))]),
                body,
            ));
            page_objs.push((leaf_num, page));
            page_objs.push((content_num, content));
        }

        objects.push((
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(kids)),
                ("Count", Object::Integer(n as i64)),
            ])),
        ));
        objects.push((3, ascii_font()));
        objects.extend(page_objs);
        objects.sort_by_key(|(num, _)| *num);

        assemble_classic(&objects, ObjRef::new(1, 0))
    }
}

/// Assembles a classic-xref PDF from `(num, obj)` pairs (object 0 free) with the
/// given `/Root`. Numbers need not be contiguous; gaps become free entries.
pub fn assemble_classic(objects: &[(u32, Object)], root: ObjRef) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = std::collections::HashMap::new();
    let mut max_num = 0u32;
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
    trailer.insert(Name::new("Root"), Object::Reference(root));
    out.extend_from_slice(b"trailer\n");
    out.extend_from_slice(&write_object(&Object::Dictionary(trailer)));
    out.extend_from_slice(b"\nstartxref\n");
    out.extend_from_slice(format!("{startxref}\n").as_bytes());
    out.extend_from_slice(b"%%EOF\n");
    out
}

/// Opens `bytes` as a document (Lenient, default limits).
pub fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("opens")
}

/// The plain text of the page at `index` (extracted via `pdf_text`). Whitespace
/// is trimmed so a marker like `"AAA"` compares cleanly.
pub fn page_text(doc: &DocumentStore, index: usize) -> String {
    let refs = pdf_core::pagetree::page_refs(doc);
    let leaf = refs[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let res = interpret_page(doc, &page);
    res.glyphs
        .iter()
        .map(|g| g.unicode.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// The ordered per-page marker text of the whole document.
pub fn all_page_text(doc: &DocumentStore) -> Vec<String> {
    let count = pdf_core::pagetree::page_count(doc);
    (0..count).map(|i| page_text(doc, i)).collect()
}

/// Saves `doc` (full save, garbage=1) and reopens the bytes, returning the new
/// document — the reparse oracle used throughout the M3c tests.
pub fn save_reopen(doc: &DocumentStore) -> DocumentStore {
    let bytes = doc
        .save_to_vec(&pdf_core::SaveOptions::default().with_garbage(1))
        .expect("save");
    open(&bytes)
}

/// The `/Count` value of the root `/Pages` node (independent of the page-tree
/// walk — asserts the writer emitted a consistent count).
pub fn pages_count_key(doc: &DocumentStore) -> i64 {
    let root = doc.root().unwrap();
    let catalog = doc.resolve(root).unwrap();
    let pages_ref = catalog
        .as_dict()
        .unwrap()
        .get(&Name::new("Pages"))
        .and_then(Object::as_reference)
        .unwrap();
    let pages = doc.resolve(pages_ref).unwrap();
    pages
        .as_dict()
        .unwrap()
        .get(&Name::new("Count"))
        .and_then(Object::as_i64)
        .unwrap()
}

/// Counts how many objects in the document resolve to a dict with `/Type` ==
/// `type_name` (used to assert "shared font copied once").
pub fn count_objects_of_type(doc: &DocumentStore, type_name: &str) -> usize {
    let mut count = 0;
    for num in doc.xref().object_numbers() {
        if num == 0 {
            continue;
        }
        if let Ok(obj) = doc.get_object(num, 0) {
            if let Some(d) = obj.as_dict() {
                if d.get(&Name::new("Type"))
                    .and_then(Object::as_name)
                    .is_some_and(|t| t.as_bytes() == type_name.as_bytes())
                {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Asserts every `/Reference` reachable from the page leaves resolves to a
/// non-null object (no dangling refs after a merge). Returns the number of refs
/// checked.
pub fn assert_no_dangling_refs(doc: &DocumentStore) -> usize {
    let refs = pdf_core::pagetree::page_refs(doc);
    let mut checked = 0;
    let mut stack: Vec<ObjRef> = refs;
    let mut seen = std::collections::HashSet::new();
    while let Some(r) = stack.pop() {
        if !seen.insert(r.num) {
            continue;
        }
        let obj = doc.resolve(r).expect("ref resolves");
        assert!(!obj.is_null(), "dangling reference: {r:?}");
        collect_child_refs(obj.as_ref(), &mut stack);
        checked += 1;
    }
    checked
}

fn collect_child_refs(obj: &Object, out: &mut Vec<ObjRef>) {
    match obj {
        Object::Reference(r) => out.push(*r),
        Object::Array(a) => a.iter().for_each(|v| collect_child_refs(v, out)),
        Object::Dictionary(d) => d.values().for_each(|v| collect_child_refs(v, out)),
        Object::Stream(s) => s.dict.values().for_each(|v| collect_child_refs(v, out)),
        _ => {}
    }
}

/// Parses a single indirect object from `bytes` (test convenience).
pub fn parse_one_indirect(bytes: &[u8]) -> (ObjRef, Object) {
    let mut p = Parser::new(bytes);
    p.parse_indirect_object().unwrap()
}

/// A document with a **nested, two-level** page tree and inheritable attributes
/// (`/MediaBox`, `/Rotate`) declared only on the intermediate nodes — so flatten
/// must materialize them onto the leaves.
///
/// Layout:
/// - 1: catalog
/// - 2: root /Pages (`/MediaBox [0 0 400 500]`, `/Count 3`, Kids [3, 6])
/// - 3: intermediate /Pages (`/Rotate 90`, Kids [4, 5], Count 2)
/// - 4: leaf "AAA"   (no MediaBox, no Rotate → inherits both)
/// - 5: leaf "BBB"   (no MediaBox, no Rotate → inherits both)
/// - 6: leaf "CCC"   (no Rotate → inherits root MediaBox, no rotate)
/// - 7: shared font
/// - 8,9,10: content streams for leaves 4,5,6
pub fn nested_doc() -> Vec<u8> {
    let media = |x1: i64, y1: i64| {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(x1),
            Object::Integer(y1),
        ])
    };
    let leaf = |content: u32| {
        Object::Dictionary(dict([
            ("Type", name_obj("Page")),
            ("Parent", rref(3)),
            ("Contents", rref(content)),
            (
                "Resources",
                Object::Dictionary(dict([(
                    "Font",
                    Object::Dictionary(dict([("F1", rref(7))])),
                )])),
            ),
        ]))
    };
    let content = |marker: &str| {
        let body = marker_content(marker);
        Object::Stream(StreamObj::new_encoded(
            dict([("Length", Object::Integer(body.len() as i64))]),
            body,
        ))
    };

    let leaf6 = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2)),
        ("Contents", rref(10)),
        (
            "Resources",
            Object::Dictionary(dict([(
                "Font",
                Object::Dictionary(dict([("F1", rref(7))])),
            )])),
        ),
    ]));

    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3), rref(6)])),
                ("Count", Object::Integer(3)),
                ("MediaBox", media(400, 500)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Parent", rref(2)),
                ("Kids", Object::Array(vec![rref(4), rref(5)])),
                ("Count", Object::Integer(2)),
                ("Rotate", Object::Integer(90)),
            ])),
        ),
        (4, leaf(8)),
        (5, leaf(9)),
        (6, leaf6),
        (7, ascii_font()),
        (8, content("AAA")),
        (9, content("BBB")),
        (10, content("CCC")),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// A two-page document where **both** pages reference the same font (object 3)
/// and the same XObject form (object 11). Used as the `insert_pdf` source for the
/// `MERGE-DEDUP-*` tests — the shared font/XObject must be copied exactly once.
pub fn shared_resource_doc() -> Vec<u8> {
    let media = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ])
    };
    // A trivial form XObject (empty content) shared by both pages.
    let xobject = Object::Stream(StreamObj::new_encoded(
        dict([
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Form")),
            ("BBox", media()),
            ("Length", Object::Integer(0)),
        ]),
        Vec::new(),
    ));
    let page = |content: u32| {
        Object::Dictionary(dict([
            ("Type", name_obj("Page")),
            ("Parent", rref(2)),
            ("MediaBox", media()),
            ("Contents", rref(content)),
            (
                "Resources",
                Object::Dictionary(dict([
                    ("Font", Object::Dictionary(dict([("F1", rref(3))]))),
                    ("XObject", Object::Dictionary(dict([("X1", rref(11))]))),
                ])),
            ),
        ]))
    };
    let content = |marker: &str| {
        let body = marker_content(marker);
        Object::Stream(StreamObj::new_encoded(
            dict([("Length", Object::Integer(body.len() as i64))]),
            body,
        ))
    };

    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(4), rref(6)])),
                ("Count", Object::Integer(2)),
            ])),
        ),
        (3, ascii_font()),
        (4, page(5)),
        (5, content("SRC1")),
        (6, page(7)),
        (7, content("SRC2")),
        (11, xobject),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// A one-page document with a **cyclic** object graph reachable from the page:
/// object 11 references object 12, and object 12 references object 11 back
/// (via `/Resources → /Cycle`). `insert_pdf` must copy this without looping.
pub fn cyclic_doc() -> Vec<u8> {
    let media = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ])
    };
    let body = marker_content("CYC");
    let content = Object::Stream(StreamObj::new_encoded(
        dict([("Length", Object::Integer(body.len() as i64))]),
        body,
    ));
    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2)),
        ("MediaBox", media()),
        ("Contents", rref(5)),
        (
            "Resources",
            Object::Dictionary(dict([
                ("Font", Object::Dictionary(dict([("F1", rref(3))]))),
                ("Cycle", rref(11)),
            ])),
        ),
    ]));
    // 11 → 12 → 11 (a 2-cycle of plain dicts).
    let a = Object::Dictionary(dict([("Tag", name_obj("A")), ("Next", rref(12))]));
    let b = Object::Dictionary(dict([("Tag", name_obj("B")), ("Back", rref(11))]));

    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(4)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (3, ascii_font()),
        (4, page),
        (5, content),
        (11, a),
        (12, b),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// Counts objects that resolve to a stream whose `/Subtype` == `subtype`.
pub fn count_streams_of_subtype(doc: &DocumentStore, subtype: &str) -> usize {
    let mut count = 0;
    for num in doc.xref().object_numbers() {
        if num == 0 {
            continue;
        }
        if let Ok(obj) = doc.get_object(num, 0) {
            if obj.as_stream().is_some() {
                if let Some(d) = obj.as_dict() {
                    if d.get(&Name::new("Subtype"))
                        .and_then(Object::as_name)
                        .is_some_and(|t| t.as_bytes() == subtype.as_bytes())
                    {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}
