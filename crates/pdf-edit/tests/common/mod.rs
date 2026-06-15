//! Shared self-built-PDF fixture builders for the M3c page-ops / merge tests
//! (PRD §10: self-built fixtures only).
//!
//! The central builder is [`MultiPage`]: a document with N pages, each carrying
//! an identifiable single-word content stream (e.g. page 0 shows `"AAA"`, page 1
//! `"BBB"`), so a test can assert the page *order* after an edit by extracting
//! each page's text. Fonts use a WinAnsi Type1 with explicit `/Widths` so
//! `pdf_text::interpret_page` extracts the marker reliably.

#![allow(dead_code)] // each test file uses a subset of the helpers

pub mod testfont;

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
pub fn ascii_font() -> Object {
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

/// Saves `doc` (full save, garbage=1) and returns the raw bytes (for byte-level
/// / decompressed-corpus assertions in the M4a content-insertion tests).
pub fn save_bytes(doc: &DocumentStore) -> Vec<u8> {
    doc.save_to_vec(&pdf_core::SaveOptions::default().with_garbage(1))
        .expect("save")
}

// === M4a content-insertion helpers (PRD §8.8) =============================

/// A single-page blank document: catalog (1) → pages (2) → one leaf (3) with an
/// empty `/Contents` stream (4). The page is `width × height` (default Letter).
/// Used as the canvas for `insert_text` / `insert_image` / `draw_*` tests.
pub fn blank_page(width: i64, height: i64) -> Vec<u8> {
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                (
                    "MediaBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Integer(width),
                        Object::Integer(height),
                    ]),
                ),
                ("Contents", rref(4)),
                ("Resources", Object::Dictionary(Dict::new())),
            ])),
        ),
        (
            4,
            Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(0))]),
                Vec::new(),
            )),
        ),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// The positioned glyphs (PDF user space) of the page at `index`, via the M2
/// interpreter. The strongest M4a oracle: inserted text shows up here.
pub fn page_glyphs(doc: &DocumentStore, index: usize) -> Vec<pdf_text::PositionedGlyph> {
    let leaf = pdf_core::pagetree::page_refs(doc)[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    interpret_page(doc, &page).glyphs
}

/// The image inventory (with CTM) of the page at `index`, via the M2
/// interpreter — used to assert `insert_image` placement on reopen.
pub fn page_images(doc: &DocumentStore, index: usize) -> Vec<pdf_text::ImageRef> {
    let leaf = pdf_core::pagetree::page_refs(doc)[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    interpret_page(doc, &page).images
}

/// The concatenated, decoded `/Contents` bytes of the page at `index` (so a test
/// can grep for raw operators like `re`, `c`, `Do`).
pub fn page_content_bytes(doc: &DocumentStore, index: usize) -> Vec<u8> {
    let leaf = pdf_core::pagetree::page_refs(doc)[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let contents = doc
        .resolve_dict_key(&page, &Name::new("Contents"))
        .ok()
        .flatten();
    let mut out = Vec::new();
    let push = |s: &StreamObj, out: &mut Vec<u8>| {
        if let Ok(b) = doc.decode_stream(s).and_then(|o| o.into_decoded()) {
            out.extend_from_slice(&b);
            out.push(b'\n');
        }
    };
    match contents.as_deref() {
        Some(Object::Stream(s)) => push(s, &mut out),
        Some(Object::Array(arr)) => {
            for item in arr {
                if let Some(r) = item.as_reference() {
                    if let Ok(o) = doc.resolve(r) {
                        if let Some(s) = o.as_stream() {
                            push(s, &mut out);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// The font dictionaries registered under `/Resources /Font` of the page at
/// `index` (resolved through the overlay), the robust reopen-safe way to assert
/// what `insert_text` registered (objects may be packed into ObjStms on save).
pub fn page_fonts(doc: &DocumentStore, index: usize) -> Vec<Dict> {
    let leaf = pdf_core::pagetree::page_refs(doc)[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let resources = doc
        .resolve_dict_key(&page, &Name::new("Resources"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .unwrap_or_default();
    let fonts = match resources.get(&Name::new("Font")) {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_dict().cloned())
            .unwrap_or_default(),
        _ => return Vec::new(),
    };
    fonts
        .values()
        .filter_map(|v| match v {
            Object::Dictionary(d) => Some(d.clone()),
            Object::Reference(r) => doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned()),
            _ => None,
        })
        .collect()
}

/// Resolves the first image XObject dictionary registered under
/// `/Resources /XObject` of the page at `index` (for `/Filter` / `/ColorSpace`
/// assertions). Returns the XObject stream dict.
pub fn first_xobject_dict(doc: &DocumentStore, index: usize) -> Dict {
    let leaf = pdf_core::pagetree::page_refs(doc)[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let resources = doc
        .resolve_dict_key(&page, &Name::new("Resources"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .expect("resources");
    let xobjects = match resources.get(&Name::new("XObject")) {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(r)) => doc.resolve(*r).unwrap().as_dict().cloned().unwrap(),
        _ => panic!("no /XObject dict"),
    };
    let first = xobjects
        .values()
        .next()
        .and_then(Object::as_reference)
        .expect("xobject ref");
    doc.resolve(first)
        .unwrap()
        .as_stream()
        .unwrap()
        .dict
        .clone()
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

// === M4b annotation helpers (PRD §8.8) ====================================

/// Saves `doc` and reopens it (alias for [`save_reopen`], spelled out for the
/// annotation tests' reparse-oracle assertions).
pub fn save_reopen_annot(doc: &DocumentStore) -> DocumentStore {
    save_reopen(doc)
}

/// Resolves the dictionaries of all `/Annots` entries on the page at `index`
/// (through the overlay), in array order. Reopen-safe (resolves references).
pub fn annot_dicts(doc: &DocumentStore, index: usize) -> Vec<Dict> {
    let leaf = pdf_core::pagetree::page_refs(doc)[index];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let annots = match page.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec))
            .unwrap_or_default(),
        _ => return Vec::new(),
    };
    annots
        .iter()
        .filter_map(|o| match o {
            Object::Dictionary(d) => Some(d.clone()),
            Object::Reference(r) => doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned()),
            _ => None,
        })
        .collect()
}

/// The decoded bytes of an annotation dict's `/AP /N` Form XObject content
/// stream (empty if absent). Used to grep the appearance for color/path ops.
pub fn annot_ap_bytes(doc: &DocumentStore, annot: &Dict) -> Vec<u8> {
    let ap = match annot.get(&Name::new("AP")) {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(r)) => match doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned())
        {
            Some(d) => d,
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let n_ref = match ap.get(&Name::new("N")) {
        Some(Object::Reference(r)) => *r,
        _ => return Vec::new(),
    };
    let Ok(obj) = doc.resolve(n_ref) else {
        return Vec::new();
    };
    let Some(stream) = obj.as_stream() else {
        return Vec::new();
    };
    doc.decode_stream(stream)
        .and_then(|o| o.into_decoded())
        .unwrap_or_default()
}

/// The `/AP /N` Form XObject *dict* of an annotation (for `/BBox` / `/Subtype`
/// assertions), if present.
pub fn annot_ap_dict(doc: &DocumentStore, annot: &Dict) -> Option<Dict> {
    let ap = match annot.get(&Name::new("AP")) {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(r)) => doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned())?,
        _ => return None,
    };
    let n_ref = ap.get(&Name::new("N")).and_then(Object::as_reference)?;
    doc.resolve(n_ref).ok().and_then(|o| o.as_dict().cloned())
}

/// The `/Subtype` name of an annotation dict (as a `String`).
pub fn annot_subtype(annot: &Dict) -> String {
    annot
        .get(&Name::new("Subtype"))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
        .unwrap_or_default()
}

/// Runs `qpdf --check` over `bytes`, returning `Some(true)` for a clean/warn
/// result (exit 0/3), `Some(false)` for an error, and `None` when qpdf is
/// absent (so callers can skip). Cross-platform binary discovery.
pub fn qpdf_check(bytes: &[u8]) -> Option<bool> {
    use std::io::Write;
    use std::process::Command;
    let qpdf = [
        "qpdf",
        "qpdf.exe",
        "/opt/homebrew/bin/qpdf",
        "/usr/local/bin/qpdf",
    ]
    .into_iter()
    .find(|c| Command::new(c).arg("--version").output().is_ok())?;
    let mut tmp = std::env::temp_dir();
    tmp.push(format!(
        "oxide-pdf_m4b_qpdf_{}_{}.pdf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    {
        let mut f = std::fs::File::create(&tmp).ok()?;
        f.write_all(bytes).ok()?;
    }
    let out = Command::new(qpdf).arg("--check").arg(&tmp).output().ok()?;
    let _ = std::fs::remove_file(&tmp);
    let code = out.status.code().unwrap_or(-1);
    Some(code == 0 || code == 3)
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

// === M4c form (AcroForm) helpers (PRD §8.8) ===============================

/// A trivial empty Form XObject stream with the given `/BBox` (used as a
/// checkbox/radio on-state `/AP /N` appearance with a recognizable body).
fn ap_stream(marker: &str, w: i64, h: i64) -> Object {
    // A tiny body that draws a filled square — enough to be a non-empty,
    // bakeable appearance and to grep for the marker in the decompressed corpus.
    let body = format!("q 0 0 0 rg 1 1 {} {} re f Q % {marker}", w - 2, h - 2).into_bytes();
    Object::Stream(StreamObj::new_encoded(
        dict([
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Form")),
            ("FormType", Object::Integer(1)),
            (
                "BBox",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(w),
                    Object::Integer(h),
                ]),
            ),
            ("Length", Object::Integer(0)),
        ]),
        body,
    ))
}

/// A literal PDF string object.
fn pstr(s: &str) -> Object {
    Object::String(pdf_core::PdfString {
        bytes: s.as_bytes().to_vec(),
        kind: pdf_core::StringKind::Literal,
    })
}

fn rect_obj(x0: i64, y0: i64, x1: i64, y1: i64) -> Object {
    Object::Array(vec![
        Object::Integer(x0),
        Object::Integer(y0),
        Object::Integer(x1),
        Object::Integer(y1),
    ])
}

/// A single-page AcroForm fixture exercising all field types (PRD §8.8). Layout:
///
/// - 1: catalog (`/AcroForm` ref 10)
/// - 2: pages
/// - 3: page leaf (`/Annots` = all widgets)
/// - 4: page content (empty)
/// - 5: shared font (Helvetica)
/// - 10: AcroForm (`/Fields` [11 12 15 18], `/DR`, `/DA`, `/NeedAppearances false`)
/// - 11: **text field** `tx1` (merged widget, `/FT /Tx`, `/V (init)`, `/Q 0`)
/// - 12: **checkbox** `cb1` (merged widget, `/FT /Btn`, `/AP /N <</On 13 /Off 14>>`, `/AS /Off`)
/// - 13/14: checkbox On/Off appearance streams
/// - 15: **radio group** `rg1` (`/FT /Btn`, `/Ff 32768`, `/Kids [16 17]`)
/// - 16: radio kid A (widget, `/AP /N <</A 20 /Off 21>>`, `/AS /Off`)
/// - 17: radio kid B (widget, `/AP /N <</B 22 /Off 23>>`, `/AS /Off`)
/// - 20..23: radio appearance streams
/// - 18: **combo box** `ch1` (merged widget, `/FT /Ch`, `/Ff 131072`, `/Opt [a b c]`)
///
/// Widgets on the page `/Annots`: 11, 12, 16, 17, 18 (the radio group node 15 is
/// not a widget; its kids are).
pub fn acroform_doc() -> Vec<u8> {
    let widget = |extra: Dict| -> Dict {
        let mut d = dict([
            ("Type", name_obj("Annot")),
            ("Subtype", name_obj("Widget")),
            ("P", rref(3)),
        ]);
        for (k, v) in extra {
            d.insert(k, v);
        }
        d
    };

    // 11: text field (merged field+widget).
    let text_field = {
        let mut d = widget(dict([
            ("FT", name_obj("Tx")),
            ("T", pstr("tx1")),
            ("TU", pstr("Text One")),
            ("Rect", rect_obj(72, 700, 272, 720)),
            ("V", pstr("init")),
            ("DA", pstr("0 0 1 rg /Helv 12 Tf")),
            ("Q", Object::Integer(0)),
        ]));
        d.insert(Name::new("Type"), name_obj("Annot"));
        Object::Dictionary(d)
    };

    // 12: checkbox (merged), /AP /N <</On 13 /Off 14>>, /AS /Off.
    let checkbox = {
        let ap_n = dict([("On", rref(13)), ("Off", rref(14))]);
        let ap = dict([("N", Object::Dictionary(ap_n))]);
        Object::Dictionary(widget(dict([
            ("FT", name_obj("Btn")),
            ("T", pstr("cb1")),
            ("Rect", rect_obj(72, 660, 92, 680)),
            ("AP", Object::Dictionary(ap)),
            ("AS", name_obj("Off")),
            ("V", name_obj("Off")),
        ])))
    };

    // 15: radio group node (NOT a widget) with kids 16, 17.
    let radio_group = Object::Dictionary(dict([
        ("FT", name_obj("Btn")),
        ("Ff", Object::Integer(32768)),
        ("T", pstr("rg1")),
        ("Kids", Object::Array(vec![rref(16), rref(17)])),
        ("V", name_obj("Off")),
    ]));
    // 16: radio kid A — on-state "A".
    let radio_a = {
        let ap_n = dict([("A", rref(20)), ("Off", rref(21))]);
        let ap = dict([("N", Object::Dictionary(ap_n))]);
        Object::Dictionary(dict([
            ("Type", name_obj("Annot")),
            ("Subtype", name_obj("Widget")),
            ("P", rref(3)),
            ("Parent", rref(15)),
            ("Rect", rect_obj(72, 620, 92, 640)),
            ("AP", Object::Dictionary(ap)),
            ("AS", name_obj("Off")),
        ]))
    };
    // 17: radio kid B — on-state "B".
    let radio_b = {
        let ap_n = dict([("B", rref(22)), ("Off", rref(23))]);
        let ap = dict([("N", Object::Dictionary(ap_n))]);
        Object::Dictionary(dict([
            ("Type", name_obj("Annot")),
            ("Subtype", name_obj("Widget")),
            ("P", rref(3)),
            ("Parent", rref(15)),
            ("Rect", rect_obj(120, 620, 140, 640)),
            ("AP", Object::Dictionary(ap)),
            ("AS", name_obj("Off")),
        ]))
    };

    // 18: combo box (merged), /Opt [a b c].
    let combo = Object::Dictionary(widget(dict([
        ("FT", name_obj("Ch")),
        ("Ff", Object::Integer(131072)),
        ("T", pstr("ch1")),
        ("Rect", rect_obj(72, 580, 272, 600)),
        (
            "Opt",
            Object::Array(vec![pstr("alpha"), pstr("beta"), pstr("gamma")]),
        ),
        ("DA", pstr("0 g /Helv 11 Tf")),
    ])));

    // 10: AcroForm.
    let mut dr_font = Dict::new();
    dr_font.insert(Name::new("Helv"), rref(5));
    let dr = dict([("Font", Object::Dictionary(dr_font))]);
    let acroform = Object::Dictionary(dict([
        (
            "Fields",
            Object::Array(vec![rref(11), rref(12), rref(15), rref(18)]),
        ),
        ("NeedAppearances", Object::Boolean(false)),
        ("DA", pstr("0 0 0 rg /Helv 12 Tf")),
        ("DR", Object::Dictionary(dr)),
    ]));

    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2)),
        ("MediaBox", rect_obj(0, 0, 612, 792)),
        ("Contents", rref(4)),
        (
            "Resources",
            Object::Dictionary(dict([(
                "Font",
                Object::Dictionary(dict([("Helv", rref(5))])),
            )])),
        ),
        (
            "Annots",
            Object::Array(vec![rref(11), rref(12), rref(16), rref(17), rref(18)]),
        ),
    ]));

    let helv = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ]));

    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2)),
                ("AcroForm", rref(10)),
            ])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (3, page),
        (
            4,
            Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(0))]),
                Vec::new(),
            )),
        ),
        (5, helv),
        (10, acroform),
        (11, text_field),
        (12, checkbox),
        (13, ap_stream("cb-on", 20, 20)),
        (14, ap_stream("cb-off", 20, 20)),
        (15, radio_group),
        (16, radio_a),
        (17, radio_b),
        (18, combo),
        (20, ap_stream("ra-on", 20, 20)),
        (21, ap_stream("ra-off", 20, 20)),
        (22, ap_stream("rb-on", 20, 20)),
        (23, ap_stream("rb-off", 20, 20)),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// A form fixture with a **hierarchical** field name: a non-terminal parent
/// field `addr` whose kid is the terminal text field `city`, so the FQN is
/// `addr.city`. Layout:
/// - 1 catalog (/AcroForm 10), 2 pages, 3 page (/Annots [12]), 4 content, 5 font
/// - 10 AcroForm (/Fields [11])
/// - 11 parent field `addr` (/T addr, /Kids [12]) — non-terminal
/// - 12 terminal text widget `city` (/FT /Tx, /T city, /Parent 11, /V ...)
pub fn acroform_hierarchical_doc() -> Vec<u8> {
    let parent = Object::Dictionary(dict([
        ("T", pstr("addr")),
        ("Kids", Object::Array(vec![rref(12)])),
    ]));
    let city = Object::Dictionary(dict([
        ("Type", name_obj("Annot")),
        ("Subtype", name_obj("Widget")),
        ("P", rref(3)),
        ("Parent", rref(11)),
        ("FT", name_obj("Tx")),
        ("T", pstr("city")),
        ("Rect", rect_obj(72, 700, 272, 720)),
        ("V", pstr("Paris")),
        ("DA", pstr("0 g /Helv 12 Tf")),
    ]));
    let acroform = Object::Dictionary(dict([("Fields", Object::Array(vec![rref(11)]))]));
    let page = Object::Dictionary(dict([
        ("Type", name_obj("Page")),
        ("Parent", rref(2)),
        ("MediaBox", rect_obj(0, 0, 612, 792)),
        ("Contents", rref(4)),
        (
            "Resources",
            Object::Dictionary(dict([(
                "Font",
                Object::Dictionary(dict([("Helv", rref(5))])),
            )])),
        ),
        ("Annots", Object::Array(vec![rref(12)])),
    ]));
    let helv = Object::Dictionary(dict([
        ("Type", name_obj("Font")),
        ("Subtype", name_obj("Type1")),
        ("BaseFont", name_obj("Helvetica")),
        ("Encoding", name_obj("WinAnsiEncoding")),
    ]));
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([
                ("Type", name_obj("Catalog")),
                ("Pages", rref(2)),
                ("AcroForm", rref(10)),
            ])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (3, page),
        (
            4,
            Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(0))]),
                Vec::new(),
            )),
        ),
        (5, helv),
        (10, acroform),
        (11, parent),
        (12, city),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// Resolves the dict of an object number through the overlay (reopen-safe).
pub fn obj_dict(doc: &DocumentStore, num: u32) -> Dict {
    doc.get_object(num, 0)
        .ok()
        .and_then(|o| o.as_dict().cloned())
        .unwrap_or_default()
}

/// The decoded `/AP /N` bytes of a *text/choice* widget dict (single appearance
/// stream). Empty if `/N` is a state sub-dict or absent.
pub fn widget_ap_n_bytes(doc: &DocumentStore, widget: &Dict) -> Vec<u8> {
    let ap = match widget.get(&Name::new("AP")) {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(r)) => match doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned())
        {
            Some(d) => d,
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let n = match ap.get(&Name::new("N")) {
        Some(Object::Reference(r)) => *r,
        _ => return Vec::new(),
    };
    doc.resolve(n)
        .ok()
        .and_then(|o| o.as_stream().cloned())
        .and_then(|s| doc.decode_stream(&s).ok()?.into_decoded().ok())
        .unwrap_or_default()
}

/// The `/V` value of a field object number as a name string (for `/AS`/`/V` name
/// assertions), or empty.
pub fn name_value(doc: &DocumentStore, num: u32, key: &str) -> String {
    obj_dict(doc, num)
        .get(&Name::new(key))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
        .unwrap_or_default()
}

/// Counts objects resolving to a dict whose `/Subtype` == `Widget`.
pub fn count_widgets(doc: &DocumentStore) -> usize {
    let mut count = 0;
    for num in doc.xref().object_numbers() {
        if num == 0 {
            continue;
        }
        if let Ok(obj) = doc.get_object(num, 0) {
            if let Some(d) = obj.as_dict() {
                if d.get(&Name::new("Subtype"))
                    .and_then(Object::as_name)
                    .is_some_and(|t| t.as_bytes() == b"Widget")
                {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Reopens `bytes` and concatenates the **decoded** body of every stream object
/// plus the raw bytes, forming a decompressed corpus a test can byte-grep (the
/// M4 decompressed-corpus discipline: a compressed-only grep is a false pass).
pub fn decompress_corpus(bytes: &[u8]) -> Vec<u8> {
    let doc = open(bytes);
    let mut corpus = bytes.to_vec();
    for num in doc.xref().object_numbers() {
        if num == 0 {
            continue;
        }
        if let Ok(obj) = doc.get_object(num, 0) {
            if let Some(s) = obj.as_stream() {
                if let Ok(decoded) = doc.decode_stream(s).and_then(|o| o.into_decoded()) {
                    corpus.extend_from_slice(&decoded);
                    corpus.push(b'\n');
                }
            }
        }
    }
    corpus
}

// === M4d redaction fixtures (PRD §8.8) ====================================

/// Saves `doc` with **deflate on** (garbage=1, deflate=1) and returns the bytes.
/// The redaction security gate must hold over the *decompressed* corpus of a
/// compressed save — a compressed-only grep is a false pass (PRD §12 M4).
pub fn save_full_deflate_bytes(doc: &DocumentStore) -> Vec<u8> {
    doc.save_to_vec(
        &pdf_core::SaveOptions::default()
            .with_garbage(1)
            .with_deflate(true),
    )
    .expect("save")
}

/// A single-page document (612×792) showing two text runs on one line via one
/// `Tj` each at a fixed baseline: a leading visible run, then the secret run.
///
/// Layout (user space, baseline y=700, font 12, width 0.6em = 7.2pt/char):
/// - run A `lead` starts at x=72.
/// - run B `secret` starts at `x = 72 + lead.len()*7.2` (right after A).
///
/// Returns `(bytes, secret_topleft_rect)` where the rect (PyMuPDF top-left
/// space) tightly covers the secret run — feed it straight to `add_redact_annot`.
pub fn text_secret_doc(lead: &str, secret: &str) -> (Vec<u8>, pdf_core::geom::Rect) {
    let char_w = 12.0 * 0.6; // 7.2 pt per glyph
    let x_lead = 72.0;
    let x_secret = x_lead + lead.len() as f64 * char_w;
    let x_secret_end = x_secret + secret.len() as f64 * char_w;
    // One BT block: position A, show A; position B, show B.
    let body = format!(
        "BT /F1 12 Tf 1 0 0 1 {x_lead} 700 Tm ({lead}) Tj \
         1 0 0 1 {x_secret} 700 Tm ({secret}) Tj ET"
    )
    .into_bytes();
    let bytes = simple_text_page(body);
    // Top-left rect: user y 698..710 → top-left y (792-710)..(792-698) = 82..94.
    let rect = pdf_core::geom::Rect::new(x_secret - 1.0, 82.0, x_secret_end + 1.0, 96.0);
    (bytes, rect)
}

/// A single-page document whose page content draws `body` (user space), with a
/// shared ASCII Helvetica font under `/F1`. Object layout: 1 catalog, 2 pages,
/// 3 leaf, 4 content, 5 font.
pub fn simple_text_page(body: Vec<u8>) -> Vec<u8> {
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                (
                    "MediaBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Integer(612),
                        Object::Integer(792),
                    ]),
                ),
                ("Contents", rref(4)),
                (
                    "Resources",
                    Object::Dictionary(dict([(
                        "Font",
                        Object::Dictionary(dict([("F1", rref(5))])),
                    )])),
                ),
            ])),
        ),
        (
            4,
            Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(body.len() as i64))]),
                body,
            )),
        ),
        (5, ascii_font()),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// A single-page document that shows the secret **inside a Form XObject** (object
/// 6) referenced by the page via `/X1 Do`. The page also shows a visible `lead`
/// run directly. Returns `(bytes, secret_topleft_rect)`.
pub fn form_secret_doc(lead: &str, secret: &str) -> (Vec<u8>, pdf_core::geom::Rect) {
    let char_w = 12.0 * 0.6;
    let x_secret = 200.0;
    let x_secret_end = x_secret + secret.len() as f64 * char_w;
    // The form draws the secret at user (x_secret, 700).
    let form_body = format!("BT /F1 12 Tf 1 0 0 1 {x_secret} 700 Tm ({secret}) Tj ET").into_bytes();
    // The page draws the lead run directly, then invokes the form.
    let page_body = format!("BT /F1 12 Tf 1 0 0 1 72 700 Tm ({lead}) Tj ET\n/X1 Do").into_bytes();

    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                (
                    "MediaBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Integer(612),
                        Object::Integer(792),
                    ]),
                ),
                ("Contents", rref(4)),
                (
                    "Resources",
                    Object::Dictionary(dict([
                        ("Font", Object::Dictionary(dict([("F1", rref(5))]))),
                        ("XObject", Object::Dictionary(dict([("X1", rref(6))]))),
                    ])),
                ),
            ])),
        ),
        (
            4,
            Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(page_body.len() as i64))]),
                page_body,
            )),
        ),
        (5, ascii_font()),
        (
            6,
            Object::Stream(StreamObj::new_encoded(
                dict([
                    ("Type", name_obj("XObject")),
                    ("Subtype", name_obj("Form")),
                    ("FormType", Object::Integer(1)),
                    (
                        "BBox",
                        Object::Array(vec![
                            Object::Integer(0),
                            Object::Integer(0),
                            Object::Integer(612),
                            Object::Integer(792),
                        ]),
                    ),
                    (
                        "Resources",
                        Object::Dictionary(dict([(
                            "Font",
                            Object::Dictionary(dict([("F1", rref(5))])),
                        )])),
                    ),
                    ("Length", Object::Integer(form_body.len() as i64)),
                ]),
                form_body,
            )),
        ),
    ];
    let bytes = assemble_classic(&objects, ObjRef::new(1, 0));
    let rect = pdf_core::geom::Rect::new(x_secret - 1.0, 82.0, x_secret_end + 1.0, 96.0);
    (bytes, rect)
}

/// A single-page document placing a raw Flate RGB image XObject (`/X1`) filling
/// `[x, y_topleft, x+w, y_topleft+h]` (top-left space). The image is `iw×ih`
/// solid `(r,g,b)` so a covered region zeroes to black, distinguishable from the
/// fill. Returns the PDF bytes (image object is 6).
pub fn rgb_image_page(
    iw: u32,
    ih: u32,
    rgb: (u8, u8, u8),
    place_x: f64,
    place_y_topleft: f64,
    place_w: f64,
    place_h: f64,
) -> Vec<u8> {
    use pdf_core::filters::flate;
    let mut pixels = Vec::with_capacity((iw * ih * 3) as usize);
    for _ in 0..(iw * ih) {
        pixels.push(rgb.0);
        pixels.push(rgb.1);
        pixels.push(rgb.2);
    }
    let encoded = flate::encode(&pixels);
    // user-space placement: y_user_lower = 792 - (place_y_topleft + place_h).
    let y_user = 792.0 - (place_y_topleft + place_h);
    let content = format!("q {place_w} 0 0 {place_h} {place_x} {y_user} cm /X1 Do Q").into_bytes();
    image_page_with(iw, ih, encoded, "FlateDecode", "DeviceRGB", content)
}

/// A single-page document placing a **DCTDecode** (JPEG) image (`/X1`) — used to
/// prove fail-closed behavior (its pixels cannot be edited in v1). The JPEG body
/// is arbitrary opaque bytes (never decoded; redaction must fail before that).
pub fn dct_image_page(
    iw: u32,
    ih: u32,
    place_x: f64,
    place_y_topleft: f64,
    place_w: f64,
    place_h: f64,
) -> Vec<u8> {
    let jpeg = vec![0xFFu8, 0xD8, 0xFF, 0xD9]; // SOI…EOI placeholder
    let y_user = 792.0 - (place_y_topleft + place_h);
    let content = format!("q {place_w} 0 0 {place_h} {place_x} {y_user} cm /X1 Do Q").into_bytes();
    image_page_with(iw, ih, jpeg, "DCTDecode", "DeviceRGB", content)
}

/// Shared image-page builder: 1 catalog, 2 pages, 3 leaf (`/XObject /X1` → 6),
/// 4 content, 6 image stream.
fn image_page_with(
    iw: u32,
    ih: u32,
    encoded: Vec<u8>,
    filter: &str,
    colorspace: &str,
    content: Vec<u8>,
) -> Vec<u8> {
    let img = Object::Stream(StreamObj::new_encoded(
        dict([
            ("Type", name_obj("XObject")),
            ("Subtype", name_obj("Image")),
            ("Width", Object::Integer(i64::from(iw))),
            ("Height", Object::Integer(i64::from(ih))),
            ("ColorSpace", name_obj(colorspace)),
            ("BitsPerComponent", Object::Integer(8)),
            ("Filter", name_obj(filter)),
            ("Length", Object::Integer(encoded.len() as i64)),
        ]),
        encoded,
    ));
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                (
                    "MediaBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Integer(612),
                        Object::Integer(792),
                    ]),
                ),
                ("Contents", rref(4)),
                (
                    "Resources",
                    Object::Dictionary(dict([(
                        "XObject",
                        Object::Dictionary(dict([("X1", rref(6))])),
                    )])),
                ),
            ])),
        ),
        (
            4,
            Object::Stream(StreamObj::new_encoded(
                dict([("Length", Object::Integer(content.len() as i64))]),
                content,
            )),
        ),
        (6, img),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// Decodes the first image XObject (`/X1` on page 0) into raw bytes (for pixel
/// assertions after `REDACT-IMAGE` pixel-blanking).
pub fn first_image_pixels(doc: &DocumentStore) -> (usize, usize, usize, Vec<u8>) {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page dict");
    let resources = doc
        .resolve_dict_key(&page, &Name::new("Resources"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .expect("resources");
    let xobjects = match resources.get(&Name::new("XObject")) {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(r)) => doc.resolve(*r).unwrap().as_dict().cloned().unwrap(),
        _ => panic!("no /XObject"),
    };
    let r = xobjects
        .values()
        .next()
        .and_then(Object::as_reference)
        .expect("img ref");
    let obj = doc.resolve(r).unwrap();
    let stream = obj.as_stream().unwrap();
    let w = stream
        .dict
        .get(&Name::new("Width"))
        .and_then(Object::as_i64)
        .unwrap() as usize;
    let h = stream
        .dict
        .get(&Name::new("Height"))
        .and_then(Object::as_i64)
        .unwrap() as usize;
    let n = match stream
        .dict
        .get(&Name::new("ColorSpace"))
        .and_then(Object::as_name)
        .and_then(Name::as_str)
    {
        Some("DeviceGray") => 1,
        _ => 3,
    };
    let pixels = doc
        .decode_stream(stream)
        .and_then(|o| o.into_decoded())
        .unwrap()
        .to_vec();
    (w, h, n, pixels)
}

/// Whether the catalog still carries an `/AcroForm` entry.
pub fn catalog_has_acroform(doc: &DocumentStore) -> bool {
    doc.root()
        .and_then(|r| doc.resolve(r).ok())
        .and_then(|o| o.as_dict().cloned())
        .map(|c| c.contains_key(&Name::new("AcroForm")))
        .unwrap_or(false)
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
