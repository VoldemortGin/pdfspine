//! Per-render cache of rasterized glyph coverage masks (M6b performance).
//!
//! A page shows the same glyph — same font program, glyph id, device-space 2×2
//! linear map, and sub-pixel phase — hundreds of times. `Pixmap::fill_path`
//! re-runs the full anti-aliased scan conversion + raster pipeline for every
//! occurrence, which dominates text-page render time. This cache rasterizes each
//! *distinct* glyph's coverage once (into a small [`tiny_skia::Mask`] — the
//! identical scan converter `Pixmap::fill_path` uses) and then blits it for
//! every occurrence, replaying tiny-skia's `lowp` premultiplied source-over
//! compositing byte-for-byte.
//!
//! Correctness rests on two tiny-skia 0.11 facts:
//!  * `Mask::fill_path` and `Pixmap::fill_path` both call the same
//!    `scan::path_aa::fill_path`, so a *fresh* mask's `u8` at a pixel equals the
//!    coverage `Pixmap::fill_path` would apply there for the same path+transform.
//!  * an integer device-pixel translation shifts that coverage by the same
//!    integer with identical values (the 4× supersample grid is pixel-aligned).
//!
//! So a glyph rasterized at a *quantized* sub-pixel phase and blitted at an
//! integer pixel offset reproduces `fill_path`'s output to within the phase
//! rounding (≤ 1/8 px). The glyph fill is always an opaque solid color with
//! `FillRule::Winding`, `SourceOver`, anti-aliasing on — so the coverage is
//! colour-independent (the colour is applied only at blit time) and the two
//! composite branches below mirror tiny-skia exactly:
//!  * **no clip** — opaque source + `SourceOver` + no mask is strength-reduced
//!    to `Source`, i.e. a `lerp` toward the destination (a single `div255`);
//!  * **with clip** — stays `SourceOver` (the reduction requires no mask):
//!    `MaskU8` then coverage scale then `s + div255(d·(255−sa))`.

use std::collections::HashMap;

use tiny_skia::{FillRule, Mask, Paint, Pixmap as SkPixmap, Transform};

/// Sub-pixel phase bins per axis (4 ⇒ 1/4-px steps, ≤ 1/8 px positional error).
const PHASE_STEPS: i32 = 4;
/// Skip the cache for glyphs whose device bbox exceeds this many pixels: they
/// are rare and large, so a cached mask would not amortize and would bloat the
/// cache. Such glyphs fall back to a direct `Pixmap::fill_path`.
const MAX_MASK_PIXELS: u32 = 128 * 128;
/// Clear the whole cache once its coverage bytes exceed this (per render). A
/// safety valve for pathological pages; normal text pages stay well under 1 MB.
const MAX_CACHE_BYTES: usize = 24 * 1024 * 1024;

/// Cache key: the font entry index, the resolved glyph id, the device-space 2×2
/// linear part (exact `f32` bit patterns — every glyph in a run shares it, so
/// hit rates are high without any quantization), and the sub-pixel phase.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphMaskKey {
    font_idx: usize,
    gid: u16,
    /// `(sx, ky, kx, sy)` bit patterns — the device linear map (no translation).
    linear: [u32; 4],
    /// `phase_x * PHASE_STEPS + phase_y`, each in `0..PHASE_STEPS`.
    phase: u8,
}

/// A rasterized glyph coverage mask plus its integer device-space bbox origin
/// (relative to the glyph's quantized origin).
struct CachedMask {
    w: u32,
    h: u32,
    ox: i32,
    oy: i32,
    data: Vec<u8>,
}

/// Per-render glyph coverage cache. Lives on the page's `FontCache`, so it is
/// created and dropped with a single `render_page` (Type3 sub-streams get their
/// own, like the outline-path cache).
#[derive(Default)]
pub(crate) struct GlyphMaskCache {
    masks: HashMap<GlyphMaskKey, CachedMask>,
    bytes: usize,
}

/// tiny-skia's `lowp` `div255`: `(v + 255) >> 8` (truncating — matches
/// `src/pipeline/lowp.rs`; **not** the round-to-nearest form used elsewhere).
#[inline]
fn div255(v: u16) -> u16 {
    (v + 255) >> 8
}

impl GlyphMaskCache {
    /// Fills one glyph outline (`path`, in font units, y-up) at device
    /// `transform` in the opaque sRGB colour `rgb`, reusing a cached coverage
    /// mask when the same glyph (font/id/2×2/phase) has been drawn before.
    ///
    /// `key` is the outline's `(font entry index, glyph id)` — the same key the
    /// outline-path cache uses. Oversized or degenerate glyphs fall back to a
    /// direct `Pixmap::fill_path`, so the output is always defined and matches
    /// the previous (uncached) behaviour exactly.
    pub(crate) fn fill_glyph(
        &mut self,
        pixmap: &mut SkPixmap,
        clip: Option<&Mask>,
        path: &tiny_skia::Path,
        transform: Transform,
        key: (usize, u16),
        rgb: [u8; 3],
    ) {
        // Split the device translation into an integer pixel offset + a quantized
        // sub-pixel phase (rounded to 1/PHASE_STEPS px; a phase that rounds up to
        // a whole pixel carries into the integer part).
        let (itx, px) = quantize(transform.tx);
        let (ity, py) = quantize(transform.ty);

        let mask_key = GlyphMaskKey {
            font_idx: key.0,
            gid: key.1,
            linear: [
                transform.sx.to_bits(),
                transform.ky.to_bits(),
                transform.kx.to_bits(),
                transform.sy.to_bits(),
            ],
            phase: (px * PHASE_STEPS + py) as u8,
        };

        if !self.masks.contains_key(&mask_key) {
            let qx = px as f32 / PHASE_STEPS as f32;
            let qy = py as f32 / PHASE_STEPS as f32;
            match rasterize(path, transform, qx, qy) {
                Some(cached) => {
                    if self.bytes + cached.data.len() > MAX_CACHE_BYTES {
                        self.masks.clear();
                        self.bytes = 0;
                    }
                    self.bytes += cached.data.len();
                    self.masks.insert(mask_key, cached);
                }
                None => {
                    // Oversized / degenerate: draw directly (rare).
                    fill_direct(pixmap, clip, path, transform, rgb);
                    return;
                }
            }
        }

        let cached = &self.masks[&mask_key];
        blit(pixmap, clip, cached, itx, ity, rgb);
    }
}

/// Integer part + phase bin for one axis. A fractional part that rounds up to a
/// whole pixel carries into the integer part (so phase `0` is shared between the
/// sub-pixel-0 and sub-pixel-≈1 cases, one integer pixel apart).
#[inline]
fn quantize(t: f32) -> (i32, i32) {
    let floor = t.floor();
    let mut phase = ((t - floor) * PHASE_STEPS as f32).round() as i32;
    let mut int = floor as i32;
    if phase >= PHASE_STEPS {
        phase = 0;
        int += 1;
    }
    (int, phase)
}

/// Rasterizes `path`'s coverage at the given 2×2 linear part + quantized
/// sub-pixel phase `(qx, qy)` into a tightly sized mask, returning it with its
/// integer bbox origin. Returns `None` for an oversized or degenerate glyph
/// (the caller then draws it directly).
fn rasterize(path: &tiny_skia::Path, transform: Transform, qx: f32, qy: f32) -> Option<CachedMask> {
    let (sx, ky, kx, sy) = (transform.sx, transform.ky, transform.kx, transform.sy);
    // Device bbox: transform the outline's control-point bounds (a superset of
    // the ink) by the affine and round out, padding 1 px so no partially covered
    // edge pixel is lost. A superset is safe — extra pixels have zero coverage.
    let b = path.bounds();
    let corners = [
        (b.left(), b.top()),
        (b.right(), b.top()),
        (b.left(), b.bottom()),
        (b.right(), b.bottom()),
    ];
    let (mut minx, mut miny) = (f32::INFINITY, f32::INFINITY);
    let (mut maxx, mut maxy) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (x, y) in corners {
        let dx = sx * x + kx * y + qx;
        let dy = ky * x + sy * y + qy;
        minx = minx.min(dx);
        maxx = maxx.max(dx);
        miny = miny.min(dy);
        maxy = maxy.max(dy);
    }
    if !(minx.is_finite() && miny.is_finite() && maxx.is_finite() && maxy.is_finite()) {
        return None;
    }
    let x0 = minx.floor() as i32 - 1;
    let y0 = miny.floor() as i32 - 1;
    let x1 = maxx.ceil() as i32 + 1;
    let y1 = maxy.ceil() as i32 + 1;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let w = (x1 - x0) as u32;
    let h = (y1 - y0) as u32;
    if w.checked_mul(h)? > MAX_MASK_PIXELS {
        return None;
    }
    let mut mask = Mask::new(w, h)?;
    // Shift by the integer bbox origin so the outline lands inside the mask; the
    // fractional (sub-pixel) part of the translation is preserved exactly.
    let t = Transform::from_row(sx, ky, kx, sy, qx - x0 as f32, qy - y0 as f32);
    mask.fill_path(path, FillRule::Winding, true, t);
    Some(CachedMask {
        w,
        h,
        ox: x0,
        oy: y0,
        data: mask.data().to_vec(),
    })
}

/// Blits a cached coverage mask at integer device offset `(itx, ity)`, replaying
/// tiny-skia's `lowp` premultiplied compositing byte-for-byte (see the module
/// docs for the two branches).
fn blit(
    pixmap: &mut SkPixmap,
    clip: Option<&Mask>,
    m: &CachedMask,
    itx: i32,
    ity: i32,
    rgb: [u8; 3],
) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let ox = itx + m.ox;
    let oy = ity + m.oy;
    // Mask rows/cols that land on-canvas.
    let j0 = (-oy).max(0);
    let i0 = (-ox).max(0);
    let mut j1 = (ph - oy).min(m.h as i32);
    let mut i1 = (pw - ox).min(m.w as i32);
    let (sr, sg, sb) = (rgb[0] as u16, rgb[1] as u16, rgb[2] as u16);
    let mw = m.w as usize;
    let pw_us = pw as usize;

    match clip {
        None => {
            let dst = pixmap.data_mut();
            for j in j0..j1 {
                let mrow = j as usize * mw;
                let drow = (oy + j) as usize * pw_us;
                for i in i0..i1 {
                    let cov = m.data[mrow + i as usize] as u16;
                    if cov == 0 {
                        continue;
                    }
                    let ic = 255 - cov;
                    let di = (drow + (ox + i) as usize) * 4;
                    // opaque Source: lerp(D, S, cov) = div255(D·(255−cov) + S·cov).
                    dst[di] = div255(dst[di] as u16 * ic + sr * cov) as u8;
                    dst[di + 1] = div255(dst[di + 1] as u16 * ic + sg * cov) as u8;
                    dst[di + 2] = div255(dst[di + 2] as u16 * ic + sb * cov) as u8;
                    dst[di + 3] = div255(dst[di + 3] as u16 * ic + 255 * cov) as u8;
                }
            }
        }
        Some(mask) => {
            let cw = mask.width() as i32;
            let ch = mask.height() as i32;
            // The clip mask is device-sized; clamp defensively (a pixel outside
            // the clip has zero coverage, so it leaves the destination unchanged
            // — exactly what skipping it does).
            j1 = j1.min(ch - oy);
            i1 = i1.min(cw - ox);
            let cw_us = cw as usize;
            // `pixmap` and the clip `mask` are disjoint borrows, so the clip is
            // read in place (no per-glyph copy of the device-size mask).
            let cdata = mask.data();
            let dst = pixmap.data_mut();
            for j in j0..j1 {
                let mrow = j as usize * mw;
                let py = (oy + j) as usize;
                let drow = py * pw_us;
                let crow = py * cw_us;
                for i in i0..i1 {
                    let cov = m.data[mrow + i as usize] as u16;
                    if cov == 0 {
                        continue;
                    }
                    let clipv = cdata[crow + (ox + i) as usize] as u16;
                    if clipv == 0 {
                        continue;
                    }
                    // S = (r, g, b, 255); MaskU8: S = div255(S·clip); then scale
                    // by coverage; then SourceOver over the destination D.
                    let mr = div255(sr * clipv);
                    let mg = div255(sg * clipv);
                    let mb = div255(sb * clipv);
                    // div255(255·clip) == clip.
                    let fr = div255(mr * cov);
                    let fg = div255(mg * cov);
                    let fb = div255(mb * cov);
                    let fa = div255(clipv * cov);
                    let ia = 255 - fa;
                    let di = (drow + (ox + i) as usize) * 4;
                    dst[di] = (fr + div255(dst[di] as u16 * ia)) as u8;
                    dst[di + 1] = (fg + div255(dst[di + 1] as u16 * ia)) as u8;
                    dst[di + 2] = (fb + div255(dst[di + 2] as u16 * ia)) as u8;
                    dst[di + 3] = (fa + div255(dst[di + 3] as u16 * ia)) as u8;
                }
            }
        }
    }
}

/// Draws the glyph directly with `Pixmap::fill_path` (the pre-cache path),
/// exactly matching the previous behaviour: opaque solid colour, `Winding`,
/// `SourceOver`, anti-aliased.
fn fill_direct(
    pixmap: &mut SkPixmap,
    clip: Option<&Mask>,
    path: &tiny_skia::Path,
    transform: Transform,
    rgb: [u8; 3],
) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 0xFF);
    pixmap.fill_path(path, &paint, FillRule::Winding, transform, clip);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{PathBuilder, Pixmap};

    /// A rotated/scaled filled quad in font-unit space (covers both AA edges and
    /// a fully covered interior).
    fn quad() -> tiny_skia::Path {
        let mut pb = PathBuilder::new();
        pb.move_to(40.0, 40.0);
        pb.line_to(760.0, 80.0);
        pb.line_to(720.0, 900.0);
        pb.line_to(60.0, 840.0);
        pb.close();
        pb.finish().unwrap()
    }

    fn white_pixmap(w: u32, h: u32) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        pm
    }

    /// The reference: exactly the fill the renderer issued before the cache.
    fn reference_fill(
        pm: &mut Pixmap,
        path: &tiny_skia::Path,
        t: Transform,
        rgb: [u8; 3],
        clip: Option<&Mask>,
    ) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 0xFF);
        pm.fill_path(path, &paint, FillRule::Winding, t, clip);
    }

    /// Render one glyph through the cache at the *exact* sub-pixel phase (no
    /// quantization) so the only thing under test is the raster+blit fidelity.
    fn exact_phase_fill(
        pm: &mut Pixmap,
        path: &tiny_skia::Path,
        t: Transform,
        rgb: [u8; 3],
        clip: Option<&Mask>,
    ) {
        let qx = t.tx - t.tx.floor();
        let qy = t.ty - t.ty.floor();
        let cached = rasterize(path, t, qx, qy).expect("small glyph rasterizes");
        blit(
            pm,
            clip,
            &cached,
            t.tx.floor() as i32,
            t.ty.floor() as i32,
            rgb,
        );
    }

    #[test]
    fn blit_matches_fill_path_no_clip() {
        let path = quad();
        // font-unit → device: /1000 upem scale, y-flip, fractional translation.
        let t = Transform::from_row(0.032, 0.004, -0.003, -0.030, 7.37, 33.91);
        let rgb = [10, 120, 200];

        let mut a = white_pixmap(48, 48);
        reference_fill(&mut a, &path, t, rgb, None);
        let mut b = white_pixmap(48, 48);
        exact_phase_fill(&mut b, &path, t, rgb, None);
        assert_eq!(
            a.data(),
            b.data(),
            "no-clip blit must be byte-identical to fill_path"
        );
    }

    #[test]
    fn blit_matches_fill_path_with_clip() {
        let path = quad();
        let t = Transform::from_row(0.033, 0.0, 0.0, -0.031, 6.6, 34.4);
        let rgb = [200, 30, 60];

        // A varied clip mask (not all-255) to exercise MaskU8.
        let mut clip = Mask::new(48, 48).unwrap();
        {
            let cw = 48usize;
            let d = clip.data_mut();
            for y in 0..48usize {
                for x in 0..48usize {
                    d[y * cw + x] = ((x * 5 + y * 3) % 256) as u8;
                }
            }
        }

        let mut a = white_pixmap(48, 48);
        reference_fill(&mut a, &path, t, rgb, Some(&clip));
        let mut b = white_pixmap(48, 48);
        exact_phase_fill(&mut b, &path, t, rgb, Some(&clip));
        assert_eq!(
            a.data(),
            b.data(),
            "clipped blit must be byte-identical to fill_path"
        );
    }

    #[test]
    fn cache_hit_equals_miss() {
        let path = quad();
        let t = Transform::from_row(0.03, 0.0, 0.0, -0.03, 5.25, 30.75);
        let rgb = [0, 0, 0];
        let key = (3usize, 42u16);

        // First draw (miss) into a, second draw (hit) into b — same start state.
        let mut cache = GlyphMaskCache::default();
        let mut a = white_pixmap(48, 48);
        cache.fill_glyph(&mut a, None, &path, t, key, rgb);
        assert_eq!(cache.masks.len(), 1, "first draw populates the cache");
        let mut b = white_pixmap(48, 48);
        cache.fill_glyph(&mut b, None, &path, t, key, rgb);
        assert_eq!(cache.masks.len(), 1, "second draw is a hit (no new entry)");
        assert_eq!(a.data(), b.data(), "cache hit must equal cache miss");
    }

    #[test]
    fn oversized_falls_back_to_fill_path() {
        let path = quad();
        // Scale so the device bbox blows past MAX_MASK_PIXELS.
        let t = Transform::from_row(2.0, 0.0, 0.0, 2.0, 0.0, 0.0);
        let rgb = [12, 34, 56];

        let mut cache = GlyphMaskCache::default();
        let mut a = white_pixmap(1800, 1800);
        cache.fill_glyph(&mut a, None, &path, t, (0, 0), rgb);
        assert!(cache.masks.is_empty(), "oversized glyph is not cached");

        let mut b = white_pixmap(1800, 1800);
        reference_fill(&mut b, &path, t, rgb, None);
        assert_eq!(a.data(), b.data(), "fallback must equal a direct fill_path");
    }
}
