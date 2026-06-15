//! Text rendering — glyph outlines → filled paths (M6b).
//!
//! Module owner: **M6b** (text). Resolves each [`pdf_text::PositionedGlyph`] to a
//! font program, extracts its outline via the existing `ttf-parser`
//! `OutlineBuilder` (through [`pdf_fonts`]), and fills it on the [`Canvas`] —
//! NOT a separate glyph-raster crate (PRD §8.11). The signatures below are
//! frozen; M6b fills the bodies. See `ARCHITECTURE.md`.

use pdf_core::geom::Matrix;
use pdf_text::PositionedGlyph;

use crate::canvas::Canvas;
use crate::error::{Error, Result};
use crate::vector::Paint;

/// Renders a single positioned glyph onto `canvas`.
///
/// The glyph's outline (font-unit space) is mapped by its text matrix and the
/// device `ctm`, then filled with `paint`. Invisible glyphs (render mode 3) and
/// glyphs whose outline cannot be resolved are skipped without error by the
/// implementation.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] until M6b implements outline extraction + fill.
pub fn draw_glyph(
    canvas: &mut Canvas,
    glyph: &PositionedGlyph,
    paint: Paint,
    ctm: Matrix,
) -> Result<()> {
    let _ = (canvas.pixmap_mut(), glyph, paint, ctm);
    Err(Error::Unsupported("text::draw_glyph"))
}

/// Renders a run of positioned glyphs that share a font + size onto `canvas`.
///
/// A batched form of [`draw_glyph`] (the common case: a show-text operator emits
/// a contiguous run). The implementation resolves the font program once and
/// fills each glyph's outline.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] until M6b implements the run path.
pub fn draw_text_run(
    canvas: &mut Canvas,
    glyphs: &[PositionedGlyph],
    paint: Paint,
    ctm: Matrix,
) -> Result<()> {
    let _ = (canvas.pixmap_mut(), glyphs, paint, ctm);
    Err(Error::Unsupported("text::draw_text_run"))
}
