use std::ops::{BitAnd, BitOr};

use super::{IRect, Matrix, Point, Quad};

/// An axis-aligned rectangle, matching PyMuPDF `fitz.Rect`.
///
/// `(x0, y0)` is the top-left and `(x1, y1)` the bottom-right corner of a
/// finite, normalized rectangle.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// The empty rectangle `(0, 0, 0, 0)` (PyMuPDF `EMPTY_RECT()`).
pub const EMPTY_RECT: Rect = Rect {
    x0: 0.0,
    y0: 0.0,
    x1: 0.0,
    y1: 0.0,
};

/// The infinite rectangle (PyMuPDF `INFINITE_RECT()` =
/// `Rect(-2^31 + 1, -2^31 + 1, 2^31 - 1, 2^31 - 1)`).
pub const INFINITE_RECT: Rect = Rect {
    x0: -2_147_483_647.0,
    y0: -2_147_483_647.0,
    x1: 2_147_483_647.0,
    y1: 2_147_483_647.0,
};

impl Rect {
    /// Creates a rectangle from its four edges (not normalized).
    #[inline]
    #[must_use]
    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect { x0, y0, x1, y1 }
    }

    /// The width `x1 - x0` (of the normalized rectangle, since width uses the
    /// signed difference like PyMuPDF). For a normalized rect this is `>= 0`.
    #[inline]
    #[must_use]
    pub fn width(&self) -> f64 {
        (self.x1 - self.x0).abs()
    }

    /// The height `y1 - y0`. For a normalized rect this is `>= 0`.
    #[inline]
    #[must_use]
    pub fn height(&self) -> f64 {
        (self.y1 - self.y0).abs()
    }

    /// The area `width * height`.
    #[inline]
    #[must_use]
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// The top-left corner `(x0, y0)` (PyMuPDF `tl`).
    #[inline]
    #[must_use]
    pub fn top_left(&self) -> Point {
        Point::new(self.x0, self.y0)
    }

    /// The top-right corner `(x1, y0)` (PyMuPDF `tr`).
    #[inline]
    #[must_use]
    pub fn top_right(&self) -> Point {
        Point::new(self.x1, self.y0)
    }

    /// The bottom-left corner `(x0, y1)` (PyMuPDF `bl`).
    #[inline]
    #[must_use]
    pub fn bottom_left(&self) -> Point {
        Point::new(self.x0, self.y1)
    }

    /// The bottom-right corner `(x1, y1)` (PyMuPDF `br`).
    #[inline]
    #[must_use]
    pub fn bottom_right(&self) -> Point {
        Point::new(self.x1, self.y1)
    }

    /// Returns a normalized copy with `x0 <= x1` and `y0 <= y1`.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> Rect {
        let (x0, x1) = if self.x0 <= self.x1 {
            (self.x0, self.x1)
        } else {
            (self.x1, self.x0)
        };
        let (y0, y1) = if self.y0 <= self.y1 {
            (self.y0, self.y1)
        } else {
            (self.y1, self.y0)
        };
        Rect { x0, y0, x1, y1 }
    }

    /// Whether the rectangle is empty (zero area), i.e. `x0 >= x1` or
    /// `y0 >= y1`. Matches PyMuPDF `is_empty`.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    /// Whether the rectangle is the infinite rectangle. Matches PyMuPDF
    /// `is_infinite`.
    #[inline]
    #[must_use]
    pub fn is_infinite(&self) -> bool {
        *self == INFINITE_RECT
    }

    /// Whether this rectangle is valid (normalized: `x0 <= x1`, `y0 <= y1`).
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.x0 <= self.x1 && self.y0 <= self.y1
    }

    /// Whether this rectangle contains a point (inclusive on all edges).
    #[inline]
    #[must_use]
    pub fn contains_point(&self, p: Point) -> bool {
        let r = self.normalize();
        p.x >= r.x0 && p.x <= r.x1 && p.y >= r.y0 && p.y <= r.y1
    }

    /// Whether this rectangle fully contains another rectangle.
    ///
    /// An empty rectangle is contained in any rectangle (matching PyMuPDF,
    /// where `Rect.contains(empty)` is true).
    #[inline]
    #[must_use]
    pub fn contains_rect(&self, other: &Rect) -> bool {
        if other.is_empty() {
            return true;
        }
        let a = self.normalize();
        let b = other.normalize();
        a.x0 <= b.x0 && a.y0 <= b.y0 && a.x1 >= b.x1 && a.y1 >= b.y1
    }

    /// The smallest rectangle containing both `self` and `other`
    /// (PyMuPDF `|` / `include_rect`).
    ///
    /// Follows PyMuPDF `include_rect` order exactly: if `other` (the argument)
    /// is empty, `self` is returned unchanged; else if `self` is empty, `other`
    /// is returned. Consequently — like PyMuPDF — union is **not** commutative
    /// when *both* operands are empty.
    #[inline]
    #[must_use]
    pub fn union(&self, other: &Rect) -> Rect {
        if other.is_empty() {
            return self.normalize();
        }
        if self.is_empty() {
            return other.normalize();
        }
        let a = self.normalize();
        let b = other.normalize();
        Rect {
            x0: a.x0.min(b.x0),
            y0: a.y0.min(b.y0),
            x1: a.x1.max(b.x1),
            y1: a.y1.max(b.y1),
        }
    }

    /// The largest rectangle contained in both `self` and `other`
    /// (PyMuPDF `&` / `intersect`). Returns an empty rectangle when they do
    /// not overlap.
    #[inline]
    #[must_use]
    pub fn intersect(&self, other: &Rect) -> Rect {
        let a = self.normalize();
        let b = other.normalize();
        let x0 = a.x0.max(b.x0);
        let y0 = a.y0.max(b.y0);
        let x1 = a.x1.min(b.x1);
        let y1 = a.y1.min(b.y1);
        if x0 > x1 || y0 > y1 {
            EMPTY_RECT
        } else {
            Rect { x0, y0, x1, y1 }
        }
    }

    /// Whether this rectangle has a non-empty intersection with `other`.
    #[inline]
    #[must_use]
    pub fn intersects(&self, other: &Rect) -> bool {
        !self.intersect(other).is_empty()
    }

    /// Transforms the rectangle by a matrix and returns the axis-aligned
    /// bounding box of the transformed corners (PyMuPDF `Rect * matrix`).
    #[inline]
    #[must_use]
    pub fn transform(&self, m: &Matrix) -> Rect {
        self.quad().transform(m).rect()
    }

    /// The quad of this rectangle's four corners (PyMuPDF `Rect.quad`).
    #[inline]
    #[must_use]
    pub fn quad(&self) -> Quad {
        Quad::from_rect(self)
    }

    /// Rounds outward to the smallest [`IRect`] containing this rectangle
    /// (PyMuPDF `Rect.round`/`irect`): `x0`/`y0` floor, `x1`/`y1` ceil.
    #[inline]
    #[must_use]
    pub fn round(&self) -> IRect {
        let r = self.normalize();
        IRect {
            x0: r.x0.floor() as i32,
            y0: r.y0.floor() as i32,
            x1: r.x1.ceil() as i32,
            y1: r.y1.ceil() as i32,
        }
    }
}

impl From<IRect> for Rect {
    #[inline]
    fn from(r: IRect) -> Self {
        Rect {
            x0: f64::from(r.x0),
            y0: f64::from(r.y0),
            x1: f64::from(r.x1),
            y1: f64::from(r.y1),
        }
    }
}

impl From<(f64, f64, f64, f64)> for Rect {
    #[inline]
    fn from((x0, y0, x1, y1): (f64, f64, f64, f64)) -> Self {
        Rect { x0, y0, x1, y1 }
    }
}

/// `r1 | r2` — union (smallest enclosing rectangle).
impl BitOr for Rect {
    type Output = Rect;
    #[inline]
    fn bitor(self, rhs: Rect) -> Rect {
        self.union(&rhs)
    }
}

/// `r1 & r2` — intersection (largest enclosed rectangle).
impl BitAnd for Rect {
    type Output = Rect;
    #[inline]
    fn bitand(self, rhs: Rect) -> Rect {
        self.intersect(&rhs)
    }
}
