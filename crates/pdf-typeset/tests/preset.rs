//! TS-6 preset-geometry tests: every v1 subset preset gets at least one
//! geometric assertion (key coordinates / segment counts / closedness),
//! plus adjust-value handling, ECMA pin clamps, and the bounding-box
//! degradation policy.

use pdf_typeset::ops::PathSeg;
use pdf_typeset::preset::{is_supported, preset_outline, PresetOutline, SUPPORTED_PRESETS};
use pdf_typeset::Rect;

/// The workhorse fixture: 100 × 60 at offset (10, 20) — `ss = 60`.
fn r1() -> Rect {
    Rect::new(10.0, 20.0, 110.0, 80.0)
}

/// A 100 × 100 square at the origin (star/pentagon factor checks).
fn sq() -> Rect {
    Rect::new(0.0, 0.0, 100.0, 100.0)
}

fn outline(name: &str, rect: Rect, adj: &[(&str, f64)]) -> PresetOutline {
    preset_outline(name, rect, adj)
}

fn segs(name: &str, rect: Rect, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let out = outline(name, rect, adj);
    assert!(!out.degraded, "{name} unexpectedly degraded");
    out.segs
}

/// On-curve endpoints of every Move/Line/Curve segment, in order.
fn endpoints(segs: &[PathSeg]) -> Vec<(f64, f64)> {
    segs.iter()
        .filter_map(|seg| match *seg {
            PathSeg::MoveTo { x, y } | PathSeg::LineTo { x, y } | PathSeg::CurveTo { x, y, .. } => {
                Some((x, y))
            }
            PathSeg::Close => None,
        })
        .collect()
}

fn count_close(segs: &[PathSeg]) -> usize {
    segs.iter().filter(|s| matches!(s, PathSeg::Close)).count()
}

fn count_curves(segs: &[PathSeg]) -> usize {
    segs.iter()
        .filter(|s| matches!(s, PathSeg::CurveTo { .. }))
        .count()
}

fn count_moves(segs: &[PathSeg]) -> usize {
    segs.iter()
        .filter(|s| matches!(s, PathSeg::MoveTo { .. }))
        .count()
}

#[track_caller]
fn assert_pt(got: (f64, f64), want: (f64, f64), tol: f64) {
    assert!(
        (got.0 - want.0).abs() <= tol && (got.1 - want.1).abs() <= tol,
        "point {got:?} != {want:?} (tol {tol})"
    );
}

#[track_caller]
fn assert_pts(segs: &[PathSeg], want: &[(f64, f64)], tol: f64) {
    let got = endpoints(segs);
    assert_eq!(got.len(), want.len(), "vertex count: {got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert_pt(*g, *w, tol);
    }
}

#[track_caller]
fn assert_contains(segs: &[PathSeg], want: (f64, f64), tol: f64) {
    assert!(
        endpoints(segs)
            .iter()
            .any(|g| (g.0 - want.0).abs() <= tol && (g.1 - want.1).abs() <= tol),
        "no endpoint near {want:?} in {:?}",
        endpoints(segs)
    );
}

// --- subset inventory --------------------------------------------------------

#[test]
fn subset_has_35_presets_and_all_resolve_undegraded() {
    assert_eq!(SUPPORTED_PRESETS.len(), 35);
    for name in SUPPORTED_PRESETS {
        assert!(is_supported(name));
        let out = outline(name, r1(), &[]);
        assert!(!out.degraded, "{name} degraded");
        assert!(
            matches!(out.segs.first(), Some(PathSeg::MoveTo { .. })),
            "{name} does not start with MoveTo"
        );
        assert!(out.segs.len() >= 2, "{name} outline too short");
    }
}

#[test]
fn unknown_preset_degrades_to_bounding_box() {
    assert!(!is_supported("cloudCallout"));
    let out = outline("cloudCallout", r1(), &[("adj1", 4_000.0)]);
    assert!(out.degraded);
    assert_pts(
        &out.segs,
        &[(10.0, 20.0), (110.0, 20.0), (110.0, 80.0), (10.0, 80.0)],
        1e-9,
    );
    assert_eq!(count_close(&out.segs), 1);
}

#[test]
fn preset_names_are_not_case_folded() {
    assert!(outline("Rect", r1(), &[]).degraded);
    assert!(outline("ROUNDRECT", r1(), &[]).degraded);
}

// --- basic closed shapes -----------------------------------------------------

#[test]
fn rect_is_the_four_corners() {
    let s = segs("rect", r1(), &[]);
    assert_pts(
        &s,
        &[(10.0, 20.0), (110.0, 20.0), (110.0, 80.0), (10.0, 80.0)],
        1e-9,
    );
    assert_eq!(s.len(), 5);
    assert_eq!(count_close(&s), 1);
}

#[test]
fn round_rect_honors_adj_radius() {
    // adj 25000 on ss = 60 → radius 15.
    let s = segs("roundRect", r1(), &[("adj", 25_000.0)]);
    assert_eq!(s.len(), 10); // M + 4 lines + 4 corner curves + close
    assert_eq!(count_curves(&s), 4);
    assert!(
        matches!(s[0], PathSeg::MoveTo { x, y } if (x - 25.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9)
    );
    assert!(
        matches!(s[1], PathSeg::LineTo { x, y } if (x - 95.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9)
    );
    // First corner curve lands on the right edge at y = top + radius.
    assert!(
        matches!(s[2], PathSeg::CurveTo { x, y, .. } if (x - 110.0).abs() < 1e-9 && (y - 35.0).abs() < 1e-9)
    );
}

#[test]
fn round_rect_default_radius_is_16667_of_short_side() {
    let s = segs("roundRect", r1(), &[]);
    let rd = 60.0 * 16_667.0 / 100_000.0;
    assert!(matches!(s[0], PathSeg::MoveTo { x, .. } if (x - (10.0 + rd)).abs() < 1e-9));
}

#[test]
fn round_rect_pins_adj_to_half_short_side() {
    let clamped = segs("roundRect", r1(), &[("adj", 200_000.0)]);
    let max = segs("roundRect", r1(), &[("adj", 50_000.0)]);
    assert_eq!(clamped, max);
    assert!(matches!(clamped[0], PathSeg::MoveTo { x, .. } if (x - 40.0).abs() < 1e-9));
}

#[test]
fn ellipse_touches_all_four_edge_midpoints() {
    let s = segs("ellipse", r1(), &[]);
    assert_eq!(count_curves(&s), 4);
    assert_eq!(count_close(&s), 1);
    assert_pts(
        &s,
        &[
            (110.0, 50.0), // start: right midpoint
            (60.0, 80.0),  // bottom
            (10.0, 50.0),  // left
            (60.0, 20.0),  // top
            (110.0, 50.0), // back to start
        ],
        1e-9,
    );
}

#[test]
fn ellipse_quarter_arcs_use_the_standard_kappa() {
    let s = segs("ellipse", r1(), &[]);
    let PathSeg::CurveTo { x1, y1, .. } = s[1] else {
        panic!("second seg is not a curve");
    };
    // First control point of the 0°→90° quarter: (cx + rx, cy + ry·kappa).
    assert!((x1 - 110.0).abs() < 1e-9);
    assert!(((y1 - 50.0) / 30.0 - 0.552_284_749_8).abs() < 1e-9);
}

#[test]
fn triangle_apex_follows_adj() {
    assert_pts(
        &segs("triangle", r1(), &[]),
        &[(10.0, 80.0), (60.0, 20.0), (110.0, 80.0)],
        1e-9,
    );
    assert_pts(
        &segs("triangle", r1(), &[("adj", 0.0)]),
        &[(10.0, 80.0), (10.0, 20.0), (110.0, 80.0)],
        1e-9,
    );
    // Pinned to 100000: apex cannot pass the right edge.
    assert_pts(
        &segs("triangle", r1(), &[("adj", 250_000.0)]),
        &[(10.0, 80.0), (110.0, 20.0), (110.0, 80.0)],
        1e-9,
    );
}

#[test]
fn rt_triangle_has_the_right_angle_at_bottom_left() {
    let s = segs("rtTriangle", r1(), &[]);
    assert_pts(&s, &[(10.0, 80.0), (10.0, 20.0), (110.0, 80.0)], 1e-9);
    assert_eq!(count_close(&s), 1);
}

#[test]
fn diamond_is_the_four_edge_midpoints() {
    assert_pts(
        &segs("diamond", r1(), &[]),
        &[(10.0, 50.0), (60.0, 20.0), (110.0, 50.0), (60.0, 80.0)],
        1e-9,
    );
}

#[test]
fn parallelogram_skews_by_ss_fraction() {
    // adj 25000 on ss = 60 → skew 15.
    assert_pts(
        &segs("parallelogram", r1(), &[]),
        &[(10.0, 80.0), (25.0, 20.0), (110.0, 20.0), (95.0, 80.0)],
        1e-9,
    );
}

#[test]
fn trapezoid_insets_the_top_edge() {
    assert_pts(
        &segs("trapezoid", r1(), &[]),
        &[(10.0, 80.0), (25.0, 20.0), (95.0, 20.0), (110.0, 80.0)],
        1e-9,
    );
    // maxAdj = 50000·w/ss caps the inset at half the width.
    assert_pts(
        &segs("trapezoid", r1(), &[("adj", 999_999.0)]),
        &[(10.0, 80.0), (60.0, 20.0), (60.0, 20.0), (110.0, 80.0)],
        1e-6,
    );
}

#[test]
fn pentagon_matches_the_ecma_hf_vf_normalization() {
    let s = segs("pentagon", sq(), &[]);
    assert_pts(
        &s,
        &[
            (50.0, 0.0),     // apex on the top edge
            (100.0, 38.197), // right spike on the right edge
            (80.902, 100.0), // base corner on the bottom edge
            (19.098, 100.0), // base corner
            (0.0, 38.197),   // left spike on the left edge
        ],
        0.05,
    );
    assert_eq!(count_close(&s), 1);
}

#[test]
fn hexagon_insets_follow_adj() {
    assert_pts(
        &segs("hexagon", r1(), &[]),
        &[
            (10.0, 50.0),
            (25.0, 20.0),
            (95.0, 20.0),
            (110.0, 50.0),
            (95.0, 80.0),
            (25.0, 80.0),
        ],
        1e-9,
    );
}

#[test]
fn octagon_cuts_corners_by_default_29289() {
    let d = 60.0 * 29_289.0 / 100_000.0;
    assert_pts(
        &segs("octagon", r1(), &[]),
        &[
            (10.0, 20.0 + d),
            (10.0 + d, 20.0),
            (110.0 - d, 20.0),
            (110.0, 20.0 + d),
            (110.0, 80.0 - d),
            (110.0 - d, 80.0),
            (10.0 + d, 80.0),
            (10.0, 80.0 - d),
        ],
        1e-9,
    );
}

#[test]
fn plus_arm_inset_is_ss_fraction() {
    let s = segs("plus", r1(), &[]);
    assert_pts(
        &s,
        &[
            (10.0, 35.0),
            (25.0, 35.0),
            (25.0, 20.0),
            (95.0, 20.0),
            (95.0, 35.0),
            (110.0, 35.0),
            (110.0, 65.0),
            (95.0, 65.0),
            (95.0, 80.0),
            (25.0, 80.0),
            (25.0, 65.0),
            (10.0, 65.0),
        ],
        1e-9,
    );
    assert_eq!(count_close(&s), 1);
}

// --- arc family ---------------------------------------------------------------

#[test]
fn arc_default_is_an_open_quarter_from_top_to_right() {
    let s = segs("arc", r1(), &[]);
    assert_eq!(count_close(&s), 0);
    assert_eq!(count_curves(&s), 1);
    assert!(
        matches!(s[0], PathSeg::MoveTo { x, y } if (x - 60.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9)
    );
    assert!(
        matches!(s[1], PathSeg::CurveTo { x, y, .. } if (x - 110.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9)
    );
}

#[test]
fn arc_honors_custom_angles() {
    // 0° → 90°: right midpoint sweeping clockwise (y-down) to the bottom.
    let s = segs("arc", r1(), &[("adj1", 0.0), ("adj2", 5_400_000.0)]);
    assert!(
        matches!(s[0], PathSeg::MoveTo { x, y } if (x - 110.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9)
    );
    assert!(
        matches!(s[1], PathSeg::CurveTo { x, y, .. } if (x - 60.0).abs() < 1e-9 && (y - 80.0).abs() < 1e-9)
    );
}

#[test]
fn pie_default_is_a_closed_270_degree_wedge_through_the_center() {
    let s = segs("pie", r1(), &[]);
    assert_eq!(count_curves(&s), 3); // 270° → 3 quarter segments
    assert_eq!(count_close(&s), 1);
    assert!(
        matches!(s[0], PathSeg::MoveTo { x, y } if (x - 110.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9)
    );
    // The wedge closes over the center.
    assert!(
        s.iter()
            .any(|g| matches!(*g, PathSeg::LineTo { x, y } if (x - 60.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9))
    );
}

#[test]
fn pie_sweep_wraps_across_zero() {
    // 350° → 20°: a 30° slice, one Bézier segment.
    let s = segs(
        "pie",
        r1(),
        &[("adj1", 21_000_000.0), ("adj2", 1_200_000.0)],
    );
    assert_eq!(count_curves(&s), 1);
}

#[test]
fn chord_closes_the_arc_without_the_center() {
    let s = segs("chord", r1(), &[]);
    assert_eq!(count_curves(&s), 3); // 45° → 270° = 225° sweep
    assert_eq!(count_close(&s), 1);
    assert!(!s.iter().any(|g| matches!(g, PathSeg::LineTo { .. })));
    let want = (
        60.0 + 50.0 * 45f64.to_radians().cos(),
        50.0 + 30.0 * 45f64.to_radians().sin(),
    );
    assert!(
        matches!(s[0], PathSeg::MoveTo { x, y } if (x - want.0).abs() < 1e-9 && (y - want.1).abs() < 1e-9)
    );
}

#[test]
fn donut_has_two_subpaths_with_opposite_windings() {
    let s = segs("donut", r1(), &[]);
    assert_eq!(count_moves(&s), 2);
    assert_eq!(count_close(&s), 2);
    assert_eq!(count_curves(&s), 8);
    // Ring thickness default 25000 · ss = 15: inner subpath starts at
    // (cx + (rx − 15), cy) and immediately sweeps *up* (reverse winding).
    let inner_start = s
        .iter()
        .skip(6)
        .find(|g| matches!(g, PathSeg::MoveTo { .. }));
    assert!(
        matches!(inner_start, Some(&PathSeg::MoveTo { x, y }) if (x - 95.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9)
    );
    let PathSeg::CurveTo { x, y, .. } = s[7] else {
        panic!("inner first quarter is not a curve");
    };
    assert_pt((x, y), (60.0, 35.0), 1e-9); // top of the inner ring
}

// --- connectors ----------------------------------------------------------------

#[test]
fn line_and_straight_connector_are_the_diagonal() {
    for name in ["line", "straightConnector1"] {
        let s = segs(name, r1(), &[]);
        assert_eq!(s.len(), 2);
        assert_eq!(count_close(&s), 0);
        assert_pts(&s, &[(10.0, 20.0), (110.0, 80.0)], 1e-9);
    }
}

#[test]
fn bent_connector2_is_an_open_elbow() {
    let s = segs("bentConnector2", r1(), &[]);
    assert_eq!(count_close(&s), 0);
    assert_pts(&s, &[(10.0, 20.0), (110.0, 20.0), (110.0, 80.0)], 1e-9);
}

#[test]
fn bent_connector3_elbow_follows_adj1() {
    assert_pts(
        &segs("bentConnector3", r1(), &[]),
        &[(10.0, 20.0), (60.0, 20.0), (60.0, 80.0), (110.0, 80.0)],
        1e-9,
    );
    assert_pts(
        &segs("bentConnector3", r1(), &[("adj1", 25_000.0)]),
        &[(10.0, 20.0), (35.0, 20.0), (35.0, 80.0), (110.0, 80.0)],
        1e-9,
    );
}

// --- arrows --------------------------------------------------------------------

#[test]
fn right_arrow_default_geometry() {
    // ss = 60: shaft half-thickness 15, head length 30.
    assert_pts(
        &segs("rightArrow", r1(), &[]),
        &[
            (10.0, 35.0),
            (80.0, 35.0),
            (80.0, 20.0),
            (110.0, 50.0),
            (80.0, 80.0),
            (80.0, 65.0),
            (10.0, 65.0),
        ],
        1e-9,
    );
}

#[test]
fn right_arrow_head_is_pinned_to_the_width() {
    // maxAdj2 = 100000·w/ss caps the head at the full width.
    let s = segs("rightArrow", r1(), &[("adj2", 500_000.0)]);
    assert_contains(&s, (10.0, 20.0), 1e-6); // head base reaches the left edge
    assert_contains(&s, (110.0, 50.0), 1e-9); // apex
}

#[test]
fn left_arrow_points_left() {
    assert_pts(
        &segs("leftArrow", r1(), &[]),
        &[
            (110.0, 35.0),
            (40.0, 35.0),
            (40.0, 20.0),
            (10.0, 50.0),
            (40.0, 80.0),
            (40.0, 65.0),
            (110.0, 65.0),
        ],
        1e-9,
    );
}

#[test]
fn up_arrow_points_up() {
    assert_pts(
        &segs("upArrow", r1(), &[]),
        &[
            (10.0, 50.0),
            (60.0, 20.0),
            (110.0, 50.0),
            (75.0, 50.0),
            (75.0, 80.0),
            (45.0, 80.0),
            (45.0, 50.0),
        ],
        1e-9,
    );
}

#[test]
fn down_arrow_points_down() {
    assert_pts(
        &segs("downArrow", r1(), &[]),
        &[
            (10.0, 50.0),
            (60.0, 80.0),
            (110.0, 50.0),
            (75.0, 50.0),
            (75.0, 20.0),
            (45.0, 20.0),
            (45.0, 50.0),
        ],
        1e-9,
    );
}

#[test]
fn left_right_arrow_has_two_apexes() {
    assert_pts(
        &segs("leftRightArrow", r1(), &[]),
        &[
            (10.0, 50.0),
            (40.0, 20.0),
            (40.0, 35.0),
            (80.0, 35.0),
            (80.0, 20.0),
            (110.0, 50.0),
            (80.0, 80.0),
            (80.0, 65.0),
            (40.0, 65.0),
            (40.0, 80.0),
        ],
        1e-9,
    );
}

#[test]
fn arrow_shaft_thickness_follows_adj1() {
    // adj1 = 100000 → shaft fills the height: shaft edges at vc ± ss/2.
    let s = segs("rightArrow", r1(), &[("adj1", 100_000.0)]);
    assert_contains(&s, (10.0, 20.0), 1e-9);
    assert_contains(&s, (10.0, 80.0), 1e-9);
}

// --- stars / chevrons / callout / flowchart -------------------------------------

#[test]
fn star4_spikes_sit_on_the_axes() {
    let s = segs("star4", r1(), &[]);
    let pts = endpoints(&s);
    assert_eq!(pts.len(), 8);
    assert_pt(pts[0], (60.0, 20.0), 1e-9); // top spike
    assert_pt(pts[2], (110.0, 50.0), 1e-9); // right
    assert_pt(pts[4], (60.0, 80.0), 1e-9); // bottom
    assert_pt(pts[6], (10.0, 50.0), 1e-9); // left
                                           // Default adj 12500 → inner radius ratio 1/4, first inner vertex at 45°.
    let inv = 2f64.sqrt() / 2.0;
    assert_pt(
        pts[1],
        (60.0 + 50.0 / 4.0 * inv, 50.0 - 30.0 / 4.0 * inv),
        1e-9,
    );
}

#[test]
fn star5_outer_ring_matches_the_pentagon_and_inner_follows_adj() {
    let s = segs("star5", sq(), &[]);
    let pts = endpoints(&s);
    assert_eq!(pts.len(), 10);
    assert_pt(pts[0], (50.0, 0.0), 0.05);
    assert_pt(pts[2], (100.0, 38.197), 0.05);
    assert_pt(pts[4], (80.902, 100.0), 0.05);
    // Default adj 19098 → the regular-star inner ratio 0.38196 at −54°.
    assert_pt(pts[1], (61.803, 38.197), 0.05);
}

#[test]
fn star6_side_spikes_touch_the_edges() {
    let s = segs("star6", sq(), &[]);
    let pts = endpoints(&s);
    assert_eq!(pts.len(), 12);
    assert_pt(pts[0], (50.0, 0.0), 1e-6); // top spike
    assert_pt(pts[2], (100.0, 25.0), 1e-3); // upper-right spike on the edge
    assert_pt(pts[6], (50.0, 100.0), 1e-6); // bottom spike
}

#[test]
fn chevron_default_geometry() {
    assert_pts(
        &segs("chevron", r1(), &[]),
        &[
            (10.0, 20.0),
            (80.0, 20.0),
            (110.0, 50.0),
            (80.0, 80.0),
            (10.0, 80.0),
            (40.0, 50.0),
        ],
        1e-9,
    );
}

#[test]
fn home_plate_default_geometry() {
    assert_pts(
        &segs("homePlate", r1(), &[]),
        &[
            (10.0, 20.0),
            (80.0, 20.0),
            (110.0, 50.0),
            (80.0, 80.0),
            (10.0, 80.0),
        ],
        1e-9,
    );
}

#[test]
fn wedge_rect_callout_default_wedge_points_below() {
    let s = segs("wedgeRectCallout", r1(), &[]);
    assert_eq!(endpoints(&s).len(), 16);
    assert_eq!(count_close(&s), 1);
    // Default adj (−20833, 62500): apex at center + (−0.20833·w, 0.625·h).
    assert_contains(&s, (60.0 - 20.833, 50.0 + 37.5), 1e-3);
    // The rectangle body keeps its four corners.
    for corner in [(10.0, 20.0), (110.0, 20.0), (110.0, 80.0), (10.0, 80.0)] {
        assert_contains(&s, corner, 1e-9);
    }
}

#[test]
fn wedge_rect_callout_wedge_switches_to_the_right_edge() {
    // Target far right of center: the wedge must leave through the right edge.
    let s = segs(
        "wedgeRectCallout",
        r1(),
        &[("adj1", 200_000.0), ("adj2", 0.0)],
    );
    assert_contains(&s, (60.0 + 200.0, 50.0), 1e-6);
    // And the bottom-edge candidate collapses onto the bottom edge.
    assert!(
        endpoints(&s).iter().all(|&(_, y)| y <= 80.0 + 1e-9),
        "bottom edge must stay flat when the wedge points right"
    );
}

#[test]
fn flowchart_process_and_decision_reuse_rect_and_diamond() {
    assert_eq!(segs("flowChartProcess", r1(), &[]), segs("rect", r1(), &[]));
    assert_eq!(
        segs("flowChartDecision", r1(), &[]),
        segs("diamond", r1(), &[])
    );
}

#[test]
fn flowchart_terminator_is_a_stadium_with_ecma_insets() {
    let s = segs("flowChartTerminator", r1(), &[]);
    let inset = 100.0 * 3_475.0 / 21_600.0;
    assert_eq!(count_curves(&s), 4); // two 180° caps, two quarters each
    assert_eq!(count_close(&s), 1);
    assert!(
        matches!(s[0], PathSeg::MoveTo { x, y } if (x - (10.0 + inset)).abs() < 1e-9 && (y - 20.0).abs() < 1e-9)
    );
    assert!(
        matches!(s[1], PathSeg::LineTo { x, y } if (x - (110.0 - inset)).abs() < 1e-9 && (y - 20.0).abs() < 1e-9)
    );
    // The right cap ends at the bottom of the straight run.
    assert!(
        matches!(s[3], PathSeg::CurveTo { x, y, .. } if (x - (110.0 - inset)).abs() < 1e-9 && (y - 80.0).abs() < 1e-9)
    );
}

#[test]
fn flowchart_data_is_the_ecma_one_fifth_parallelogram() {
    assert_pts(
        &segs("flowChartData", r1(), &[]),
        &[(10.0, 80.0), (30.0, 20.0), (110.0, 20.0), (90.0, 80.0)],
        1e-9,
    );
}

// --- robustness ------------------------------------------------------------------

#[test]
fn unknown_adj_names_fall_back_to_defaults() {
    assert_eq!(
        segs("roundRect", r1(), &[("bogus", 1.0)]),
        segs("roundRect", r1(), &[])
    );
}

#[test]
fn zero_size_rect_never_panics() {
    let r = Rect::new(5.0, 5.0, 5.0, 5.0);
    for name in SUPPORTED_PRESETS {
        let out = outline(
            name,
            r,
            &[("adj", 25_000.0), ("adj1", 50_000.0), ("adj2", 50_000.0)],
        );
        assert!(!out.degraded, "{name}");
        for (x, y) in endpoints(&out.segs) {
            assert!(
                x.is_finite() && y.is_finite(),
                "{name} produced non-finite points"
            );
        }
    }
}

#[test]
fn outline_is_offset_by_the_rect_origin() {
    let a = outline("diamond", Rect::new(0.0, 0.0, 100.0, 60.0), &[]);
    let b = outline("diamond", r1(), &[]);
    let a_pts = endpoints(&a.segs);
    let b_pts = endpoints(&b.segs);
    for (pa, pb) in a_pts.iter().zip(&b_pts) {
        assert_pt((pa.0 + 10.0, pa.1 + 20.0), *pb, 1e-9);
    }
}
