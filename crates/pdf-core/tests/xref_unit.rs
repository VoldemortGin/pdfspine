//! `XREF-*` (classic table), `XREFSTM-*` (xref streams), `PREV-*` (`/Prev`
//! chains), `HYBRID-*` (hybrid-reference). Self-built fixtures. PRD §8.2.

mod common;

use common::*;
use pdf_core::source::Source;
use pdf_core::xref::{parse_xref_chain, XrefEntry};
use pdf_core::{DocumentStore, Error, Limits, Object};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::unbounded_decode()).expect("open")
}

fn xref_of(bytes: &[u8]) -> pdf_core::xref::XrefTable {
    let src = Source::from_bytes(bytes.to_vec());
    parse_xref_chain(&src, 0, &Limits::unbounded_decode()).expect("xref")
}

// --- XREF-* (classic table) ----------------------------------------------

#[test]
fn xref_001_startxref_discovery() {
    // XREF-001: find_startxref scans the tail and returns the table offset.
    let bytes = Pdf::new().obj(1, 0, Object::Integer(7)).root(1, 0).build();
    let src = Source::from_bytes(bytes.clone());
    let off = pdf_core::xref::find_startxref(&src).unwrap();
    assert_eq!(&bytes[off..off + 4], b"xref");
}

#[test]
fn xref_002_single_subsection() {
    // XREF-002
    let bytes = Pdf::new()
        .obj(1, 0, Object::Integer(11))
        .obj(2, 0, Object::Integer(22))
        .root(1, 0)
        .build();
    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
    assert!(matches!(xref.get(2), Some(XrefEntry::Uncompressed { .. })));
    // The recorded offset really points at `1 0 obj`.
    if let Some(XrefEntry::Uncompressed { offset, .. }) = xref.get(1) {
        assert_eq!(&bytes[offset..offset + 7], b"1 0 obj");
    }
}

#[test]
fn xref_003_multi_subsection() {
    // XREF-003: two disjoint subsections (0..2) and (5..6).
    let mut p = RawPdf::new();
    p.header();
    p.push_object(1, 0, &Object::Integer(1));
    p.push_object(5, 0, &Object::Integer(5));
    let (o1, o5) = (p.offset_of(1), p.offset_of(5));
    let xref_at = p.pos();
    // Hand-write a two-subsection table.
    p.raw(b"xref\n");
    p.raw(b"0 2\n");
    p.raw(b"0000000000 65535 f \n");
    p.raw(format!("{o1:010} 00000 n \n").as_bytes());
    p.raw(b"5 1\n");
    p.raw(format!("{o5:010} 00000 n \n").as_bytes());
    p.raw(b"trailer\n");
    p.raw(b"<< /Size 6 /Root 1 0 R >>\n");
    p.raw(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
    assert!(matches!(xref.get(5), Some(XrefEntry::Uncompressed { .. })));
    assert_eq!(xref.get(3), None);
}

#[test]
fn xref_004_free_entry() {
    // XREF-004: an object marked free (`f`).
    let mut p = RawPdf::new();
    p.header();
    p.push_object(1, 0, &Object::Integer(1));
    let o1 = p.offset_of(1);
    let xref_at = p.pos();
    p.raw(b"xref\n0 3\n");
    p.raw(b"0000000000 65535 f \n");
    p.raw(format!("{o1:010} 00000 n \n").as_bytes());
    p.raw(b"0000000000 00000 f \n"); // object 2 free
    p.raw(b"trailer\n<< /Size 3 /Root 1 0 R >>\n");
    p.raw(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert_eq!(xref.get(2), Some(XrefEntry::Free));
}

#[test]
fn xref_005_generation_preserved() {
    // XREF-005: a nonzero generation number is recorded.
    let mut p = RawPdf::new();
    p.header();
    p.push_object(1, 3, &Object::Integer(1));
    let o1 = p.offset_of(1);
    let xref_at = p.pos();
    p.raw(b"xref\n0 2\n");
    p.raw(b"0000000000 65535 f \n");
    p.raw(format!("{o1:010} 00003 n \n").as_bytes());
    p.raw(b"trailer\n<< /Size 2 /Root 1 3 R >>\n");
    p.raw(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert_eq!(
        xref.get(1),
        Some(XrefEntry::Uncompressed { offset: o1, gen: 3 })
    );
}

#[test]
fn xref_006_trailer_parse() {
    // XREF-006
    let bytes = Pdf::new()
        .obj(1, 0, Object::Integer(1))
        .root(1, 0)
        .trailer_key("Info", rref(9, 0))
        .build();
    let xref = xref_of(&bytes);
    let t = xref.trailer();
    assert_eq!(t.get(&n("Root")).unwrap(), &rref(1, 0));
    assert_eq!(t.get(&n("Info")).unwrap(), &rref(9, 0));
    assert!(t.get(&n("Size")).is_some());
}

#[test]
fn xref_007_object_resolved_by_offset() {
    // XREF-007: resolve uses the offset and yields the serialized object.
    let bytes = Pdf::new()
        .obj(1, 0, Object::Dictionary(dict([("K", Object::Integer(99))])))
        .root(1, 0)
        .build();
    let doc = open(&bytes);
    let obj = doc.get_object(1, 0).unwrap();
    assert_eq!(
        obj.as_dict().unwrap().get(&n("K")).unwrap(),
        &Object::Integer(99)
    );
}

#[test]
fn xref_008_short_entry_variant() {
    // XREF-008: 19-byte entries with a single `\n` terminator (no trailing
    // space before EOL). Our scanner tolerates the variant.
    let mut p = RawPdf::new();
    p.header();
    p.push_object(1, 0, &Object::Integer(1));
    let o1 = p.offset_of(1);
    let xref_at = p.pos();
    p.raw(b"xref\n0 2\n");
    p.raw(b"0000000000 65535 f\n"); // 19-byte: no trailing space
    p.raw(format!("{o1:010} 00000 n\n").as_bytes());
    p.raw(b"trailer\n<< /Size 2 /Root 1 0 R >>\n");
    p.raw(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
}

#[test]
fn xref_009_last_startxref_wins() {
    // XREF-009: two %%EOF/startxref; the LAST one is used.
    let mut p = RawPdf::new();
    p.header();
    p.push_object(1, 0, &Object::Integer(111));
    let o1 = p.offset_of(1);
    // First (bogus) xref + startxref pointing at a wrong offset.
    let bad_xref = p.pos();
    p.raw(b"xref\n0 2\n0000000000 65535 f \n0000000000 00000 f \n");
    p.raw(b"trailer\n<< /Size 2 /Root 1 0 R >>\n");
    p.raw(format!("startxref\n{bad_xref}\n%%EOF\n").as_bytes());
    // Second (good) xref + startxref — this is the one that must win.
    let good_xref = p.pos();
    p.raw(b"xref\n0 2\n");
    p.raw(b"0000000000 65535 f \n");
    p.raw(format!("{o1:010} 00000 n \n").as_bytes());
    p.raw(b"trailer\n<< /Size 2 /Root 1 0 R >>\n");
    p.raw(format!("startxref\n{good_xref}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
}

#[test]
fn xref_010_missing_startxref_typed_error() {
    // XREF-010: garbage with no startxref → typed Error::Xref, no panic.
    let src = Source::from_bytes(b"%PDF-1.7\nnothing here at all".to_vec());
    let err = parse_xref_chain(&src, 0, &Limits::default()).unwrap_err();
    assert!(matches!(err, Error::Xref { .. }), "{err:?}");
}

// --- XREFSTM-* (cross-reference streams) ----------------------------------

/// Builds a doc whose xref is a `/Type /XRef` stream. `obj_specs` are the
/// uncompressed objects (besides the xref stream itself); the xref stream is the
/// last object. Returns the bytes.
fn doc_with_xref_stream(predictor: bool) -> Vec<u8> {
    let mut p = RawPdf::new();
    p.header();
    // Object 1: catalog. Object 2: a value.
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    p.push_object(2, 0, &Object::Integer(4242));
    let (o1, o2) = (p.offset_of(1), p.offset_of(2));

    // The xref stream is object 3; it records itself too.
    let xref_off = p.pos();
    // Records for objects 0..4: type/field2/field3.
    // 0: free (0,0,65535) | 1: (1,o1,0) | 2: (1,o2,0) | 3: (1,xref_off,0)
    let records = vec![
        (0u64, 0u64, 65535u64),
        (1, o1 as u64, 0),
        (1, o2 as u64, 0),
        (1, xref_off as u64, 0),
    ];
    let widths = [1usize, 2, 2];
    let data = pack_xref_records(&records, widths);
    let cols = widths.iter().sum::<usize>();

    let xstream = xref_stream_object(
        &data,
        widths,
        None,
        4,
        [("Root", rref(1, 0))],
        if predictor { Some(cols) } else { None },
    );
    p.push_object(3, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    p.finish()
}

#[test]
fn xrefstm_001_three_entry_types() {
    // XREFSTM-001 / XREFSTM-005 / XREFSTM-007
    let bytes = doc_with_xref_stream(false);
    let xref = xref_of(&bytes);
    assert_eq!(xref.get(0), Some(XrefEntry::Free));
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
    assert!(matches!(xref.get(2), Some(XrefEntry::Uncompressed { .. })));
    // Resolve through it.
    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(2, 0).unwrap().as_ref(),
        &Object::Integer(4242)
    );
}

#[test]
fn xrefstm_002_index_ranges() {
    // XREFSTM-002: /Index with a non-zero start.
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    p.push_object(7, 0, &Object::Integer(777));
    let (o1, o7) = (p.offset_of(1), p.offset_of(7));
    let xref_off = p.pos();
    // Index [0 2 7 2]: covers objects {0,1} then {7,8(xrefstream)}.
    let records = vec![
        (0u64, 0u64, 65535u64),  // obj 0 free
        (1, o1 as u64, 0),       // obj 1
        (1, o7 as u64, 0),       // obj 7
        (1, xref_off as u64, 0), // obj 8 (xref stream)
    ];
    let widths = [1usize, 2, 2];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(
        &data,
        widths,
        Some(vec![0, 2, 7, 2]),
        9,
        [("Root", rref(1, 0))],
        None,
    );
    p.push_object(8, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(7), Some(XrefEntry::Uncompressed { .. })));
    assert_eq!(xref.get(2), None);
    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(7, 0).unwrap().as_ref(),
        &Object::Integer(777)
    );
}

#[test]
fn xrefstm_003_predictor_encoded() {
    // XREFSTM-003: PNG-up predictor on the xref stream data.
    let bytes = doc_with_xref_stream(true);
    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(2, 0).unwrap().as_ref(),
        &Object::Integer(4242)
    );
}

#[test]
fn xrefstm_004_varied_w_widths() {
    // XREFSTM-004: /W [1 3 2].
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    let o1 = p.offset_of(1);
    let xref_off = p.pos();
    let records = vec![
        (0u64, 0u64, 65535u64),
        (1, o1 as u64, 0),
        (1, xref_off as u64, 0),
    ];
    let widths = [1usize, 3, 2];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(&data, widths, None, 3, [("Root", rref(1, 0))], None);
    p.push_object(2, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    if let Some(XrefEntry::Uncompressed { offset, .. }) = xref.get(1) {
        assert_eq!(offset, o1);
    } else {
        panic!("obj 1 not uncompressed");
    }
}

#[test]
fn xrefstm_006_default_w_field_width_zero() {
    // XREFSTM-006: /W [0 2 1] → field-1 width 0 means "type defaults to 1".
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    let o1 = p.offset_of(1);
    let xref_off = p.pos();
    // With width-0 type field, every record is type-1; emit obj0 too (its offset
    // is irrelevant / treated as uncompressed but unused).
    let records = vec![
        (0u64, 0u64, 0u64),
        (0, o1 as u64, 0),
        (0, xref_off as u64, 0),
    ];
    let widths = [0usize, 2, 1];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(&data, widths, None, 3, [("Root", rref(1, 0))], None);
    p.push_object(2, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { offset, .. }) if offset == o1));
}

#[test]
fn xrefstm_008_malformed_w_typed_error() {
    // XREFSTM-008: /W with the wrong length → typed Error::Xref.
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    let xref_off = p.pos();
    // /W has only 2 entries → invalid.
    let data = vec![0u8; 6];
    let xstream = xref_stream_object(&data, [1, 2, 0], None, 2, [("Root", rref(1, 0))], None);
    // Patch the /W to be length-2 by rebuilding the dict manually.
    let xstream = match xstream {
        Object::Stream(mut s) => {
            s.dict.insert(
                n("W"),
                Object::Array(vec![Object::Integer(1), Object::Integer(2)]),
            );
            Object::Stream(s)
        }
        o => o,
    };
    p.push_object(2, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let src = Source::from_bytes(bytes);
    let err = parse_xref_chain(&src, 0, &Limits::unbounded_decode()).unwrap_err();
    assert!(matches!(err, Error::Xref { .. }), "{err:?}");
}

// --- PREV-* (/Prev chains) ------------------------------------------------

/// A two-revision file: revision 1 defines objects 1 (catalog) & 2; revision 2
/// (appended) overrides object 2 and chains back via `/Prev`.
fn two_revision(override_obj2: Object, free_obj2: bool) -> Vec<u8> {
    let mut p = RawPdf::new();
    p.header();
    // Revision 1 body.
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    p.push_object(2, 0, &Object::Integer(100));
    let (o1, o2v1) = (p.offset_of(1), p.offset_of(2));
    let rev1_xref = p.pos();
    p.classic_xref(
        3,
        &[(1, o1, 0, true), (2, o2v1, 0, true)],
        dict([("Root", rref(1, 0))]),
    );

    // Revision 2: append a new object 2 (or just free it) + a new xref with
    // /Prev → rev1_xref.
    let o2v2 = if free_obj2 {
        0
    } else {
        p.push_object(2, 0, &override_obj2)
    };
    let rev2_xref = p.pos();
    // A real incremental update writes only the objects it changed. Rev2's
    // subsection covers object 2 alone (`2 1`); object 1 stays defined by rev1
    // via the /Prev chain (newest-wins must not clobber it).
    p.raw(b"xref\n2 1\n");
    if free_obj2 {
        p.raw(b"0000000000 00000 f \n");
    } else {
        p.raw(format!("{o2v2:010} 00000 n \n").as_bytes());
    }
    p.raw(format!("trailer\n<< /Size 3 /Root 1 0 R /Prev {rev1_xref} >>\n").as_bytes());
    p.raw(format!("startxref\n{rev2_xref}\n%%EOF\n").as_bytes());
    p.finish()
}

#[test]
fn prev_001_chain_followed() {
    // PREV-001: object only defined in the older section is still visible.
    let bytes = two_revision(Object::Integer(200), false);
    let xref = xref_of(&bytes);
    // Object 1 came from the OLD section (rev2 marks it free) — newest-wins means
    // rev2's free for obj1, but obj1 is only really defined in rev1. The chain
    // must surface it from rev1.
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
}

#[test]
fn prev_002_newest_wins() {
    // PREV-002: object 2 overridden in rev2 resolves to the NEW value.
    let bytes = two_revision(Object::Integer(200), false);
    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(2, 0).unwrap().as_ref(),
        &Object::Integer(200)
    );
}

#[test]
fn prev_003_later_section_refrees() {
    // PREV-003: rev2 re-frees object 2 → it resolves as missing.
    let bytes = two_revision(Object::Null, true);
    let xref = xref_of(&bytes);
    assert_eq!(xref.get(2), Some(XrefEntry::Free));
    let doc = open(&bytes);
    let err = doc.get_object(2, 0).unwrap_err();
    assert!(
        matches!(err, Error::MissingObject { num: 2, .. }),
        "{err:?}"
    );
}

#[test]
fn prev_004_prev_cycle_terminates() {
    // PREV-004: a /Prev that points back at the same section must not loop.
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    let o1 = p.offset_of(1);
    let xref_at = p.pos();
    p.raw(b"xref\n0 2\n");
    p.raw(b"0000000000 65535 f \n");
    p.raw(format!("{o1:010} 00000 n \n").as_bytes());
    // /Prev points at itself → cycle.
    p.raw(format!("trailer\n<< /Size 2 /Root 1 0 R /Prev {xref_at} >>\n").as_bytes());
    p.raw(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    // Must terminate and still produce the table (no hang, no panic).
    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(1), Some(XrefEntry::Uncompressed { .. })));
}

// --- HYBRID-* (hybrid-reference) ------------------------------------------

/// A hybrid file: a classic table covers object 1 (catalog); an `/XRefStm`
/// overlay covers object 2. Both must resolve.
fn hybrid_file() -> Vec<u8> {
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    p.push_object(2, 0, &Object::Integer(2222));
    let (o1, o2) = (p.offset_of(1), p.offset_of(2));

    // The xref stream (object 3) covers object 2 (+ itself).
    let stm_off = p.pos();
    let records = vec![
        (0u64, 0u64, 65535u64),
        (0, 0, 0),              // obj 1 placeholder (provided by classic table)
        (1, o2 as u64, 0),      // obj 2
        (1, stm_off as u64, 0), // obj 3 (the stream)
    ];
    let widths = [1usize, 2, 2];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(&data, widths, None, 4, [], None);
    p.push_object(3, 0, &xstream);

    // Classic table covers only object 1; trailer has /XRefStm → stm_off.
    let classic_at = p.pos();
    p.raw(b"xref\n0 2\n");
    p.raw(b"0000000000 65535 f \n");
    p.raw(format!("{o1:010} 00000 n \n").as_bytes());
    p.raw(format!("trailer\n<< /Size 4 /Root 1 0 R /XRefStm {stm_off} >>\n").as_bytes());
    p.raw(format!("startxref\n{classic_at}\n%%EOF\n").as_bytes());
    p.finish()
}

#[test]
fn hybrid_001_xrefstm_overlay_resolves() {
    // HYBRID-001: object only in the /XRefStm stream resolves.
    let bytes = hybrid_file();
    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(2, 0).unwrap().as_ref(),
        &Object::Integer(2222)
    );
}

#[test]
fn hybrid_002_classic_object_resolves() {
    // HYBRID-002: object in the classic table resolves (both ways work).
    let bytes = hybrid_file();
    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(1, 0)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(&n("Type"))
            .unwrap(),
        &name_obj("Catalog")
    );
}
