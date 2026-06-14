use std::ops::Mul;

use super::Point;

/// A 2-D affine transform `[a, b, c, d, e, f]`, matching PyMuPDF `fitz.Matrix`.
///
/// Maps `(x, y)` to `(a*x + c*y + e, b*x + d*y + f)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    /// The identity matrix `[1, 0, 0, 1, 0, 0]`.
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Creates a matrix from its six components.
    #[inline]
    #[must_use]
    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Matrix { a, b, c, d, e, f }
    }

    /// The identity matrix (same as [`Matrix::IDENTITY`]).
    #[inline]
    #[must_use]
    pub const fn identity() -> Self {
        Matrix::IDENTITY
    }

    /// A scaling matrix `[sx, 0, 0, sy, 0, 0]`.
    #[inline]
    #[must_use]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Matrix {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }

    /// A translation matrix `[1, 0, 0, 1, tx, ty]`.
    #[inline]
    #[must_use]
    pub const fn translate(tx: f64, ty: f64) -> Self {
        Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    /// A shear matrix `[1, h, v, 1, 0, 0]`, matching `fitz.Matrix(1, h, v, 1)`.
    #[inline]
    #[must_use]
    pub const fn shear(h: f64, v: f64) -> Self {
        Matrix {
            a: 1.0,
            b: h,
            c: v,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// A counter-clockwise rotation by `deg` degrees about the origin.
    ///
    /// The identity matrix becomes
    /// `[cos(deg), sin(deg), -sin(deg), cos(deg), 0, 0]` (PyMuPDF
    /// `Matrix(deg)` / `prerotate`). The cardinal angles 0/90/180/270 (and
    /// their multiples and negatives) are **special-cased to be bit-exact** so
    /// the zero/one entries carry no float drift — a requirement of the
    /// `COORD-ROT-*` contract.
    #[inline]
    #[must_use]
    pub fn rotate(deg: f64) -> Self {
        // Normalize to [0, 360) for the cardinal fast-path. Using rem_euclid
        // keeps negatives and large multiples exact for the cardinal cases.
        let norm = deg.rem_euclid(360.0);
        if norm == 0.0 {
            Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
        } else if norm == 90.0 {
            Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0)
        } else if norm == 180.0 {
            Matrix::new(-1.0, 0.0, 0.0, -1.0, 0.0, 0.0)
        } else if norm == 270.0 {
            Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0)
        } else {
            let r = norm.to_radians();
            let (s, c) = r.sin_cos();
            Matrix::new(c, s, -s, c, 0.0, 0.0)
        }
    }

    /// The matrix product `m1 * m2` (PyMuPDF `Matrix.concat`).
    ///
    /// Matrix multiplication is not commutative; `m1` is the left operand.
    /// The result applies `m1` first, then `m2`, when transforming a point as
    /// `p * (m1 * m2)`.
    #[inline]
    #[must_use]
    pub fn concat(m1: &Matrix, m2: &Matrix) -> Matrix {
        Matrix {
            a: m1.a * m2.a + m1.b * m2.c,
            b: m1.a * m2.b + m1.b * m2.d,
            c: m1.c * m2.a + m1.d * m2.c,
            d: m1.c * m2.b + m1.d * m2.d,
            e: m1.e * m2.a + m1.f * m2.c + m2.e,
            f: m1.e * m2.b + m1.f * m2.d + m2.f,
        }
    }

    /// The determinant `a*d - b*c`.
    #[inline]
    #[must_use]
    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// Whether the matrix is invertible (non-zero determinant).
    #[inline]
    #[must_use]
    pub fn is_invertible(&self) -> bool {
        self.determinant() != 0.0
    }

    /// The inverse matrix, or `None` if singular (determinant 0).
    ///
    /// PyMuPDF returns the zero/degenerate matrix for non-invertible input;
    /// we return `None` so the caller chooses the policy.
    #[inline]
    #[must_use]
    pub fn invert(&self) -> Option<Matrix> {
        let det = self.determinant();
        if det == 0.0 {
            return None;
        }
        let inv = 1.0 / det;
        let a = self.d * inv;
        let b = -self.b * inv;
        let c = -self.c * inv;
        let d = self.a * inv;
        // Translation of the inverse: -(e, f) mapped through the inverse linear
        // part.
        let e = -(self.e * a + self.f * c);
        let f = -(self.e * b + self.f * d);
        Some(Matrix { a, b, c, d, e, f })
    }

    /// Pre-multiplies a scale: `self = scale(sx, sy) * self`.
    #[inline]
    #[must_use]
    pub fn prescale(&self, sx: f64, sy: f64) -> Matrix {
        Matrix::concat(&Matrix::scale(sx, sy), self)
    }

    /// Pre-multiplies a translation: `self = translate(tx, ty) * self`.
    #[inline]
    #[must_use]
    pub fn pretranslate(&self, tx: f64, ty: f64) -> Matrix {
        Matrix::concat(&Matrix::translate(tx, ty), self)
    }

    /// Pre-multiplies a rotation: `self = rotate(deg) * self`.
    #[inline]
    #[must_use]
    pub fn prerotate(&self, deg: f64) -> Matrix {
        Matrix::concat(&Matrix::rotate(deg), self)
    }

    /// Transforms a point by this matrix (`Point::transform`).
    #[inline]
    #[must_use]
    pub fn transform_point(&self, p: Point) -> Point {
        p.transform(self)
    }
}

impl Default for Matrix {
    #[inline]
    fn default() -> Self {
        Matrix::IDENTITY
    }
}

/// `m1 * m2` — matrix concatenation, matching PyMuPDF.
impl Mul for Matrix {
    type Output = Matrix;
    #[inline]
    fn mul(self, rhs: Matrix) -> Matrix {
        Matrix::concat(&self, &rhs)
    }
}
