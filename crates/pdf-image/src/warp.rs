//! Owned quadrilateral resampling in local pixel-boundary coordinates.

use crate::error::{Error, Result};
use crate::pixmap::Pixmap;

/// Resamples the convex quad `(ul, ur, ll, lr)` into a new alpha-bearing pixmap.
///
/// # Errors
/// Rejects invalid geometry, malformed source storage and excessive allocation.
pub fn warp(source: &Pixmap, quad: [[f64; 2]; 4], width: u32, height: u32) -> Result<Pixmap> {
    validate_quad(quad)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels > crate::codecs::MAX_IMAGE_PIXELS {
        return Err(Error::LimitExceeded("image pixel count exceeds cap"));
    }
    let components = usize::from(source.colorspace.components());
    let output_n = components + 1;
    let output_stride = (width as usize)
        .checked_mul(output_n)
        .ok_or(Error::LimitExceeded("warp stride overflow"))?;
    let bytes = output_stride
        .checked_mul(height as usize)
        .ok_or(Error::LimitExceeded("warp sample size overflow"))?;
    if bytes == 0 {
        return Pixmap::try_new(width, height, source.colorspace, true, Vec::new());
    }
    let source_n = components + usize::from(source.alpha);
    let stride = (source.width as usize)
        .checked_mul(source_n)
        .ok_or(Error::LimitExceeded("source stride overflow"))?;
    let source_bytes = stride
        .checked_mul(source.height as usize)
        .ok_or(Error::LimitExceeded("source sample size overflow"))?;
    if source.width == 0
        || source.height == 0
        || source.n as usize != source_n
        || source.stride != stride
        || source.samples.len() != source_bytes
    {
        return Err(Error::InvalidArgument("invalid warp source storage"));
    }
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(bytes)
        .map_err(|_| Error::LimitExceeded("warp sample allocation failed"))?;
    for row in 0..height {
        let v = (f64::from(row) + 0.5) / f64::from(height);
        let left = lerp_point(quad[0], quad[2], v);
        let right = lerp_point(quad[1], quad[3], v);
        for col in 0..width {
            let u = (f64::from(col) + 0.5) / f64::from(width);
            let point = lerp_point(left, right, u);
            // Boundary coordinates place source pixel centers at i + 0.5.
            let x = (point[0] - 0.5).clamp(0.0, f64::from(source.width - 1));
            let y = (point[1] - 0.5).clamp(0.0, f64::from(source.height - 1));
            let x0 = x.floor() as u32;
            let y0 = y.floor() as u32;
            let x1 = (x0 + 1).min(source.width - 1);
            let y1 = (y0 + 1).min(source.height - 1);
            let fx = x - f64::from(x0);
            let fy = y - f64::from(y0);
            for channel in 0..source_n {
                let sample = |sx: u32, sy: u32| {
                    f64::from(
                        source.samples[sy as usize * stride + sx as usize * source_n + channel],
                    )
                };
                let top = lerp(sample(x0, y0), sample(x1, y0), fx);
                let bottom = lerp(sample(x0, y1), sample(x1, y1), fx);
                samples.push(lerp(top, bottom, fy).round().clamp(0.0, 255.0) as u8);
            }
            if !source.alpha {
                samples.push(255);
            }
        }
    }
    Pixmap::try_new(width, height, source.colorspace, true, samples)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1.0 - t) * a + t * b
}

fn lerp_point(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]
}

fn validate_quad(quad: [[f64; 2]; 4]) -> Result<()> {
    if quad.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::InvalidArgument("warp quad must be finite"));
    }
    // Scaling before subtraction avoids overflow for finite large coordinates.
    let scale = quad.iter().flatten().fold(0.0_f64, |a, b| a.max(b.abs()));
    if scale == 0.0 {
        return Err(Error::InvalidArgument("degenerate warp quad"));
    }
    let perimeter = [quad[0], quad[1], quad[3], quad[2]].map(|p| [p[0] / scale, p[1] / scale]);
    let mut sign = 0.0_f64;
    for i in 0..4 {
        let a = perimeter[i];
        let b = perimeter[(i + 1) % 4];
        let c = perimeter[(i + 2) % 4];
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if cross == 0.0 || (sign != 0.0 && cross.signum() != sign) {
            return Err(Error::InvalidArgument(
                "warp quad must be convex and nondegenerate",
            ));
        }
        sign = cross.signum();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixmap::Colorspace;

    fn square(w: f64, h: f64) -> [[f64; 2]; 4] {
        [[0.0, 0.0], [w, 0.0], [0.0, h], [w, h]]
    }

    #[test]
    fn identity_and_four_color_center() {
        let source = Pixmap::new(
            2,
            2,
            Colorspace::Rgb,
            false,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
        );
        let result = warp(&source, square(2.0, 2.0), 2, 2).unwrap();
        assert_eq!(
            &*result.samples,
            &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]
        );
        let center = warp(&source, square(2.0, 2.0), 1, 1).unwrap();
        assert_eq!(&*center.samples, &[128, 128, 128, 255]);
    }

    #[test]
    fn asymmetric_quad_center_is_corner_average() {
        // Each channel directly encodes one coordinate. At the center of a
        // bilinear quad, the coordinate is the average of its four corners.
        let samples = (0..256)
            .flat_map(|y| (0..256).flat_map(move |x| [x as u8, y as u8, 0]))
            .collect();
        let source = Pixmap::new(256, 256, Colorspace::Rgb, false, samples);
        for (quad, expected) in [
            (
                [[80., 20.], [160., 20.], [20., 220.], [240., 220.]],
                [125, 120, 0, 255],
            ),
            (
                [[40., 20.], [210., 50.], [10., 210.], [240., 240.]],
                [125, 130, 0, 255],
            ),
        ] {
            let result = warp(&source, quad, 1, 1).unwrap();
            assert_eq!(&*result.samples, &expected);
        }
    }

    #[test]
    fn alpha_channels_and_storage_are_independent() {
        let mut source = Pixmap::new(
            2,
            1,
            Colorspace::Rgb,
            true,
            vec![0, 0, 0, 0, 128, 0, 0, 128],
        );
        let view = source.samples.clone();
        let result = warp(&source, square(2., 1.), 1, 1).unwrap();
        assert_eq!(&*result.samples, &[64, 0, 0, 64]);
        source.set_pixel(1, 0, &[0, 0, 0, 128]).unwrap();
        assert_eq!(view[4], 128);
        assert_eq!(&*result.samples, &[64, 0, 0, 64]);
        for cs in [Colorspace::Gray, Colorspace::Cmyk] {
            let source = Pixmap::new(1, 1, cs, false, vec![70; cs.components() as usize]);
            let result = warp(&source, square(1., 1.), 1, 1).unwrap();
            assert_eq!(result.n, cs.components() + 1);
            assert_eq!(result.samples.last(), Some(&255));
            assert_eq!(result.samples[0], 70);
        }
    }

    #[test]
    fn clamps_edges_and_allows_empty_destination() {
        let source = Pixmap::new(2, 1, Colorspace::Gray, false, vec![10, 90]);
        let left = [[-4., -4.], [-2., -4.], [-4., -2.], [-2., -2.]];
        assert_eq!(&*warp(&source, left, 1, 1).unwrap().samples, &[10, 255]);
        let right = [[4., 4.], [6., 4.], [4., 6.], [6., 6.]];
        assert_eq!(&*warp(&source, right, 1, 1).unwrap().samples, &[90, 255]);
        let empty = warp(&source, square(2., 1.), 0, 10).unwrap();
        assert_eq!((empty.width, empty.height), (0, 10));
        assert!(empty.samples.is_empty());
    }

    #[test]
    fn finite_large_coordinates_clamp_without_intermediate_overflow() {
        let source = Pixmap::new(1, 1, Colorspace::Gray, false, vec![71]);
        let quad = [
            [1.0e308, 1.0e308],
            [1.1e308, 1.0e308],
            [1.0e308, 1.1e308],
            [1.1e308, 1.1e308],
        ];
        assert_eq!(
            &*warp(&source, quad, 2, 2).unwrap().samples,
            &[71, 255, 71, 255, 71, 255, 71, 255]
        );
        let mut broken = source;
        broken.stride = 0;
        assert!(warp(&broken, square(1.0, 1.0), 1, 1).is_err());
    }

    #[test]
    fn rejects_invalid_geometry_and_excessive_allocation() {
        let source = Pixmap::new(1, 1, Colorspace::Gray, false, vec![1]);
        for quad in [
            [[0., 0.], [1., 1.], [0., 1.], [1., 0.]],
            [[0., 0.], [0., 0.], [0., 1.], [0., 1.]],
            [[0., 0.], [1., 0.], [0.5, 0.2], [1., 1.]],
            [[f64::NAN, 0.], [1., 0.], [0., 1.], [1., 1.]],
        ] {
            assert!(warp(&source, quad, 1, 1).is_err());
        }
        assert!(matches!(
            warp(&source, square(1., 1.), u32::MAX, u32::MAX),
            Err(Error::LimitExceeded(_))
        ));
    }
}
