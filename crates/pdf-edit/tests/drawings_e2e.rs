//! `DRAWINGS-*` — vector path extraction via `get_drawings` / `get_cdrawings`
//! (PRD §8.8). Author vector content with the M4a `Shape` / `draw_*` helpers,
//! then extract and assert item geometry / type / color / rect.

mod common;

use common::{blank_page, open};

use pdf_core::geom::{Point, Rect};
use pdf_edit::color::Color;
use pdf_edit::drawings::{DrawItem, Drawing};
use pdf_edit::{draw_line, draw_rect, get_cdrawings, get_drawings, Shape};
use pdf_text::PaintKind;

/// Packs an RGB `Color` to `0x00RRGGBB` (the interpreter's color encoding).
fn pack(c: Color) -> u32 {
    let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
}

fn only(drawings: &[Drawing]) -> &Drawing {
    assert_eq!(drawings.len(), 1, "expected exactly one drawing");
    &drawings[0]
}

#[test]
fn drawings_001_stroked_rect_has_re_item_and_color() {
    // DRAWINGS-001: draw_rect (stroke) → type "s" with an ("re", rect) item.
    let doc = open(&blank_page(612, 792));
    let red = Color::new(1.0, 0.0, 0.0);
    draw_rect(
        &doc,
        0,
        Rect::new(100.0, 100.0, 200.0, 150.0),
        Some(red),
        None,
        2.0,
    )
    .unwrap();

    let drawings = get_drawings(&doc, 0);
    let d = only(&drawings);
    assert_eq!(d.kind, PaintKind::Stroke);
    assert_eq!(d.type_str(), "s");
    assert_eq!(d.color, Some(pack(red)));
    assert_eq!(d.fill, None);
    assert!((d.width - 2.0).abs() < 1e-6);
    assert_eq!(d.items.len(), 1);
    assert!(matches!(d.items[0], DrawItem::Rect(_)));
    // The drawing rect spans (device space) ~ width 100, height 50.
    assert!((d.rect.width() - 100.0).abs() < 1.0);
    assert!((d.rect.height() - 50.0).abs() < 1.0);
}

#[test]
fn drawings_002_line_has_line_item() {
    // DRAWINGS-002: draw_line → type "s" with an ("l", p1, p2) item.
    let doc = open(&blank_page(612, 792));
    draw_line(
        &doc,
        0,
        Point::new(50.0, 50.0),
        Point::new(250.0, 50.0),
        Color::new(0.0, 0.0, 1.0),
        1.0,
    )
    .unwrap();

    let drawings = get_drawings(&doc, 0);
    let d = only(&drawings);
    assert_eq!(d.kind, PaintKind::Stroke);
    // draw_line emits `m … l`; the captured item is the single segment.
    assert!(d.items.iter().any(|it| matches!(it, DrawItem::Line(..))));
    assert!((d.rect.width() - 200.0).abs() < 1.0);
}

#[test]
fn drawings_003_filled_rect_sets_fill_only() {
    // DRAWINGS-003: filled rect → type "f" with fill set, color None.
    let doc = open(&blank_page(612, 792));
    let green = Color::new(0.0, 1.0, 0.0);
    draw_rect(
        &doc,
        0,
        Rect::new(10.0, 10.0, 60.0, 60.0),
        None,
        Some(green),
        1.0,
    )
    .unwrap();

    let d = get_drawings(&doc, 0);
    let d = only(&d);
    assert_eq!(d.kind, PaintKind::Fill);
    assert_eq!(d.type_str(), "f");
    assert_eq!(d.fill, Some(pack(green)));
    assert_eq!(d.color, None);
}

#[test]
fn drawings_004_fill_and_stroke() {
    // DRAWINGS-004: fill+stroke rect → type "fs" with both colors.
    let doc = open(&blank_page(612, 792));
    let stroke = Color::new(1.0, 0.0, 0.0);
    let fill = Color::new(0.0, 0.0, 1.0);
    draw_rect(
        &doc,
        0,
        Rect::new(10.0, 10.0, 60.0, 60.0),
        Some(stroke),
        Some(fill),
        1.0,
    )
    .unwrap();

    let d = get_drawings(&doc, 0);
    let d = only(&d);
    assert_eq!(d.kind, PaintKind::FillStroke);
    assert_eq!(d.type_str(), "fs");
    assert_eq!(d.color, Some(pack(stroke)));
    assert_eq!(d.fill, Some(pack(fill)));
}

#[test]
fn drawings_005_even_odd_and_close_path() {
    // DRAWINGS-005: even-odd fill (f*) sets even_odd; closed polyline sets
    // close_path.
    let doc = open(&blank_page(612, 792));
    let mut s = Shape::new(&doc, 0).unwrap();
    s.draw_polyline(&[
        Point::new(10.0, 10.0),
        Point::new(60.0, 10.0),
        Point::new(35.0, 60.0),
    ]);
    // fill (even-odd) + close path.
    s.finish(None, Some(Color::new(0.5, 0.5, 0.5)), 1.0, None, true, true);
    s.commit().unwrap();

    let d = get_drawings(&doc, 0);
    let d = only(&d);
    assert_eq!(d.kind, PaintKind::Fill);
    assert!(d.even_odd, "f* must set even_odd");
    assert!(d.close_path, "h must set close_path");
}

#[test]
fn drawings_006_cdrawings_raw_user_space() {
    // DRAWINGS-006: get_cdrawings keeps user-space (y-up) geometry — for a rect
    // drawn at top-left (100,100)-(200,150), the user-space y differs from the
    // device-space y of get_drawings (flipped about the page height).
    let doc = open(&blank_page(612, 792));
    draw_rect(
        &doc,
        0,
        Rect::new(100.0, 100.0, 200.0, 150.0),
        Some(Color::new(0.0, 0.0, 0.0)),
        None,
        1.0,
    )
    .unwrap();

    let dev = get_drawings(&doc, 0);
    let raw = get_cdrawings(&doc, 0);
    assert_eq!(dev.len(), 1);
    assert_eq!(raw.len(), 1);
    // Same geometry shape (a rect item), same width/height.
    assert!((dev[0].rect.width() - raw[0].rect.width()).abs() < 1e-6);
    assert!((dev[0].rect.height() - raw[0].rect.height()).abs() < 1e-6);
    // But the vertical position differs (device y-down vs user y-up).
    assert!(
        (dev[0].rect.y0 - raw[0].rect.y0).abs() > 1.0,
        "device vs user vertical frame must differ"
    );
}

#[test]
fn drawings_007_curve_item() {
    // DRAWINGS-007: a cubic Bézier is captured as a ("c", …) item.
    let doc = open(&blank_page(612, 792));
    let mut s = Shape::new(&doc, 0).unwrap();
    s.draw_bezier(
        Point::new(10.0, 10.0),
        Point::new(30.0, 80.0),
        Point::new(70.0, 80.0),
        Point::new(90.0, 10.0),
    );
    s.finish(
        Some(Color::new(0.0, 0.0, 0.0)),
        None,
        1.0,
        None,
        false,
        false,
    );
    s.commit().unwrap();

    let d = get_drawings(&doc, 0);
    let d = only(&d);
    assert!(
        d.items.iter().any(|it| matches!(it, DrawItem::Curve(..))),
        "expected a curve item"
    );
}

#[test]
fn drawings_prop_001_empty_page_no_drawings() {
    // DRAWINGS-PROP-001: empty page → no drawings; never panics.
    let doc = open(&blank_page(612, 792));
    assert!(get_drawings(&doc, 0).is_empty());
    assert!(get_cdrawings(&doc, 0).is_empty());
    // Out-of-range page → empty, no panic.
    assert!(get_drawings(&doc, 99).is_empty());
}
