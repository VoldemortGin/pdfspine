//! RENDER-VEC-* — deterministic vector rasterization tests (M6a).
//!
//! Each test builds a [`Canvas`] directly, paints geometry via the public
//! [`pdf_render::vector`] API, converts to a [`pdf_image::Pixmap`], and asserts
//! exact sample values at chosen `(x, y)` device pixels. The base transform is
//! the PDF→device y-flip (PDF bottom-left origin → pixmap top-left).

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_text::model::{DrawPath, PaintKind, PathItem};

use pdf_render::canvas::Canvas;
use pdf_render::vector::{
    fill_path, set_clip, stroke_path, BlendMode, LineCapStyle, LineJoinStyle, Paint, StrokeStyle,
};

/// The PDF→device base transform for a page of pixel height `h`: identity scale
/// with a y-flip (`y_device = h - y_user`).
fn yflip(h: u32) -> Matrix {
    Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, f64::from(h))
}

/// A blank opaque-RGB canvas filled with white, sized `w × h`, y-flipped.
fn white_canvas(w: u32, h: u32) -> Canvas {
    let mut c = Canvas::blank(w, h, yflip(h), Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    c
}

/// Reads the RGB triple at device `(x, y)` from an RGB pixmap.
fn rgb_at(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    assert_eq!(pm.colorspace, Colorspace::Rgb);
    let idx = (y as usize) * pm.stride + (x as usize) * pm.n as usize;
    (pm.samples[idx], pm.samples[idx + 1], pm.samples[idx + 2])
}

/// A fill-only `DrawPath` from a list of items + fill color.
fn fill_drawpath(items: Vec<PathItem>, fill: u32, even_odd: bool) -> DrawPath {
    DrawPath {
        kind: PaintKind::Fill,
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        color: None,
        fill: Some(fill),
        width: 0.0,
        dashes: String::new(),
        close_path: false,
        even_odd,
        items,
    }
}

/// A single user-space rectangle path item.
fn rect_item(x0: f64, y0: f64, x1: f64, y1: f64) -> PathItem {
    PathItem::Rect(Rect::new(x0, y0, x1, y1))
}

// === RENDER-VEC-FILL-RECT ================================================

/// A filled rect at known *device* coords: inside pixels equal the fill color,
/// outside pixels stay the white background. (Identity base transform so user
/// space == device space, no y-flip confusion.)
#[test]
fn render_vec_fill_rect_solid() {
    let mut c = Canvas::blank(20, 20, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    // Device-space rect [4,4 .. 16,16], filled red.
    let dp = fill_drawpath(vec![rect_item(4.0, 4.0, 16.0, 16.0)], 0xFF0000, false);
    fill_path(
        &mut c,
        &dp,
        Paint::from_rgb(0xFF0000),
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();

    // Center is solid red.
    assert_eq!(rgb_at(&pm, 10, 10), (255, 0, 0));
    // Well inside is solid red.
    assert_eq!(rgb_at(&pm, 6, 6), (255, 0, 0));
    // Outside (corner) is white background.
    assert_eq!(rgb_at(&pm, 1, 1), (255, 255, 255));
    assert_eq!(rgb_at(&pm, 18, 18), (255, 255, 255));
}

// === RENDER-VEC-YFLIP ====================================================

/// The y-flip is correct: a rect at the *bottom* of PDF user space (small y)
/// lands at the *bottom rows* of the device pixmap (large device y).
#[test]
fn render_vec_yflip_bottom_to_bottom() {
    let h = 20;
    let mut c = white_canvas(20, h);
    // User-space rect near the bottom: y in [2,6].
    let dp = fill_drawpath(vec![rect_item(4.0, 2.0, 16.0, 6.0)], 0x0000FF, false);
    fill_path(
        &mut c,
        &dp,
        Paint::from_rgb(0x0000FF),
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();

    // Device y for user y=4 is h-4 = 16 → bottom region is blue.
    assert_eq!(rgb_at(&pm, 10, 16), (0, 0, 255));
    // The top region (small device y) is untouched white.
    assert_eq!(rgb_at(&pm, 10, 3), (255, 255, 255));
}

// === RENDER-VEC-EVENODD vs NONZERO =======================================

/// A donut: an outer rect with an inner rect wound the *same* direction.
/// Even-odd leaves the inner hole transparent (white background); nonzero
/// fills the whole outer rect. Both rects are CCW (same winding) so the rules
/// diverge in the hole.
#[test]
fn render_vec_even_odd_vs_nonzero_donut() {
    // Outer [2,2..18,18], inner [7,7..13,13]. tiny-skia `push_rect` always
    // emits a CW rect, so two nested push_rects share winding → with EvenOdd
    // the inner cancels (hole), with Winding it stays filled.
    let items = vec![
        rect_item(2.0, 2.0, 18.0, 18.0),
        rect_item(7.0, 7.0, 13.0, 13.0),
    ];

    // Even-odd: hole at the center.
    let mut c_eo = Canvas::blank(20, 20, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c_eo.fill_background([255, 255, 255, 255]);
    let dp_eo = fill_drawpath(items.clone(), 0x000000, true);
    fill_path(
        &mut c_eo,
        &dp_eo,
        Paint::from_rgb(0x000000),
        Matrix::IDENTITY,
        true,
    )
    .unwrap();
    let pm_eo = c_eo.into_pixmap().unwrap();
    assert_eq!(rgb_at(&pm_eo, 10, 10), (255, 255, 255), "even-odd hole");
    assert_eq!(rgb_at(&pm_eo, 4, 4), (0, 0, 0), "even-odd ring");

    // Nonzero: the center is filled (both rects same winding → coverage 2).
    let mut c_nz = Canvas::blank(20, 20, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c_nz.fill_background([255, 255, 255, 255]);
    let dp_nz = fill_drawpath(items, 0x000000, false);
    fill_path(
        &mut c_nz,
        &dp_nz,
        Paint::from_rgb(0x000000),
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let pm_nz = c_nz.into_pixmap().unwrap();
    assert_eq!(rgb_at(&pm_nz, 10, 10), (0, 0, 0), "nonzero filled center");
    assert_eq!(rgb_at(&pm_nz, 4, 4), (0, 0, 0), "nonzero ring");
}

// === RENDER-VEC-STROKE ===================================================

/// A horizontal stroke of width 4: the line band is painted, rows away from it
/// stay white. Verifies width is honored.
#[test]
fn render_vec_stroke_width() {
    let mut c = Canvas::blank(20, 20, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    // A horizontal segment across the middle (device y=10).
    let items = vec![PathItem::Line(
        Point::new(2.0, 10.0),
        Point::new(18.0, 10.0),
    )];
    let dp = DrawPath {
        kind: PaintKind::Stroke,
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        color: Some(0xFF0000),
        fill: None,
        width: 4.0,
        dashes: String::new(),
        close_path: false,
        even_odd: false,
        items,
    };
    let style = StrokeStyle {
        width: 4.0,
        ..Default::default()
    };
    stroke_path(
        &mut c,
        &dp,
        Paint::from_rgb(0xFF0000),
        &style,
        Matrix::IDENTITY,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();

    // On the line: red.
    assert_eq!(rgb_at(&pm, 10, 10), (255, 0, 0));
    // Within the 4px band (±2 around y=10): still red at y=9 and y=11.
    assert_eq!(rgb_at(&pm, 10, 9), (255, 0, 0));
    // Far from the line: white.
    assert_eq!(rgb_at(&pm, 10, 2), (255, 255, 255));
    assert_eq!(rgb_at(&pm, 10, 17), (255, 255, 255));
}

/// Round vs butt cap: a round cap paints pixels *beyond* the segment endpoint
/// (a half-disk); a butt cap does not. Compare the same vertical segment.
#[test]
fn render_vec_stroke_cap_round_vs_butt() {
    // Vertical segment from (10,5) to (10,15), width 6. End at y=15; with a
    // round cap the half-disk reaches ~y=18, a butt cap stops at y=15.
    let seg = vec![PathItem::Line(
        Point::new(10.0, 5.0),
        Point::new(10.0, 15.0),
    )];
    let dp = DrawPath {
        kind: PaintKind::Stroke,
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        color: Some(0x000000),
        fill: None,
        width: 6.0,
        dashes: String::new(),
        close_path: false,
        even_odd: false,
        items: seg,
    };

    let probe = |cap: LineCapStyle| -> (u8, u8, u8) {
        let mut c = Canvas::blank(20, 24, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
        c.fill_background([255, 255, 255, 255]);
        let style = StrokeStyle {
            width: 6.0,
            cap,
            ..Default::default()
        };
        stroke_path(
            &mut c,
            &dp,
            Paint::from_rgb(0x000000),
            &style,
            Matrix::IDENTITY,
        )
        .unwrap();
        let pm = c.into_pixmap().unwrap();
        rgb_at(&pm, 10, 17) // 2px beyond the y=15 endpoint
    };

    let round = probe(LineCapStyle::Round);
    let butt = probe(LineCapStyle::Butt);
    assert!(round.0 < 128, "round cap paints beyond endpoint: {round:?}");
    assert_eq!(butt, (255, 255, 255), "butt cap stops at endpoint");
}

/// Bevel vs miter join: at a sharp corner a miter join sticks out past the
/// corner; a bevel cuts it. Probe the outer corner tip.
#[test]
fn render_vec_stroke_join_miter_vs_bevel() {
    // An L-shape: (4,4)->(4,16)->(16,16), width 6. Outer corner near (1,1).
    let items = vec![
        PathItem::Line(Point::new(4.0, 4.0), Point::new(4.0, 16.0)),
        PathItem::Line(Point::new(4.0, 16.0), Point::new(16.0, 16.0)),
    ];
    let dp = DrawPath {
        kind: PaintKind::Stroke,
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        color: Some(0x000000),
        fill: None,
        width: 6.0,
        dashes: String::new(),
        close_path: false,
        even_odd: false,
        items,
    };
    let probe = |join: LineJoinStyle| -> (u8, u8, u8) {
        let mut c = Canvas::blank(24, 24, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
        c.fill_background([255, 255, 255, 255]);
        let style = StrokeStyle {
            width: 6.0,
            join,
            miter_limit: 10.0,
            ..Default::default()
        };
        stroke_path(
            &mut c,
            &dp,
            Paint::from_rgb(0x000000),
            &style,
            Matrix::IDENTITY,
        )
        .unwrap();
        let pm = c.into_pixmap().unwrap();
        // The outer corner tip: the miter triangle fills (1,17) black, the
        // bevel cuts the corner and leaves it white.
        rgb_at(&pm, 1, 17)
    };
    let miter = probe(LineJoinStyle::Miter);
    let bevel = probe(LineJoinStyle::Bevel);
    assert_eq!(miter, (0, 0, 0), "miter fills the corner tip");
    assert_eq!(bevel, (255, 255, 255), "bevel cuts the corner tip");
}

// === RENDER-VEC-CLIP =====================================================

/// A clip restricts a subsequent fill to the clip region: fill a big rect but
/// clip to a smaller box → only the box is painted.
#[test]
fn render_vec_clip_restricts_fill() {
    let mut c = Canvas::blank(20, 20, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    // Clip to [6,6 .. 14,14].
    set_clip(
        &mut c,
        &[rect_item(6.0, 6.0, 14.0, 14.0)],
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    // Fill the whole canvas red.
    let dp = fill_drawpath(vec![rect_item(0.0, 0.0, 20.0, 20.0)], 0xFF0000, false);
    fill_path(
        &mut c,
        &dp,
        Paint::from_rgb(0xFF0000),
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();

    // Inside the clip: red.
    assert_eq!(rgb_at(&pm, 10, 10), (255, 0, 0));
    // Outside the clip: untouched white.
    assert_eq!(rgb_at(&pm, 2, 2), (255, 255, 255));
    assert_eq!(rgb_at(&pm, 18, 18), (255, 255, 255));
}

/// Nested clips intersect (only the overlap survives), and `save`/`restore`
/// (q/Q) drops the inner clip again.
#[test]
fn render_vec_clip_intersect_and_restore() {
    let mut c = Canvas::blank(20, 20, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    set_clip(
        &mut c,
        &[rect_item(4.0, 4.0, 16.0, 16.0)],
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    // Intersect with a second clip → overlap [8,4..16,16] only.
    set_clip(
        &mut c,
        &[rect_item(8.0, 0.0, 20.0, 20.0)],
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let dp = fill_drawpath(vec![rect_item(0.0, 0.0, 20.0, 20.0)], 0xFF0000, false);
    fill_path(
        &mut c,
        &dp,
        Paint::from_rgb(0xFF0000),
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();
    // In the overlap (x>=8, 4<=y<16): red.
    assert_eq!(rgb_at(&pm, 12, 10), (255, 0, 0));
    // In the first clip but not the second (x<8): clipped out → white.
    assert_eq!(rgb_at(&pm, 5, 10), (255, 255, 255));
}

// === RENDER-VEC-CTM ======================================================

/// CTM translate: a rect drawn at user origin but translated by the CTM lands
/// at the translated device position.
#[test]
fn render_vec_ctm_translate() {
    let mut c = Canvas::blank(30, 30, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    // Rect [0,0..6,6] translated by (+10,+10).
    let dp = fill_drawpath(vec![rect_item(0.0, 0.0, 6.0, 6.0)], 0x00AA00, false);
    let ctm = Matrix::translate(10.0, 10.0);
    fill_path(&mut c, &dp, Paint::from_rgb(0x00AA00), ctm, false).unwrap();
    let pm = c.into_pixmap().unwrap();
    // The translated box center (~13,13) is green.
    assert_eq!(rgb_at(&pm, 13, 13), (0, 0xAA, 0));
    // The original (untranslated) position is white.
    assert_eq!(rgb_at(&pm, 3, 3), (255, 255, 255));
}

/// CTM scale: a unit rect scaled by 8 fills an 8×8 device region.
#[test]
fn render_vec_ctm_scale() {
    let mut c = Canvas::blank(20, 20, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    // Unit rect [1,1..2,2] scaled ×8 → device [8,8..16,16].
    let dp = fill_drawpath(vec![rect_item(1.0, 1.0, 2.0, 2.0)], 0x0000FF, false);
    let ctm = Matrix::scale(8.0, 8.0);
    fill_path(&mut c, &dp, Paint::from_rgb(0x0000FF), ctm, false).unwrap();
    let pm = c.into_pixmap().unwrap();
    assert_eq!(rgb_at(&pm, 12, 12), (0, 0, 255));
    assert_eq!(rgb_at(&pm, 3, 3), (255, 255, 255));
}

/// CTM rotate (90°): a tall thin rect becomes wide and thin after a 90° CTM,
/// placed correctly. Rotate about a point that keeps it in-bounds.
#[test]
fn render_vec_ctm_rotate() {
    let mut c = Canvas::blank(30, 30, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    // A rect [0,0..2,10] (tall). Rotate +90° then translate to keep on-canvas.
    // rotate(90): (x,y) -> (-y, x); translate by (+25,+5) maps it on-canvas as a
    // wide rect spanning device x in [15,25], y in [5,7].
    let dp = fill_drawpath(vec![rect_item(0.0, 0.0, 2.0, 10.0)], 0xFF00FF, false);
    let ctm = Matrix::concat(&Matrix::rotate(90.0), &Matrix::translate(25.0, 5.0));
    fill_path(&mut c, &dp, Paint::from_rgb(0xFF00FF), ctm, false).unwrap();
    let pm = c.into_pixmap().unwrap();
    // After rotation it's a horizontal band: probe a point inside it.
    assert_eq!(rgb_at(&pm, 18, 6), (255, 0, 255));
    // A point that would only be inside the *unrotated* tall rect is white.
    assert_eq!(rgb_at(&pm, 1, 6), (255, 255, 255));
}

// === RENDER-VEC-ALPHA ====================================================

/// Constant-alpha blending: a 50%-alpha red over white yields ~ (255,128,128).
#[test]
fn render_vec_alpha_blend_over_white() {
    let mut c = Canvas::blank(10, 10, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    let dp = fill_drawpath(vec![rect_item(1.0, 1.0, 9.0, 9.0)], 0xFF0000, false);
    let paint = Paint::from_rgb_alpha(0xFF0000, 128);
    fill_path(&mut c, &dp, paint, Matrix::IDENTITY, false).unwrap();
    let pm = c.into_pixmap().unwrap();
    let (r, g, b) = rgb_at(&pm, 5, 5);
    // red over white at 50%: R stays ~255, G,B drop to ~128 (±2 for rounding).
    assert!(r >= 253, "R≈255 got {r}");
    assert!((120..=136).contains(&g), "G≈128 got {g}");
    assert!((120..=136).contains(&b), "B≈128 got {b}");
}

/// Multiply blend: red (255,0,0) multiplied over a yellow (255,255,0)
/// background yields red — multiply of channels (255*255, 255*0, 0*0)/255.
#[test]
fn render_vec_blend_multiply() {
    let mut c = Canvas::blank(10, 10, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    // Yellow background.
    c.fill_background([255, 255, 0, 255]);
    let dp = fill_drawpath(vec![rect_item(1.0, 1.0, 9.0, 9.0)], 0xFF0000, false);
    let paint = Paint::from_rgb(0xFF0000).with_blend(BlendMode::Multiply);
    fill_path(&mut c, &dp, paint, Matrix::IDENTITY, false).unwrap();
    let pm = c.into_pixmap().unwrap();
    let (r, g, b) = rgb_at(&pm, 5, 5);
    assert_eq!((r, g, b), (255, 0, 0), "yellow × red = red");
}

// === RENDER-VEC-OUTPUT-FORMAT ============================================

/// `into_pixmap` honors the requested alpha channel and colorspace dims.
#[test]
fn render_vec_into_pixmap_rgba_dims() {
    let mut c = Canvas::blank(8, 5, Matrix::IDENTITY, Colorspace::Rgb, true).unwrap();
    let dp = fill_drawpath(vec![rect_item(0.0, 0.0, 8.0, 5.0)], 0x123456, false);
    fill_path(
        &mut c,
        &dp,
        Paint::from_rgb(0x123456),
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();
    assert_eq!((pm.width, pm.height), (8, 5));
    assert_eq!(pm.n, 4);
    assert!(pm.alpha);
    assert_eq!(pm.samples.len(), 8 * 5 * 4);
    // The filled, opaque pixel: straight RGBA = (0x12,0x34,0x56,0xFF).
    let idx = 4usize; // pixel (1,0)
    assert_eq!(&pm.samples[idx..idx + 4], &[0x12, 0x34, 0x56, 0xFF]);
}

// === RENDER-VEC-DASH =====================================================

/// A dashed stroke paints gaps: a long horizontal segment with a `[4 4]` dash
/// has painted runs and white gaps along its length.
#[test]
fn render_vec_stroke_dash() {
    let mut c = Canvas::blank(40, 12, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    let items = vec![PathItem::Line(Point::new(2.0, 6.0), Point::new(38.0, 6.0))];
    let dp = DrawPath {
        kind: PaintKind::Stroke,
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        color: Some(0x000000),
        fill: None,
        width: 3.0,
        dashes: "[4 4] 0".to_string(),
        close_path: false,
        even_odd: false,
        items,
    };
    let style = StrokeStyle {
        width: 3.0,
        dash_array: vec![4.0, 4.0],
        dash_phase: 0.0,
        ..Default::default()
    };
    stroke_path(
        &mut c,
        &dp,
        Paint::from_rgb(0x000000),
        &style,
        Matrix::IDENTITY,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();
    // Scan the stroke row: there must be both painted (dark) and gap (white).
    let mut saw_dark = false;
    let mut saw_white = false;
    for x in 2..38 {
        let (r, _, _) = rgb_at(&pm, x, 6);
        if r < 64 {
            saw_dark = true;
        }
        if r > 200 {
            saw_white = true;
        }
    }
    assert!(saw_dark, "dash has painted runs");
    assert!(saw_white, "dash has gaps");
}

/// An empty path is a tolerant no-op (the canvas stays white, no error).
#[test]
fn render_vec_empty_path_noop() {
    let mut c = Canvas::blank(6, 6, Matrix::IDENTITY, Colorspace::Rgb, false).unwrap();
    c.fill_background([255, 255, 255, 255]);
    let dp = fill_drawpath(vec![], 0xFF0000, false);
    fill_path(
        &mut c,
        &dp,
        Paint::from_rgb(0xFF0000),
        Matrix::IDENTITY,
        false,
    )
    .unwrap();
    let pm = c.into_pixmap().unwrap();
    assert_eq!(rgb_at(&pm, 3, 3), (255, 255, 255));
}
