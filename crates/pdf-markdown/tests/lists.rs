//! `MD-LIST-*` — ordered / unordered / nested lists and GFM task lists.

mod common;

use common::{assert_in_order, full_text, raw, render};

#[test]
fn unordered_list_items_extract_in_order() {
    let bytes = render("- alpha\n- beta\n- gamma\n");
    assert_in_order(&full_text(&bytes), &["alpha", "beta", "gamma"]);
    // Bullets are filled circles (four Bézier curves + fill).
    let raw = raw(&bytes);
    assert!(raw.contains(" c\n"), "bullet curves missing");
}

#[test]
fn ordered_list_renders_ordinal_markers() {
    let bytes = render("1. first\n2. second\n3. third\n");
    let text = full_text(&bytes);
    assert_in_order(&text, &["1.", "first", "2.", "second", "3.", "third"]);
}

#[test]
fn ordered_list_honors_start_number() {
    let bytes = render("4. fourth\n5. fifth\n");
    assert_in_order(&full_text(&bytes), &["4.", "fourth", "5.", "fifth"]);
}

#[test]
fn nested_lists_keep_item_order() {
    let md = "- outer one\n  - inner a\n  - inner b\n- outer two\n";
    let bytes = render(md);
    assert_in_order(
        &full_text(&bytes),
        &["outer one", "inner a", "inner b", "outer two"],
    );
}

#[test]
fn ordered_inside_unordered_nesting() {
    let md = "- top\n  1. one\n  2. two\n";
    let bytes = render(md);
    assert_in_order(&full_text(&bytes), &["top", "1.", "one", "2.", "two"]);
}

#[test]
fn task_list_draws_checkboxes() {
    let bytes = render("- [x] done task\n- [ ] open task\n");
    let text = full_text(&bytes);
    assert_in_order(&text, &["done task", "open task"]);
    let raw = raw(&bytes);
    // Two stroked checkbox squares…
    assert!(
        raw.matches("re S").count() >= 2,
        "expected two checkbox rects:\n{raw}"
    );
    // …and one check mark (two stroked segments beyond strikethrough-free text).
    assert!(raw.contains(" l S"), "check mark strokes missing");
}

#[test]
fn multi_paragraph_list_item_stays_indented() {
    let md = "1. first para\n\n   second para of the same item\n\n2. next item\n";
    let bytes = render(md);
    assert_in_order(
        &full_text(&bytes),
        &["first para", "second para of the same item", "next item"],
    );
}
