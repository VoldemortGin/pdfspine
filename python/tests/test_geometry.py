"""Exhaustive tests for the pure-Python PyMuPDF geometry algebra (PRD §9.5).

These assert the value types match PyMuPDF 1.24.x's documented arithmetic
*exactly* — a Tier-A contract. They are written test-first (TDD) and cover the
full surface: construction overloads, sequence protocol, operator semantics,
transforms, inversion round-trips, rotation exactness at cardinal angles,
intersect/union/contains/morph/torect, and Quad convexity/rect.
"""

import math

import pytest

from pdfspine.geometry import (
    Identity,
    IRect,
    Matrix,
    Point,
    Quad,
    Rect,
)
import pdfspine.geometry as geom


EPS = 1e-9


def approx_seq(a, b, tol=1e-6):
    a = tuple(a)
    b = tuple(b)
    assert len(a) == len(b)
    for x, y in zip(a, b):
        assert math.isclose(float(x), float(y), abs_tol=tol), (a, b)


# --------------------------------------------------------------------------- #
# Matrix construction overloads
# --------------------------------------------------------------------------- #
class TestMatrixConstruction:
    def test_default_is_identity(self):
        assert tuple(Matrix()) == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)

    def test_single_scalar_is_rotation(self):
        # PyMuPDF: Matrix(degree) -> anti-clockwise rotation matrix.
        assert tuple(Matrix(90)) == (0.0, 1.0, -1.0, 0.0, 0.0, 0.0)
        assert tuple(Matrix(180)) == (-1.0, 0.0, 0.0, -1.0, 0.0, 0.0)
        assert tuple(Matrix(270)) == (0.0, -1.0, 1.0, 0.0, 0.0, 0.0)

    def test_two_scales(self):
        assert tuple(Matrix(2.0, 3.0)) == (2.0, 0.0, 0.0, 3.0, 0.0, 0.0)

    def test_six_components(self):
        assert tuple(Matrix(1, 2, 3, 4, 5, 6)) == (1.0, 2.0, 3.0, 4.0, 5.0, 6.0)

    def test_from_matrix(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        assert tuple(Matrix(m)) == tuple(m)

    def test_from_sequence(self):
        assert tuple(Matrix((1, 2, 3, 4, 5, 6))) == (1.0, 2.0, 3.0, 4.0, 5.0, 6.0)

    def test_single_scalar_general_angle(self):
        # A non-cardinal single scalar rotates by that many degrees.
        r = math.radians(45)
        approx_seq(Matrix(45), (math.cos(r), math.sin(r), -math.sin(r), math.cos(r), 0, 0))

    def test_bad_arg_count(self):
        with pytest.raises((ValueError, TypeError)):
            Matrix(1, 2, 3)


# --------------------------------------------------------------------------- #
# Matrix rotation exactness (prerotate gives the rotation algebra)
# --------------------------------------------------------------------------- #
class TestMatrixRotation:
    def test_prerotate_cardinals_exact(self):
        assert tuple(Matrix(1, 0, 0, 1, 0, 0).prerotate(0)) == (1, 0, 0, 1, 0, 0)
        assert tuple(Matrix(1, 0, 0, 1, 0, 0).prerotate(90)) == (0, 1, -1, 0, 0, 0)
        assert tuple(Matrix(1, 0, 0, 1, 0, 0).prerotate(180)) == (-1, 0, 0, -1, 0, 0)
        assert tuple(Matrix(1, 0, 0, 1, 0, 0).prerotate(270)) == (0, -1, 1, 0, 0, 0)

    def test_prerotate_negative_wraps(self):
        assert tuple(Matrix(1, 0, 0, 1, 0, 0).prerotate(-90)) == (0, -1, 1, 0, 0, 0)
        assert tuple(Matrix(1, 0, 0, 1, 0, 0).prerotate(360)) == (1, 0, 0, 1, 0, 0)

    def test_prerotate_general_angle(self):
        m = Matrix(1, 0, 0, 1, 0, 0).prerotate(45)
        r = math.radians(45)
        approx_seq(m, (math.cos(r), math.sin(r), -math.sin(r), math.cos(r), 0, 0))

    def test_prerotate_returns_self(self):
        m = Matrix()
        assert m.prerotate(90) is m

    def test_prerotate_preserves_translation(self):
        m = Matrix(1, 0, 0, 1, 5, 7).prerotate(90)
        assert (m.e, m.f) == (5, 7)


# --------------------------------------------------------------------------- #
# Matrix prescale / preshear / pretranslate
# --------------------------------------------------------------------------- #
class TestMatrixPreOps:
    def test_prescale(self):
        m = Matrix(1, 2, 3, 4, 5, 6).prescale(2, 3)
        assert tuple(m) == (2, 4, 9, 12, 5, 6)

    def test_prescale_returns_self(self):
        m = Matrix()
        assert m.prescale(2, 2) is m

    def test_preshear(self):
        m = Matrix(1, 0, 0, 1, 0, 0).preshear(2, 3)
        # a += v*c, b += v*d, c += h*a_old, d += h*b_old
        assert tuple(m) == (1, 3, 2, 1, 0, 0)

    def test_pretranslate(self):
        m = Matrix(2, 0, 0, 3, 0, 0).pretranslate(4, 5)
        # e += tx*a + ty*c ; f += tx*b + ty*d
        assert tuple(m) == (2, 0, 0, 3, 8, 15)

    def test_pretranslate_returns_self(self):
        m = Matrix()
        assert m.pretranslate(1, 1) is m


# --------------------------------------------------------------------------- #
# Matrix multiplication / concat / invert
# --------------------------------------------------------------------------- #
class TestMatrixAlgebra:
    def test_mul_identity(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        approx_seq(m * Matrix(), m)
        approx_seq(Matrix() * m, m)

    def test_mul_scalar(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        approx_seq(m * 2, (2, 4, 6, 8, 10, 12))

    def test_concat_order(self):
        # concat(one, two) == one-then-two (translate then scale)
        t = Matrix(1, 0, 0, 1, 10, 20)
        s = Matrix(2, 0, 0, 2, 0, 0)
        m = Matrix().concat(t, s)
        # point (0,0): translate->(10,20), scale->(20,40)
        approx_seq(Point(0, 0) * m, (20, 40))

    def test_concat_returns_self(self):
        m = Matrix()
        assert m.concat(Matrix(), Matrix()) is m

    def test_invert_roundtrip(self):
        m = Matrix(1, 0, 0, 1, 0, 0).prescale(2, 3).pretranslate(5, 7)
        inv = ~m
        approx_seq(m * inv, (1, 0, 0, 1, 0, 0))

    def test_invert_method_returns_zero_on_success(self):
        m = Matrix(2, 0, 0, 2, 0, 0)
        assert m.invert() == 0
        approx_seq(m, (0.5, 0, 0, 0.5, 0, 0))

    def test_invert_method_returns_one_on_degenerate(self):
        m = Matrix(0, 0, 0, 0, 0, 0)
        # degenerate: returns 1 and leaves the matrix unchanged
        assert m.invert() == 1
        assert tuple(m) == (0, 0, 0, 0, 0, 0)

    def test_invert_with_src(self):
        src = Matrix(2, 0, 0, 4, 0, 0)
        dst = Matrix()
        assert dst.invert(src) == 0
        approx_seq(dst, (0.5, 0, 0, 0.25, 0, 0))

    def test_dunder_invert_degenerate_leaves_identity(self):
        # PyMuPDF: ~M builds a fresh Matrix() (identity) and calls .invert();
        # on a degenerate source invert() is a no-op, so the result is identity.
        assert tuple(~Matrix(0, 0, 0, 0, 0, 0)) == (1, 0, 0, 1, 0, 0)

    def test_add_elementwise(self):
        approx_seq(Matrix(1, 2, 3, 4, 5, 6) + Matrix(1, 1, 1, 1, 1, 1),
                   (2, 3, 4, 5, 6, 7))

    def test_add_scalar(self):
        approx_seq(Matrix(1, 2, 3, 4, 5, 6) + 1, (2, 3, 4, 5, 6, 7))

    def test_sub_elementwise(self):
        approx_seq(Matrix(2, 3, 4, 5, 6, 7) - Matrix(1, 1, 1, 1, 1, 1),
                   (1, 2, 3, 4, 5, 6))

    def test_neg(self):
        approx_seq(-Matrix(1, -2, 3, -4, 5, -6), (-1, 2, -3, 4, -5, 6))

    def test_abs_and_norm(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        expect = math.sqrt(1 + 4 + 9 + 16 + 25 + 36)
        assert math.isclose(abs(m), expect)
        assert math.isclose(m.norm(), expect)


class TestMatrixSequenceAndProps:
    def test_len_and_index(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        assert len(m) == 6
        assert [m[i] for i in range(6)] == [1, 2, 3, 4, 5, 6]

    def test_properties(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        assert (m.a, m.b, m.c, m.d, m.e, m.f) == (1, 2, 3, 4, 5, 6)

    def test_equality_to_tuple(self):
        assert Matrix(1, 2, 3, 4, 5, 6) == (1, 2, 3, 4, 5, 6)

    def test_is_rectilinear(self):
        assert Matrix(1, 0, 0, 1, 0, 0).is_rectilinear is True
        assert Matrix(0, 1, 1, 0, 0, 0).is_rectilinear is True  # |a|,|d| ~ 0
        assert Matrix(1, 2, 3, 4, 0, 0).is_rectilinear is False

    def test_unpacking(self):
        a, b, c, d, e, f = Matrix(1, 2, 3, 4, 5, 6)
        assert (a, b, c, d, e, f) == (1, 2, 3, 4, 5, 6)


# --------------------------------------------------------------------------- #
# Point
# --------------------------------------------------------------------------- #
class TestPoint:
    def test_construct(self):
        assert tuple(Point(1, 2)) == (1.0, 2.0)
        assert tuple(Point()) == (0.0, 0.0)
        assert tuple(Point((3, 4))) == (3.0, 4.0)
        assert tuple(Point(Point(5, 6))) == (5.0, 6.0)

    def test_sequence(self):
        p = Point(1, 2)
        assert len(p) == 2
        assert p[0] == 1 and p[1] == 2
        x, y = p
        assert (x, y) == (1, 2)

    def test_equality(self):
        assert Point(1, 2) == (1, 2)
        assert Point(1, 2) == Point(1, 2)

    def test_add_sub(self):
        assert tuple(Point(1, 2) + Point(3, 4)) == (4, 6)
        assert tuple(Point(5, 7) - Point(1, 2)) == (4, 5)
        assert tuple(Point(1, 2) + 1) == (2, 3)

    def test_mul_scalar(self):
        assert tuple(Point(2, 3) * 2) == (4, 6)

    def test_mul_matrix(self):
        m = Matrix(1, 0, 0, 1, 10, 20)
        assert tuple(Point(1, 2) * m) == (11, 22)

    def test_mul_seq_is_dot_product(self):
        # PyMuPDF: Point * length-2 sequence is the dot product (a float).
        assert Point(1, 2) * (3, 4) == 1 * 3 + 2 * 4

    def test_truediv_scalar(self):
        approx_seq(Point(4, 6) / 2, (2, 3))

    def test_truediv_matrix(self):
        m = Matrix(2, 0, 0, 2, 0, 0)
        approx_seq(Point(4, 6) / m, (2, 3))

    def test_abs_is_hypot(self):
        assert math.isclose(abs(Point(3, 4)), 5.0)

    def test_distance_to_point(self):
        assert math.isclose(Point(0, 0).distance_to(Point(3, 4)), 5.0)

    def test_distance_to_point_units(self):
        # 72 px == 1 inch
        assert math.isclose(Point(0, 0).distance_to(Point(72, 0), "in"), 1.0)
        assert math.isclose(Point(0, 0).distance_to(Point(72, 0), "cm"), 2.54)
        assert math.isclose(Point(0, 0).distance_to(Point(72, 0), "mm"), 25.4)

    def test_distance_to_rect_inside_is_zero(self):
        assert Point(5, 5).distance_to(Rect(0, 0, 10, 10)) == 0.0

    def test_distance_to_rect_edge(self):
        # point to the right of the rect, within its y-band
        assert math.isclose(Point(15, 5).distance_to(Rect(0, 0, 10, 10)), 5.0)

    def test_distance_to_rect_corner(self):
        # point past the bottom-right corner
        assert math.isclose(
            Point(13, 14).distance_to(Rect(0, 0, 10, 10)), 5.0
        )

    def test_no_pt_unit(self):
        with pytest.raises(KeyError):
            Point(0, 0).distance_to(Point(1, 0), "pt")

    def test_unit(self):
        approx_seq(Point(3, 4).unit, (0.6, 0.8))

    def test_unit_zero(self):
        assert tuple(Point(0, 0).unit) == (0, 0)

    def test_abs_unit(self):
        approx_seq(Point(-3, 4).abs_unit, (0.6, 0.8))

    def test_norm(self):
        assert math.isclose(Point(3, 4).norm(), 5.0)

    def test_transform_in_place(self):
        p = Point(1, 2)
        m = Matrix(1, 0, 0, 1, 10, 20)
        assert p.transform(m) is p
        assert tuple(p) == (11, 22)


# --------------------------------------------------------------------------- #
# Rect
# --------------------------------------------------------------------------- #
class TestRect:
    def test_construct(self):
        assert tuple(Rect()) == (0, 0, 0, 0)
        assert tuple(Rect(1, 2, 3, 4)) == (1, 2, 3, 4)
        assert tuple(Rect((1, 2, 3, 4))) == (1, 2, 3, 4)
        assert tuple(Rect(Rect(1, 2, 3, 4))) == (1, 2, 3, 4)

    def test_construct_from_points(self):
        # PyMuPDF: Rect(p1, p2) from two point-likes
        assert tuple(Rect(Point(1, 2), Point(3, 4))) == (1, 2, 3, 4)

    def test_sequence(self):
        r = Rect(1, 2, 3, 4)
        assert len(r) == 4
        assert [r[i] for i in range(4)] == [1, 2, 3, 4]
        x0, y0, x1, y1 = r
        assert (x0, y0, x1, y1) == (1, 2, 3, 4)

    def test_width_height(self):
        assert Rect(1, 2, 4, 6).width == 3
        assert Rect(1, 2, 4, 6).height == 4

    def test_width_clamped_to_zero(self):
        assert Rect(4, 2, 1, 6).width == 0  # max(0, x1-x0)

    def test_corners(self):
        r = Rect(1, 2, 3, 4)
        assert tuple(r.tl) == (1, 2)
        assert tuple(r.tr) == (3, 2)
        assert tuple(r.bl) == (1, 4)
        assert tuple(r.br) == (3, 4)
        assert tuple(r.top_left) == (1, 2)
        assert tuple(r.top_right) == (3, 2)
        assert tuple(r.bottom_left) == (1, 4)
        assert tuple(r.bottom_right) == (3, 4)

    def test_is_empty(self):
        assert Rect(0, 0, 0, 10).is_empty is True   # zero width
        assert Rect(0, 0, 10, 0).is_empty is True   # zero height
        assert Rect(0, 0, 10, 10).is_empty is False

    def test_is_valid(self):
        assert Rect(0, 0, 10, 10).is_valid is True
        assert Rect(10, 0, 0, 10).is_valid is False

    def test_is_infinite(self):
        assert Rect(geom.FZ_MIN_INF_RECT, geom.FZ_MIN_INF_RECT,
                    geom.FZ_MAX_INF_RECT, geom.FZ_MAX_INF_RECT).is_infinite is True
        assert Rect(0, 0, 10, 10).is_infinite is False

    def test_normalize(self):
        r = Rect(10, 20, 0, 5).normalize()
        assert tuple(r) == (0, 5, 10, 20)

    def test_normalize_returns_self(self):
        r = Rect(1, 2, 3, 4)
        assert r.normalize() is r

    def test_round(self):
        # floor(x+0.001)/ceil(x-0.001)
        r = Rect(0.5, 0.5, 2.5, 2.5).round()
        assert isinstance(r, IRect)
        assert tuple(r) == (0, 0, 3, 3)

    def test_round_integral_stable(self):
        assert tuple(Rect(1, 2, 3, 4).round()) == (1, 2, 3, 4)

    def test_irect_property(self):
        assert tuple(Rect(0.5, 0.5, 2.5, 2.5).irect) == (0, 0, 3, 3)

    def test_abs_is_area(self):
        assert abs(Rect(0, 0, 3, 4)) == 12.0

    def test_abs_empty_is_zero(self):
        assert abs(Rect(0, 0, 0, 10)) == 0.0

    def test_norm(self):
        r = Rect(1, 2, 3, 4)
        assert math.isclose(r.norm(), math.sqrt(1 + 4 + 9 + 16))

    def test_get_area(self):
        assert math.isclose(Rect(0, 0, 3, 4).get_area(), 12.0)

    def test_quad(self):
        q = Rect(1, 2, 3, 4).quad
        assert tuple(q.ul) == (1, 2)
        assert tuple(q.ur) == (3, 2)
        assert tuple(q.ll) == (1, 4)
        assert tuple(q.lr) == (3, 4)


class TestRectOps:
    def test_include_point(self):
        r = Rect(0, 0, 10, 10).include_point(Point(15, 20))
        assert tuple(r) == (0, 0, 15, 20)

    def test_include_point_returns_self(self):
        r = Rect(0, 0, 10, 10)
        assert r.include_point(Point(1, 1)) is r

    def test_include_rect(self):
        r = Rect(0, 0, 5, 5).include_rect(Rect(3, 3, 10, 12))
        assert tuple(r) == (0, 0, 10, 12)

    def test_intersect(self):
        r = Rect(0, 0, 10, 10).intersect(Rect(5, 5, 20, 20))
        assert tuple(r) == (5, 5, 10, 10)

    def test_intersect_returns_self(self):
        r = Rect(0, 0, 10, 10)
        assert r.intersect(Rect(1, 1, 2, 2)) is r

    def test_and_operator(self):
        r = Rect(0, 0, 10, 10) & Rect(5, 5, 20, 20)
        assert tuple(r) == (5, 5, 10, 10)

    def test_or_with_point(self):
        r = Rect(0, 0, 10, 10) | Point(15, 20)
        assert tuple(r) == (0, 0, 15, 20)

    def test_or_with_rect(self):
        r = Rect(0, 0, 5, 5) | Rect(3, 3, 10, 12)
        assert tuple(r) == (0, 0, 10, 12)

    def test_or_does_not_mutate(self):
        a = Rect(0, 0, 5, 5)
        _ = a | Rect(3, 3, 10, 12)
        assert tuple(a) == (0, 0, 5, 5)

    def test_add_sub_scalar(self):
        approx_seq(Rect(1, 2, 3, 4) + 1, (2, 3, 4, 5))
        approx_seq(Rect(2, 3, 4, 5) - 1, (1, 2, 3, 4))

    def test_add_elementwise(self):
        approx_seq(Rect(1, 2, 3, 4) + (10, 20, 30, 40), (11, 22, 33, 44))

    def test_mul_scalar(self):
        approx_seq(Rect(1, 2, 3, 4) * 2, (2, 4, 6, 8))

    def test_mul_matrix(self):
        m = Matrix(1, 0, 0, 1, 10, 20)
        approx_seq(Rect(0, 0, 10, 10) * m, (10, 20, 20, 30))

    def test_truediv_matrix(self):
        m = Matrix(2, 0, 0, 2, 0, 0)
        approx_seq(Rect(2, 4, 6, 8) / m, (1, 2, 3, 4))

    def test_contains_point(self):
        r = Rect(0, 0, 10, 10)
        assert r.contains(Point(5, 5)) is True
        # half-open: the far edge is NOT contained
        assert r.contains(Point(10, 10)) is False
        assert r.contains(Point(0, 0)) is True

    def test_contains_rect(self):
        r = Rect(0, 0, 10, 10)
        assert r.contains(Rect(1, 1, 5, 5)) is True
        assert r.contains(Rect(1, 1, 20, 5)) is False

    def test_contains_number(self):
        assert Rect(1, 2, 3, 4).contains(3) is True
        assert Rect(1, 2, 3, 4).contains(99) is False

    def test_contains_dunder(self):
        assert Point(5, 5) in Rect(0, 0, 10, 10)

    def test_intersects(self):
        assert Rect(0, 0, 10, 10).intersects(Rect(5, 5, 20, 20)) is True
        assert Rect(0, 0, 10, 10).intersects(Rect(20, 20, 30, 30)) is False

    def test_intersects_empty_false(self):
        assert Rect(0, 0, 0, 0).intersects(Rect(0, 0, 10, 10)) is False

    def test_transform(self):
        r = Rect(0, 0, 10, 10)
        m = Matrix(2, 0, 0, 2, 0, 0)
        assert r.transform(m) is r
        approx_seq(r, (0, 0, 20, 20))

    def test_transform_rotation_bbox(self):
        # rotating a rect produces the bounding box of the rotated corners
        r = Rect(0, 0, 10, 20)
        m = Matrix(0, 1, -1, 0, 0, 0)  # 90 deg
        r.transform(m)
        approx_seq(r, (-20, 0, 0, 10))

    def test_morph(self):
        # morph by identity about a fixed point leaves the geometry put
        q = Rect(0, 0, 10, 10).morph(Point(5, 5), Matrix(1, 0, 0, 1, 0, 0))
        approx_seq(q.rect, (0, 0, 10, 10))

    def test_torect(self):
        src = Rect(0, 0, 10, 10)
        dst = Rect(100, 200, 120, 240)
        m = src.torect(dst)
        approx_seq(src.tl * m, dst.tl)
        approx_seq(src.br * m, dst.br)

    def test_torect_requires_finite_nonempty(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 0, 0).torect(Rect(0, 0, 10, 10))


# --------------------------------------------------------------------------- #
# IRect
# --------------------------------------------------------------------------- #
class TestIRect:
    def test_construct(self):
        assert tuple(IRect(1, 2, 3, 4)) == (1, 2, 3, 4)
        assert tuple(IRect((1, 2, 3, 4))) == (1, 2, 3, 4)

    def test_coerces_to_int(self):
        r = IRect(0.4, 0.6, 2.4, 2.6)
        assert all(isinstance(v, int) for v in r)
        # floor on x0/y0, ceil on x1/y1
        assert tuple(r) == (0, 0, 3, 3)

    def test_rect_property(self):
        r = IRect(1, 2, 3, 4).rect
        assert isinstance(r, Rect)
        assert tuple(r) == (1, 2, 3, 4)

    def test_width_height(self):
        assert IRect(1, 2, 4, 6).width == 3
        assert IRect(1, 2, 4, 6).height == 4

    def test_get_area(self):
        assert IRect(0, 0, 3, 4).get_area() == 12

    def test_is_empty(self):
        assert IRect(0, 0, 0, 10).is_empty is True
        assert IRect(0, 0, 10, 10).is_empty is False

    def test_transform_returns_irect(self):
        r = IRect(0, 0, 10, 10).transform(Matrix(2, 0, 0, 2, 0, 0))
        assert isinstance(r, IRect)
        assert tuple(r) == (0, 0, 20, 20)

    def test_intersect_returns_irect(self):
        r = IRect(0, 0, 10, 10).intersect(IRect(5, 5, 20, 20))
        assert isinstance(r, IRect)
        assert tuple(r) == (5, 5, 10, 10)

    def test_include_point(self):
        r = IRect(0, 0, 10, 10).include_point(Point(15, 20))
        assert isinstance(r, IRect)
        assert tuple(r) == (0, 0, 15, 20)

    def test_include_rect(self):
        r = IRect(0, 0, 5, 5).include_rect(IRect(3, 3, 10, 12))
        assert tuple(r) == (0, 0, 10, 12)

    def test_morph(self):
        q = IRect(0, 0, 10, 10).morph(Point(5, 5), Matrix(1, 0, 0, 1, 0, 0))
        approx_seq(q.rect, (0, 0, 10, 10))

    def test_torect(self):
        m = IRect(0, 0, 10, 10).torect(IRect(0, 0, 20, 20))
        approx_seq(Point(10, 10) * m, (20, 20))

    def test_normalize(self):
        r = IRect(10, 20, 0, 5).normalize()
        assert tuple(r) == (0, 5, 10, 20)


# --------------------------------------------------------------------------- #
# Quad
# --------------------------------------------------------------------------- #
class TestQuad:
    def test_construct_from_points(self):
        q = Quad(Point(0, 0), Point(10, 0), Point(0, 10), Point(10, 10))
        assert tuple(q.ul) == (0, 0)
        assert tuple(q.ur) == (10, 0)
        assert tuple(q.ll) == (0, 10)
        assert tuple(q.lr) == (10, 10)

    def test_construct_from_quad(self):
        q0 = Quad(Point(0, 0), Point(10, 0), Point(0, 10), Point(10, 10))
        q = Quad(q0)
        assert tuple(q.ul) == (0, 0)
        assert tuple(q.lr) == (10, 10)

    def test_sequence(self):
        q = Quad(Point(0, 0), Point(10, 0), Point(0, 10), Point(10, 10))
        assert len(q) == 4
        assert tuple(q[0]) == (0, 0)
        assert tuple(q[3]) == (10, 10)

    def test_equality(self):
        a = Quad(Point(0, 0), Point(1, 0), Point(0, 1), Point(1, 1))
        b = Quad(Point(0, 0), Point(1, 0), Point(0, 1), Point(1, 1))
        assert a == b

    def test_rect(self):
        q = Quad(Point(1, 1), Point(9, 0), Point(0, 8), Point(10, 10))
        approx_seq(q.rect, (0, 0, 10, 10))

    def test_width_height(self):
        q = Rect(0, 0, 10, 20).quad
        assert math.isclose(q.width, 10)
        assert math.isclose(q.height, 20)

    def test_is_rectangular(self):
        assert Rect(0, 0, 10, 20).quad.is_rectangular is True
        skew = Quad(Point(0, 0), Point(10, 1), Point(0, 10), Point(10, 10))
        assert skew.is_rectangular is False

    def test_is_convex_true(self):
        assert Rect(0, 0, 10, 10).quad.is_convex is True

    def test_is_convex_false(self):
        # a self-intersecting / concave arrangement
        bad = Quad(Point(0, 0), Point(10, 0), Point(10, 10), Point(0, 10))
        # ll and lr swapped vs a rect => not convex (bowtie)
        assert bad.is_convex is False

    def test_is_empty(self):
        degenerate = Quad(Point(0, 0), Point(0, 0), Point(0, 0), Point(0, 0))
        assert degenerate.is_empty is True
        assert Rect(0, 0, 10, 10).quad.is_empty is False

    def test_transform_in_place(self):
        q = Rect(0, 0, 10, 10).quad
        m = Matrix(1, 0, 0, 1, 5, 5)
        assert q.transform(m) is q
        assert tuple(q.ul) == (5, 5)
        assert tuple(q.lr) == (15, 15)

    def test_mul_matrix_returns_new(self):
        q0 = Rect(0, 0, 10, 10).quad
        q = q0 * Matrix(2, 0, 0, 2, 0, 0)
        assert tuple(q.lr) == (20, 20)
        assert tuple(q0.lr) == (10, 10)  # original untouched

    def test_morph(self):
        q = Rect(0, 0, 10, 10).quad.morph(Point(0, 0), Matrix(2, 0, 0, 2, 0, 0))
        approx_seq(q.lr, (20, 20))


# --------------------------------------------------------------------------- #
# Module-level helpers / singletons
# --------------------------------------------------------------------------- #
class TestModuleHelpers:
    def test_identity_singleton(self):
        assert tuple(Identity) == (1, 0, 0, 1, 0, 0)

    def test_identity_readonly(self):
        # attempting to mutate Identity must not change it
        try:
            Identity.a = 5.0
        except Exception:
            pass
        assert tuple(Identity) == (1, 0, 0, 1, 0, 0)

    def test_empty_infinite_rect_factories(self):
        assert tuple(geom.INFINITE_RECT()) == (
            geom.FZ_MIN_INF_RECT, geom.FZ_MIN_INF_RECT,
            geom.FZ_MAX_INF_RECT, geom.FZ_MAX_INF_RECT,
        )
        assert geom.INFINITE_RECT().is_infinite is True
        assert tuple(geom.EMPTY_RECT()) == (
            geom.FZ_MAX_INF_RECT, geom.FZ_MAX_INF_RECT,
            geom.FZ_MIN_INF_RECT, geom.FZ_MIN_INF_RECT,
        )

    def test_empty_infinite_irect_factories(self):
        assert tuple(geom.INFINITE_IRECT()) == (
            geom.FZ_MIN_INF_RECT, geom.FZ_MIN_INF_RECT,
            geom.FZ_MAX_INF_RECT, geom.FZ_MAX_INF_RECT,
        )

    def test_empty_infinite_quad_factories(self):
        assert geom.INFINITE_QUAD().is_infinite is True

    def test_paper_size(self):
        assert geom.paper_size("a4") == (595, 842)
        assert geom.paper_size("A4-L") == (842, 595)

    def test_paper_rect(self):
        approx_seq(geom.paper_rect("a4"), (0, 0, 595, 842))

    def test_constants_present(self):
        assert geom.FZ_MIN_INF_RECT == -2147483648
        assert geom.FZ_MAX_INF_RECT == 2147483520
