//! Object-parser unit tests — the `OBJ-*` catalog (M1a).
//!
//! Spec source of truth: ISO 32000-1 §7.3 (objects), §7.3.8 (streams), §7.3.10
//! (indirect objects), plus PRD §8.1 tolerance requirements.

use pdf_core::error::Error;
use pdf_core::object::parse::Parser;
use pdf_core::object::{Name, ObjRef, Object, StringKind};

fn parse_one(buf: &[u8]) -> Object {
    Parser::new(buf)
        .parse_object()
        .expect("parse_object failed")
}

#[test]
fn obj_keywords() {
    // OBJ-001: null / true / false.
    assert_eq!(parse_one(b"null"), Object::Null);
    assert_eq!(parse_one(b"true"), Object::Boolean(true));
    assert_eq!(parse_one(b"false"), Object::Boolean(false));
}

#[test]
fn obj_numbers() {
    // OBJ-002: integer and real.
    assert_eq!(parse_one(b"42"), Object::Integer(42));
    match parse_one(b"2.5") {
        Object::Real(r) => assert!((r - 2.5).abs() < 1e-9),
        other => panic!("expected real, got {other:?}"),
    }
}

#[test]
fn obj_strings() {
    // OBJ-003: literal and hex string carry bytes + kind.
    let lit = parse_one(b"(hi)");
    let s = lit.as_string().unwrap();
    assert_eq!(s.bytes, b"hi");
    assert_eq!(s.kind, StringKind::Literal);

    let hex = parse_one(b"<6869>");
    let s = hex.as_string().unwrap();
    assert_eq!(s.bytes, b"hi");
    assert_eq!(s.kind, StringKind::Hex);
}

#[test]
fn obj_name() {
    // OBJ-004: name decoded into `Name`.
    let n = parse_one(b"/Sub#74ype"); // #74 == 't'
    assert_eq!(n.as_name().unwrap(), &Name::new("Subtype"));
}

#[test]
fn obj_array() {
    // OBJ-005: empty + heterogeneous array.
    assert_eq!(parse_one(b"[]"), Object::Array(vec![]));
    assert_eq!(
        parse_one(b"[1 (two) /three true]"),
        Object::Array(vec![
            Object::Integer(1),
            Object::from(pdf_core::PdfString::literal(b"two".to_vec())),
            Object::Name(Name::new("three")),
            Object::Boolean(true),
        ])
    );
}

#[test]
fn obj_dict() {
    // OBJ-006: empty + nested dict.
    assert_eq!(parse_one(b"<<>>"), Object::Dictionary(Default::default()));
    let d = parse_one(b"<< /A 1 /B << /C 2 >> >>");
    let outer = d.as_dict().unwrap();
    assert_eq!(outer.get(&Name::new("A")), Some(&Object::Integer(1)));
    let inner = outer.get(&Name::new("B")).unwrap().as_dict().unwrap();
    assert_eq!(inner.get(&Name::new("C")), Some(&Object::Integer(2)));
}

#[test]
fn obj_reference() {
    // OBJ-007: `12 0 R` -> Reference.
    assert_eq!(parse_one(b"12 0 R"), Object::Reference(ObjRef::new(12, 0)));
}

#[test]
fn obj_r_is_reference_keyword() {
    // OBJ-008: with a non-int after the int, the int stands alone (R needs the
    // `int int R` shape). A bare `7` parses as Integer, not a name/keyword.
    assert_eq!(parse_one(b"7 /Name"), Object::Integer(7));
    // And `1 2 R` is unambiguously a reference, not three objects.
    assert_eq!(parse_one(b"1 2 R"), Object::Reference(ObjRef::new(1, 2)));
}

#[test]
fn obj_nested_array_dict_reference() {
    // OBJ-009: array containing a dict containing a reference.
    let o = parse_one(b"[ << /Kids [ 3 0 R ] >> ]");
    let arr = o.as_array().unwrap();
    let dict = arr[0].as_dict().unwrap();
    let kids = dict.get(&Name::new("Kids")).unwrap().as_array().unwrap();
    assert_eq!(kids[0], Object::Reference(ObjRef::new(3, 0)));
}

#[test]
fn obj_dict_duplicate_key_last_wins() {
    // OBJ-010: duplicate key -> last value wins (ISO 32000-1 §7.3.7).
    let d = parse_one(b"<< /K 1 /K 2 >>");
    assert_eq!(
        d.as_dict().unwrap().get(&Name::new("K")),
        Some(&Object::Integer(2))
    );
}

#[test]
fn obj_indirect_no_stream() {
    // OBJ-011: `N G obj <obj> endobj`.
    let (r, obj) = Parser::new(b"5 0 obj << /Type /Catalog >> endobj")
        .parse_indirect_object()
        .unwrap();
    assert_eq!(r, ObjRef::new(5, 0));
    assert_eq!(
        obj.as_dict().unwrap().get(&Name::new("Type")),
        Some(&Object::Name(Name::new("Catalog")))
    );
}

#[test]
fn obj_indirect_stream_with_length() {
    // OBJ-012: stream body read by an integer `/Length`.
    let buf = b"1 0 obj << /Length 5 >> stream\nhello\nendstream endobj";
    let (_, obj) = Parser::new(buf).parse_indirect_object().unwrap();
    let s = obj.as_stream().unwrap();
    assert_eq!(s.raw_bytes().as_ref(), b"hello");
    assert_eq!(s.dict.get(&Name::new("Length")), Some(&Object::Integer(5)));
}

#[test]
fn obj_indirect_stream_scan_when_length_missing() {
    // OBJ-013: no `/Length` -> scan to `endstream` (PRD §8.1 repair path).
    let buf = b"2 0 obj << /Filter /FlateDecode >> stream\nABCDEFG\nendstream endobj";
    let (_, obj) = Parser::new(buf).parse_indirect_object().unwrap();
    assert_eq!(obj.as_stream().unwrap().raw_bytes().as_ref(), b"ABCDEFG");
}

#[test]
fn obj_stream_eol_variants() {
    // OBJ-014: the single EOL after `stream` is consumed for CRLF and bare LF,
    // and not folded into the payload.
    let crlf = b"1 0 obj << /Length 3 >> stream\r\nabc\r\nendstream endobj";
    let (_, o1) = Parser::new(crlf).parse_indirect_object().unwrap();
    assert_eq!(o1.as_stream().unwrap().raw_bytes().as_ref(), b"abc");

    let lf = b"1 0 obj << /Length 3 >> stream\nabc\nendstream endobj";
    let (_, o2) = Parser::new(lf).parse_indirect_object().unwrap();
    assert_eq!(o2.as_stream().unwrap().raw_bytes().as_ref(), b"abc");
}

#[test]
fn obj_truncated_indirect_errors() {
    // OBJ-015: a truncated object is a typed error, never a panic.
    let r = Parser::new(b"3 0 obj << /A").parse_indirect_object();
    assert!(matches!(
        r,
        Err(Error::UnexpectedEof { .. }) | Err(Error::Syntax { .. })
    ));
}

#[test]
fn obj_unexpected_delimiter_errors() {
    // OBJ-016: a lone closing delimiter / odd dict token count is a typed error.
    assert!(matches!(
        Parser::new(b"]").parse_object(),
        Err(Error::Syntax { .. })
    ));
    assert!(matches!(
        Parser::new(b"<< /A >>").parse_object(),
        Err(Error::Syntax { .. })
    ));
}
