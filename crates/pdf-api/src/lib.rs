#![forbid(unsafe_code)]
//! `pdf-api` — the unified ergonomic facade over the oxipdf core crates and the
//! only crate `py-bindings` depends on (PRD §9.1).
//!
//! In M0 it re-exports the geometry value types from [`pdf_core::geom`]. The
//! document/page/text/image surface lands in M1+.

/// The crate version string (the workspace version), surfaced to Python as
/// `oxipdf.__version__`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Geometry value types, re-exported from `pdf-core` (PRD §7 / M0).
pub mod geom {
    pub use pdf_core::geom::*;
}

// Convenience flat re-exports of the most-used geometry types.
pub use pdf_core::geom::{IRect, Matrix, Point, Quad, Rect};
