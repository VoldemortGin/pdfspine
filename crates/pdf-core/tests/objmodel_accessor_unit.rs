//! OBJACC-* / NAME-* / STREAMDATA-* — object-model accessor/predicate, Name, and StreamData coverage. ISO 32000-1 §7.3.

use bytes::Bytes;

use pdf_core::Limits;
use pdf_core::{DecodeOutcome, Dict, Name, ObjRef, Object, PdfString, StreamData, StreamObj};

// --- OBJACC: Object predicates & accessors --------------------------------

#[test]
fn objacc_is_null_true_for_null() {
    assert!(Object::Null.is_null());
}

#[test]
fn objacc_is_null_false_for_non_null() {
    assert!(!Object::Integer(7).is_null());
    assert!(!Object::Boolean(false).is_null());
}

#[test]
fn objacc_as_bool_some_for_boolean() {
    assert_eq!(Object::Boolean(true).as_bool(), Some(true));
    assert_eq!(Object::Boolean(false).as_bool(), Some(false));
}

#[test]
fn objacc_as_bool_none_for_non_boolean() {
    assert_eq!(Object::Integer(1).as_bool(), None);
}

#[test]
fn objacc_as_i64_some_for_integer() {
    assert_eq!(Object::Integer(-42).as_i64(), Some(-42));
}

#[test]
fn objacc_as_i64_none_for_non_integer() {
    assert_eq!(Object::Real(1.5).as_i64(), None);
}

#[test]
fn objacc_as_f64_some_for_real() {
    assert_eq!(Object::Real(2.5).as_f64(), Some(2.5));
}

#[test]
fn objacc_as_f64_some_for_integer_widened() {
    // Integers are accepted by as_f64 and widened to f64.
    assert_eq!(Object::Integer(3).as_f64(), Some(3.0));
}

#[test]
fn objacc_as_f64_none_for_boolean() {
    assert_eq!(Object::Boolean(true).as_f64(), None);
}

#[test]
fn objacc_as_string_some_for_string() {
    let s = PdfString::literal(b"hi".to_vec());
    let obj = Object::String(s.clone());
    assert_eq!(obj.as_string(), Some(&s));
}

#[test]
fn objacc_as_string_none_for_non_string() {
    assert!(Object::Integer(0).as_string().is_none());
}

#[test]
fn objacc_as_name_some_for_name() {
    let n = Name::new("Subtype");
    let obj = Object::Name(n.clone());
    assert_eq!(obj.as_name(), Some(&n));
}

#[test]
fn objacc_as_name_none_for_non_name() {
    assert!(Object::Null.as_name().is_none());
}

#[test]
fn objacc_as_array_some_for_array() {
    let arr = vec![Object::Integer(1), Object::Integer(2)];
    let obj = Object::Array(arr.clone());
    assert_eq!(obj.as_array(), Some(arr.as_slice()));
}

#[test]
fn objacc_as_array_none_for_non_array() {
    assert!(Object::Null.as_array().is_none());
}

#[test]
fn objacc_as_dict_some_for_dictionary() {
    let mut d = Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("Catalog")));
    let obj = Object::Dictionary(d.clone());
    assert_eq!(obj.as_dict(), Some(&d));
}

#[test]
fn objacc_as_dict_some_for_stream_yields_stream_dict() {
    // A stream's dict is addressable as a dictionary via as_dict().
    let mut d = Dict::new();
    d.insert(Name::new("Length"), Object::Integer(5));
    let stream = StreamObj::new_encoded(d.clone(), b"hello".to_vec());
    let obj = Object::Stream(stream);
    assert_eq!(obj.as_dict(), Some(&d));
}

#[test]
fn objacc_as_dict_none_for_integer() {
    assert!(Object::Integer(0).as_dict().is_none());
}

#[test]
fn objacc_as_stream_some_for_stream() {
    let stream = StreamObj::new_encoded(Dict::new(), b"x".to_vec());
    let obj = Object::Stream(stream.clone());
    assert_eq!(obj.as_stream(), Some(&stream));
}

#[test]
fn objacc_as_stream_none_for_dictionary() {
    // A plain dictionary is not a stream.
    assert!(Object::Dictionary(Dict::new()).as_stream().is_none());
}

#[test]
fn objacc_as_reference_some_for_reference() {
    let r = ObjRef::new(12, 3);
    assert_eq!(Object::Reference(r).as_reference(), Some(r));
}

#[test]
fn objacc_as_reference_none_for_non_reference() {
    assert!(Object::Integer(12).as_reference().is_none());
}

// --- OBJACC: From impls into Object ---------------------------------------

#[test]
fn objacc_from_bool() {
    assert_eq!(Object::from(true), Object::Boolean(true));
}

#[test]
fn objacc_from_i64() {
    assert_eq!(Object::from(9_i64), Object::Integer(9));
}

#[test]
fn objacc_from_i32_widens_to_integer() {
    assert_eq!(Object::from(5_i32), Object::Integer(5));
}

#[test]
fn objacc_from_f64() {
    assert_eq!(Object::from(1.25_f64), Object::Real(1.25));
}

#[test]
fn objacc_from_pdfstring() {
    let s = PdfString::literal(b"abc".to_vec());
    assert_eq!(Object::from(s.clone()), Object::String(s));
}

#[test]
fn objacc_from_name() {
    let n = Name::new("Type");
    assert_eq!(Object::from(n.clone()), Object::Name(n));
}

#[test]
fn objacc_from_vec_object() {
    let arr = vec![Object::Null, Object::Boolean(true)];
    assert_eq!(Object::from(arr.clone()), Object::Array(arr));
}

#[test]
fn objacc_from_dict() {
    let mut d = Dict::new();
    d.insert(Name::new("K"), Object::Integer(1));
    assert_eq!(Object::from(d.clone()), Object::Dictionary(d));
}

#[test]
fn objacc_from_streamobj() {
    let stream = StreamObj::new_encoded(Dict::new(), b"s".to_vec());
    assert_eq!(Object::from(stream.clone()), Object::Stream(stream));
}

#[test]
fn objacc_from_objref() {
    let r = ObjRef::new(1, 0);
    assert_eq!(Object::from(r), Object::Reference(r));
}

// --- OBJACC: ObjRef::new const fn -----------------------------------------

#[test]
fn objacc_objref_new_records_num_and_gen() {
    let r = ObjRef::new(42, 7);
    assert_eq!(r.num, 42);
    assert_eq!(r.gen, 7);
}

// --- NAME: constructors, accessors, predicates ----------------------------

#[test]
fn name_new_and_as_bytes_roundtrip() {
    let n = Name::new("Subtype");
    assert_eq!(n.as_bytes(), b"Subtype");
}

#[test]
fn name_from_decoded_and_as_bytes_roundtrip() {
    let n = Name::from_decoded(b"Pages".to_vec());
    assert_eq!(n.as_bytes(), b"Pages");
}

#[test]
fn name_as_str_some_for_valid_utf8() {
    let n = Name::new("Catalog");
    assert_eq!(n.as_str(), Some("Catalog"));
}

#[test]
fn name_as_str_none_for_invalid_utf8() {
    let n = Name::from_decoded(vec![0xff, 0xfe]);
    assert_eq!(n.as_str(), None);
}

#[test]
fn name_is_empty_true_for_empty_new() {
    assert!(Name::new("").is_empty());
}

#[test]
fn name_is_empty_true_for_empty_from_decoded() {
    assert!(Name::from_decoded(Vec::new()).is_empty());
}

#[test]
fn name_is_empty_false_for_non_empty() {
    assert!(!Name::new("Type").is_empty());
}

#[test]
fn name_from_str_ref() {
    let n: Name = Name::from("Type");
    assert_eq!(n.as_bytes(), b"Type");
}

#[test]
fn name_from_string() {
    let n: Name = Name::from(String::from("Type"));
    assert_eq!(n.as_bytes(), b"Type");
}

#[test]
fn name_debug_valid_utf8_arm() {
    let n = Name::new("Subtype");
    let s = format!("{n:?}");
    assert_eq!(s, "Name(/Subtype)");
}

#[test]
fn name_debug_invalid_utf8_arm() {
    let n = Name::from_decoded(vec![0xff, 0xfe]);
    let s = format!("{n:?}");
    // Invalid UTF-8 routes through the `{:?}` byte path, not the `/{s}` path.
    assert!(s.starts_with("Name(/["));
    assert!(s.contains("255"));
    assert!(s.contains("254"));
}

#[test]
fn name_ordering_and_equality() {
    assert!(Name::new("A") < Name::new("B"));
    assert_eq!(Name::new("X"), Name::new("X"));
}

// --- STREAMDATA: owned_bytes / bytes / len / is_empty ---------------------

#[test]
fn streamdata_owned_bytes_some_for_encoded() {
    let data = StreamData::Encoded(Bytes::from_static(b"abc"));
    assert_eq!(data.owned_bytes().map(|b| b.as_ref()), Some(&b"abc"[..]));
}

#[test]
fn streamdata_owned_bytes_some_for_decoded() {
    let data = StreamData::Decoded(Bytes::from_static(b"xyz"));
    assert_eq!(data.owned_bytes().map(|b| b.as_ref()), Some(&b"xyz"[..]));
}

#[test]
fn streamdata_owned_bytes_none_for_raw() {
    let data = StreamData::Raw { offset: 0, len: 4 };
    assert!(data.owned_bytes().is_none());
}

#[test]
fn streamdata_bytes_returns_for_encoded() {
    let data = StreamData::Encoded(Bytes::from_static(b"abc"));
    assert_eq!(data.bytes().as_ref(), b"abc");
}

#[test]
fn streamdata_bytes_returns_for_decoded() {
    let data = StreamData::Decoded(Bytes::from_static(b"xyz"));
    assert_eq!(data.bytes().as_ref(), b"xyz");
}

#[test]
#[should_panic(expected = "Raw")]
fn streamdata_bytes_panics_for_raw() {
    let data = StreamData::Raw { offset: 0, len: 0 };
    let _ = data.bytes();
}

#[test]
fn streamdata_len_for_raw_returns_recorded_len() {
    let data = StreamData::Raw { offset: 10, len: 7 };
    assert_eq!(data.len(), 7);
}

#[test]
fn streamdata_len_for_encoded_returns_byte_length() {
    let data = StreamData::Encoded(Bytes::from_static(b"abcd"));
    assert_eq!(data.len(), 4);
}

#[test]
fn streamdata_len_for_decoded_returns_byte_length() {
    let data = StreamData::Decoded(Bytes::from_static(b"abcde"));
    assert_eq!(data.len(), 5);
}

#[test]
fn streamdata_is_empty_true_for_empty_encoded() {
    let data = StreamData::Encoded(Bytes::new());
    assert!(data.is_empty());
}

#[test]
fn streamdata_is_empty_false_for_non_empty() {
    let data = StreamData::Encoded(Bytes::from_static(b"a"));
    assert!(!data.is_empty());
}

// --- STREAMOBJ: constructors, raw_bytes, decode, decoded ------------------

#[test]
fn streamobj_new_encoded_holds_encoded_payload_and_dict() {
    let mut d = Dict::new();
    d.insert(Name::new("Length"), Object::Integer(3));
    let stream = StreamObj::new_encoded(d.clone(), b"abc".to_vec());
    assert_eq!(stream.dict, d);
    match &stream.data {
        StreamData::Encoded(b) => assert_eq!(b.as_ref(), b"abc"),
        other => panic!("expected Encoded, got {other:?}"),
    }
}

#[test]
fn streamobj_raw_bytes_returns_for_encoded() {
    let stream = StreamObj::new_encoded(Dict::new(), b"hello".to_vec());
    assert_eq!(stream.raw_bytes().as_ref(), b"hello");
}

#[test]
#[should_panic(expected = "Raw")]
fn streamobj_raw_bytes_panics_for_raw() {
    let stream = StreamObj {
        dict: Dict::new(),
        data: StreamData::Raw { offset: 0, len: 0 },
    };
    let _ = stream.raw_bytes();
}

#[test]
fn streamobj_decode_decoded_payload_returns_bytes_verbatim() {
    let stream = StreamObj {
        dict: Dict::new(),
        data: StreamData::Decoded(Bytes::from_static(b"verbatim")),
    };
    let outcome = stream.decode(&Limits::unbounded_decode()).unwrap();
    match outcome {
        DecodeOutcome::Decoded(bytes) => assert_eq!(bytes, b"verbatim".to_vec()),
        other => panic!("expected Decoded, got {other:?}"),
    }
}

#[test]
fn streamobj_decode_encoded_no_filter_returns_bytes_as_is() {
    // No /Filter entry means decode passes the bytes straight through.
    let stream = StreamObj::new_encoded(Dict::new(), b"hello".to_vec());
    let outcome = stream.decode(&Limits::unbounded_decode()).unwrap();
    match outcome {
        DecodeOutcome::Decoded(bytes) => assert_eq!(bytes, b"hello".to_vec()),
        other => panic!("expected Decoded, got {other:?}"),
    }
}

#[test]
fn streamobj_decode_raw_payload_is_unsupported_error() {
    let stream = StreamObj {
        dict: Dict::new(),
        data: StreamData::Raw { offset: 0, len: 0 },
    };
    let err = stream.decode(&Limits::unbounded_decode()).unwrap_err();
    assert!(matches!(err, pdf_core::Error::Unsupported(_)));
}

#[test]
fn streamobj_decoded_encoded_no_filter_yields_decoded_variant() {
    let stream = StreamObj::new_encoded(Dict::new(), b"hello".to_vec());
    let decoded = stream.decoded(&Limits::unbounded_decode()).unwrap();
    match &decoded.data {
        StreamData::Decoded(b) => assert_eq!(b.as_ref(), b"hello"),
        other => panic!("expected Decoded, got {other:?}"),
    }
    assert_eq!(
        decoded.data.owned_bytes().map(|b| b.as_ref()),
        Some(&b"hello"[..])
    );
}
