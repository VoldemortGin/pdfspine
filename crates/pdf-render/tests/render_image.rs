//! M6c image-compositing render tests (`RENDER-IMG-*`).
//!
//! Each test builds a [`Canvas`], composites a small decoded [`Pixmap`] under a
//! known CTM, then samples the resulting device pixels and asserts the painted
//! region matches the source color (within AA tolerance) and the background is
//! untouched outside the placement. Geometry cases cover scale, rotation, flip,
//! soft-mask alpha, stencil image masks, and out-of-canvas clipping.
//!
//! Pixels are read back with [`sample_device_rgba`] (un-premultiplied device
//! RGBA), an M6c test-support entry that does not depend on M6a's
//! `Canvas::into_pixmap` (still a stub during parallel development).

use pdf_core::geom::Matrix;
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_render::canvas::Canvas;
use pdf_render::image::{draw_image, draw_image_mask, sample_device_rgba};
use pdf_render::vector::Paint;

/// A `w × h` device canvas, identity base transform. The backing buffer starts
/// transparent so we can see exactly where a draw painted.
fn canvas(w: u32, h: u32) -> Canvas {
    Canvas::blank(w, h, Matrix::IDENTITY, Colorspace::Rgb, true).expect("blank canvas")
}

/// A `w × h` device canvas with the realistic page→device **y-flip** base
/// transform (PDF user space is y-up, the pixmap is y-down). Orientation tests
/// use this so an image's first sample row lands at the device top — the actual
/// `render_page` pipeline always composes this flip.
fn canvas_yflip(w: u32, h: u32) -> Canvas {
    let base = Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, h as f64);
    Canvas::blank(w, h, base, Colorspace::Rgb, true).expect("blank canvas")
}

/// A solid-color `w × h` RGB pixmap.
fn solid_rgb(w: u32, h: u32, rgb: [u8; 3]) -> Pixmap {
    let mut data = Vec::with_capacity(w as usize * h as usize * 3);
    for _ in 0..(w as usize * h as usize) {
        data.extend_from_slice(&rgb);
    }
    Pixmap::new(w, h, Colorspace::Rgb, false, data)
}

/// Reads an un-premultiplied device pixel.
fn px(cv: &Canvas, x: u32, y: u32) -> [u8; 4] {
    sample_device_rgba(cv, x, y).expect("pixel in range")
}

const TOL: i32 = 4;

fn close(a: u8, b: u8) -> bool {
    (a as i32 - b as i32).abs() <= TOL
}

fn assert_rgb(got: [u8; 4], want: [u8; 3]) {
    assert!(
        close(got[0], want[0]) && close(got[1], want[1]) && close(got[2], want[2]),
        "rgb {got:?} != {want:?}"
    );
}

// RENDER-IMG-FILL: image scaled to fill the whole canvas paints its color.
#[test]
fn render_img_fill_whole_canvas() {
    let mut cv = canvas(8, 8);
    let img = solid_rgb(2, 2, [200, 30, 40]);
    // Unit square -> the full 8x8 device area.
    let ctm = Matrix::scale(8.0, 8.0);
    draw_image(&mut cv, &img, ctm, 255).unwrap();
    // Center pixels are solidly the image color.
    assert_rgb(px(&cv, 4, 4), [200, 30, 40]);
}

// RENDER-IMG-RECT: image placed into a known sub-rect; inside matches, outside
// stays transparent background.
#[test]
fn render_img_into_subrect() {
    let mut cv = canvas(10, 10);
    let img = solid_rgb(1, 1, [0, 0, 255]);
    // Map the unit square to device rect [2,2]..[6,6] (4x4). Base transform is
    // identity (y-down device space), so scale(4,4)*translate(2,2) places it.
    let ctm = Matrix::scale(4.0, 4.0) * Matrix::translate(2.0, 2.0);
    draw_image(&mut cv, &img, ctm, 255).unwrap();

    // Inside the rect: blue, opaque.
    let inside = px(&cv, 4, 4);
    assert!(close(inside[0], 0) && close(inside[1], 0) && close(inside[2], 255));
    assert!(inside[3] > 250, "inside opaque, got a={}", inside[3]);
    // Outside the rect: untouched (transparent).
    assert_eq!(px(&cv, 0, 0)[3], 0, "outside stays transparent");
}

// RENDER-IMG-ORIENT: a 2x2 four-quadrant image keeps quadrant colors when blown
// up; verifies orientation (row 0 is the TOP of the placement in device space
// after the image y-flip, with an identity y-down base transform).
#[test]
fn render_img_quadrant_orientation() {
    // Image rows: top = [red, green], bottom = [blue, white].
    let data = vec![
        255, 0, 0, /*tl*/ 0, 255, 0, /*tr*/
        0, 0, 255, /*bl*/ 255, 255, 255, /*br*/
    ];
    let img = Pixmap::new(2, 2, Colorspace::Rgb, false, data);
    let mut cv = canvas_yflip(20, 20);
    // unit square -> page [0,20]x[0,20]; the y-flip base then maps it to device.
    let ctm = Matrix::scale(20.0, 20.0);
    draw_image(&mut cv, &img, ctm, 255).unwrap();
    // top-left device pixel -> image top-left -> red
    let tl = px(&cv, 3, 3);
    assert!(
        close(tl[0], 255) && close(tl[1], 0) && close(tl[2], 0),
        "tl {tl:?}"
    );
    // top-right -> green
    let tr = px(&cv, 16, 3);
    assert!(
        close(tr[0], 0) && close(tr[1], 255) && close(tr[2], 0),
        "tr {tr:?}"
    );
    // bottom-left -> blue
    let bl = px(&cv, 3, 16);
    assert!(
        close(bl[0], 0) && close(bl[1], 0) && close(bl[2], 255),
        "bl {bl:?}"
    );
}

// RENDER-IMG-FLIP: a CTM with negative d (vertical flip) flips orientation.
#[test]
fn render_img_vertical_flip() {
    let data = vec![
        255, 0, 0, 255, 0, 0, // top row red
        0, 0, 255, 0, 0, 255, // bottom row blue
    ];
    let img = Pixmap::new(2, 2, Colorspace::Rgb, false, data);
    let mut cv = canvas_yflip(20, 20);
    // Upright placement is ctm = scale(20,20) (row0 red at device top). Flipping
    // the unit square's v (negate d, shift e/f) mirrors it, so row0 (red) lands
    // at the device BOTTOM and row1 (blue) at the top.
    let ctm = Matrix::new(20.0, 0.0, 0.0, -20.0, 0.0, 20.0);
    draw_image(&mut cv, &img, ctm, 255).unwrap();
    let top = px(&cv, 10, 3);
    let bottom = px(&cv, 10, 16);
    assert!(
        close(top[0], 0) && close(top[2], 255),
        "flipped top should be blue {top:?}"
    );
    assert!(
        close(bottom[0], 255) && close(bottom[2], 0),
        "flipped bottom should be red {bottom:?}"
    );
}

// RENDER-IMG-ROTATE: a 90-degree rotation places the image without panic and
// keeps the color (single-color, so we only check coverage + color).
#[test]
fn render_img_rotated_90() {
    let img = solid_rgb(4, 4, [10, 220, 90]);
    let mut cv = canvas(40, 40);
    let ctm = Matrix::scale(20.0, 20.0) * Matrix::rotate(90.0) * Matrix::translate(30.0, 5.0);
    draw_image(&mut cv, &img, ctm, 255).unwrap();
    let mut found = false;
    for y in 0..40 {
        for x in 0..40 {
            let p = px(&cv, x, y);
            if p[3] > 200 && close(p[0], 10) && close(p[1], 220) && close(p[2], 90) {
                found = true;
            }
        }
    }
    assert!(found, "rotated image color not found anywhere");
}

// RENDER-IMG-GRAY: a grayscale source paints equal R=G=B.
#[test]
fn render_img_grayscale() {
    let img = Pixmap::new(2, 2, Colorspace::Gray, false, vec![128, 128, 128, 128]);
    let mut cv = canvas(8, 8);
    draw_image(&mut cv, &img, Matrix::scale(8.0, 8.0), 255).unwrap();
    let p = px(&cv, 4, 4);
    assert!(
        close(p[0], 128) && close(p[1], 128) && close(p[2], 128),
        "gray {p:?}"
    );
}

// RENDER-IMG-CMYK: a CMYK source is converted to RGB before compositing.
#[test]
fn render_img_cmyk() {
    // Pure cyan: C=255,M=0,Y=0,K=0 -> RGB ~ (0,255,255).
    let img = Pixmap::new(1, 1, Colorspace::Cmyk, false, vec![255, 0, 0, 0]);
    let mut cv = canvas(8, 8);
    draw_image(&mut cv, &img, Matrix::scale(8.0, 8.0), 255).unwrap();
    let p = px(&cv, 4, 4);
    assert!(
        close(p[0], 0) && close(p[1], 255) && close(p[2], 255),
        "cmyk->rgb {p:?}"
    );
}

// RENDER-IMG-SMASK: a soft mask (grayscale alpha plane) blends the image.
#[test]
fn render_img_smask_alpha() {
    let img = solid_rgb(2, 2, [255, 0, 0]);
    let img = img
        .with_smask_gray(&[128, 128, 128, 128], 2, 2)
        .expect("attach smask");
    let mut cv = canvas(8, 8);
    draw_image(&mut cv, &img, Matrix::scale(8.0, 8.0), 255).unwrap();
    let p = px(&cv, 4, 4);
    assert!((p[3] as i32 - 128).abs() <= 6, "smask alpha {p:?}");
    assert!(close(p[0], 255), "smask keeps red {p:?}");
}

// RENDER-IMG-CA: constant alpha (ca) scales coverage.
#[test]
fn render_img_constant_alpha() {
    let img = solid_rgb(2, 2, [0, 255, 0]);
    let mut cv = canvas(8, 8);
    draw_image(&mut cv, &img, Matrix::scale(8.0, 8.0), 64).unwrap();
    let p = px(&cv, 4, 4);
    assert!((p[3] as i32 - 64).abs() <= 6, "ca alpha {p:?}");
}

// RENDER-IMG-MASK-STENCIL: an /ImageMask paints the fill color through the 1-bpp
// stencil. Mask bit 0 = paint (default /Decode), bit 1 = transparent.
#[test]
fn render_img_stencil_mask() {
    // 2x2 stencil: top-left painted (0), others not painted (1).
    // 1bpp packed MSB-first, rows byte-aligned: row0 = 0b0100_0000, row1 = 0b1100_0000
    let bits = vec![0b0100_0000u8, 0b1100_0000u8];
    let fill = Paint::from_rgb(0x00_FF8800); // orange
    let mut cv = canvas_yflip(20, 20);
    draw_image_mask(&mut cv, &bits, 2, 2, fill, Matrix::scale(20.0, 20.0), 255).unwrap();
    // top-left quadrant painted orange
    let tl = px(&cv, 4, 4);
    assert!(
        close(tl[0], 0xFF) && close(tl[1], 0x88) && close(tl[2], 0x00),
        "stencil tl {tl:?}"
    );
    assert!(tl[3] > 200, "stencil tl opaque {tl:?}");
    // top-right quadrant NOT painted -> transparent
    assert_eq!(px(&cv, 16, 4)[3], 0, "stencil tr transparent");
}

// RENDER-IMG-CLIP-OOB: placement entirely off-canvas paints nothing, no panic.
#[test]
fn render_img_out_of_canvas() {
    let img = solid_rgb(4, 4, [255, 0, 0]);
    let mut cv = canvas(10, 10);
    let ctm = Matrix::scale(4.0, 4.0) * Matrix::translate(100.0, 100.0);
    draw_image(&mut cv, &img, ctm, 255).unwrap();
    for y in 0..10 {
        for x in 0..10 {
            assert_eq!(px(&cv, x, y)[3], 0, "({x},{y}) should be untouched");
        }
    }
}

// RENDER-IMG-PARTIAL: partly off-canvas placement clips, no panic, paints the
// visible part.
#[test]
fn render_img_partially_off_canvas() {
    let img = solid_rgb(4, 4, [0, 200, 200]);
    let mut cv = canvas(10, 10);
    let ctm = Matrix::scale(8.0, 8.0) * Matrix::translate(-4.0, -4.0);
    draw_image(&mut cv, &img, ctm, 255).unwrap();
    assert!(px(&cv, 1, 1)[3] > 200, "visible part painted");
    assert_eq!(px(&cv, 9, 9)[3], 0, "far corner untouched");
}

// RENDER-IMG-ALPHA-SRC: a source pixmap already carrying alpha composites with
// its own per-pixel alpha.
#[test]
fn render_img_source_alpha() {
    let data = vec![
        0, 255, 0, 255, // tl opaque
        0, 255, 0, 0, // tr transparent
        0, 255, 0, 255, // bl opaque
        0, 255, 0, 0, // br transparent
    ];
    let img = Pixmap::new(2, 2, Colorspace::Rgb, true, data);
    let mut cv = canvas(20, 20);
    draw_image(&mut cv, &img, Matrix::scale(20.0, 20.0), 255).unwrap();
    assert!(px(&cv, 4, 4)[3] > 200, "left opaque");
    assert_eq!(px(&cv, 16, 4)[3], 0, "right transparent");
}

// RENDER-IMG-ZERO-ALPHA: alpha=0 paints nothing.
#[test]
fn render_img_zero_alpha_noop() {
    let img = solid_rgb(4, 4, [255, 0, 0]);
    let mut cv = canvas(8, 8);
    draw_image(&mut cv, &img, Matrix::scale(8.0, 8.0), 0).unwrap();
    assert_eq!(px(&cv, 4, 4)[3], 0, "alpha=0 paints nothing");
}
