use std::ops::{Add, Div, Mul, Neg, Sub};

use super::Matrix;

/// A point in the plane, matching PyMuPDF `fitz.Point`.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// The origin `(0, 0)`.
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    /// Creates a point from coordinates.
    #[inline]
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    /// The Euclidean norm (distance from origin), i.e. `abs(p)` in PyMuPDF.
    #[inline]
    #[must_use]
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }

    /// Transforms this point by `m`:
    /// `(a*x + c*y + e, b*x + d*y + f)`. Equivalent to `point * matrix` in
    /// PyMuPDF.
    #[inline]
    #[must_use]
    pub fn transform(self, m: &Matrix) -> Point {
        Point {
            x: m.a * self.x + m.c * self.y + m.e,
            y: m.b * self.x + m.d * self.y + m.f,
        }
    }
}

impl From<(f64, f64)> for Point {
    #[inline]
    fn from((x, y): (f64, f64)) -> Self {
        Point { x, y }
    }
}

impl From<Point> for (f64, f64) {
    #[inline]
    fn from(p: Point) -> Self {
        (p.x, p.y)
    }
}

impl Add for Point {
    type Output = Point;
    #[inline]
    fn add(self, rhs: Point) -> Point {
        Point {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Point {
    type Output = Point;
    #[inline]
    fn sub(self, rhs: Point) -> Point {
        Point {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Neg for Point {
    type Output = Point;
    #[inline]
    fn neg(self) -> Point {
        Point {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl Mul<f64> for Point {
    type Output = Point;
    #[inline]
    fn mul(self, s: f64) -> Point {
        Point {
            x: self.x * s,
            y: self.y * s,
        }
    }
}

impl Div<f64> for Point {
    type Output = Point;
    #[inline]
    fn div(self, s: f64) -> Point {
        Point {
            x: self.x / s,
            y: self.y / s,
        }
    }
}

/// `point * matrix` — transform, matching PyMuPDF operator semantics.
impl Mul<Matrix> for Point {
    type Output = Point;
    #[inline]
    fn mul(self, m: Matrix) -> Point {
        self.transform(&m)
    }
}
