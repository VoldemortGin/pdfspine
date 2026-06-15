#![forbid(unsafe_code)]
//! `pdf-render` — the page rasterizer (page → [`pdf_image::Pixmap`]).
//!
//! Turns a parsed PDF page into pixels (PRD §8.11). The architecture keeps **all
//! PDF intelligence first-party** — content interpretation ([`pdf_text`]),
//! colorspaces, the CTM, and glyph-outline extraction ([`pdf_fonts`] +
//! `ttf-parser`) — and uses the [`tiny_skia`] crate only as the leaf
//! anti-aliased fill/stroke/clip primitive that paints the resulting paths into
//! a pixel buffer.
//!
//! # `#![forbid(unsafe_code)]`
//!
//! First-party code is 100% safe. The rasterizer dependency may use `unsafe`
//! internally (SIMD) — that is a dependency, not first-party code.
//!
//! # Module ownership (frozen for parallel M6a/b/c/d — see `ARCHITECTURE.md`)
//!
//! - [`canvas`] — the raster target wrapping a tiny-skia pixmap (M6a).
//! - [`vector`] — fill/stroke/clip of geometry paths (M6a).
//! - [`text`]   — glyph + text-run rendering (M6b).
//! - [`image`]  — composite a decoded image under a CTM (M6c).
//! - [`render`] — the `render_page` entry point + [`DisplayList`] that drive the
//!   others by replaying the interpreter's ordered render-op stream (M6d).
//! - [`error`]  — the crate [`Error`]/[`Result`] types (shared).
//!
//! There is no `todo!()`/`unimplemented!()`/`panic!` anywhere — arbitrary input
//! never panics and is honored under `Limits` (PRD §8.1 / §9.6.2). Unsupported
//! constructs (Type1/Type3 glyphs, mesh shadings, tiling patterns) degrade to a
//! safe no-op rather than an error, so a page always renders what it can.

pub mod canvas;
pub mod error;
pub mod image;
pub mod render;
pub mod svg;
pub mod text;
pub mod vector;

pub use canvas::Canvas;
pub use error::{Error, Result};
pub use render::{render_page, DisplayList, RenderOptions};
pub use svg::{get_svg_image, SvgOptions};
