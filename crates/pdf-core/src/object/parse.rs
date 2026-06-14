//! Object parsing — single objects and indirect objects (ISO 32000-1 §7.3).
//!
//! Built on top of [`crate::lexer::Lexer`]. The parser is the layer that knows
//! grammar (arrays, dicts, `N G R`, `N G obj … endobj`, streams). Like the
//! lexer it never panics on malformed input — it returns a typed [`Error`].

use bytes::Bytes;

use crate::error::{Error, Result};
use crate::lexer::{is_whitespace, Keyword, Lexer, Token};

use super::{Dict, Name, ObjRef, Object, PdfString, StreamObj};

/// A recursive-descent object parser over a [`Lexer`].
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    /// When set, the most recently parsed stream's body range (within the parse
    /// buffer) — captured for the lazy source-backed (`Raw`) path so the
    /// `DocumentStore` can record `(offset, len)` instead of copying the body.
    last_stream_body: Option<(usize, usize)>,
}

impl<'a> Parser<'a> {
    /// Creates a parser over `buf`.
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Parser {
            lexer: Lexer::new(buf),
            last_stream_body: None,
        }
    }

    /// Creates a parser from an existing lexer (sharing position).
    #[must_use]
    pub fn from_lexer(lexer: Lexer<'a>) -> Self {
        Parser {
            lexer,
            last_stream_body: None,
        }
    }

    /// The `(start, len)` byte range, **within the parse buffer**, of the most
    /// recently parsed stream's body — or `None` if the last parsed object was
    /// not a stream. Used by the `DocumentStore` to build a source-backed
    /// [`crate::object::StreamData::Raw`] payload (PRD §9.2).
    #[must_use]
    pub fn last_stream_body(&self) -> Option<(usize, usize)> {
        self.last_stream_body
    }

    /// The current byte offset.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.lexer.offset()
    }

    /// Parses a single object value at the current position.
    ///
    /// Handles the `N G R` and `N G obj` look-ahead: a bare integer is buffered
    /// and, if followed by `int R` / `int obj`, folded into a reference or
    /// indirect object.
    pub fn parse_object(&mut self) -> Result<Object> {
        let tok = self.lexer.next_token()?;
        self.parse_object_from(tok)
    }

    /// Parses an object given an already-read leading token.
    fn parse_object_from(&mut self, tok: Token) -> Result<Object> {
        match tok {
            Token::Integer(i) => self.parse_after_integer(i),
            Token::Real(r) => Ok(Object::Real(r)),
            Token::LiteralString(b) => Ok(Object::String(PdfString::literal(b))),
            Token::HexString(b) => Ok(Object::String(PdfString::hex(b))),
            Token::Name(b) => Ok(Object::Name(Name::from_decoded(b))),
            Token::ArrayOpen => self.parse_array(),
            Token::DictOpen => self.parse_dict_or_stream(),
            Token::Keyword(Keyword::True) => Ok(Object::Boolean(true)),
            Token::Keyword(Keyword::False) => Ok(Object::Boolean(false)),
            Token::Keyword(Keyword::Null) => Ok(Object::Null),
            Token::Eof => Err(Error::eof(self.lexer.offset())),
            Token::DictClose | Token::ArrayClose => Err(Error::syntax(
                self.lexer.offset(),
                "unexpected closing delimiter",
            )),
            Token::Keyword(_) => Err(Error::syntax(
                self.lexer.offset(),
                "unexpected keyword where object expected",
            )),
        }
    }

    /// After reading an integer, look ahead for `G R` (reference) or `G obj`
    /// (indirect object). Otherwise the integer stands alone.
    fn parse_after_integer(&mut self, first: i64) -> Result<Object> {
        // Snapshot so we can rewind if the look-ahead does not match.
        let after_first = self.lexer.offset();
        let lookahead = self.lexer.next_token()?;
        match lookahead {
            Token::Integer(gen) if first >= 0 && (0..=u16::MAX as i64).contains(&gen) => {
                let third = self.lexer.next_token()?;
                match third {
                    Token::Keyword(Keyword::R) => {
                        Ok(Object::Reference(ObjRef::new(first as u32, gen as u16)))
                    }
                    Token::Keyword(Keyword::Obj) => {
                        let r = ObjRef::new(first as u32, gen as u16);
                        self.parse_indirect_body(r).map(|(_, obj)| obj)
                    }
                    _ => {
                        // Not a ref/obj: the integer was a plain integer.
                        self.lexer.seek(after_first);
                        Ok(Object::Integer(first))
                    }
                }
            }
            _ => {
                self.lexer.seek(after_first);
                Ok(Object::Integer(first))
            }
        }
    }

    /// Parses an array `[ … ]` (the `[` already consumed).
    fn parse_array(&mut self) -> Result<Object> {
        let mut items = Vec::new();
        loop {
            let tok = self.lexer.next_token()?;
            match tok {
                Token::ArrayClose => return Ok(Object::Array(items)),
                Token::Eof => return Err(Error::eof(self.lexer.offset())),
                other => items.push(self.parse_object_from(other)?),
            }
        }
    }

    /// Parses `<< … >>` then, if immediately followed by `stream`, the stream
    /// body. The `<<` is already consumed.
    fn parse_dict_or_stream(&mut self) -> Result<Object> {
        let dict = self.parse_dict_body()?;

        // Look ahead for a `stream` keyword (PDF allows whitespace/comments
        // between >> and stream).
        let before = self.lexer.offset();
        let tok = self.lexer.next_token()?;
        if tok == Token::Keyword(Keyword::Stream) {
            let stream = self.parse_stream_body(dict)?;
            Ok(Object::Stream(stream))
        } else {
            self.lexer.seek(before);
            Ok(Object::Dictionary(dict))
        }
    }

    /// Parses dictionary entries up to and including `>>`.
    fn parse_dict_body(&mut self) -> Result<Dict> {
        let mut dict = Dict::new();
        loop {
            let key_tok = self.lexer.next_token()?;
            let key = match key_tok {
                Token::DictClose => return Ok(dict),
                Token::Name(b) => Name::from_decoded(b),
                Token::Eof => return Err(Error::eof(self.lexer.offset())),
                _ => {
                    return Err(Error::syntax(
                        self.lexer.offset(),
                        "dictionary key is not a name",
                    ))
                }
            };
            // Value must exist; a `>>` here is an odd token count.
            let val_tok = self.lexer.next_token()?;
            match val_tok {
                Token::DictClose => {
                    return Err(Error::syntax(
                        self.lexer.offset(),
                        "dictionary key without value (odd token count)",
                    ))
                }
                Token::Eof => return Err(Error::eof(self.lexer.offset())),
                other => {
                    let value = self.parse_object_from(other)?;
                    // Duplicate keys: last wins (ISO 32000-1 §7.3.7 / PRD §8.1).
                    dict.insert(key, value);
                }
            }
        }
    }

    /// Parses an indirect object body after `N G obj`, returning the ref and the
    /// contained object. Stops at `endobj` (tolerating its absence after a
    /// stream).
    fn parse_indirect_body(&mut self, r: ObjRef) -> Result<(ObjRef, Object)> {
        let obj = self.parse_object()?;
        // Consume an optional trailing `endobj`.
        let before = self.lexer.offset();
        match self.lexer.next_token()? {
            Token::Keyword(Keyword::EndObj) | Token::Eof => {}
            _ => self.lexer.seek(before),
        }
        Ok((r, obj))
    }

    /// Reads a stream body after the `stream` keyword (ISO 32000-1 §7.3.8).
    ///
    /// Exactly one EOL must follow `stream` (CRLF or bare LF; we also tolerate a
    /// bare CR). The body length is taken from an integer `/Length` in `dict`
    /// when present and sane, else found by scanning to `endstream`.
    fn parse_stream_body(&mut self, dict: Dict) -> Result<StreamObj> {
        let buf = self.lexer.buffer();
        let mut p = self.lexer.offset();

        // Skip the single EOL after `stream`. ISO requires CRLF or LF; tolerate
        // a lone CR and any incidental spaces some generators emit.
        // First skip spaces/tabs (lenient).
        while matches!(buf.get(p), Some(b' ') | Some(b'\t')) {
            p += 1;
        }
        match buf.get(p) {
            Some(b'\r') => {
                p += 1;
                if buf.get(p) == Some(&b'\n') {
                    p += 1;
                }
            }
            Some(b'\n') => p += 1,
            _ => {
                // No EOL after stream — malformed but recoverable; body starts
                // here.
            }
        }
        let body_start = p;

        // Determine the body length.
        let declared = dict.get(&Name::new("Length")).and_then(Object::as_i64);
        let (body_end, after) = match declared {
            Some(len) if len >= 0 && body_start + (len as usize) <= buf.len() => {
                let end = body_start + len as usize;
                // Validate that `endstream` follows (allowing whitespace). If it
                // does not, fall back to scanning (the `/Length` lied — common).
                if endstream_follows(buf, end) {
                    (end, end)
                } else {
                    scan_to_endstream(buf, body_start)?
                }
            }
            _ => scan_to_endstream(buf, body_start)?,
        };

        let data = Bytes::copy_from_slice(&buf[body_start..body_end]);
        // Record the body's location (within the parse buffer) for the lazy
        // source-backed `Raw` path (PRD §9.2).
        self.last_stream_body = Some((body_start, body_end.saturating_sub(body_start)));

        // Advance the lexer past the body and consume `endstream`.
        self.lexer.seek(after);
        let before = self.lexer.offset();
        match self.lexer.next_token()? {
            Token::Keyword(Keyword::EndStream) => {}
            _ => self.lexer.seek(before),
        }
        Ok(StreamObj::new_encoded(dict, data))
    }

    /// Parses a complete indirect object `N G obj … endobj` from the current
    /// position. Public entry point for callers that have an object's offset.
    pub fn parse_indirect_object(&mut self) -> Result<(ObjRef, Object)> {
        let n = match self.lexer.next_token()? {
            Token::Integer(n) if n >= 0 => n as u32,
            Token::Eof => return Err(Error::eof(self.lexer.offset())),
            _ => return Err(Error::syntax(self.lexer.offset(), "expected object number")),
        };
        let g = match self.lexer.next_token()? {
            Token::Integer(g) if (0..=u16::MAX as i64).contains(&g) => g as u16,
            _ => {
                return Err(Error::syntax(
                    self.lexer.offset(),
                    "expected generation number",
                ))
            }
        };
        match self.lexer.next_token()? {
            Token::Keyword(Keyword::Obj) => {}
            _ => return Err(Error::syntax(self.lexer.offset(), "expected 'obj' keyword")),
        }
        let r = ObjRef::new(n, g);
        self.parse_indirect_body(r)
    }
}

/// Returns `true` if, after skipping whitespace from `pos`, the bytes spell
/// `endstream`.
fn endstream_follows(buf: &[u8], mut pos: usize) -> bool {
    while matches!(buf.get(pos), Some(&b) if is_whitespace(b)) {
        pos += 1;
    }
    buf.get(pos..pos + 9) == Some(b"endstream")
}

/// Scans forward from `body_start` for the `endstream` keyword, trimming the one
/// EOL that conventionally precedes it. Returns `(body_end, resume_pos)` where
/// `resume_pos` points at `endstream`.
fn scan_to_endstream(buf: &[u8], body_start: usize) -> Result<(usize, usize)> {
    let needle = b"endstream";
    let mut i = body_start;
    while i + needle.len() <= buf.len() {
        if &buf[i..i + needle.len()] == needle {
            // Trim a single trailing EOL between body and `endstream`.
            let mut end = i;
            if end > body_start && buf[end - 1] == b'\n' {
                end -= 1;
                if end > body_start && buf[end - 1] == b'\r' {
                    end -= 1;
                }
            } else if end > body_start && buf[end - 1] == b'\r' {
                end -= 1;
            }
            return Ok((end, i));
        }
        i += 1;
    }
    Err(Error::eof(buf.len()))
}
