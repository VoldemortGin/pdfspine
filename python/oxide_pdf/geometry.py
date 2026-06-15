"""PyMuPDF-compatible geometry value types (PRD §9.5).

Pure-Python ``Point``/``Rect``/``Matrix``/``IRect``/``Quad`` whose arithmetic
matches PyMuPDF (``fitz``) exactly — a Tier-A documented contract. They behave as
sequences (``r[0]``, ``tuple(r)``, unpacking) like PyMuPDF's classes, so existing
code reading ``page.rect`` keeps working.

Only the subset M1 needs is implemented; the full algebra (operator overloads,
transforms) is fleshed out alongside the rest of the shim in M5.
"""

from __future__ import annotations

import math
from typing import Iterator


class Point:
    """A 2-D point ``(x, y)`` (PyMuPDF ``fitz.Point``)."""

    __slots__ = ("x", "y")

    def __init__(self, x: float = 0.0, y: float = 0.0) -> None:
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
            return tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __repr__(self) -> str:
        return f"Point({self.x}, {self.y})"

    @property
    def abs_unit(self) -> "Point":
        n = math.hypot(self.x, self.y)
        return Point(self.x / n, self.y / n) if n else Point(0.0, 0.0)


class Rect:
    """An axis-aligned rectangle ``(x0, y0, x1, y1)`` (PyMuPDF ``fitz.Rect``).

    ``(x0, y0)`` is the top-left, ``(x1, y1)`` the bottom-right corner in
    PyMuPDF's top-left/y-down device space.
    """

    __slots__ = ("x0", "y0", "x1", "y1")

    def __init__(self, *args: float) -> None:
        if len(args) == 0:
            x0 = y0 = x1 = y1 = 0.0
        elif len(args) == 1:
            x0, y0, x1, y1 = args[0]
        elif len(args) == 4:
            x0, y0, x1, y1 = args
        else:
            raise ValueError("Rect takes 0, 1 (sequence) or 4 arguments")
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
            return tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __repr__(self) -> str:
        return f"Rect({self.x0}, {self.y0}, {self.x1}, {self.y1})"

    # --- PyMuPDF geometry surface ---
    @property
    def width(self) -> float:
        return abs(self.x1 - self.x0)

    @property
    def height(self) -> float:
        return abs(self.y1 - self.y0)

    @property
    def is_empty(self) -> bool:
        return self.x0 >= self.x1 or self.y0 >= self.y1

    @property
    def top_left(self) -> Point:
        return Point(self.x0, self.y0)

    tl = top_left

    @property
    def bottom_right(self) -> Point:
        return Point(self.x1, self.y1)

    br = bottom_right

    @property
    def tr(self) -> Point:
        return Point(self.x1, self.y0)

    @property
    def bl(self) -> Point:
        return Point(self.x0, self.y1)

    def round(self) -> "IRect":
        return IRect(
            math.floor(min(self.x0, self.x1)),
            math.floor(min(self.y0, self.y1)),
            math.ceil(max(self.x0, self.x1)),
            math.ceil(max(self.y0, self.y1)),
        )

    @property
    def irect(self) -> "IRect":
        return self.round()


class IRect:
    """An integer rectangle (PyMuPDF ``fitz.IRect``)."""

    __slots__ = ("x0", "y0", "x1", "y1")

    def __init__(self, *args: int) -> None:
        if len(args) == 0:
            x0 = y0 = x1 = y1 = 0
        elif len(args) == 1:
            x0, y0, x1, y1 = args[0]
        elif len(args) == 4:
            x0, y0, x1, y1 = args
        else:
            raise ValueError("IRect takes 0, 1 (sequence) or 4 arguments")
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
            return tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __repr__(self) -> str:
        return f"IRect({self.x0}, {self.y0}, {self.x1}, {self.y1})"

    @property
    def width(self) -> int:
        return abs(self.x1 - self.x0)

    @property
    def height(self) -> int:
        return abs(self.y1 - self.y0)

    @property
    def rect(self) -> Rect:
        return Rect(self.x0, self.y0, self.x1, self.y1)


class Matrix:
    """A 2-D affine matrix ``[a b c d e f]`` (PyMuPDF ``fitz.Matrix``).

    Maps a point ``(x, y)`` to ``(a*x + c*y + e, b*x + d*y + f)``.
    """

    __slots__ = ("a", "b", "c", "d", "e", "f")

    def __init__(self, *args: float) -> None:
        if len(args) == 0:
            vals = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
        elif len(args) == 1 and isinstance(args[0], (int, float)):
            # Matrix(degrees) → rotation matrix.
            return self._init_rotation(float(args[0]))
        elif len(args) == 1:
            vals = tuple(args[0])
        elif len(args) == 6:
            vals = tuple(float(v) for v in args)
        else:
            raise ValueError("Matrix takes 0, 1 (sequence/degrees) or 6 arguments")
        self.a, self.b, self.c, self.d, self.e, self.f = (float(v) for v in vals)

    def _init_rotation(self, degrees: float) -> None:
        # Bit-exact at cardinal angles (matches PyMuPDF / oxide_pdf COORD-ROT-*).
        m = degrees % 360.0
        if m == 0.0:
            vals = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
        elif m == 90.0:
            vals = (0.0, 1.0, -1.0, 0.0, 0.0, 0.0)
        elif m == 180.0:
            vals = (-1.0, 0.0, 0.0, -1.0, 0.0, 0.0)
        elif m == 270.0:
            vals = (0.0, -1.0, 1.0, 0.0, 0.0, 0.0)
        else:
            r = math.radians(m)
            s, co = math.sin(r), math.cos(r)
            vals = (co, s, -s, co, 0.0, 0.0)
        self.a, self.b, self.c, self.d, self.e, self.f = vals

    def __iter__(self) -> Iterator[float]:
        yield from (self.a, self.b, self.c, self.d, self.e, self.f)

    def __len__(self) -> int:
        return 6

    def __getitem__(self, i: int) -> float:
        return (self.a, self.b, self.c, self.d, self.e, self.f)[i]

    def __eq__(self, other: object) -> bool:
        try:
            return tuple(self) == tuple(other)  # type: ignore[arg-type]
        except TypeError:
            return NotImplemented

    def __repr__(self) -> str:
        return f"Matrix({self.a}, {self.b}, {self.c}, {self.d}, {self.e}, {self.f})"

    def __mul__(self, other: "Matrix") -> "Matrix":
        """Matrix product ``self * other`` (PyMuPDF ``concat`` order)."""
        return Matrix(
            self.a * other.a + self.b * other.c,
            self.a * other.b + self.b * other.d,
            self.c * other.a + self.d * other.c,
            self.c * other.b + self.d * other.d,
            self.e * other.a + self.f * other.c + other.e,
            self.e * other.b + self.f * other.d + other.f,
        )


class Quad:
    """A quadrilateral with corners ``ul, ur, ll, lr`` (PyMuPDF ``fitz.Quad``)."""

    __slots__ = ("ul", "ur", "ll", "lr")

    def __init__(self, ul: Point, ur: Point, ll: Point, lr: Point) -> None:
        self.ul = ul
        self.ur = ur
        self.ll = ll
        self.lr = lr

    def __iter__(self) -> Iterator[Point]:
        yield from (self.ul, self.ur, self.ll, self.lr)

    def __repr__(self) -> str:
        return f"Quad({self.ul}, {self.ur}, {self.ll}, {self.lr})"

    @property
    def rect(self) -> Rect:
        xs = [p.x for p in self]
        ys = [p.y for p in self]
        return Rect(min(xs), min(ys), max(xs), max(ys))
