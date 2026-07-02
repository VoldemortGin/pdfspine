//! `MD-PAGE-*` — cross-page flow: long documents paginate without losing text
//! (the `insert_textbox` overflow TRAP is designed out).

mod common;

use common::{full_text, page_count, render};

#[test]
fn long_document_spans_multiple_pages_without_losing_text() {
    let mut md = String::new();
    for i in 0..120 {
        md.push_str(&format!(
            "Paragraph number {i} carries a marker token anchor{i} and some \
             extra words so each paragraph occupies real vertical space.\n\n"
        ));
    }
    let bytes = render(&md);
    assert!(
        page_count(&bytes) > 1,
        "120 paragraphs must not fit one page"
    );
    let text = full_text(&bytes);
    for i in 0..120 {
        assert!(
            text.contains(&format!("anchor{i}")),
            "paragraph {i} lost across page break"
        );
    }
}

#[test]
fn single_paragraph_longer_than_a_page_flows_line_by_line() {
    let md = "word ".repeat(3000);
    let bytes = render(&md);
    assert!(page_count(&bytes) > 1);
    assert_eq!(
        full_text(&bytes).matches("word").count(),
        3000,
        "wrapped paragraph must keep every word across pages"
    );
}

#[test]
fn heading_gap_does_not_dangle_at_page_top() {
    // Fill most of a page, then a heading: the heading moves to page 2 (or fits
    // cleanly) but its text must never be dropped.
    let filler = "filler line for vertical space\n\n".repeat(40);
    let bytes = render(&format!("{filler}\n# Late Heading\n\ntrailing body"));
    let text = full_text(&bytes);
    assert!(text.contains("Late Heading"));
    assert!(text.contains("trailing body"));
}

#[test]
fn blockquote_spanning_pages_keeps_all_lines() {
    // Separate quote paragraphs so each contributes real vertical space.
    let mut md = String::new();
    for i in 0..70 {
        md.push_str(&format!(
            "> quote paragraph {i} with enough extra words to fill a whole \
             wrapped line of text inside the quote area.\n>\n"
        ));
    }
    md.push_str("> final quote line");
    let bytes = render(&md);
    assert!(page_count(&bytes) > 1, "long quote must span pages");
    let text = full_text(&bytes);
    assert!(text.contains("quote paragraph 0"));
    assert!(text.contains("quote paragraph 69"));
    assert!(text.contains("final quote line"));
}
