//! SVG export facade (M7) — `Page::get_svg_image` renders a page to a
//! standalone SVG document string via [`pdf_render::get_svg_image`].
//!
//! The orphan rule forbids inherent `impl Page` here (`Page` is `pdf-core`'s),
//! so the entry point is the free function [`page_get_svg_image`].

use pdf_core::geom::Matrix;
use pdf_core::page::Page;

use pdf_render::{get_svg_image, SvgOptions};

use crate::error::Result;

/// Renders `page` to a standalone SVG document string (PyMuPDF
/// `Page.get_svg_image`). `matrix` is an extra page-space → device transform
/// applied on top of the page transform; pass [`Matrix::IDENTITY`] for 1:1.
///
/// # Errors
///
/// A typed [`Error`](crate::Error) from the render path.
pub fn page_get_svg_image(page: &Page, matrix: Matrix) -> Result<String> {
    let opts = SvgOptions { matrix };
    Ok(get_svg_image(page.document(), page, &opts)?)
}
