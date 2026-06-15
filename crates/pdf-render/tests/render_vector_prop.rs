//! RENDER-VEC-PROP-* — property tests for vector rasterization (M6a).
//!
//! Arbitrary paths fed through fill / stroke / clip must **never panic** and the
//! converted [`pdf_image::Pixmap`] must always carry the canvas dimensions and a
//! correctly-sized sample buffer (PRD §8.1 — arbitrary input never panics).

use proptest::prelude::*;

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_image::pixmap::Colorspace;
use pdf_text::model::{DrawPath, PaintKind, PathItem};

use pdf_render::canvas::Canvas;
use pdf_render::vector::{fill_path, set_clip, stroke_path, Paint, StrokeStyle};

/// Coordinates spanning the interesting range (well outside the canvas, plus a
/// few non-finite-ish extremes via huge magnitudes).
fn coord() -> impl Strategy<Value = f64> {
    prop_oneof![
        -1.0e6..1.0e6_f64,
        Just(0.0),
        Just(f64::MAX),
        Just(f64::MIN),
        Just(1.0e30),
    ]
}

fn point() -> impl Strategy<Value = Point> {
    (coord(), coord()).prop_map(|(x, y)| Point::new(x, y))
}

fn path_item() -> impl Strategy<Value = PathItem> {
    prop_oneof![
        (point(), point()).prop_map(|(a, b)| PathItem::Line(a, b)),
        (point(), point(), point(), point()).prop_map(|(a, b, c, d)| PathItem::Curve(a, b, c, d)),
        (coord(), coord(), coord(), coord())
            .prop_map(|(x0, y0, x1, y1)| PathItem::Rect(Rect::new(x0, y0, x1, y1))),
    ]
}

fn items() -> impl Strategy<Value = Vec<PathItem>> {
    prop::collection::vec(path_item(), 0..24)
}

fn matrix() -> impl Strategy<Value = Matrix> {
    (
        -10.0..10.0_f64,
        -5.0..5.0,
        -5.0..5.0,
        -10.0..10.0,
        -50.0..50.0,
        -50.0..50.0,
    )
        .prop_map(|(a, b, c, d, e, f)| Matrix::new(a, b, c, d, e, f))
}

fn drawpath(items: Vec<PathItem>, even_odd: bool, width: f64) -> DrawPath {
    DrawPath {
        kind: PaintKind::FillStroke,
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        color: Some(0x112233),
        fill: Some(0x445566),
        width,
        dashes: String::new(),
        close_path: false,
        even_odd,
        items,
    }
}

proptest! {
    /// RENDER-VEC-PROP-FILL: arbitrary path + arbitrary CTM never panics, and the
    /// output pixmap has the canvas dims + a correctly sized buffer.
    #[test]
    fn prop_fill_never_panics(
        items in items(),
        ctm in matrix(),
        even_odd in any::<bool>(),
        rgb in any::<u32>(),
        alpha in any::<u8>(),
    ) {
        let mut c = Canvas::blank(17, 13, Matrix::scale(1.0, -1.0), Colorspace::Rgb, false).unwrap();
        let dp = drawpath(items, even_odd, 0.0);
        let paint = Paint::from_rgb_alpha(rgb & 0x00FF_FFFF, alpha);
        // Must return Ok and never panic, regardless of geometry.
        fill_path(&mut c, &dp, paint, ctm, even_odd).unwrap();
        let pm = c.into_pixmap().unwrap();
        prop_assert_eq!((pm.width, pm.height), (17, 13));
        prop_assert_eq!(pm.samples.len(), 17 * 13 * pm.n as usize);
    }

    /// RENDER-VEC-PROP-STROKE: arbitrary stroke geometry/width never panics.
    #[test]
    fn prop_stroke_never_panics(
        items in items(),
        ctm in matrix(),
        width in 0.0..1000.0_f64,
        rgb in any::<u32>(),
    ) {
        let mut c = Canvas::blank(11, 19, Matrix::IDENTITY, Colorspace::Gray, true).unwrap();
        let dp = drawpath(items, false, width);
        let style = StrokeStyle { width: width as f32, ..Default::default() };
        stroke_path(&mut c, &dp, Paint::from_rgb(rgb & 0x00FF_FFFF), &style, ctm).unwrap();
        let pm = c.into_pixmap().unwrap();
        prop_assert_eq!((pm.width, pm.height), (11, 19));
        prop_assert_eq!(pm.n, 2); // Gray + alpha
        prop_assert_eq!(pm.samples.len(), 11 * 19 * 2);
    }

    /// RENDER-VEC-PROP-CLIP: arbitrary clip paths never panic and never enlarge
    /// the drawable region — a fill after clipping is still bounded by the
    /// canvas (we only assert no-panic + dims here; coverage shrink is covered by
    /// the deterministic clip test).
    #[test]
    fn prop_clip_never_panics(
        clip_items in items(),
        fill_items in items(),
        ctm in matrix(),
        even_odd in any::<bool>(),
    ) {
        let mut c = Canvas::blank(23, 7, Matrix::IDENTITY, Colorspace::Cmyk, false).unwrap();
        set_clip(&mut c, &clip_items, ctm, even_odd).unwrap();
        let dp = drawpath(fill_items, even_odd, 0.0);
        fill_path(&mut c, &dp, Paint::from_rgb(0xABCDEF), ctm, even_odd).unwrap();
        let pm = c.into_pixmap().unwrap();
        prop_assert_eq!((pm.width, pm.height), (23, 7));
        prop_assert_eq!(pm.colorspace, Colorspace::Cmyk);
        prop_assert_eq!(pm.samples.len(), 23 * 7 * 4);
    }

    /// RENDER-VEC-PROP-SINGULAR-CTM: a non-invertible (singular) CTM is tolerated
    /// (no panic, valid output).
    #[test]
    fn prop_singular_ctm_ok(items in items()) {
        // A rank-1 (singular) matrix: both rows collinear.
        let singular = Matrix::new(2.0, 4.0, 1.0, 2.0, 3.0, 5.0);
        let mut c = Canvas::blank(9, 9, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
        let dp = drawpath(items, false, 2.0);
        fill_path(&mut c, &dp, Paint::from_rgb(0x010203), singular, false).unwrap();
        let pm = c.into_pixmap().unwrap();
        prop_assert_eq!((pm.width, pm.height), (9, 9));
    }
}
