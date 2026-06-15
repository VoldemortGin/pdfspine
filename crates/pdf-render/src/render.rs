//! The page render entry point — `render_page` (M6d).
//!
//! Module owner: **M6d** (full-page orchestration + DisplayList + wiring). Drives
//! the interpreter ([`pdf_text`]) and dispatches each drawcall to
//! [`crate::vector`] / [`crate::text`] / [`crate::image`] onto a [`Canvas`], then
//! converts to a [`Pixmap`]. The signature + [`RenderOptions`] shape below are
//! the frozen contract; M6d fills the body. See `ARCHITECTURE.md`.

use pdf_core::geom::{IRect, Matrix};
use pdf_core::{DocumentStore, Page};
use pdf_image::pixmap::{Colorspace, Pixmap};

use crate::error::{Error, Result};

/// Options controlling a page render (PyMuPDF `Page.get_pixmap` parameters,
/// PRD §8.11).
///
/// `matrix` and `dpi` are alternative ways to set the scale: when `dpi` is
/// `Some`, it derives a uniform `dpi/72` scale (matching PyMuPDF, which ignores
/// `matrix` when `dpi` is given); otherwise `matrix` is used.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// The page user-space → device transform (scale/rotate). Defaults to the
    /// identity (1 pt = 1 px). Ignored when `dpi` is `Some`.
    pub matrix: Matrix,
    /// Optional render resolution; when set, overrides `matrix` with a uniform
    /// `dpi/72` scale.
    pub dpi: Option<u32>,
    /// Output colorspace (PyMuPDF `colorspace`). Defaults to RGB.
    pub colorspace: Colorspace,
    /// Whether the output pixmap carries an alpha channel.
    pub alpha: bool,
    /// Optional device-space clip rect; only this sub-rectangle is rendered
    /// (PyMuPDF `clip`). `None` renders the full transformed page bound.
    pub clip: Option<IRect>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            matrix: Matrix::IDENTITY,
            dpi: None,
            colorspace: Colorspace::Rgb,
            alpha: false,
            clip: None,
        }
    }
}

/// Renders `page` of `doc` to a [`Pixmap`] under `opts` (PRD §8.11).
///
/// This is the crate's top-level entry point: it computes the device transform
/// and target geometry, builds a [`crate::Canvas`], runs the content
/// interpreter, dispatches each drawcall to the vector/text/image modules, and
/// converts the canvas to the requested output format.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] for the M6 scaffold (no rendering is performed
/// yet). The real implementation surfaces [`Error::InvalidArgument`] /
/// [`Error::LimitExceeded`] for bad geometry and propagates `pdf-core` /
/// `pdf-image` errors.
pub fn render_page(doc: &DocumentStore, page: &Page, opts: &RenderOptions) -> Result<Pixmap> {
    let _ = (doc, page, opts);
    Err(Error::Unsupported("render_page"))
}
