//! Vector path rendering — fill / stroke / clip (M6a).
//!
//! Module owner: **M6a** (vector + canvas). Translates a [`pdf_text::DrawPath`]
//! (constructed path in PDF user space, with paint metadata) into rasterizer
//! drawcalls on a [`Canvas`]. The signatures below are frozen; M6a fills the
//! bodies. See `ARCHITECTURE.md`.

use tiny_skia::{
    BlendMode as SkBlendMode, Color, FillRule, LineCap, LineJoin, Mask, Paint as SkPaint,
    PathBuilder, Shader, Stroke, StrokeDash,
};

use pdf_core::geom::{Matrix, Point, Rect};
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
    /// The compositing mode (PDF `/BM`; defaults to Normal/SourceOver).
    pub blend: BlendMode,
}

/// The subset of PDF blend modes the rasterizer supports (PRD §8.11). Normal
/// (source-over) is always correct; `Multiply` and `Screen` map straight onto
/// tiny-skia's separable equivalents. Other PDF blend modes (Overlay, Darken,
/// …) are recognized by the interpreter but rendered as Normal here (documented
/// deferral — see `ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    /// Source-over compositing (PDF `Normal` / `Compatible`).
    #[default]
    Normal,
    /// Multiply blend.
    Multiply,
    /// Screen blend.
    Screen,
}

impl BlendMode {
    /// Maps to the rasterizer blend mode (unsupported → `SourceOver`).
    fn to_sk(self) -> SkBlendMode {
        match self {
            BlendMode::Normal => SkBlendMode::SourceOver,
            BlendMode::Multiply => SkBlendMode::Multiply,
            BlendMode::Screen => SkBlendMode::Screen,
        }
    }
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
            blend: BlendMode::Normal,
        }
    }

    /// Builds a paint from a packed `0x00RRGGBB` sRGB color and a constant alpha
    /// (PDF `CA`/`ca` 0..=255).
    #[must_use]
    pub fn from_rgb_alpha(rgb: u32, alpha: u8) -> Self {
        let mut p = Self::from_rgb(rgb);
        p.rgba[3] = alpha;
        p
    }

    /// Sets the blend mode (builder style).
    #[must_use]
    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        self.blend = blend;
        self
    }

    /// The tiny-skia [`Color`] (straight sRGBA8 → the rasterizer premultiplies).
    fn to_color(self) -> Color {
        Color::from_rgba8(self.rgba[0], self.rgba[1], self.rgba[2], self.rgba[3])
    }

    /// The tiny-skia [`SkPaint`] for this color + blend mode, anti-aliased.
    fn to_sk_paint<'a>(self) -> SkPaint<'a> {
        SkPaint {
            shader: Shader::SolidColor(self.to_color()),
            blend_mode: self.blend.to_sk(),
            anti_alias: true,
            force_hq_pipeline: false,
        }
    }
}

/// Stroke parameters (line width, joins, caps, dashes) in device space.
///
/// A thin first-party mirror of the rasterizer's stroke options so the public
/// stub surface does not leak the dependency type. Widths/dashes are in device
/// pixels (already CTM-scaled by the caller via [`scale_for_ctm`]).
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    /// Stroke width in device pixels (already CTM-scaled by the caller). A width
    /// of `0` requests a 1px hairline (tiny-skia semantics).
    pub width: f32,
    /// The line cap (butt / round / square).
    pub cap: LineCapStyle,
    /// The line join (miter / round / bevel).
    pub join: LineJoinStyle,
    /// The miter limit (PDF `M`; default 10, tiny-skia default 4).
    pub miter_limit: f32,
    /// The dash segment lengths in device pixels (empty = solid).
    pub dash_array: Vec<f32>,
    /// The dash phase in device pixels.
    pub dash_phase: f32,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            cap: LineCapStyle::Butt,
            join: LineJoinStyle::Miter,
            miter_limit: 10.0,
            dash_array: Vec::new(),
            dash_phase: 0.0,
        }
    }
}

/// PDF line-cap style (`J` operator: 0 butt, 1 round, 2 square).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCapStyle {
    /// `J 0` — butt (no extension).
    #[default]
    Butt,
    /// `J 1` — round.
    Round,
    /// `J 2` — projecting square.
    Square,
}

/// PDF line-join style (`j` operator: 0 miter, 1 round, 2 bevel).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoinStyle {
    /// `j 0` — miter.
    #[default]
    Miter,
    /// `j 1` — round.
    Round,
    /// `j 2` — bevel.
    Bevel,
}

impl StrokeStyle {
    /// Builds the tiny-skia [`Stroke`] for this style. The dash array is dropped
    /// when it is empty or invalid (a solid stroke), matching the PDF default.
    fn to_sk(&self) -> Stroke {
        let dash = if self.dash_array.is_empty() {
            None
        } else {
            StrokeDash::new(self.dash_array.clone(), self.dash_phase)
        };
        Stroke {
            width: self.width.max(0.0),
            miter_limit: self.miter_limit,
            line_cap: match self.cap {
                LineCapStyle::Butt => LineCap::Butt,
                LineCapStyle::Round => LineCap::Round,
                LineCapStyle::Square => LineCap::Square,
            },
            line_join: match self.join {
                LineJoinStyle::Miter => LineJoin::Miter,
                LineJoinStyle::Round => LineJoin::Round,
                LineJoinStyle::Bevel => LineJoin::Bevel,
            },
            dash,
        }
    }
}

/// The average linear scale factor a CTM applies, `sqrt(|det|)` — used to map a
/// user-space line width (PDF `w`) into device pixels. Falls back to `1.0` for a
/// degenerate (zero-determinant) matrix.
#[must_use]
pub fn scale_for_ctm(ctm: Matrix, base: Matrix) -> f32 {
    let m = Matrix::concat(&ctm, &base);
    let det = m.determinant().abs();
    if det.is_finite() && det > 0.0 {
        det.sqrt() as f32
    } else {
        1.0
    }
}

/// Fills `path` (PDF user space) into `canvas` with `paint`, composing `ctm`
/// onto the canvas base transform. `even_odd` selects the even-odd vs nonzero
/// winding fill rule.
///
/// # Errors
///
/// Returns [`Error::InvalidArgument`] only if the path produces no buildable
/// geometry *and* a caller contract is violated; in practice an empty/degenerate
/// path is a tolerant no-op (`Ok(())`) so arbitrary input never errors here.
pub fn fill_path(
    canvas: &mut Canvas,
    path: &DrawPath,
    paint: Paint,
    ctm: Matrix,
    even_odd: bool,
) -> Result<()> {
    fill_items(canvas, &path.items, path.close_path, paint, ctm, even_odd)
}

/// Fills a raw [`PathItem`] list (the shared core of [`fill_path`] and the text
/// glyph fill in M6b). `close` closes the final sub-path before filling.
///
/// # Errors
///
/// Never errors for arbitrary input — an unbuildable path is a no-op `Ok(())`.
pub fn fill_items(
    canvas: &mut Canvas,
    items: &[PathItem],
    close: bool,
    paint: Paint,
    ctm: Matrix,
    even_odd: bool,
) -> Result<()> {
    let Some(skpath) = build_path(items, close) else {
        return Ok(());
    };
    let transform = canvas.device_transform(ctm);
    let sk_paint = paint.to_sk_paint();
    let rule = if even_odd {
        FillRule::EvenOdd
    } else {
        FillRule::Winding
    };
    // Clone the clip mask out so the &mut pixmap borrow doesn't conflict.
    let clip = canvas.clip().cloned();
    canvas
        .pixmap_mut()
        .fill_path(&skpath, &sk_paint, rule, transform, clip.as_ref());
    Ok(())
}

/// Strokes `path` (PDF user space) into `canvas` with `paint`/`style`, composing
/// `ctm` onto the canvas base transform.
///
/// # Errors
///
/// Never errors for arbitrary input — an unbuildable path is a no-op `Ok(())`.
pub fn stroke_path(
    canvas: &mut Canvas,
    path: &DrawPath,
    paint: Paint,
    style: &StrokeStyle,
    ctm: Matrix,
) -> Result<()> {
    stroke_items(canvas, &path.items, path.close_path, paint, style, ctm)
}

/// Strokes a raw [`PathItem`] list (the shared core of [`stroke_path`]).
///
/// # Errors
///
/// Never errors for arbitrary input — an unbuildable path is a no-op `Ok(())`.
pub fn stroke_items(
    canvas: &mut Canvas,
    items: &[PathItem],
    close: bool,
    paint: Paint,
    style: &StrokeStyle,
    ctm: Matrix,
) -> Result<()> {
    let Some(skpath) = build_path(items, close) else {
        return Ok(());
    };
    let transform = canvas.device_transform(ctm);
    let sk_paint = paint.to_sk_paint();
    let stroke = style.to_sk();
    let clip = canvas.clip().cloned();
    canvas
        .pixmap_mut()
        .stroke_path(&skpath, &sk_paint, &stroke, transform, clip.as_ref());
    Ok(())
}

/// Intersects the current clip region with `path` (PDF user space) under `ctm`.
///
/// The clip is a rasterizer mask the subsequent draws are restricted to (PDF
/// `W`/`W*` followed by a path-painting operator). `even_odd` selects the clip
/// fill rule. An unbuildable path leaves the clip unchanged (`Ok(())`).
///
/// # Errors
///
/// Never errors for arbitrary input.
pub fn set_clip(
    canvas: &mut Canvas,
    items: &[PathItem],
    ctm: Matrix,
    even_odd: bool,
) -> Result<()> {
    let Some(skpath) = build_path(items, true) else {
        return Ok(());
    };
    let transform = canvas.device_transform(ctm);
    let rule = if even_odd {
        FillRule::EvenOdd
    } else {
        FillRule::Winding
    };
    let (w, h) = (canvas.width(), canvas.height());
    let Some(mut mask) = Mask::new(w, h) else {
        return Err(Error::LimitExceeded("clip mask too large"));
    };
    mask.fill_path(&skpath, rule, true, transform);
    canvas.intersect_clip(mask);
    Ok(())
}

/// Builds a tiny-skia [`Path`] from a [`PathItem`] list, emitting a `move_to`
/// whenever a segment does not continue from the current pen (a new sub-path).
/// `close` closes the final sub-path. Returns `None` for an empty/degenerate
/// path that tiny-skia cannot build (e.g. a single point, or all-NaN coords).
fn build_path(items: &[PathItem], close: bool) -> Option<tiny_skia::Path> {
    if items.is_empty() {
        return None;
    }
    let mut pb = PathBuilder::new();
    let mut pen: Option<Point> = None;

    /// Whether two points coincide within a tiny tolerance (avoids spurious
    /// `move_to` from float drift between a segment end and the next start).
    fn same(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
    }

    let start_subpath = |pb: &mut PathBuilder, p: Point, pen: &mut Option<Point>| {
        if pen.is_none_or(|cur| !same(cur, p)) {
            pb.move_to(p.x as f32, p.y as f32);
        }
        *pen = Some(p);
    };

    for item in items {
        match *item {
            PathItem::Line(a, b) => {
                if !finite(a) || !finite(b) {
                    continue;
                }
                start_subpath(&mut pb, a, &mut pen);
                pb.line_to(b.x as f32, b.y as f32);
                pen = Some(b);
            }
            PathItem::Curve(a, c1, c2, end) => {
                if !finite(a) || !finite(c1) || !finite(c2) || !finite(end) {
                    continue;
                }
                start_subpath(&mut pb, a, &mut pen);
                pb.cubic_to(
                    c1.x as f32,
                    c1.y as f32,
                    c2.x as f32,
                    c2.y as f32,
                    end.x as f32,
                    end.y as f32,
                );
                pen = Some(end);
            }
            PathItem::Rect(r) => {
                let r = r.normalize();
                if !rect_finite(r) {
                    continue;
                }
                if let Some(skr) =
                    tiny_skia::Rect::from_ltrb(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32)
                {
                    pb.push_rect(skr);
                    // `push_rect` is a self-contained closed sub-path; the pen
                    // returns to the rect origin.
                    pen = Some(Point::new(r.x0, r.y0));
                }
            }
        }
    }

    if close {
        pb.close();
    }
    pb.finish()
}

/// Whether a point has finite coordinates.
#[inline]
fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Whether a rect has finite edges and positive extent.
#[inline]
fn rect_finite(r: Rect) -> bool {
    r.x0.is_finite()
        && r.y0.is_finite()
        && r.x1.is_finite()
        && r.y1.is_finite()
        && r.x1 > r.x0
        && r.y1 > r.y0
}
