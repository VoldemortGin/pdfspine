//! Geometry property tests — the `GEOM-PROP-*` / `COORD-ROT-PROP-*` catalog.
//!
//! Spec source of truth: algebraic invariants of the PyMuPDF geometry contract
//! (PRD §10.2 property-examples list). Generators are ours.

use pdf_core::geom::{IRect, Matrix, Point, Rect};
use proptest::prelude::*;

const EPS: f64 = 1e-6;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * (1.0 + a.abs().max(b.abs()))
}

fn point_approx(p: Point, q: Point) -> bool {
    approx(p.x, q.x) && approx(p.y, q.y)
}

// Bounded finite coordinates keep float error analysable.
fn coord() -> impl Strategy<Value = f64> {
    -1.0e4..1.0e4f64
}

fn point() -> impl Strategy<Value = Point> {
    (coord(), coord()).prop_map(|(x, y)| Point::new(x, y))
}

fn rect() -> impl Strategy<Value = Rect> {
    (coord(), coord(), coord(), coord()).prop_map(|(a, b, c, d)| Rect::new(a, b, c, d))
}

// A guaranteed-non-empty rectangle (strictly positive width and height). Used
// where PyMuPDF `include_rect` semantics make the empty-operand case
// intentionally order-dependent (union is not commutative for empty rects).
fn nonempty_rect() -> impl Strategy<Value = Rect> {
    (coord(), coord(), 1.0..1.0e4f64, 1.0..1.0e4f64)
        .prop_map(|(x0, y0, w, h)| Rect::new(x0, y0, x0 + w, y0 + h))
}

// Invertible matrices: keep determinant comfortably away from zero.
fn invertible_matrix() -> impl Strategy<Value = Matrix> {
    (
        1.0..10.0f64,
        -5.0..5.0f64,
        -5.0..5.0f64,
        1.0..10.0f64,
        coord(),
        coord(),
    )
        .prop_map(|(a, b, c, d, e, f)| Matrix::new(a, b, c, d, e, f))
        .prop_filter("non-singular", |m| m.determinant().abs() > 1.0)
}

proptest! {
    #[test]
    fn prop_point_transform_identity_is_noop(p in point()) {
        // GEOM-PROP-001: identity transform leaves the point unchanged.
        prop_assert!(point_approx(p.transform(&Matrix::IDENTITY), p));
    }

    #[test]
    fn prop_matrix_invert_roundtrip(m in invertible_matrix(), p in point()) {
        // GEOM-PROP-002: p transformed by m then m^-1 returns to p.
        let inv = m.invert().unwrap();
        let back = p.transform(&m).transform(&inv);
        prop_assert!(point_approx(back, p), "back {back:?} p {p:?}");
    }

    #[test]
    fn prop_matrix_concat_inverse_is_identity(m in invertible_matrix(), p in point()) {
        // GEOM-PROP-003: concat(m, m^-1) acts as identity on points.
        let inv = m.invert().unwrap();
        let composed = Matrix::concat(&m, &inv);
        prop_assert!(point_approx(p.transform(&composed), p));
    }

    #[test]
    fn prop_concat_matches_sequential_transform(
        m1 in invertible_matrix(),
        m2 in invertible_matrix(),
        p in point(),
    ) {
        // GEOM-PROP-004: p * (m1 * m2) == (p * m1) * m2.
        let combined = Matrix::concat(&m1, &m2);
        let lhs = p.transform(&combined);
        let rhs = p.transform(&m1).transform(&m2);
        prop_assert!(point_approx(lhs, rhs), "lhs {lhs:?} rhs {rhs:?}");
    }

    #[test]
    fn prop_normalize_idempotent(r in rect()) {
        // GEOM-PROP-005: normalize is idempotent.
        let once = r.normalize();
        let twice = once.normalize();
        prop_assert_eq!(once, twice);
        prop_assert!(once.is_valid());
    }

    #[test]
    fn prop_union_commutative(a in nonempty_rect(), b in nonempty_rect()) {
        // GEOM-PROP-006: union is commutative for non-empty rects. (PyMuPDF's
        // `include_rect` is intentionally order-dependent when *both* operands
        // are empty — see Rect::union — so commutativity is only asserted on
        // the non-empty domain.)
        prop_assert_eq!(a.union(&b), b.union(&a));
    }

    #[test]
    fn prop_intersect_commutative(a in rect(), b in rect()) {
        // GEOM-PROP-007: intersect is commutative.
        prop_assert_eq!(a.intersect(&b), b.intersect(&a));
    }

    #[test]
    fn prop_union_contains_both(a in nonempty_rect(), b in nonempty_rect()) {
        // GEOM-PROP-008: the union contains both (non-empty) operands.
        let u = a.union(&b);
        prop_assert!(u.contains_rect(&a));
        prop_assert!(u.contains_rect(&b));
    }

    #[test]
    fn prop_intersect_contained_in_both(a in rect(), b in rect()) {
        // GEOM-PROP-009: the intersection is contained in both operands.
        let i = a.intersect(&b);
        if !i.is_empty() {
            prop_assert!(a.contains_rect(&i));
            prop_assert!(b.contains_rect(&i));
        }
    }

    #[test]
    fn prop_area_equals_width_times_height(r in rect()) {
        // GEOM-PROP-010: area == width * height.
        prop_assert!(approx(r.area(), r.width() * r.height()));
    }

    #[test]
    fn prop_normalize_preserves_area(r in rect()) {
        // GEOM-PROP-011: normalize preserves area.
        prop_assert!(approx(r.area(), r.normalize().area()));
    }

    #[test]
    fn prop_rotation_cardinal_compose_to_identity(
        // GEOM-PROP-012 / COORD-ROT-PROP: any cardinal multiple composed with
        // its complement returns to identity, bit-exact.
        k in 0u32..4,
        p in point(),
    ) {
        let deg = f64::from(k) * 90.0;
        let m = Matrix::rotate(deg);
        let back = Matrix::rotate(360.0 - deg);
        let composed = Matrix::concat(&m, &back);
        prop_assert!(point_approx(p.transform(&composed), p));
    }

    #[test]
    fn prop_irect_round_contains_rect(r in rect()) {
        // GEOM-PROP-013: rounding outward yields an IRect whose Rect contains
        // the original (normalized) rect.
        let ir: IRect = r.round();
        let outer = ir.rect();
        let n = r.normalize();
        prop_assert!(outer.x0 <= n.x0 + EPS);
        prop_assert!(outer.y0 <= n.y0 + EPS);
        prop_assert!(outer.x1 + EPS >= n.x1);
        prop_assert!(outer.y1 + EPS >= n.y1);
    }

    #[test]
    fn prop_quad_rect_contains_corners(r in rect(), m in invertible_matrix()) {
        // GEOM-PROP-014: the bounding rect of a transformed quad contains all
        // four transformed corners.
        let q = r.quad().transform(&m);
        let bb = q.rect();
        for c in [q.ul, q.ur, q.ll, q.lr] {
            prop_assert!(bb.contains_point(c), "corner {c:?} outside {bb:?}");
        }
    }
}
