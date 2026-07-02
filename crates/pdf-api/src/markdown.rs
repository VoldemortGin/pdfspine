//! Markdown → PDF facade — the pdfspine-original `markdown_to_pdf` extension
//! (PRD-NEXT §9). A thin wrapper over [`pdf_markdown`] so the PyO3 layer keeps
//! depending only on `pdf-api`. This is **not** part of the fitz-compat
//! surface (no COMPAT.toml entry): it is a self-authored authoring API,
//! orthogonal to the pdf→md extraction path.

use crate::error::{Error, Result};

// Re-export the options struct so the bindings depend only on `pdf-api`.
pub use pdf_markdown::Options;

/// Renders `markdown` (CommonMark + GFM tables / strikethrough / task lists)
/// to PDF bytes under `options` (page geometry, margins, body size, optional
/// user/CJK-fallback TTFs, image base dir). Deterministic; images load from
/// local paths / `data:` URIs only (never the network).
///
/// # Errors
///
/// - [`Error::Unsupported`] for unusable geometry options, bad image data /
///   unresolvable image paths, remote image URLs, or unparseable font
///   programs — a bad *input*, not a syntax fault (the same policy as the
///   image path's `InvalidArgument` mapping in `error.rs`).
/// - Any propagated `pdf-core` write error.
pub fn markdown_to_pdf(markdown: &str, options: &Options) -> Result<Vec<u8>> {
    pdf_markdown::markdown_to_pdf(markdown, options).map_err(|e| match e {
        pdf_core::Error::InvalidArgument(m) => Error::Unsupported(m.to_string()),
        other => Error::from(other),
    })
}
