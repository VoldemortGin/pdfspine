//! XREFSTM-EDGE-* — xref-stream /W & /Index variants via the public chain. PRD §8.2.

mod common;

use common::*;
use pdf_core::source::Source;
use pdf_core::xref::{parse_xref_chain, XrefEntry};
use pdf_core::{DocumentStore, Limits, Object};

fn open(bytes: &[u8]) -> DocumentStore {
    DocumentStore::from_bytes(bytes.to_vec(), Limits::unbounded_decode()).expect("open")
}

fn xref_of(bytes: &[u8]) -> pdf_core::xref::XrefTable {
    let src = Source::from_bytes(bytes.to_vec());
    parse_xref_chain(&src, 0, &Limits::unbounded_decode()).expect("xref")
}

/// XREFSTM-EDGE-1: a `/Type /XRef` stream with the default `/Index` (none) and
/// `/W [1 2 2]` resolves an uncompressed object through the public chain.
#[test]
fn xrefstm_edge_default_index_resolves_object() {
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    p.push_object(2, 0, &Object::Integer(31337));
    let (o1, o2) = (p.offset_of(1), p.offset_of(2));

    let xref_off = p.pos();
    let widths = [1usize, 2, 2];
    let records = vec![
        (0u64, 0u64, 65535u64),
        (1, o1 as u64, 0),
        (1, o2 as u64, 0),
        (1, xref_off as u64, 0),
    ];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(&data, widths, None, 4, [("Root", rref(1, 0))], None);
    p.push_object(3, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(2, 0).unwrap().as_ref(),
        &Object::Integer(31337)
    );
}

/// XREFSTM-EDGE-2: an `/Index` with a non-zero start segment ([0 2 9 2]) is
/// honoured — only the declared object numbers exist, and the high-numbered one
/// resolves.
#[test]
fn xrefstm_edge_index_nonzero_start() {
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    p.push_object(9, 0, &Object::Integer(909));
    let (o1, o9) = (p.offset_of(1), p.offset_of(9));

    let xref_off = p.pos();
    // Index [0 2 9 2]: covers {0,1} then {9, 10(=xref stream)}.
    let widths = [1usize, 2, 2];
    let records = vec![
        (0u64, 0u64, 65535u64),  // obj 0 free
        (1, o1 as u64, 0),       // obj 1
        (1, o9 as u64, 0),       // obj 9
        (1, xref_off as u64, 0), // obj 10 (xref stream)
    ];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(
        &data,
        widths,
        Some(vec![0, 2, 9, 2]),
        11,
        [("Root", rref(1, 0))],
        None,
    );
    p.push_object(10, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    assert!(matches!(xref.get(9), Some(XrefEntry::Uncompressed { .. })));
    // Object numbers outside the declared /Index segments are absent.
    assert_eq!(xref.get(5), None);

    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(9, 0).unwrap().as_ref(),
        &Object::Integer(909)
    );
}

/// XREFSTM-EDGE-3: wide `/W [1 3 2]` field widths pack/round-trip correctly
/// through the public chain.
#[test]
fn xrefstm_edge_wide_w_widths() {
    let mut p = RawPdf::new();
    p.header();
    p.push_object(
        1,
        0,
        &Object::Dictionary(dict([("Type", name_obj("Catalog"))])),
    );
    p.push_object(2, 0, &Object::Integer(1212));
    let (o1, o2) = (p.offset_of(1), p.offset_of(2));

    let xref_off = p.pos();
    let widths = [1usize, 3, 2];
    let records = vec![
        (0u64, 0u64, 65535u64),
        (1, o1 as u64, 0),
        (1, o2 as u64, 0),
        (1, xref_off as u64, 0),
    ];
    let data = pack_xref_records(&records, widths);
    let xstream = xref_stream_object(&data, widths, None, 4, [("Root", rref(1, 0))], None);
    p.push_object(3, 0, &xstream);
    p.raw(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    let bytes = p.finish();

    let xref = xref_of(&bytes);
    if let Some(XrefEntry::Uncompressed { offset, .. }) = xref.get(1) {
        assert_eq!(offset, o1);
    } else {
        panic!("obj 1 not uncompressed");
    }

    let doc = open(&bytes);
    assert_eq!(
        doc.get_object(2, 0).unwrap().as_ref(),
        &Object::Integer(1212)
    );
}
