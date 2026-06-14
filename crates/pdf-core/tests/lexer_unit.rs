//! Lexer unit tests — the `LEXER-*` catalog (M1a).
//!
//! Spec source of truth: ISO 32000-1 §7.2 (lexical conventions) and §7.3
//! (object syntax), plus the malformation-tolerance requirements of PRD §8.1.
//! The overriding contract: the lexer is **total** — it never panics, and
//! malformed input yields a typed error, not a crash.

use pdf_core::error::Error;
use pdf_core::lexer::{Keyword, Lexer, Token};

/// Collects all tokens (stopping at `Eof`) or returns the first error.
fn lex_all(buf: &[u8]) -> Result<Vec<Token>, Error> {
    let mut lx = Lexer::new(buf);
    let mut out = Vec::new();
    loop {
        let t = lx.next_token()?;
        if t == Token::Eof {
            break;
        }
        out.push(t);
    }
    Ok(out)
}

#[test]
fn lexer_whitespace_skipped() {
    // LEXER-001: all six whitespace bytes are skipped between tokens.
    let buf = b"\0\t\n\x0c\r 1\0\t\n 2";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::Integer(1), Token::Integer(2)]
    );
}

#[test]
fn lexer_comment_skipped() {
    // LEXER-002: comment `%`…EOL is skipped; tokens around it survive.
    let buf = b"1 % this is a comment )(/ junk\n 2";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::Integer(1), Token::Integer(2)]
    );
}

#[test]
fn lexer_integers() {
    // LEXER-003: integer literals with optional sign.
    assert_eq!(
        lex_all(b"0 123 +17 -98").unwrap(),
        vec![
            Token::Integer(0),
            Token::Integer(123),
            Token::Integer(17),
            Token::Integer(-98),
        ]
    );
}

#[test]
fn lexer_reals() {
    // LEXER-004: reals incl. trailing dot, leading dot, signs.
    let toks = lex_all(b"34.5 -3.62 +.002 4. .5").unwrap();
    let reals: Vec<f64> = toks
        .iter()
        .map(|t| match t {
            Token::Real(r) => *r,
            other => panic!("expected real, got {other:?}"),
        })
        .collect();
    let expected = [34.5, -3.62, 0.002, 4.0, 0.5];
    for (got, exp) in reals.iter().zip(expected) {
        assert!((got - exp).abs() < 1e-9, "got {got}, expected {exp}");
    }
}

#[test]
fn lexer_real_exponent_tolerated() {
    // LEXER-005: scientific notation is tolerated (PRD §8.1).
    let toks = lex_all(b"1e3 1.2E-2").unwrap();
    match (&toks[0], &toks[1]) {
        (Token::Real(a), Token::Real(b)) => {
            assert!((a - 1000.0).abs() < 1e-6);
            assert!((b - 0.012).abs() < 1e-9);
        }
        other => panic!("expected two reals, got {other:?}"),
    }
}

#[test]
fn lexer_literal_string_escapes() {
    // LEXER-006: literal string with the named escapes.
    let buf = br"(a\nb\rc\td\be\ff\(g\)h\\i)";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::LiteralString(
            b"a\nb\rc\td\x08e\x0cf(g)h\\i".to_vec()
        )]
    );
}

#[test]
fn lexer_literal_string_octal() {
    // LEXER-007: octal escapes, including <3 digits and overflow wrap.
    // \101 = 'A'; \1 = 0x01; \12 = 0x0A; \400 -> wraps mod 256 -> 0x00.
    let buf = br"(\101\1\12\400)";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::LiteralString(vec![0x41, 0x01, 0x0A, 0x00])]
    );
}

#[test]
fn lexer_literal_string_line_continuation() {
    // LEXER-008: backslash + EOL elides the newline (CRLF and bare LF).
    let buf = b"(line1\\\nline2\\\r\nline3)";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::LiteralString(b"line1line2line3".to_vec())]
    );
}

#[test]
fn lexer_literal_string_nested_and_raw_newline() {
    // LEXER-009: balanced nested parens + a raw (unescaped) newline are kept.
    let buf = b"(a (b (c) d) e\nf)";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::LiteralString(b"a (b (c) d) e\nf".to_vec())]
    );
}

#[test]
fn lexer_hex_string() {
    // LEXER-010: hex string; embedded whitespace ignored.
    let buf = b"<48 65 6C\n6C 6F>";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::HexString(b"Hello".to_vec())]
    );
}

#[test]
fn lexer_hex_string_odd_padded() {
    // LEXER-011: odd nibble count pads a trailing 0 (<41A> -> 0x41 0xA0).
    let buf = b"<41A>";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![Token::HexString(vec![0x41, 0xA0])]
    );
}

#[test]
fn lexer_name_basic_and_empty() {
    // LEXER-012: `/Name`; `/` alone is the empty name.
    assert_eq!(
        lex_all(b"/Type /").unwrap(),
        vec![Token::Name(b"Type".to_vec()), Token::Name(Vec::new())]
    );
}

#[test]
fn lexer_name_hex_escape() {
    // LEXER-013: `#XX` decoded (`/A#42C` -> "ABC", and `/Lime#20Green`).
    assert_eq!(
        lex_all(b"/A#42C /Lime#20Green").unwrap(),
        vec![
            Token::Name(b"ABC".to_vec()),
            Token::Name(b"Lime Green".to_vec()),
        ]
    );
}

#[test]
fn lexer_dict_delimiters() {
    // LEXER-014: `<<` / `>>`.
    assert_eq!(
        lex_all(b"<< >>").unwrap(),
        vec![Token::DictOpen, Token::DictClose]
    );
}

#[test]
fn lexer_array_delimiters() {
    // LEXER-015: `[` / `]`.
    assert_eq!(
        lex_all(b"[ ]").unwrap(),
        vec![Token::ArrayOpen, Token::ArrayClose]
    );
}

#[test]
fn lexer_keywords() {
    // LEXER-016: all structure keywords classify correctly.
    let buf = b"obj endobj stream endstream R true false null xref trailer startxref";
    assert_eq!(
        lex_all(buf).unwrap(),
        vec![
            Token::Keyword(Keyword::Obj),
            Token::Keyword(Keyword::EndObj),
            Token::Keyword(Keyword::Stream),
            Token::Keyword(Keyword::EndStream),
            Token::Keyword(Keyword::R),
            Token::Keyword(Keyword::True),
            Token::Keyword(Keyword::False),
            Token::Keyword(Keyword::Null),
            Token::Keyword(Keyword::Xref),
            Token::Keyword(Keyword::Trailer),
            Token::Keyword(Keyword::StartXref),
        ]
    );
}

#[test]
fn lexer_keyword_vs_name() {
    // LEXER-017: `true` is a keyword; `/true` is a name (disambiguation).
    assert_eq!(
        lex_all(b"true /true").unwrap(),
        vec![Token::Keyword(Keyword::True), Token::Name(b"true".to_vec()),]
    );
}

#[test]
fn lexer_eof_idempotent() {
    // LEXER-018: EOF at end, and repeated `next_token` stays EOF.
    let mut lx = Lexer::new(b"  ");
    assert_eq!(lx.next_token().unwrap(), Token::Eof);
    assert_eq!(lx.next_token().unwrap(), Token::Eof);
    assert_eq!(lx.next_token().unwrap(), Token::Eof);
}

#[test]
fn lexer_truncated_literal_string_errors() {
    // LEXER-019: an unterminated `(` is a typed error, not a panic.
    let mut lx = Lexer::new(b"(abc");
    assert!(matches!(lx.next_token(), Err(Error::UnexpectedEof { .. })));
}

#[test]
fn lexer_truncated_hex_string_errors() {
    // LEXER-020: an unterminated `<` is a typed error.
    let mut lx = Lexer::new(b"<48");
    assert!(matches!(lx.next_token(), Err(Error::UnexpectedEof { .. })));
}

#[test]
fn lexer_truncated_name_escape_errors() {
    // LEXER-021: `/A#` with no following hex digits is a typed error.
    let mut lx = Lexer::new(b"/A#");
    assert!(matches!(lx.next_token(), Err(Error::UnexpectedEof { .. })));
}

#[test]
fn lexer_number_delimiter_boundary() {
    // LEXER-022: a delimiter ends a number with no whitespace ("1[2]" etc.).
    assert_eq!(
        lex_all(b"1[2]/Three").unwrap(),
        vec![
            Token::Integer(1),
            Token::ArrayOpen,
            Token::Integer(2),
            Token::ArrayClose,
            Token::Name(b"Three".to_vec()),
        ]
    );
}
