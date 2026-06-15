//! Vector path rendering — fill / stroke / clip (M6a).
//!
//! Module owner: **M6a** (vector + canvas). Translates a [`pdf_text::DrawPath`]
//! (constructed path in PDF user space, with paint metadata) into rasterizer
//! drawcalls on a [`Canvas`]. The signatures below are frozen; M6a fills the
//! bodies. See `ARCHITECTURE.md`.

use pdf_core::geom::Matrix;
use pdf_text::model::{DrawPath, PathItem};

use crate::canvas::Canvas;
use crate::error::{Error, Result};

/// An RGBA fill/stroke color (un-premultiplied sRGB 0–255), the first-party
/// paint type fed to the rasterizer. Decoupled from the rasterizer's own color
/// type so colorspace conversion stays first-party.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    /// Red, green, blue, alpha (un-premultiplied sRGB, 0–255).
    pub rgba: [u8; 4],
}

impl Paint {
    /// Builds an opaque paint from a packed `0x00RRGGBB` sRGB color (the form
    /// produced by the interpreter for fill/stroke colors).
    #[must_use]
    pub fn from_rgb(rgb: u32) -> Self {
        Self {
            rgba: [
                ((rgb >> 16) & 0xFF) as u8,
                ((rgb >> 8) & 0xFF) as u8,
                (rgb & 0xFF) as u8,
                0xFF,
            ],
        }
    }
}

/// Stroke parameters (line width, joins, caps, dashes) in device space.
///
/// A thin first-party mirror of the rasterizer's stroke options so the public
/// stub surface does not leak the dependency type. Fields will grow (joins/caps/
/// dash phase) as M6a needs them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StrokeStyle {
    /// Stroke width in device pixels (already CTM-scaled by the caller).
    pub width: f32,
}

/// Fills `path` (PDF user space) into `canvas` with `paint`, composing `ctm`
/// onto the canvas base transform. `even_odd` selects the even-odd vs nonzero
/// winding fill rule.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] until M6a implements path translation +
/// `Pixmap::fill_path`.
pub fn fill_path(
    canvas: &mut Canvas,
    path: &DrawPath,
    paint: Paint,
    ctm: Matrix,
    even_odd: bool,
) -> Result<()> {
    let _ = (canvas.pixmap_mut(), path, paint, ctm, even_odd);
    Err(Error::Unsupported("vector::fill_path"))
}

/// Strokes `path` (PDF user space) into `canvas` with `paint`/`style`, composing
/// `ctm` onto the canvas base transform.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] until M6a implements path translation +
/// `Pixmap::stroke_path`.
pub fn stroke_path(
    canvas: &mut Canvas,
    path: &DrawPath,
    paint: Paint,
    style: &StrokeStyle,
    ctm: Matrix,
) -> Result<()> {
    let _ = (canvas.pixmap_mut(), path, paint, style, ctm);
    Err(Error::Unsupported("vector::stroke_path"))
}

/// Intersects the current clip region with `path` (PDF user space) under `ctm`.
///
/// The clip is a rasterizer mask the subsequent draws are restricted to (PDF
/// `W`/`W*` followed by a path-painting operator). `even_odd` selects the clip
/// fill rule.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] until M6a implements clip-mask intersection.
pub fn set_clip(
    canvas: &mut Canvas,
    items: &[PathItem],
    ctm: Matrix,
    even_odd: bool,
) -> Result<()> {
    let _ = (canvas.pixmap_mut(), items, ctm, even_odd);
    Err(Error::Unsupported("vector::set_clip"))
}
