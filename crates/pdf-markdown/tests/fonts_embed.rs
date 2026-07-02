//! `MD-FONT-*` — `font=` / `cjk_font=` embedding (CJK Option A) using the
//! repo's Liberation TTFs (CI-safe, no system font paths).

mod common;

use common::{full_text, raw, render_with};
use pdf_markdown::{markdown_to_pdf, Options};

const SANS: &[u8] = include_bytes!("../../pdf-fonts/fonts/liberation/LiberationSans-Regular.ttf");
const MONO: &[u8] = include_bytes!("../../pdf-fonts/fonts/liberation/LiberationMono-Regular.ttf");

#[test]
fn user_font_embeds_as_type0_and_round_trips() {
    let mut opts = Options::default();
    opts.font = Some(SANS.to_vec());
    let bytes = render_with("# Heading\n\nBody text in Liberation Sans.", &opts);
    let text = full_text(&bytes);
    assert!(text.contains("Heading"));
    assert!(text.contains("Body text in Liberation Sans."));
    let raw = raw(&bytes);
    assert!(raw.contains("/Type0"), "user font must embed as Type0");
    assert!(raw.contains("/Identity-H"), "Identity-H encoding expected");
    assert!(raw.contains("/FontFile2"), "font program must be embedded");
    assert!(raw.contains("/ToUnicode"), "ToUnicode map required");
}

#[test]
fn user_font_program_is_embedded_once_not_per_run() {
    // Many runs (bold/italic degrade to the same face) — exactly one FontFile2.
    let md = "**a** *b* c **d** *e* f\n\ng h i j k l m n o p\n";
    let mut opts = Options::default();
    opts.font = Some(SANS.to_vec());
    let bytes = render_with(md, &opts);
    let raw = raw(&bytes);
    assert_eq!(
        raw.matches("/FontFile2").count(),
        1,
        "font program must be written exactly once per document"
    );
}

#[test]
fn cjk_font_falls_back_per_character() {
    // Cyrillic is outside WinAnsi → falls back to the provided TTF (Liberation
    // covers Cyrillic), while Latin stays on the Base-14 body face.
    let mut opts = Options::default();
    opts.cjk_font = Some(SANS.to_vec());
    let bytes = render_with("Hello Привет world", &opts);
    let text = full_text(&bytes);
    assert!(text.contains("Hello"), "latin must stay");
    assert!(
        text.contains("Привет"),
        "fallback-font text must round-trip, got {text:?}"
    );
    assert!(text.contains("world"));
    let raw = raw(&bytes);
    assert!(raw.contains("/Type0"), "fallback font must embed as Type0");
    assert!(
        raw.contains("/Helvetica"),
        "latin text must remain on Base-14"
    );
}

#[test]
fn without_fallback_the_same_text_degrades_to_question_marks() {
    let bytes = render_with("Hello Привет world", &Options::default());
    let text = full_text(&bytes);
    assert!(text.contains("??"), "expected degradation, got {text:?}");
    assert!(!raw(&bytes).contains("/Type0"), "nothing should embed");
}

#[test]
fn user_font_falls_back_per_character_too() {
    // U+0237 "ȷ" exists in Liberation Sans but NOT in Liberation Mono: with
    // font=Mono + cjk_font=Sans it must switch face for that one character,
    // embedding BOTH programs.
    let mut opts = Options::default();
    opts.font = Some(MONO.to_vec());
    opts.cjk_font = Some(SANS.to_vec());
    let bytes = render_with("mix ȷ done", &opts);
    let text = full_text(&bytes);
    assert!(text.contains("mix"));
    assert!(text.contains("done"));
    assert!(
        text.contains('ȷ'),
        "fallback char must round-trip: {text:?}"
    );
    assert_eq!(
        raw(&bytes).matches("/FontFile2").count(),
        2,
        "both the user font and the fallback font must be embedded"
    );
}

#[test]
fn bad_font_bytes_yield_typed_error_not_panic() {
    let mut opts = Options::default();
    opts.font = Some(b"this is not a font program".to_vec());
    let err = markdown_to_pdf("x", &opts).expect_err("bad TTF must fail");
    assert!(matches!(err, pdf_core::error::Error::Unsupported(_)));

    let mut opts = Options::default();
    opts.cjk_font = Some(vec![0u8; 16]);
    let err = markdown_to_pdf("x", &opts).expect_err("bad fallback TTF must fail");
    assert!(matches!(err, pdf_core::error::Error::Unsupported(_)));
}
