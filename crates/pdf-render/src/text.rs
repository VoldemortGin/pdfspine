//! Text rendering — glyph outlines → filled paths (M6b).
//!
//! Module owner: **M6b** (text). Resolves each [`pdf_text::PositionedGlyph`] to a
//! glyph id in a parsed font program, extracts its outline with the `ttf-parser`
//! [`OutlineBuilder`], converts it to a [`tiny_skia::Path`], and fills (and/or
//! strokes) it onto the [`Canvas`] at the device position implied by the glyph's
//! origin + size — NOT a separate glyph-raster crate (PRD §8.11).
//!
//! # Font-program coverage
//!
//! - **Embedded TrueType (`FontFile2`)** and **embedded OpenType/CFF
//!   (`FontFile3`)** parse through `ttf-parser` and rasterize fully.
//! - **Embedded Type1 (`FontFile`, PFB/PFA)** is not parseable by `ttf-parser`,
//!   so it rasterizes through the first-party [`crate::type1`] outliner
//!   (eexec-decrypt + charstring interpreter), feeding the same `PathSink`.
//! - **Type3 fonts** (each glyph is a mini content stream) are not handled in
//!   this outline pipeline; the page driver ([`crate::render`]) renders them by
//!   recursively interpreting each glyph's `/CharProcs` procedure.
//! - **Non-embedded standard-14 fonts** have no bundled outlines in this crate,
//!   so they are not rasterized (documented gap — text stays extractable). No
//!   substitute font is bundled, to avoid license-uncertain assets.
//!
//! # The frozen-signature gap (note for M6d / orchestrator)
//!
//! [`PositionedGlyph`] carries no font-program bytes and the frozen
//! [`draw_glyph`]/[`draw_text_run`] signatures take no font/`DocumentStore`
//! argument, so they cannot by themselves resolve an embedded program. They
//! therefore handle only what is decidable from the glyph alone (mode-3 skip)
//! and otherwise no-op safely. The real outline→pixel pipeline is
//! [`GlyphFont`] + [`draw_glyph_with_font`] / [`draw_text_run_with_font`], which
//! the page driver (M6d) calls once it has resolved the page's font programs.

use std::collections::HashMap;

use pdf_core::geom::{Matrix, Point};
use pdf_text::PositionedGlyph;
use tiny_skia::{FillRule, Paint as SkPaint, PathBuilder, Stroke, Transform};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::canvas::Canvas;
use crate::error::{Error, Result};
use crate::vector::{Paint, StrokeStyle};

/// A parsed embedded font program: sfnt-wrapped, bare CFF, or Adobe Type1.
///
/// - [`Sfnt`](FontProgram::Sfnt): a `ttf-parser` [`Face`] — `FontFile2`
///   TrueType, or `FontFile3` OpenType/CFF (`OTTO` sfnt).
/// - [`Cff`](FontProgram::Cff): a **bare** CFF table — `FontFile3` with
///   `/Subtype /Type1C` (simple) or `/CIDFontType0C` (CID-keyed). These are raw
///   CFF data with no sfnt directory, so `Face::parse` rejects them
///   (`UnknownMagic`); `ttf-parser`'s public `cff` table parses them directly.
/// - [`Type1`](FontProgram::Type1): an eexec-encrypted Adobe **Type1** program —
///   `FontFile` (PFB/PFA). `ttf-parser` cannot parse it; the first-party
///   [`crate::type1`] outliner does (PRD-NEXT P4-2).
///
/// All variants are boxed: `Face` / `cff::Table` / `Type1Font` are large structs,
/// so an unboxed enum would size every value to the largest variant (clippy
/// `large_enum_variant`). Each box costs one allocation per font (built once per
/// page replay) — negligible.
enum FontProgram<'a> {
    Sfnt(Box<Face<'a>>),
    Cff(Box<ttf_parser::cff::Table<'a>>),
    /// An eexec-encrypted Adobe **Type1** program (`FontFile`, PFB/PFA), parsed
    /// by the first-party [`crate::type1`] outliner — `ttf-parser` cannot parse
    /// it. Owns its parsed charstrings (it does not borrow `'a`).
    Type1(Box<crate::type1::Type1Font>),
}

/// A parsed embedded font program ready for glyph rasterization.
///
/// Wraps the parsed program (see [`FontProgram`]) plus its design grid size.
/// Construction parses the program once; the per-glyph accessors are cheap. The
/// page driver (M6d) resolves the font dict to its embedded program and builds
/// one of these per font, then renders runs through [`draw_text_run_with_font`].
pub struct GlyphFont<'a> {
    program: FontProgram<'a>,
    /// The design grid size: sfnt `units_per_em`, or `round(1/FontMatrix.sx)`
    /// for bare CFF (default 1000). Never zero.
    upem: u16,
    /// For a **CID-keyed CFF** (`CIDFontType0C`): the `CID → program GID` map
    /// recovered from the CFF charset. A Type0 font with `Identity-H` +
    /// `Identity` `CIDToGIDMap` hands the renderer the *CID* (not a GID), and a
    /// subset CFF's GIDs are renumbered, so the CID must be translated through the
    /// charset. `None` for sfnt programs and for SID (simple) CFF.
    cid_to_gid: Option<HashMap<u16, u16>>,
}

impl<'a> GlyphFont<'a> {
    /// Parses an embedded font program (`FontFile2`/`FontFile3` bytes).
    ///
    /// Tries an sfnt parse first (TrueType / OpenType-CFF); on failure falls back
    /// to a **bare CFF** parse (`FontFile3` `/Type1C` / `/CIDFontType0C`, which
    /// carry no sfnt wrapper). `index` selects a face inside a TrueType/OpenType
    /// collection (0 for the common single-face program; ignored for bare CFF).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if the bytes are none of: a `ttf-parser`-parseable
    /// sfnt, a bare CFF, or an eexec-encrypted Type1 (`FontFile`) program.
    pub fn from_program(data: &'a [u8], index: u32) -> Result<Self> {
        if let Ok(face) = Face::parse(data, index) {
            let upem = face.units_per_em().max(1);
            return Ok(Self {
                program: FontProgram::Sfnt(Box::new(face)),
                upem,
                cid_to_gid: None,
            });
        }
        // Bare CFF: no sfnt directory, so derive the em size from the CFF
        // FontMatrix (`sx = 1/upem`; standard 0.001 → 1000).
        if let Some(cff) = ttf_parser::cff::Table::parse(data) {
            let sx = cff.matrix().sx.abs();
            let upem = if sx.is_finite() && sx > f32::EPSILON {
                (1.0 / sx).round().clamp(1.0, f32::from(u16::MAX)) as u16
            } else {
                1000
            };
            let cid_to_gid = build_cff_cid_map(&cff);
            return Ok(Self {
                program: FontProgram::Cff(Box::new(cff)),
                upem,
                cid_to_gid,
            });
        }
        // Adobe Type1 (`FontFile`, PFB/PFA): eexec-encrypted, not parseable by
        // `ttf-parser`. Parse with the first-party outliner (PRD-NEXT P4-2).
        if let Some(t1) = crate::type1::Type1Font::parse(data) {
            let upem = t1.units_per_em();
            return Ok(Self {
                program: FontProgram::Type1(Box::new(t1)),
                upem,
                cid_to_gid: None,
            });
        }
        Err(Error::Unsupported("text::GlyphFont program"))
    }

    /// The font's design grid size (`units_per_em`); the divisor that scales
    /// glyph-unit outlines to a unit font size. Never zero (clamped to 1).
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.upem.max(1)
    }

    /// The number of glyphs in the program.
    #[must_use]
    pub fn num_glyphs(&self) -> u16 {
        match &self.program {
            FontProgram::Sfnt(face) => face.number_of_glyphs(),
            FontProgram::Cff(cff) => cff.number_of_glyphs(),
            FontProgram::Type1(t1) => t1.num_glyphs(),
        }
    }

    /// Looks up the glyph id for a Unicode scalar via the font's `cmap`, if any.
    ///
    /// Only meaningful for sfnt programs (a TrueType/OpenType `cmap` is keyed by
    /// Unicode). Bare CFF returns `None`: its built-in encoding is keyed by the
    /// 1-byte font *code*, not Unicode, so a Unicode lookup would mis-resolve —
    /// the caller uses the name path ([`glyph_for_name`](Self::glyph_for_name))
    /// instead.
    #[must_use]
    pub fn glyph_for_char(&self, c: char) -> Option<u16> {
        match &self.program {
            FontProgram::Sfnt(face) => face.glyph_index(c).map(|g| g.0),
            // Bare CFF / Type1 have no Unicode cmap; resolve via the name path.
            FontProgram::Cff(_) | FontProgram::Type1(_) => None,
        }
    }

    /// Looks up the glyph id for a PostScript glyph **name** via the program's
    /// CFF charset / `post` table (`ttf-parser`'s `glyph-names` feature).
    ///
    /// This is the resolution path for **simple** CFF/Type1 fonts (`FontFile3`):
    /// their program usually carries no `cmap`, so a code is mapped to a glyph
    /// name through the PDF `/Encoding` (here approximated by the AGL name of the
    /// glyph's Unicode) and then to a glyph id by name. Returns `None` when the
    /// program exposes no name table or the name is absent.
    #[must_use]
    pub fn glyph_for_name(&self, name: &str) -> Option<u16> {
        match &self.program {
            FontProgram::Sfnt(face) => face.glyph_index_by_name(name).map(|g| g.0),
            FontProgram::Cff(cff) => cff.glyph_index_by_name(name).map(|g| g.0),
            FontProgram::Type1(t1) => t1.glyph_for_name(name),
        }
    }

    /// Looks up the glyph id for a 1-byte character `code` via an embedded
    /// **Type1** program's builtin `/Encoding` (P4-2r). Only Type1 programs
    /// carry a code-keyed builtin encoding here; all other program kinds return
    /// `None` (they resolve by Unicode `cmap` or by AGL name instead).
    #[must_use]
    pub fn glyph_for_code(&self, code: u8) -> Option<u16> {
        match &self.program {
            FontProgram::Type1(t1) => t1.glyph_for_code(code),
            FontProgram::Sfnt(_) | FontProgram::Cff(_) => None,
        }
    }

    /// Whether this program is a **CID-keyed CFF** (`CIDFontType0C`), i.e. it
    /// carries a `CID → GID` charset that PDF CIDs must be translated through.
    #[must_use]
    pub fn is_cid_keyed(&self) -> bool {
        self.cid_to_gid.is_some()
    }

    /// Translates a PDF **CID** to the program **GID** via the CID-keyed CFF
    /// charset. `None` when this is not a CID-keyed CFF, or the CID is absent from
    /// the (subset) charset.
    #[must_use]
    pub fn gid_for_cid(&self, cid: u16) -> Option<u16> {
        self.cid_to_gid.as_ref().and_then(|m| m.get(&cid).copied())
    }

    /// Builds the glyph outline (in font units, y-up) as a [`tiny_skia::Path`].
    ///
    /// Returns `None` for an absent, empty, or degenerate outline (a space
    /// glyph, `.notdef` with no contour, or a bad/unsupported glyph) — the
    /// caller then draws nothing without error.
    ///
    /// `pub(crate)` so the page driver can build the outline once per
    /// `(font, gid)` and cache the resulting [`tiny_skia::Path`] across repeated
    /// occurrences of the same glyph (a glyph appears many times on a page).
    pub(crate) fn outline_path(&self, gid: u16) -> Option<tiny_skia::Path> {
        let mut sink = PathSink {
            builder: PathBuilder::new(),
        };
        // A missing glyph returns None/Err; an empty builder (whitespace)
        // `finish`es to None as well.
        match &self.program {
            FontProgram::Sfnt(face) => {
                face.outline_glyph(GlyphId(gid), &mut sink)?;
            }
            FontProgram::Cff(cff) => {
                cff.outline(GlyphId(gid), &mut sink).ok()?;
            }
            FontProgram::Type1(t1) => {
                if !t1.outline(gid, &mut sink) {
                    return None;
                }
            }
        }
        sink.builder.finish()
    }
}

/// Builds the `CID → GID` map for a **CID-keyed CFF** by inverting the charset
/// (`glyph_cid(gid) = cid`), or `None` for a SID (simple) CFF.
///
/// First probes glyph 0/1: a SID CFF returns `None` from `glyph_cid`, so a
/// simple CFF pays only the probe (no full scan). For a CID CFF it walks every
/// glyph once (subset fonts are small) and records the first GID seen per CID.
fn build_cff_cid_map(cff: &ttf_parser::cff::Table) -> Option<HashMap<u16, u16>> {
    let n = cff.number_of_glyphs();
    let is_cid =
        cff.glyph_cid(GlyphId(0)).is_some() || (n > 1 && cff.glyph_cid(GlyphId(1)).is_some());
    if !is_cid {
        return None;
    }
    let mut map: HashMap<u16, u16> = HashMap::new();
    for gid in 0..n {
        if let Some(cid) = cff.glyph_cid(GlyphId(gid)) {
            map.entry(cid).or_insert(gid);
        }
    }
    (!map.is_empty()).then_some(map)
}

/// Bridges `ttf-parser`'s [`OutlineBuilder`] callbacks into a tiny-skia
/// [`PathBuilder`]. Coordinates are passed through unchanged (font units, y-up);
/// the device transform is applied later at fill time.
struct PathSink {
    builder: PathBuilder,
}

impl OutlineBuilder for PathSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

/// Renders a single positioned glyph onto `canvas`.
///
/// Frozen-signature entry point. Because [`PositionedGlyph`] carries no font
/// program (see the module docs), this resolves only what the glyph alone
/// decides: an invisible glyph (render mode 3) is skipped. With no reachable
/// embedded program it cannot rasterize an outline, so it succeeds without
/// drawing (text stays extractable). Real rasterization goes through
/// [`draw_glyph_with_font`].
///
/// # Errors
///
/// Never errors for well-formed input; the `Result` is kept for signature
/// compatibility with the other draw modules.
pub fn draw_glyph(
    _canvas: &mut Canvas,
    _glyph: &PositionedGlyph,
    _paint: Paint,
    _ctm: Matrix,
) -> Result<()> {
    // No font program is reachable from this signature — nothing to rasterize.
    // (Mode-3 handling lives in `draw_glyph_with_font`; here every code path is
    // a safe no-op.)
    Ok(())
}

/// Renders a run of positioned glyphs onto `canvas`.
///
/// Frozen-signature batched form of [`draw_glyph`]; see its docs and the module
/// note on the font-program gap. Real rasterization goes through
/// [`draw_text_run_with_font`].
///
/// # Errors
///
/// Never errors for well-formed input.
pub fn draw_text_run(
    _canvas: &mut Canvas,
    _glyphs: &[PositionedGlyph],
    _paint: Paint,
    _ctm: Matrix,
) -> Result<()> {
    Ok(())
}

/// Renders one positioned glyph with a resolved font program.
///
/// `gid` is the glyph id in `font` (resolved by the caller via the font's
/// `cmap` / `CIDToGIDMap`). The glyph's font-unit outline is scaled by
/// `glyph.size / units_per_em`, translated to `glyph.origin` (PDF user space),
/// then mapped through `ctm` and the canvas base transform, and painted per the
/// glyph's [`render_mode`](PositionedGlyph::render_mode):
///
/// | `Tr` | action |
/// |------|--------|
/// | 0    | fill |
/// | 1    | stroke |
/// | 2    | fill + stroke |
/// | 3    | invisible (skipped) |
/// | 4    | fill (+ add to clip — clip accumulation is a documented partial) |
/// | 5    | stroke (+ clip) |
/// | 6    | fill + stroke (+ clip) |
/// | 7    | clip only (no paint — documented partial) |
///
/// The fill color is the glyph's own `color`; `paint`/`stroke` supply the
/// stroke color/width (the interpreter does not carry a separate per-glyph
/// stroke color in [`PositionedGlyph`]). Missing/degenerate outlines draw
/// nothing without error.
///
/// # Errors
///
/// Never errors for well-formed input; the `Result` mirrors the sibling modules.
pub fn draw_glyph_with_font(
    canvas: &mut Canvas,
    glyph: &PositionedGlyph,
    gid: u16,
    font: &GlyphFont,
    stroke_paint: Paint,
    stroke: &StrokeStyle,
    ctm: Matrix,
) -> Result<()> {
    // Render mode 3 (and the reserved 8..) paints nothing.
    let mode = glyph.render_mode;
    if mode == 3 {
        return Ok(());
    }
    let do_fill = matches!(mode, 0 | 2 | 4 | 6);
    let do_stroke = matches!(mode, 1 | 2 | 5 | 6);
    // Mode 7 is clip-only: no paint here (clip accumulation is a documented
    // partial — see module docs). Nothing to draw.
    if !do_fill && !do_stroke {
        return Ok(());
    }

    let Some(path) = font.outline_path(gid) else {
        return Ok(()); // whitespace / missing / degenerate glyph: no draw.
    };

    let upem = f64::from(font.units_per_em());
    let Some(transform) = glyph_device_transform(glyph, upem, canvas.base_transform(), ctm) else {
        return Ok(()); // singular transform (zero size / collapsed ctm): no draw.
    };
    let mut scratch = SkPaint::default();
    draw_glyph_path(
        canvas,
        glyph,
        &path,
        transform,
        stroke_paint,
        stroke,
        &mut scratch,
    );
    Ok(())
}

/// Paints a **prebuilt** glyph outline `path` (font units, y-up) at a
/// **precomputed** device `transform` — the hot inner step the page driver calls
/// for every glyph occurrence with a cached [`tiny_skia::Path`] (so the outline
/// is extracted once per `(font, gid)`, not per occurrence) and a per-glyph
/// font-unit → device transform (see [`glyph_transform_from_trm`] /
/// [`glyph_device_transform`]).
///
/// `scratch` is a reusable [`SkPaint`] the caller keeps across glyphs so each
/// glyph does not allocate a fresh paint/shader; only its color is updated here.
/// The render-mode logic mirrors [`draw_glyph_with_font`].
pub(crate) fn draw_glyph_path(
    canvas: &mut Canvas,
    glyph: &PositionedGlyph,
    path: &tiny_skia::Path,
    transform: Transform,
    stroke_paint: Paint,
    stroke: &StrokeStyle,
    scratch: &mut SkPaint<'static>,
) {
    let mode = glyph.render_mode;
    if mode == 3 {
        return;
    }
    let do_fill = matches!(mode, 0 | 2 | 4 | 6);
    let do_stroke = matches!(mode, 1 | 2 | 5 | 6);
    if !do_fill && !do_stroke {
        return;
    }

    let pixmap = canvas.pixmap_mut();
    scratch.anti_alias = true;

    if do_fill {
        let [r, g, b] = unpack_rgb(glyph.color);
        scratch.set_color_rgba8(r, g, b, 0xFF);
        pixmap.fill_path(path, scratch, FillRule::Winding, transform, None);
    }

    if do_stroke {
        let [r, g, b, a] = stroke_paint.rgba;
        scratch.set_color_rgba8(r, g, b, a);
        let sk_stroke = Stroke {
            width: stroke.width.max(f32::MIN_POSITIVE),
            ..Stroke::default()
        };
        pixmap.stroke_path(path, scratch, &sk_stroke, transform, None);
    }
}

/// Renders a run of glyphs that share one resolved font program.
///
/// `gids[i]` is the glyph id (in `font`) for `glyphs[i]`; the two slices must be
/// the same length (mismatched-length input renders the common prefix). Each
/// glyph is positioned by its own `origin`/`size`, so the run "advances"
/// naturally; the font program is borrowed once for the whole run.
///
/// # Errors
///
/// Never errors for well-formed input.
pub fn draw_text_run_with_font(
    canvas: &mut Canvas,
    glyphs: &[PositionedGlyph],
    gids: &[u16],
    font: &GlyphFont,
    stroke_paint: Paint,
    stroke: &StrokeStyle,
    ctm: Matrix,
) -> Result<()> {
    for (glyph, &gid) in glyphs.iter().zip(gids.iter()) {
        draw_glyph_with_font(canvas, glyph, gid, font, stroke_paint, stroke, ctm)?;
    }
    Ok(())
}

/// Builds the font-unit → device-pixel [`Transform`] from a glyph's **full
/// text-rendering matrix** `trm` (`= params · Tm · CTM`, em-space → PDF user
/// space; see [`pdf_text::TextRun::trms`]).
///
/// Composition (a font-unit point, y-up, transformed left-to-right):
/// `scale(1/upem) · trm · base`. Because `trm` carries the *entire* linear part —
/// the CTM / text-matrix scale, horizontal scaling (`Th`), and any rotation /
/// shear — this places and sizes each glyph exactly as the content stream
/// specifies, where the scalar `size`-only path could not. Returns `None` if the
/// transform is non-finite or collapses to zero area.
pub(crate) fn glyph_transform_from_trm(trm: Matrix, upem: f64, base: Matrix) -> Option<Transform> {
    if !upem.is_finite() || upem == 0.0 {
        return None;
    }
    let inv_upem = 1.0 / upem;
    // font-unit -> em space: divide by units_per_em; then trm -> user; then base.
    let full = Matrix::scale(inv_upem, inv_upem) * trm * base;
    finalize_transform(full)
}

/// Builds the font-unit → device-pixel [`Transform`] for one glyph from its
/// axis-aligned `origin` + scalar `size` (the legacy, rotation-free placement).
///
/// Composition (a font-unit point `(gx, gy)`, y-up, transformed left-to-right):
/// `scale(s) · translate(origin) · ctm · base`, where `s = size / units_per_em`.
/// This cannot express the CTM / text-matrix scale (only `Tfs`), so the page
/// driver prefers [`glyph_transform_from_trm`]; this remains the fallback when no
/// `trm` is available. Returns `None` if the transform is non-finite or collapses
/// to zero area.
fn glyph_device_transform(
    glyph: &PositionedGlyph,
    upem: f64,
    base: Matrix,
    ctm: Matrix,
) -> Option<Transform> {
    let s = glyph.size / upem;
    if !s.is_finite() || s == 0.0 {
        return None;
    }
    let Point { x: ox, y: oy } = glyph.origin;
    // font-unit -> user space: scale by `s`, then translate to the origin.
    let placement = Matrix::new(s, 0.0, 0.0, s, ox, oy);
    // user space -> device pixels: ctm, then the canvas base transform.
    let full = placement * ctm * base;
    finalize_transform(full)
}

/// Converts a composed [`Matrix`] into a [`tiny_skia::Transform`], rejecting a
/// non-finite or zero-determinant (collapsed) result (nothing to fill).
fn finalize_transform(full: Matrix) -> Option<Transform> {
    let t = Transform::from_row(
        full.a as f32,
        full.b as f32,
        full.c as f32,
        full.d as f32,
        full.e as f32,
        full.f as f32,
    );
    let finite = t.sx.is_finite()
        && t.ky.is_finite()
        && t.kx.is_finite()
        && t.sy.is_finite()
        && t.tx.is_finite()
        && t.ty.is_finite();
    if !finite {
        return None;
    }
    // Reject a collapsed (zero-determinant) linear part: nothing to fill.
    let det = t.sx * t.sy - t.kx * t.ky;
    if det == 0.0 {
        return None;
    }
    Some(t)
}

/// Unpacks a `0x00RRGGBB` sRGB color into `[r, g, b]`.
#[inline]
fn unpack_rgb(rgb: u32) -> [u8; 3] {
    [
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
    ]
}

#[cfg(test)]
mod tests {
    //! `RENDER-TEXT-*` / `RENDER-TEXT-PROP-*` — the glyph-outline → pixel
    //! pipeline. These are unit tests (not in `tests/`) because reading the
    //! rendered buffer needs the `pub(crate)` `Canvas::pixmap()` accessor — the
    //! integration-test crate can only read pixels through `into_pixmap`, which
    //! is M6a's (still-stubbed) responsibility (noted in the M6b report).
    //!
    //! The test font is synthesized in-code (`build_box_ttf`): a structurally
    //! valid TrueType program whose glyph 1 is a single filled box outline. It
    //! is authored here, so it is license-clean and self-contained.

    use super::*;
    use pdf_core::geom::{Point, Rect};
    use pdf_image::pixmap::Colorspace;
    use pdf_text::model::WritingDir;
    use proptest::prelude::*;

    // ----- synthetic test font (real box outline) -------------------------

    const UPEM: u16 = 1000;
    // Glyph 1's box outline, in font units (y-up).
    const BOX_X_MIN: i16 = 100;
    const BOX_Y_MIN: i16 = 0;
    const BOX_X_MAX: i16 = 900;
    const BOX_Y_MAX: i16 = 700;

    fn checksum(data: &[u8]) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < data.len() {
            let mut word = [0u8; 4];
            let n = (data.len() - i).min(4);
            word[..n].copy_from_slice(&data[i..i + n]);
            sum = sum.wrapping_add(u32::from_be_bytes(word));
            i += 4;
        }
        sum
    }

    fn pad4(data: &mut Vec<u8>) {
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
    }

    /// A single simple TrueType glyph: one closed contour, 4 on-curve points
    /// forming the axis-aligned box `[x_min,y_min .. x_max,y_max]`.
    fn box_glyph() -> Vec<u8> {
        let mut g = Vec::new();
        // glyph header.
        g.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours
        g.extend_from_slice(&BOX_X_MIN.to_be_bytes()); // xMin
        g.extend_from_slice(&BOX_Y_MIN.to_be_bytes()); // yMin
        g.extend_from_slice(&BOX_X_MAX.to_be_bytes()); // xMax
        g.extend_from_slice(&BOX_Y_MAX.to_be_bytes()); // yMax
                                                       // endPtsOfContours: last point index = 3.
        g.extend_from_slice(&3u16.to_be_bytes());
        // instructionLength = 0.
        g.extend_from_slice(&0u16.to_be_bytes());
        // 4 points, all on-curve (flag 0x01 = ON_CURVE).
        g.extend(std::iter::repeat_n(0x01u8, 4));
        // x coordinates as i16 deltas (flags use the long form: no x-short, no
        // x-same -> a signed 16-bit delta each).
        // Points CCW: (x_min,y_min) -> (x_max,y_min) -> (x_max,y_max) -> (x_min,y_max).
        let xs = [BOX_X_MIN, BOX_X_MAX, BOX_X_MAX, BOX_X_MIN];
        let ys = [BOX_Y_MIN, BOX_Y_MIN, BOX_Y_MAX, BOX_Y_MAX];
        let mut prev = 0i16;
        for &x in &xs {
            g.extend_from_slice(&(x - prev).to_be_bytes());
            prev = x;
        }
        let mut prev = 0i16;
        for &y in &ys {
            g.extend_from_slice(&(y - prev).to_be_bytes());
            prev = y;
        }
        g
    }

    struct Table {
        tag: [u8; 4],
        data: Vec<u8>,
        checksum: u32,
    }

    fn new_table(tag: [u8; 4], data: Vec<u8>) -> Table {
        let checksum = checksum(&data);
        Table {
            tag,
            data,
            checksum,
        }
    }

    fn pow2_floor(n: u16) -> u16 {
        let mut p = 1u16;
        while p * 2 <= n {
            p *= 2;
        }
        p
    }

    fn log2_floor(n: u16) -> u16 {
        let mut p = 0u16;
        let mut v = n;
        while v > 1 {
            v /= 2;
            p += 1;
        }
        p
    }

    /// Format-4 cmap with a single (3,1) subtable mapping `chars[i]` -> gid i+1.
    fn build_cmap(chars: &[char]) -> Vec<u8> {
        let mut mappings: Vec<(u16, u16)> = chars
            .iter()
            .enumerate()
            .map(|(i, &c)| (c as u16, (i as u16) + 1))
            .collect();
        mappings.sort_by_key(|&(cp, _)| cp);

        let mut end_code = Vec::new();
        let mut start_code = Vec::new();
        let mut id_delta = Vec::new();
        let mut id_range_offset = Vec::new();
        for &(cp, gid) in &mappings {
            end_code.push(cp);
            start_code.push(cp);
            id_delta.push((gid as i32 - cp as i32) as i16);
            id_range_offset.push(0u16);
        }
        end_code.push(0xFFFF);
        start_code.push(0xFFFF);
        id_delta.push(1);
        id_range_offset.push(0);

        let seg_count = end_code.len() as u16;
        let seg_count_x2 = seg_count * 2;
        let search_range = 2 * pow2_floor(seg_count);
        let entry_selector = log2_floor(search_range / 2);
        let range_shift = seg_count_x2 - search_range;

        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes());
        let length_pos = sub.len();
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&seg_count_x2.to_be_bytes());
        sub.extend_from_slice(&search_range.to_be_bytes());
        sub.extend_from_slice(&entry_selector.to_be_bytes());
        sub.extend_from_slice(&range_shift.to_be_bytes());
        for &e in &end_code {
            sub.extend_from_slice(&e.to_be_bytes());
        }
        sub.extend_from_slice(&0u16.to_be_bytes());
        for &s in &start_code {
            sub.extend_from_slice(&s.to_be_bytes());
        }
        for &d in &id_delta {
            sub.extend_from_slice(&d.to_be_bytes());
        }
        for &r in &id_range_offset {
            sub.extend_from_slice(&r.to_be_bytes());
        }
        let sub_len = sub.len() as u16;
        sub[length_pos..length_pos + 2].copy_from_slice(&sub_len.to_be_bytes());

        let mut cmap = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes());
        cmap.extend_from_slice(&1u16.to_be_bytes());
        cmap.extend_from_slice(&3u16.to_be_bytes());
        cmap.extend_from_slice(&1u16.to_be_bytes());
        cmap.extend_from_slice(&12u32.to_be_bytes());
        cmap.extend_from_slice(&sub);
        cmap
    }

    fn assemble_font(tables: &mut [Table]) -> Vec<u8> {
        let num_tables = tables.len() as u16;
        let search_range = pow2_floor(num_tables) * 16;
        let entry_selector = log2_floor(pow2_floor(num_tables));
        let range_shift = num_tables * 16 - search_range;

        let offset_table_len = 12;
        let dir_len = 16 * tables.len();
        let mut running = offset_table_len + dir_len;
        let mut offsets: Vec<u32> = Vec::with_capacity(tables.len());
        for t in tables.iter() {
            offsets.push(running as u32);
            running += t.data.len();
            running += (4 - running % 4) % 4;
        }

        let mut out = Vec::with_capacity(running);
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&num_tables.to_be_bytes());
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(&range_shift.to_be_bytes());
        for (i, t) in tables.iter().enumerate() {
            out.extend_from_slice(&t.tag);
            out.extend_from_slice(&t.checksum.to_be_bytes());
            out.extend_from_slice(&offsets[i].to_be_bytes());
            out.extend_from_slice(&(t.data.len() as u32).to_be_bytes());
        }
        let mut head_offset = 0usize;
        for (i, t) in tables.iter().enumerate() {
            assert_eq!(out.len() as u32, offsets[i]);
            if &t.tag == b"head" {
                head_offset = out.len();
            }
            out.extend_from_slice(&t.data);
            pad4(&mut out);
        }
        let total = checksum(&out);
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(total);
        let pos = head_offset + 8;
        out[pos..pos + 4].copy_from_slice(&adjustment.to_be_bytes());
        out
    }

    /// Builds a structurally valid TrueType font with one real box glyph
    /// (glyph 1), mapping each char in `chars` to glyph ids 1.. via cmap.
    /// Glyph 0 (`.notdef`) is empty; subsequent glyph ids reuse the box outline
    /// so a multi-glyph run all renders the same box shape.
    fn build_box_ttf(chars: &[char]) -> Vec<u8> {
        let num_glyphs: u16 = (chars.len() as u16) + 1;
        let advance: u16 = 1000;

        // glyf: glyph 0 empty; glyphs 1.. each a box.
        let one = box_glyph();
        let mut glyf: Vec<u8> = Vec::new();
        let mut loca_offsets: Vec<u32> = vec![0]; // start of glyph 0.
                                                  // glyph 0 empty -> next offset equals current (0).
        loca_offsets.push(glyf.len() as u32);
        for _ in 1..num_glyphs {
            glyf.extend_from_slice(&one);
            // glyf entries must be 2-byte aligned.
            if !glyf.len().is_multiple_of(2) {
                glyf.push(0);
            }
            loca_offsets.push(glyf.len() as u32);
        }

        // loca: use long format (indexToLocFormat = 1) to avoid /2 constraints.
        let mut loca = Vec::new();
        for &o in &loca_offsets {
            loca.extend_from_slice(&o.to_be_bytes());
        }

        let mut head = Vec::new();
        head.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        head.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        head.extend_from_slice(&0u32.to_be_bytes()); // checkSumAdjustment
        head.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        head.extend_from_slice(&0u16.to_be_bytes()); // flags
        head.extend_from_slice(&UPEM.to_be_bytes());
        head.extend_from_slice(&0i64.to_be_bytes());
        head.extend_from_slice(&0i64.to_be_bytes());
        head.extend_from_slice(&BOX_X_MIN.to_be_bytes());
        head.extend_from_slice(&BOX_Y_MIN.to_be_bytes());
        head.extend_from_slice(&BOX_X_MAX.to_be_bytes());
        head.extend_from_slice(&BOX_Y_MAX.to_be_bytes());
        head.extend_from_slice(&0u16.to_be_bytes()); // macStyle
        head.extend_from_slice(&8u16.to_be_bytes());
        head.extend_from_slice(&2i16.to_be_bytes());
        head.extend_from_slice(&1i16.to_be_bytes()); // indexToLocFormat = long
        head.extend_from_slice(&0i16.to_be_bytes());

        let mut hhea = Vec::new();
        hhea.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        hhea.extend_from_slice(&800i16.to_be_bytes()); // ascender
        hhea.extend_from_slice(&(-200i16).to_be_bytes()); // descender
        hhea.extend_from_slice(&0i16.to_be_bytes()); // lineGap
        hhea.extend_from_slice(&advance.to_be_bytes());
        hhea.extend_from_slice(&0i16.to_be_bytes());
        hhea.extend_from_slice(&0i16.to_be_bytes());
        hhea.extend_from_slice(&(advance as i16).to_be_bytes());
        hhea.extend_from_slice(&1i16.to_be_bytes());
        hhea.extend_from_slice(&0i16.to_be_bytes());
        hhea.extend_from_slice(&0i16.to_be_bytes());
        for _ in 0..4 {
            hhea.extend_from_slice(&0i16.to_be_bytes());
        }
        hhea.extend_from_slice(&0i16.to_be_bytes()); // metricDataFormat
        hhea.extend_from_slice(&num_glyphs.to_be_bytes()); // numberOfHMetrics

        let mut maxp = Vec::new();
        maxp.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        maxp.extend_from_slice(&num_glyphs.to_be_bytes());
        maxp.extend_from_slice(&4u16.to_be_bytes()); // maxPoints
        maxp.extend_from_slice(&1u16.to_be_bytes()); // maxContours
        for _ in 0..11 {
            maxp.extend_from_slice(&0u16.to_be_bytes());
        }

        let mut hmtx = Vec::new();
        for _ in 0..num_glyphs {
            hmtx.extend_from_slice(&advance.to_be_bytes());
            hmtx.extend_from_slice(&0i16.to_be_bytes());
        }

        let cmap = build_cmap(chars);

        let mut post = Vec::new();
        post.extend_from_slice(&0x0003_0000u32.to_be_bytes());
        post.extend_from_slice(&0i32.to_be_bytes());
        post.extend_from_slice(&(-200i16).to_be_bytes());
        post.extend_from_slice(&50i16.to_be_bytes());
        for _ in 0..4 {
            post.extend_from_slice(&0u32.to_be_bytes());
        }
        post.extend_from_slice(&0u32.to_be_bytes());

        let mut tables = vec![
            new_table(*b"cmap", cmap),
            new_table(*b"glyf", glyf),
            new_table(*b"head", head),
            new_table(*b"hhea", hhea),
            new_table(*b"hmtx", hmtx),
            new_table(*b"loca", loca),
            new_table(*b"maxp", maxp),
            new_table(*b"post", post),
        ];
        tables.sort_by_key(|t| t.tag);
        assemble_font(&mut tables)
    }

    // ----- helpers --------------------------------------------------------

    /// A canvas with an identity base transform plus a y-flip so PDF user space
    /// (y-up, bottom-left origin) maps onto the top-left-origin pixmap. For a
    /// `h`-pixel canvas: `(x, y) -> (x, h - y)`.
    fn canvas(w: u32, h: u32) -> Canvas {
        let flip = Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, f64::from(h));
        Canvas::blank(w, h, flip, Colorspace::Rgb, true).unwrap()
    }

    fn glyph_at(origin: Point, size: f64, color: u32, mode: u8) -> PositionedGlyph {
        PositionedGlyph {
            unicode: "A".into(),
            code: u32::from('A'),
            origin,
            // bbox is only metadata for the renderer; an approximate box here.
            bbox: Rect::new(origin.x, origin.y, origin.x + size, origin.y + size),
            font_name: "F1".into(),
            size,
            color,
            render_mode: mode,
            writing_dir: WritingDir::Horizontal,
            advance_dir: (1.0, 0.0),
            ascender: 0.8,
            descender: -0.2,
        }
    }

    /// Counts pixels whose alpha (channel 3 of RGBA) is non-zero.
    fn count_painted(canvas: &Canvas) -> usize {
        canvas
            .pixmap()
            .pixels()
            .iter()
            .filter(|p| p.alpha() != 0)
            .count()
    }

    /// True if device pixel `(x, y)` has been painted (non-zero alpha).
    fn painted_at(canvas: &Canvas, x: u32, y: u32) -> bool {
        let idx = (y * canvas.width() + x) as usize;
        canvas.pixmap().pixels()[idx].alpha() != 0
    }

    // ----- RENDER-TEXT-001: a glyph fills its box region -------------------

    #[test]
    fn render_text_001_glyph_fills_box_region() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();

        let mut cv = canvas(100, 100);
        // size 100 at upem 1000 -> scale 0.1; box (100..900, 0..700) user units
        // -> (10..90, 0..70). Origin at (0, 10) user space (so y in-bounds after
        // the flip). bottom-left origin: user y 10..80 -> device y 20..90.
        let g = glyph_at(Point::new(0.0, 10.0), 100.0, 0x000000, 0);
        draw_glyph_with_font(
            &mut cv,
            &g,
            gid,
            &font,
            Paint::from_rgb(0),
            &StrokeStyle::default(),
            Matrix::IDENTITY,
        )
        .unwrap();

        // Center of the box is painted; a corner well outside is not.
        assert!(count_painted(&cv) > 0, "glyph produced no pixels");
        assert!(painted_at(&cv, 50, 50), "box center should be filled");
        assert!(!painted_at(&cv, 2, 2), "far corner should be empty");
        assert!(!painted_at(&cv, 97, 97), "far corner should be empty");
    }

    // ----- RENDER-TEXT-002: invisible (mode 3) paints nothing -------------

    #[test]
    fn render_text_002_invisible_mode_no_pixels() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();
        let mut cv = canvas(100, 100);
        let g = glyph_at(Point::new(0.0, 10.0), 100.0, 0x000000, 3);
        draw_glyph_with_font(
            &mut cv,
            &g,
            gid,
            &font,
            Paint::from_rgb(0),
            &StrokeStyle::default(),
            Matrix::IDENTITY,
        )
        .unwrap();
        assert_eq!(count_painted(&cv), 0, "mode 3 must paint nothing");
    }

    // ----- RENDER-TEXT-003: fill color is the glyph color -----------------

    #[test]
    fn render_text_003_fill_color_matches_glyph() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();
        let mut cv = canvas(100, 100);
        // Pure red fill.
        let g = glyph_at(Point::new(0.0, 10.0), 100.0, 0x00FF_0000, 0);
        draw_glyph_with_font(
            &mut cv,
            &g,
            gid,
            &font,
            Paint::from_rgb(0),
            &StrokeStyle::default(),
            Matrix::IDENTITY,
        )
        .unwrap();
        // The pixmap is premultiplied; alpha is 0xFF at the center, so the stored
        // red channel equals the source red.
        let idx = (50 * cv.width() + 50) as usize;
        let px = cv.pixmap().pixels()[idx];
        assert_eq!(px.alpha(), 0xFF);
        assert_eq!(px.red(), 0xFF, "red channel");
        assert_eq!(px.green(), 0x00, "green channel");
        assert_eq!(px.blue(), 0x00, "blue channel");
    }

    // ----- RENDER-TEXT-004: size scales the filled region -----------------

    #[test]
    fn render_text_004_size_scales_coverage() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();

        let mut small = canvas(200, 200);
        let gs = glyph_at(Point::new(0.0, 10.0), 50.0, 0x000000, 0);
        draw_glyph_with_font(
            &mut small,
            &gs,
            gid,
            &font,
            Paint::from_rgb(0),
            &StrokeStyle::default(),
            Matrix::IDENTITY,
        )
        .unwrap();

        let mut big = canvas(200, 200);
        let gb = glyph_at(Point::new(0.0, 10.0), 150.0, 0x000000, 0);
        draw_glyph_with_font(
            &mut big,
            &gb,
            gid,
            &font,
            Paint::from_rgb(0),
            &StrokeStyle::default(),
            Matrix::IDENTITY,
        )
        .unwrap();

        let n_small = count_painted(&small);
        let n_big = count_painted(&big);
        assert!(n_small > 0 && n_big > 0);
        // Triple the font size -> ~9x the area; require a clear monotone jump.
        assert!(
            n_big > n_small * 4,
            "larger size must cover more pixels (small={n_small}, big={n_big})"
        );
    }

    // ----- RENDER-TEXT-005: origin positions the glyph --------------------

    #[test]
    fn render_text_005_origin_positions_glyph() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();

        // Two glyphs at different x origins land in different columns.
        let mut cv = canvas(200, 100);
        let left = glyph_at(Point::new(0.0, 10.0), 80.0, 0x000000, 0);
        let right = glyph_at(Point::new(100.0, 10.0), 80.0, 0x000000, 0);
        let stroke = StrokeStyle::default();
        draw_glyph_with_font(
            &mut cv,
            &left,
            gid,
            &font,
            Paint::from_rgb(0),
            &stroke,
            Matrix::IDENTITY,
        )
        .unwrap();
        // Left glyph occupies roughly x in [8, 72]; nothing past x=110 yet.
        assert!(painted_at(&cv, 40, 50), "left glyph center painted");
        let right_before = (10..100).any(|y| painted_at(&cv, 150, y));
        assert!(!right_before, "right column empty before second glyph");

        draw_glyph_with_font(
            &mut cv,
            &right,
            gid,
            &font,
            Paint::from_rgb(0),
            &stroke,
            Matrix::IDENTITY,
        )
        .unwrap();
        let right_after = (10..100).any(|y| painted_at(&cv, 150, y));
        assert!(right_after, "right glyph fills the shifted column");
    }

    // ----- RENDER-TEXT-006: a run advances over multiple glyphs ------------

    #[test]
    fn render_text_006_run_advances() {
        let ttf = build_box_ttf(&['A', 'B', 'C']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let mut cv = canvas(300, 100);

        let glyphs = vec![
            glyph_at(Point::new(0.0, 10.0), 80.0, 0x000000, 0),
            glyph_at(Point::new(90.0, 10.0), 80.0, 0x000000, 0),
            glyph_at(Point::new(180.0, 10.0), 80.0, 0x000000, 0),
        ];
        let gids = vec![
            font.glyph_for_char('A').unwrap(),
            font.glyph_for_char('B').unwrap(),
            font.glyph_for_char('C').unwrap(),
        ];
        draw_text_run_with_font(
            &mut cv,
            &glyphs,
            &gids,
            &font,
            Paint::from_rgb(0),
            &StrokeStyle::default(),
            Matrix::IDENTITY,
        )
        .unwrap();

        // Each glyph paints in its own column band.
        assert!(painted_at(&cv, 40, 50), "glyph 0 column");
        assert!(painted_at(&cv, 130, 50), "glyph 1 column");
        assert!(painted_at(&cv, 220, 50), "glyph 2 column");
    }

    // ----- RENDER-TEXT-007: stroke mode (1) paints with stroke color ------

    #[test]
    fn render_text_007_stroke_mode_uses_stroke_color() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();
        let mut cv = canvas(120, 120);
        // Glyph fill color is red, but mode 1 (stroke only) must use the stroke
        // paint (green), and the box interior stays unpainted.
        let g = glyph_at(Point::new(0.0, 10.0), 100.0, 0x00FF_0000, 1);
        draw_glyph_with_font(
            &mut cv,
            &g,
            gid,
            &font,
            Paint::from_rgb(0x00_00_FF_00),
            &StrokeStyle {
                width: 4.0,
                ..StrokeStyle::default()
            },
            Matrix::IDENTITY,
        )
        .unwrap();
        assert!(count_painted(&cv) > 0, "stroke produced pixels");
        // A painted edge pixel should be green, not red.
        let mut found_green = false;
        for y in 0..cv.height() {
            for x in 0..cv.width() {
                let idx = (y * cv.width() + x) as usize;
                let px = cv.pixmap().pixels()[idx];
                if px.alpha() != 0 {
                    assert_eq!(px.red(), 0, "stroke must not use the fill (red)");
                    if px.green() > 0 {
                        found_green = true;
                    }
                }
            }
        }
        assert!(found_green, "stroke color (green) should appear");
    }

    // ----- RENDER-TEXT-008: clip-only mode (7) paints nothing -------------

    #[test]
    fn render_text_008_clip_only_mode_no_paint() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();
        let mut cv = canvas(100, 100);
        let g = glyph_at(Point::new(0.0, 10.0), 100.0, 0x000000, 7);
        draw_glyph_with_font(
            &mut cv,
            &g,
            gid,
            &font,
            Paint::from_rgb(0),
            &StrokeStyle::default(),
            Matrix::IDENTITY,
        )
        .unwrap();
        assert_eq!(count_painted(&cv), 0, "mode 7 (clip-only) paints nothing");
    }

    // ----- RENDER-TEXT-009: fill+stroke mode (2) paints interior + edge ----

    #[test]
    fn render_text_009_fill_stroke_mode() {
        let ttf = build_box_ttf(&['A']);
        let font = GlyphFont::from_program(&ttf, 0).unwrap();
        let gid = font.glyph_for_char('A').unwrap();
        let mut cv = canvas(120, 120);
        let g = glyph_at(Point::new(0.0, 10.0), 100.0, 0x00FF_0000, 2);
        draw_glyph_with_font(
            &mut cv,
            &g,
            gid,
            &font,
            Paint::from_rgb(0x00_00_00_FF),
            &StrokeStyle {
                width: 3.0,
                ..StrokeStyle::default()
            },
            Matrix::IDENTITY,
        )
        .unwrap();
        // Interior filled with red (the glyph color).
        let idx = (60 * cv.width() + 50) as usize;
        let center = cv.pixmap().pixels()[idx];
        assert_eq!(center.alpha(), 0xFF);
        assert_eq!(center.red(), 0xFF, "interior is the fill color");
    }

    // ----- RENDER-TEXT-010: the frozen no-font entry points are safe no-ops -

    #[test]
    fn render_text_010_frozen_entrypoints_noop() {
        let mut cv = canvas(50, 50);
        let g = glyph_at(Point::new(0.0, 10.0), 20.0, 0x000000, 0);
        draw_glyph(&mut cv, &g, Paint::from_rgb(0), Matrix::IDENTITY).unwrap();
        draw_text_run(&mut cv, &[g], Paint::from_rgb(0), Matrix::IDENTITY).unwrap();
        // No font program is reachable, so nothing is painted (text stays
        // extractable; rasterization needs the font-aware path).
        assert_eq!(count_painted(&cv), 0);
    }

    // ----- RENDER-TEXT-011: the full Trm scales the glyph (CTM-scale fix) ---

    #[test]
    fn render_text_011_trm_carries_ctm_scale() {
        // A PDF that bakes the font size / scale into Tm or the CTM produces a
        // Trm whose linear part is NOT just `Tfs`. e.g. Tfs=10 under a 0.5× CTM
        // gives Trm linear 5. The transform must scale the outline by Trm/upem
        // (5/1000), NOT by size/upem (10/1000) — the regression this guards.
        let upem = 1000.0;
        let base = Matrix::IDENTITY;
        let trm = Matrix::new(5.0, 0.0, 0.0, 5.0, 100.0, 200.0);
        let t = glyph_transform_from_trm(trm, upem, base).expect("finite transform");
        assert!((t.sx - 0.005).abs() < 1e-9, "sx={} (want 0.005)", t.sx);
        assert!((t.sy - 0.005).abs() < 1e-9, "sy={} (want 0.005)", t.sy);
        assert!((t.tx - 100.0).abs() < 1e-6, "tx={}", t.tx);
        assert!((t.ty - 200.0).abs() < 1e-6, "ty={}", t.ty);

        // The legacy `size`-only path (size=10, ctm=identity) ignores the 0.5×
        // CTM and would scale by 0.01 — double. Confirms the two paths diverge,
        // i.e. the Trm path genuinely accounts for the CTM scale.
        let glyph = glyph_at(Point::new(100.0, 200.0), 10.0, 0x000000, 0);
        let legacy =
            glyph_device_transform(&glyph, upem, base, Matrix::IDENTITY).expect("finite legacy");
        assert!(
            (legacy.sx - 0.01).abs() < 1e-9,
            "legacy sx={} (want 0.01)",
            legacy.sx
        );
        assert!(
            (legacy.sx - t.sx).abs() > 1e-6,
            "Trm path must differ from the size-only path"
        );
    }

    // ----- RENDER-TEXT-012: the full Trm carries rotation/shear --------------

    #[test]
    fn render_text_012_trm_carries_rotation() {
        // Trm = scale(5) · rotate(90°) → linear [0, 5, -5, 0]. The transform must
        // reproduce the off-diagonal (rotation) terms that the size-only path
        // (diagonal only) cannot express.
        let upem = 1000.0;
        let base = Matrix::IDENTITY;
        let trm = Matrix::new(0.0, 5.0, -5.0, 0.0, 10.0, 20.0);
        let t = glyph_transform_from_trm(trm, upem, base).expect("finite transform");
        assert!((t.sx - 0.0).abs() < 1e-9, "sx={}", t.sx);
        assert!((t.ky - 0.005).abs() < 1e-9, "ky={}", t.ky);
        assert!((t.kx + 0.005).abs() < 1e-9, "kx={}", t.kx);
        assert!((t.sy - 0.0).abs() < 1e-9, "sy={}", t.sy);
    }

    // ----- RENDER-TEXT-013: degenerate Trm / upem yields no transform --------

    #[test]
    fn render_text_013_trm_degenerate_none() {
        let base = Matrix::IDENTITY;
        // Zero upem: no scale possible.
        assert!(
            glyph_transform_from_trm(Matrix::new(5.0, 0.0, 0.0, 5.0, 0.0, 0.0), 0.0, base)
                .is_none()
        );
        // Collapsed linear part (zero determinant): nothing to fill.
        assert!(
            glyph_transform_from_trm(Matrix::new(0.0, 0.0, 0.0, 0.0, 1.0, 2.0), 1000.0, base)
                .is_none()
        );
    }

    // ----- RENDER-TEXT-PROP-001: missing glyph id never panics / draws -----

    proptest! {
        #[test]
        fn render_text_prop_001_missing_glyph_no_panic(gid in 0u16..5000) {
            let ttf = build_box_ttf(&['A']);
            let font = GlyphFont::from_program(&ttf, 0).unwrap();
            let mut cv = canvas(40, 40);
            let g = glyph_at(Point::new(0.0, 5.0), 30.0, 0x000000, 0);
            // Any gid (including out-of-range / .notdef) must not panic; an
            // absent/empty outline simply paints nothing.
            let r = draw_glyph_with_font(
                &mut cv, &g, gid, &font,
                Paint::from_rgb(0), &StrokeStyle::default(), Matrix::IDENTITY,
            );
            prop_assert!(r.is_ok());
            if gid == 0 || gid >= font.num_glyphs() {
                prop_assert_eq!(count_painted(&cv), 0);
            }
        }

        // ----- RENDER-TEXT-PROP-002: arbitrary size/origin never panics ----
        #[test]
        fn render_text_prop_002_arbitrary_geometry_no_panic(
            size in -1000.0f64..1000.0,
            ox in -500.0f64..500.0,
            oy in -500.0f64..500.0,
            mode in 0u8..16,
        ) {
            let ttf = build_box_ttf(&['A']);
            let font = GlyphFont::from_program(&ttf, 0).unwrap();
            let gid = font.glyph_for_char('A').unwrap();
            let mut cv = canvas(64, 64);
            let g = glyph_at(Point::new(ox, oy), size, 0x0012_3456, mode);
            let r = draw_glyph_with_font(
                &mut cv, &g, gid, &font,
                Paint::from_rgb(0x00_AB_CD_EF), &StrokeStyle { width: 2.0, ..StrokeStyle::default() },
                Matrix::IDENTITY,
            );
            prop_assert!(r.is_ok());
        }

        // ----- RENDER-TEXT-PROP-003: bad font bytes -> Err, never panic ----
        #[test]
        fn render_text_prop_003_bad_program_errors(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            // Random bytes are almost never a valid sfnt; parse must not panic
            // and must return a typed error (or, vanishingly rarely, Ok).
            let _ = GlyphFont::from_program(&bytes, 0);
        }
    }
}
