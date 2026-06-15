//! Image rendering — composite a decoded image under a CTM (M6c).
//!
//! Module owner: **M6c** (image + shading/pattern). Maps a decoded
//! [`pdf_image::pixmap::Pixmap`] (the unit square `[0,1]²` in image space) onto
//! the page via the placement matrix and paints it on the [`Canvas`]. The
//! signature below is frozen; M6c fills the body. See `ARCHITECTURE.md`.

use pdf_core::geom::Matrix;
use pdf_image::pixmap::Pixmap;

use crate::canvas::Canvas;
use crate::error::{Error, Result};

/// Composites the decoded `image` onto `canvas`.
///
/// `ctm` is the image-placement matrix (the unit square is mapped by `ctm` onto
/// the page, per [`pdf_text::ImageRef::ctm`]); the implementation composes it
/// with the canvas base transform, samples the source pixmap, and blends with
/// `alpha` (0–255, an extra constant-alpha factor, e.g. from a soft mask or a
/// transparency group). The source pixmap's own colorspace/`/SMask` are honored.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] until M6c implements sampling + compositing.
pub fn draw_image(canvas: &mut Canvas, image: &Pixmap, ctm: Matrix, alpha: u8) -> Result<()> {
    let _ = (canvas.pixmap_mut(), image, ctm, alpha);
    Err(Error::Unsupported("image::draw_image"))
}
