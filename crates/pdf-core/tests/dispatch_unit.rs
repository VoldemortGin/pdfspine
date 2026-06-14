//! Stream-decode dispatcher tests — the `DISPATCH-*` catalog (M1b).
//!
//! Spec source of truth: ISO 32000-1 §7.4.1 (filter chains), PRD §8.3
//! (abbreviations, parallel `/DecodeParms`, image-filter "leave encoded"
//! policy). Exercises [`pdf_core::filters::decode_stream`] plus the lazy
//! [`pdf_core::StreamObj::decoded`] production path (DISPATCH-010).

use pdf_core::filters::{ascii85, flate, lzw, DecodeOutcome};
use pdf_core::object::{Dict, Name, Object};
use pdf_core::{decode_stream, Limits, StreamData, StreamObj};

fn dict(entries: &[(&str, Object)]) -> Dict {
    entries
        .iter()
        .map(|(k, v)| (Name::new(k), v.clone()))
        .collect()
}

fn name(s: &str) -> Object {
    Object::Name(Name::new(s))
}

fn decoded(d: &Dict, raw: &[u8]) -> Vec<u8> {
    match decode_stream(d, raw, &Limits::unbounded_decode()).unwrap() {
        DecodeOutcome::Decoded(b) => b,
        other => panic!("expected Decoded, got {other:?}"),
    }
}

#[test]
fn dispatch_001_single_flate() {
    // DISPATCH-001: a single /Filter /FlateDecode decodes.
    let d = dict(&[("Filter", name("FlateDecode"))]);
    let raw = flate::encode(b"the quick brown fox");
    assert_eq!(decoded(&d, &raw), b"the quick brown fox");
}

#[test]
fn dispatch_002_no_filter_verbatim() {
    // DISPATCH-002: no /Filter → bytes returned verbatim.
    let d = Dict::new();
    assert_eq!(decoded(&d, b"raw bytes"), b"raw bytes");
    // An explicit /Filter null behaves the same.
    let dn = dict(&[("Filter", Object::Null)]);
    assert_eq!(decoded(&dn, b"raw bytes"), b"raw bytes");
}

#[test]
fn dispatch_003_chain_ascii85_then_flate() {
    // DISPATCH-003: [ASCII85Decode FlateDecode] applied left-to-right.
    let payload = b"chained filters payload \x00\x01\x02";
    let flated = flate::encode(payload); // inner filter output
    let a85 = ascii85::encode(&flated); // outer filter output (applied first on decode)
    let d = dict(&[(
        "Filter",
        Object::Array(vec![name("ASCII85Decode"), name("FlateDecode")]),
    )]);
    assert_eq!(decoded(&d, &a85), payload);
}

#[test]
fn dispatch_004_abbreviations() {
    // DISPATCH-004: inline-image abbreviations Fl / A85 / AHx / RL / LZW.
    let payload = b"abbreviated";
    let flated = flate::encode(payload);
    let a85 = ascii85::encode(&flated);
    let d = dict(&[("Filter", Object::Array(vec![name("A85"), name("Fl")]))]);
    assert_eq!(decoded(&d, &a85), payload);

    // LZW abbreviation.
    let dl = dict(&[("Filter", name("LZW"))]);
    let lz = lzw::encode(payload, true);
    assert_eq!(decoded(&dl, &lz), payload);
}

#[test]
fn dispatch_005_decodeparms_predictor() {
    // DISPATCH-005: a /DecodeParms predictor is applied to its filter's output.
    // Build: raw rows -> PNG-Up predict -> flate encode. Decode must flate then
    // unpredict back to the raw rows.
    use pdf_core::filters::predictor::{predict, PredictorParams};
    let raw: Vec<u8> = (0..40u8).collect(); // 8 rows x 5 cols, colors=1, bpc=8
    let p = PredictorParams {
        predictor: 12,
        colors: 1,
        bits_per_component: 8,
        columns: 5,
    };
    let predicted = predict(&raw, &p).unwrap();
    let stream = flate::encode(&predicted);

    let parms = dict(&[
        ("Predictor", Object::Integer(12)),
        ("Colors", Object::Integer(1)),
        ("BitsPerComponent", Object::Integer(8)),
        ("Columns", Object::Integer(5)),
    ]);
    let d = dict(&[
        ("Filter", name("FlateDecode")),
        ("DecodeParms", Object::Dictionary(parms)),
    ]);
    assert_eq!(decoded(&d, &stream), raw);
}

#[test]
fn dispatch_006_decodeparms_array_with_null() {
    // DISPATCH-006: /DecodeParms as an array with a null for the un-parametrized
    // filter, a dict for the predictor filter.
    use pdf_core::filters::predictor::{predict, PredictorParams};
    let raw: Vec<u8> = (0..24u8).collect(); // 4 rows x 6
    let p = PredictorParams {
        predictor: 12,
        colors: 1,
        bits_per_component: 8,
        columns: 6,
    };
    let predicted = predict(&raw, &p).unwrap();
    let flated = flate::encode(&predicted);
    let a85 = ascii85::encode(&flated);

    let parms = dict(&[
        ("Predictor", Object::Integer(12)),
        ("Columns", Object::Integer(6)),
    ]);
    let d = dict(&[
        (
            "Filter",
            Object::Array(vec![name("ASCII85Decode"), name("FlateDecode")]),
        ),
        (
            "DecodeParms",
            Object::Array(vec![Object::Null, Object::Dictionary(parms)]),
        ),
    ]);
    assert_eq!(decoded(&d, &a85), raw);
}

#[test]
fn dispatch_007_image_filter_leave_encoded() {
    // DISPATCH-007: an image-only filter is left encoded (not an error).
    let d = dict(&[("Filter", name("DCTDecode"))]);
    let raw = b"\xff\xd8\xff\xe0 jpeg-ish bytes";
    match decode_stream(&d, raw, &Limits::default()).unwrap() {
        DecodeOutcome::ImageEncoded { filter, bytes } => {
            assert_eq!(filter, "DCTDecode");
            assert_eq!(bytes, raw);
        }
        other => panic!("expected ImageEncoded, got {other:?}"),
    }
}

#[test]
fn dispatch_008_image_filter_midchain() {
    // DISPATCH-008: chain [ASCII85Decode DCTDecode] → decode A85, then leave the
    // DCT-encoded bytes as ImageEncoded.
    let jpeg = b"\xff\xd8 fake jpeg \x00\x10";
    let a85 = ascii85::encode(jpeg);
    let d = dict(&[(
        "Filter",
        Object::Array(vec![name("ASCII85Decode"), name("DCTDecode")]),
    )]);
    match decode_stream(&d, &a85, &Limits::default()).unwrap() {
        DecodeOutcome::ImageEncoded { filter, bytes } => {
            assert_eq!(filter, "DCTDecode");
            assert_eq!(bytes, jpeg); // A85 already peeled off
        }
        other => panic!("expected ImageEncoded, got {other:?}"),
    }
}

#[test]
fn dispatch_009_unknown_filter_errors() {
    // DISPATCH-009: an unrecognized filter name is a typed Err, not a panic.
    let d = dict(&[("Filter", name("NotARealFilter"))]);
    let err = decode_stream(&d, b"x", &Limits::default()).unwrap_err();
    assert_eq!(err.kind(), "filter");
}

#[test]
fn dispatch_010_streamobj_decoded_lazy() {
    // DISPATCH-010: StreamObj::decoded produces a StreamData::Decoded payload
    // lazily; the original stream keeps its Encoded bytes.
    let payload = b"lazy decode me";
    let raw = flate::encode(payload);
    let d = dict(&[("Filter", name("FlateDecode"))]);
    let stream = StreamObj::new_encoded(d, raw.clone());

    // Original is still Encoded.
    assert!(matches!(stream.data, StreamData::Encoded(_)));

    let dec = stream.decoded(&Limits::unbounded_decode()).unwrap();
    match &dec.data {
        StreamData::Decoded(b) => assert_eq!(&b[..], payload),
        other => panic!("expected Decoded, got {other:?}"),
    }
    // The original is untouched (lazy / non-mutating).
    assert!(matches!(stream.data, StreamData::Encoded(_)));

    // An image-filter stream is returned unchanged (still Encoded).
    let img = StreamObj::new_encoded(
        dict(&[("Filter", name("JPXDecode"))]),
        b"jpx bytes".to_vec(),
    );
    let img_dec = img.decoded(&Limits::default()).unwrap();
    assert!(matches!(img_dec.data, StreamData::Encoded(_)));
}
