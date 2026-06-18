"""PyMuPDF-compatible geometry value types (PRD §9.5).

Pure-Python ``Point``/``Rect``/``Matrix``/``IRect``/``Quad`` whose arithmetic
matches PyMuPDF (``fitz``) 1.24.x *exactly* — a Tier-A documented contract. They
behave as sequences (``r[0]``, ``tuple(r)``, unpacking) like PyMuPDF's classes,
so existing code reading ``page.rect`` keeps working.

The full algebra (operator overloads, transforms, inversion, morph/torect, Quad
convexity) mirrors PyMuPDF's own pure-Python implementation, including the C
``fz_*`` numeric formulas its thin wrappers delegate to.
"""

from __future__ import annotations

import math
import sys
from typing import Iterator, Sequence, Union

# PyMuPDF tolerance for "rectilinear"/"empty-unit" comparisons (src/__init__.py).
EPSILON = 1e-5

# Infinite-rect sentinels (largest int32 surviving a float32 round-trip / int32
# min). PyMuPDF: FZ_MIN_INF_RECT = -0x80000000, FZ_MAX_INF_RECT = 0x7fffff80.
FZ_MIN_INF_RECT = -0x80000000  # -2147483648
FZ_MAX_INF_RECT = 0x7FFFFF80  # 2147483520

# fz_round_rect clamps the rounded integer corners to ±2**24 (MAX_SAFE_INT).
_MAX_SAFE_INT = 16777216


class Point:
    """A 2-D point ``(x, y)`` (PyMuPDF ``fitz.Point``)."""

    __slots__ = ("x", "y")

    def __init__(self, *args: float) -> None:
        if len(args) == 0:
            x, y = 0.0, 0.0
        elif len(args) == 1:
            x, y = args[0]
        elif len(args) == 2:
            x, y = args
        else:
            raise ValueError("Point: bad arg count")
        self.x = float(x)
        self.y = float(y)

    def __iter__(self) -> Iterator[float]:
        yield self.x
        yield self.y

    def __len__(self) -> int:
        return 2

    def __getitem__(self, i: int) -> float:
        return (self.x, self.y)[i]

    def __eq__(self, other: object) -> bool:
        try:
            return len(other) == 2 and tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __hash__(self) -> int:
        return hash(tuple(self))

    def __repr__(self) -> str:
        return f"Point({self.x}, {self.y})"

    # --- arithmetic ---
    def __add__(self, p: object) -> "Point":
        if hasattr(p, "__float__"):
            return Point(self.x + p, self.y + p)  # type: ignore[operator]
        if len(p) != 2:  # type: ignore[arg-type]
            raise ValueError("Point: bad seq len")
        return Point(self.x + p[0], self.y + p[1])  # type: ignore[index]

    def __sub__(self, p: object) -> "Point":
        if hasattr(p, "__float__"):
            return Point(self.x - p, self.y - p)  # type: ignore[operator]
        if len(p) != 2:  # type: ignore[arg-type]
            raise ValueError("Point: bad seq len")
        return Point(self.x - p[0], self.y - p[1])  # type: ignore[index]

    def __mul__(self, m: object):
        if hasattr(m, "__float__"):
            return Point(self.x * m, self.y * m)  # type: ignore[operator]
        if hasattr(m, "__getitem__") and len(m) == 2:  # type: ignore[arg-type]
            # dot product
            return self.x * m[0] + self.y * m[1]  # type: ignore[index]
        p = Point(self)
        return p.transform(m)  # type: ignore[arg-type]

    def __truediv__(self, m: object) -> "Point":
        if hasattr(m, "__float__"):
            return Point(self.x * 1.0 / m, self.y * 1.0 / m)  # type: ignore[operator]
        sign, inv = _invert_matrix(m)  # type: ignore[arg-type]
        if sign == 1:
            raise ZeroDivisionError("matrix not invertible")
        p = Point(self)
        return p.transform(Matrix(inv))

    def __abs__(self) -> float:
        return math.sqrt(self.x * self.x + self.y * self.y)

    def __neg__(self) -> "Point":
        return Point(-self.x, -self.y)

    norm = __abs__

    def distance_to(self, *args) -> float:
        """Distance to another point or to a rectangle (PyMuPDF semantics).

        Units: ``px`` (default), ``in``, ``cm``, ``mm`` — there is no ``pt``.
        """
        if not len(args) > 0:
            raise ValueError("at least one parameter must be given")
        x = args[0]
        if len(x) == 2:
            x = Point(x)
        elif len(x) == 4:
            x = Rect(x)
        else:
            raise ValueError("arg1 must be point-like or rect-like")
        unit = args[1] if len(args) > 1 else "px"
        u = {"px": (1.0, 1.0), "in": (1.0, 72.0), "cm": (2.54, 72.0), "mm": (25.4, 72.0)}
        f = u[unit][0] / u[unit][1]
        if isinstance(x, Point):
            return abs(self - x) * f
        # x is a rectangle (finite copy)
        r = Rect(x.top_left, x.top_left)
        r = r | x.bottom_right
        if self in r:
            return 0.0
        if self.x > r.x1:
            if self.y >= r.y1:
                return self.distance_to(r.bottom_right, unit)
            elif self.y <= r.y0:
                return self.distance_to(r.top_right, unit)
            else:
                return (self.x - r.x1) * f
        elif r.x0 <= self.x <= r.x1:
            if self.y >= r.y1:
                return (self.y - r.y1) * f
            else:
                return (r.y0 - self.y) * f
        else:
            if self.y >= r.y1:
                return self.distance_to(r.bottom_left, unit)
            elif self.y <= r.y0:
                return self.distance_to(r.top_left, unit)
            else:
                return (r.x0 - self.x) * f

    def transform(self, m) -> "Point":
        """Apply matrix ``m`` in place and return ``self``."""
        if len(m) != 6:
            raise ValueError("Matrix: bad seq len")
        a, b, c, d, e, f = m
        self.x, self.y = self.x * a + self.y * c + e, self.x * b + self.y * d + f
        return self

    @property
    def unit(self) -> "Point":
        s = self.x * self.x + self.y * self.y
        if s < EPSILON:
            return Point(0, 0)
        s = math.sqrt(s)
        return Point(self.x / s, self.y / s)

    @property
    def abs_unit(self) -> "Point":
        s = self.x * self.x + self.y * self.y
        if s < EPSILON:
            return Point(0, 0)
        s = math.sqrt(s)
        return Point(abs(self.x) / s, abs(self.y) / s)


class Rect:
    """An axis-aligned rectangle ``(x0, y0, x1, y1)`` (PyMuPDF ``fitz.Rect``).

    ``(x0, y0)`` is the top-left, ``(x1, y1)`` the bottom-right corner in
    PyMuPDF's top-left/y-down device space.
    """

    __slots__ = ("x0", "y0", "x1", "y1")

    def __init__(self, *args: float) -> None:
        x0, y0, x1, y1 = _make_rect(*args)
        self.x0 = float(x0)
        self.y0 = float(y0)
        self.x1 = float(x1)
        self.y1 = float(y1)

    # --- sequence protocol (PyMuPDF Rect is a 4-sequence) ---
    def __iter__(self) -> Iterator[float]:
        yield from (self.x0, self.y0, self.x1, self.y1)

    def __len__(self) -> int:
        return 4

    def __getitem__(self, i: int) -> float:
        return (self.x0, self.y0, self.x1, self.y1)[i]

    def __eq__(self, other: object) -> bool:
        try:
            return len(other) == 4 and tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __hash__(self) -> int:
        return hash(tuple(self))

    def __repr__(self) -> str:
        return f"Rect({self.x0}, {self.y0}, {self.x1}, {self.y1})"

    # --- dimensions ---
    @property
    def width(self) -> float:
        return max(0.0, self.x1 - self.x0)

    @property
    def height(self) -> float:
        return max(0.0, self.y1 - self.y0)

    def __abs__(self) -> float:
        if self.is_empty or self.is_infinite:
            return 0.0
        return (self.x1 - self.x0) * (self.y1 - self.y0)

    def norm(self) -> float:
        return math.sqrt(sum(c * c for c in self))

    def get_area(self, unit: str = "px") -> float:
        u = {"px": (1.0, 1.0), "in": (1.0, 72.0), "cm": (2.54, 72.0), "mm": (25.4, 72.0)}
        f = (u[unit][0] / u[unit][1]) ** 2
        return abs(self) * f

    # --- predicates ---
    @property
    def is_empty(self) -> bool:
        return self.x0 >= self.x1 or self.y0 >= self.y1

    @property
    def is_valid(self) -> bool:
        return self.x0 <= self.x1 and self.y0 <= self.y1

    @property
    def is_infinite(self) -> bool:
        return (
            self.x0 == self.y0 == FZ_MIN_INF_RECT
            and self.x1 == self.y1 == FZ_MAX_INF_RECT
        )

    # --- corners ---
    @property
    def top_left(self) -> Point:
        return Point(self.x0, self.y0)

    tl = top_left

    @property
    def bottom_right(self) -> Point:
        return Point(self.x1, self.y1)

    br = bottom_right

    @property
    def top_right(self) -> Point:
        return Point(self.x1, self.y0)

    tr = top_right

    @property
    def bottom_left(self) -> Point:
        return Point(self.x0, self.y1)

    bl = bottom_left

    @property
    def quad(self) -> "Quad":
        return Quad(self.tl, self.tr, self.bl, self.br)

    # --- rounding ---
    def round(self) -> "IRect":
        return IRect(_round_rect(self))

    @property
    def irect(self) -> "IRect":
        return self.round()

    def normalize(self) -> "Rect":
        if self.x1 < self.x0:
            self.x0, self.x1 = self.x1, self.x0
        if self.y1 < self.y0:
            self.y0, self.y1 = self.y1, self.y0
        return self

    # --- set / geometry operations (mutate self, return self) ---
    def include_point(self, p) -> "Rect":
        if len(p) != 2:
            raise ValueError("Point: bad seq len")
        self.x0, self.y0, self.x1, self.y1 = _include_point_in_rect(self, p)
        return self

    def include_rect(self, r) -> "Rect":
        if len(r) != 4:
            raise ValueError("Rect: bad seq len")
        r = Rect(r)
        if r.is_infinite or self.is_infinite:
            self.x0, self.y0, self.x1, self.y1 = (
                FZ_MIN_INF_RECT,
                FZ_MIN_INF_RECT,
                FZ_MAX_INF_RECT,
                FZ_MAX_INF_RECT,
            )
        elif r.is_empty:
            return self
        elif self.is_empty:
            self.x0, self.y0, self.x1, self.y1 = r.x0, r.y0, r.x1, r.y1
        else:
            self.x0, self.y0, self.x1, self.y1 = _union_rect(self, r)
        return self

    def intersect(self, r) -> "Rect":
        if not len(r) == 4:
            raise ValueError("Rect: bad seq len")
        r = Rect(r)
        if r.is_infinite:
            return self
        elif self.is_infinite:
            self.x0, self.y0, self.x1, self.y1 = r.x0, r.y0, r.x1, r.y1
        elif r.is_empty:
            self.x0, self.y0, self.x1, self.y1 = r.x0, r.y0, r.x1, r.y1
        elif self.is_empty:
            return self
        else:
            self.x0, self.y0, self.x1, self.y1 = _intersect_rect(self, r)
        return self

    def transform(self, m) -> "Rect":
        if not len(m) == 6:
            raise ValueError("Matrix: bad seq len")
        self.x0, self.y0, self.x1, self.y1 = _transform_rect(self, m)
        return self

    def morph(self, p, m) -> "Quad":
        if self.is_infinite:
            return INFINITE_QUAD()
        return self.quad.morph(p, m)

    def torect(self, r) -> "Matrix":
        r = Rect(r)
        if self.is_infinite or self.is_empty or r.is_infinite or r.is_empty:
            raise ValueError("rectangles must be finite and not empty")
        return (
            Matrix(1, 0, 0, 1, -self.x0, -self.y0)
            * Matrix(r.width / self.width, r.height / self.height)
            * Matrix(1, 0, 0, 1, r.x0, r.y0)
        )

    # --- queries ---
    def contains(self, x) -> bool:
        return self.__contains__(x)

    def __contains__(self, x) -> bool:
        if hasattr(x, "__float__"):
            return x in tuple(self)
        try:
            length = len(x)
        except TypeError:
            return False
        if length == 2:
            return _is_point_in_rect(x, self)
        if length == 4:
            try:
                r = Rect(x)
            except Exception:
                r = Quad(x).rect
            return (
                self.x0 <= r.x0 <= r.x1 <= self.x1
                and self.y0 <= r.y0 <= r.y1 <= self.y1
            )
        return False

    def intersects(self, x) -> bool:
        r1 = Rect(x)
        if self.is_empty or self.is_infinite or r1.is_empty or r1.is_infinite:
            return False
        r = Rect(self)
        if r.intersect(r1).is_empty:
            return False
        return True

    # --- operators (return NEW objects) ---
    def __or__(self, x) -> "Rect":
        if not hasattr(x, "__len__"):
            raise ValueError("bad operand 2")
        r = Rect(self)
        if len(x) == 2:
            return r.include_point(x)
        if len(x) == 4:
            return r.include_rect(x)
        raise ValueError("bad operand 2")

    def __and__(self, x) -> "Rect":
        if not hasattr(x, "__len__"):
            raise ValueError("bad operand 2")
        r1 = Rect(x)
        r = Rect(self)
        return r.intersect(r1)

    def __add__(self, p) -> "Rect":
        if hasattr(p, "__float__"):
            return Rect(self.x0 + p, self.y0 + p, self.x1 + p, self.y1 + p)
        if len(p) != 4:
            raise ValueError("Rect: bad seq len")
        return Rect(self.x0 + p[0], self.y0 + p[1], self.x1 + p[2], self.y1 + p[3])

    def __sub__(self, p) -> "Rect":
        if hasattr(p, "__float__"):
            return Rect(self.x0 - p, self.y0 - p, self.x1 - p, self.y1 - p)
        if len(p) != 4:
            raise ValueError("Rect: bad seq len")
        return Rect(self.x0 - p[0], self.y0 - p[1], self.x1 - p[2], self.y1 - p[3])

    def __mul__(self, m) -> "Rect":
        if hasattr(m, "__float__"):
            return Rect(self.x0 * m, self.y0 * m, self.x1 * m, self.y1 * m)
        r = Rect(self)
        return r.transform(m)

    def __truediv__(self, m) -> "Rect":
        if hasattr(m, "__float__"):
            return Rect(
                self.x0 * 1.0 / m, self.y0 * 1.0 / m, self.x1 * 1.0 / m, self.y1 * 1.0 / m
            )
        sign, im = _invert_matrix(m)
        if sign == 1:
            raise ZeroDivisionError(f"Matrix not invertible: {m}")
        r = Rect(self)
        return r.transform(Matrix(im))


class IRect:
    """An integer rectangle (PyMuPDF ``fitz.IRect``)."""

    __slots__ = ("x0", "y0", "x1", "y1")

    def __init__(self, *args: int) -> None:
        x0, y0, x1, y1 = _make_irect(*args)
        self.x0 = int(x0)
        self.y0 = int(y0)
        self.x1 = int(x1)
        self.y1 = int(y1)

    def __iter__(self) -> Iterator[int]:
        yield from (self.x0, self.y0, self.x1, self.y1)

    def __len__(self) -> int:
        return 4

    def __getitem__(self, i: int) -> int:
        return (self.x0, self.y0, self.x1, self.y1)[i]

    def __eq__(self, other: object) -> bool:
        try:
            return len(other) == 4 and tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __hash__(self) -> int:
        return hash(tuple(self))

    def __repr__(self) -> str:
        return f"IRect({self.x0}, {self.y0}, {self.x1}, {self.y1})"

    # --- dimensions ---
    @property
    def width(self) -> int:
        return max(0, self.x1 - self.x0)

    @property
    def height(self) -> int:
        return max(0, self.y1 - self.y0)

    def get_area(self, unit: str = "px") -> float:
        return self.rect.get_area(unit)

    # --- predicates (same form as Rect) ---
    @property
    def is_empty(self) -> bool:
        return self.x0 >= self.x1 or self.y0 >= self.y1

    @property
    def is_valid(self) -> bool:
        return self.x0 <= self.x1 and self.y0 <= self.y1

    @property
    def is_infinite(self) -> bool:
        return (
            self.x0 == self.y0 == FZ_MIN_INF_RECT
            and self.x1 == self.y1 == FZ_MAX_INF_RECT
        )

    # --- corners ---
    @property
    def top_left(self) -> Point:
        return Point(self.x0, self.y0)

    tl = top_left

    @property
    def bottom_right(self) -> Point:
        return Point(self.x1, self.y1)

    br = bottom_right

    @property
    def top_right(self) -> Point:
        return Point(self.x1, self.y0)

    tr = top_right

    @property
    def bottom_left(self) -> Point:
        return Point(self.x0, self.y1)

    bl = bottom_left

    @property
    def quad(self) -> "Quad":
        return Quad(self.tl, self.tr, self.bl, self.br)

    @property
    def rect(self) -> Rect:
        return Rect(self)

    @property
    def irect(self) -> "IRect":
        return self

    def round(self) -> "IRect":
        return self

    def normalize(self) -> "IRect":
        if self.x1 < self.x0:
            self.x0, self.x1 = self.x1, self.x0
        if self.y1 < self.y0:
            self.y0, self.y1 = self.y1, self.y0
        return self

    # --- operations: defer to Rect then re-round back to IRect ---
    def include_point(self, p) -> "IRect":
        return self.rect.include_point(p).irect

    def include_rect(self, r) -> "IRect":
        return self.rect.include_rect(r).irect

    def intersect(self, r) -> "IRect":
        return self.rect.intersect(r).round()

    def transform(self, m) -> "IRect":
        return self.rect.transform(m).round()

    def morph(self, p, m) -> "Quad":
        return self.rect.morph(p, m)

    def torect(self, r) -> "Matrix":
        return self.rect.torect(r)

    def contains(self, x) -> bool:
        return self.rect.contains(x)

    def __contains__(self, x) -> bool:
        return self.rect.__contains__(x)

    def intersects(self, x) -> bool:
        return self.rect.intersects(x)

    def __or__(self, x) -> "IRect":
        return Rect.__or__(self.rect, x).round()

    def __and__(self, x) -> "IRect":
        return Rect.__and__(self.rect, x).round()

    def __add__(self, p) -> "IRect":
        return Rect.__add__(self.rect, p).round()

    def __sub__(self, p) -> "IRect":
        return Rect.__sub__(self.rect, p).round()

    def __mul__(self, m) -> "IRect":
        return Rect.__mul__(self.rect, m).round()

    def __truediv__(self, m) -> "IRect":
        return Rect.__truediv__(self.rect, m).round()


class Matrix:
    """A 2-D affine matrix ``[a b c d e f]`` (PyMuPDF ``fitz.Matrix``).

    Maps a point ``(x, y)`` to ``(a*x + c*y + e, b*x + d*y + f)``.
    """

    __slots__ = ("a", "b", "c", "d", "e", "f")

    def __init__(self, *args: float) -> None:
        if len(args) == 0:
            vals = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
        elif len(args) == 1 and isinstance(args[0], (int, float)):
            # Matrix(degree) → anti-clockwise rotation matrix (PyMuPDF docs).
            self.a, self.b, self.c, self.d, self.e, self.f = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            self.prerotate(float(args[0]))
            return
        elif len(args) == 1:
            vals = tuple(args[0])
            if len(vals) != 6:
                raise ValueError("Matrix: bad seq len")
        elif len(args) == 2:
            # Matrix(sx, sy) → scaling matrix.
            vals = (float(args[0]), 0.0, 0.0, float(args[1]), 0.0, 0.0)
        elif len(args) == 6:
            vals = tuple(float(v) for v in args)
        else:
            raise ValueError("Matrix takes 0, 1, 2 or 6 arguments")
        self.a, self.b, self.c, self.d, self.e, self.f = (float(v) for v in vals)

    def __iter__(self) -> Iterator[float]:
        yield from (self.a, self.b, self.c, self.d, self.e, self.f)

    def __len__(self) -> int:
        return 6

    def __getitem__(self, i: int) -> float:
        return (self.a, self.b, self.c, self.d, self.e, self.f)[i]

    def __eq__(self, other: object) -> bool:
        try:
            return len(other) == 6 and tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __hash__(self) -> int:
        return hash(tuple(self))

    def __repr__(self) -> str:
        return f"Matrix({self.a}, {self.b}, {self.c}, {self.d}, {self.e}, {self.f})"

    def __bool__(self) -> bool:
        return any(x != 0 for x in self)

    # --- products ---
    def __mul__(self, m) -> "Matrix":
        if hasattr(m, "__float__"):
            return Matrix(
                self.a * m, self.b * m, self.c * m, self.d * m, self.e * m, self.f * m
            )
        return Matrix().concat(self, m)

    def __truediv__(self, m) -> "Matrix":
        if hasattr(m, "__float__"):
            return Matrix(
                self.a * 1.0 / m,
                self.b * 1.0 / m,
                self.c * 1.0 / m,
                self.d * 1.0 / m,
                self.e * 1.0 / m,
                self.f * 1.0 / m,
            )
        sign, im = _invert_matrix(m)
        if sign == 1:
            raise ZeroDivisionError("matrix not invertible")
        return Matrix().concat(self, Matrix(im))

    def __add__(self, m) -> "Matrix":
        if hasattr(m, "__float__"):
            return Matrix(
                self.a + m, self.b + m, self.c + m, self.d + m, self.e + m, self.f + m
            )
        if len(m) != 6:
            raise ValueError("Matrix: bad seq len")
        return Matrix(
            self.a + m[0],
            self.b + m[1],
            self.c + m[2],
            self.d + m[3],
            self.e + m[4],
            self.f + m[5],
        )

    def __sub__(self, m) -> "Matrix":
        if hasattr(m, "__float__"):
            return Matrix(
                self.a - m, self.b - m, self.c - m, self.d - m, self.e - m, self.f - m
            )
        if len(m) != 6:
            raise ValueError("Matrix: bad seq len")
        return Matrix(
            self.a - m[0],
            self.b - m[1],
            self.c - m[2],
            self.d - m[3],
            self.e - m[4],
            self.f - m[5],
        )

    def __neg__(self) -> "Matrix":
        return Matrix(-self.a, -self.b, -self.c, -self.d, -self.e, -self.f)

    def __abs__(self) -> float:
        return math.sqrt(sum(c * c for c in self))

    norm = __abs__

    def __invert__(self) -> "Matrix":
        m1 = Matrix()
        m1.invert(self)
        return m1

    __inv__ = __invert__

    def concat(self, one, two) -> "Matrix":
        """``self = one * two`` (one-then-two), then return ``self``."""
        if not len(one) == len(two) == 6:
            raise ValueError("Matrix: bad seq len")
        self.a, self.b, self.c, self.d, self.e, self.f = _concat_matrix(one, two)
        return self

    def invert(self, src=None) -> int:
        """Invert ``src`` (or self) into self. Return 0 on success, 1 if degenerate."""
        sign, dst = _invert_matrix(src if src is not None else self)
        if sign == 1:
            return 1
        self.a, self.b, self.c, self.d, self.e, self.f = dst
        return 0

    # --- pre-operations (mutate self, return self) ---
    def prerotate(self, theta: float) -> "Matrix":
        theta = float(theta)
        while theta < 0:
            theta += 360
        while theta >= 360:
            theta -= 360
        if abs(0 - theta) < EPSILON:
            pass
        elif abs(90.0 - theta) < EPSILON:
            a = self.a
            b = self.b
            self.a = self.c
            self.b = self.d
            self.c = -a
            self.d = -b
        elif abs(180.0 - theta) < EPSILON:
            self.a = -self.a
            self.b = -self.b
            self.c = -self.c
            self.d = -self.d
        elif abs(270.0 - theta) < EPSILON:
            a = self.a
            b = self.b
            self.a = -self.c
            self.b = -self.d
            self.c = a
            self.d = b
        else:
            rad = math.radians(theta)
            s = math.sin(rad)
            c = math.cos(rad)
            a = self.a
            b = self.b
            self.a = c * a + s * self.c
            self.b = c * b + s * self.d
            self.c = -s * a + c * self.c
            self.d = -s * b + c * self.d
        return self

    def prescale(self, sx: float, sy: float) -> "Matrix":
        sx = float(sx)
        sy = float(sy)
        self.a *= sx
        self.b *= sx
        self.c *= sy
        self.d *= sy
        return self

    def preshear(self, h: float, v: float) -> "Matrix":
        h = float(h)
        v = float(v)
        a, b = self.a, self.b
        self.a += v * self.c
        self.b += v * self.d
        self.c += h * a
        self.d += h * b
        return self

    def pretranslate(self, tx: float, ty: float) -> "Matrix":
        tx = float(tx)
        ty = float(ty)
        self.e += tx * self.a + ty * self.c
        self.f += tx * self.b + ty * self.d
        return self

    @property
    def is_rectilinear(self) -> bool:
        return (abs(self.b) < EPSILON and abs(self.c) < EPSILON) or (
            abs(self.a) < EPSILON and abs(self.d) < EPSILON
        )


class IdentityMatrix(Matrix):
    """The read-only identity matrix ``[1 0 0 1 0 0]`` (PyMuPDF ``Identity``)."""

    def __init__(self) -> None:
        Matrix.__init__(self, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0)

    def __repr__(self) -> str:
        return "IdentityMatrix(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)"

    def __hash__(self) -> int:
        return hash((1.0, 0.0, 0.0, 1.0, 0.0, 0.0))

    def __setattr__(self, name: str, value: float) -> None:
        # Pin the identity values regardless of assignment (read-only).
        if name in ("a", "d"):
            super().__setattr__(name, 1.0)
        elif name in ("b", "c", "e", "f"):
            super().__setattr__(name, 0.0)
        else:
            super().__setattr__(name, value)


class Quad:
    """A quadrilateral with corners ``ul, ur, ll, lr`` (PyMuPDF ``fitz.Quad``)."""

    __slots__ = ("ul", "ur", "ll", "lr")

    def __init__(self, *args) -> None:
        if len(args) == 0:
            ul = ur = ll = lr = Point()
        elif len(args) == 1:
            seq = args[0]
            ul, ur, ll, lr = (Point(p) for p in seq)
        elif len(args) == 4:
            ul, ur, ll, lr = (Point(p) for p in args)
        else:
            raise ValueError("Quad: bad arg count")
        self.ul = ul
        self.ur = ur
        self.ll = ll
        self.lr = lr

    def __iter__(self) -> Iterator[Point]:
        yield from (self.ul, self.ur, self.ll, self.lr)

    def __len__(self) -> int:
        return 4

    def __getitem__(self, i: int) -> Point:
        return (self.ul, self.ur, self.ll, self.lr)[i]

    def __eq__(self, other: object) -> bool:
        try:
            return len(other) == 4 and all(  # type: ignore[arg-type]
                tuple(a) == tuple(b) for a, b in zip(self, other)  # type: ignore[arg-type]
            )
        except TypeError:
            return NotImplemented

    def __hash__(self) -> int:
        return hash(tuple(tuple(p) for p in self))

    def __repr__(self) -> str:
        return f"Quad({self.ul}, {self.ur}, {self.ll}, {self.lr})"

    @property
    def rect(self) -> Rect:
        r = Rect()
        r.x0 = min(self.ul.x, self.ur.x, self.lr.x, self.ll.x)
        r.y0 = min(self.ul.y, self.ur.y, self.lr.y, self.ll.y)
        r.x1 = max(self.ul.x, self.ur.x, self.lr.x, self.ll.x)
        r.y1 = max(self.ul.y, self.ur.y, self.lr.y, self.ll.y)
        return r

    @property
    def width(self) -> float:
        return max(abs(self.ul - self.ur), abs(self.ll - self.lr))

    @property
    def height(self) -> float:
        return max(abs(self.ul - self.ll), abs(self.ur - self.lr))

    def __abs__(self) -> float:
        if self.is_empty:
            return 0.0
        return abs(self.ul - self.ur) * abs(self.ul - self.ll)

    @property
    def is_empty(self) -> bool:
        return self.width < EPSILON or self.height < EPSILON

    @property
    def is_infinite(self) -> bool:
        return self.rect.is_infinite

    @property
    def is_convex(self) -> bool:
        m = _planish_line(self.ul, self.lr)
        p1 = self.ll * m
        p2 = self.ur * m
        if p1.y * p2.y > 0:
            return False
        m = _planish_line(self.ll, self.ur)
        p1 = self.lr * m
        p2 = self.ul * m
        if p1.y * p2.y > 0:
            return False
        return True

    @property
    def is_rectangular(self) -> bool:
        sine = _sine_between(self.ul, self.ur, self.lr)
        if abs(sine - 1) > EPSILON:
            return False
        sine = _sine_between(self.ur, self.lr, self.ll)
        if abs(sine - 1) > EPSILON:
            return False
        sine = _sine_between(self.lr, self.ll, self.ul)
        if abs(sine - 1) > EPSILON:
            return False
        return True

    def transform(self, m) -> "Quad":
        if not hasattr(m, "__float__"):
            if len(m) != 6:
                raise ValueError("Matrix: bad seq len")
        self.ul *= m
        self.ur *= m
        self.ll *= m
        self.lr *= m
        return self

    def __mul__(self, m) -> "Quad":
        q = Quad(self)
        return q.transform(m)

    def morph(self, p, m) -> "Quad":
        if self.is_infinite:
            return INFINITE_QUAD()
        delta = Matrix(1, 1).pretranslate(p.x, p.y)
        return self * ~delta * m * delta


# --------------------------------------------------------------------------- #
# Construction helpers
# --------------------------------------------------------------------------- #
def _make_rect(*args) -> tuple[float, float, float, float]:
    if len(args) == 0:
        return 0.0, 0.0, 0.0, 0.0
    if len(args) == 1:
        seq = args[0]
        if len(seq) == 4:
            return tuple(float(v) for v in seq)  # type: ignore[return-value]
        raise ValueError("Rect: bad seq len")
    if len(args) == 2:
        # two point-likes
        p1, p2 = args
        return float(p1[0]), float(p1[1]), float(p2[0]), float(p2[1])
    if len(args) == 4:
        return tuple(float(v) for v in args)  # type: ignore[return-value]
    raise ValueError("Rect: bad arg count")


def _make_irect(*args) -> tuple[int, int, int, int]:
    x0, y0, x1, y1 = _make_rect(*args)
    return (
        int(math.floor(x0)),
        int(math.floor(y0)),
        int(math.ceil(x1)),
        int(math.ceil(y1)),
    )


# --------------------------------------------------------------------------- #
# Numeric primitives (mirror MuPDF's fz_* formulas PyMuPDF delegates to)
# --------------------------------------------------------------------------- #
def _concat_matrix(one, two) -> tuple[float, ...]:
    a1, b1, c1, d1, e1, f1 = one
    a2, b2, c2, d2, e2, f2 = two
    return (
        a1 * a2 + b1 * c2,
        a1 * b2 + b1 * d2,
        c1 * a2 + d1 * c2,
        c1 * b2 + d1 * d2,
        e1 * a2 + f1 * c2 + e2,
        e1 * b2 + f1 * d2 + f2,
    )


def _invert_matrix(src) -> tuple[int, tuple[float, ...]]:
    """Return ``(0, inverse)`` or ``(1, ())`` if degenerate (PyMuPDF semantics)."""
    a, b, c, d, e, f = src
    det = a * d - b * c
    if det < -sys.float_info.epsilon or det > sys.float_info.epsilon:
        rdet = 1.0 / det
        ia = d * rdet
        ib = -b * rdet
        ic = -c * rdet
        id_ = a * rdet
        ie = -e * ia - f * ic
        if_ = -e * ib - f * id_
        return 0, (ia, ib, ic, id_, ie, if_)
    return 1, ()


def _transform_rect(r, m) -> tuple[float, float, float, float]:
    rect = Rect(r)
    if rect.is_infinite:
        return rect.x0, rect.y0, rect.x1, rect.y1
    a, b, c, d, e, f = m

    def tx(px, py):
        return px * a + py * c + e, px * b + py * d + f

    xs0, ys0 = tx(rect.x0, rect.y0)
    xs1, ys1 = tx(rect.x0, rect.y1)
    xs2, ys2 = tx(rect.x1, rect.y1)
    xs3, ys3 = tx(rect.x1, rect.y0)
    return (
        min(xs0, xs1, xs2, xs3),
        min(ys0, ys1, ys2, ys3),
        max(xs0, xs1, xs2, xs3),
        max(ys0, ys1, ys2, ys3),
    )


def _round_rect(r) -> tuple[int, int, int, int]:
    def clamp(v: float) -> int:
        return max(-_MAX_SAFE_INT, min(_MAX_SAFE_INT, v))

    return (
        clamp(int(math.floor(r.x0 + 0.001))),
        clamp(int(math.floor(r.y0 + 0.001))),
        clamp(int(math.ceil(r.x1 - 0.001))),
        clamp(int(math.ceil(r.y1 - 0.001))),
    )


def _include_point_in_rect(r, p) -> tuple[float, float, float, float]:
    rect = Rect(r)
    if rect.is_infinite:
        return rect.x0, rect.y0, rect.x1, rect.y1
    px, py = p[0], p[1]
    x0 = min(rect.x0, px)
    y0 = min(rect.y0, py)
    x1 = max(rect.x1, px)
    y1 = max(rect.y1, py)
    return x0, y0, x1, y1


def _intersect_rect(a, b) -> tuple[float, float, float, float]:
    return (
        max(a[0], b[0]),
        max(a[1], b[1]),
        min(a[2], b[2]),
        min(a[3], b[3]),
    )


def _union_rect(a, b) -> tuple[float, float, float, float]:
    return (
        min(a[0], b[0]),
        min(a[1], b[1]),
        max(a[2], b[2]),
        max(a[3], b[3]),
    )


def _is_point_in_rect(p, r) -> bool:
    px, py = p[0], p[1]
    return r.x0 <= px < r.x1 and r.y0 <= py < r.y1


def _normalize_vector(x: float, y: float) -> tuple[float, float]:
    length = x * x + y * y
    if length != 0.0:
        length = math.sqrt(length)
        x /= length
        y /= length
    return x, y


def _hor_matrix(c, p) -> tuple[float, ...]:
    """Matrix mapping ``c`` to the origin and ``p`` onto the +x axis (fz_hor).

    ``s = normalize(p - c) = (cos, sin)``; result = translate(-c) then rotate.
    """
    sx, sy = _normalize_vector(p[0] - c[0], p[1] - c[1])
    m1 = (1.0, 0.0, 0.0, 1.0, -c[0], -c[1])
    m2 = (sx, -sy, sy, sx, 0.0, 0.0)
    return _concat_matrix(m1, m2)


def _planish_line(p1, p2) -> "Matrix":
    return Matrix(_hor_matrix(p1, p2))


def _sine_between(c, p, q) -> float:
    """Sine of the angle at ``p`` between lines ``pc`` and ``pq``.

    Mirrors PyMuPDF ``util_sine_between(C, P, Q)``: translate so ``p`` is the
    origin, rotate so ``p→q`` lands on the +x axis, transform ``c``, normalize,
    return its ``y`` component.
    """
    sx, sy = _normalize_vector(q[0] - p[0], q[1] - p[1])
    m1 = (1.0, 0.0, 0.0, 1.0, -p[0], -p[1])
    m2 = (sx, -sy, sy, sx, 0.0, 0.0)
    m = _concat_matrix(m1, m2)
    cx = c[0] * m[0] + c[1] * m[2] + m[4]
    cy = c[0] * m[1] + c[1] * m[3] + m[5]
    _, ny = _normalize_vector(cx, cy)
    return ny


# --------------------------------------------------------------------------- #
# Module-level singletons & factories (PyMuPDF surface)
# --------------------------------------------------------------------------- #
Identity = IdentityMatrix()


def EMPTY_RECT() -> Rect:
    return Rect(FZ_MAX_INF_RECT, FZ_MAX_INF_RECT, FZ_MIN_INF_RECT, FZ_MIN_INF_RECT)


def EMPTY_IRECT() -> IRect:
    return IRect(FZ_MAX_INF_RECT, FZ_MAX_INF_RECT, FZ_MIN_INF_RECT, FZ_MIN_INF_RECT)


def EMPTY_QUAD() -> Quad:
    return EMPTY_RECT().quad


def INFINITE_RECT() -> Rect:
    return Rect(FZ_MIN_INF_RECT, FZ_MIN_INF_RECT, FZ_MAX_INF_RECT, FZ_MAX_INF_RECT)


def INFINITE_IRECT() -> IRect:
    return IRect(FZ_MIN_INF_RECT, FZ_MIN_INF_RECT, FZ_MAX_INF_RECT, FZ_MAX_INF_RECT)


def INFINITE_QUAD() -> Quad:
    return INFINITE_RECT().quad


def paper_sizes() -> dict[str, tuple[int, int]]:
    """PyMuPDF's paper-size table (portrait, points @72dpi)."""
    return {
        "a0": (2384, 3370),
        "a1": (1684, 2384),
        "a2": (1191, 1684),
        "a3": (842, 1191),
        "a4": (595, 842),
        "a5": (420, 595),
        "a6": (298, 420),
        "a7": (210, 298),
        "a8": (147, 210),
        "a9": (105, 147),
        "a10": (74, 105),
        "b0": (2835, 4008),
        "b1": (2004, 2835),
        "b2": (1417, 2004),
        "b3": (1001, 1417),
        "b4": (709, 1001),
        "b5": (499, 709),
        "b6": (354, 499),
        "b7": (249, 354),
        "b8": (176, 249),
        "b9": (125, 176),
        "b10": (88, 125),
        "c0": (2599, 3677),
        "c1": (1837, 2599),
        "c2": (1298, 1837),
        "c3": (918, 1298),
        "c4": (649, 918),
        "c5": (459, 649),
        "c6": (323, 459),
        "c7": (230, 323),
        "c8": (162, 230),
        "c9": (113, 162),
        "c10": (79, 113),
        "card-4x6": (288, 432),
        "card-5x7": (360, 504),
        "commercial": (297, 684),
        "executive": (522, 756),
        "invoice": (396, 612),
        "ledger": (792, 1224),
        "legal": (612, 1008),
        "legal-13": (612, 936),
        "letter": (612, 792),
        "monarch": (279, 540),
        "tabloid-extra": (864, 1296),
    }


def paper_size(s: str) -> tuple[int, int]:
    """``(width, height)`` for a paper name; ``-L``/``-P`` choose orientation."""
    size = s.lower()
    orient = "p"
    if size.endswith("-l"):
        orient = "l"
        size = size[:-2]
    if size.endswith("-p"):
        size = size[:-2]
    rc = paper_sizes().get(size, (-1, -1))
    if orient == "p":
        return rc
    return (rc[1], rc[0])


def paper_rect(s: str) -> Rect:
    """A ``Rect`` for the given paper size with top-left at the origin."""
    width, height = paper_size(s)
    return Rect(0.0, 0.0, width, height)


# --------------------------------------------------------------------------- #
# PyMuPDF "*_like" type aliases (accepted anywhere a geometry value is wanted)
# --------------------------------------------------------------------------- #
point_like = Union[Point, Sequence[float]]
rect_like = Union[Rect, IRect, Sequence[float]]
matrix_like = Union[Matrix, Sequence[float]]
quad_like = Union[Quad, Rect, IRect, Sequence]
