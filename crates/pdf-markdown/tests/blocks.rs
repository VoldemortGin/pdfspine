//! `MD-BLOCK-*` — headings, paragraphs, inline styles, blockquotes, rules.

mod common;

use common::{assert_in_order, full_text, page_count, raw, render};

#[test]
fn headings_h1_to_h6_extract_in_order() {
    let md = "# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n";
    let bytes = render(md);
    assert_eq!(page_count(&bytes), 1);
    let text = full_text(&bytes);
    assert_in_order(&text, &["One", "Two", "Three", "Four", "Five", "Six"]);
    // Headings use the bold Base-14 face (F1 = Helvetica-Bold).
    let raw = raw(&bytes);
    assert!(raw.contains("/Helvetica-Bold"), "missing bold heading face");
}

#[test]
fn paragraph_text_round_trips_with_wrapping() {
    let sentence = "The quick brown fox jumps over the lazy dog and keeps running. ";
    let md = sentence.repeat(6); // long enough to force several wrapped lines
    let bytes = render(&md);
    assert_eq!(page_count(&bytes), 1);
    let text = full_text(&bytes);
    // Every word survives wrapping (count one marker word's occurrences).
    assert_eq!(text.matches("quick").count(), 6);
    assert_eq!(text.matches("running").count(), 6);
}

#[test]
fn inline_styles_render_with_the_right_faces() {
    let md = "normal **bold** *italic* ***both*** `mono` ~~gone~~ [link](https://example.com)";
    let bytes = render(md);
    let text = full_text(&bytes);
    for word in ["normal", "bold", "italic", "both", "mono", "gone", "link"] {
        assert!(text.contains(word), "missing {word:?} in {text:?}");
    }
    let raw = raw(&bytes);
    assert!(raw.contains("/Helvetica-Bold"), "bold face missing");
    assert!(raw.contains("/Helvetica-Oblique"), "italic face missing");
    assert!(
        raw.contains("/Helvetica-BoldOblique"),
        "bold-italic face missing"
    );
    assert!(raw.contains("/Courier"), "code face missing");
    // Strikethrough draws a stroked segment; links set a non-black fill color.
    assert!(raw.contains(" l S"), "strikethrough line op missing");
    assert!(
        raw.contains("0.05 0.25 0.7 rg"),
        "link fill color missing in content"
    );
}

#[test]
fn hard_break_splits_lines() {
    let bytes = render("first line\\\nsecond line");
    let text = full_text(&bytes);
    assert_in_order(&text, &["first line", "second line"]);
    // Two separate lines → the second baseline is a fresh text object; the
    // plain-text serialization keeps them on separate lines.
    let first = text.lines().position(|l| l.contains("first line"));
    let second = text.lines().position(|l| l.contains("second line"));
    assert!(first.is_some() && second.is_some() && first < second);
}

#[test]
fn blockquote_draws_bar_and_indents_text() {
    let bytes = render("before\n\n> quoted wisdom\n> continues here\n\nafter");
    let text = full_text(&bytes);
    assert_in_order(&text, &["before", "quoted wisdom", "after"]);
    // The bar is a filled rect in the quote-bar gray.
    assert!(
        raw(&bytes).contains("0.62 0.62 0.62 rg"),
        "quote bar fill missing"
    );
}

#[test]
fn nested_blockquote_draws_two_bars() {
    let bytes = render("> outer\n>> inner\n");
    let text = full_text(&bytes);
    assert_in_order(&text, &["outer", "inner"]);
    let raw = raw(&bytes);
    assert!(
        raw.matches("0.62 0.62 0.62 rg").count() >= 2,
        "expected two quote bars, got:\n{raw}"
    );
}

#[test]
fn horizontal_rule_strokes_a_full_width_line() {
    let bytes = render("above\n\n---\n\nbelow");
    assert_in_order(&full_text(&bytes), &["above", "below"]);
    // Rule color + a stroked line op.
    let raw = raw(&bytes);
    assert!(raw.contains("0.6 0.6 0.6 RG"), "rule stroke color missing");
    assert!(raw.contains(" l S"), "rule line op missing");
}

#[test]
fn unencodable_chars_degrade_to_question_marks_without_fallback_font() {
    // No `font`/`cjk_font` set → CJK degrades to '?' on the Base-14 path.
    let bytes = render("mix 你好 end");
    let text = full_text(&bytes);
    assert!(text.contains("mix"), "latin prefix lost");
    assert!(text.contains("??"), "expected ?? degradation, got {text:?}");
    assert!(text.contains("end"), "latin suffix lost");
}
