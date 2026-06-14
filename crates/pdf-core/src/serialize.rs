//! Deterministic [`Object`] → bytes serializer (ISO 32000-1 §7.3, PRD §9.2).
//!
//! Output is deterministic: dictionary keys are emitted in [`Dict`]
//! (`BTreeMap`) order, reals use a canonical fixed formatting, names re-encode
//! `#XX` for non-regular bytes, and streams carry a recomputed `/Length`. The
//! round-trip property `parse(serialize(o)) == normalize(o)` (catalog
//! `SER-PROP-001`) holds for any finite-real object.

use crate::object::{Dict, Name, ObjRef, Object, PdfString, StreamData, StreamObj, StringKind};

/// Serializes a single object to bytes (no indirect wrapper).
#[must_use]
pub fn write_object(obj: &Object) -> Vec<u8> {
    let mut out = Vec::new();
    write_object_into(&mut out, obj);
    out
}

/// Serializes an indirect object: `num gen obj <body> endobj\n`.
#[must_use]
pub fn write_indirect(r: ObjRef, obj: &Object) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("{} {} obj\n", r.num, r.gen).as_bytes());
    write_object_into(&mut out, obj);
    out.extend_from_slice(b"\nendobj\n");
    out
}

fn write_object_into(out: &mut Vec<u8>, obj: &Object) {
    match obj {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(true) => out.extend_from_slice(b"true"),
        Object::Boolean(false) => out.extend_from_slice(b"false"),
        Object::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Object::Real(r) => out.extend_from_slice(format_real(*r).as_bytes()),
        Object::String(s) => write_string(out, s),
        Object::Name(n) => write_name(out, n),
        Object::Array(items) => write_array(out, items),
        Object::Dictionary(d) => write_dict(out, d),
        Object::Stream(s) => write_stream(out, s),
        Object::Reference(r) => {
            out.extend_from_slice(format!("{} {} R", r.num, r.gen).as_bytes());
        }
    }
}

/// Canonical real formatting (ISO 32000-1 §7.3.3): no exponent, a leading `0`
/// before the point, trailing zeros trimmed but a `.0` always retained so the
/// value re-parses as a real (not an integer); `-0` normalised to `0.0`.
fn format_real(r: f64) -> String {
    if r == 0.0 {
        return "0.0".to_string();
    }
    // Use enough precision to round-trip, then trim trailing zeros while always
    // leaving at least one fractional digit.
    let mut s = format!("{r:.6}");
    debug_assert!(s.contains('.'));
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }
    s
}

fn write_string(out: &mut Vec<u8>, s: &PdfString) {
    match s.kind {
        StringKind::Hex => write_hex_string(out, &s.bytes),
        StringKind::Literal => write_literal_string(out, &s.bytes),
    }
}

fn write_literal_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'(');
    for &b in bytes {
        match b {
            b'(' => out.extend_from_slice(b"\\("),
            b')' => out.extend_from_slice(b"\\)"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0C => out.extend_from_slice(b"\\f"),
            // Printable ASCII passes through; everything else is octal-escaped
            // so the literal form survives any byte payload losslessly.
            0x20..=0x7E => out.push(b),
            other => {
                out.push(b'\\');
                out.push(b'0' + ((other >> 6) & 0x7));
                out.push(b'0' + ((other >> 3) & 0x7));
                out.push(b'0' + (other & 0x7));
            }
        }
    }
    out.push(b')');
}

fn write_hex_string(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(b'<');
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0xF) as usize]);
    }
    out.push(b'>');
}

fn write_name(out: &mut Vec<u8>, n: &Name) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(b'/');
    for &b in n.as_bytes() {
        // Regular, printable, non-`#` bytes pass through; everything else uses
        // a `#XX` escape (ISO 32000-1 §7.3.5).
        if b > 0x20 && b < 0x7F && b != b'#' && !is_name_delimiter(b) {
            out.push(b);
        } else {
            out.push(b'#');
            out.push(HEX[(b >> 4) as usize]);
            out.push(HEX[(b & 0xF) as usize]);
        }
    }
}

fn is_name_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn write_array(out: &mut Vec<u8>, items: &[Object]) {
    out.push(b'[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        write_object_into(out, item);
    }
    out.push(b']');
}

fn write_dict(out: &mut Vec<u8>, dict: &Dict) {
    out.extend_from_slice(b"<<");
    for (k, v) in dict {
        write_name(out, k);
        out.push(b' ');
        write_object_into(out, v);
    }
    out.extend_from_slice(b">>");
}

fn write_stream(out: &mut Vec<u8>, s: &StreamObj) {
    // Emit the dict with a correct `/Length` for the payload, overriding any
    // stale value in the source dict (ISO 32000-1 §7.3.8).
    let payload = match &s.data {
        StreamData::Encoded(b) | StreamData::Decoded(b) => b,
    };
    let mut dict = s.dict.clone();
    dict.insert(Name::new("Length"), Object::Integer(payload.len() as i64));
    write_dict(out, &dict);
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream");
}
