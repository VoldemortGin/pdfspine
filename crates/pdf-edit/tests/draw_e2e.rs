//! M4a vector drawing — `DRAW-*` / `SHAPE-*` (PRD §8.8).
//!
//! `get_drawings` (structured vector extraction) lands in M4d, so the oracle
//! here is the **decoded page content bytes** after save → reopen: the expected
//! path-construction + paint operators must be present, coordinates converted
//! from top-left space, and existing content preserved.

mod common;

use common::{blank_page, open, page_content_bytes, save_reopen};

use pdf_core::geom::{Point, Rect};
use pdf_edit::{
    draw_bezier, draw_circle, draw_curve, draw_line, draw_oval, draw_polyline, draw_rect, Color,
    Shape,
};

/// The decoded content of page 0 after save → reopen, as a UTF-8 string.
fn content_after(doc: &pdf_core::DocumentStore) -> String {
    let re = save_reopen(doc);
    String::from_utf8_lossy(&page_content_bytes(&re, 0)).to_string()
}

// === DRAW-* ===============================================================

/// `DRAW-LINE-001`: `draw_line` emits `m … l … S`.
#[test]
fn draw_line_001() {
    let doc = open(&blank_page(612, 792));
    draw_line(
        &doc,
        0,
        Point::new(10.0, 10.0),
        Point::new(100.0, 50.0),
        Color::BLACK,
        2.0,
    )
    .unwrap();
    let c = content_after(&doc);
    assert!(c.contains(" m"), "no moveto: {c}");
    assert!(c.contains(" l"), "no lineto: {c}");
    assert!(c.contains("S"), "no stroke: {c}");
    // y is flipped: top-left (10,10) → user y = 792-10 = 782.
    assert!(c.contains("10 782 m"), "expected flipped origin in {c}");
}

/// `DRAW-RECT-001`: `draw_rect` emits `re` + stroke, coordinates flipped.
#[test]
fn draw_rect_001() {
    let doc = open(&blank_page(612, 792));
    draw_rect(
        &doc,
        0,
        Rect::new(50.0, 50.0, 150.0, 120.0),
        Some(Color::BLACK),
        None,
        1.0,
    )
    .unwrap();
    let c = content_after(&doc);
    assert!(c.contains(" re"), "no rectangle op: {c}");
    assert!(c.contains("S"), "no stroke: {c}");
    // width 100, height 70; lower-left user y = 792 - 120 = 672.
    assert!(c.contains("50 672 100 70 re"), "wrong re params: {c}");
}

/// `DRAW-CIRCLE-001`: `draw_circle` emits four cubic Béziers closed with `h`.
#[test]
fn draw_circle_001() {
    let doc = open(&blank_page(612, 792));
    draw_circle(
        &doc,
        0,
        Point::new(100.0, 100.0),
        40.0,
        Some(Color::BLACK),
        None,
        1.0,
    )
    .unwrap();
    let c = content_after(&doc);
    assert_eq!(c.matches(" c\n").count(), 4, "expected 4 Béziers in {c}");
    assert!(c.contains("h"), "circle not closed: {c}");
}

/// `DRAW-OVAL-001`: `draw_oval` emits four Béziers fitting the rect.
#[test]
fn draw_oval_001() {
    let doc = open(&blank_page(612, 792));
    draw_oval(
        &doc,
        0,
        Rect::new(50.0, 50.0, 250.0, 150.0),
        Some(Color::BLACK),
        None,
        1.0,
    )
    .unwrap();
    let c = content_after(&doc);
    assert_eq!(c.matches(" c\n").count(), 4, "expected 4 Béziers in {c}");
}

/// `DRAW-BEZIER-001`: `draw_bezier` emits a single `c`.
#[test]
fn draw_bezier_001() {
    let doc = open(&blank_page(612, 792));
    draw_bezier(
        &doc,
        0,
        Point::new(0.0, 0.0),
        Point::new(20.0, 80.0),
        Point::new(60.0, 80.0),
        Point::new(80.0, 0.0),
        Color::BLACK,
        1.0,
    )
    .unwrap();
    let c = content_after(&doc);
    assert_eq!(c.matches(" c\n").count(), 1, "expected 1 Bézier in {c}");
}

/// `DRAW-POLYLINE-001`: `draw_polyline` emits `m` + chained `l`; `draw_curve`
/// emits cubic `c` segments.
#[test]
fn draw_polyline_001() {
    let doc = open(&blank_page(612, 792));
    draw_polyline(
        &doc,
        0,
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 20.0),
            Point::new(30.0, 5.0),
            Point::new(50.0, 40.0),
        ],
        Color::BLACK,
        1.0,
    )
    .unwrap();
    let c = content_after(&doc);
    assert_eq!(c.matches(" m\n").count(), 1, "one moveto expected: {c}");
    assert_eq!(c.matches(" l\n").count(), 3, "three linetos expected: {c}");

    // draw_curve over the same points → cubic segments.
    let doc2 = open(&blank_page(612, 792));
    draw_curve(
        &doc2,
        0,
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 20.0),
            Point::new(30.0, 5.0),
            Point::new(50.0, 40.0),
        ],
        Color::BLACK,
        1.0,
    )
    .unwrap();
    let c2 = content_after(&doc2);
    assert!(c2.matches(" c\n").count() >= 2, "curve segments: {c2}");
}

/// `DRAW-FILL-001`: fill color → `rg` + `f`; stroke color → `RG` + `S`; both →
/// `B`.
#[test]
fn draw_fill_001() {
    // Fill only.
    let doc = open(&blank_page(612, 792));
    draw_rect(
        &doc,
        0,
        Rect::new(10.0, 10.0, 60.0, 60.0),
        None,
        Some(Color::new(1.0, 0.0, 0.0)),
        1.0,
    )
    .unwrap();
    let c = content_after(&doc);
    assert!(c.contains("1 0 0 rg"), "no fill color: {c}");
    assert!(c.contains("f\n"), "no fill paint: {c}");

    // Both stroke + fill → `B`.
    let doc2 = open(&blank_page(612, 792));
    draw_rect(
        &doc2,
        0,
        Rect::new(10.0, 10.0, 60.0, 60.0),
        Some(Color::new(0.0, 0.0, 1.0)),
        Some(Color::new(1.0, 0.0, 0.0)),
        1.0,
    )
    .unwrap();
    let c2 = content_after(&doc2);
    assert!(c2.contains("0 0 1 RG"), "no stroke color: {c2}");
    assert!(c2.contains("1 0 0 rg"), "no fill color: {c2}");
    assert!(c2.contains("B\n"), "no fill+stroke paint: {c2}");
}

/// `DRAW-WIDTH-001`: line width emits `w`; dashes emit `d` (via the Shape API).
#[test]
fn draw_width_001() {
    let doc = open(&blank_page(612, 792));
    let mut s = Shape::new(&doc, 0).unwrap();
    s.draw_line(Point::new(0.0, 0.0), Point::new(50.0, 50.0));
    s.finish(Some(Color::BLACK), None, 3.5, Some("[3 2] 0"), false, false);
    s.commit().unwrap();
    let c = content_after(&doc);
    assert!(c.contains("3.5 w"), "no width op: {c}");
    assert!(c.contains("[3 2] 0 d"), "no dash op: {c}");
}

// === SHAPE-* ==============================================================

/// `SHAPE-001`: a `Shape` accumulates several path ops then `finish` + `commit`
/// emits one balanced `q … Q` chunk containing all the operators.
#[test]
fn shape_001_accumulate_commit() {
    let doc = open(&blank_page(612, 792));
    let mut s = Shape::new(&doc, 0).unwrap();
    s.draw_line(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
    s.draw_rect(Rect::new(20.0, 20.0, 40.0, 40.0));
    s.draw_circle(Point::new(80.0, 80.0), 15.0);
    s.finish(Some(Color::BLACK), None, 1.0, None, false, false);
    s.commit().unwrap();

    let c = content_after(&doc);
    assert!(c.contains(" l"), "missing line: {c}");
    assert!(c.contains(" re"), "missing rect: {c}");
    assert_eq!(c.matches(" c\n").count(), 4, "missing circle Béziers: {c}");
    assert_eq!(c.matches("q\n").count(), 1, "expected one q group: {c}");
    assert_eq!(c.matches("Q\n").count(), 1, "expected one Q group: {c}");
}

/// `SHAPE-002`: multiple `finish` blocks with different colors are all
/// committed.
#[test]
fn shape_002_multi_finish() {
    let doc = open(&blank_page(612, 792));
    let mut s = Shape::new(&doc, 0).unwrap();
    s.draw_rect(Rect::new(10.0, 10.0, 40.0, 40.0));
    s.finish(
        None,
        Some(Color::new(1.0, 0.0, 0.0)),
        1.0,
        None,
        false,
        false,
    );
    s.draw_circle(Point::new(100.0, 100.0), 20.0);
    s.finish(
        Some(Color::new(0.0, 0.0, 1.0)),
        None,
        2.0,
        None,
        false,
        false,
    );
    s.commit().unwrap();

    let c = content_after(&doc);
    assert!(c.contains("1 0 0 rg"), "first fill color missing: {c}");
    assert!(c.contains("0 0 1 RG"), "second stroke color missing: {c}");
    assert!(c.contains(" re"), "rect missing: {c}");
    assert_eq!(c.matches(" c\n").count(), 4, "circle missing: {c}");
}

// === INSERT-PROP-* (draw path) ============================================

/// `INSERT-PROP-001`: drawing onto a page with existing text leaves the text
/// extractable (no corruption).
#[test]
fn insert_prop_001_preserves_text() {
    let doc = open(&common::MultiPage::new(&["AAA"]).build());
    draw_rect(
        &doc,
        0,
        Rect::new(10.0, 10.0, 100.0, 100.0),
        Some(Color::BLACK),
        None,
        1.0,
    )
    .unwrap();
    let re = save_reopen(&doc);
    let text = common::page_text(&re, 0);
    assert!(text.contains("AAA"), "existing text lost: {text:?}");
}

/// `INSERT-PROP-004`: repeated insertions accumulate (two draws → two groups).
#[test]
fn insert_prop_004_repeated_insertions() {
    let doc = open(&blank_page(612, 792));
    draw_line(
        &doc,
        0,
        Point::new(0.0, 0.0),
        Point::new(10.0, 10.0),
        Color::BLACK,
        1.0,
    )
    .unwrap();
    draw_line(
        &doc,
        0,
        Point::new(20.0, 20.0),
        Point::new(30.0, 30.0),
        Color::BLACK,
        1.0,
    )
    .unwrap();
    let c = content_after(&doc);
    // Two separate stroked segments → two linetos.
    assert_eq!(c.matches(" l\n").count(), 2, "expected two segments: {c}");
}
