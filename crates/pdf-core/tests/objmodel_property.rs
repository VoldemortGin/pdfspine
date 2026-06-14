//! Object-model property tests — `SER-PROP-001` and `LEXER-PROP-001` (M1a).
//!
//! Spec source of truth: PRD §10.7 (round-trip property) and §8.1 (lexer never
//! panics). Generators are ours.

use pdf_core::lexer::{Lexer, Token};
use pdf_core::object::parse::Parser;
use pdf_core::object::{Dict, Name, ObjRef, Object, PdfString, StreamObj, StringKind};
use pdf_core::serialize::write_object;
use proptest::prelude::*;

// --- canonical real formatting, mirroring the serializer ------------------

/// Mirrors `serialize::format_real`: 6-decimal fixed form, trailing zeros and
/// point trimmed, `-0` normalised. `normalize` routes every real through this
/// so that `parse(serialize(o))` is comparable to `normalize(o)`.
fn canonical_real(r: f64) -> f64 {
    if r == 0.0 {
        return 0.0;
    }
    let mut s = format!("{r:.6}");
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }
    s.parse::<f64>().unwrap()
}

/// Canonicalises an object into the form the parser will reproduce after the
/// serializer has run: reals rounded to 6 decimals, and the empty-name /
/// bytes-preserving identities that already hold for names and strings.
fn normalize(o: &Object) -> Object {
    match o {
        Object::Real(r) => Object::Real(canonical_real(*r)),
        Object::Array(items) => Object::Array(items.iter().map(normalize).collect()),
        Object::Dictionary(d) => Object::Dictionary(normalize_dict(d)),
        Object::Stream(s) => Object::Stream(StreamObj::new_encoded(
            // /Length is recomputed by the serializer; reflect that here.
            {
                let mut d = normalize_dict(&s.dict);
                d.insert(
                    Name::new("Length"),
                    Object::Integer(s.raw_bytes().len() as i64),
                );
                d
            },
            s.raw_bytes().clone(),
        )),
        other => other.clone(),
    }
}

fn normalize_dict(d: &Dict) -> Dict {
    d.iter().map(|(k, v)| (k.clone(), normalize(v))).collect()
}

// --- generators -----------------------------------------------------------

fn real() -> impl Strategy<Value = f64> {
    // Bounded, finite reals keep equality well-defined and avoid exponent
    // formatting that the canonical (non-scientific) writer does not emit.
    (-1.0e6..1.0e6f64).prop_map(canonical_real)
}

fn name() -> impl Strategy<Value = Name> {
    // Arbitrary printable-ish byte names (incl. delimiters/space) to exercise
    // #XX re-encoding round-trips.
    proptest::collection::vec(0x21u8..0x7f, 0..8).prop_map(Name::from_decoded)
}

fn pdf_string() -> impl Strategy<Value = PdfString> {
    let bytes = proptest::collection::vec(any::<u8>(), 0..16);
    (
        bytes,
        prop_oneof![Just(StringKind::Literal), Just(StringKind::Hex)],
    )
        .prop_map(|(b, kind)| PdfString { bytes: b, kind })
}

fn leaf() -> impl Strategy<Value = Object> {
    prop_oneof![
        Just(Object::Null),
        any::<bool>().prop_map(Object::Boolean),
        any::<i64>().prop_map(Object::Integer),
        real().prop_map(Object::Real),
        pdf_string().prop_map(Object::String),
        name().prop_map(Object::Name),
        (any::<u32>(), any::<u16>()).prop_map(|(n, g)| Object::Reference(ObjRef::new(n, g))),
    ]
}

/// Recursive object generator (arrays + dicts of bounded depth). Streams are
/// covered separately because their `/Length` recompute changes the dict.
fn object() -> impl Strategy<Value = Object> {
    leaf().prop_recursive(3, 24, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(Object::Array),
            proptest::collection::vec((name(), inner), 0..6).prop_map(|kvs| {
                let mut d = Dict::new();
                for (k, v) in kvs {
                    d.insert(k, v);
                }
                Object::Dictionary(d)
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// SER-PROP-001: parse(serialize(o)) == normalize(o) for generated objects.
    #[test]
    fn ser_prop_roundtrip(o in object()) {
        let bytes = write_object(&o);
        let parsed = Parser::new(&bytes).parse_object().expect("re-parse failed");
        prop_assert_eq!(parsed, normalize(&o));
    }

    /// SER-PROP-001 (stream variant): streams round-trip with recomputed length.
    #[test]
    fn ser_prop_roundtrip_stream(
        dict in proptest::collection::vec((name(), leaf()), 0..4),
        body in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut d = Dict::new();
        for (k, v) in dict {
            d.insert(k, v);
        }
        let obj = Object::Stream(StreamObj::new_encoded(d, body));
        let bytes = write_object(&obj);
        let parsed = Parser::new(&bytes).parse_object().expect("re-parse failed");
        prop_assert_eq!(parsed, normalize(&obj));
    }

    /// LEXER-PROP-001: tokenizing arbitrary bytes never panics and terminates.
    #[test]
    fn lexer_prop_no_panic(data in proptest::collection::vec(any::<u8>(), 0..256)) {
        let mut lx = Lexer::new(&data);
        // Bound the loop generously; the lexer must always make progress to EOF
        // or return an error.
        for _ in 0..(data.len() * 4 + 16) {
            match lx.next_token() {
                Ok(Token::Eof) => break,
                Ok(_) => continue,
                Err(_) => break, // typed error is acceptable; the point is no panic
            }
        }
    }
}
