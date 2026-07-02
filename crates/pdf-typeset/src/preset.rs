//! OOXML `prstGeom` preset-shape outlines — the TS-6 v1 subset (PRD §10).
//!
//! Pure geometry: [`preset_outline`] maps a preset name + bounding rect + raw
//! `avLst` adjust values to [`PathSeg`]s in **absolute top-left page
//! coordinates** (the [`crate::ops`] convention, y grows downward). Painting
//! (fill / stroke / alpha / even-odd) and placement transforms (`rot` /
//! `flipH` / `flipV` → `Op::Group { transform }`) stay with the caller, as
//! does appending `ExportWarning::PresetDegraded` whenever
//! [`PresetOutline::degraded`] is set.
//!
//! Guide formulas follow ECMA-376 `presetShapeDefinitions.xml`: adjust values
//! arrive as the raw `<a:gd fmla="val …"/>` numbers (100000 = 100%; angles in
//! 60000ths of a degree, clockwise from the +x axis — which is the natural
//! parameterization in y-down coordinates) and are clamped with the same
//! `pin` bounds the spec uses. PRD-allowed approximation: arc points are the
//! *parametric* ellipse points `c + (rx·cosθ, ry·sinθ)` rather than ECMA's
//! ray-intersection angles — identical for circles, near-identical for mild
//! eccentricity. Arcs become cubic Béziers via `α = 4/3·tan(Δ/4)`, which for
//! a 90° sweep is exactly the standard circle kappa `0.5522847498`.

use crate::ops::{translate_segs, PathSeg};
use crate::Rect;

/// The v1 preset subset (PRD §10 locked decision, ~35 presets). Every other
/// `prstGeom` value degrades to its bounding-box rectangle.
pub const SUPPORTED_PRESETS: &[&str] = &[
    "rect",
    "roundRect",
    "ellipse",
    "line",
    "straightConnector1",
    "bentConnector2",
    "bentConnector3",
    "triangle",
    "rtTriangle",
    "diamond",
    "parallelogram",
    "trapezoid",
    "pentagon",
    "hexagon",
    "octagon",
    "plus",
    "arc",
    "pie",
    "chord",
    "donut",
    "rightArrow",
    "leftArrow",
    "upArrow",
    "downArrow",
    "leftRightArrow",
    "star4",
    "star5",
    "star6",
    "chevron",
    "homePlate",
    "wedgeRectCallout",
    "flowChartProcess",
    "flowChartDecision",
    "flowChartTerminator",
    "flowChartData",
];

/// Whether `name` is in the v1 preset subset (exact match — `prstGeom`
/// values are a fixed OOXML enum, never case-folded).
#[must_use]
pub fn is_supported(name: &str) -> bool {
    SUPPORTED_PRESETS.contains(&name)
}

/// The outline of one resolved preset.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetOutline {
    /// The outline segments, in absolute top-left page coordinates. Closed
    /// shapes end their subpaths with [`PathSeg::Close`]; connectors (`line`,
    /// `straightConnector1`, `bentConnector2/3`) and `arc` are open. `donut`
    /// emits its inner subpath in the opposite winding, so it fills correctly
    /// under nonzero winding *and* even-odd.
    pub segs: Vec<PathSeg>,
    /// `true` when `name` was outside the v1 subset and the segments are the
    /// bounding-box fallback — the caller must append
    /// `ExportWarning::PresetDegraded { preset }`.
    pub degraded: bool,
}

/// Resolves a `prstGeom` preset to its outline inside `rect`.
///
/// `adj` carries the shape's raw `avLst` adjust values as `(name, value)`
/// pairs (e.g. `("adj", 25000.0)`, `("adj1", 16_200_000.0)`); missing names
/// take the ECMA-376 defaults. Unknown preset names return the bounding-box
/// rectangle with [`PresetOutline::degraded`] set (degrade-never-panic).
#[must_use]
pub fn preset_outline(name: &str, rect: Rect, adj: &[(&str, f64)]) -> PresetOutline {
    let w = rect.x1 - rect.x0;
    let h = rect.y1 - rect.y0;
    let ss = w.min(h);
    let segs = match name {
        "rect" | "flowChartProcess" => Some(rect_path(w, h)),
        "roundRect" => Some(round_rect(w, h, ss, adj)),
        "ellipse" => Some(ellipse(w, h)),
        "line" | "straightConnector1" => Some(polyline(&[(0.0, 0.0), (w, h)])),
        "bentConnector2" => Some(polyline(&[(0.0, 0.0), (w, 0.0), (w, h)])),
        "bentConnector3" => Some(bent_connector3(w, h, adj)),
        "triangle" => Some(triangle(w, h, adj)),
        "rtTriangle" => Some(polygon(&[(0.0, h), (0.0, 0.0), (w, h)])),
        "diamond" | "flowChartDecision" => Some(diamond(w, h)),
        "parallelogram" => Some(parallelogram(w, h, ss, adj)),
        "trapezoid" => Some(trapezoid(w, h, ss, adj)),
        "pentagon" => Some(pentagon(w, h)),
        "hexagon" => Some(hexagon(w, h, ss, adj)),
        "octagon" => Some(octagon(w, h, ss, adj)),
        "plus" => Some(plus(w, h, ss, adj)),
        "arc" => Some(arc(w, h, adj)),
        "pie" => Some(pie(w, h, adj)),
        "chord" => Some(chord(w, h, adj)),
        "donut" => Some(donut(w, h, ss, adj)),
        "rightArrow" => Some(right_arrow(w, h, ss, adj)),
        "leftArrow" => Some(left_arrow(w, h, ss, adj)),
        "upArrow" => Some(up_arrow(w, h, ss, adj)),
        "downArrow" => Some(down_arrow(w, h, ss, adj)),
        "leftRightArrow" => Some(left_right_arrow(w, h, ss, adj)),
        "star4" => Some(star4(w, h, adj)),
        "star5" => Some(star5(w, h, adj)),
        "star6" => Some(star6(w, h, adj)),
        "chevron" => Some(chevron(w, h, ss, adj)),
        "homePlate" => Some(home_plate(w, h, ss, adj)),
        "wedgeRectCallout" => Some(wedge_rect_callout(w, h, adj)),
        "flowChartTerminator" => Some(flow_chart_terminator(w, h)),
        "flowChartData" => Some(polygon(&[(0.0, h), (w / 5.0, 0.0), (w, 0.0), (w * 0.8, h)])),
        _ => None,
    };
    let (mut segs, degraded) = match segs {
        Some(segs) => (segs, false),
        None => (rect_path(w, h), true),
    };
    translate_segs(&mut segs, rect.x0, rect.y0);
    PresetOutline { segs, degraded }
}

// --- guide-formula helpers ---------------------------------------------------

/// One OOXML angle unit is 1/60000 of a degree.
const OOXML_DEG: f64 = 60_000.0;

/// Looks up a raw `avLst` adjust value by name, with the ECMA default.
fn lookup(adj: &[(&str, f64)], name: &str, default: f64) -> f64 {
    adj.iter()
        .find(|(n, _)| *n == name)
        .map_or(default, |&(_, v)| v)
}

/// ECMA `pin lo v hi` — clamps `v` into `[lo, hi]`, degrading safely when the
/// bound itself is degenerate (`hi ≤ lo`, e.g. a zero-size rect made a
/// `maxAdj` guide meaningless).
fn pin(lo: f64, v: f64, hi: f64) -> f64 {
    if hi <= lo {
        return lo;
    }
    v.max(lo).min(hi)
}

/// `100000·numer/ss` — the recurring `maxAdj` guide, 0 when `ss` is degenerate.
fn max_adj(scale: f64, numer: f64, ss: f64) -> f64 {
    if ss > 0.0 {
        scale * numer / ss
    } else {
        0.0
    }
}

/// The parametric ellipse point at `deg` degrees (y-down: clockwise visually).
fn arc_point(cx: f64, cy: f64, rx: f64, ry: f64, deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (cx + rx * a.cos(), cy + ry * a.sin())
}

/// Appends cubic Béziers approximating the parametric ellipse arc from
/// `start_deg` sweeping `sweep_deg` (≤ 90° per segment; `α = 4/3·tan(Δ/4)`,
/// which for an exact quarter sweep is the standard circle kappa
/// `0.5522847498`). The current point must already be the arc start.
fn push_arc(
    segs: &mut Vec<PathSeg>,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    start_deg: f64,
    sweep_deg: f64,
) {
    if sweep_deg == 0.0 {
        return;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (sweep_deg.abs() / 90.0).ceil().max(1.0) as usize;
    #[allow(clippy::cast_precision_loss)]
    let step = sweep_deg / n as f64;
    for i in 0..n {
        #[allow(clippy::cast_precision_loss)]
        let a = (start_deg + step * i as f64).to_radians();
        let b = a + step.to_radians();
        let alpha = 4.0 / 3.0 * ((b - a) / 4.0).tan();
        let (sin_a, cos_a) = a.sin_cos();
        let (sin_b, cos_b) = b.sin_cos();
        segs.push(PathSeg::CurveTo {
            x1: cx + rx * (cos_a - alpha * sin_a),
            y1: cy + ry * (sin_a + alpha * cos_a),
            x2: cx + rx * (cos_b + alpha * sin_b),
            y2: cy + ry * (sin_b - alpha * cos_b),
            x: cx + rx * cos_b,
            y: cy + ry * sin_b,
        });
    }
}

/// The ECMA `stAng`/`swAng` pair for arc/pie/chord, in degrees: start pinned
/// into one revolution, sweep normalized positive (`sw11 > 0 ? sw11 : sw12` —
/// equal angles mean a full 360° sweep).
fn angle_pair(adj: &[(&str, f64)], d1: f64, d2: f64) -> (f64, f64) {
    let st = pin(0.0, lookup(adj, "adj1", d1), 21_599_999.0);
    let en = pin(0.0, lookup(adj, "adj2", d2), 21_599_999.0);
    let sw = en - st;
    let sw = if sw > 0.0 { sw } else { sw + 21_600_000.0 };
    (st / OOXML_DEG, sw / OOXML_DEG)
}

/// A closed polygon through `pts` (local coordinates).
fn polygon(pts: &[(f64, f64)]) -> Vec<PathSeg> {
    let mut segs = polyline(pts);
    segs.push(PathSeg::Close);
    segs
}

/// An open polyline through `pts` (local coordinates).
fn polyline(pts: &[(f64, f64)]) -> Vec<PathSeg> {
    pts.iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            if i == 0 {
                PathSeg::MoveTo { x, y }
            } else {
                PathSeg::LineTo { x, y }
            }
        })
        .collect()
}

/// Alternating outer/inner star vertices around center `(cx, cy)`: `n`
/// spikes on the `(orx, ory)` ellipse, outer first at `start_deg`, inner
/// vertices on the `(irx, iry)` ellipse midway between spikes.
fn star_points(
    (cx, cy): (f64, f64),
    (orx, ory): (f64, f64),
    (irx, iry): (f64, f64),
    n: usize,
    start_deg: f64,
) -> Vec<(f64, f64)> {
    #[allow(clippy::cast_precision_loss)]
    let step = 360.0 / n as f64;
    let mut pts = Vec::with_capacity(2 * n);
    for k in 0..n {
        #[allow(clippy::cast_precision_loss)]
        let base = start_deg + step * k as f64;
        pts.push(arc_point(cx, cy, orx, ory, base));
        pts.push(arc_point(cx, cy, irx, iry, base + step / 2.0));
    }
    pts
}

// --- shape builders (local coordinates: (0,0)..(w,h)) ------------------------

fn rect_path(w: f64, h: f64) -> Vec<PathSeg> {
    polygon(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)])
}

/// `roundRect` — `adj` (default 16667) = corner radius as a fraction of the
/// short side, pinned to `[0, 50000]`.
fn round_rect(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 16_667.0), 50_000.0);
    let rd = ss * a / 100_000.0;
    let mut segs = vec![
        PathSeg::MoveTo { x: rd, y: 0.0 },
        PathSeg::LineTo { x: w - rd, y: 0.0 },
    ];
    push_arc(&mut segs, w - rd, rd, rd, rd, 270.0, 90.0);
    segs.push(PathSeg::LineTo { x: w, y: h - rd });
    push_arc(&mut segs, w - rd, h - rd, rd, rd, 0.0, 90.0);
    segs.push(PathSeg::LineTo { x: rd, y: h });
    push_arc(&mut segs, rd, h - rd, rd, rd, 90.0, 90.0);
    segs.push(PathSeg::LineTo { x: 0.0, y: rd });
    push_arc(&mut segs, rd, rd, rd, rd, 180.0, 90.0);
    segs.push(PathSeg::Close);
    segs
}

fn ellipse(w: f64, h: f64) -> Vec<PathSeg> {
    let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let (x, y) = arc_point(cx, cy, rx, ry, 0.0);
    let mut segs = vec![PathSeg::MoveTo { x, y }];
    push_arc(&mut segs, cx, cy, rx, ry, 0.0, 360.0);
    segs.push(PathSeg::Close);
    segs
}

/// `bentConnector3` — `adj1` (default 50000) = elbow x as a fraction of the
/// width (unpinned: connectors may route outside their bounds).
fn bent_connector3(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let x1 = w * lookup(adj, "adj1", 50_000.0) / 100_000.0;
    polyline(&[(0.0, 0.0), (x1, 0.0), (x1, h), (w, h)])
}

/// `triangle` — `adj` (default 50000) = apex x as a fraction of the width,
/// pinned to `[0, 100000]`.
fn triangle(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 50_000.0), 100_000.0);
    polygon(&[(0.0, h), (w * a / 100_000.0, 0.0), (w, h)])
}

fn diamond(w: f64, h: f64) -> Vec<PathSeg> {
    let (hc, vc) = (w / 2.0, h / 2.0);
    polygon(&[(0.0, vc), (hc, 0.0), (w, vc), (hc, h)])
}

/// `parallelogram` — `adj` (default 25000): top-edge skew `ss·a/100000`,
/// pinned to `[0, 100000·w/ss]`.
fn parallelogram(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 25_000.0), max_adj(100_000.0, w, ss));
    let x2 = ss * a / 100_000.0;
    polygon(&[(0.0, h), (x2, 0.0), (w, 0.0), (w - x2, h)])
}

/// `trapezoid` — `adj` (default 25000): per-side top inset `ss·a/100000`,
/// pinned to `[0, 50000·w/ss]` (long edge at the bottom, per ECMA).
fn trapezoid(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 25_000.0), max_adj(50_000.0, w, ss));
    let x2 = ss * a / 100_000.0;
    polygon(&[(0.0, h), (x2, 0.0), (w - x2, 0.0), (w, h)])
}

/// The ECMA pentagon/star5 width factor `hf = 105146/100000`.
const PENTA_HF: f64 = 1.051_46;
/// The ECMA pentagon/star5 height factor `vf = 110557/100000`.
const PENTA_VF: f64 = 1.105_57;
/// The ECMA star6 width factor `hf = 115470/100000`.
const HEX_HF: f64 = 1.154_70;

/// The five regular-pentagon vertices scaled to fill the rect (ECMA `hf`/`vf`
/// normalization: apex on the top edge, side spikes on the left/right edges,
/// base corners on the bottom edge).
fn pentagon_points(w: f64, h: f64) -> Vec<(f64, f64)> {
    let (rx, ry) = (w / 2.0 * PENTA_HF, h / 2.0 * PENTA_VF);
    let (cx, cy) = (w / 2.0, h / 2.0 * PENTA_VF);
    (0..5)
        .map(|k| arc_point(cx, cy, rx, ry, -90.0 + 72.0 * f64::from(k)))
        .collect()
}

fn pentagon(w: f64, h: f64) -> Vec<PathSeg> {
    polygon(&pentagon_points(w, h))
}

/// `hexagon` — `adj` (default 25000): per-side x inset `ss·a/100000`,
/// pinned to `[0, 50000·w/ss]`.
fn hexagon(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 25_000.0), max_adj(50_000.0, w, ss));
    let x1 = ss * a / 100_000.0;
    let vc = h / 2.0;
    polygon(&[
        (0.0, vc),
        (x1, 0.0),
        (w - x1, 0.0),
        (w, vc),
        (w - x1, h),
        (x1, h),
    ])
}

/// `octagon` — `adj` (default 29289): corner cut `ss·a/100000`, pinned to
/// `[0, 50000]`.
fn octagon(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 29_289.0), 50_000.0);
    let d = ss * a / 100_000.0;
    polygon(&[
        (0.0, d),
        (d, 0.0),
        (w - d, 0.0),
        (w, d),
        (w, h - d),
        (w - d, h),
        (d, h),
        (0.0, h - d),
    ])
}

/// `plus` — `adj` (default 25000): arm inset `ss·a/100000`, pinned to
/// `[0, 50000]`.
fn plus(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 25_000.0), 50_000.0);
    let d = ss * a / 100_000.0;
    polygon(&[
        (0.0, d),
        (d, d),
        (d, 0.0),
        (w - d, 0.0),
        (w - d, d),
        (w, d),
        (w, h - d),
        (w - d, h - d),
        (w - d, h),
        (d, h),
        (d, h - d),
        (0.0, h - d),
    ])
}

/// `arc` — `adj1`/`adj2` (defaults 16200000/0): an **open** elliptical arc
/// from `stAng` sweeping clockwise to `enAng` (stroke intent; a fill closes
/// the chord implicitly).
fn arc(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (st, sw) = angle_pair(adj, 16_200_000.0, 0.0);
    let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let (x, y) = arc_point(cx, cy, rx, ry, st);
    let mut segs = vec![PathSeg::MoveTo { x, y }];
    push_arc(&mut segs, cx, cy, rx, ry, st, sw);
    segs
}

/// `pie` — `adj1`/`adj2` (defaults 0/16200000): arc + two radii through the
/// center, closed.
fn pie(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (st, sw) = angle_pair(adj, 0.0, 16_200_000.0);
    let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let (x, y) = arc_point(cx, cy, rx, ry, st);
    let mut segs = vec![PathSeg::MoveTo { x, y }];
    push_arc(&mut segs, cx, cy, rx, ry, st, sw);
    segs.push(PathSeg::LineTo { x: cx, y: cy });
    segs.push(PathSeg::Close);
    segs
}

/// `chord` — `adj1`/`adj2` (defaults 2700000/16200000): arc closed straight
/// across.
fn chord(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (st, sw) = angle_pair(adj, 2_700_000.0, 16_200_000.0);
    let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let (x, y) = arc_point(cx, cy, rx, ry, st);
    let mut segs = vec![PathSeg::MoveTo { x, y }];
    push_arc(&mut segs, cx, cy, rx, ry, st, sw);
    segs.push(PathSeg::Close);
    segs
}

/// `donut` — `adj` (default 25000): ring thickness `ss·a/100000`, pinned to
/// `[0, 50000]`; inner subpath wound opposite the outer one.
fn donut(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 25_000.0), 50_000.0);
    let dr = ss * a / 100_000.0;
    let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let mut segs = ellipse(w, h);
    let (irx, iry) = ((rx - dr).max(0.0), (ry - dr).max(0.0));
    let (x, y) = arc_point(cx, cy, irx, iry, 0.0);
    segs.push(PathSeg::MoveTo { x, y });
    push_arc(&mut segs, cx, cy, irx, iry, 0.0, -360.0);
    segs.push(PathSeg::Close);
    segs
}

/// The shared arrow guides: `(shaft half-thickness, head length)` from
/// `adj1`/`adj2` (defaults 50000/50000); `axis` is the arrow-direction extent
/// (`w` for horizontal arrows) bounding `maxAdj2`, `head_scale` is 100000 for
/// single heads and 50000 for double heads.
fn arrow_guides(axis: f64, ss: f64, head_scale: f64, adj: &[(&str, f64)]) -> (f64, f64) {
    let a1 = pin(0.0, lookup(adj, "adj1", 50_000.0), 100_000.0);
    let a2 = pin(
        0.0,
        lookup(adj, "adj2", 50_000.0),
        max_adj(head_scale, axis, ss),
    );
    (ss * a1 / 200_000.0, ss * a2 / 100_000.0)
}

fn right_arrow(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (dy, head) = arrow_guides(w, ss, 100_000.0, adj);
    let (vc, x1) = (h / 2.0, w - head);
    polygon(&[
        (0.0, vc - dy),
        (x1, vc - dy),
        (x1, 0.0),
        (w, vc),
        (x1, h),
        (x1, vc + dy),
        (0.0, vc + dy),
    ])
}

fn left_arrow(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (dy, head) = arrow_guides(w, ss, 100_000.0, adj);
    let vc = h / 2.0;
    polygon(&[
        (w, vc - dy),
        (head, vc - dy),
        (head, 0.0),
        (0.0, vc),
        (head, h),
        (head, vc + dy),
        (w, vc + dy),
    ])
}

fn up_arrow(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (dx, head) = arrow_guides(h, ss, 100_000.0, adj);
    let hc = w / 2.0;
    polygon(&[
        (0.0, head),
        (hc, 0.0),
        (w, head),
        (hc + dx, head),
        (hc + dx, h),
        (hc - dx, h),
        (hc - dx, head),
    ])
}

fn down_arrow(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (dx, head) = arrow_guides(h, ss, 100_000.0, adj);
    let (hc, y2) = (w / 2.0, h - head);
    polygon(&[
        (0.0, y2),
        (hc, h),
        (w, y2),
        (hc + dx, y2),
        (hc + dx, 0.0),
        (hc - dx, 0.0),
        (hc - dx, y2),
    ])
}

fn left_right_arrow(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (dy, head) = arrow_guides(w, ss, 50_000.0, adj);
    let (vc, x2, x3) = (h / 2.0, head, w - head);
    polygon(&[
        (0.0, vc),
        (x2, 0.0),
        (x2, vc - dy),
        (x3, vc - dy),
        (x3, 0.0),
        (w, vc),
        (x3, h),
        (x3, vc + dy),
        (x2, vc + dy),
        (x2, h),
    ])
}

/// `star4` — `adj` (default 12500): inner radius = `a/50000` of the outer,
/// pinned to `[0, 50000]`; spikes on the axes.
fn star4(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 12_500.0), 50_000.0);
    let (rx, ry) = (w / 2.0, h / 2.0);
    let r = a / 50_000.0;
    polygon(&star_points((rx, ry), (rx, ry), (rx * r, ry * r), 4, -90.0))
}

/// `star5` — `adj` (default 19098): inner radius = `a/50000` of the outer;
/// outer vertices are the ECMA `hf`/`vf`-normalized pentagon points.
fn star5(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 19_098.0), 50_000.0);
    let (rx, ry) = (w / 2.0 * PENTA_HF, h / 2.0 * PENTA_VF);
    let (cx, cy) = (w / 2.0, h / 2.0 * PENTA_VF);
    let r = a / 50_000.0;
    polygon(&star_points((cx, cy), (rx, ry), (rx * r, ry * r), 5, -90.0))
}

/// `star6` — `adj` (default 28868): inner radius = `a/50000` of the outer;
/// outer x-radius carries the ECMA `hf` factor so the side spikes touch the
/// rect edges.
fn star6(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 28_868.0), 50_000.0);
    let (rx, ry) = (w / 2.0 * HEX_HF, h / 2.0);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let r = a / 50_000.0;
    polygon(&star_points((cx, cy), (rx, ry), (rx * r, ry * r), 6, -90.0))
}

/// `chevron` — `adj` (default 50000): point depth `ss·a/100000`, pinned to
/// `[0, 100000·w/ss]`.
fn chevron(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 50_000.0), max_adj(100_000.0, w, ss));
    let (x1, vc) = (ss * a / 100_000.0, h / 2.0);
    polygon(&[
        (0.0, 0.0),
        (w - x1, 0.0),
        (w, vc),
        (w - x1, h),
        (0.0, h),
        (x1, vc),
    ])
}

/// `homePlate` — `adj` (default 50000): point depth `ss·a/100000`, pinned to
/// `[0, 100000·w/ss]`.
fn home_plate(w: f64, h: f64, ss: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let a = pin(0.0, lookup(adj, "adj", 50_000.0), max_adj(100_000.0, w, ss));
    let (x1, vc) = (ss * a / 100_000.0, h / 2.0);
    polygon(&[(0.0, 0.0), (w - x1, 0.0), (w, vc), (w - x1, h), (0.0, h)])
}

/// `wedgeRectCallout` — `adj1`/`adj2` (defaults −20833/62500): the callout
/// target offset from the center as fractions of w/h (unpinned — targets sit
/// outside the shape). The ECMA guides pick one edge for the wedge; the other
/// three candidate apexes collapse onto their edges (collinear, harmless).
fn wedge_rect_callout(w: f64, h: f64, adj: &[(&str, f64)]) -> Vec<PathSeg> {
    let (hc, vc) = (w / 2.0, h / 2.0);
    let dx_pos = w * lookup(adj, "adj1", -20_833.0) / 100_000.0;
    let dy_pos = h * lookup(adj, "adj2", 62_500.0) / 100_000.0;
    let (x_pos, y_pos) = (hc + dx_pos, vc + dy_pos);
    let dq = if w > 0.0 { dx_pos * h / w } else { 0.0 };
    let dz = dy_pos.abs() - dq.abs();
    let x1 = w * if dx_pos > 0.0 { 7.0 } else { 2.0 } / 12.0;
    let x2 = w * if dx_pos > 0.0 { 10.0 } else { 5.0 } / 12.0;
    let y1 = h * if dy_pos > 0.0 { 7.0 } else { 2.0 } / 12.0;
    let y2 = h * if dy_pos > 0.0 { 10.0 } else { 5.0 } / 12.0;
    let t1 = if dx_pos > 0.0 { 0.0 } else { x_pos };
    let xl = if dz > 0.0 { 0.0 } else { t1 };
    let t2 = if dy_pos > 0.0 { x1 } else { x_pos };
    let xt = if dz > 0.0 { t2 } else { x1 };
    let t3 = if dx_pos > 0.0 { x_pos } else { w };
    let xr = if dz > 0.0 { w } else { t3 };
    let t4 = if dy_pos > 0.0 { x_pos } else { x1 };
    let xb = if dz > 0.0 { t4 } else { x1 };
    let t5 = if dx_pos > 0.0 { y1 } else { y_pos };
    let yl = if dz > 0.0 { y1 } else { t5 };
    let t6 = if dy_pos > 0.0 { 0.0 } else { y_pos };
    let yt = if dz > 0.0 { t6 } else { 0.0 };
    let t7 = if dx_pos > 0.0 { y_pos } else { y1 };
    let yr = if dz > 0.0 { y1 } else { t7 };
    let t8 = if dy_pos > 0.0 { y_pos } else { h };
    let yb = if dz > 0.0 { t8 } else { h };
    polygon(&[
        (0.0, 0.0),
        (x1, 0.0),
        (xt, yt),
        (x2, 0.0),
        (w, 0.0),
        (w, y1),
        (xr, yr),
        (w, y2),
        (w, h),
        (x2, h),
        (xb, yb),
        (x1, h),
        (0.0, h),
        (0.0, y2),
        (xl, yl),
        (0.0, y1),
    ])
}

/// `flowChartTerminator` — a stadium: fixed ECMA path-space insets
/// (`3475/21600` of the width) with half-ellipse caps.
fn flow_chart_terminator(w: f64, h: f64) -> Vec<PathSeg> {
    let rx = w * 3_475.0 / 21_600.0;
    let ry = h / 2.0;
    let (xa, xb) = (rx, w - rx);
    let mut segs = vec![
        PathSeg::MoveTo { x: xa, y: 0.0 },
        PathSeg::LineTo { x: xb, y: 0.0 },
    ];
    push_arc(&mut segs, xb, ry, rx, ry, 270.0, 180.0);
    segs.push(PathSeg::LineTo { x: xa, y: h });
    push_arc(&mut segs, xa, ry, rx, ry, 90.0, 180.0);
    segs.push(PathSeg::Close);
    segs
}
