"""Branch/edge coverage for the pure-Python geometry algebra (`geometry.py`).

These complement ``test_geometry.py`` by driving the *leftover* operator,
constructor and predicate branches that the happy-path suite skips: the
``ValueError``/``TypeError``/``ZeroDivisionError`` guards on bad argument
counts and lengths, the ``__eq__`` fall-through against foreign types, the
``__repr__``/``__hash__``/``__bool__`` dunders, the full ``IRect`` surface, the
infinite/empty short-circuits of the set operations, and the four ``Point``
distance-to-rectangle regions plus the rotated ``Quad`` convexity/rectangularity
sub-checks. PyMuPDF 1.24 semantics are the spec.
"""

import math

import pytest

from hypothesis import given, settings
from hypothesis import strategies as st

from pdfspine.geometry import (
    IRect,
    Matrix,
    Point,
    Quad,
    Rect,
)
import pdfspine.geometry as geom


def approx_seq(a, b, tol=1e-6):
    a = tuple(a)
    b = tuple(b)
    assert len(a) == len(b)
    for x, y in zip(a, b):
        assert math.isclose(float(x), float(y), abs_tol=tol), (a, b)


# --------------------------------------------------------------------------- #
# Point — dunders, arithmetic guards, distance-to regions
# --------------------------------------------------------------------------- #
class TestPointBranches:
    def test_repr_hash_neg(self):
        assert repr(Point(1.0, 2.0)) == "Point(1.0, 2.0)"
        assert hash(Point(1, 2)) == hash((1.0, 2.0))
        assert tuple(-Point(1, -2)) == (-1, 2)

    def test_eq_foreign_type_is_false(self):
        # len(other) raises TypeError -> __eq__ returns NotImplemented -> False.
        assert (Point(1, 2) == object()) is False
        assert Point(1, 2) != object()

    def test_init_bad_arg_count(self):
        with pytest.raises(ValueError):
            Point(1, 2, 3)

    def test_add_bad_seq_len(self):
        with pytest.raises(ValueError):
            Point(1, 2) + (1, 2, 3)

    def test_sub_scalar_and_bad_seq_len(self):
        assert tuple(Point(5, 7) - 1) == (4, 6)
        with pytest.raises(ValueError):
            Point(1, 2) - (1, 2, 3)

    def test_truediv_singular_matrix_raises(self):
        with pytest.raises(ZeroDivisionError):
            Point(4, 6) / Matrix(0, 0, 0, 0, 0, 0)

    def test_transform_bad_matrix_len(self):
        with pytest.raises(ValueError):
            Point(1, 2).transform((1, 2, 3))

    def test_abs_unit_zero_vector(self):
        assert tuple(Point(0, 0).abs_unit) == (0, 0)

    def test_distance_to_requires_arg(self):
        with pytest.raises(ValueError):
            Point(0, 0).distance_to()

    def test_distance_to_bad_arg_shape(self):
        with pytest.raises(ValueError):
            Point(0, 0).distance_to((1, 2, 3))

    def test_distance_to_rect_all_regions(self):
        r = Rect(0, 0, 10, 10)
        d = math.hypot(5, 5)
        # right + above -> top-right corner
        assert math.isclose(Point(15, -5).distance_to(r), d)
        # within x-band, below / above -> vertical gap
        assert math.isclose(Point(5, 15).distance_to(r), 5.0)
        assert math.isclose(Point(5, -5).distance_to(r), 5.0)
        # left + below / above -> the two left corners
        assert math.isclose(Point(-5, 15).distance_to(r), d)
        assert math.isclose(Point(-5, -5).distance_to(r), d)
        # left, within y-band -> horizontal gap
        assert math.isclose(Point(-5, 5).distance_to(r), 5.0)


# --------------------------------------------------------------------------- #
# Rect — contains variants, set-op predicate branches, operator guards
# --------------------------------------------------------------------------- #
class TestRectBranches:
    def test_repr(self):
        assert repr(Rect(1.0, 2.0, 3.0, 4.0)) == "Rect(1.0, 2.0, 3.0, 4.0)"

    def test_eq_tuple_and_foreign(self):
        assert Rect(1, 2, 3, 4) == (1, 2, 3, 4)
        assert (Rect(1, 2, 3, 4) == object()) is False

    def test_contains_non_sequence_is_false(self):
        assert (None in Rect(0, 0, 10, 10)) is False

    def test_contains_quad(self):
        big = Rect(0, 0, 100, 100)
        assert (Rect(10, 10, 20, 20).quad in big) is True
        assert (Rect(10, 10, 200, 20).quad in big) is False

    def test_contains_wrong_length_is_false(self):
        assert ((1, 2, 3) in Rect(0, 0, 10, 10)) is False

    def test_intersect_bad_seq_len(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 10, 10).intersect((1, 2, 3))

    def test_intersect_with_infinite_returns_self_unchanged(self):
        r = Rect(1, 2, 3, 4)
        assert r.intersect(geom.INFINITE_RECT()) is r
        assert tuple(r) == (1, 2, 3, 4)

    def test_intersect_when_self_infinite_takes_other(self):
        r = geom.INFINITE_RECT()
        r.intersect(Rect(1, 2, 3, 4))
        assert tuple(r) == (1, 2, 3, 4)

    def test_intersect_with_empty_argument(self):
        r = Rect(0, 0, 10, 10)
        empty = Rect(5, 5, 5, 5)  # zero width/height -> is_empty
        r.intersect(empty)
        assert tuple(r) == (5, 5, 5, 5)

    def test_intersect_when_self_empty_returns_self(self):
        r = Rect(5, 5, 5, 5)
        assert r.intersect(Rect(0, 0, 10, 10)) is r
        assert tuple(r) == (5, 5, 5, 5)

    def test_include_rect_bad_seq_len(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 10, 10).include_rect((1, 2, 3))

    def test_include_rect_infinite_becomes_infinite(self):
        r = Rect(0, 0, 10, 10)
        r.include_rect(geom.INFINITE_RECT())
        assert r.is_infinite is True

    def test_include_rect_empty_argument_is_noop(self):
        r = Rect(0, 0, 10, 10)
        assert r.include_rect(Rect(5, 5, 5, 5)) is r
        assert tuple(r) == (0, 0, 10, 10)

    def test_include_point_bad_seq_len(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 10, 10).include_point((1, 2, 3))

    def test_sub_elementwise_and_bad_len(self):
        approx_seq(Rect(11, 22, 33, 44) - (1, 2, 3, 4), (10, 20, 30, 40))
        with pytest.raises(ValueError):
            Rect(0, 0, 1, 1) - (1, 2, 3)

    def test_add_bad_seq_len(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 1, 1) + (1, 2, 3)

    def test_or_bad_operands(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 1, 1) | 5  # no __len__
        with pytest.raises(ValueError):
            Rect(0, 0, 1, 1) | (1, 2, 3)  # wrong length

    def test_and_bad_operand(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 1, 1) & 5  # no __len__

    def test_truediv_scalar_and_singular(self):
        approx_seq(Rect(2, 4, 6, 8) / 2, (1, 2, 3, 4))
        with pytest.raises(ZeroDivisionError):
            Rect(0, 0, 1, 1) / Matrix(0, 0, 0, 0, 0, 0)

    def test_transform_bad_matrix_len(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 1, 1).transform((1, 2, 3))

    def test_morph_infinite_returns_infinite_quad(self):
        q = geom.INFINITE_RECT().morph(Point(0, 0), Matrix())
        assert q.is_infinite is True


# --------------------------------------------------------------------------- #
# Matrix — dunders and arithmetic guards
# --------------------------------------------------------------------------- #
class TestMatrixBranches:
    def test_repr_hash_bool(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        assert repr(m) == "Matrix(1.0, 2.0, 3.0, 4.0, 5.0, 6.0)"
        assert hash(m) == hash((1.0, 2.0, 3.0, 4.0, 5.0, 6.0))
        assert bool(Matrix(0, 0, 0, 0, 0, 0)) is False
        assert bool(Matrix()) is True

    def test_eq_foreign_type_is_false(self):
        assert (Matrix(1, 2, 3, 4, 5, 6) == object()) is False

    def test_init_bad_seq_len(self):
        with pytest.raises(ValueError):
            Matrix((1, 2, 3))

    def test_truediv_scalar(self):
        approx_seq(Matrix(2, 4, 6, 8, 10, 12) / 2, (1, 2, 3, 4, 5, 6))

    def test_truediv_matrix_and_singular(self):
        m = Matrix(1, 2, 3, 4, 5, 6)
        d = Matrix(2, 0, 0, 2, 0, 0)
        approx_seq(m / d, Matrix().concat(m, ~d))
        with pytest.raises(ZeroDivisionError):
            m / Matrix(0, 0, 0, 0, 0, 0)

    def test_sub_scalar_and_bad_len(self):
        approx_seq(Matrix(2, 3, 4, 5, 6, 7) - 1, (1, 2, 3, 4, 5, 6))
        with pytest.raises(ValueError):
            Matrix(1, 2, 3, 4, 5, 6) - (1, 2, 3)

    def test_add_bad_len(self):
        with pytest.raises(ValueError):
            Matrix(1, 2, 3, 4, 5, 6) + (1, 2, 3)

    def test_concat_bad_len(self):
        with pytest.raises(ValueError):
            Matrix().concat((1, 2, 3), Matrix())


# --------------------------------------------------------------------------- #
# IdentityMatrix — read-only pinning + non-pinned attribute path
# --------------------------------------------------------------------------- #
class TestIdentityMatrixBranches:
    def test_repr_and_hash(self):
        assert repr(geom.Identity) == "IdentityMatrix(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)"
        assert hash(geom.Identity) == hash((1.0, 0.0, 0.0, 1.0, 0.0, 0.0))

    def test_pinned_components_ignore_assignment(self):
        im = geom.IdentityMatrix()
        im.a = 9.0
        im.b = 9.0
        assert tuple(im) == (1, 0, 0, 1, 0, 0)

    def test_non_component_attribute_passes_through(self):
        im = geom.IdentityMatrix()
        im.extra = 5  # name not in (a,b,c,d,e,f) -> plain setattr
        assert im.extra == 5
        assert tuple(im) == (1, 0, 0, 1, 0, 0)


# --------------------------------------------------------------------------- #
# IRect — the whole thin surface (mostly single-line delegators)
# --------------------------------------------------------------------------- #
class TestIRectBranches:
    def test_getitem_hash_repr(self):
        r = IRect(1, 2, 3, 4)
        assert r[2] == 3
        assert hash(r) == hash((1, 2, 3, 4))
        assert repr(r) == "IRect(1, 2, 3, 4)"

    def test_eq_tuple_and_foreign(self):
        assert IRect(1, 2, 3, 4) == (1, 2, 3, 4)
        assert (IRect(1, 2, 3, 4) == object()) is False

    def test_predicates(self):
        assert IRect(0, 0, 10, 10).is_valid is True
        assert IRect(10, 0, 0, 10).is_valid is False
        assert geom.INFINITE_IRECT().is_infinite is True
        assert IRect(0, 0, 10, 10).is_infinite is False

    def test_corners_and_quad(self):
        r = IRect(1, 2, 3, 4)
        assert tuple(r.top_left) == (1, 2)
        assert tuple(r.top_right) == (3, 2)
        assert tuple(r.bottom_left) == (1, 4)
        assert tuple(r.bottom_right) == (3, 4)
        assert tuple(r.quad.ul) == (1, 2)
        assert tuple(r.quad.lr) == (3, 4)

    def test_irect_and_round_are_identity(self):
        r = IRect(1, 2, 3, 4)
        assert r.irect is r
        assert r.round() is r

    def test_containment_and_intersects(self):
        r = IRect(0, 0, 10, 10)
        assert r.contains(Point(5, 5)) is True
        assert (Point(5, 5) in r) is True
        assert r.intersects(IRect(5, 5, 20, 20)) is True
        assert r.intersects(IRect(20, 20, 30, 30)) is False

    def test_set_operators_return_irect(self):
        u = IRect(0, 0, 5, 5) | IRect(3, 3, 10, 12)
        assert isinstance(u, IRect) and tuple(u) == (0, 0, 10, 12)
        i = IRect(0, 0, 10, 10) & IRect(5, 5, 20, 20)
        assert isinstance(i, IRect) and tuple(i) == (5, 5, 10, 10)

    def test_arithmetic_operators(self):
        assert tuple(IRect(1, 2, 3, 4) + (10, 20, 30, 40)) == (11, 22, 33, 44)
        assert tuple(IRect(11, 22, 33, 44) - (10, 20, 30, 40)) == (1, 2, 3, 4)
        assert tuple(IRect(0, 0, 10, 10) * Matrix(2, 0, 0, 2, 0, 0)) == (0, 0, 20, 20)
        assert tuple(IRect(2, 4, 6, 8) / Matrix(2, 0, 0, 2, 0, 0)) == (1, 2, 3, 4)


# --------------------------------------------------------------------------- #
# Quad — constructor overloads, dunders, convexity / rectangularity sub-checks
# --------------------------------------------------------------------------- #
class TestQuadBranches:
    def test_default_is_all_origin(self):
        q = Quad()
        assert tuple(q.ul) == tuple(q.ur) == tuple(q.ll) == tuple(q.lr) == (0, 0)

    def test_bad_arg_count(self):
        with pytest.raises(ValueError):
            Quad(Point(0, 0), Point(1, 1))

    def test_repr_and_hash(self):
        q = Rect(0, 0, 1, 1).quad
        assert repr(q).startswith("Quad(")
        assert hash(q) == hash(tuple(tuple(p) for p in q))

    def test_eq_foreign_type_is_false(self):
        assert (Rect(0, 0, 1, 1).quad == 5) is False

    def test_abs_empty_and_nonempty(self):
        assert abs(Quad()) == 0.0
        assert math.isclose(abs(Rect(0, 0, 3, 4).quad), 12.0)

    def test_is_convex_second_check_false(self):
        # first convex check passes, the second (ll->ur diagonal) fails.
        q = Quad(Point(0, 0), Point(10, 0), Point(0, 10), Point(3, 3))
        assert q.is_convex is False

    def test_is_rectangular_second_and_third_check_false(self):
        # first corner is a right angle, but the second is not.
        q2 = Quad(Point(0, 0), Point(10, 0), Point(5, 5), Point(10, 10))
        assert q2.is_rectangular is False
        # first two corners are right angles, the third is not.
        q3 = Quad(Point(0, 0), Point(10, 0), Point(-5, 10), Point(10, 10))
        assert q3.is_rectangular is False

    def test_transform_bad_matrix_len(self):
        with pytest.raises(ValueError):
            Rect(0, 0, 1, 1).quad.transform((1, 2, 3))

    def test_morph_infinite_returns_infinite_quad(self):
        q = geom.INFINITE_QUAD().morph(Point(0, 0), Matrix())
        assert q.is_infinite is True


# --------------------------------------------------------------------------- #
# Module helpers / factories
# --------------------------------------------------------------------------- #
class TestModuleHelperBranches:
    def test_transform_rect_infinite_is_unchanged(self):
        r = geom.INFINITE_RECT()
        r.transform(Matrix(2, 0, 0, 2, 0, 0))
        assert r.is_infinite is True

    def test_include_point_on_infinite_is_unchanged(self):
        r = geom.INFINITE_RECT()
        r.include_point(Point(5, 5))
        assert r.is_infinite is True

    def test_make_rect_bad_seq_len(self):
        with pytest.raises(ValueError):
            Rect((1, 2, 3))

    def test_make_rect_bad_arg_count(self):
        with pytest.raises(ValueError):
            Rect(1, 2, 3)

    def test_paper_size_explicit_portrait_suffix(self):
        # a bare "-p" suffix is stripped and returns the portrait dims.
        assert geom.paper_size("a4-p") == (595, 842)

    def test_empty_irect_and_quad_factories(self):
        assert tuple(geom.EMPTY_IRECT()) == (
            geom.FZ_MAX_INF_RECT,
            geom.FZ_MAX_INF_RECT,
            geom.FZ_MIN_INF_RECT,
            geom.FZ_MIN_INF_RECT,
        )
        # EMPTY_QUAD is the quad of the (inverted-corner) empty rect.
        eq = geom.EMPTY_QUAD()
        assert tuple(eq.ul) == (geom.FZ_MAX_INF_RECT, geom.FZ_MAX_INF_RECT)
        assert tuple(eq.lr) == (geom.FZ_MIN_INF_RECT, geom.FZ_MIN_INF_RECT)


# --------------------------------------------------------------------------- #
# Property-based algebraic identities (fast, deterministic-bounded)
# --------------------------------------------------------------------------- #
_finite = st.floats(min_value=-1e4, max_value=1e4, allow_nan=False, allow_infinity=False)


class TestGeometryProperties:
    @settings(max_examples=40, deadline=None)
    @given(_finite, _finite, _finite, _finite)
    def test_point_add_sub_are_inverse(self, x, y, dx, dy):
        p = Point(x, y)
        approx_seq((p + (dx, dy)) - (dx, dy), p, tol=1e-3)

    @settings(max_examples=40, deadline=None)
    @given(
        st.floats(min_value=0.1, max_value=100, allow_nan=False),
        st.floats(min_value=0.1, max_value=100, allow_nan=False),
        _finite,
        _finite,
    )
    def test_matrix_invert_roundtrip_is_identity(self, sx, sy, tx, ty):
        m = Matrix(1, 0, 0, 1, 0, 0).prescale(sx, sy).pretranslate(tx, ty)
        approx_seq(m * ~m, (1, 0, 0, 1, 0, 0), tol=1e-3)
