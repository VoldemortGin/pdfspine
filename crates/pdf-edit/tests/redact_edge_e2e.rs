//! `REDACT-*` edge coverage — the seldom-exercised content-rewrite surfaces of
//! `apply_redactions` (PRD §8.8): `TJ` arrays with numeric adjustments, the full
//! text-state operator set (`Tc`/`Tw`/`Tz`/`TL`/`Ts`/`Td`/`TD`/`T*`/`'`/`"`),
//! unmappable-font verbatim re-emission, Form XObjects with a `/Matrix`, `Do`
//! edge cases, `/Contents` arrays, inline images + complex operands, and the
//! image pixel-blank error paths (bpc / colorspace / short-buffer fail-closed).
//!
//! Every case asserts a real redaction property (secret gone from the
//! decompressed corpus, survivors preserved, applied count, fail-closed error).

mod common;

use common::{
    ascii_font, assemble_classic, decompress_corpus, dict, first_image_pixels, name_obj, open,
    page_content_bytes, page_glyphs, rref, save_bytes, simple_text_page,
};

use pdf_core::error::Error;
use pdf_core::geom::Rect;
use pdf_core::{Name, ObjRef, Object, StreamObj};
use pdf_edit::{add_redact_annot, annot_count, apply_redactions, get_drawings};
use pdf_text::interpret_page;

/// The plain extracted text of page 0 (concatenated glyph unicode).
fn page_text(doc: &pdf_core::DocumentStore) -> String {
    let leaf = pdf_core::pagetree::page_refs(doc)[0];
    let page = pdf_core::pagetree::page_dict(doc, leaf).expect("page");
    interpret_page(doc, &page)
        .glyphs
        .iter()
        .map(|g| g.unicode.as_str())
        .collect()
}

/// Whether `haystack` contains `needle` (byte search).
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// === REDACT-TEXT (TJ arrays, text-state operators, unmapped fonts) =========

#[test]
fn redact_text_005_tj_array_numeric_adjust() {
    // A single `TJ` array mixing kept runs, numeric adjustments and a secret run
    // in the middle. The rewriter must drop only the secret glyphs and fold the
    // dropped advance into the surrounding adjustments (rewrite_show_array).
    let body =
        b"BT /F1 12 Tf 1 0 0 1 72 700 Tm [(KEEPA) -300 (SECRETX) 200 (KEEPB)] TJ ET".to_vec();
    let bytes = simple_text_page(body);
    let doc = open(&bytes);
    // "KEEPA" ends ~x=108; "SECRETX" spans ~111.6..162; "KEEPB" starts ~159.6.
    // Rect x[110,158] catches every SECRETX glyph but not KEEPA/KEEPB.
    add_redact_annot(&doc, 0, Rect::new(110.0, 80.0, 158.0, 98.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);

    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    assert!(
        !contains(&corpus, b"SECRETX"),
        "secret inside a TJ array must be gone from the corpus"
    );
    let text = page_text(&open(&out));
    assert!(!text.contains("SECRETX"), "get_text must not show secret");
    assert!(text.contains("KEEPA"), "leading run survives: {text:?}");
    assert!(text.contains("KEEPB"), "trailing run survives: {text:?}");
}

#[test]
fn redact_text_006_tj_array_unmapped_font_verbatim() {
    // A `TJ` array shown with (a) no current font and (b) a font missing from
    // resources: the rewriter cannot map codes, so it must re-emit the array
    // verbatim (emit_tj_array_verbatim) — the text is preserved, not corrupted.
    let body =
        b"BT [(NOFONTA) -50 (NOFONTB)] TJ /FZ 12 Tf [(MISSINGA) 30 (MISSINGB)] TJ ET".to_vec();
    let bytes = simple_text_page(body);
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, Rect::new(10.0, 10.0, 20.0, 20.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);

    let corpus = decompress_corpus(&save_bytes(&doc));
    // Unmappable text is re-emitted verbatim (documented v1 limitation).
    for needle in [
        &b"NOFONTA"[..],
        &b"NOFONTB"[..],
        &b"MISSINGA"[..],
        &b"MISSINGB"[..],
    ] {
        assert!(
            contains(&corpus, needle),
            "verbatim TJ array must preserve {:?}",
            std::str::from_utf8(needle).unwrap()
        );
    }
}

#[test]
fn redact_text_007_text_state_operators() {
    // Exercise the full text-state / positioning operator set through one page:
    // Tc Tw Tz TL Ts (state), Td TD T* (positioning), ' and " (move + show), a
    // second Tf (font-cache hit). One middle line is redacted; the rest survive.
    let body = b"BT /F1 12 Tf 2 Tc 5 Tw 90 Tz 14 TL 0 Ts \
                 1 0 0 1 72 700 Tm (LINEONE) Tj \
                 0 -14 Td (LINETWO) Tj \
                 /F1 10 Tf 0 -14 TD (LINETHR) Tj \
                 T* (LINEFOR) Tj \
                 (LINEFIV) ' \
                 2 3 (LINESIX) \" ET"
        .to_vec();
    let bytes = simple_text_page(body);
    let doc = open(&bytes);
    // "LINETHR" sits at baseline y=672 (font 10) → top-left y ≈ [112,122].
    add_redact_annot(&doc, 0, Rect::new(70.0, 110.0, 320.0, 124.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);

    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    assert!(!contains(&corpus, b"LINETHR"), "redacted line gone");
    let text = page_text(&open(&out));
    assert!(!text.contains("LINETHR"));
    for kept in ["LINEONE", "LINETWO", "LINEFOR", "LINEFIV", "LINESIX"] {
        assert!(text.contains(kept), "survivor {kept} missing: {text:?}");
    }
}

#[test]
fn redact_text_008_unmapped_literal_and_escapes() {
    // A `Tj` literal shown with no font, whose bytes contain every character
    // escape_show must escape (\\ ( ) \n \r \t), then a `Tj` with a missing font.
    // Both are re-emitted verbatim via emit_tj_literal + escape_show.
    let mut body = Vec::new();
    body.extend_from_slice(b"BT (a\\\\b\\(c\\)d\\ne\\rf\\tg) Tj /FZ 12 Tf (KEPTFONT) Tj ET");
    let bytes = simple_text_page(body);
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, Rect::new(10.0, 10.0, 20.0, 20.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);

    let corpus = decompress_corpus(&save_bytes(&doc));
    // Unmapped literals survive; the special characters are re-escaped, so the
    // escaped backslash+paren run is present in the re-emitted content.
    assert!(
        contains(&corpus, b"KEPTFONT"),
        "missing-font literal preserved"
    );
    assert!(
        contains(&corpus, br"\(c\)"),
        "escape_show must re-escape the parens"
    );
}

#[test]
fn redact_text_009_contents_array_two_streams() {
    // A page whose /Contents is an *array* of two content streams. The rewriter
    // must concatenate both (page_content_bytes array arm), redact across them,
    // and free every old content object (content_refs array arm).
    let s1 = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (KEEPARR) Tj ET".to_vec();
    let s2 = b"BT /F1 12 Tf 1 0 0 1 72 680 Tm (SECRETARR) Tj ET".to_vec();
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media_box()),
                ("Contents", Object::Array(vec![rref(4), rref(6)])),
                (
                    "Resources",
                    Object::Dictionary(dict([(
                        "Font",
                        Object::Dictionary(dict([("F1", rref(5))])),
                    )])),
                ),
            ])),
        ),
        (4, stream_obj(s1)),
        (5, ascii_font()),
        (6, stream_obj(s2)),
    ];
    let bytes = assemble_classic(&objects, ObjRef::new(1, 0));
    let doc = open(&bytes);
    // "SECRETARR" at baseline y=680 → top-left y ≈ [102,114].
    add_redact_annot(&doc, 0, Rect::new(70.0, 100.0, 300.0, 118.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);

    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    assert!(
        !contains(&corpus, b"SECRETARR"),
        "secret in 2nd stream gone"
    );
    let text = page_text(&open(&out));
    assert!(
        text.contains("KEEPARR"),
        "1st stream text survives: {text:?}"
    );
    assert!(!text.contains("SECRETARR"));
}

/// The user-space origin of the first glyph of `word` on page 0 (`None` when
/// absent). Glyphs come in content order and the test font maps one ASCII
/// char per glyph, so the char index into the concatenated unicode is the
/// glyph index.
fn word_origin(doc: &pdf_core::DocumentStore, word: &str) -> Option<(f64, f64)> {
    let glyphs = page_glyphs(doc, 0);
    let text: String = glyphs.iter().map(|g| g.unicode.as_str()).collect();
    let g = &glyphs[text.find(word)?];
    Some((g.origin.x, g.origin.y))
}

/// Asserts `word` is shown on page 0 with its first glyph at `(x, y)`.
fn assert_origin(doc: &pdf_core::DocumentStore, word: &str, x: f64, y: f64) {
    let got = word_origin(doc, word).unwrap_or_else(|| panic!("{word} absent"));
    assert!(
        (got.0 - x).abs() < 0.01 && (got.1 - y).abs() < 0.01,
        "{word} expected at ({x}, {y}), got {got:?}"
    );
}

/// Asserts `word` survives the redaction at exactly its original origin.
fn assert_unshifted(before: &pdf_core::DocumentStore, after: &pdf_core::DocumentStore, word: &str) {
    let b = word_origin(before, word).unwrap_or_else(|| panic!("{word} absent before redaction"));
    assert_origin(after, word, b.0, b.1);
}

/// How many times `needle` occurs in `haystack` (non-overlapping byte search).
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

#[test]
fn redact_text_010_quote_operator_keeps_line_advance() {
    // Three lines: `Tj`, then two `'` (T* + show). The secret sits on the
    // middle `'` line next to a kept run. The rewriter used to emit the `'`
    // show as a bare `TJ` and lose the implicit line advance, so survivors
    // (and every later line) fell onto the previous baseline.
    let body = b"BT /F1 12 Tf 14 TL 1 0 0 1 72 700 Tm (LINEONE) Tj \
                 (KEEPB SECRETL) ' (LINETHR) ' ET"
        .to_vec();
    let before = open(&simple_text_page(body.clone()));
    assert_origin(&before, "LINETHR", 72.0, 672.0);

    let doc = open(&simple_text_page(body));
    // Line 2 baseline y=686; "SECRETL" spans x 115.2..165.6 → top-left rect.
    add_redact_annot(&doc, 0, Rect::new(114.0, 96.0, 167.0, 108.0), None, None).unwrap();
    assert_eq!(apply_redactions(&doc, 0).unwrap(), 1);

    let out = save_bytes(&doc);
    assert!(!contains(&decompress_corpus(&out), b"SECRETL"));
    let after = open(&out);
    for word in ["LINEONE", "KEEPB", "LINETHR"] {
        assert_unshifted(&before, &after, word);
    }
    let content = page_content_bytes(&after, 0);
    assert_eq!(
        count(&content, b"T*"),
        2,
        "each `'` must become an explicit T*: {}",
        String::from_utf8_lossy(&content)
    );
}

#[test]
fn redact_text_011_dquote_operator_keeps_spacing() {
    // `aw ac (s) "` sets Tw/Tc for the rest of the text object: the following
    // `'` line inherits them, so dropping the operands (the old bare-`TJ`
    // rewrite) would re-space "LINE THR" as well as misplace the line.
    let body = b"BT /F1 12 Tf 14 TL 1 0 0 1 72 700 Tm (LINEONE) Tj \
                 3 1 (KEEPB SECRETL) \" (LINE THR) ' ET"
        .to_vec();
    let before = open(&simple_text_page(body.clone()));
    // Tc=1, Tw=3: "LINE " = 4·8.2 + 11.2 → "THR" starts at x=116 (108 without).
    assert_origin(&before, "THR", 116.0, 672.0);

    let doc = open(&simple_text_page(body));
    // Line 2 baseline y=686; "SECRETL" spans x 124.2..181.6 → top-left rect.
    add_redact_annot(&doc, 0, Rect::new(123.0, 96.0, 183.0, 108.0), None, None).unwrap();
    assert_eq!(apply_redactions(&doc, 0).unwrap(), 1);

    let out = save_bytes(&doc);
    assert!(!contains(&decompress_corpus(&out), b"SECRETL"));
    let after = open(&out);
    for word in ["LINEONE", "KEEPB", "LINE", "THR"] {
        assert_unshifted(&before, &after, word);
    }
    let content = page_content_bytes(&after, 0);
    let shown = String::from_utf8_lossy(&content);
    assert!(contains(&content, b"3 Tw"), "aw re-emitted as Tw: {shown}");
    assert!(contains(&content, b"1 Tc"), "ac re-emitted as Tc: {shown}");
    assert_eq!(
        count(&content, b"T*"),
        2,
        "`\"` and `'` each become a T*: {shown}"
    );
}

#[test]
fn redact_text_012_mixed_show_operators_dropped_line() {
    // `Tj` / `'` / `"` mixed, with untouched runs on both sides of a `'` line
    // that is dropped *entirely* (nothing survives to emit a TJ): the explicit
    // T* alone must still carry the advance so lines four and five keep their
    // baselines, and the untouched `'` / `"` runs are preserved as-is.
    let body = b"BT /F1 12 Tf 14 TL 1 0 0 1 72 700 Tm (LINEONE) Tj \
                 (LINETWO) ' (SECRETL) ' 2 1 (LINEFOR) \" (LINEFIV) ' ET"
        .to_vec();
    let before = open(&simple_text_page(body.clone()));
    assert_origin(&before, "LINEFIV", 72.0, 644.0);

    let doc = open(&simple_text_page(body));
    // Line 3 baseline y=672 → top-left y 110..122; cover the whole line.
    add_redact_annot(&doc, 0, Rect::new(70.0, 110.0, 200.0, 122.0), None, None).unwrap();
    assert_eq!(apply_redactions(&doc, 0).unwrap(), 1);

    let out = save_bytes(&doc);
    assert!(!contains(&decompress_corpus(&out), b"SECRETL"));
    let after = open(&out);
    assert_eq!(page_text(&after), "LINEONELINETWOLINEFORLINEFIV");
    for word in ["LINEONE", "LINETWO", "LINEFOR", "LINEFIV"] {
        assert_unshifted(&before, &after, word);
    }
    let content = page_content_bytes(&after, 0);
    let shown = String::from_utf8_lossy(&content);
    assert_eq!(
        count(&content, b"T*"),
        4,
        "three `'` and one `\"` → four explicit T*: {shown}"
    );
    assert!(contains(&content, b"2 Tw"), "{shown}");
    assert!(contains(&content, b"1 Tc"), "{shown}");
}

#[test]
fn redact_text_013_quote_operators_unmapped_font_keep_advance() {
    // `'` / `"` shown with a font missing from resources take the verbatim
    // `Tj` path: the line advance and spacing operands must be expanded there
    // too, so the unmappable text is preserved *and* stays on its lines.
    let body = b"BT /FZ 12 Tf 14 TL 1 0 0 1 72 700 Tm (NOMAPONE) Tj \
                 (NOMAPTWO) ' 4 2 (NOMAPTHR) \" ET"
        .to_vec();
    let doc = open(&simple_text_page(body));
    add_redact_annot(&doc, 0, Rect::new(10.0, 10.0, 20.0, 20.0), None, None).unwrap();
    assert_eq!(apply_redactions(&doc, 0).unwrap(), 1);

    let after = open(&save_bytes(&doc));
    let content = page_content_bytes(&after, 0);
    let shown = String::from_utf8_lossy(&content);
    assert!(contains(&content, b"T*\n(NOMAPTWO) Tj"), "{shown}");
    assert!(
        contains(&content, b"4 Tw\n2 Tc\nT*\n(NOMAPTHR) Tj"),
        "{shown}"
    );
    assert!(contains(&content, b"(NOMAPONE) Tj"), "{shown}");
}

// === REDACT-FORM (Form XObject recursion) =================================

#[test]
fn redact_form_001_form_matrix_rewritten() {
    // A Form XObject carrying an explicit `/Matrix` array (so array_to_matrix
    // runs) whose text falls under the rect: the form stream is rewritten in
    // place and the secret removed from the form body.
    let secret = "MATRIXSECRET";
    let char_w = 12.0 * 0.6;
    let x_secret = 200.0;
    let x_end = x_secret + secret.len() as f64 * char_w;
    let form_body = format!("BT /F1 12 Tf 1 0 0 1 {x_secret} 700 Tm ({secret}) Tj ET").into_bytes();
    let page_body = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (VISFORM) Tj ET\n/X1 Do".to_vec();
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media_box()),
                ("Contents", rref(4)),
                (
                    "Resources",
                    Object::Dictionary(dict([
                        ("Font", Object::Dictionary(dict([("F1", rref(5))]))),
                        ("XObject", Object::Dictionary(dict([("X1", rref(6))]))),
                    ])),
                ),
            ])),
        ),
        (4, stream_obj(page_body)),
        (5, ascii_font()),
        (
            6,
            Object::Stream(StreamObj::new_encoded(
                dict([
                    ("Type", name_obj("XObject")),
                    ("Subtype", name_obj("Form")),
                    ("FormType", Object::Integer(1)),
                    (
                        "Matrix",
                        Object::Array(vec![
                            Object::Integer(1),
                            Object::Integer(0),
                            Object::Integer(0),
                            Object::Integer(1),
                            Object::Integer(0),
                            Object::Integer(0),
                        ]),
                    ),
                    ("BBox", media_box()),
                    (
                        "Resources",
                        Object::Dictionary(dict([(
                            "Font",
                            Object::Dictionary(dict([("F1", rref(5))])),
                        )])),
                    ),
                    ("Length", Object::Integer(form_body.len() as i64)),
                ]),
                form_body,
            )),
        ),
    ];
    let bytes = assemble_classic(&objects, ObjRef::new(1, 0));
    let doc = open(&bytes);
    let rect = Rect::new(x_secret - 1.0, 82.0, x_end + 1.0, 96.0);
    add_redact_annot(&doc, 0, rect, None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);

    let corpus = decompress_corpus(&save_bytes(&doc));
    assert!(
        !contains(&corpus, secret.as_bytes()),
        "secret drawn via a form with a /Matrix must be scrubbed"
    );
    let text = page_text(&open(&save_bytes(&doc)));
    assert!(
        text.contains("VISFORM"),
        "page-level text survives: {text:?}"
    );
}

#[test]
fn redact_form_002_do_edge_cases() {
    // Three `Do` edge cases the rewriter must tolerate without dropping page
    // text: a bare `Do` (no name operand), a `Do` naming an absent XObject, and
    // a `Do` on an XObject whose /Subtype is neither Image nor Form.
    let page_body =
        b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (KEEPDO) Tj ET\nDo\n/Ghost Do\n/X1 Do".to_vec();
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media_box()),
                ("Contents", rref(4)),
                (
                    "Resources",
                    Object::Dictionary(dict([
                        ("Font", Object::Dictionary(dict([("F1", rref(5))]))),
                        ("XObject", Object::Dictionary(dict([("X1", rref(6))]))),
                    ])),
                ),
            ])),
        ),
        (4, stream_obj(page_body)),
        (5, ascii_font()),
        (
            6,
            Object::Stream(StreamObj::new_encoded(
                dict([
                    ("Type", name_obj("XObject")),
                    ("Subtype", name_obj("PS")), // not Image / Form → kept as-is
                    ("Length", Object::Integer(0)),
                ]),
                Vec::new(),
            )),
        ),
    ];
    let bytes = assemble_classic(&objects, ObjRef::new(1, 0));
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, Rect::new(10.0, 10.0, 20.0, 20.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);
    let text = page_text(&open(&save_bytes(&doc)));
    assert!(
        text.contains("KEEPDO"),
        "text survives odd Do ops: {text:?}"
    );
}

// === REDACT-COVER (fill color, multiple annots) ===========================

#[test]
fn redact_cover_003_manual_ic_gray_and_badlen() {
    // Two hand-authored /Redact annots on one page with unusual /IC arrays: a
    // 1-element gray fill (color_from_array gray path) and a 4-element array
    // (invalid length → falls back to black). Both secrets are removed.
    let content = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (SECRA) Tj \
                    1 0 0 1 72 680 Tm (SECRB) Tj ET"
        .to_vec();
    let redact_annot = |rect: [f64; 4], ic: Vec<Object>| -> Object {
        Object::Dictionary(dict([
            ("Type", name_obj("Annot")),
            ("Subtype", name_obj("Redact")),
            ("P", rref(3)),
            (
                "Rect",
                Object::Array(rect.iter().map(|&v| Object::Real(v)).collect()),
            ),
            ("IC", Object::Array(ic)),
        ]))
    };
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media_box()),
                ("Contents", rref(4)),
                (
                    "Resources",
                    Object::Dictionary(dict([(
                        "Font",
                        Object::Dictionary(dict([("F1", rref(5))])),
                    )])),
                ),
                ("Annots", Object::Array(vec![rref(10), rref(11)])),
            ])),
        ),
        (4, stream_obj(content)),
        (5, ascii_font()),
        // /Rect is user space; SECRA cell ≈ y[697.6,709.6], SECRB ≈ y[677.6,689.6].
        (
            10,
            redact_annot([72.0, 696.0, 110.0, 711.0], vec![Object::Real(0.5)]),
        ),
        (
            11,
            redact_annot(
                [72.0, 676.0, 110.0, 691.0],
                vec![
                    Object::Real(1.0),
                    Object::Real(0.0),
                    Object::Real(0.0),
                    Object::Real(0.0),
                ],
            ),
        ),
    ];
    let bytes = assemble_classic(&objects, ObjRef::new(1, 0));
    let doc = open(&bytes);
    assert_eq!(annot_count(&doc, 0), 2);

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 2, "both hand-authored redact annots applied");

    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    assert!(!contains(&corpus, b"SECRA"), "gray-IC secret removed");
    assert!(!contains(&corpus, b"SECRB"), "bad-length-IC secret removed");
    let re = open(&out);
    assert_eq!(annot_count(&re, 0), 0, "redact annots removed");
    // Two cover boxes drawn (one gray, one defaulted black).
    let fills = get_drawings(&re, 0)
        .iter()
        .filter(|d| d.fill.is_some())
        .count();
    assert!(fills >= 2, "a cover box per region, got {fills}");
}

// === REDACT-PROP (no contents, verbatim operators) ========================

#[test]
fn redact_prop_004_page_without_contents() {
    // A page with no /Contents key at all: the rewrite yields empty content, a
    // trailing newline is appended, and only the cover box is written.
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media_box()),
                ("Resources", Object::Dictionary(pdf_core::Dict::new())),
            ])),
        ),
    ];
    let bytes = assemble_classic(&objects, ObjRef::new(1, 0));
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, Rect::new(100.0, 100.0, 200.0, 200.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1, "redaction applies even to a content-less page");

    let re = open(&save_bytes(&doc));
    let fills = get_drawings(&re, 0)
        .iter()
        .filter(|d| d.fill.is_some())
        .count();
    assert!(fills >= 1, "cover box drawn onto the previously-empty page");
}

#[test]
fn redact_prop_005_inline_image_and_operands_reemitted() {
    // A content stream with an inline image, a marked-content `BDC` carrying a
    // nested dictionary operand (string / real / array / bool / null values) and
    // a malformed `cm` (too few operands). All must be re-emitted verbatim and
    // the visible text preserved.
    let mut body = Vec::new();
    body.extend_from_slice(b"q 1 2 3 cm ");
    body.extend_from_slice(b"20 0 0 20 100 100 cm ");
    body.extend_from_slice(b"BI /W 2 /H 2 /CS /G /BPC 8 /F /AHx ID 00ff00ff EI Q\n");
    body.extend_from_slice(b"/Span <</S (barstr) /R 1.5 /A [1 2 3] /B true /Nl null>> BDC ");
    body.extend_from_slice(b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (VISIBLE) Tj ET EMC");
    let bytes = simple_text_page(body);
    let doc = open(&bytes);
    add_redact_annot(&doc, 0, Rect::new(10.0, 10.0, 20.0, 20.0), None, None).unwrap();

    let n = apply_redactions(&doc, 0).unwrap();
    assert_eq!(n, 1);

    let out = save_bytes(&doc);
    let corpus = decompress_corpus(&out);
    // Inline image markers survive the rewrite.
    assert!(contains(&corpus, b"BI"), "inline image re-emitted");
    assert!(
        contains(&corpus, b"EI"),
        "inline image terminator re-emitted"
    );
    // Complex operand parts round-trip through write_operand.
    assert!(contains(&corpus, b"barstr"), "string operand re-emitted");
    assert!(contains(&corpus, b"1.5"), "real operand re-emitted");
    assert!(contains(&corpus, b"true"), "bool operand re-emitted");
    assert!(contains(&corpus, b"null"), "null operand re-emitted");
    let text = page_text(&open(&out));
    assert!(text.contains("VISIBLE"), "text preserved: {text:?}");
}

// === REDACT-IMAGE (filter variants + pixel-blank error paths) =============

#[test]
fn redact_image_004_raw_and_filter_array_partial() {
    // Partial coverage of (a) a raw, unfiltered RGB image and (b) an RGB image
    // whose /Filter is a single-element array — both are pixel-editable, so the
    // covered columns are zeroed and the rest preserved.
    for filter in [None, Some(Object::Array(vec![name_obj("FlateDecode")]))] {
        let raw = filter.is_none();
        let pixels = vec![255u8; 8 * 8 * 3]; // solid white
        let stream_bytes = if raw {
            pixels
        } else {
            pdf_core::filters::flate::encode(&pixels)
        };
        let bytes = image_page(
            8,
            8,
            8,
            "DeviceRGB",
            filter,
            stream_bytes,
            100.0,
            100.0,
            80.0,
            80.0,
        );
        let doc = open(&bytes);
        // Left half only → partial coverage (not fully removed).
        add_redact_annot(&doc, 0, Rect::new(100.0, 100.0, 140.0, 180.0), None, None).unwrap();
        apply_redactions(&doc, 0).unwrap();

        let re = open(&save_bytes(&doc));
        let (w, h, n, px) = first_image_pixels(&re);
        assert_eq!((w, h, n), (8, 8, 3));
        let pixel = |row: usize, col: usize, ch: usize| px[(row * w + col) * n + ch];
        assert_eq!(pixel(0, 0, 0), 0, "left column zeroed (raw={raw})");
        assert_eq!(pixel(7, 7, 0), 255, "right column preserved (raw={raw})");
    }
}

#[test]
fn redact_image_005_non_overlapping_and_fully_covered() {
    // (a) An image nowhere near the rect is left byte-for-byte intact.
    {
        let bytes = image_page_rgb(4, 4, (0, 0, 255), 100.0, 100.0, 40.0, 40.0);
        let doc = open(&bytes);
        add_redact_annot(&doc, 0, Rect::new(400.0, 400.0, 450.0, 450.0), None, None).unwrap();
        let n = apply_redactions(&doc, 0).unwrap();
        assert_eq!(n, 1);
        let (_, _, _, px) = first_image_pixels(&open(&save_bytes(&doc)));
        assert_eq!(px[2], 255, "non-overlapping image's blue channel intact");
        assert_eq!(px[0], 0, "non-overlapping image's red channel intact");
    }
    // (b) A fully-covered image has its `/X1 Do` dropped from the content.
    {
        let bytes = image_page_rgb(4, 4, (255, 0, 0), 100.0, 100.0, 50.0, 50.0);
        let doc = open(&bytes);
        add_redact_annot(&doc, 0, Rect::new(90.0, 90.0, 160.0, 160.0), None, None).unwrap();
        apply_redactions(&doc, 0).unwrap();
        let re = open(&save_bytes(&doc));
        let leaf = pdf_core::pagetree::page_refs(&re)[0];
        let page = pdf_core::pagetree::page_dict(&re, leaf).unwrap();
        assert!(
            interpret_page(&re, &page).images.is_empty(),
            "fully-covered image removed"
        );
    }
}

#[test]
fn redact_image_006_pixel_edit_error_paths() {
    // Three partially-covered images the rewriter cannot pixel-edit must all
    // fail closed with Error::Redaction (never silently leaving secret pixels):
    // a non-8-bit image, an unsupported color space, and a short pixel buffer.
    let rect = Rect::new(100.0, 100.0, 140.0, 180.0); // left half → partial

    // (a) BitsPerComponent != 8.
    {
        let bytes = image_page(
            8,
            8,
            1,
            "DeviceGray",
            Some(name_obj("FlateDecode")),
            pdf_core::filters::flate::encode(&[0u8; 8]),
            100.0,
            100.0,
            80.0,
            80.0,
        );
        let doc = open(&bytes);
        add_redact_annot(&doc, 0, rect, None, None).unwrap();
        assert!(matches!(
            apply_redactions(&doc, 0).unwrap_err(),
            Error::Redaction(_)
        ));
    }
    // (b) Unsupported color space (CMYK).
    {
        let bytes = image_page(
            8,
            8,
            8,
            "DeviceCMYK",
            Some(name_obj("FlateDecode")),
            pdf_core::filters::flate::encode(&[0u8; 8 * 8 * 4]),
            100.0,
            100.0,
            80.0,
            80.0,
        );
        let doc = open(&bytes);
        add_redact_annot(&doc, 0, rect, None, None).unwrap();
        assert!(matches!(
            apply_redactions(&doc, 0).unwrap_err(),
            Error::Redaction(_)
        ));
    }
    // (c) Short pixel buffer (fewer bytes than Width*Height*3).
    {
        let bytes = image_page(
            8,
            8,
            8,
            "DeviceRGB",
            Some(name_obj("FlateDecode")),
            pdf_core::filters::flate::encode(&[255u8; 30]),
            100.0,
            100.0,
            80.0,
            80.0,
        );
        let doc = open(&bytes);
        add_redact_annot(&doc, 0, rect, None, None).unwrap();
        assert!(matches!(
            apply_redactions(&doc, 0).unwrap_err(),
            Error::Redaction(_)
        ));
    }
}

// === local builders =======================================================

/// The shared 612×792 `/MediaBox` array.
fn media_box() -> Object {
    Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ])
}

/// A content-stream object carrying `body` verbatim.
fn stream_obj(body: Vec<u8>) -> Object {
    Object::Stream(StreamObj::new_encoded(
        dict([("Length", Object::Integer(body.len() as i64))]),
        body,
    ))
}

/// A single-page document (612×792) placing an image XObject `/X1` (object 6)
/// with the given dimensions / color space / filter and raw stream bytes.
#[allow(clippy::too_many_arguments)]
fn image_page(
    iw: i64,
    ih: i64,
    bpc: i64,
    colorspace: &str,
    filter: Option<Object>,
    stream_bytes: Vec<u8>,
    place_x: f64,
    place_y_topleft: f64,
    place_w: f64,
    place_h: f64,
) -> Vec<u8> {
    let mut img_dict = dict([
        ("Type", name_obj("XObject")),
        ("Subtype", name_obj("Image")),
        ("Width", Object::Integer(iw)),
        ("Height", Object::Integer(ih)),
        ("ColorSpace", name_obj(colorspace)),
        ("BitsPerComponent", Object::Integer(bpc)),
        ("Length", Object::Integer(stream_bytes.len() as i64)),
    ]);
    if let Some(f) = filter {
        img_dict.insert(Name::new("Filter"), f);
    }
    let y_user = 792.0 - (place_y_topleft + place_h);
    let content = format!("q {place_w} 0 0 {place_h} {place_x} {y_user} cm /X1 Do Q").into_bytes();
    let objects: Vec<(u32, Object)> = vec![
        (
            1,
            Object::Dictionary(dict([("Type", name_obj("Catalog")), ("Pages", rref(2))])),
        ),
        (
            2,
            Object::Dictionary(dict([
                ("Type", name_obj("Pages")),
                ("Kids", Object::Array(vec![rref(3)])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (
            3,
            Object::Dictionary(dict([
                ("Type", name_obj("Page")),
                ("Parent", rref(2)),
                ("MediaBox", media_box()),
                ("Contents", rref(4)),
                (
                    "Resources",
                    Object::Dictionary(dict([(
                        "XObject",
                        Object::Dictionary(dict([("X1", rref(6))])),
                    )])),
                ),
            ])),
        ),
        (4, stream_obj(content)),
        (
            6,
            Object::Stream(StreamObj::new_encoded(img_dict, stream_bytes)),
        ),
    ];
    assemble_classic(&objects, ObjRef::new(1, 0))
}

/// A single-page document placing a solid-color raw Flate RGB image.
fn image_page_rgb(
    iw: i64,
    ih: i64,
    rgb: (u8, u8, u8),
    place_x: f64,
    place_y_topleft: f64,
    place_w: f64,
    place_h: f64,
) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((iw * ih * 3) as usize);
    for _ in 0..(iw * ih) {
        pixels.push(rgb.0);
        pixels.push(rgb.1);
        pixels.push(rgb.2);
    }
    let encoded = pdf_core::filters::flate::encode(&pixels);
    image_page(
        iw,
        ih,
        8,
        "DeviceRGB",
        Some(name_obj("FlateDecode")),
        encoded,
        place_x,
        place_y_topleft,
        place_w,
        place_h,
    )
}
