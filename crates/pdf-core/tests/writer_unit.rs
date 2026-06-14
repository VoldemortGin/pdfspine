//! M3a full-save / writer structural tests — `SAVE-FULL-*`,
//! `SAVE-STREAM-DEFLATE-*`, `SAVE-XREF-*`, `SAVE-XREFSTREAM-*` (PRD §8.7).

mod common;

use common::{dict, find_first, simple_doc, SIMPLE_CONTENT};

use pdf_core::object::{Name, Object, StreamObj};
use pdf_core::{DocumentStore, Limits, SaveOptions, XrefStyle};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::default()).expect("opens")
}

// --- SAVE-FULL-* ----------------------------------------------------------

/// `SAVE-FULL-001`: save → reopen → equal `page_count`.
#[test]
fn save_full_001_page_count_preserved() {
    let doc = open(&simple_doc());
    let before = pdf_core::pagetree::page_count(&doc);
    let bytes = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&bytes);
    assert_eq!(pdf_core::pagetree::page_count(&re), before);
    assert_eq!(before, 1);
}

/// `SAVE-FULL-002`: every original in-use object survives (value-equal).
#[test]
fn save_full_002_all_objects_survive() {
    let doc = open(&simple_doc());
    let bytes = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&bytes);
    for num in 1u32..=5 {
        let a = doc.get_object(num, 0).unwrap();
        let b = re.get_object(num, 0).unwrap();
        // Streams compare by decoded body; other objects compare directly.
        if a.as_stream().is_some() {
            assert_eq!(doc.xref_stream(num).unwrap(), re.xref_stream(num).unwrap());
            assert_eq!(a.as_dict().unwrap(), b.as_dict().unwrap());
        } else {
            assert_eq!(*a, *b, "object {num} differs after save→reopen");
        }
    }
}

/// `SAVE-FULL-003`: the content-stream body (text source) is byte-equal across
/// save → reopen (text-extraction substance lives in pdf-text; here we assert
/// the underlying stream bytes survive, which is what extraction reads).
#[test]
fn save_full_003_content_stream_text_preserved() {
    let doc = open(&simple_doc());
    let bytes = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&bytes);
    assert_eq!(re.xref_stream(4).unwrap(), SIMPLE_CONTENT);
}

/// `SAVE-FULL-004`: output begins with `%PDF-` + a binary-comment line.
#[test]
fn save_full_004_header_and_binary_marker() {
    let doc = open(&simple_doc());
    let bytes = doc.save_to_vec(&SaveOptions::default()).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.7\n"), "header present");
    // Second line is a comment with high bytes.
    let second_line_start = find_first(&bytes, b"\n").unwrap() + 1;
    assert_eq!(bytes[second_line_start], b'%');
    assert!(
        bytes[second_line_start + 1] >= 0x80,
        "binary marker has high bytes"
    );
}

/// `SAVE-FULL-005`: trailer `/Root` preserved; `/Size` == max obj num + 1.
#[test]
fn save_full_005_root_and_size() {
    let doc = open(&simple_doc());
    let bytes = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&bytes);
    assert_eq!(re.root(), doc.root());
    // 5 objects → /Size 6.
    let size = re
        .trailer()
        .get(&Name::new("Size"))
        .and_then(Object::as_i64)
        .unwrap();
    assert_eq!(size, 6);
}

/// `SAVE-FULL-006`: `/ID` present, 2 elements (each 16 bytes).
#[test]
fn save_full_006_id_present_two_elements() {
    let doc = open(&simple_doc());
    let bytes = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&bytes);
    let id = re.trailer().get(&Name::new("ID")).unwrap();
    let arr = id.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    for el in arr {
        let s = el.as_string().unwrap();
        assert_eq!(s.bytes.len(), 16, "/ID element is 16 bytes");
    }
}

/// `SAVE-FULL-007`: an `/Info` reference is carried over when present.
#[test]
fn save_full_007_info_carried_over() {
    // A fixture whose trailer already carries an indirect `/Info`.
    let d = open(&build_doc_with_info());
    assert!(d.trailer().get(&Name::new("Info")).is_some());
    let bytes = d.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&bytes);
    assert!(
        re.trailer().get(&Name::new("Info")).is_some(),
        "/Info carried over"
    );
}

/// `SAVE-FULL-008`: a minimal doc (catalog + empty pages tree) saves + reopens.
#[test]
fn save_full_008_minimal_doc() {
    let bytes = build_minimal_doc();
    let doc = open(&bytes);
    let saved = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&saved);
    assert_eq!(re.root(), doc.root());
    assert_eq!(pdf_core::pagetree::page_count(&re), 0);
}

/// `SAVE-FULL-009`: save → reopen → save again yields the same live-object set.
#[test]
fn save_full_009_idempotent_object_set() {
    let doc = open(&simple_doc());
    let once = doc.save_to_vec(&SaveOptions::default()).unwrap();
    let re = open(&once);
    let twice = re.save_to_vec(&SaveOptions::default()).unwrap();
    let re2 = open(&twice);
    for num in 1u32..=5 {
        let a = re.get_object(num, 0).unwrap();
        let b = re2.get_object(num, 0).unwrap();
        if a.as_stream().is_some() {
            assert_eq!(re.xref_stream(num).unwrap(), re2.xref_stream(num).unwrap());
        } else {
            assert_eq!(*a, *b);
        }
    }
}

/// `SAVE-FULL-010`: classic-table output ends with `startxref`/`%%EOF`.
#[test]
fn save_full_010_ends_with_startxref_eof() {
    let doc = open(&simple_doc());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_xref_style(XrefStyle::Table))
        .unwrap();
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(40)..]);
    assert!(tail.contains("startxref"), "tail has startxref: {tail}");
    assert!(
        tail.trim_end().ends_with("%%EOF"),
        "tail ends with %%EOF: {tail}"
    );
}

// --- SAVE-STREAM-DEFLATE-* ------------------------------------------------

/// `SAVE-STREAM-DEFLATE-001`: a plain stream saved with deflate carries
/// `/FlateDecode`.
#[test]
fn save_stream_deflate_001_filter_added() {
    let doc = open(&simple_doc());
    // The content stream (4) is parsed as a Raw (already 'encoded' but no filter).
    // Replace it with a decoded body so the deflate policy applies.
    doc.update_stream(
        pdf_core::ObjRef::new(4, 0),
        dict([]),
        SIMPLE_CONTENT.to_vec(),
        false,
    )
    .unwrap();
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_deflate(true))
        .unwrap();
    let re = open(&bytes);
    let filter = re.xref_get_key(4, "Filter").unwrap().unwrap();
    assert!(filter.contains("FlateDecode"), "got filter {filter}");
}

/// `SAVE-STREAM-DEFLATE-002`: a deflated stream reopens + decodes to the
/// original bytes.
#[test]
fn save_stream_deflate_002_roundtrip() {
    let doc = open(&simple_doc());
    let body = b"some plain content stream data to compress".to_vec();
    doc.update_stream(pdf_core::ObjRef::new(4, 0), dict([]), body.clone(), false)
        .unwrap();
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_deflate(true))
        .unwrap();
    let re = open(&bytes);
    assert_eq!(re.xref_stream(4).unwrap(), body);
}

/// `SAVE-STREAM-DEFLATE-003`: an already-`/FlateDecode` stream is not
/// double-deflated (its body decodes to the original once).
#[test]
fn save_stream_deflate_003_no_double_deflate() {
    let doc = open(&simple_doc());
    let body = b"already deflated payload".to_vec();
    let encoded = common::flate_encode(&body);
    // Provide an already-encoded body with a matching /Filter.
    doc.update_stream(
        pdf_core::ObjRef::new(4, 0),
        dict([("Filter", Object::Name(Name::new("FlateDecode")))]),
        encoded.clone(),
        true,
    )
    .unwrap();
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_deflate(true))
        .unwrap();
    let re = open(&bytes);
    // One level of decode yields the original (not double-wrapped).
    assert_eq!(re.xref_stream(4).unwrap(), body);
    // Filter is a single FlateDecode, not an array of two.
    let filter = re.xref_get_key(4, "Filter").unwrap().unwrap();
    assert!(!filter.contains('['), "no nested filter array: {filter}");
}

/// `SAVE-STREAM-DEFLATE-004`: an image-filtered stream (`/DCTDecode`) is left
/// untouched (no FlateDecode wrapping, body byte-equal).
#[test]
fn save_stream_deflate_004_image_left_alone() {
    let doc = open(&simple_doc());
    let jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4, 0xFF, 0xD9];
    doc.update_stream(
        pdf_core::ObjRef::new(4, 0),
        dict([("Filter", Object::Name(Name::new("DCTDecode")))]),
        jpeg_bytes.clone(),
        true,
    )
    .unwrap();
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_deflate(true))
        .unwrap();
    let re = open(&bytes);
    let raw = re.xref_stream_raw(4).unwrap();
    assert_eq!(raw, jpeg_bytes, "image body untouched");
    let filter = re.xref_get_key(4, "Filter").unwrap().unwrap();
    assert!(filter.contains("DCTDecode"));
    assert!(!filter.contains("FlateDecode"));
}

/// `SAVE-STREAM-DEFLATE-005`: deflate=false keeps bodies as-is and recomputes
/// `/Length`.
#[test]
fn save_stream_deflate_005_keep_as_is() {
    let doc = open(&simple_doc());
    let body = b"verbatim body".to_vec();
    doc.update_stream(pdf_core::ObjRef::new(4, 0), dict([]), body.clone(), false)
        .unwrap();
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_deflate(false))
        .unwrap();
    let re = open(&bytes);
    let raw = re.xref_stream_raw(4).unwrap();
    assert_eq!(raw, body);
    let len = re
        .xref_get_key(4, "Length")
        .unwrap()
        .unwrap()
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(len, body.len());
}

// --- SAVE-XREF-* / SAVE-XREFSTREAM-* --------------------------------------

/// `SAVE-XREF-001`: the classic table has an object-0 free head.
#[test]
fn save_xref_001_free_head() {
    let doc = open(&simple_doc());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_xref_style(XrefStyle::Table))
        .unwrap();
    let xref_at = find_first(&bytes, b"xref\n").unwrap();
    let after = &bytes[xref_at..];
    let s = String::from_utf8_lossy(after);
    assert!(
        s.contains("0000000000 65535 f"),
        "object 0 free head present"
    );
}

/// `SAVE-XREF-002`: classic-table output reopens; objects intact.
#[test]
fn save_xref_002_table_reopens() {
    let doc = open(&simple_doc());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_xref_style(XrefStyle::Table))
        .unwrap();
    let re = open(&bytes);
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
    assert_eq!(re.xref_stream(4).unwrap(), SIMPLE_CONTENT);
}

/// `SAVE-XREFSTREAM-001`: xref-stream output has `/Type /XRef`, `/W`, `/Size`.
#[test]
fn save_xrefstream_001_dict_keys() {
    let doc = open(&simple_doc());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_xref_style(XrefStyle::Stream))
        .unwrap();
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/Type /XRef") || s.contains("/Type/XRef"),
        "Type XRef"
    );
    assert!(s.contains("/W "), "has /W");
    assert!(s.contains("/Size "), "has /Size");
}

/// `SAVE-XREFSTREAM-002` + `003`: xref-stream output is parseable by the M1c
/// xref-stream reader (via a normal `DocumentStore::open`) and objects survive.
#[test]
fn save_xrefstream_002_003_reader_roundtrip() {
    let doc = open(&simple_doc());
    let bytes = doc
        .save_to_vec(&SaveOptions::default().with_xref_style(XrefStyle::Stream))
        .unwrap();
    // Strict open forces the real (non-repair) xref-stream parser to run.
    let re = DocumentStore::from_bytes_with(bytes, pdf_core::ParseMode::Strict, Limits::default())
        .expect("xref-stream output parses cleanly");
    assert!(!re.parse_was_repaired(), "clean parse, no repair");
    assert_eq!(pdf_core::pagetree::page_count(&re), 1);
    assert_eq!(re.xref_stream(4).unwrap(), SIMPLE_CONTENT);
}

// --- fixtures -------------------------------------------------------------

fn build_doc_with_info() -> Vec<u8> {
    use common::{name_obj, rref, Pdf};
    let media = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ]);
    Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
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
        .obj(
            3,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2, 0)),
                ("MediaBox", media),
            ])),
        )
        .obj(
            4,
            0,
            Object::Dictionary(dict([(
                "Producer",
                Object::String(pdf_core::PdfString::literal(b"oxipdf".to_vec())),
            )])),
        )
        .root(1, 0)
        .trailer_key("Info", rref(4, 0))
        .build()
}

fn build_minimal_doc() -> Vec<u8> {
    use common::{name_obj, rref, Pdf};
    Pdf::new()
        .obj(
            1,
            0,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
        )
        .obj(
            2,
            0,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![])),
                ("Count", Object::Integer(0)),
            ])),
        )
        .root(1, 0)
        .build()
}

/// Keep `StreamObj` import exercised (used in the deflate tests indirectly).
#[test]
fn fixture_streamobj_smoke() {
    let s = StreamObj::new_encoded(dict([]), b"x".to_vec());
    assert_eq!(s.raw_bytes().as_ref(), b"x");
}
