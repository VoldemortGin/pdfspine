use std::ops::{BitAnd, BitOr};

use super::{Point, Rect};

/// An axis-aligned rectangle with integer coordinates, matching PyMuPDF
/// `fitz.IRect`. `(x0, y0)` is the top-left, `(x1, y1)` the bottom-right.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    /// Creates an integer rectangle from its four edges (not normalized).
    #[inline]
    #[must_use]
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        IRect { x0, y0, x1, y1 }
    }

    /// The width (absolute), matching `Rect` semantics.
    #[inline]
    #[must_use]
    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).abs()
    }

    /// The height (absolute).
    #[inline]
    #[must_use]
    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).abs()
    }

    /// The area `width * height` (returned as `i64` to avoid overflow on large
    /// boxes).
    #[inline]
    #[must_use]
    pub fn area(&self) -> i64 {
        i64::from(self.width()) * i64::from(self.height())
    }

    /// The top-left corner `(x0, y0)`.
    #[inline]
    #[must_use]
    pub fn top_left(&self) -> Point {
        Point::new(f64::from(self.x0), f64::from(self.y0))
    }

    /// The bottom-right corner `(x1, y1)`.
    #[inline]
    #[must_use]
    pub fn bottom_right(&self) -> Point {
        Point::new(f64::from(self.x1), f64::from(self.y1))
    }

    /// Returns a normalized copy with `x0 <= x1` and `y0 <= y1`.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> IRect {
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
        IRect { x0, y0, x1, y1 }
    }

    /// Whether the rectangle is empty (`x0 >= x1` or `y0 >= y1`).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    /// Whether this rectangle contains a point (inclusive).
    #[inline]
    #[must_use]
    pub fn contains_point(&self, p: Point) -> bool {
        self.rect().contains_point(p)
    }

    /// Whether this rectangle fully contains another integer rectangle.
    #[inline]
    #[must_use]
    pub fn contains_rect(&self, other: &IRect) -> bool {
        self.rect().contains_rect(&other.rect())
    }

    /// The union (smallest enclosing rectangle), following PyMuPDF
    /// `include_rect` argument-first order (see [`Rect::union`]).
    #[inline]
    #[must_use]
    pub fn union(&self, other: &IRect) -> IRect {
        if other.is_empty() {
            return self.normalize();
        }
        if self.is_empty() {
            return other.normalize();
        }
        let a = self.normalize();
        let b = other.normalize();
        IRect {
            x0: a.x0.min(b.x0),
            y0: a.y0.min(b.y0),
            x1: a.x1.max(b.x1),
            y1: a.y1.max(b.y1),
        }
    }

    /// The intersection (largest enclosed rectangle); empty when disjoint.
    #[inline]
    #[must_use]
    pub fn intersect(&self, other: &IRect) -> IRect {
        let a = self.normalize();
        let b = other.normalize();
        let x0 = a.x0.max(b.x0);
        let y0 = a.y0.max(b.y0);
        let x1 = a.x1.min(b.x1);
        let y1 = a.y1.min(b.y1);
        if x0 > x1 || y0 > y1 {
            IRect::default()
        } else {
            IRect { x0, y0, x1, y1 }
        }
    }

    /// The [`Rect`] equivalent (PyMuPDF `IRect.rect`).
    #[inline]
    #[must_use]
    pub fn rect(&self) -> Rect {
        Rect::from(*self)
    }
}

impl From<Rect> for IRect {
    /// Rounds outward (`Rect::round`).
    #[inline]
    fn from(r: Rect) -> Self {
        r.round()
    }
}

impl From<(i32, i32, i32, i32)> for IRect {
    #[inline]
    fn from((x0, y0, x1, y1): (i32, i32, i32, i32)) -> Self {
        IRect { x0, y0, x1, y1 }
    }
}

/// `r1 | r2` — union.
impl BitOr for IRect {
    type Output = IRect;
    #[inline]
    fn bitor(self, rhs: IRect) -> IRect {
        self.union(&rhs)
    }
}

/// `r1 & r2` — intersection.
impl BitAnd for IRect {
    type Output = IRect;
    #[inline]
    fn bitand(self, rhs: IRect) -> IRect {
        self.intersect(&rhs)
    }
}
