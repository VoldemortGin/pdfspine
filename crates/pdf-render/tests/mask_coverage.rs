//! Frozen pristine tiny-skia 0.11.4 public Mask output, generated before the
//! local coverage patch. Includes nonzero destinations and overlapping fills.
use tiny_skia::{FillRule, Mask, Path, PathBuilder, Rect, Transform};

fn paths() -> Vec<Path> {
    let mut paths = Vec::new();
    let mut p = PathBuilder::new();
    p.move_to(0., 0.);
    p.line_to(22., 0.);
    p.line_to(22., 25.);
    p.line_to(0., 25.);
    p.close();
    paths.push(p.finish().unwrap());
    let mut p = PathBuilder::new();
    p.move_to(-8., 3.);
    p.cubic_to(30., -20., -5., 45., 29., 28.);
    p.quad_to(2., 43., -8., 3.);
    p.close();
    paths.push(p.finish().unwrap());
    let mut p = PathBuilder::new();
    p.move_to(0., 0.);
    p.line_to(30., 30.);
    p.line_to(0., 30.);
    p.line_to(30., 0.);
    p.close();
    paths.push(p.finish().unwrap());
    let mut p = PathBuilder::new();
    p.push_rect(Rect::from_xywh(-2., -2., 33., 33.).unwrap());
    p.push_rect(Rect::from_xywh(4., 4., 19., 19.).unwrap());
    paths.push(p.finish().unwrap());
    paths
}

#[test]
fn mask_coverage_matches_pristine_release_across_geometry_and_overlap() {
    let expected = [
        (1, 0x870e8771563b9c69_u64),
        (2, 0xd5e7ade465ca76c1_u64),
        (7, 0x4a403b77eca4a28c_u64),
        (15, 0x2ea9768623557869_u64),
        (16, 0x09aaccd15c843f5c_u64),
        (17, 0x939b6a98d58448e8_u64),
        (31, 0x4bc843c2d41a7d0b_u64),
        (32, 0xe7e4e9b8b8f93c10_u64),
        (63, 0xaac20d7a627da20d_u64),
        (129, 0xdc8568147eb05acc_u64),
    ];
    let paths = paths();
    for (width, expected_hash) in expected {
        let mut hash = 14_695_981_039_346_656_037_u64;
        for path in &paths {
            for linear in [
                [1., 0., 0., 1.],
                [-1., 0., 0., 1.],
                [0., 1., -1., 0.],
                [1., 0.2, 0.3, 1.],
                [0.03, 0., 0., 0.04],
                [3., 0., 0., 2.],
            ] {
                for phase in 0..16 {
                    for rule in [FillRule::Winding, FillRule::EvenOdd] {
                        let mut mask = Mask::new(width, 37).unwrap();
                        for (i, dst) in mask.data_mut().iter_mut().enumerate() {
                            *dst = ((i * 47 + phase * 11) % 256) as u8;
                        }
                        let transform = Transform::from_row(
                            linear[0],
                            linear[1],
                            linear[2],
                            linear[3],
                            (phase / 4) as f32 / 4. - 3.,
                            (phase % 4) as f32 / 4. + 1.,
                        );
                        mask.fill_path(path, rule, true, transform);
                        mask.fill_path(path, rule, true, Transform::from_translate(0.75, -1.25));
                        for byte in mask.data() {
                            hash = (hash ^ u64::from(*byte)).wrapping_mul(1_099_511_628_211);
                        }
                    }
                }
            }
        }
        assert_eq!(
            hash, expected_hash,
            "pristine mask bytes differ at width {width}"
        );
    }
}
