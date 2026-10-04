//! Layout and table detection under extreme text-state / CTM values: glyph
//! geometry is content-controlled, so `Tz`, `Tf`, `Tm`, `cm` at absurd or
//! non-finite magnitudes must neither panic nor size an allocation.

mod common;

use common::{run_with_font, winansi_type1};
use pdf_core::geom::Rect;
use pdf_core::Object;
use pdf_text::layout::{page_transform, textpage_from_glyphs};
use pdf_text::tables::{drawings_to_device, find_tables, Strategy, TableOptions};
use pdf_text::{serialize, words};

fn font() -> Object {
    winansi_type1("Helvetica", 32, &[500; 96])
}

fn letter() -> Rect {
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

/// Four ordinary body lines (enough lines/glyphs to reach the column-gutter
/// detector), then `tail` (the extreme operator) and one more shown line.
fn page_with(tail: &str) -> Vec<u8> {
    format!(
        "BT /F1 12 Tf 14 TL 72 720 Td \
         (Reading order matters when a document has multiple lines.) Tj T* \
         (Each line should appear in the same sequence it was written.) Tj T* \
         (A faithful extractor preserves top to bottom ordering.) Tj T* \
         (The fourth line keeps the page comfortably multi-line.) Tj T* \
         {tail} (This line carries the extreme value.) Tj ET"
    )
    .into_bytes()
}

/// Runs the full text + table pipeline on `content` and returns the plain text.
fn pipeline(content: &[u8]) -> String {
    let res = run_with_font(font(), content);
    let tp = textpage_from_glyphs(&res.glyphs, &res.images, letter(), 0);
    let text = serialize::to_text(&tp, 0);
    let w = words(&tp);
    let dr = drawings_to_device(&res.drawings, &page_transform(letter(), 0));
    for s in [Strategy::Lines, Strategy::Text] {
        let _ = find_tables(&tp, &w, &dr, &TableOptions::with_strategy(s));
    }
    text
}

#[test]
fn astronomical_horizontal_scaling_does_not_panic() {
    // `1e30 Tz` made one glyph ~1e29 pt wide; the gutter histogram sized its
    // bin vector from that width (`capacity overflow` panic).
    let text = pipeline(&page_with("1e30 Tz"));
    assert!(text.contains("Reading order matters"), "{text}");
}

#[test]
fn extreme_text_state_and_ctm_values_are_survivable() {
    for tail in [
        "1e8 Tz",
        "-1e30 Tz",
        "/F1 1e30 Tf",
        "/F1 -1e30 Tf",
        "1e30 Tc 1e30 Tw",
        "1e30 TL T*",
        "1e30 Ts",
        "1e300 0 0 1e300 0 0 Tm",
        "11 0 0 11 -2147483649 541 Tm",
        "1e308 0 0 1e308 1e308 1e308 Tm",
        "0 0 0 0 0 0 Tm",
        "ET 1e300 0 0 1e300 0 0 cm BT /F1 12 Tf",
        "ET 1e308 1e308 1e308 1e308 1e308 1e308 cm BT /F1 12 Tf",
    ] {
        // A giant glyph may legitimately join (and reorder) a neighbouring
        // line's cluster; the ordinary lines in between must survive intact.
        let text = pipeline(&page_with(tail));
        assert!(
            text.contains("A faithful extractor preserves top to bottom ordering."),
            "{tail}: {text}"
        );
    }
}

#[test]
fn extreme_ruling_coordinates_are_survivable() {
    let content = b"0.5 w 50 100 m 50 700 l S 1e300 100 m -1e300 100 l S \
        1e308 1e308 m -1e308 -1e308 l S 60 1e300 m 60 -1e300 l S \
        1e300 1e300 1e300 1e300 re S 100 200 300 1e308 re f";
    let mut page = page_with("1e30 Tz");
    page.extend_from_slice(b" ");
    page.extend_from_slice(content);
    let _ = pipeline(&page);
}
