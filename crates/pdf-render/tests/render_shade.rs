//! M6c shading render tests (`RENDER-SHADE-*`).
//!
//! Axial (type 2) and radial (type 3) shadings are evaluated into a tiny-skia
//! gradient and filled over a device rect. We assert the endpoints carry the
//! expected ramp colors and that the function evaluators (exponential /
//! stitching / sampled) produce the right intermediate colors.

use pdf_core::geom::Matrix;
use pdf_image::pixmap::Colorspace;
use pdf_render::canvas::Canvas;
use pdf_render::image::{
    draw_axial_shading, draw_radial_shading, sample_device_rgba, PdfFunction, ShadingColor,
};

fn canvas(w: u32, h: u32) -> Canvas {
    Canvas::blank(w, h, Matrix::IDENTITY, Colorspace::Rgb, true).expect("blank canvas")
}

fn px(cv: &Canvas, x: u32, y: u32) -> [u8; 4] {
    sample_device_rgba(cv, x, y).expect("pixel in range")
}

fn close(a: u8, b: u8) -> bool {
    (a as i32 - b as i32).abs() <= 8
}

// A 2-color exponential ramp red->blue across t in [0,1].
fn red_to_blue() -> PdfFunction {
    PdfFunction::Exponential {
        domain: [0.0, 1.0],
        c0: vec![1.0, 0.0, 0.0],
        c1: vec![0.0, 0.0, 1.0],
        n: 1.0,
    }
}

// RENDER-SHADE-AXIAL-ENDS: a horizontal axial gradient has the start color at
// the left endpoint and the end color at the right endpoint.
#[test]
fn render_shade_axial_endpoints() {
    let mut cv = canvas(20, 4);
    // Coords in device space: from (0,2) to (20,2) -> red ramps to blue.
    draw_axial_shading(
        &mut cv,
        (0.0, 2.0),
        (20.0, 2.0),
        &red_to_blue(),
        Colorspace::Rgb,
        (false, false),
        Matrix::IDENTITY,
        255,
    )
    .unwrap();
    // Sample at the exact endpoints (Pad spread clamps to the stop colors).
    let left = px(&cv, 0, 2);
    let right = px(&cv, 19, 2);
    assert!(
        close(left[0], 255) && close(left[2], 0),
        "left red {left:?}"
    );
    assert!(
        close(right[0], 0) && close(right[2], 255),
        "right blue {right:?}"
    );
}

// RENDER-SHADE-AXIAL-MID: the midpoint of a linear ramp is the average color.
#[test]
fn render_shade_axial_midpoint() {
    let mut cv = canvas(20, 4);
    draw_axial_shading(
        &mut cv,
        (0.0, 2.0),
        (20.0, 2.0),
        &red_to_blue(),
        Colorspace::Rgb,
        (false, false),
        Matrix::IDENTITY,
        255,
    )
    .unwrap();
    let mid = px(&cv, 10, 2);
    // Roughly halfway: ~purple. Allow a wide tolerance for gradient stop spacing.
    assert!(mid[0] > 60 && mid[0] < 200, "mid red comp {mid:?}");
    assert!(mid[2] > 60 && mid[2] < 200, "mid blue comp {mid:?}");
}

// RENDER-SHADE-RADIAL: a radial gradient has the inner color near the center and
// the outer color near the rim.
#[test]
fn render_shade_radial_center() {
    let mut cv = canvas(20, 20);
    // Center (10,10), inner radius 0 -> outer radius 10. red center -> blue rim.
    draw_radial_shading(
        &mut cv,
        (10.0, 10.0, 0.0),
        (10.0, 10.0, 10.0),
        &red_to_blue(),
        Colorspace::Rgb,
        (false, false),
        Matrix::IDENTITY,
        255,
    )
    .unwrap();
    // Near the center the color is dominated by the inner (red) stop; the pixel
    // center is ~0.7px off the focal point so allow a small blue bleed.
    let center = px(&cv, 10, 10);
    assert!(center[0] > 230 && center[2] < 30, "center red {center:?}");
    // Near the rim it is dominated by the outer (blue) stop.
    let rim = px(&cv, 10, 1);
    assert!(rim[2] > 200 && rim[0] < 60, "rim blue {rim:?}");
}

// RENDER-SHADE-FUNC-EXP: exponential function evaluation at t.
#[test]
fn func_exponential_eval() {
    let f = red_to_blue();
    let ShadingColor(c) = f.eval(0.0);
    assert!((c[0] - 1.0).abs() < 1e-6 && (c[2] - 0.0).abs() < 1e-6);
    let ShadingColor(c) = f.eval(1.0);
    assert!((c[0] - 0.0).abs() < 1e-6 && (c[2] - 1.0).abs() < 1e-6);
    let ShadingColor(c) = f.eval(0.5);
    assert!((c[0] - 0.5).abs() < 1e-6 && (c[2] - 0.5).abs() < 1e-6);
}

// RENDER-SHADE-FUNC-EXP-N: exponential with n=2 is non-linear.
#[test]
fn func_exponential_n2() {
    let f = PdfFunction::Exponential {
        domain: [0.0, 1.0],
        c0: vec![0.0],
        c1: vec![1.0],
        n: 2.0,
    };
    let ShadingColor(c) = f.eval(0.5);
    assert!((c[0] - 0.25).abs() < 1e-6, "0.5^2 = 0.25, got {}", c[0]);
}

// RENDER-SHADE-FUNC-STITCH: a stitching function picks the right sub-function.
#[test]
fn func_stitching_eval() {
    // Two sub-functions over [0,0.5) and [0.5,1]: first red->green, second green->blue.
    let f = PdfFunction::Stitching {
        domain: [0.0, 1.0],
        functions: vec![
            PdfFunction::Exponential {
                domain: [0.0, 1.0],
                c0: vec![1.0, 0.0, 0.0],
                c1: vec![0.0, 1.0, 0.0],
                n: 1.0,
            },
            PdfFunction::Exponential {
                domain: [0.0, 1.0],
                c0: vec![0.0, 1.0, 0.0],
                c1: vec![0.0, 0.0, 1.0],
                n: 1.0,
            },
        ],
        bounds: vec![0.5],
        encode: vec![[0.0, 1.0], [0.0, 1.0]],
    };
    // At t=0 -> red.
    let ShadingColor(c0) = f.eval(0.0);
    assert!((c0[0] - 1.0).abs() < 1e-6);
    // At t=0.5 -> green (boundary, start of second).
    let ShadingColor(cm) = f.eval(0.5);
    assert!((cm[1] - 1.0).abs() < 1e-6, "mid green {cm:?}");
    // At t=1 -> blue.
    let ShadingColor(c1) = f.eval(1.0);
    assert!((c1[2] - 1.0).abs() < 1e-6);
}

// RENDER-SHADE-FUNC-SAMPLED: a sampled (type 0) function interpolates samples.
#[test]
fn func_sampled_eval() {
    // 2 samples of a 1-channel function: 0.0 at t=0, 1.0 at t=1.
    let f = PdfFunction::Sampled {
        domain: vec![[0.0, 1.0]],
        size: vec![2],
        bits_per_sample: 8,
        n_outputs: 1,
        encode: vec![[0.0, 1.0]],
        decode: vec![[0.0, 1.0]],
        samples: vec![0, 255],
    };
    let ShadingColor(c0) = f.eval(0.0);
    assert!((c0[0] - 0.0).abs() < 1e-3, "{c0:?}");
    let ShadingColor(c1) = f.eval(1.0);
    assert!((c1[0] - 1.0).abs() < 1e-3, "{c1:?}");
    let ShadingColor(cm) = f.eval(0.5);
    assert!((cm[0] - 0.5).abs() < 1e-2, "midpoint interp {cm:?}");
}

// RENDER-SHADE-NOPANIC: degenerate axis (start==end) does not panic.
#[test]
fn render_shade_degenerate_axis() {
    let mut cv = canvas(8, 8);
    let r = draw_axial_shading(
        &mut cv,
        (4.0, 4.0),
        (4.0, 4.0),
        &red_to_blue(),
        Colorspace::Rgb,
        (false, false),
        Matrix::IDENTITY,
        255,
    );
    // Either Ok (no-op / single color) or a typed error; must not panic.
    let _ = r;
}
