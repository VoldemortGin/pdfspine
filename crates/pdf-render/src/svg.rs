//! Page → standalone SVG document export (M7, PyMuPDF `Page.get_svg_image`).
//!
//! Module owner: **M7-svg**. Reuses the *exact same* ordered drawcall stream the
//! rasterizer replays onto pixels — [`pdf_text::interpret_page_render`], the
//! interpreter's opt-in render sink (the [`crate::render::DisplayList`] source) —
//! and serializes each [`RenderOp`] into SVG markup instead of painting it. There
//! is no second content walk; the *same* `Vec<RenderOp>` that `render_page` uses
//! drives this exporter, so SVG and PNG stay byte-for-byte consistent in z-order
//! and geometry.
//!
//! # Coordinate model
//!
//! Every paint op carries geometry in **PDF user space** (the CTM already applied
//! to fill/stroke/clip points; an explicit placement CTM for images/shadings).
//! A single outer `<g transform="matrix(…)">` carries the page transform
//! ([`pdf_text::page_transform`]: y-flip + `/Rotate`, CropBox-relative) composed
//! with the caller's scale `matrix`, so user-space coordinates map straight into
//! the SVG viewport (top-left origin, y-down). The `<svg viewBox/width/height>`
//! is sized to the displayed page (`page_size × matrix`).
//!
//! # Op coverage
//!
//! - **Vector** ([`RenderOp::Fill`]/[`RenderOp::Stroke`]) → `<path d=…>` with
//!   `fill`/`stroke`/`stroke-width`/`fill-rule`/`*-opacity`.
//! - **Clip** ([`RenderOp::Clip`]) → a `<clipPath>` def + a `clip-path` group
//!   that scopes following siblings until the matching `Q`.
//! - **Text** ([`RenderOp::Text`]) → glyph **outlines** as `<path>` (extracted
//!   from the embedded program via `ttf-parser`, the same source the rasterizer
//!   uses), so rendering is font-independent. A non-embedded / unparseable font
//!   falls back to a `<text>` element.
//! - **Image** ([`RenderOp::Image`]) → `<image href="data:image/png;base64,…">`
//!   (the decoded Pixmap re-encoded to PNG), placed by its CTM.
//! - **Shading** ([`RenderOp::Shading`]) → a `<linearGradient>`/`<radialGradient>`
//!   def + a filled rect over the page.
//!
//! Deferrals (documented gaps, never errors): shading types 1/4–7 (only axial 2 /
//! radial 3 emit a gradient), tiling patterns, blend modes, dashed-stroke
//! patterns (solid stroke is emitted). Malformed input never panics — a broken
//! op is skipped, mirroring the rasterizer's degradation contract.

use std::fmt::Write as _;

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_core::Page;
use pdf_core::{Dict, DocumentStore, Name, Object};
use pdf_image::codecs::decode_image_stream;
use pdf_image::pixmap::Pixmap;
use pdf_text::model::{PathItem, PositionedGlyph};
use pdf_text::{interpret_page_render, ImageOp, RenderOp, ShadingOp, TextRun};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::error::Result;

/// Options controlling an SVG export (PyMuPDF `Page.get_svg_image(matrix=…)`).
#[derive(Clone, Debug)]
pub struct SvgOptions {
    /// The page user-space → device scale/rotate applied on top of the page
    /// transform. Defaults to the identity (1 pt = 1 SVG user unit).
    pub matrix: Matrix,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            matrix: Matrix::IDENTITY,
        }
    }
}

/// Exports `page` of `doc` to a standalone SVG document string (PyMuPDF
/// `Page.get_svg_image`).
///
/// Records the page's ordered [`RenderOp`] stream (reusing the rasterizer's
/// interpreter sink) and serializes it to a well-formed `<svg>` document. An
/// empty / contentless page yields a valid empty `<svg>`; malformed content
/// degrades op-by-op and never panics.
///
/// # Errors
///
/// Currently infallible for the serialization itself (the `Result` mirrors the
/// sibling render entry points and leaves room for future limit checks).
pub fn get_svg_image(doc: &DocumentStore, page: &Page, opts: &SvgOptions) -> Result<String> {
    let cropbox = page.cropbox();
    let rotate = page.rotation();
    let ops = match page.dict() {
        Some(dict) => interpret_page_render(doc, &dict),
        None => Vec::new(),
    };
    Ok(serialize(doc, &ops, cropbox, rotate, opts))
}

/// Serializes an ordered op stream into a standalone SVG document.
fn serialize(
    doc: &DocumentStore,
    ops: &[RenderOp],
    cropbox: Rect,
    rotate: i32,
    opts: &SvgOptions,
) -> String {
    // Displayed page size (post-/Rotate) × the caller's scale.
    let (pw, ph) = pdf_text::page_size(cropbox, rotate);
    let sx = opts.matrix.a.abs().max(opts.matrix.b.abs());
    let sy = opts.matrix.d.abs().max(opts.matrix.c.abs());
    let vw = round2(pw * if sx == 0.0 { 1.0 } else { sx });
    let vh = round2(ph * if sy == 0.0 { 1.0 } else { sy });

    // The outer transform mapping PDF user space → SVG viewport: page transform
    // (y-flip + rotate) then the caller's scale matrix.
    let page_t = pdf_text::page_transform(cropbox, rotate);
    let device = Matrix::concat(&page_t, &opts.matrix);

    let mut w = SvgWriter::new();
    let _ = write!(
        w.out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{vw}\" height=\"{vh}\" viewBox=\"0 0 {vw} {vh}\">\n"
    );
    // <defs> collects clipPaths + gradients referenced by id below. We buffer the
    // body first so defs (built while walking) can precede it.
    let mut body = String::new();
    let _ = writeln!(body, "<g transform=\"matrix({})\">", fmt_matrix(&device));

    walk_ops(doc, ops, &mut w, &mut body);

    body.push_str("</g>\n");

    if !w.defs.is_empty() {
        let _ = write!(w.out, "<defs>\n{}</defs>\n", w.defs);
    }
    w.out.push_str(&body);
    w.out.push_str("</svg>\n");
    w.out
}

/// Accumulator for the SVG document: the output prologue, a `<defs>` buffer
/// (clipPaths / gradients), and an id counter for unique references.
struct SvgWriter {
    out: String,
    defs: String,
    next_id: u32,
}

impl SvgWriter {
    fn new() -> Self {
        SvgWriter {
            out: String::new(),
            defs: String::new(),
            next_id: 0,
        }
    }

    fn fresh_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }
}

/// Walks the op stream, appending element markup to `body` and definitions to
/// `w.defs`. Save/Restore are modeled as nested `<g>` groups; a `Clip` op opens
/// a `clip-path` group that the matching `Restore` closes.
fn walk_ops(doc: &DocumentStore, ops: &[RenderOp], w: &mut SvgWriter, body: &mut String) {
    // For each open save scope, the number of `</g>` to emit on its Restore (1
    // for the save group itself, plus 1 per clip group opened within it).
    let mut scopes: Vec<u32> = Vec::new();

    for op in ops {
        match op {
            RenderOp::Save => {
                body.push_str("<g>\n");
                scopes.push(1);
            }
            RenderOp::Restore => {
                let n = scopes.pop().unwrap_or(1);
                for _ in 0..n {
                    body.push_str("</g>\n");
                }
            }
            RenderOp::Fill {
                items,
                close,
                color,
                alpha,
                even_odd,
            } => {
                write_fill(body, items, *close, *color, *alpha, *even_odd);
            }
            RenderOp::Stroke {
                items,
                close,
                color,
                alpha,
                width,
                ..
            } => {
                write_stroke(body, items, *close, *color, *alpha, *width);
            }
            RenderOp::Clip { items, even_odd } => {
                let id = w.fresh_id("clip");
                let d = path_data(items, false);
                let rule = if *even_odd {
                    " clip-rule=\"evenodd\""
                } else {
                    ""
                };
                let _ = writeln!(
                    w.defs,
                    "<clipPath id=\"{id}\"><path d=\"{d}\"{rule}/></clipPath>"
                );
                let _ = writeln!(body, "<g clip-path=\"url(#{id})\">");
                // This clip group is closed by the enclosing Restore (or at the
                // document end). Charge one extra `</g>` to the current scope.
                if let Some(top) = scopes.last_mut() {
                    *top += 1;
                } else {
                    // Clip outside any q/Q: treat as an implicit top-level scope
                    // so it still balances at the end.
                    scopes.push(1);
                }
            }
            RenderOp::Text(run) => write_text(doc, run, body),
            RenderOp::Image(img) => write_image(doc, img, body),
            RenderOp::Shading(sh) => write_shading(doc, sh, w, body),
        }
    }

    // Close any scopes left open by unbalanced content (malformed input).
    while let Some(n) = scopes.pop() {
        for _ in 0..n {
            body.push_str("</g>\n");
        }
    }
}

// === vector ================================================================

/// Emits a `<path>` for a fill op.
fn write_fill(
    body: &mut String,
    items: &[PathItem],
    close: bool,
    color: u32,
    alpha: u8,
    even_odd: bool,
) {
    if items.is_empty() {
        return;
    }
    let d = path_data(items, close);
    if d.is_empty() {
        return;
    }
    let rule = if even_odd { "evenodd" } else { "nonzero" };
    let _ = writeln!(
        body,
        "<path d=\"{d}\" fill=\"{}\"{} fill-rule=\"{rule}\"/>",
        hex_color(color),
        opacity_attr("fill-opacity", alpha),
    );
}

/// Emits a `<path>` for a stroke op.
fn write_stroke(
    body: &mut String,
    items: &[PathItem],
    close: bool,
    color: u32,
    alpha: u8,
    width: f64,
) {
    if items.is_empty() {
        return;
    }
    let d = path_data(items, close);
    if d.is_empty() {
        return;
    }
    // A zero/negative width is a hairline (1 user unit).
    let sw = if width.is_finite() && width > 0.0 {
        round2(width)
    } else {
        1.0
    };
    let _ = writeln!(
        body,
        "<path d=\"{d}\" fill=\"none\" stroke=\"{}\"{} stroke-width=\"{sw}\"/>",
        hex_color(color),
        opacity_attr("stroke-opacity", alpha),
    );
}

/// Builds an SVG path `d` string from constructed path items (user space).
fn path_data(items: &[PathItem], close: bool) -> String {
    let mut d = String::new();
    // Track the current subpath start so we know when to emit a new `M`.
    let mut cur: Option<Point> = None;
    for item in items {
        match item {
            PathItem::Line(p0, p1) => {
                if cur != Some(*p0) {
                    let _ = write!(d, "M{} {}", num(p0.x), num(p0.y));
                }
                let _ = write!(d, "L{} {}", num(p1.x), num(p1.y));
                cur = Some(*p1);
            }
            PathItem::Curve(p0, c1, c2, p1) => {
                if cur != Some(*p0) {
                    let _ = write!(d, "M{} {}", num(p0.x), num(p0.y));
                }
                let _ = write!(
                    d,
                    "C{} {} {} {} {} {}",
                    num(c1.x),
                    num(c1.y),
                    num(c2.x),
                    num(c2.y),
                    num(p1.x),
                    num(p1.y)
                );
                cur = Some(*p1);
            }
            PathItem::Rect(r) => {
                // A rectangle as an explicit closed subpath (M..L..L..L..Z).
                let _ = write!(
                    d,
                    "M{} {}L{} {}L{} {}L{} {}Z",
                    num(r.x0),
                    num(r.y0),
                    num(r.x1),
                    num(r.y0),
                    num(r.x1),
                    num(r.y1),
                    num(r.x0),
                    num(r.y1)
                );
                cur = None;
            }
        }
    }
    if close && !d.is_empty() && !d.ends_with('Z') {
        d.push('Z');
    }
    d
}

// === text ==================================================================

/// Emits glyph outlines (`<path>`) for a text run, or a `<text>` fallback when
/// the embedded font program is unavailable / unparseable.
fn write_text(doc: &DocumentStore, run: &TextRun, body: &mut String) {
    if run.glyphs.is_empty() {
        return;
    }
    // Mode 3 (invisible) and 7 (clip-only) paint nothing.
    let program = resolve_font_program(doc, &run.font_dict);
    let face = program
        .as_deref()
        .and_then(|bytes| Face::parse(bytes, 0).ok());

    match face {
        Some(face) => write_glyph_outlines(&face, run, body),
        None => write_text_fallback(run, body),
    }
}

/// Emits each glyph's outline as a filled `<path>` under a per-glyph transform.
fn write_glyph_outlines(face: &Face, run: &TextRun, body: &mut String) {
    let upem = f64::from(face.units_per_em().max(1));
    let fill = hex_color(run.fill_color);
    let op = opacity_attr("fill-opacity", run.fill_alpha);

    for (glyph, &gid) in run.glyphs.iter().zip(run.gids.iter()) {
        if matches!(glyph.render_mode, 3 | 7) {
            continue;
        }
        let resolved = resolve_gid(face, glyph, gid);
        let mut sink = SvgOutline {
            d: String::new(),
            upem,
        };
        if face.outline_glyph(GlyphId(resolved), &mut sink).is_none() {
            continue; // whitespace / missing glyph.
        }
        if sink.d.is_empty() {
            continue;
        }
        let s = glyph.size / upem;
        if !s.is_finite() || s == 0.0 {
            continue;
        }
        // font-unit (y-up, /upem normalized in the sink) → user space:
        // scale by size, translate to the glyph origin.
        let Point { x: ox, y: oy } = glyph.origin;
        let m = Matrix::new(glyph.size, 0.0, 0.0, glyph.size, ox, oy);
        let _ = writeln!(
            body,
            "<path d=\"{}\" fill=\"{fill}\"{op} transform=\"matrix({})\"/>",
            sink.d,
            fmt_matrix(&m)
        );
    }
}

/// A `<text>` fallback: positions one element at the run's first glyph origin
/// with the concatenated Unicode. Used when no outline program is available.
fn write_text_fallback(run: &TextRun, body: &mut String) {
    let visible: String = run
        .glyphs
        .iter()
        .filter(|g| !matches!(g.render_mode, 3 | 7))
        .flat_map(|g| g.unicode.chars())
        .collect();
    if visible.is_empty() {
        return;
    }
    let first = &run.glyphs[0];
    let size = round2(first.size);
    // The text element lives in the y-up user space of the outer group; flip its
    // own y so the glyphs read upright.
    let m = Matrix::new(1.0, 0.0, 0.0, -1.0, first.origin.x, first.origin.y);
    let _ = writeln!(
        body,
        "<text x=\"0\" y=\"0\" font-size=\"{size}\" fill=\"{}\"{} transform=\"matrix({})\">{}</text>",
        hex_color(run.fill_color),
        opacity_attr("fill-opacity", run.fill_alpha),
        fmt_matrix(&m),
        escape_xml(&visible),
    );
}

/// An `OutlineBuilder` that emits SVG path data, normalizing font units to a
/// unit em (divide by `upem`) and flipping y so the outline is upright in the
/// y-up user space the glyph transform applies (scale uses the font size).
struct SvgOutline {
    d: String,
    upem: f64,
}

impl SvgOutline {
    #[inline]
    fn nx(&self, v: f32) -> f64 {
        f64::from(v) / self.upem
    }
    #[inline]
    fn ny(&self, v: f32) -> f64 {
        f64::from(v) / self.upem
    }
}

impl OutlineBuilder for SvgOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "M{} {}", num(self.nx(x)), num(self.ny(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "L{} {}", num(self.nx(x)), num(self.ny(y)));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(
            self.d,
            "Q{} {} {} {}",
            num(self.nx(x1)),
            num(self.ny(y1)),
            num(self.nx(x)),
            num(self.ny(y))
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(
            self.d,
            "C{} {} {} {} {} {}",
            num(self.nx(x1)),
            num(self.ny(y1)),
            num(self.nx(x2)),
            num(self.ny(y2)),
            num(self.nx(x)),
            num(self.ny(y))
        );
    }
    fn close(&mut self) {
        self.d.push('Z');
    }
}

/// Resolves the program glyph id for one positioned glyph (mirrors the
/// rasterizer's `resolve_gid`: prefer a cmap lookup by Unicode, else the mapper
/// gid when it is a valid non-`.notdef` index).
fn resolve_gid(face: &Face, glyph: &PositionedGlyph, mapper_gid: u32) -> u16 {
    if let Some(ch) = glyph.unicode.chars().next() {
        if let Some(g) = face.glyph_index(ch) {
            if g.0 != 0 {
                return g.0;
            }
        }
    }
    let g = u16::try_from(mapper_gid).unwrap_or(0);
    if g != 0 && g < face.number_of_glyphs() {
        return g;
    }
    0
}

// === images ================================================================

/// Emits an `<image>` with a base64 PNG data URI for an image op. A decode
/// failure is swallowed (the degradation contract — never abort the document).
fn write_image(doc: &DocumentStore, img: &ImageOp, body: &mut String) {
    if img.alpha == 0 {
        return;
    }
    let decoded = match decode_image_stream(doc, &img.dict, &img.raw) {
        Ok(d) => d,
        Err(_) => return,
    };
    // Stencil image masks have no color plane to encode as PNG; skip (the raster
    // path paints them with the fill color — a documented SVG deferral).
    let is_mask = dict_bool(&img.dict, "ImageMask") || dict_bool(&img.dict, "IM");
    if is_mask {
        return;
    }
    let pix = match Pixmap::from_decoded(&decoded) {
        Ok(p) => p,
        Err(_) => return,
    };
    if pix.width == 0 || pix.height == 0 {
        return;
    }
    let png = match pix.to_png_bytes() {
        Ok(b) => b,
        Err(_) => return,
    };
    let b64 = base64_encode(&png);

    // The image-placement CTM maps the unit square (origin bottom-left, y-up)
    // onto the page. SVG <image> draws into a `width × height` box with origin
    // top-left, y-down, so compose: scale to 1×1 then flip y into the unit
    // square the CTM expects. m = [1/w 0 0 -1/h 0 1] · ctm.
    let pre = Matrix::new(
        1.0 / f64::from(pix.width),
        0.0,
        0.0,
        -1.0 / f64::from(pix.height),
        0.0,
        1.0,
    );
    let m = Matrix::concat(&pre, &img.ctm);
    let _ = writeln!(
        body,
        "<image width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" \
         transform=\"matrix({})\" href=\"data:image/png;base64,{b64}\"/>",
        pix.width,
        pix.height,
        fmt_matrix(&m),
    );
}

// === shadings ==============================================================

/// Emits a `<linearGradient>`/`<radialGradient>` def + a filled page-covering
/// rect for an axial (type 2) / radial (type 3) shading. Other types are a
/// documented deferral (skipped).
fn write_shading(doc: &DocumentStore, sh: &ShadingOp, w: &mut SvgWriter, body: &mut String) {
    let dict = &sh.dict;
    let stype = dict
        .get(&Name::new("ShadingType"))
        .and_then(Object::as_i64)
        .unwrap_or(0);
    let coords: Vec<f64> = dict
        .get(&Name::new("Coords"))
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_f64).collect())
        .unwrap_or_default();
    // Two endpoint colors sampled from the /Function at t=0 and t=1.
    let (c0, c1) = sample_shading_endpoints(doc, dict);

    let id = w.fresh_id("grad");
    let grad = match stype {
        2 if coords.len() >= 4 => format!(
            "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" \
             x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">\
             <stop offset=\"0\" stop-color=\"{}\"/>\
             <stop offset=\"1\" stop-color=\"{}\"/></linearGradient>\n",
            num(coords[0]),
            num(coords[1]),
            num(coords[2]),
            num(coords[3]),
            hex_color(c0),
            hex_color(c1),
        ),
        3 if coords.len() >= 6 => format!(
            "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" \
             cx=\"{}\" cy=\"{}\" r=\"{}\" fx=\"{}\" fy=\"{}\">\
             <stop offset=\"0\" stop-color=\"{}\"/>\
             <stop offset=\"1\" stop-color=\"{}\"/></radialGradient>\n",
            num(coords[3]),
            num(coords[4]),
            num(coords[5].max(0.0)),
            num(coords[0]),
            num(coords[1]),
            hex_color(c0),
            hex_color(c1),
        ),
        _ => return, // types 1 / 4–7: deferred.
    };
    w.defs.push_str(&grad);

    // Fill a large rect (in the shading's CTM space) referencing the gradient.
    // The shading CTM maps user space; a generous box covers the page.
    let m = sh.ctm;
    let _ = writeln!(
        body,
        "<g transform=\"matrix({})\">\
         <rect x=\"-100000\" y=\"-100000\" width=\"200000\" height=\"200000\" \
         fill=\"url(#{id})\"{}/></g>",
        fmt_matrix(&m),
        opacity_attr("fill-opacity", sh.alpha),
    );
}

/// Samples a shading's `/Function` at `t=0` and `t=1` for the two gradient
/// stops. Falls back to black→white when the function cannot be evaluated (so a
/// gradient is still emitted, never a hard error).
fn sample_shading_endpoints(doc: &DocumentStore, dict: &Dict) -> (u32, u32) {
    let cs = shading_is_rgb(doc, dict);
    let func = dict.get(&Name::new("Function"));
    let (a, b) = match func {
        Some(f) => (eval_simple(doc, f, 0.0), eval_simple(doc, f, 1.0)),
        None => (None, None),
    };
    let to_rgb = |vals: Option<Vec<f32>>| -> u32 {
        match vals {
            Some(v) => components_to_rgb(&v, cs),
            None => 0x0000_0000,
        }
    };
    let c0 = to_rgb(a);
    let c1 = match to_rgb(b) {
        0 if func.is_none() => 0x00FF_FFFF, // black→white default fallback.
        other => other,
    };
    (c0, c1)
}

/// Whether the shading colorspace is (Device)RGB-ish (else treat as gray).
fn shading_is_rgb(doc: &DocumentStore, dict: &Dict) -> bool {
    let cs = doc
        .resolve_dict_key(dict, &Name::new("ColorSpace"))
        .ok()
        .flatten();
    match cs.as_deref() {
        Some(Object::Name(n)) => !matches!(n.as_str(), Some("DeviceGray" | "CalGray" | "G")),
        Some(Object::Array(a)) => a
            .first()
            .and_then(Object::as_name)
            .and_then(Name::as_str)
            .map(|n| !matches!(n, "DeviceGray" | "CalGray" | "G"))
            .unwrap_or(true),
        _ => true,
    }
}

/// Maps function output components to a packed `0x00RRGGBB` color.
fn components_to_rgb(v: &[f32], is_rgb: bool) -> u32 {
    let clamp = |x: f32| ((x.clamp(0.0, 1.0) * 255.0).round() as u32) & 0xFF;
    if is_rgb && v.len() >= 3 {
        (clamp(v[0]) << 16) | (clamp(v[1]) << 8) | clamp(v[2])
    } else if !v.is_empty() {
        let g = clamp(v[0]);
        (g << 16) | (g << 8) | g
    } else {
        0
    }
}

/// Evaluates a simple type-2 (exponential) `/Function` at `t` for the gradient
/// endpoints. Other function types / indirect arrays return `None` (the caller
/// falls back). Intentionally minimal — gradient endpoints only need t∈{0,1}.
fn eval_simple(doc: &DocumentStore, obj: &Object, t: f32) -> Option<Vec<f32>> {
    let resolved = match obj {
        Object::Reference(r) => doc.resolve(*r).ok()?,
        other => std::sync::Arc::new(other.clone()),
    };
    let d = match resolved.as_ref() {
        Object::Dictionary(d) => d.clone(),
        Object::Stream(s) => s.dict.clone(),
        Object::Array(a) => {
            // Per-channel array of single-output functions: sample each.
            let mut out = Vec::new();
            for f in a {
                if let Some(mut v) = eval_simple(doc, f, t) {
                    out.append(&mut v);
                }
            }
            return if out.is_empty() { None } else { Some(out) };
        }
        _ => return None,
    };
    let ftype = d.get(&Name::new("FunctionType")).and_then(Object::as_i64)?;
    if ftype != 2 {
        return None; // only the common exponential endpoints are sampled.
    }
    let c0 = read_floats(&d, "C0").unwrap_or_else(|| vec![0.0]);
    let c1 = read_floats(&d, "C1").unwrap_or_else(|| vec![1.0]);
    let n = d
        .get(&Name::new("N"))
        .and_then(Object::as_f64)
        .unwrap_or(1.0) as f32;
    let tn = t.powf(n);
    let len = c0.len().max(c1.len());
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let a = c0.get(i).copied().unwrap_or(0.0);
        let b = c1.get(i).copied().unwrap_or(0.0);
        out.push(a + tn * (b - a));
    }
    Some(out)
}

fn read_floats(d: &Dict, key: &str) -> Option<Vec<f32>> {
    let a = d.get(&Name::new(key)).and_then(Object::as_array)?;
    Some(
        a.iter()
            .filter_map(|o| o.as_f64().map(|v| v as f32))
            .collect(),
    )
}

// === font program resolution (mirrors render.rs) ===========================

/// Resolves a font dict's embedded program bytes (`/FontFile2` / `/FontFile3`).
/// `None` for a non-embedded / bare Type1 / unresolvable font.
fn resolve_font_program(doc: &DocumentStore, font_dict: &Dict) -> Option<Vec<u8>> {
    let descriptor = font_descriptor(doc, font_dict)?;
    for key in ["FontFile2", "FontFile3"] {
        if let Some(obj) = doc
            .resolve_dict_key(&descriptor, &Name::new(key))
            .ok()
            .flatten()
        {
            if let Some(stream) = obj.as_stream() {
                if let Ok(bytes) = doc.decode_stream(stream).and_then(|o| o.into_decoded()) {
                    if !bytes.is_empty() {
                        return Some(bytes);
                    }
                }
            }
        }
    }
    None
}

/// Resolves a font dict's `/FontDescriptor`, descending into a Type0 composite
/// font's `/DescendantFonts[0]`.
fn font_descriptor(doc: &DocumentStore, font_dict: &Dict) -> Option<Dict> {
    if let Some(d) = doc
        .resolve_dict_key(font_dict, &Name::new("FontDescriptor"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
    {
        return Some(d);
    }
    let df = doc
        .resolve_dict_key(font_dict, &Name::new("DescendantFonts"))
        .ok()
        .flatten()?;
    let arr = df.as_array()?;
    let first = arr.first()?;
    let descendant = match first {
        Object::Reference(r) => doc.resolve(*r).ok()?,
        other => std::sync::Arc::new(other.clone()),
    };
    let descendant = descendant.as_dict()?;
    doc.resolve_dict_key(descendant, &Name::new("FontDescriptor"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
}

// === formatting helpers ====================================================

/// Formats a [`Matrix`] as the six `a,b,c,d,e,f` values for `matrix(…)`.
fn fmt_matrix(m: &Matrix) -> String {
    format!(
        "{},{},{},{},{},{}",
        num(m.a),
        num(m.b),
        num(m.c),
        num(m.d),
        num(m.e),
        num(m.f)
    )
}

/// Formats a float compactly: integers without a decimal point, others rounded
/// to a few significant places, NaN/inf as `0`.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 {
        return "0".to_string(); // avoid "-0".
    }
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        // Trim trailing zeros.
        let s = format!("{r:.3}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

/// Rounds a positive dimension to 2 places (viewBox / width / height).
fn round2(v: f64) -> f64 {
    if !v.is_finite() {
        return 1.0;
    }
    let r = (v * 100.0).round() / 100.0;
    if r <= 0.0 {
        1.0
    } else {
        r
    }
}

/// A packed `0x00RRGGBB` color as `#rrggbb`.
fn hex_color(rgb: u32) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgb >> 16) & 0xFF,
        (rgb >> 8) & 0xFF,
        rgb & 0xFF
    )
}

/// An ` name="o"` opacity attribute (omitted when fully opaque).
fn opacity_attr(name: &str, alpha: u8) -> String {
    if alpha == 255 {
        String::new()
    } else {
        format!(" {name}=\"{}\"", num(f64::from(alpha) / 255.0))
    }
}

/// Escapes XML special characters in text/attribute content.
fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Standard base64 (RFC 4648) encoder — no external dependency, keeps the crate
/// graph unchanged.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// A boolean dict value (default false).
fn dict_bool(dict: &Dict, key: &str) -> bool {
    dict.get(&Name::new(key))
        .and_then(Object::as_bool)
        .unwrap_or(false)
}
