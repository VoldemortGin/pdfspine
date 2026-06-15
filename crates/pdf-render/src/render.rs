//! The page render entry point — `render_page` + `DisplayList` (M6d).
//!
//! Module owner: **M6d** (full-page orchestration + DisplayList + wiring).
//!
//! # Design — ordered display list, replayed onto a `Canvas`
//!
//! The M2 content interpreter ([`pdf_text`]) already parses content streams,
//! tracks the graphics/text state machine (`q/Q/cm`, color, text state), recurses
//! Form XObjects (depth/cycle guarded), and builds positioned glyphs + paths +
//! image placements. Its flat [`pdf_text::InterpretResult`] groups drawcalls by
//! *kind*, which loses the **z-order** rendering needs.
//!
//! Rather than fork the interpreter, M6d reuses it through an **opt-in ordered
//! sink**: [`pdf_text::interpret_page_render`] runs the same state machine but
//! records an ordered [`pdf_text::RenderOp`] stream in document order. M6d
//! **replays** that list onto a [`Canvas`], dispatching each op to the M6a/b/c
//! primitives ([`crate::vector`] / [`crate::text`] / [`crate::image`]). The
//! text-extraction path attaches no sink, so the M2 tests stay byte-identical
//! (verified: all `pdf-text` tests still green).
//!
//! A [`DisplayList`] is exactly this recorded op stream: `page.get_displaylist()`
//! records it once; `dl.get_pixmap()` replays it (PyMuPDF `DisplayList`).
//!
//! # Coverage / documented gaps
//!
//! - Glyphs rasterize for **embedded** TrueType (`/FontFile2`) and OpenType/CFF
//!   (`/FontFile3`) programs, and for **Type3** fonts (each glyph is a
//!   content-stream `/CharProcs` procedure, drawn recursively through the
//!   `FontMatrix · Trm` mapping — d0/d1, colored/uncolored, depth-guarded).
//!   Bare Type1 (`/FontFile` PFB) and non-embedded standard-14 fonts are
//!   **not** rasterized (text stays extractable; no license-uncertain
//!   substitute font is bundled).
//! - Images: XObject + inline, Gray/RGB/CMYK, `/SMask` soft masks, stencil
//!   `/ImageMask`. Tiling patterns and shading-pattern fills are deferred; the
//!   bare `sh` operator paints axial/radial (types 2 & 3).
//! - Rotated/sheared text uses upright glyph placement (the interpreter's
//!   `PositionedGlyph` keeps only the axis-aligned origin/bbox) — a documented
//!   approximation; positions are correct.

use std::collections::HashMap;

use pdf_core::geom::{IRect, Matrix, Rect};
use pdf_core::{Dict, DocumentStore, Name, Object, Page};
use pdf_image::codecs::decode_image_stream;
use pdf_image::pixmap::{Colorspace, Pixmap};
use pdf_text::{interpret_page_render, ImageOp, RenderOp, ShadingOp, TextRun};

use crate::canvas::Canvas;
use crate::error::{Error, Result};
use crate::image::{
    draw_axial_shading, draw_image, draw_image_mask, draw_radial_shading, PdfFunction,
};
use crate::text::GlyphFont;
use crate::vector::{
    fill_items, scale_for_ctm, set_clip, stroke_items, LineCapStyle, LineJoinStyle, Paint,
    StrokeStyle,
};

/// Max device pixels for a render target (never-OOM guard, PRD §9.6.2). ~178 MP
/// (the largest 16384² target tiny-skia can comfortably allocate).
const MAX_RENDER_PIXELS: u64 = 16384 * 16384;

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

/// A recorded, replayable page render — the PyMuPDF `DisplayList`.
///
/// `page.get_displaylist()` records the ordered drawcall stream once
/// ([`DisplayList::from_page`]); `dl.get_pixmap()` replays it onto a fresh canvas
/// any number of times at any scale (cheaper than re-interpreting the page).
pub struct DisplayList {
    ops: Vec<RenderOp>,
    /// The page CropBox (user space) — the geometry the device transform is
    /// built from.
    cropbox: Rect,
    /// The page `/Rotate` value.
    rotate: i32,
}

impl DisplayList {
    /// Records the ordered drawcall stream for `page` (PyMuPDF
    /// `Page.get_displaylist`).
    #[must_use]
    pub fn from_page(doc: &DocumentStore, page: &Page) -> Self {
        let ops = match page.dict() {
            Some(dict) => interpret_page_render(doc, &dict),
            None => Vec::new(),
        };
        DisplayList {
            ops,
            cropbox: page.cropbox(),
            rotate: page.rotation(),
        }
    }

    /// The display list's source rect (the page CropBox), PyMuPDF
    /// `DisplayList.rect`.
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.cropbox
    }

    /// The number of recorded drawcalls (diagnostic).
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether the list recorded no drawcalls.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Replays the recorded drawcalls into a [`Pixmap`] under `opts` (PyMuPDF
    /// `DisplayList.get_pixmap`).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] / [`Error::LimitExceeded`] for bad geometry;
    /// propagates `pdf-core` / `pdf-image` errors during op replay.
    pub fn get_pixmap(&self, doc: &DocumentStore, opts: &RenderOptions) -> Result<Pixmap> {
        let mut canvas = build_canvas(self.cropbox, self.rotate, opts)?;
        replay(&mut canvas, doc, &self.ops, &RenderCtx::page())?;
        canvas.into_pixmap()
    }
}

/// Renders `page` of `doc` to a [`Pixmap`] under `opts` (PRD §8.11).
///
/// Computes the device transform + target geometry, builds a [`Canvas`], records
/// the page's ordered drawcalls (reusing the [`pdf_text`] interpreter via its
/// opt-in render sink), replays them in z-order, and converts to the requested
/// output format. Never panics on malformed content (PRD §8.1).
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a degenerate render geometry,
/// [`Error::LimitExceeded`] when the target would exceed the pixel ceiling, and
/// propagated `pdf-core` / `pdf-image` errors.
pub fn render_page(doc: &DocumentStore, page: &Page, opts: &RenderOptions) -> Result<Pixmap> {
    let cropbox = page.cropbox();
    let rotate = page.rotation();
    let mut canvas = build_canvas(cropbox, rotate, opts)?;

    let ops = match page.dict() {
        Some(dict) => interpret_page_render(doc, &dict),
        None => Vec::new(),
    };
    replay(&mut canvas, doc, &ops, &RenderCtx::page())?;
    canvas.into_pixmap()
}

// === canvas + device transform ============================================

/// Builds a blank background canvas for a page CropBox / rotation under `opts`.
///
/// The device transform composes the §8.6.1 page transform (y-flip + `/Rotate`,
/// MediaBox-relative — here CropBox) with the render scale (DPI → `dpi/72`, else
/// `opts.matrix`). The pixel target is the transformed page bound, clamped to the
/// optional clip. The background is opaque white unless `alpha`.
fn build_canvas(cropbox: Rect, rotate: i32, opts: &RenderOptions) -> Result<Canvas> {
    let scale = render_scale(opts);
    // Page transform (CropBox-relative, y-down, rotate applied), 1pt = 1px.
    let page_t = pdf_text::page_transform(cropbox, rotate);
    // Compose the requested scale on top: device = page_t · scale.
    let base = Matrix::concat(&page_t, &scale);

    let (pw, ph) = pdf_text::page_size(cropbox, rotate);
    // Transformed page bound in device pixels (the displayed page × scale).
    let dev_w = (pw * scale.a.abs()).round();
    let dev_h = (ph * scale.d.abs()).round();
    let mut width = (dev_w.max(1.0)) as u64;
    let mut height = (dev_h.max(1.0)) as u64;

    // Optional device-space clip: render only that sub-rectangle. We translate
    // the base transform so the clip origin maps to (0,0) and size the target to
    // the clip extent.
    let mut base = base;
    if let Some(clip) = opts.clip {
        let cw = (clip.x1 - clip.x0).max(0) as u64;
        let ch = (clip.y1 - clip.y0).max(0) as u64;
        if cw > 0 && ch > 0 {
            width = cw;
            height = ch;
            base = Matrix::concat(
                &base,
                &Matrix::translate(-f64::from(clip.x0), -f64::from(clip.y0)),
            );
        }
    }

    if width == 0 || height == 0 {
        return Err(Error::InvalidArgument("zero render dimension"));
    }
    if width.saturating_mul(height) > MAX_RENDER_PIXELS {
        return Err(Error::LimitExceeded("render target too large"));
    }
    let width = u32::try_from(width).map_err(|_| Error::LimitExceeded("render width too large"))?;
    let height =
        u32::try_from(height).map_err(|_| Error::LimitExceeded("render height too large"))?;

    let mut canvas = Canvas::blank(width, height, base, opts.colorspace, opts.alpha)?;
    if !opts.alpha {
        canvas.fill_background([255, 255, 255, 255]);
    }
    Ok(canvas)
}

/// The render scale matrix from `opts` (DPI overrides matrix, PyMuPDF semantics).
fn render_scale(opts: &RenderOptions) -> Matrix {
    match opts.dpi {
        Some(dpi) => {
            let s = f64::from(dpi) / 72.0;
            Matrix::scale(s, s)
        }
        None => opts.matrix,
    }
}

// === replay ===============================================================

/// Maximum Type3 glyph-procedure recursion depth (a Type3 glyph whose proc
/// shows more Type3 text). Guards against runaway / cyclic CharProcs.
const MAX_TYPE3_DEPTH: u32 = 8;

/// The context an op stream is replayed under.
///
/// The page renders with the defaults ([`RenderCtx::page`]); a Type3 glyph
/// procedure recurses with an `extra_ctm` (the glyph-space → user-space mapping,
/// `FontMatrix · Trm`) composed on top of every op's geometry, an incremented
/// `depth` (recursion guard), and — for an **uncolored** (`d1`) glyph — a
/// `fill_override` that forces every fill to the text's current fill color
/// (`d1` glyphs take their color from the text state, not the proc).
#[derive(Clone, Copy)]
struct RenderCtx {
    /// Glyph-space → user-space matrix to compose on top of each op's geometry
    /// (identity for the page; `FontMatrix · Trm` inside a Type3 glyph proc).
    extra_ctm: Matrix,
    /// Forced fill color (`Some` for an uncolored `d1` Type3 glyph), else the
    /// op's own color is used.
    fill_override: Option<u32>,
    /// Current Type3 glyph-proc recursion depth (0 at the page).
    depth: u32,
}

impl RenderCtx {
    /// The page-level context: no extra transform, no color override, depth 0.
    fn page() -> Self {
        RenderCtx {
            extra_ctm: Matrix::IDENTITY,
            fill_override: None,
            depth: 0,
        }
    }
}

/// Replays an ordered [`RenderOp`] stream onto `canvas`, maintaining the full
/// graphics state (clip stack via the canvas's own `save`/`restore`).
///
/// `ctx` carries the Type3 glyph-proc transform / color override / recursion
/// depth (see [`RenderCtx`]); the page passes [`RenderCtx::page`].
fn replay(
    canvas: &mut Canvas,
    doc: &DocumentStore,
    ops: &[RenderOp],
    ctx: &RenderCtx,
) -> Result<()> {
    // Per-stream font program cache: the font dict identity (its BaseFont + a
    // hash of the FontFile length) is awkward to key, so cache by the program
    // bytes' pointer is impossible across clones — instead cache the *resolved
    // program bytes* keyed by a small fingerprint. We key on the font dict's
    // BaseFont + FontDescriptor object identity is not available; use a Vec of
    // (fingerprint, bytes) so repeated runs of the same font reuse the parse.
    let mut font_cache: FontCache = FontCache::new();

    for op in ops {
        match op {
            RenderOp::Save => canvas.save(),
            RenderOp::Restore => canvas.restore(),
            RenderOp::Fill {
                items,
                close,
                color,
                alpha,
                even_odd,
            } => {
                let color = ctx.fill_override.unwrap_or(*color);
                let paint = Paint::from_rgb_alpha(color, *alpha);
                // Geometry already carries the CTM (interpreter applies it);
                // `ctx.extra_ctm` adds the Type3 glyph-space → user-space map
                // (identity at the page level).
                fill_items(canvas, items, *close, paint, ctx.extra_ctm, *even_odd)?;
            }
            RenderOp::Stroke {
                items,
                close,
                color,
                alpha,
                width,
                ctm,
                dashes,
            } => {
                let color = ctx.fill_override.unwrap_or(*color);
                let paint = Paint::from_rgb_alpha(color, *alpha);
                let eff_ctm = Matrix::concat(ctm, &ctx.extra_ctm);
                let dev_scale = scale_for_ctm(eff_ctm, canvas.base_transform());
                let style = stroke_style(*width, dev_scale, dashes);
                stroke_items(canvas, items, *close, paint, &style, ctx.extra_ctm)?;
            }
            RenderOp::Clip { items, even_odd } => {
                set_clip(canvas, items, ctx.extra_ctm, *even_odd)?;
            }
            RenderOp::Text(run) => draw_text(canvas, doc, run, &mut font_cache, ctx)?,
            RenderOp::Image(img) => draw_image_op(canvas, doc, img)?,
            RenderOp::Shading(sh) => draw_shading_op(canvas, doc, sh)?,
        }
    }
    Ok(())
}

/// Builds a device-space [`StrokeStyle`] from a user-space line width, the CTM's
/// average device scale, and the dash string (`"[a b …] phase"`).
fn stroke_style(width: f64, dev_scale: f32, dashes: &str) -> StrokeStyle {
    // A zero width is a 1-device-pixel hairline (PDF + tiny-skia semantics).
    let w = if width <= 0.0 {
        1.0
    } else {
        (width as f32 * dev_scale).max(f32::MIN_POSITIVE)
    };
    let (dash_array, dash_phase) = parse_dashes(dashes, dev_scale);
    StrokeStyle {
        width: w,
        cap: LineCapStyle::Butt,
        join: LineJoinStyle::Miter,
        miter_limit: 10.0,
        dash_array,
        dash_phase,
    }
}

/// Parses a `"[a b c] phase"` dash string into device-scaled `(array, phase)`.
fn parse_dashes(dashes: &str, dev_scale: f32) -> (Vec<f32>, f32) {
    let s = dashes.trim();
    if s.is_empty() {
        return (Vec::new(), 0.0);
    }
    let Some(open) = s.find('[') else {
        return (Vec::new(), 0.0);
    };
    let Some(close) = s.find(']') else {
        return (Vec::new(), 0.0);
    };
    if close <= open {
        return (Vec::new(), 0.0);
    }
    let inside = &s[open + 1..close];
    let array: Vec<f32> = inside
        .split_whitespace()
        .filter_map(|t| t.parse::<f32>().ok())
        .map(|v| (v * dev_scale).max(0.0))
        .collect();
    // A zero-length / all-zero dash array means solid.
    if array.is_empty() || array.iter().all(|&v| v == 0.0) {
        return (Vec::new(), 0.0);
    }
    let phase = s[close + 1..]
        .split_whitespace()
        .next()
        .and_then(|t| t.parse::<f32>().ok())
        .unwrap_or(0.0)
        * dev_scale;
    (array, phase.max(0.0))
}

// === text =================================================================

/// A small per-page cache of parsed embedded font programs.
///
/// Keyed by the resolved program bytes (a font dict cloned per show op does not
/// preserve identity, but the underlying program bytes are identical, so we key
/// on a cheap fingerprint of `(len, first/last bytes)`). On a miss we resolve the
/// `/FontFile*` program and parse it once; the bytes are stored so the `GlyphFont`
/// borrow stays valid for the whole replay.
struct FontCache {
    /// `(fingerprint, program bytes)` — the parsed `GlyphFont` borrows `bytes`.
    entries: Vec<(u64, Box<[u8]>)>,
    /// `fingerprint → Some(index into entries)` or `None` (no usable program).
    index: HashMap<u64, Option<usize>>,
    /// Per-`(entry index, glyph id)` extracted outline path. A glyph appears
    /// many times on a page; extracting + building its [`tiny_skia::Path`] is
    /// done once and reused for every occurrence. `None` caches a
    /// whitespace/missing/degenerate glyph (no outline) so it is not re-probed.
    glyph_paths: HashMap<(usize, u16), Option<tiny_skia::Path>>,
}

impl FontCache {
    fn new() -> Self {
        FontCache {
            entries: Vec::new(),
            index: HashMap::new(),
            glyph_paths: HashMap::new(),
        }
    }
}

/// Draws a [`TextRun`]: resolve the font program, then paint each glyph via the
/// font-aware M6b pipeline. A non-embedded / unparseable font is skipped (text
/// stays extractable; documented gap).
fn draw_text(
    canvas: &mut Canvas,
    doc: &DocumentStore,
    run: &TextRun,
    cache: &mut FontCache,
    ctx: &RenderCtx,
) -> Result<()> {
    if run.glyphs.is_empty() {
        return Ok(());
    }
    // Type3 fonts draw each glyph as a content-stream procedure (CharProc),
    // recursively, rather than rasterizing an outline program.
    if is_type3(&run.font_dict) {
        return draw_type3_run(canvas, doc, run, ctx);
    }
    // Resolve + cache the embedded font program bytes.
    let program = match resolve_font_program(doc, &run.font_dict) {
        Some(p) => p,
        None => return Ok(()), // non-embedded / Type1 / Type3: no outline pipeline.
    };
    let fp = fingerprint(&program);
    let idx = match cache.index.get(&fp) {
        Some(slot) => *slot,
        None => {
            // Parse once to validate; store bytes so the Face borrow lives.
            let slot = if GlyphFont::from_program(&program, 0).is_ok() {
                cache.entries.push((fp, program.into_boxed_slice()));
                Some(cache.entries.len() - 1)
            } else {
                None
            };
            cache.index.insert(fp, slot);
            slot
        }
    };
    let Some(idx) = idx else {
        return Ok(()); // unparseable program (e.g. bare Type1): documented gap.
    };

    // Split-borrow the cache: `font` borrows `entries[idx]` immutably while we
    // mutate `glyph_paths` (disjoint fields, so both borrows are valid at once).
    let FontCache {
        entries,
        glyph_paths,
        ..
    } = cache;
    let font = match GlyphFont::from_program(&entries[idx].1, 0) {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    let upem = f64::from(font.units_per_em());

    // The stroke paint (for stroked text render modes) + device width.
    let stroke_paint = Paint::from_rgb_alpha(run.stroke_color, run.fill_alpha);
    let dev_scale = scale_for_ctm(run.ctm, canvas.base_transform());
    let stroke = StrokeStyle {
        width: (run.stroke_width as f32 * dev_scale).max(f32::MIN_POSITIVE),
        ..StrokeStyle::default()
    };
    // One reusable paint for the whole run: each glyph only updates its color,
    // avoiding a fresh paint/shader allocation per glyph occurrence.
    let mut scratch = tiny_skia::Paint::default();

    for (glyph, &gid) in run.glyphs.iter().zip(run.gids.iter()) {
        // GID resolution:
        // - Simple (non-CID) fonts: `FontMapper::gid` returns the *code*, which is
        //   not the program glyph id. Resolve via the embedded program's `cmap`
        //   using the glyph's Unicode (the common WinAnsi/Standard simple case).
        // - Type0/CID fonts: the supplied `gid` is the CIDToGIDMap-resolved glyph
        //   id (Identity CID programs usually have no usable cmap), so use it.
        let resolved = resolve_gid(&font, glyph, gid);
        // The glyph outline is extracted once per `(font, gid)` and reused for
        // every later occurrence (the per-render glyph-Path cache).
        let path = glyph_paths
            .entry((idx, resolved))
            .or_insert_with(|| font.outline_path(resolved));
        let Some(path) = path.as_ref() else {
            continue; // whitespace / missing / degenerate glyph: no draw.
        };
        crate::text::draw_glyph_path(
            canvas,
            glyph,
            path,
            upem,
            stroke_paint,
            &stroke,
            run.ctm,
            &mut scratch,
        );
    }
    Ok(())
}

// === Type3 glyph rendering ================================================

/// Whether a font dict is `/Subtype /Type3`.
fn is_type3(font_dict: &Dict) -> bool {
    font_dict
        .get(&Name::new("Subtype"))
        .and_then(Object::as_name)
        .and_then(Name::as_str)
        == Some("Type3")
}

/// Renders a [`TextRun`] painted in a Type3 font: each shown code resolves to a
/// glyph name (`/Encoding /Differences`), whose `/CharProcs` content stream is
/// interpreted and replayed onto `canvas`, transformed from glyph space to
/// device by `/FontMatrix · Trm` (the per-glyph text-rendering matrix).
///
/// The proc draws with the Type3 font's own `/Resources`. A `d0` glyph is
/// *colored* (the proc sets its own color); a `d1` glyph is *uncolored* (it
/// paints in the text's current fill color — applied via a fill override). A
/// missing / unresolvable CharProc draws nothing (never an error). Recursion is
/// depth-guarded ([`MAX_TYPE3_DEPTH`]).
fn draw_type3_run(
    canvas: &mut Canvas,
    doc: &DocumentStore,
    run: &TextRun,
    ctx: &RenderCtx,
) -> Result<()> {
    if ctx.depth >= MAX_TYPE3_DEPTH {
        return Ok(()); // runaway / cyclic CharProc: stop.
    }
    // Render mode 3 (invisible) paints nothing.
    if run.render_mode == 3 {
        return Ok(());
    }

    let font_matrix = type3_font_matrix(doc, &run.font_dict);
    let Some(char_procs) = type3_dict_key(doc, &run.font_dict, "CharProcs") else {
        return Ok(()); // no glyph procedures: nothing to draw.
    };
    let differences = type3_differences(doc, &run.font_dict);
    let resources = type3_dict_key(doc, &run.font_dict, "Resources").unwrap_or_default();

    for glyph in &run.glyphs {
        // Resolve the glyph name for this code via /Encoding /Differences.
        let Some(name) = differences.get(&glyph.code).cloned() else {
            continue; // unmapped code: no glyph.
        };
        // Fetch + decode the CharProc content stream.
        let Some(proc_bytes) = resolve_charproc(doc, &char_procs, &name) else {
            continue; // missing CharProc: draw nothing (tolerant).
        };

        // Per-glyph text-rendering matrix (upright approximation): the
        // interpreter keeps only the glyph `origin` (= (0,0)·Trm) and `size`
        // (= Tfs) — rotation/shear are lost (the documented upright-text
        // approximation). Trm ≈ [Tfs 0 0 Tfs ox oy].
        let trm = Matrix::new(
            glyph.size,
            0.0,
            0.0,
            glyph.size,
            glyph.origin.x,
            glyph.origin.y,
        );
        // glyph space → user space → (page) device: FontMatrix · Trm, then any
        // enclosing Type3 extra_ctm.
        let glyph_to_user = Matrix::concat(&Matrix::concat(&font_matrix, &trm), &ctx.extra_ctm);

        // Interpret the CharProc into an ordered op stream (glyph space, the
        // proc's own coordinates; base CTM identity), using the font Resources.
        let proc_ops = interpret_charproc(doc, &proc_bytes, &resources);
        if proc_ops.is_empty() {
            continue;
        }

        // Colored (d0) vs uncolored (d1): a d1 glyph paints in the text's
        // current fill color (override every proc fill); a d0 glyph keeps the
        // colors it sets itself.
        let fill_override = if charproc_is_uncolored(&proc_bytes) {
            Some(glyph.color)
        } else {
            None
        };

        let glyph_ctx = RenderCtx {
            extra_ctm: glyph_to_user,
            fill_override,
            depth: ctx.depth + 1,
        };
        // Isolate the glyph proc's graphics state (it must not leak q/Q or clip
        // into the surrounding text).
        canvas.save();
        replay(canvas, doc, &proc_ops, &glyph_ctx)?;
        canvas.restore();
    }
    Ok(())
}

/// The Type3 `/FontMatrix` (glyph space → text space). Defaults to the standard
/// `[0.001 0 0 0.001 0 0]` (1000-unit glyph space) when absent / malformed.
fn type3_font_matrix(doc: &DocumentStore, font_dict: &Dict) -> Matrix {
    let default = Matrix::new(0.001, 0.0, 0.0, 0.001, 0.0, 0.0);
    let Some(obj) = doc
        .resolve_dict_key(font_dict, &Name::new("FontMatrix"))
        .ok()
        .flatten()
    else {
        return default;
    };
    let Some(arr) = obj.as_array() else {
        return default;
    };
    let v: Vec<f64> = arr.iter().filter_map(Object::as_f64).collect();
    if v.len() < 6 {
        return default;
    }
    Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5])
}

/// Resolves a Type3 font dict key to a (possibly indirect) dict.
fn type3_dict_key(doc: &DocumentStore, font_dict: &Dict, key: &str) -> Option<Dict> {
    doc.resolve_dict_key(font_dict, &Name::new(key))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
}

/// Builds the `code → glyph name` map from `/Encoding /Differences`.
///
/// `/Encoding` is usually an indirect dict carrying `/Differences [code /name …
/// code /name …]`; a run of names after a code increments the code by one each.
fn type3_differences(doc: &DocumentStore, font_dict: &Dict) -> HashMap<u32, Name> {
    let mut map = HashMap::new();
    let Some(enc) = doc
        .resolve_dict_key(font_dict, &Name::new("Encoding"))
        .ok()
        .flatten()
    else {
        return map;
    };
    let Some(enc_dict) = enc.as_dict() else {
        return map;
    };
    let Some(diffs) = enc_dict
        .get(&Name::new("Differences"))
        .and_then(Object::as_array)
    else {
        return map;
    };
    let mut code: u32 = 0;
    for item in diffs {
        match item {
            Object::Integer(n) if *n >= 0 => code = *n as u32,
            Object::Real(r) if *r >= 0.0 => code = *r as u32,
            Object::Name(name) => {
                map.insert(code, name.clone());
                code = code.saturating_add(1);
            }
            _ => {}
        }
    }
    map
}

/// Fetches + decodes the CharProc content stream for `name` from a `/CharProcs`
/// dict. `None` when the entry is absent or not a decodable stream.
fn resolve_charproc(doc: &DocumentStore, char_procs: &Dict, name: &Name) -> Option<Vec<u8>> {
    let obj = doc.resolve_dict_key(char_procs, name).ok().flatten()?;
    let stream = obj.as_stream()?;
    let bytes = doc.decode_stream(stream).ok()?.into_decoded().ok()?;
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Interprets a CharProc content buffer (already decoded) under the Type3 font's
/// `/Resources`, returning the ordered op stream in glyph space (base CTM
/// identity). Wraps the bytes in a synthetic page dict so the existing
/// recording interpreter can run them.
fn interpret_charproc(doc: &DocumentStore, proc_bytes: &[u8], resources: &Dict) -> Vec<RenderOp> {
    // A synthetic single-content "page" whose /Contents is an inline decoded
    // stream and whose /Resources are the font's. The interpreter decodes a
    // Decoded payload verbatim and runs it with an identity base CTM.
    let content_stream = Object::Stream(pdf_core::StreamObj {
        dict: Dict::new(),
        data: pdf_core::StreamData::Decoded(proc_bytes.to_vec().into()),
    });
    let mut page = Dict::new();
    page.insert(Name::new("Contents"), content_stream);
    page.insert(
        Name::new("Resources"),
        Object::Dictionary(resources.clone()),
    );
    interpret_page_render(doc, &page)
}

/// Whether a CharProc is **uncolored** (`d1`) vs colored (`d0`).
///
/// A glyph procedure begins with either `wx wy d0` (colored — the proc sets its
/// own color) or `wx wy llx lly urx ury d1` (uncolored — the glyph is painted in
/// the text's current color). We scan the leading tokens for the first `d0`/`d1`
/// operator. Absent either, treat as colored (use the proc's own colors).
fn charproc_is_uncolored(proc_bytes: &[u8]) -> bool {
    for tok in proc_bytes.split(|b| b.is_ascii_whitespace()) {
        match tok {
            b"d1" => return true,
            b"d0" => return false,
            _ => {}
        }
    }
    false
}

/// Resolves the program glyph id for one positioned glyph.
///
/// `mapper_gid` is what `FontMapper::gid(code)` returned: for a Type0/CID font
/// it is the (CIDToGIDMap-resolved) program glyph id; for a simple font it is the
/// raw character code (not a glyph id). So the resolution order is:
/// 1. If the supplied `mapper_gid` is in range and non-`.notdef`, trust it
///    (the CID path) — but only when it differs from a plausible code so we
///    don't mistake a simple-font code for a gid;
/// 2. else look the glyph up in the program `cmap` by its Unicode scalar (the
///    simple-font path);
/// 3. else fall back to `mapper_gid` clamped into range.
fn resolve_gid(font: &GlyphFont, glyph: &pdf_text::PositionedGlyph, mapper_gid: u32) -> u16 {
    // Prefer a cmap lookup by Unicode (correct for simple WinAnsi/Standard fonts,
    // and harmless for CID fonts whose program also carries a cmap).
    if let Some(ch) = glyph.unicode.chars().next() {
        if let Some(g) = font.glyph_for_char(ch) {
            if g != 0 {
                return g;
            }
        }
    }
    // No cmap hit: use the mapper gid (the Identity-CID path) when it is a valid,
    // non-notdef glyph index.
    let g = u16::try_from(mapper_gid).unwrap_or(0);
    if g != 0 && g < font.num_glyphs() {
        return g;
    }
    0
}

/// A cheap fingerprint of a font program for the per-page parse cache.
fn fingerprint(bytes: &[u8]) -> u64 {
    let len = bytes.len() as u64;
    let head = bytes
        .iter()
        .take(16)
        .fold(0u64, |h, &b| h.wrapping_mul(131).wrapping_add(u64::from(b)));
    let tail = bytes
        .iter()
        .rev()
        .take(16)
        .fold(0u64, |h, &b| h.wrapping_mul(131).wrapping_add(u64::from(b)));
    len ^ head.rotate_left(17) ^ tail.rotate_left(31)
}

/// Resolves a font dict's **embedded** program bytes (`/FontFile2` TrueType or
/// `/FontFile3` OpenType/CFF), decompressing the stream. `None` for a
/// non-embedded font, a bare Type1 `/FontFile` (PFB — not parseable by the
/// outline pipeline), or any resolution failure.
fn resolve_font_program(doc: &DocumentStore, font_dict: &Dict) -> Option<Vec<u8>> {
    let descriptor = font_descriptor(doc, font_dict)?;
    // Prefer FontFile2 (TrueType) / FontFile3 (CFF/OpenType); FontFile (Type1
    // PFB) is not parseable by ttf-parser, so we skip it (documented gap).
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

/// Resolves a font dict's `/FontDescriptor`, following the descendant CIDFont for
/// a Type0 composite font.
fn font_descriptor(doc: &DocumentStore, font_dict: &Dict) -> Option<Dict> {
    if let Some(d) = doc
        .resolve_dict_key(font_dict, &Name::new("FontDescriptor"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
    {
        return Some(d);
    }
    // Type0: descend into /DescendantFonts[0].
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

// === images ===============================================================

/// Draws an [`ImageOp`]: decode the image stream (XObject or inline) to a
/// [`Pixmap`] (honoring `/SMask`), or paint a stencil `/ImageMask` with the
/// current fill color. Decode failures are swallowed (the §8.4.1 degradation
/// contract — a broken image never aborts the page render).
fn draw_image_op(canvas: &mut Canvas, doc: &DocumentStore, img: &ImageOp) -> Result<()> {
    if img.alpha == 0 {
        return Ok(());
    }
    let is_mask = dict_bool(&img.dict, "ImageMask") || dict_bool(&img.dict, "IM");

    let decoded = match decode_image_stream(doc, &img.dict, &img.raw) {
        Ok(d) => d,
        Err(_) => return Ok(()), // undecodable image: skip, never abort the page.
    };

    if is_mask {
        // Stencil mask: 1-bpp packed bits, painted with the fill color.
        if decoded.width == 0 || decoded.height == 0 {
            return Ok(());
        }
        let paint = Paint::from_rgb(img.fill_color);
        let _ = draw_image_mask(
            canvas,
            &decoded.data,
            decoded.width,
            decoded.height,
            paint,
            img.ctm,
            img.alpha,
        );
        return Ok(());
    }

    // Regular image → Pixmap, optionally with an /SMask soft-mask alpha plane.
    let mut pix = match Pixmap::from_decoded(&decoded) {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    if let Some((mask, mw, mh)) = resolve_smask(doc, &img.dict) {
        if let Ok(p) = pix.clone().with_smask_gray(&mask, mw, mh) {
            pix = p;
        }
    }
    let _ = draw_image(canvas, &pix, img.ctm, img.alpha);
    Ok(())
}

/// Resolves a `/SMask` to an 8-bit gray alpha plane `(bytes, w, h)`.
fn resolve_smask(doc: &DocumentStore, dict: &Dict) -> Option<(Vec<u8>, u32, u32)> {
    let r = dict
        .get(&Name::new("SMask"))
        .and_then(Object::as_reference)?;
    let obj = doc.resolve(r).ok()?;
    let stream = obj.as_stream()?;
    let raw = doc.stream_raw_bytes(stream).ok()?;
    let decoded = decode_image_stream(doc, &stream.dict, &raw).ok()?;
    if decoded.components != 1 {
        return None;
    }
    let pix = Pixmap::from_decoded(&decoded).ok()?;
    if pix.colorspace != Colorspace::Gray {
        return None;
    }
    Some((pix.samples().to_vec(), pix.width, pix.height))
}

// === shadings =============================================================

/// Draws a [`ShadingOp`] (`sh` operator): parse the `/Shading` dict (types 2 & 3:
/// axial / radial) and paint the gradient. Other shading types (1, 4–7) are a
/// documented deferral (skipped, never an error).
fn draw_shading_op(canvas: &mut Canvas, doc: &DocumentStore, sh: &ShadingOp) -> Result<()> {
    let dict = &sh.dict;
    let stype = dict
        .get(&Name::new("ShadingType"))
        .and_then(Object::as_i64)
        .unwrap_or(0);
    let cs = shading_colorspace(doc, dict);
    let Some(func_obj) = dict.get(&Name::new("Function")) else {
        return Ok(());
    };
    let func = match resolve_function(doc, func_obj) {
        Some(f) => f,
        None => return Ok(()),
    };
    let extend = shading_extend(dict);
    let coords: Vec<f64> = dict
        .get(&Name::new("Coords"))
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_f64).collect())
        .unwrap_or_default();

    match stype {
        2 if coords.len() >= 4 => {
            let _ = draw_axial_shading(
                canvas,
                (coords[0], coords[1]),
                (coords[2], coords[3]),
                &func,
                cs,
                extend,
                sh.ctm,
                sh.alpha,
            );
        }
        3 if coords.len() >= 6 => {
            let _ = draw_radial_shading(
                canvas,
                (coords[0], coords[1], coords[2]),
                (coords[3], coords[4], coords[5]),
                &func,
                cs,
                extend,
                sh.ctm,
                sh.alpha,
            );
        }
        _ => {} // types 1 / 4–7: deferred (documented gap).
    }
    Ok(())
}

/// The shading colorspace (Gray / RGB / CMYK; defaults to RGB for arrays /
/// unknown).
fn shading_colorspace(doc: &DocumentStore, dict: &Dict) -> Colorspace {
    let cs = doc
        .resolve_dict_key(dict, &Name::new("ColorSpace"))
        .ok()
        .flatten();
    match cs.as_deref() {
        Some(Object::Name(n)) => name_to_colorspace(n.as_str().unwrap_or("")),
        Some(Object::Array(a)) => a
            .first()
            .and_then(Object::as_name)
            .and_then(Name::as_str)
            .map(name_to_colorspace)
            .unwrap_or(Colorspace::Rgb),
        _ => Colorspace::Rgb,
    }
}

fn name_to_colorspace(name: &str) -> Colorspace {
    match name {
        "DeviceGray" | "CalGray" | "G" => Colorspace::Gray,
        "DeviceCMYK" | "CMYK" => Colorspace::Cmyk,
        _ => Colorspace::Rgb,
    }
}

/// The `/Extend [e0 e1]` pair (default `[false false]`).
fn shading_extend(dict: &Dict) -> (bool, bool) {
    let arr = dict.get(&Name::new("Extend")).and_then(Object::as_array);
    match arr {
        Some(a) if a.len() >= 2 => (
            a[0].as_bool().unwrap_or(false),
            a[1].as_bool().unwrap_or(false),
        ),
        _ => (false, false),
    }
}

/// Resolves a `/Function` object (a dict, a stream, or an array of single-output
/// functions) into a [`PdfFunction`]. Supports types 0 (sampled), 2
/// (exponential) and 3 (stitching); type 4 (PostScript) is deferred (`None`).
fn resolve_function(doc: &DocumentStore, obj: &Object) -> Option<PdfFunction> {
    let obj = resolve_obj(doc, obj)?;
    match obj.as_ref() {
        Object::Array(arr) => {
            // An array of 1-output functions: combine into one multi-output
            // function by sampling each at eval-time. We approximate by wrapping
            // each as a sub-function and building a synthetic stitching-free
            // combiner — but the renderer's `PdfFunction` is single-valued per
            // call, so we instead merge n single-output exponentials/sampled into
            // one exponential when all are type 2, else take the first.
            combine_function_array(doc, arr)
        }
        Object::Dictionary(d) => function_from_dict(doc, d, None),
        Object::Stream(s) => {
            let data = doc.decode_stream(s).and_then(|o| o.into_decoded()).ok();
            function_from_dict(doc, &s.dict, data.as_deref())
        }
        _ => None,
    }
}

/// Combines an array of single-output `/Function`s into one multi-output
/// [`PdfFunction`]. Common for separation/RGB ramps `[f_r f_g f_b]`. When every
/// element is a type-2 exponential we merge into one exponential whose `c0`/`c1`
/// concatenate the per-channel endpoints (the typical case); otherwise we fall
/// back to the first function.
fn combine_function_array(doc: &DocumentStore, arr: &[Object]) -> Option<PdfFunction> {
    if arr.is_empty() {
        return None;
    }
    let funcs: Vec<PdfFunction> = arr
        .iter()
        .filter_map(|o| resolve_function(doc, o))
        .collect();
    if funcs.is_empty() {
        return None;
    }
    // Merge a vector of type-2 exponentials into one (concatenated outputs).
    let mut c0 = Vec::new();
    let mut c1 = Vec::new();
    let mut domain = [0.0f32, 1.0];
    let mut n = 1.0f32;
    let mut all_exp = true;
    for f in &funcs {
        if let PdfFunction::Exponential {
            domain: d,
            c0: a,
            c1: b,
            n: e,
        } = f
        {
            domain = *d;
            n = *e;
            c0.extend_from_slice(a);
            c1.extend_from_slice(b);
        } else {
            all_exp = false;
            break;
        }
    }
    if all_exp && !c0.is_empty() {
        return Some(PdfFunction::Exponential { domain, c0, c1, n });
    }
    funcs.into_iter().next()
}

/// Builds a [`PdfFunction`] from a function dict + optional decoded stream data
/// (for a type-0 sampled function). `None` for type 4 or a malformed dict.
fn function_from_dict(doc: &DocumentStore, d: &Dict, data: Option<&[u8]>) -> Option<PdfFunction> {
    let ftype = d.get(&Name::new("FunctionType")).and_then(Object::as_i64)?;
    let domain = read_pair(d, "Domain").unwrap_or([0.0, 1.0]);
    match ftype {
        2 => {
            let c0 = read_floats(d, "C0").unwrap_or_else(|| vec![0.0]);
            let c1 = read_floats(d, "C1").unwrap_or_else(|| vec![1.0]);
            let n = d
                .get(&Name::new("N"))
                .and_then(Object::as_f64)
                .unwrap_or(1.0) as f32;
            Some(PdfFunction::Exponential { domain, c0, c1, n })
        }
        3 => {
            let sub = d.get(&Name::new("Functions")).and_then(Object::as_array)?;
            let functions: Vec<PdfFunction> = sub
                .iter()
                .filter_map(|o| resolve_function(doc, o))
                .collect();
            if functions.is_empty() {
                return None;
            }
            let bounds = read_floats(d, "Bounds").unwrap_or_default();
            let encode = read_pairs(d, "Encode");
            Some(PdfFunction::Stitching {
                domain,
                functions,
                bounds,
                encode,
            })
        }
        0 => {
            let data = data?;
            let size = d
                .get(&Name::new("Size"))
                .and_then(Object::as_array)
                .and_then(|a| a.first())
                .and_then(Object::as_i64)
                .map(|v| v.max(0) as usize)?;
            let bits_per_sample = d
                .get(&Name::new("BitsPerSample"))
                .and_then(Object::as_i64)? as u8;
            let decode = read_pairs(d, "Decode");
            let n_outputs = if decode.is_empty() { 1 } else { decode.len() };
            let encode = read_pair(d, "Encode").unwrap_or([0.0, (size.max(1) - 1) as f32]);
            let decode = if decode.is_empty() {
                vec![[0.0, 1.0]; n_outputs]
            } else {
                decode
            };
            Some(PdfFunction::Sampled {
                domain,
                size,
                bits_per_sample,
                n_outputs,
                encode,
                decode,
                samples: data.to_vec(),
            })
        }
        _ => None, // type 4 (PostScript): deferred.
    }
}

/// Reads a `[lo hi]` pair from a dict key.
fn read_pair(d: &Dict, key: &str) -> Option<[f32; 2]> {
    let a = d.get(&Name::new(key)).and_then(Object::as_array)?;
    if a.len() < 2 {
        return None;
    }
    Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32])
}

/// Reads a flat float array from a dict key.
fn read_floats(d: &Dict, key: &str) -> Option<Vec<f32>> {
    let a = d.get(&Name::new(key)).and_then(Object::as_array)?;
    Some(
        a.iter()
            .filter_map(|o| o.as_f64().map(|v| v as f32))
            .collect(),
    )
}

/// Reads a flat array as consecutive `[lo hi]` pairs.
fn read_pairs(d: &Dict, key: &str) -> Vec<[f32; 2]> {
    let Some(a) = d.get(&Name::new(key)).and_then(Object::as_array) else {
        return Vec::new();
    };
    let flat: Vec<f32> = a
        .iter()
        .filter_map(|o| o.as_f64().map(|v| v as f32))
        .collect();
    flat.chunks_exact(2).map(|c| [c[0], c[1]]).collect()
}

// === small object helpers =================================================

/// Resolves an indirect object reference (else returns the object as-is).
fn resolve_obj(doc: &DocumentStore, obj: &Object) -> Option<std::sync::Arc<Object>> {
    match obj {
        Object::Reference(r) => doc.resolve(*r).ok(),
        other => Some(std::sync::Arc::new(other.clone())),
    }
}

/// A boolean dict value (default false).
fn dict_bool(dict: &Dict, key: &str) -> bool {
    dict.get(&Name::new(key))
        .and_then(Object::as_bool)
        .unwrap_or(false)
}
