//! `MD-CODE-*` — fenced / indented code blocks (Courier + background rect).

mod common;

use common::{assert_in_order, full_text, page_count, raw, render};

#[test]
fn code_block_preserves_lines_and_draws_background() {
    let md = "```\nfn main() {\n    let x = 1;\n}\n```\n";
    let bytes = render(md);
    let text = full_text(&bytes);
    assert_in_order(&text, &["fn main() {", "let x = 1;", "}"]);
    let raw = raw(&bytes);
    assert!(
        raw.contains("0.95 0.95 0.95 rg"),
        "code background fill missing"
    );
    assert!(raw.contains("/Courier"), "code face missing");
}

#[test]
fn indented_code_block_works_too() {
    let bytes = render("para\n\n    indented code line\n");
    assert_in_order(&full_text(&bytes), &["para", "indented code line"]);
    assert!(raw(&bytes).contains("0.95 0.95 0.95 rg"));
}

#[test]
fn long_code_line_wraps_instead_of_overflowing() {
    let long = "x".repeat(400);
    let bytes = render(&format!("```\n{long}\n```\n"));
    assert_eq!(page_count(&bytes), 1);
    let text = full_text(&bytes);
    let total_x = text.matches('x').count();
    assert_eq!(total_x, 400, "wrapped code must not lose characters");
}

#[test]
fn huge_code_block_paginates_with_background_on_each_page() {
    let body: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let bytes = render(&format!("```\n{body}```\n"));
    assert!(page_count(&bytes) > 1, "200 code lines must span pages");
    let text = full_text(&bytes);
    assert!(text.contains("line 0"));
    assert!(
        text.contains("line 199"),
        "last code line lost across pages"
    );
    // One background rect per page chunk.
    assert!(
        raw(&bytes).matches("0.95 0.95 0.95 rg").count() >= 2,
        "each page chunk needs its own background"
    );
}

#[test]
fn inline_code_uses_courier_without_background() {
    let bytes = render("call `f(x)` now");
    assert!(full_text(&bytes).contains("f(x)"));
    let raw = raw(&bytes);
    assert!(raw.contains("/Courier"));
    assert!(
        !raw.contains("0.95 0.95 0.95 rg"),
        "inline code must not draw the block background"
    );
}
