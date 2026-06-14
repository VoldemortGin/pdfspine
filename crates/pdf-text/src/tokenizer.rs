//! Content-stream tokenizer (M2b, PRD §8.6.2).
//!
//! Reuses the `pdf-core` [`Lexer`] for operands (numbers, strings, names,
//! arrays, dicts) and surfaces bare keywords as **operators**. Content streams
//! are *not* a sequence of indirect objects — operands precede an operator in
//! postfix order — so this is a thin layer that groups lexer tokens into
//! `(operands, operator)` events, plus a dedicated inline-image path.
//!
//! The tokenizer is **total**: arbitrary / truncated bytes never panic. On a
//! lexer error it skips one byte and resyncs (PRD §8.1).

use pdf_core::lexer::{Keyword, Lexer, Token};
use pdf_core::Object;

/// One parsed content event, in postfix order.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// An operand pushed onto the operand stack (number/string/name/array/dict).
    Operand(Object),
    /// An operator (e.g. `Tj`, `cm`, `BT`) with its mnemonic bytes.
    Operator(Vec<u8>),
    /// An inline image: the parsed parameter dict (key/value `Object`s) plus the
    /// raw, **undecoded** image body bytes between `ID` and `EI`.
    InlineImage { params: Object, data: Vec<u8> },
}

/// Splits a decoded content stream into a flat list of [`Event`]s.
///
/// Operands accumulate as `Event::Operand`; a keyword becomes
/// `Event::Operator` (the interpreter pops the operands it needs). `BI` triggers
/// the inline-image path, which consumes through a robust `EI` scan.
#[must_use]
pub fn tokenize(content: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut lexer = Lexer::new(content);
    loop {
        let before = lexer.offset();
        let tok = match lexer.next_token() {
            Ok(t) => t,
            Err(_) => {
                // Resync: advance one byte past the failed position and retry.
                let next = lexer.offset().max(before).saturating_add(1);
                if next >= content.len() {
                    break;
                }
                lexer.seek(next);
                continue;
            }
        };
        match tok {
            Token::Eof => break,
            Token::Integer(i) => events.push(Event::Operand(Object::Integer(i))),
            Token::Real(r) => events.push(Event::Operand(Object::Real(r))),
            Token::LiteralString(b) | Token::HexString(b) => {
                events.push(Event::Operand(Object::String(
                    pdf_core::PdfString::literal(b),
                )));
            }
            Token::Name(n) => events.push(Event::Operand(Object::Name(name_from(n)))),
            Token::ArrayOpen => {
                let arr = collect_array(&mut lexer);
                events.push(Event::Operand(Object::Array(arr)));
            }
            Token::DictOpen => {
                let d = collect_dict(&mut lexer);
                events.push(Event::Operand(Object::Dictionary(d)));
            }
            // Stray closers: ignore (resync).
            Token::ArrayClose | Token::DictClose => {}
            Token::Keyword(k) => match k {
                Keyword::True => events.push(Event::Operand(Object::Boolean(true))),
                Keyword::False => events.push(Event::Operand(Object::Boolean(false))),
                Keyword::Null => events.push(Event::Operand(Object::Null)),
                Keyword::Other(name) if name == b"BI" => {
                    let img = parse_inline_image(&mut lexer);
                    events.push(img);
                }
                Keyword::Other(name) => events.push(Event::Operator(name)),
                // Structure keywords have no meaning in a content stream;
                // surface their bytes as operators so an interpreter can skip.
                other => events.push(Event::Operator(keyword_bytes(&other))),
            },
        }
    }
    events
}

/// Collects array elements until `]` / EOF (operands only; nested arrays/dicts
/// supported). Tolerant: stray operators inside an array are dropped.
fn collect_array(lexer: &mut Lexer) -> Vec<Object> {
    let mut out = Vec::new();
    loop {
        let before = lexer.offset();
        let tok = match lexer.next_token() {
            Ok(t) => t,
            Err(_) => {
                let next = lexer.offset().max(before).saturating_add(1);
                lexer.seek(next);
                if next >= lexer.buffer().len() {
                    break;
                }
                continue;
            }
        };
        match tok {
            Token::ArrayClose | Token::Eof => break,
            Token::Integer(i) => out.push(Object::Integer(i)),
            Token::Real(r) => out.push(Object::Real(r)),
            Token::LiteralString(b) | Token::HexString(b) => {
                out.push(Object::String(pdf_core::PdfString::literal(b)))
            }
            Token::Name(n) => out.push(Object::Name(name_from(n))),
            Token::ArrayOpen => out.push(Object::Array(collect_array(lexer))),
            Token::DictOpen => out.push(Object::Dictionary(collect_dict(lexer))),
            Token::Keyword(Keyword::True) => out.push(Object::Boolean(true)),
            Token::Keyword(Keyword::False) => out.push(Object::Boolean(false)),
            Token::Keyword(Keyword::Null) => out.push(Object::Null),
            // Stray closer / operator inside an array: ignore.
            _ => {}
        }
    }
    out
}

/// Collects a `<< … >>` dictionary's key/value pairs until `>>` / EOF.
fn collect_dict(lexer: &mut Lexer) -> pdf_core::Dict {
    let mut d = pdf_core::Dict::new();
    loop {
        // A key must be a name.
        let key = loop {
            let before = lexer.offset();
            match lexer.next_token() {
                Ok(Token::Name(n)) => break Some(name_from(n)),
                Ok(Token::DictClose) | Ok(Token::Eof) => break None,
                Ok(_) => {} // skip non-name garbage until a name or close
                Err(_) => {
                    let next = lexer.offset().max(before).saturating_add(1);
                    lexer.seek(next);
                    if next >= lexer.buffer().len() {
                        break None;
                    }
                }
            }
        };
        let Some(key) = key else { break };
        // The value is one object.
        let Some(val) = read_value(lexer) else { break };
        d.insert(key, val);
    }
    d
}

/// Reads a single operand value (used for dict values).
fn read_value(lexer: &mut Lexer) -> Option<Object> {
    loop {
        let before = lexer.offset();
        let tok = match lexer.next_token() {
            Ok(t) => t,
            Err(_) => {
                let next = lexer.offset().max(before).saturating_add(1);
                lexer.seek(next);
                if next >= lexer.buffer().len() {
                    return None;
                }
                continue;
            }
        };
        return Some(match tok {
            Token::Integer(i) => Object::Integer(i),
            Token::Real(r) => Object::Real(r),
            Token::LiteralString(b) | Token::HexString(b) => {
                Object::String(pdf_core::PdfString::literal(b))
            }
            Token::Name(n) => Object::Name(name_from(n)),
            Token::ArrayOpen => Object::Array(collect_array(lexer)),
            Token::DictOpen => Object::Dictionary(collect_dict(lexer)),
            Token::Keyword(Keyword::True) => Object::Boolean(true),
            Token::Keyword(Keyword::False) => Object::Boolean(false),
            Token::Keyword(Keyword::Null) => Object::Null,
            Token::DictClose | Token::ArrayClose | Token::Eof => return None,
            Token::Keyword(_) => Object::Null,
        });
    }
}

/// Parses an inline image starting just after the `BI` keyword: a parameter
/// dict (key/value pairs) terminated by `ID`, then the raw image body up to a
/// robustly-located `EI`. The lexer is left positioned just past `EI`.
fn parse_inline_image(lexer: &mut Lexer) -> Event {
    // 1. Parameter dictionary: name/value pairs until the `ID` keyword.
    let mut params = pdf_core::Dict::new();
    loop {
        let before = lexer.offset();
        let tok = match lexer.next_token() {
            Ok(t) => t,
            Err(_) => {
                let next = lexer.offset().max(before).saturating_add(1);
                lexer.seek(next);
                if next >= lexer.buffer().len() {
                    return Event::InlineImage {
                        params: Object::Dictionary(params),
                        data: Vec::new(),
                    };
                }
                continue;
            }
        };
        match tok {
            Token::Keyword(Keyword::Other(k)) if k == b"ID" => break,
            Token::Eof => {
                return Event::InlineImage {
                    params: Object::Dictionary(params),
                    data: Vec::new(),
                };
            }
            Token::Name(n) => {
                let key = name_from(n);
                let val = read_value(lexer).unwrap_or(Object::Null);
                params.insert(key, val);
            }
            // Anything else before `ID`: tolerate / skip.
            _ => {}
        }
    }

    // 2. The image body: exactly one whitespace byte follows `ID`, then raw
    //    bytes until `EI` (a delimiter-bounded `EI` token). Scan robustly.
    let buf = lexer.buffer();
    let mut pos = lexer.offset();
    // Skip the single whitespace separator after `ID` (per spec).
    if pos < buf.len() && pdf_core::lexer::is_whitespace(buf[pos]) {
        pos += 1;
    }
    let data_start = pos;
    let (data_end, ei_end) = find_ei(buf, data_start);
    let data = buf[data_start..data_end].to_vec();
    lexer.seek(ei_end);
    Event::InlineImage {
        params: Object::Dictionary(params),
        data,
    }
}

/// Locates the `EI` terminator of an inline image body, robust to binary data
/// that may contain the bytes `EI`. Returns `(data_end, ei_end)` — the end of
/// the image body and the offset just past the `EI` token.
///
/// A valid `EI` is preceded by whitespace and followed by whitespace / a
/// delimiter / EOF, so `EI` bytes embedded in binary data (rarely flanked just
/// so) are skipped. Falls back to the end of the buffer when none is found.
fn find_ei(buf: &[u8], start: usize) -> (usize, usize) {
    let mut i = start;
    while i + 1 < buf.len() {
        if buf[i] == b'E' && buf[i + 1] == b'I' {
            let prev_ok = i == start || pdf_core::lexer::is_whitespace(buf[i - 1]);
            let after = i + 2;
            let next_ok = after >= buf.len()
                || pdf_core::lexer::is_whitespace(buf[after])
                || pdf_core::lexer::is_delimiter(buf[after]);
            if prev_ok && next_ok {
                // Trim the single whitespace byte that precedes a well-formed EI.
                let data_end = if i > start && pdf_core::lexer::is_whitespace(buf[i - 1]) {
                    i - 1
                } else {
                    i
                };
                return (data_end, after);
            }
        }
        i += 1;
    }
    // No terminator: consume to EOF.
    (buf.len(), buf.len())
}

/// Builds a `Name` from decoded name bytes (lossy UTF-8; names are ASCII in
/// practice — non-UTF-8 bytes are replaced so the operand is still usable).
fn name_from(bytes: Vec<u8>) -> pdf_core::Name {
    pdf_core::Name::new(String::from_utf8_lossy(&bytes).as_ref())
}

/// The raw mnemonic bytes for a structure keyword (only reached for stray
/// `obj`/`R`/etc. in a content stream — surfaced so the interpreter can skip).
fn keyword_bytes(k: &Keyword) -> Vec<u8> {
    match k {
        Keyword::Obj => b"obj".to_vec(),
        Keyword::EndObj => b"endobj".to_vec(),
        Keyword::Stream => b"stream".to_vec(),
        Keyword::EndStream => b"endstream".to_vec(),
        Keyword::R => b"R".to_vec(),
        Keyword::Xref => b"xref".to_vec(),
        Keyword::Trailer => b"trailer".to_vec(),
        Keyword::StartXref => b"startxref".to_vec(),
        Keyword::Other(o) => o.clone(),
        Keyword::True => b"true".to_vec(),
        Keyword::False => b"false".to_vec(),
        Keyword::Null => b"null".to_vec(),
    }
}
