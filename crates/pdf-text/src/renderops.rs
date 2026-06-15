//! Ordered render-op stream — the document-order drawcall list (M6d, PRD §8.11).
//!
//! The M2 text path consumes the flat [`InterpretResult`](crate::model::InterpretResult)
//! (glyphs / images / drawings grouped by *kind*, which loses the z-order
//! between a fill and the text painted over it). Page **rendering** needs the
//! drawcalls in **document order** so later ops paint over earlier ones.
//!
//! Rather than fork the interpreter, [`ContentInterpreter`](crate::ContentInterpreter)
//! gains an **opt-in ordered sink**: when a [`RenderSink`] is attached it records
//! a [`RenderOp`] for every paint / clip / state operator, interleaved exactly as
//! they appear in the content stream (forms recurse inline). The text-extraction
//! path is unchanged — it attaches no sink, so M2 stays byte-for-byte identical.
//!
//! Each op is self-contained in **PDF user space** (the CTM is already applied to
//! geometry, or carried explicitly for images / shadings, matching the existing
//! [`DrawPath`]/[`ImageRef`] convention). `render_page` replays this list onto a
//! `Canvas`; the same list is the PyMuPDF `DisplayList` record.

use pdf_core::geom::Matrix;
use pdf_core::{Dict, Object};

use crate::model::{PathItem, PositionedGlyph};

/// One ordered drawcall (or state change) captured from the content stream.
///
/// Geometry in [`RenderOp::Fill`] / [`RenderOp::Stroke`] / [`RenderOp::Clip`] is
/// in PDF user space with the CTM **already applied** (the path-construction ops
/// transform points at build time, like [`DrawPath`](crate::model::DrawPath)).
/// Image / shading ops carry their CTM explicitly because the placement matrix is
/// what the rasterizer needs.
#[derive(Clone, Debug)]
pub enum RenderOp {
    /// Save the graphics state (`q`): the renderer pushes its clip + state.
    Save,
    /// Restore the graphics state (`Q`).
    Restore,
    /// Fill the path (`f`/`F`/`f*`/`B`/`B*`/`b`/`b*`) with `color` (packed
    /// `0x00RRGGBB`) at constant alpha `alpha` (0–255), `even_odd` fill rule.
    Fill {
        /// The constructed path items (user space, CTM applied).
        items: Vec<PathItem>,
        /// Whether the final sub-path was closed.
        close: bool,
        /// Packed `0x00RRGGBB` fill color.
        color: u32,
        /// Constant fill alpha (graphics-state `ca`, 0–255).
        alpha: u8,
        /// Even-odd vs nonzero winding.
        even_odd: bool,
    },
    /// Stroke the path (`S`/`s`/`B`/…) with `color` / `width` (device-scaled by
    /// the renderer) at constant alpha `alpha`.
    Stroke {
        /// The constructed path items (user space, CTM applied).
        items: Vec<PathItem>,
        /// Whether the final sub-path was closed.
        close: bool,
        /// Packed `0x00RRGGBB` stroke color.
        color: u32,
        /// Constant stroke alpha (graphics-state `CA`, 0–255).
        alpha: u8,
        /// The line width in user space (raw `w` operand).
        width: f64,
        /// The CTM in effect (to scale the user-space width into device pixels).
        ctm: Matrix,
        /// The dash pattern string (`"[…] phase"`), empty when solid.
        dashes: String,
    },
    /// Intersect the clip with the constructed path (`W`/`W*` then a paint op).
    /// Applied *after* the paint that triggered it, per the PDF clip semantics.
    Clip {
        /// The clip path items (user space, CTM applied).
        items: Vec<PathItem>,
        /// Even-odd vs nonzero winding for the clip.
        even_odd: bool,
    },
    /// Paint a run of glyphs that share one font (a single show operator).
    Text(TextRun),
    /// Paint an image XObject (`Do`) or inline image (`BI…EI`).
    Image(ImageOp),
    /// Paint a shading (`sh` operator, or a shading-pattern fill).
    Shading(ShadingOp),
}

/// A run of positioned glyphs from one show operator, with the data needed to
/// resolve and rasterize the font program.
#[derive(Clone, Debug)]
pub struct TextRun {
    /// The positioned glyphs (PDF user space), one per shown code.
    pub glyphs: Vec<PositionedGlyph>,
    /// The glyph id in the embedded font program for each glyph (parallel to
    /// `glyphs`): `FontMapper::gid(code)`.
    pub gids: Vec<u32>,
    /// The resolved font dictionary (carries `/FontDescriptor` → `/FontFile*`).
    pub font_dict: Dict,
    /// The fill color packed as `0x00RRGGBB` (for fill render modes).
    pub fill_color: u32,
    /// The stroke color packed as `0x00RRGGBB` (for stroke render modes).
    pub stroke_color: u32,
    /// The constant fill alpha (graphics-state `ca`, 0–255).
    pub fill_alpha: u8,
    /// The text render mode `Tr` (0..=7).
    pub render_mode: u8,
    /// The stroke line width in user space (for stroked text modes).
    pub stroke_width: f64,
    /// The CTM in effect at paint time.
    pub ctm: Matrix,
}

/// An image drawcall (XObject `Do` or inline `BI…EI`).
#[derive(Clone, Debug)]
pub struct ImageOp {
    /// The decoded image stream's dictionary (carries `/Width`, `/Height`,
    /// `/ColorSpace`, `/ImageMask`, `/SMask`, `/Filter`, …).
    pub dict: Dict,
    /// The raw (still filter-encoded) image bytes. For an XObject these are the
    /// stream's raw bytes; for an inline image, the `ID…EI` body.
    pub raw: Vec<u8>,
    /// The object number of the image XObject (for `/SMask` resolution), or
    /// `None` for an inline image.
    pub obj_num: Option<u32>,
    /// The image-placement CTM (unit square → page).
    pub ctm: Matrix,
    /// The current fill color packed `0x00RRGGBB` (for stencil image masks).
    pub fill_color: u32,
    /// The constant fill alpha (graphics-state `ca`, 0–255).
    pub alpha: u8,
}

/// A shading drawcall (`sh` operator). The `dict` is the raw `/Shading` resource
/// dict; the renderer parses `/ShadingType`, `/Coords`, `/Function`, `/Extend`,
/// `/ColorSpace`. The full content stream's resources are carried so the renderer
/// can resolve indirect `/Function` streams.
#[derive(Clone, Debug)]
pub struct ShadingOp {
    /// The `/Shading` dictionary (or the stream dict for a type 4–7 shading).
    pub dict: Dict,
    /// The CTM in effect at paint time.
    pub ctm: Matrix,
    /// The constant fill alpha (graphics-state `ca`, 0–255).
    pub alpha: u8,
}

/// An ordered render-op sink the interpreter writes to when rendering.
///
/// The default implementation in `render_page` is a `Vec<RenderOp>`; a
/// `DisplayList` records the same stream. A boxed trait object keeps the
/// interpreter generic over "extract text" (no sink) vs "render" (a sink) without
/// duplicating the operator dispatch.
pub trait RenderSink {
    /// Records one ordered render op.
    fn push(&mut self, op: RenderOp);
}

impl RenderSink for Vec<RenderOp> {
    fn push(&mut self, op: RenderOp) {
        Vec::push(self, op);
    }
}

/// Reads a `/Function`-style alpha from a graphics-state `/ca` or `/CA` value.
/// Helper used by the interpreter when applying `gs` ExtGState dicts.
#[must_use]
pub fn alpha_from_object(obj: &Object) -> Option<u8> {
    obj.as_f64()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}
