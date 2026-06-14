//! Geometry unit tests — the `GEOM-*` and `COORD-ROT-*` catalog.
//!
//! Spec source of truth: PyMuPDF (`fitz`) geometry algebra (Tier-A documented
//! contract, PRD §9.5) cross-checked against the documented examples in the
//! PyMuPDF Matrix/Rect/Point/Quad pages.

use pdf_core::geom::{
    paper_rect, paper_size, IRect, Matrix, Point, Quad, Rect, EMPTY_RECT, INFINITE_RECT,
};

const EPS: f64 = 1e-9;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS
}

fn matrix_approx(m: &Matrix, e: &Matrix) -> bool {
    approx(m.a, e.a)
        && approx(m.b, e.b)
        && approx(m.c, e.c)
        && approx(m.d, e.d)
        && approx(m.e, e.e)
        && approx(m.f, e.f)
}

fn point_approx(p: Point, e: Point) -> bool {
    approx(p.x, e.x) && approx(p.y, e.y)
}

// --- Matrix basics -------------------------------------------------------

#[test]
fn geom_matrix_identity() {
    // GEOM-MAT-001: identity constant + constructor agree.
    assert_eq!(Matrix::IDENTITY, Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    assert_eq!(Matrix::identity(), Matrix::IDENTITY);
    assert_eq!(Matrix::default(), Matrix::IDENTITY);
}

#[test]
fn geom_matrix_scale_translate() {
    // GEOM-MAT-002: scale and translate constructors.
    assert_eq!(
        Matrix::scale(2.0, 3.0),
        Matrix::new(2.0, 0.0, 0.0, 3.0, 0.0, 0.0)
    );
    assert_eq!(
        Matrix::translate(5.0, 7.0),
        Matrix::new(1.0, 0.0, 0.0, 1.0, 5.0, 7.0)
    );
}

#[test]
fn geom_matrix_determinant() {
    // GEOM-MAT-003: determinant a*d - b*c.
    let m = Matrix::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    assert!(approx(m.determinant(), 1.0 * 4.0 - 2.0 * 3.0)); // -2
    assert!(approx(Matrix::IDENTITY.determinant(), 1.0));
}

#[test]
fn geom_matrix_concat_documented_example() {
    // GEOM-MAT-004: PyMuPDF documents `Point(1,2) * Matrix(1,2,3,4,5,6) ==
    // Point(12, 16)`. Verify the point transform matches that contract.
    let m = Matrix::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let p = Point::new(1.0, 2.0).transform(&m);
    assert!(point_approx(p, Point::new(12.0, 16.0)), "got {p:?}");
}

#[test]
fn geom_matrix_concat_associative_with_point() {
    // GEOM-MAT-005: `p * (m1 * m2) == (p * m1) * m2` — concat applies m1 first.
    let m1 = Matrix::translate(3.0, 4.0);
    let m2 = Matrix::scale(2.0, 5.0);
    let p = Point::new(1.0, 1.0);
    let combined = Matrix::concat(&m1, &m2);
    let lhs = p.transform(&combined);
    let rhs = p.transform(&m1).transform(&m2);
    assert!(point_approx(lhs, rhs), "lhs {lhs:?} rhs {rhs:?}");
    // And the `*` operator equals `concat`.
    assert_eq!(m1 * m2, combined);
}

#[test]
fn geom_matrix_concat_identity_neutral() {
    // GEOM-MAT-006: identity is the neutral element on both sides.
    let m = Matrix::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    assert_eq!(Matrix::concat(&m, &Matrix::IDENTITY), m);
    assert_eq!(Matrix::concat(&Matrix::IDENTITY, &m), m);
}

#[test]
fn geom_matrix_invert_known() {
    // GEOM-MAT-007: invert of a known matrix; m * m^-1 == identity.
    let m = Matrix::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let inv = m.invert().expect("invertible");
    let prod = Matrix::concat(&m, &inv);
    assert!(matrix_approx(&prod, &Matrix::IDENTITY), "got {prod:?}");
    let prod2 = Matrix::concat(&inv, &m);
    assert!(matrix_approx(&prod2, &Matrix::IDENTITY), "got {prod2:?}");
}

#[test]
fn geom_matrix_invert_singular() {
    // GEOM-MAT-008: singular matrix has no inverse.
    let m = Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0); // det = 0
    assert!(!m.is_invertible());
    assert!(m.invert().is_none());
}

#[test]
fn geom_matrix_invert_inverse_matches_division_example() {
    // GEOM-MAT-009: PyMuPDF documents `Point(1,2) / Matrix(1,2,3,4,5,6) ==
    // Point(2, -2)`. Division is multiply-by-inverse.
    let m = Matrix::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let inv = m.invert().expect("invertible");
    let p = Point::new(1.0, 2.0).transform(&inv);
    assert!(point_approx(p, Point::new(2.0, -2.0)), "got {p:?}");
}

// --- Cardinal rotations (bit-exact) -------------------------------------

#[test]
fn coord_rot_0_exact() {
    // COORD-ROT-0: rotate(0) is exactly identity, no float drift.
    let m = Matrix::rotate(0.0);
    assert_eq!(m, Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
}

#[test]
fn coord_rot_90_exact() {
    // COORD-ROT-90: identity -> [cos, sin, -sin, cos] = [0, 1, -1, 0].
    let m = Matrix::rotate(90.0);
    assert_eq!(m, Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0));
    // Bit-exact zeros (no -0.0 / 6e-17 drift).
    assert_eq!(m.a, 0.0);
    assert_eq!(m.d, 0.0);
}

#[test]
fn coord_rot_180_exact() {
    // COORD-ROT-180: [-1, 0, 0, -1].
    let m = Matrix::rotate(180.0);
    assert_eq!(m, Matrix::new(-1.0, 0.0, 0.0, -1.0, 0.0, 0.0));
    assert_eq!(m.b, 0.0);
    assert_eq!(m.c, 0.0);
}

#[test]
fn coord_rot_270_exact() {
    // COORD-ROT-270: [0, -1, 1, 0].
    let m = Matrix::rotate(270.0);
    assert_eq!(m, Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0));
    assert_eq!(m.a, 0.0);
    assert_eq!(m.d, 0.0);
}

#[test]
fn coord_rot_negative_and_wraparound_exact() {
    // COORD-ROT-WRAP: -90 == 270, 360 == 0, 450 == 90, all bit-exact.
    assert_eq!(Matrix::rotate(-90.0), Matrix::rotate(270.0));
    assert_eq!(Matrix::rotate(360.0), Matrix::rotate(0.0));
    assert_eq!(Matrix::rotate(450.0), Matrix::rotate(90.0));
    assert_eq!(Matrix::rotate(-270.0), Matrix::rotate(90.0));
}

#[test]
fn coord_rot_90_transforms_point() {
    // COORD-ROT-90-PT: rotating (1, 0) by 90deg CCW -> (0, 1).
    let m = Matrix::rotate(90.0);
    let p = Point::new(1.0, 0.0).transform(&m);
    assert_eq!(p, Point::new(0.0, 1.0));
}

#[test]
fn coord_rot_four_quarter_turns_identity() {
    // COORD-ROT-CYCLE: four 90deg turns compose back to identity, bit-exact
    // thanks to the cardinal special-case.
    let q = Matrix::rotate(90.0);
    let full = Matrix::concat(&Matrix::concat(&q, &q), &Matrix::concat(&q, &q));
    assert_eq!(full, Matrix::IDENTITY);
}

#[test]
fn coord_rot_45_general_path() {
    // COORD-ROT-45: non-cardinal angle uses the trig path.
    let m = Matrix::rotate(45.0);
    let s = std::f64::consts::FRAC_1_SQRT_2;
    assert!(
        matrix_approx(&m, &Matrix::new(s, s, -s, s, 0.0, 0.0)),
        "got {m:?}"
    );
}

// --- Point ---------------------------------------------------------------

#[test]
fn geom_point_transform_identity() {
    // GEOM-PT-001: identity transform is a no-op.
    let p = Point::new(3.5, -2.0);
    assert_eq!(p.transform(&Matrix::IDENTITY), p);
}

#[test]
fn geom_point_arithmetic_and_norm() {
    // GEOM-PT-002: add/sub/neg/scale/norm.
    let a = Point::new(3.0, 4.0);
    let b = Point::new(1.0, 2.0);
    assert_eq!(a + b, Point::new(4.0, 6.0));
    assert_eq!(a - b, Point::new(2.0, 2.0));
    assert_eq!(-a, Point::new(-3.0, -4.0));
    assert_eq!(a * 2.0, Point::new(6.0, 8.0));
    assert!(approx(a.norm(), 5.0));
}

#[test]
fn geom_point_mul_matrix_operator() {
    // GEOM-PT-003: `point * matrix` operator == transform.
    let p = Point::new(1.0, 2.0);
    let m = Matrix::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    assert_eq!(p * m, p.transform(&m));
}

// --- Rect ----------------------------------------------------------------

#[test]
fn geom_rect_normalize_inverted() {
    // GEOM-RECT-001: normalize swaps inverted edges.
    let r = Rect::new(10.0, 20.0, 1.0, 2.0);
    let n = r.normalize();
    assert_eq!(n, Rect::new(1.0, 2.0, 10.0, 20.0));
    // Already-normalized is unchanged.
    assert_eq!(n.normalize(), n);
}

#[test]
fn geom_rect_width_height_area() {
    // GEOM-RECT-002: width/height/area.
    let r = Rect::new(1.0, 2.0, 5.0, 10.0);
    assert!(approx(r.width(), 4.0));
    assert!(approx(r.height(), 8.0));
    assert!(approx(r.area(), 32.0));
}

#[test]
fn geom_rect_union_known() {
    // GEOM-RECT-003: union (|) is the smallest enclosing rect.
    let a = Rect::new(0.0, 0.0, 2.0, 2.0);
    let b = Rect::new(1.0, 1.0, 4.0, 3.0);
    assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 4.0, 3.0));
    assert_eq!(a | b, Rect::new(0.0, 0.0, 4.0, 3.0));
}

#[test]
fn geom_rect_intersect_known() {
    // GEOM-RECT-004: intersect (&) is the largest enclosed rect.
    let a = Rect::new(0.0, 0.0, 2.0, 2.0);
    let b = Rect::new(1.0, 1.0, 4.0, 3.0);
    assert_eq!(a.intersect(&b), Rect::new(1.0, 1.0, 2.0, 2.0));
    assert_eq!(a & b, Rect::new(1.0, 1.0, 2.0, 2.0));
}

#[test]
fn geom_rect_intersect_disjoint_is_empty() {
    // GEOM-RECT-005: disjoint rects intersect to an empty rect.
    let a = Rect::new(0.0, 0.0, 1.0, 1.0);
    let b = Rect::new(5.0, 5.0, 6.0, 6.0);
    assert!(a.intersect(&b).is_empty());
    assert!(!a.intersects(&b));
}

#[test]
fn geom_rect_union_with_empty() {
    // GEOM-RECT-006: union with an empty rect returns the other rect.
    let a = Rect::new(2.0, 2.0, 5.0, 5.0);
    assert_eq!(a.union(&EMPTY_RECT), a);
    assert_eq!(EMPTY_RECT.union(&a), a);
}

#[test]
fn geom_rect_contains_point_and_rect() {
    // GEOM-RECT-007: contains point (inclusive) and sub-rect.
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert!(r.contains_point(Point::new(5.0, 5.0)));
    assert!(r.contains_point(Point::new(0.0, 0.0))); // edge inclusive
    assert!(!r.contains_point(Point::new(11.0, 5.0)));
    assert!(r.contains_rect(&Rect::new(1.0, 1.0, 9.0, 9.0)));
    assert!(!r.contains_rect(&Rect::new(1.0, 1.0, 11.0, 9.0)));
    // Empty rect is contained in anything.
    assert!(r.contains_rect(&EMPTY_RECT));
}

#[test]
fn geom_rect_empty_and_infinite_predicates() {
    // GEOM-RECT-008: empty / infinite predicates.
    assert!(EMPTY_RECT.is_empty());
    assert!(!EMPTY_RECT.is_infinite());
    assert!(INFINITE_RECT.is_infinite());
    assert!(!INFINITE_RECT.is_empty());
    assert!(Rect::new(0.0, 0.0, 1.0, 1.0).is_valid());
    assert!(Rect::new(5.0, 5.0, 1.0, 1.0).is_empty());
}

#[test]
fn geom_rect_round_to_irect() {
    // GEOM-RECT-009: round() floors x0/y0 and ceils x1/y1.
    let r = Rect::new(1.2, 2.8, 5.1, 9.9);
    assert_eq!(r.round(), IRect::new(1, 2, 6, 10));
    let r2 = Rect::new(-1.2, -2.8, 5.0, 9.0);
    assert_eq!(r2.round(), IRect::new(-2, -3, 5, 9));
}

#[test]
fn geom_rect_transform_translate_and_rotate() {
    // GEOM-RECT-010: transform by translate is exact; by rotate(90) gives the
    // axis-aligned envelope.
    let r = Rect::new(0.0, 0.0, 2.0, 4.0);
    let t = r.transform(&Matrix::translate(10.0, 20.0));
    assert_eq!(t, Rect::new(10.0, 20.0, 12.0, 24.0));

    let rot = r.transform(&Matrix::rotate(90.0));
    // (0,0)->(0,0), (2,0)->(0,2), (0,4)->(-4,0), (2,4)->(-4,2)
    assert_eq!(rot, Rect::new(-4.0, 0.0, 0.0, 2.0));
}

// --- IRect ---------------------------------------------------------------

#[test]
fn geom_irect_basics() {
    // GEOM-IRECT-001: width/height/area/normalize/union/intersect.
    let r = IRect::new(1, 2, 5, 10);
    assert_eq!(r.width(), 4);
    assert_eq!(r.height(), 8);
    assert_eq!(r.area(), 32);
    assert_eq!(IRect::new(5, 10, 1, 2).normalize(), IRect::new(1, 2, 5, 10));

    let a = IRect::new(0, 0, 2, 2);
    let b = IRect::new(1, 1, 4, 3);
    assert_eq!(a.union(&b), IRect::new(0, 0, 4, 3));
    assert_eq!(a.intersect(&b), IRect::new(1, 1, 2, 2));
    assert_eq!(a | b, IRect::new(0, 0, 4, 3));
    assert_eq!(a & b, IRect::new(1, 1, 2, 2));
}

#[test]
fn geom_irect_rect_roundtrip() {
    // GEOM-IRECT-002: IRect <-> Rect.
    let r = IRect::new(1, 2, 3, 4);
    assert_eq!(r.rect(), Rect::new(1.0, 2.0, 3.0, 4.0));
    assert_eq!(
        IRect::from(Rect::new(1.2, 2.8, 5.1, 9.9)),
        IRect::new(1, 2, 6, 10)
    );
}

// --- Quad ----------------------------------------------------------------

#[test]
fn geom_quad_from_rect_corner_order() {
    // GEOM-QUAD-001: corner order ul, ur, ll, lr.
    let r = Rect::new(1.0, 2.0, 5.0, 8.0);
    let q = r.quad();
    assert_eq!(q.ul, Point::new(1.0, 2.0));
    assert_eq!(q.ur, Point::new(5.0, 2.0));
    assert_eq!(q.ll, Point::new(1.0, 8.0));
    assert_eq!(q.lr, Point::new(5.0, 8.0));
}

#[test]
fn geom_quad_rect_bounding_box() {
    // GEOM-QUAD-002: bounding box of a (possibly rotated) quad.
    let q = Quad::new(
        Point::new(0.0, 1.0),
        Point::new(1.0, 0.0),
        Point::new(-1.0, 0.0),
        Point::new(0.0, -1.0),
    );
    assert_eq!(q.rect(), Rect::new(-1.0, -1.0, 1.0, 1.0));
}

#[test]
fn geom_quad_transform_roundtrip_to_rect() {
    // GEOM-QUAD-003: rect -> quad -> transform(identity) -> rect is stable.
    let r = Rect::new(1.0, 2.0, 5.0, 8.0);
    let back = r.quad().transform(&Matrix::IDENTITY).rect();
    assert_eq!(back, r);
}

#[test]
fn geom_quad_width_height() {
    // GEOM-QUAD-004: width/height from side lengths.
    let r = Rect::new(0.0, 0.0, 4.0, 3.0);
    let q = r.quad();
    assert!(approx(q.width(), 4.0));
    assert!(approx(q.height(), 3.0));
}

// --- Paper sizes ---------------------------------------------------------

#[test]
fn geom_paper_sizes_known() {
    // GEOM-PAPER-001: A4 / Letter / Legal exact dimensions.
    assert_eq!(paper_size("a4"), Some((595, 842)));
    assert_eq!(paper_size("A4"), Some((595, 842))); // case-insensitive
    assert_eq!(paper_size("letter"), Some((612, 792)));
    assert_eq!(paper_size("legal"), Some((612, 1008)));
    assert_eq!(paper_size("unknown-paper"), None);
}

#[test]
fn geom_paper_landscape_suffix() {
    // GEOM-PAPER-002: `-l` suffix swaps width/height.
    assert_eq!(paper_size("a4-l"), Some((842, 595)));
    assert_eq!(paper_size("letter-landscape"), Some((792, 612)));
}

#[test]
fn geom_paper_rect() {
    // GEOM-PAPER-003: paper_rect places the page at the origin.
    assert_eq!(paper_rect("a4"), Some(Rect::new(0.0, 0.0, 595.0, 842.0)));
    assert_eq!(paper_rect("nope"), None);
}
