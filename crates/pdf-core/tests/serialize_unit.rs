//! Serializer unit tests — the `SER-*` catalog (M1a).
//!
//! Spec source of truth: ISO 32000-1 §7.3, PRD §9.2 (deterministic output).

use pdf_core::object::parse::Parser;
use pdf_core::object::{Dict, Name, ObjRef, Object, PdfString, StreamObj};
use pdf_core::serialize::{write_indirect, write_object};

fn s(obj: &Object) -> String {
    String::from_utf8(write_object(obj)).unwrap()
}

#[test]
fn ser_scalars() {
    // SER-001: scalars and canonical real formatting.
    assert_eq!(s(&Object::Null), "null");
    assert_eq!(s(&Object::Boolean(true)), "true");
    assert_eq!(s(&Object::Boolean(false)), "false");
    assert_eq!(s(&Object::Integer(-42)), "-42");
    assert_eq!(s(&Object::Real(3.5)), "3.5");
    assert_eq!(s(&Object::Real(4.0)), "4.0"); // trailing zeros trimmed to .0
    assert_eq!(s(&Object::Real(0.0)), "0.0");
    assert_eq!(s(&Object::Real(-0.0)), "0.0"); // -0 normalised
}

#[test]
fn ser_name_reencode() {
    // SER-002: delimiters / spaces in a name re-encode as #XX.
    assert_eq!(s(&Object::Name(Name::new("Type"))), "/Type");
    assert_eq!(
        s(&Object::Name(Name::from_decoded(b"Lime Green".to_vec()))),
        "/Lime#20Green"
    );
    assert_eq!(
        s(&Object::Name(Name::from_decoded(b"a#b".to_vec()))),
        "/a#23b"
    );
}

#[test]
fn ser_literal_string() {
    // SER-003: literal re-escaped to canonical form.
    let o = Object::from(PdfString::literal(b"a(b)\\c\n".to_vec()));
    assert_eq!(s(&o), r"(a\(b\)\\c\n)");
}

#[test]
fn ser_hex_string() {
    // SER-004: hex emitted uppercase between <>.
    let o = Object::from(PdfString::hex(vec![0x00, 0xAB, 0xFF]));
    assert_eq!(s(&o), "<00ABFF>");
}

#[test]
fn ser_array_and_dict_order() {
    // SER-005: array round-trips; dict keys in BTreeMap (sorted) order.
    assert_eq!(
        s(&Object::Array(vec![Object::Integer(1), Object::Null])),
        "[1 null]"
    );

    let mut d = Dict::new();
    d.insert(Name::new("B"), Object::Integer(2));
    d.insert(Name::new("A"), Object::Integer(1));
    // Sorted: A before B regardless of insertion order.
    assert_eq!(s(&Object::Dictionary(d)), "<</A 1/B 2>>");
}

#[test]
fn ser_stream_length() {
    // SER-006: stream emits a correct /Length for the payload.
    let mut dict = Dict::new();
    dict.insert(Name::new("Length"), Object::Integer(999)); // stale, must be fixed
    let st = StreamObj::new_encoded(dict, b"hello".to_vec());
    let out = String::from_utf8(write_object(&Object::Stream(st))).unwrap();
    assert!(out.contains("/Length 5"), "got: {out}");
    assert!(out.contains("stream\nhello\nendstream"), "got: {out}");
    assert!(!out.contains("999"), "stale length leaked: {out}");
}

#[test]
fn ser_indirect_wrapper() {
    // SER-007: write_indirect emits `N G obj … endobj`.
    let out = String::from_utf8(write_indirect(ObjRef::new(7, 0), &Object::Integer(42))).unwrap();
    assert_eq!(out, "7 0 obj\n42\nendobj\n");

    // And it round-trips through the parser.
    let (r, obj) = Parser::new(out.as_bytes()).parse_indirect_object().unwrap();
    assert_eq!(r, ObjRef::new(7, 0));
    assert_eq!(obj, Object::Integer(42));
}
