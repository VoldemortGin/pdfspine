//! PyMuPDF-compatible geometry value types.
//!
//! Implements [`Matrix`], [`Point`], [`Rect`], [`IRect`] and [`Quad`] with
//! arithmetic semantics matching PyMuPDF (`fitz`) exactly. PyMuPDF arithmetic
//! is a Tier-A documented contract (PRD §9.5); the rotation matrices for the
//! cardinal angles are bit-exact (no float drift in the zero entries).
//!
//! Coordinate conventions (PDF / PyMuPDF):
//! - A `Matrix{a,b,c,d,e,f}` maps a point `(x, y)` to
//!   `(a*x + c*y + e, b*x + d*y + f)`.
//! - `Matrix::concat(m1, m2)` is the matrix product `m1 * m2`.
//! - `Rect` `|` is union (smallest enclosing); `&` is intersection (largest
//!   enclosed).
//! - A `Quad` has corners `ul, ur, ll, lr` (upper-left, upper-right,
//!   lower-left, lower-right).

mod irect;
mod matrix;
mod paper;
mod point;
mod quad;
mod rect;

pub use irect::IRect;
pub use matrix::Matrix;
pub use paper::{paper_rect, paper_size, paper_sizes, PaperOrientation};
pub use point::Point;
pub use quad::Quad;
pub use rect::{Rect, EMPTY_RECT, INFINITE_RECT};

/// Absolute tolerance used by approximate-equality helpers in tests and by
/// callers comparing geometry results. Public so downstream property tests can
/// reuse the same epsilon.
pub const EPSILON: f64 = 1e-9;
