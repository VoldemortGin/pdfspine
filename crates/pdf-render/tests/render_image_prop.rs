//! M6c image property tests (`RENDER-IMG-PROP-*`).
//!
//! Arbitrary CTMs (including degenerate / singular / huge ones) and arbitrary
//! tiny pixmaps must never panic and never write outside the canvas bounds.

use proptest::prelude::*;

use pdf_core::geom::Matrix;
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_render::canvas::Canvas;
use pdf_render::image::{draw_image, draw_image_mask};
use pdf_render::vector::Paint;

fn mat() -> impl Strategy<Value = Matrix> {
    // Mix normal-ish and pathological values.
    let comp = prop_oneof![
        (-50.0f64..50.0),
        Just(0.0),
        Just(1e9),
        Just(-1e9),
        Just(f64::NAN),
        Just(f64::INFINITY),
    ];
    (
        comp.clone(),
        comp.clone(),
        comp.clone(),
        comp.clone(),
        comp.clone(),
        comp,
    )
        .prop_map(|(a, b, c, d, e, f)| Matrix::new(a, b, c, d, e, f))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // RENDER-IMG-PROP-NOPANIC: any CTM + small image never panics.
    #[test]
    fn draw_image_never_panics(m in mat(), r in 0u8..=255, g in 0u8..=255, b in 0u8..=255, a in 0u8..=255) {
        let mut cv = Canvas::blank(16, 16, Matrix::IDENTITY, Colorspace::Rgb, true).unwrap();
        let img = Pixmap::new(3, 3, Colorspace::Rgb, false, [r, g, b].repeat(9));
        let _ = draw_image(&mut cv, &img, m, a);
        prop_assert_eq!(cv.width(), 16);
        prop_assert_eq!(cv.height(), 16);
    }

    // RENDER-IMG-PROP-MASK: arbitrary CTM + stencil mask never panics.
    #[test]
    fn draw_image_mask_never_panics(m in mat(), a in 0u8..=255) {
        let mut cv = Canvas::blank(16, 16, Matrix::IDENTITY, Colorspace::Rgb, true).unwrap();
        let bits = vec![0b1010_1010u8; 4]; // 4x4 1bpp -> row_bytes=1, 4 rows
        let _ = draw_image_mask(&mut cv, &bits, 4, 4, Paint::from_rgb(0x00FF00FF), m, a);
        prop_assert_eq!(cv.width(), 16);
    }

    // RENDER-IMG-PROP-HUGE: a huge scale (image covers far beyond canvas) is
    // bounded — clipped to the canvas, no OOM, no panic.
    #[test]
    fn draw_image_huge_scale_bounded(s in 1e3f64..1e7) {
        let mut cv = Canvas::blank(8, 8, Matrix::IDENTITY, Colorspace::Rgb, true).unwrap();
        let img = Pixmap::new(2, 2, Colorspace::Rgb, false, [10, 20, 30].repeat(4));
        let _ = draw_image(&mut cv, &img, Matrix::scale(s, s), 255);
        prop_assert_eq!(cv.width(), 8);
    }
}
