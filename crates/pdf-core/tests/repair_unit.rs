//! `MODE-*`, `REPAIR-*` — malformed-PDF repair / reconstruction and the
//! Strict/Lenient parse modes (PRD §8 intro, §8.2, §9.3). Self-built malformed
//! fixtures: a well-formed PDF from the M1c builders, corrupted in one specific
//! way per test (PRD §10).

mod common;

use common::*;
use pdf_core::{DocumentStore, Error, Limits, Object, ParseMode, RepairKind, WarningKind};

fn limits() -> Limits {
    Limits::unbounded_decode()
}

fn open_lenient(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), limits()).expect("lenient open")
}

fn open_strict(bytes: &[u8]) -> Result<DocumentStore, Error> {
    DocumentStore::from_bytes_with(bytes.to_vec(), ParseMode::Strict, limits())
}

/// A minimal valid catalog + page-tree document. `/Root` is object 1.
fn minimal_doc() -> Vec<u8> {
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
            ])),
        )
        .root(1, 0)
        .build()
}

/// A valid doc with an extra data object (obj 3) carrying a probe integer.
fn doc_with_probe(probe: i64) -> Vec<u8> {
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
                ("Probe", rref(3, 0)),
            ])),
        )
        .obj(3, 0, Object::Integer(probe))
        .root(1, 0)
        .build()
}

fn resolve_int(doc: &DocumentStore, num: u32) -> Option<i64> {
    let obj = doc.resolve(pdf_core::ObjRef::new(num, 0)).ok()?;
    obj.as_i64()
}

fn has_action(doc: &DocumentStore, kind: RepairKind) -> bool {
    doc.repair_report().iter().any(|a| a.kind == kind)
}

fn has_warning(doc: &DocumentStore, kind: WarningKind) -> bool {
    doc.warnings().iter().any(|w| w.kind == kind)
}

// =========================================================================
// MODE-*
// =========================================================================

#[test]
fn mode_001_default_is_lenient() {
    // MODE-001
    let doc = open_lenient(&minimal_doc());
    assert_eq!(doc.parse_mode(), ParseMode::Lenient);
}

#[test]
fn mode_002_strict_opens_clean_file() {
    // MODE-002
    let doc = open_strict(&minimal_doc()).expect("strict open of clean file");
    assert_eq!(doc.parse_mode(), ParseMode::Strict);
    assert!(!doc.parse_was_repaired());
    assert!(doc.repair_report().is_empty());
}

#[test]
fn mode_003_strict_surfaces_broken_xref() {
    // MODE-003: missing startxref in Strict mode → typed Error::Xref.
    let bytes = corrupt_remove_startxref(&minimal_doc());
    let err = open_strict(&bytes).unwrap_err();
    assert_eq!(err.kind(), "xref", "{err:?}");
}

#[test]
fn mode_004_lenient_repairs_broken_xref() {
    // MODE-004: same broken xref in Lenient mode → repairs and opens.
    let bytes = corrupt_remove_startxref(&minimal_doc());
    let doc = open_lenient(&bytes);
    assert!(doc.parse_was_repaired());
    assert!(doc.root().is_some());
}

#[test]
fn mode_005_strict_dangling_is_error() {
    // MODE-005: a dangling ref in a Strict doc → typed Error::MissingObject.
    let bytes = doc_with_dangling();
    let doc = open_strict(&bytes).expect("strict open (valid catalog)");
    let err = doc.resolve(pdf_core::ObjRef::new(3, 0)).unwrap_err();
    assert!(
        matches!(err, Error::MissingObject { num: 99, .. }),
        "{err:?}"
    );
}

#[test]
fn mode_006_lenient_dangling_is_null() {
    // MODE-006: the same dangling ref in Lenient mode → Null (PRD §8.1/§8.2).
    let bytes = doc_with_dangling();
    let doc = open_lenient(&bytes);
    let obj = doc.resolve(pdf_core::ObjRef::new(3, 0)).expect("resolve");
    assert_eq!(obj.as_ref(), &Object::Null);
}

/// Valid catalog/pages; object 3 references the absent object 99.
fn doc_with_dangling() -> Vec<u8> {
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
            ])),
        )
        .obj(3, 0, rref(99, 0))
        .root(1, 0)
        .build()
}

// =========================================================================
// REPAIR-XREF-*
// =========================================================================

#[test]
fn repair_xref_001_missing_startxref() {
    // REPAIR-XREF-001
    let bytes = corrupt_remove_startxref(&doc_with_probe(42));
    let doc = open_lenient(&bytes);
    assert!(doc.parse_was_repaired());
    assert!(has_action(&doc, RepairKind::XrefRebuilt));
    assert_eq!(resolve_int(&doc, 3), Some(42));
}

#[test]
fn repair_xref_002_garbage_startxref() {
    // REPAIR-XREF-002
    let bytes = corrupt_garbage_startxref(&doc_with_probe(7));
    let doc = open_lenient(&bytes);
    assert!(doc.parse_was_repaired());
    assert_eq!(resolve_int(&doc, 3), Some(7));
}

#[test]
fn repair_xref_003_wrong_offsets_scan_recovers() {
    // REPAIR-XREF-003: corrupt every xref offset digit to 0 so the table points
    // at the wrong place; the scan must find the true offsets.
    let good = doc_with_probe(123);
    let mut bytes = good.clone();
    // Mangle the xref offset lines: turn "0000000NNN 00000 n" rows into zeros.
    // Easiest: replace the body between "xref\n0 N\n...trailer" — but simplest
    // robust corruption is to drop startxref so the chain is unfindable AND
    // also rewrite an in-table offset; here we drop the table+trailer entirely.
    bytes = corrupt_remove_xref_and_trailer(&bytes);
    let doc = open_lenient(&bytes);
    assert!(doc.parse_was_repaired());
    assert_eq!(resolve_int(&doc, 3), Some(123));
}

#[test]
fn repair_xref_004_value_equals_original() {
    // REPAIR-XREF-004: post-repair object value byte-equals the original parse.
    let good = doc_with_probe(99);
    let clean = open_lenient(&good);
    let clean_val = clean.resolve(pdf_core::ObjRef::new(3, 0)).unwrap();

    let broken = corrupt_remove_startxref(&good);
    let repaired = open_lenient(&broken);
    let rep_val = repaired.resolve(pdf_core::ObjRef::new(3, 0)).unwrap();

    assert_eq!(clean_val.as_ref(), rep_val.as_ref());
}

#[test]
fn repair_xref_005_objstm_members_recovered() {
    // REPAIR-XREF-005: objects packed in an ObjStm are recovered during the scan.
    // Build a doc by hand: catalog (obj 1) + pages (obj 2) as plain objects, and
    // an ObjStm (obj 4) packing object 3 (an integer probe). Then corrupt the
    // xref so reconstruction runs.
    let mut raw = RawPdf::new();
    raw.header();
    raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
            ("Probe", rref(3, 0)),
        ])),
    );
    // ObjStm (obj 4) packing object 3 = Integer(555).
    let member = write_value(&Object::Integer(555));
    raw.push_object(4, 0, &objstm_object(&[(3, member)]));
    // No xref / trailer at all → reconstruction path.
    let bytes = raw.finish();

    let doc = open_lenient(&bytes);
    assert!(doc.parse_was_repaired());
    assert!(has_action(&doc, RepairKind::ObjStmRecovered));
    assert_eq!(resolve_int(&doc, 3), Some(555));
}

#[test]
fn repair_xref_006_recovers_nonzero_gen() {
    // REPAIR-XREF-006: a `N G obj` with G > 0 is recovered (gen captured).
    let mut raw = RawPdf::new();
    raw.header();
    raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
            ("Probe", rref(3, 0)),
        ])),
    );
    // Object 3 with generation 5.
    raw.push_object(3, 5, &Object::Integer(314));
    let bytes = raw.finish();

    let doc = open_lenient(&bytes);
    assert_eq!(resolve_int(&doc, 3), Some(314));
}

// =========================================================================
// REPAIR-LEN-*
// =========================================================================

#[test]
fn repair_len_001_wrong_length_too_short() {
    // REPAIR-LEN-001: a stream whose /Length is far too short. The parser already
    // re-derives the body to `endstream` (M1a); ensure it holds under repair.
    let bytes = doc_with_stream(b"hello world stream body", Some(3));
    let doc = open_lenient(&bytes);
    let body = stream_body(&doc, 3);
    assert_eq!(body.as_ref(), b"hello world stream body");
}

#[test]
fn repair_len_002_missing_length() {
    // REPAIR-LEN-002: a stream with NO /Length → body recovered by scan.
    let bytes = doc_with_stream(b"no length here", None);
    let doc = open_lenient(&bytes);
    let body = stream_body(&doc, 3);
    assert_eq!(body.as_ref(), b"no length here");
}

#[test]
fn repair_len_003_recovered_flate_decodes() {
    // REPAIR-LEN-003: a Flate stream with a wrong /Length, recovered + decoded.
    let original = b"The quick brown fox jumps over the lazy dog. 0123456789";
    let enc = flate_encode(original);
    // Build the stream object with a deliberately-wrong /Length (too short).
    let mut raw = RawPdf::new();
    raw.header();
    raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    // Hand-write object 3 as a Flate stream with a lying /Length.
    let off3 = raw.pos();
    let _ = off3;
    let mut obj3 = format!(
        "3 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
        5 // lie
    )
    .into_bytes();
    obj3.extend_from_slice(&enc);
    obj3.extend_from_slice(b"\nendstream\nendobj\n");
    raw.raw(&obj3);
    let bytes = raw.finish();

    let doc = open_lenient(&bytes);
    let stream = doc.resolve(pdf_core::ObjRef::new(3, 0)).unwrap();
    let s = match stream.as_ref() {
        Object::Stream(s) => s,
        other => panic!("expected stream, got {other:?}"),
    };
    let decoded = doc.decode_stream(s).unwrap().into_decoded().unwrap();
    assert_eq!(decoded.as_slice(), original);
}

/// A doc whose object 3 is a raw stream with `body` and optional declared
/// `/Length` (when `None`, the `/Length` key is omitted). Object 2's `/Probe`
/// is removed (not needed here).
fn doc_with_stream(body: &[u8], declared_len: Option<usize>) -> Vec<u8> {
    let mut raw = RawPdf::new();
    raw.header();
    raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    let mut obj3 = match declared_len {
        Some(n) => format!("3 0 obj\n<< /Length {n} >>\nstream\n").into_bytes(),
        None => b"3 0 obj\n<< >>\nstream\n".to_vec(),
    };
    obj3.extend_from_slice(body);
    obj3.extend_from_slice(b"\nendstream\nendobj\n");
    raw.raw(&obj3);
    raw.finish()
}

fn stream_body(doc: &DocumentStore, num: u32) -> bytes::Bytes {
    let obj = doc.resolve(pdf_core::ObjRef::new(num, 0)).unwrap();
    match obj.as_ref() {
        // Raw bytes (no filter) — slice from source.
        Object::Stream(s) => doc.stream_raw_bytes(s).unwrap(),
        other => panic!("expected stream, got {other:?}"),
    }
}

// =========================================================================
// REPAIR-PREFIX-*
// =========================================================================

#[test]
fn repair_prefix_001_junk_before_header_and_broken_xref() {
    // REPAIR-PREFIX-001
    let good = Pdf::new()
        .prefix(b"\x89GARBAGE HTTP HEADER\r\n\r\n")
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
                ("Probe", rref(3, 0)),
            ])),
        )
        .obj(3, 0, Object::Integer(2024))
        .root(1, 0)
        .build();
    let broken = corrupt_remove_startxref(&good);
    let doc = open_lenient(&broken);
    assert!(doc.parse_was_repaired());
    assert_ne!(doc.header_offset(), 0);
    assert_eq!(resolve_int(&doc, 3), Some(2024));
}

#[test]
fn repair_prefix_002_scanned_offsets_are_absolute() {
    // REPAIR-PREFIX-002: with a prefix bias, the scanned (absolute) offsets must
    // resolve the catalog correctly.
    let good = Pdf::new()
        .prefix(b"%!PS-Adobe junk\n")
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
            ])),
        )
        .root(1, 0)
        .build();
    let broken = corrupt_remove_xref_and_trailer(&good);
    let doc = open_lenient(&broken);
    let root = doc.resolve(doc.root().unwrap()).unwrap();
    let d = root.as_dict().unwrap();
    assert_eq!(
        d.get(&pdf_core::Name::new("Type")),
        Some(&name_obj("Catalog"))
    );
}

// =========================================================================
// REPAIR-TRUNC-*
// =========================================================================

#[test]
fn repair_trunc_001_salvages_survivors() {
    // REPAIR-TRUNC-001: cut the file right after the object body (drop the xref +
    // trailer entirely); the complete objects must still be recoverable.
    let good = doc_with_probe(88);
    let cut = corrupt_truncate(&good, body_end_offset(&good));
    let doc = open_lenient(&cut);
    assert!(doc.parse_was_repaired());
    assert_eq!(resolve_int(&doc, 3), Some(88));
}

#[test]
fn repair_trunc_002_midobject_complete_objects_resolve() {
    // REPAIR-TRUNC-002: truncate a few bytes into the LAST object; earlier
    // complete objects must still resolve, and open must not panic/hang.
    let good = doc_with_probe(11);
    // Truncate at the offset of the third object's `obj` keyword + a few bytes,
    // so object 3 is incomplete but 1 & 2 are intact.
    let off3 = find_last(&good, b"3 0 obj").unwrap();
    let cut = corrupt_truncate(&good, off3 + 4);
    let doc = open_lenient(&cut);
    // The catalog (object 1) must still resolve.
    let root = doc.resolve(doc.root().unwrap()).unwrap();
    assert!(root.as_dict().is_some());
}

#[test]
fn repair_trunc_003_catalog_survives() {
    // REPAIR-TRUNC-003: as long as the catalog + pages survive, the doc opens and
    // Root resolves to a catalog.
    let good = doc_with_probe(5);
    let cut = corrupt_truncate(&good, body_end_offset(&good));
    let doc = open_lenient(&cut);
    let root = doc.resolve(doc.root().unwrap()).unwrap();
    let d = root.as_dict().unwrap();
    assert_eq!(
        d.get(&pdf_core::Name::new("Type")),
        Some(&name_obj("Catalog"))
    );
}

// =========================================================================
// REPAIR-TRAILER-*
// =========================================================================

#[test]
fn repair_trailer_001_root_from_catalog() {
    // REPAIR-TRAILER-001: no trailer at all → /Root rebuilt by finding the
    // /Type /Catalog object.
    let good = minimal_doc();
    let broken = corrupt_remove_xref_and_trailer(&good);
    let doc = open_lenient(&broken);
    assert!(has_action(&doc, RepairKind::RootRecovered));
    assert_eq!(doc.root().unwrap().num, 1);
}

#[test]
fn repair_trailer_002_synthetic_size() {
    // REPAIR-TRAILER-002: synthetic /Size ≥ max obj num + 1.
    let good = doc_with_probe(0); // objects 1,2,3
    let broken = corrupt_remove_xref_and_trailer(&good);
    let doc = open_lenient(&broken);
    let size = doc
        .trailer()
        .get(&pdf_core::Name::new("Size"))
        .and_then(Object::as_i64)
        .unwrap();
    assert!(size >= 4, "size was {size}");
}

#[test]
fn repair_trailer_003_multiple_catalogs_last_wins() {
    // REPAIR-TRAILER-003: two /Type /Catalog objects → the one with the higher
    // object number wins as /Root.
    let mut raw = RawPdf::new();
    raw.header();
    // First catalog at obj 1.
    raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    // Second (newer) catalog at obj 5, pointing at its own pages obj 6.
    raw.push_object(
        5,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(6, 0))])),
    );
    raw.push_object(
        6,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    let bytes = raw.finish();
    let doc = open_lenient(&bytes);
    assert_eq!(doc.root().unwrap().num, 5, "last catalog should win");
}

// =========================================================================
// REPAIR-DANGLING-*
// =========================================================================

#[test]
fn repair_dangling_001_lenient_null() {
    // REPAIR-DANGLING-001 (Lenient): ref to a non-existent object → Null.
    let doc = open_lenient(&doc_with_dangling());
    let obj = doc.resolve(pdf_core::ObjRef::new(3, 0)).unwrap();
    assert_eq!(obj.as_ref(), &Object::Null);
}

#[test]
fn repair_dangling_002_lenient_null_inside_dict() {
    // REPAIR-DANGLING-002: a dangling ref as a dict *value* resolves to Null.
    let bytes = Pdf::new()
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
                ("Count", Object::Integer(0)),
                ("Kids", Object::Array(vec![])),
                ("Bad", rref(404, 0)),
            ])),
        )
        .root(1, 0)
        .build();
    let doc = open_lenient(&bytes);
    let pages = doc.resolve(pdf_core::ObjRef::new(2, 0)).unwrap();
    let d = pages.as_dict().unwrap();
    let bad = doc
        .resolve_dict_key(d, &pdf_core::Name::new("Bad"))
        .unwrap()
        .unwrap();
    assert_eq!(bad.as_ref(), &Object::Null);
}

// =========================================================================
// REPAIR-DUP-*
// =========================================================================

#[test]
fn repair_dup_001_last_definition_wins() {
    // REPAIR-DUP-001: object 3 defined twice in the body; the LAST wins.
    let mut raw = RawPdf::new();
    raw.header();
    raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    // Two definitions of object 3 — the second (777) is the newer revision.
    raw.push_object(3, 0, &Object::Integer(111));
    raw.push_object(3, 0, &Object::Integer(777));
    let bytes = raw.finish();

    let doc = open_lenient(&bytes);
    assert_eq!(resolve_int(&doc, 3), Some(777));
}

#[test]
fn repair_dup_002_last_wins_with_prefix() {
    // REPAIR-DUP-002: duplicate definitions survive a header bias.
    let mut raw = RawPdf::new();
    raw.raw(b"leading junk bytes\n");
    raw.header();
    raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    raw.push_object(3, 0, &Object::Integer(1));
    raw.push_object(3, 0, &Object::Integer(2));
    let bytes = raw.finish();

    let doc = open_lenient(&bytes);
    assert_ne!(doc.header_offset(), 0);
    assert_eq!(resolve_int(&doc, 3), Some(2));
}

// =========================================================================
// REPAIR-GATE-*
// =========================================================================

#[test]
fn repair_gate_001_unreachable_root_triggers_repair() {
    // REPAIR-GATE-001: a CLEAN xref whose /Root entry points at the wrong offset
    // (so /Root won't resolve to a catalog) must auto-fall-back to a scan that
    // does find the catalog. We simulate by writing a valid table but with a
    // trailer /Root pointing at a nonexistent object number, while a real
    // /Type /Catalog exists in the body.
    let mut raw = RawPdf::new();
    raw.header();
    let off1 = raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    let off2 = raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    // Trailer /Root → object 9 (does not exist). Classic xref over 1,2.
    let mut trailer = dict([]);
    trailer.insert(pdf_core::Name::new("Root"), rref(9, 0));
    raw.classic_xref(3, &[(1, off1, 0, true), (2, off2, 0, true)], trailer);
    let bytes = raw.finish();

    let doc = open_lenient(&bytes);
    // Repair should have corrected /Root to the real catalog (object 1).
    assert!(doc.parse_was_repaired());
    let root = doc.resolve(doc.root().unwrap()).unwrap();
    assert_eq!(
        root.as_dict().unwrap().get(&pdf_core::Name::new("Type")),
        Some(&name_obj("Catalog"))
    );
}

#[test]
fn repair_gate_002_unreachable_pages_triggers_repair() {
    // REPAIR-GATE-002: catalog's /Pages points at a missing object in the clean
    // parse, but a real /Type /Pages exists. Repair must recover a working doc.
    // Build: clean xref lists only object 1 (catalog → /Pages 2). Object 2 (a
    // valid Pages node) physically exists but is omitted from the xref table.
    let mut raw = RawPdf::new();
    raw.header();
    let off1 = raw.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2, 0))])),
    );
    let _off2 = raw.push_object(
        2,
        0,
        &Object::Dictionary(dict([
            ("Type", name_obj("Pages")),
            ("Count", Object::Integer(0)),
            ("Kids", Object::Array(vec![])),
        ])),
    );
    // xref omits object 2 (Size 2 → only object 1 in use).
    let mut trailer = dict([]);
    trailer.insert(pdf_core::Name::new("Root"), rref(1, 0));
    raw.classic_xref(2, &[(1, off1, 0, true)], trailer);
    let bytes = raw.finish();

    let doc = open_lenient(&bytes);
    assert!(doc.parse_was_repaired());
    // /Pages must now resolve to a page-tree node.
    let root = doc.resolve(doc.root().unwrap()).unwrap();
    let pages = doc
        .resolve_dict_key(root.as_dict().unwrap(), &pdf_core::Name::new("Pages"))
        .unwrap()
        .unwrap();
    assert!(pages.as_dict().is_some());
}

#[test]
fn repair_gate_003_valid_file_no_repair() {
    // REPAIR-GATE-003: a fully valid file passes the gate without repair.
    let doc = open_lenient(&minimal_doc());
    assert!(!doc.parse_was_repaired());
    assert!(doc.repair_report().is_empty());
    assert!(doc.warnings().is_empty());
}

// =========================================================================
// REPAIR-REPORT-*
// =========================================================================

#[test]
fn repair_report_001_flag_set_after_scan() {
    // REPAIR-REPORT-001
    let bytes = corrupt_remove_xref_and_trailer(&minimal_doc());
    let doc = open_lenient(&bytes);
    assert!(doc.parse_was_repaired());
}

#[test]
fn repair_report_002_lists_actions() {
    // REPAIR-REPORT-002
    let bytes = corrupt_remove_xref_and_trailer(&minimal_doc());
    let doc = open_lenient(&bytes);
    assert!(has_action(&doc, RepairKind::XrefRebuilt));
    assert!(has_action(&doc, RepairKind::TrailerSynthesized));
    assert!(has_action(&doc, RepairKind::RootRecovered));
}

#[test]
fn repair_report_003_collects_warnings() {
    // REPAIR-REPORT-003: warnings carry offset/kind/detail.
    let bytes = corrupt_remove_startxref(&minimal_doc());
    let doc = open_lenient(&bytes);
    assert!(!doc.warnings().is_empty());
    assert!(has_warning(&doc, WarningKind::XrefUnreadable));
    // Detail is non-empty English prose.
    assert!(doc.warnings().iter().all(|w| !w.detail.is_empty()));
}

#[test]
fn repair_report_004_stable_kind_strings() {
    // REPAIR-REPORT-004: discriminant strings are stable / English / kebab-case.
    assert_eq!(WarningKind::XrefUnreadable.as_str(), "xref-unreadable");
    assert_eq!(
        WarningKind::DanglingReference.as_str(),
        "dangling-reference"
    );
    assert_eq!(RepairKind::XrefRebuilt.as_str(), "xref-rebuilt");
    assert_eq!(RepairKind::RootRecovered.as_str(), "root-recovered");
    // Every kind string is lowercase ASCII with hyphens only.
    for k in [
        WarningKind::StartxrefMissing,
        WarningKind::XrefUnreadable,
        WarningKind::ValidationFailed,
        WarningKind::StreamLength,
        WarningKind::DanglingReference,
        WarningKind::UnparseableObject,
        WarningKind::TrailerReconstructed,
        WarningKind::HeaderOffset,
        WarningKind::ObjStmUndecodable,
    ] {
        let s = k.as_str();
        assert!(
            s.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
            "{s}"
        );
    }
}

#[test]
fn repair_report_005_clean_open_empty_report() {
    // REPAIR-REPORT-005
    let doc = open_lenient(&minimal_doc());
    assert!(!doc.parse_was_repaired());
    assert!(doc.repair_report().is_empty());
    assert!(doc.warnings().is_empty());
}
