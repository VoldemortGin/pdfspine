//! Byte-oriented PDF tokenizer — ISO 32000-1 §7.2, PRD §8.1.
//!
//! The lexer is **total**: every method terminates and never panics on
//! arbitrary or truncated input. It either yields a [`Token`] (including
//! [`Token::Eof`]) or a typed [`Error`] (`Syntax` / `UnexpectedEof`). All
//! indexing goes through bounds-checked helpers, so out-of-range bytes are
//! impossible (PRD §8.1: "resync on garbage, never panic").
//!
//! This single lexer is reused for content-stream tokenizing later (PRD §8.1).

use crate::error::{Error, Result};

/// A lexical token. String / name payloads are already **decoded** (escapes and
/// `#XX` resolved) so the parser never re-decodes.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// Integer literal.
    Integer(i64),
    /// Real literal.
    Real(f64),
    /// Literal string `( … )` — decoded bytes.
    LiteralString(Vec<u8>),
    /// Hex string `< … >` — decoded bytes.
    HexString(Vec<u8>),
    /// Name `/Name` — decoded bytes (without the `/`).
    Name(Vec<u8>),
    /// `<<` dictionary open.
    DictOpen,
    /// `>>` dictionary close.
    DictClose,
    /// `[` array open.
    ArrayOpen,
    /// `]` array close.
    ArrayClose,
    /// A bare keyword (`obj`, `endobj`, `stream`, `R`, `true`, …).
    Keyword(Keyword),
    /// End of input.
    Eof,
}

/// The reserved keywords recognised by the lexer (ISO 32000-1 §7.3, structure
/// keywords for §7.5). Any other regular-character run becomes
/// [`Keyword::Other`] so the parser can decide (e.g. operators in content
/// streams) without the lexer guessing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Keyword {
    /// `obj`
    Obj,
    /// `endobj`
    EndObj,
    /// `stream`
    Stream,
    /// `endstream`
    EndStream,
    /// `R` (indirect reference).
    R,
    /// `true`
    True,
    /// `false`
    False,
    /// `null`
    Null,
    /// `xref`
    Xref,
    /// `trailer`
    Trailer,
    /// `startxref`
    StartXref,
    /// Any other regular-character run.
    Other(Vec<u8>),
}

/// Returns `true` for the six PDF whitespace bytes (ISO 32000-1 Table 1):
/// NUL, TAB, LF, FF, CR, SP.
#[must_use]
pub fn is_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

/// Returns `true` for the PDF delimiter bytes (ISO 32000-1 Table 2):
/// `( ) < > [ ] { } / %`.
#[must_use]
pub fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// A regular character is anything that is neither whitespace nor a delimiter.
#[must_use]
pub fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

/// The byte-oriented PDF lexer over a borrowed buffer.
#[derive(Clone, Debug)]
pub struct Lexer<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer positioned at the start of `buf`.
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Lexer { buf, pos: 0 }
    }

    /// The current byte offset.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Repositions the cursor (clamped into range). Used by the object parser to
    /// re-scan stream bodies; never lets the cursor escape the buffer.
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.buf.len());
    }

    /// The full backing buffer (read-only). Used by stream-body scanning.
    #[must_use]
    pub fn buffer(&self) -> &'a [u8] {
        self.buf
    }

    // --- bounds-checked primitives ---------------------------------------

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    #[inline]
    fn peek_at(&self, off: usize) -> Option<u8> {
        self.buf.get(self.pos + off).copied()
    }

    #[inline]
    fn bump(&mut self) -> Option<u8> {
        let b = self.buf.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    /// Skips whitespace and `%`…EOL comments. Total; stops at EOF.
    pub fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b) if is_whitespace(b) => {
                    self.pos += 1;
                }
                Some(b'%') => {
                    // Comment runs to (but not past) the next EOL.
                    self.pos += 1;
                    while let Some(b) = self.peek() {
                        if b == b'\n' || b == b'\r' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    /// Reads the next token, skipping leading whitespace/comments.
    ///
    /// Returns [`Token::Eof`] at end of input (idempotent). Returns `Err` only
    /// for genuinely malformed tokens (e.g. an unterminated string).
    pub fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace_and_comments();
        let b = match self.peek() {
            Some(b) => b,
            None => return Ok(Token::Eof),
        };

        match b {
            b'(' => self.lex_literal_string(),
            b'<' => {
                if self.peek_at(1) == Some(b'<') {
                    self.pos += 2;
                    Ok(Token::DictOpen)
                } else {
                    self.lex_hex_string()
                }
            }
            b'>' => {
                if self.peek_at(1) == Some(b'>') {
                    self.pos += 2;
                    Ok(Token::DictClose)
                } else {
                    // A lone '>' is not a valid PDF token.
                    Err(Error::syntax(self.pos, "unexpected '>'"))
                }
            }
            b'[' => {
                self.pos += 1;
                Ok(Token::ArrayOpen)
            }
            b']' => {
                self.pos += 1;
                Ok(Token::ArrayClose)
            }
            b'/' => self.lex_name(),
            b')' => Err(Error::syntax(self.pos, "unexpected ')'")),
            b'{' => {
                self.pos += 1;
                // Braces appear only in PostScript-calculator function streams;
                // surface as keyword-like tokens so callers can resync.
                Ok(Token::Keyword(Keyword::Other(vec![b'{'])))
            }
            b'}' => {
                self.pos += 1;
                Ok(Token::Keyword(Keyword::Other(vec![b'}'])))
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.lex_number_or_keyword(),
            _ => self.lex_keyword(),
        }
    }

    // --- number / keyword -------------------------------------------------

    /// Lexes a numeric token. A run that *starts* like a number but is not a
    /// valid number (e.g. `1.2.3`, `--2`) is tolerated per PRD §8.1 by clamping
    /// to a best-effort parse; if no digits at all are present the run is
    /// surfaced as a keyword so the parser can resync.
    fn lex_number_or_keyword(&mut self) -> Result<Token> {
        let start = self.pos;
        // Consume the contiguous regular-character run.
        while let Some(b) = self.peek() {
            if is_regular(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let raw = &self.buf[start..self.pos];
        match parse_number(raw) {
            Some(tok) => Ok(tok),
            // Looked numeric but isn't (e.g. lone "-" or "."): keep as Other.
            None => Ok(Token::Keyword(Keyword::Other(raw.to_vec()))),
        }
    }

    /// Lexes a bare keyword / regular-character run and classifies it.
    fn lex_keyword(&mut self) -> Result<Token> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_regular(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            // No regular char and not a delimiter we handle: consume one byte
            // to guarantee forward progress, surface as Other.
            self.pos += 1;
            return Ok(Token::Keyword(Keyword::Other(
                self.buf[start..self.pos].to_vec(),
            )));
        }
        let raw = &self.buf[start..self.pos];
        Ok(Token::Keyword(classify_keyword(raw)))
    }

    // --- literal string ---------------------------------------------------

    /// Lexes `( … )` with full escape handling (ISO 32000-1 §7.3.4.2).
    fn lex_literal_string(&mut self) -> Result<Token> {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'('));
        self.pos += 1; // consume '('
        let mut out = Vec::new();
        let mut depth = 1usize;

        while let Some(b) = self.bump() {
            match b {
                b'\\' => {
                    let e = match self.bump() {
                        Some(e) => e,
                        None => return Err(Error::eof(self.pos)),
                    };
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'(' => out.push(b'('),
                        b')' => out.push(b')'),
                        b'\\' => out.push(b'\\'),
                        b'\r' => {
                            // Line continuation: also swallow a following \n.
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => { /* line continuation: elide */ }
                        b'0'..=b'7' => {
                            // Octal escape: up to 3 digits total (e is first).
                            let mut val = (e - b'0') as u16;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        val = val.wrapping_mul(8).wrapping_add((d - b'0') as u16);
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((val & 0xFF) as u8);
                        }
                        // Any other escaped char: the backslash is ignored and
                        // the char is taken literally (ISO 32000-1 §7.3.4.2).
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(Token::LiteralString(out));
                    }
                    out.push(b')');
                }
                // Raw EOLs are kept verbatim (only \-continuations are elided).
                other => out.push(other),
            }
        }
        // Ran off the end with unbalanced parens.
        Err(Error::eof(start))
    }

    // --- hex string -------------------------------------------------------

    /// Lexes `< … >` (ISO 32000-1 §7.3.4.3): whitespace skipped, odd nibble
    /// count padded with a trailing `0`.
    fn lex_hex_string(&mut self) -> Result<Token> {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'<'));
        self.pos += 1; // consume '<'
        let mut out = Vec::new();
        let mut hi: Option<u8> = None;

        loop {
            let b = match self.bump() {
                Some(b) => b,
                None => return Err(Error::eof(start)),
            };
            match b {
                b'>' => {
                    if let Some(h) = hi {
                        // Odd count: pad low nibble with 0.
                        out.push(h << 4);
                    }
                    return Ok(Token::HexString(out));
                }
                _ if is_whitespace(b) => {}
                _ => match hex_val(b) {
                    Some(v) => match hi {
                        None => hi = Some(v),
                        Some(h) => {
                            out.push((h << 4) | v);
                            hi = None;
                        }
                    },
                    None => {
                        return Err(Error::syntax(
                            self.pos - 1,
                            "invalid hex digit in <…> string",
                        ))
                    }
                },
            }
        }
    }

    // --- name -------------------------------------------------------------

    /// Lexes `/Name` (ISO 32000-1 §7.3.5) decoding `#XX` escapes. `/` alone is
    /// the empty name.
    fn lex_name(&mut self) -> Result<Token> {
        debug_assert_eq!(self.peek(), Some(b'/'));
        self.pos += 1; // consume '/'
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if is_whitespace(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
            if b == b'#' {
                let h = self
                    .bump()
                    .ok_or_else(|| Error::eof(self.pos))
                    .and_then(|c| {
                        hex_val(c).ok_or_else(|| {
                            Error::syntax(self.pos - 1, "invalid hex digit in name #XX escape")
                        })
                    })?;
                let l = self
                    .bump()
                    .ok_or_else(|| Error::eof(self.pos))
                    .and_then(|c| {
                        hex_val(c).ok_or_else(|| {
                            Error::syntax(self.pos - 1, "invalid hex digit in name #XX escape")
                        })
                    })?;
                out.push((h << 4) | l);
            } else {
                out.push(b);
            }
        }
        Ok(Token::Name(out))
    }
}

/// Hex digit value, or `None` for non-hex bytes.
#[inline]
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Classifies a regular-character run into a [`Keyword`].
fn classify_keyword(raw: &[u8]) -> Keyword {
    match raw {
        b"obj" => Keyword::Obj,
        b"endobj" => Keyword::EndObj,
        b"stream" => Keyword::Stream,
        b"endstream" => Keyword::EndStream,
        b"R" => Keyword::R,
        b"true" => Keyword::True,
        b"false" => Keyword::False,
        b"null" => Keyword::Null,
        b"xref" => Keyword::Xref,
        b"trailer" => Keyword::Trailer,
        b"startxref" => Keyword::StartXref,
        other => Keyword::Other(other.to_vec()),
    }
}

/// Parses a numeric token from a regular-character run, tolerating common
/// malformations (PRD §8.1): leading/trailing dot, leading `+`, scientific
/// notation. Returns `None` when no number can be recovered (caller resyncs).
fn parse_number(raw: &[u8]) -> Option<Token> {
    if raw.is_empty() {
        return None;
    }
    let s = std::str::from_utf8(raw).ok()?;

    // Integer fast-path: optional sign then all digits, no '.'/'e'.
    let looks_real = s.bytes().any(|b| matches!(b, b'.' | b'e' | b'E'));
    if !looks_real {
        if let Ok(i) = s.parse::<i64>() {
            return Some(Token::Integer(i));
        }
        // Could be an integer that overflows i64; clamp.
        if let Some(stripped) = s.strip_prefix('-') {
            if !stripped.is_empty() && stripped.bytes().all(|b| b.is_ascii_digit()) {
                return Some(Token::Integer(i64::MIN));
            }
        } else {
            let digits = s.strip_prefix('+').unwrap_or(s);
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                return Some(Token::Integer(i64::MAX));
            }
        }
        return None;
    }

    // Real path. Normalise leading/trailing dot and a leading '+' which Rust's
    // f64 parser rejects, then fall back to a tolerant scan.
    if let Some(r) = parse_real_tolerant(s) {
        return Some(Token::Real(r));
    }
    None
}

/// Best-effort real parse tolerating `.5`, `4.`, `+1.2`, `1e3`, `1.2E-2`, and
/// (per PRD §8.1) recovering a prefix from `1.2.3`.
fn parse_real_tolerant(s: &str) -> Option<f64> {
    let s = s.strip_prefix('+').unwrap_or(s);
    if let Ok(r) = s.parse::<f64>() {
        if r.is_finite() {
            return Some(r);
        }
    }
    // Tolerant scan: keep an optional leading sign, the first '.', and a single
    // trailing exponent. Drop a second '.' onwards (the `1.2.3` case).
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut seen_dot = false;
    let mut i = 0;
    if matches!(bytes.first(), Some(b'-') | Some(b'+')) {
        out.push(bytes[0] as char);
        i = 1;
    }
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'0'..=b'9' => out.push(c as char),
            b'.' if !seen_dot => {
                seen_dot = true;
                out.push('.');
            }
            b'.' => break,        // second dot: stop (recover prefix)
            b'e' | b'E' => break, // ignore exponent in the recovery path
            _ => break,
        }
        i += 1;
    }
    if out.is_empty() || out == "-" || out == "+" || out == "." {
        return None;
    }
    out.parse::<f64>().ok().filter(|r| r.is_finite())
}
