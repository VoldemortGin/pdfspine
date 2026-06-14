use super::{Matrix, Point, Rect};

/// A quadrilateral with four corners, matching PyMuPDF `fitz.Quad`.
///
/// Corner order is `ul, ur, ll, lr` (upper-left, upper-right, lower-left,
/// lower-right).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Quad {
    pub ul: Point,
    pub ur: Point,
    pub ll: Point,
    pub lr: Point,
}

impl Quad {
    /// Creates a quad from its four corners.
    #[inline]
    #[must_use]
    pub const fn new(ul: Point, ur: Point, ll: Point, lr: Point) -> Self {
        Quad { ul, ur, ll, lr }
    }

    /// Builds the axis-aligned quad from a rectangle's corners (PyMuPDF
    /// `Rect.quad`). The rectangle is normalized first.
    #[inline]
    #[must_use]
    pub fn from_rect(r: &Rect) -> Quad {
        let n = r.normalize();
        Quad {
            ul: Point::new(n.x0, n.y0),
            ur: Point::new(n.x1, n.y0),
            ll: Point::new(n.x0, n.y1),
            lr: Point::new(n.x1, n.y1),
        }
    }

    /// The smallest rectangle containing all four corners (PyMuPDF
    /// `Quad.rect`).
    #[inline]
    #[must_use]
    pub fn rect(&self) -> Rect {
        let xs = [self.ul.x, self.ur.x, self.ll.x, self.lr.x];
        let ys = [self.ul.y, self.ur.y, self.ll.y, self.lr.y];
        let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let x1 = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let y1 = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Rect { x0, y0, x1, y1 }
    }

    /// Transforms every corner by a matrix (PyMuPDF `Quad.transform`).
    #[inline]
    #[must_use]
    pub fn transform(&self, m: &Matrix) -> Quad {
        Quad {
            ul: self.ul.transform(m),
            ur: self.ur.transform(m),
            ll: self.ll.transform(m),
            lr: self.lr.transform(m),
        }
    }

    /// Whether the enclosed area is (approximately) zero, i.e. at least three
    /// corners are collinear (PyMuPDF `Quad.is_empty`).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        // Twice the signed area of the polygon ul -> ur -> lr -> ll (shoelace).
        let area2 = cross(self.ul, self.ur, self.lr) + cross(self.ul, self.lr, self.ll);
        area2.abs() <= super::EPSILON
    }

    /// The maximum length of the top and bottom sides (PyMuPDF `Quad.width`).
    #[inline]
    #[must_use]
    pub fn width(&self) -> f64 {
        let top = (self.ul - self.ur).norm();
        let bottom = (self.ll - self.lr).norm();
        top.max(bottom)
    }

    /// The maximum length of the left and right sides (PyMuPDF `Quad.height`).
    #[inline]
    #[must_use]
    pub fn height(&self) -> f64 {
        let left = (self.ul - self.ll).norm();
        let right = (self.ur - self.lr).norm();
        left.max(right)
    }
}

/// Twice the signed area of triangle `(a, b, c)` (z-component of the cross
/// product of `b - a` and `c - a`).
#[inline]
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

impl From<Rect> for Quad {
    #[inline]
    fn from(r: Rect) -> Self {
        Quad::from_rect(&r)
    }
}
