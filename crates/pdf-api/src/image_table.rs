//! Image-table reconstruction (opt-in, OCR build only) — parse a table that
//! lives **inside a raster image** (a scanned / image-only page with no text
//! layer and no vector rulings) into a structured grid, preserving everything
//! the raster carries per cell: text, bbox, background color, text color, and
//! OCR confidence.
//!
//! This is a brand-new, opt-in surface gated behind the `paddle-ocr` cargo
//! feature: the only local, deterministic OCR engine pdfspine ships is the
//! pure-Rust PaddleOCR (PP-OCRv5) engine, so the whole module compiles out of
//! the lean base build. Nothing here changes any existing default behavior.
//!
//! # Pipeline
//!
//! 1. **Render** `page` to an RGB [`pdf_image::pixmap::Pixmap`] at `opts.dpi`
//!    (scale `s = dpi/72`). The pixmap is in PyMuPDF device space (origin
//!    top-left, y down, `/Rotate` applied), so a pixel `(px, py)` maps back to
//!    a page point simply by `(px/s, py/s)` — no y-flip.
//! 2. **OCR** the pixmap directly via [`pdf_ocr::OcrEngine::recognize`] (never
//!    via `textpage_ocr`, which drops confidence + the pixel bbox), keeping each
//!    [`pdf_ocr::OcrWord`]'s pixel bbox and confidence.
//! 3. **Cluster** the words into row bands (vertical gaps) and column bands
//!    (horizontal gaps), purely geometrically and deterministically.
//! 4. **Grid**: derive `row_count+1` / `col_count+1` grid-line positions from
//!    the band extents and gap midpoints, and convert them to page points.
//! 5. **Assign** each word to the grid slot containing its center; concatenate
//!    text in reading order and average the confidence.
//! 6. **Color**: per present cell, sample a border ring for the background and
//!    the darkest pixels for the foreground, returning each channel's modal
//!    (most-common) RGB triple straight off the pixmap.
//!
//! The output [`ImageTable`] mirrors the field naming of `pdf_text`'s vector
//! [`crate::Table`] (`bbox` / `row_count` / `col_count` / `cols` / `rows` /
//! `cells`) so it feels native, while carrying the extra raster-only data that
//! the vector type has no place for.

use std::collections::HashMap;

use pdf_core::geom::Rect;
use pdf_core::page::Page;

use pdf_ocr::{OcrEngine, OcrWord};

use crate::error::{Error, Result};

/// An RGB color: three 8-bit components in `[red, green, blue]` order.
pub type Rgb = [u8; 3];

/// One cell of an image table: its grid position plus everything recovered from
/// the raster.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageTableCell {
    /// The 0-based grid row of the cell's top-left slot (top row first).
    pub row: usize,
    /// The 0-based grid column of the cell's top-left slot (left column first).
    pub col: usize,
    /// How many grid rows this cell spans; `>= 1` (a merged cell spans `> 1`).
    pub row_span: usize,
    /// How many grid columns this cell spans; `>= 1` (a merged cell spans `> 1`).
    pub col_span: usize,
    /// The cell bounding box in **page points** (device space, `/Rotate`
    /// applied — the same frame as a `pdf_text` [`crate::Table`]).
    pub rect: Rect,
    /// The OCR text of the words whose center falls inside this cell, joined in
    /// reading order (top-to-bottom, then left-to-right) with single spaces.
    pub text: String,
    /// The mean OCR confidence (`0.0..=100.0`) of the words in this cell, or
    /// `0.0` when the cell carries no words.
    pub confidence: f32,
    /// The cell background color: the modal (most-common) RGB triple sampled
    /// from a ring just inside the cell border, away from the central text.
    pub bg_color: Rgb,
    /// The cell foreground / text color: the modal RGB triple among the darkest
    /// pixels in the cell interior; falls back to [`Self::bg_color`] for a cell
    /// with no clearly-dark pixels.
    pub text_color: Rgb,
}

/// One reconstructed table recovered from a raster image.
///
/// Field naming mirrors `pdf_text`'s vector [`crate::Table`] so the two feel
/// native side by side; the per-cell raster data lives on [`ImageTableCell`].
#[derive(Clone, Debug, PartialEq)]
pub struct ImageTable {
    /// The union of every present cell rect, in page points.
    pub bbox: Rect,
    /// The number of grid rows.
    pub row_count: usize,
    /// The number of grid columns.
    pub col_count: usize,
    /// The `col_count + 1` vertical grid-line x positions (page points),
    /// left-to-right.
    pub cols: Vec<f64>,
    /// The `row_count + 1` horizontal grid-line y positions (page points),
    /// top-to-bottom.
    pub rows: Vec<f64>,
    /// The present cells only (grid slots with at least one word), in row-major
    /// order; empty slots are skipped.
    pub cells: Vec<ImageTableCell>,
}

/// The result of [`page_find_image_tables`]: every reconstructed table.
///
/// v1 uses a single-table heuristic, so `tables` holds either `0` or `1` entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageTableResult {
    /// The reconstructed tables (currently at most one).
    pub tables: Vec<ImageTable>,
}

/// Tuning knobs for image-table reconstruction.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageTableOptions {
    /// The render DPI (and the DPI handed to the OCR engine). Higher is sharper
    /// for OCR but slower. Default `150`.
    pub dpi: u32,
    /// The OCR language code (PaddleOCR / Tesseract style, e.g. `"eng"`).
    /// Default `"eng"`.
    pub language: String,
    /// Drop OCR words below this confidence (`0.0..=100.0`) before gridding.
    /// Default `0.0` (keep all).
    pub min_confidence: f32,
    /// Row-clustering gap as a fraction of the median word height: a vertical
    /// center gap wider than `row_gap_ratio * median_height` starts a new row.
    /// Default `0.5`.
    pub row_gap_ratio: f64,
    /// Column-clustering gap as a fraction of the median word width: an x-center
    /// gap wider than `col_gap_ratio * median_width` starts a new column.
    /// Default `0.7`.
    pub col_gap_ratio: f64,
}

impl Default for ImageTableOptions {
    fn default() -> Self {
        ImageTableOptions {
            dpi: 150,
            language: "eng".to_string(),
            min_confidence: 0.0,
            row_gap_ratio: 0.5,
            col_gap_ratio: 0.7,
        }
    }
}

/// High-level entry: render `page` to a raster, OCR it, reconstruct the table
/// grid, and sample per-cell colors.
///
/// `engine` currently must be `"paddle"` — the local, deterministic PaddleOCR
/// engine, the only one able to drive this offline. Any other engine string
/// (and, in the lean build without the `paddle-ocr` feature, this whole module
/// is absent) yields [`Error::Unsupported`].
///
/// Returns an [`ImageTableResult`] with at most one [`ImageTable`] (v1
/// single-table heuristic). A page whose OCR yields fewer than two usable words,
/// or a grid with no present cells, returns an empty `tables` vec rather than an
/// error.
///
/// # Errors
///
/// - [`Error::Unsupported`] when `engine` is not `"paddle"`.
/// - Render / OCR errors propagate (e.g. a zero/over-large DPI, or the
///   PaddleOCR model failing to load).
pub fn page_find_image_tables(
    page: &Page,
    engine: &str,
    opts: &ImageTableOptions,
) -> Result<ImageTableResult> {
    // Render the page to an RGB pixmap at the requested DPI.
    let pix = render_rgb(page, opts.dpi)?;
    let scale = opts.dpi as f64 / 72.0;

    // OCR directly, keeping pixel bboxes + confidence.
    let words = recognize_words(engine, &pix, &opts.language, opts.dpi)?;

    // Keep only non-empty, sufficiently-confident words.
    let words: Vec<OcrWord> = words
        .into_iter()
        .filter(|w| w.confidence >= opts.min_confidence && !w.text.trim().is_empty())
        .collect();
    if words.len() < 2 {
        return Ok(ImageTableResult { tables: vec![] });
    }

    // Cluster into row/column bands (pixel space).
    let row_bands = cluster_rows(&words, opts.row_gap_ratio);
    let col_bands = cluster_cols(&words, opts.col_gap_ratio);
    if row_bands.is_empty() || col_bands.is_empty() {
        return Ok(ImageTableResult { tables: vec![] });
    }

    // Grid-line positions in pixel space, then converted to page points.
    let row_lines_px = grid_lines(&row_bands);
    let col_lines_px = grid_lines(&col_bands);
    let rows: Vec<f64> = row_lines_px.iter().map(|&y| y / scale).collect();
    let cols: Vec<f64> = col_lines_px.iter().map(|&x| x / scale).collect();
    let row_count = row_bands.len();
    let col_count = col_bands.len();

    // Assign each word to the grid slot containing its center.
    let mut slot_words: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, w) in words.iter().enumerate() {
        let bb = w.bbox.normalize();
        let cx = (bb.x0 + bb.x1) / 2.0;
        let cy = (bb.y0 + bb.y1) / 2.0;
        let Some(r) = slot_index(&row_lines_px, cy) else {
            continue;
        };
        let Some(c) = slot_index(&col_lines_px, cx) else {
            continue;
        };
        slot_words.entry((r, c)).or_default().push(i);
    }

    // Build the present cells, row-major.
    let mut cells: Vec<ImageTableCell> = Vec::new();
    for r in 0..row_count {
        for c in 0..col_count {
            let Some(idxs) = slot_words.get(&(r, c)) else {
                continue;
            };
            if idxs.is_empty() {
                continue;
            }

            // Reading order: top-to-bottom, then left-to-right.
            let mut ordered: Vec<usize> = idxs.clone();
            ordered.sort_by(|&a, &b| {
                let (wa, wb) = (words[a].bbox.normalize(), words[b].bbox.normalize());
                let ca = (wa.y0 + wa.y1) / 2.0;
                let cb = (wb.y0 + wb.y1) / 2.0;
                ca.partial_cmp(&cb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(wa.x0.partial_cmp(&wb.x0).unwrap_or(std::cmp::Ordering::Equal))
            });

            let text = ordered
                .iter()
                .map(|&i| words[i].text.trim())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let confidence = if ordered.is_empty() {
                0.0
            } else {
                ordered.iter().map(|&i| words[i].confidence).sum::<f32>() / ordered.len() as f32
            };

            // Column span: widen when any assigned word clearly straddles
            // multiple column slots (horizontal spanning headers are common).
            let col_span = max_h_span(&col_lines_px, &ordered, &words, c, col_count);
            let row_span = max_v_span(&row_lines_px, &ordered, &words, r, row_count);

            // Cell pixel rect from the grid lines, spans applied.
            let px0 = col_lines_px[c];
            let px1 = col_lines_px[(c + col_span).min(col_count)];
            let py0 = row_lines_px[r];
            let py1 = row_lines_px[(r + row_span).min(row_count)];

            let bg_color = sample_bg_color(&pix, px0, py0, px1, py1);
            let text_color = sample_text_color(&pix, px0, py0, px1, py1, bg_color);

            let rect = Rect::new(px0 / scale, py0 / scale, px1 / scale, py1 / scale);
            cells.push(ImageTableCell {
                row: r,
                col: c,
                row_span,
                col_span,
                rect,
                text,
                confidence,
                bg_color,
                text_color,
            });
        }
    }

    if cells.is_empty() {
        return Ok(ImageTableResult { tables: vec![] });
    }

    let bbox = cells
        .iter()
        .fold(Rect::default(), |acc, cell| acc.union(&cell.rect));

    Ok(ImageTableResult {
        tables: vec![ImageTable {
            bbox,
            row_count,
            col_count,
            cols,
            rows,
            cells,
        }],
    })
}

// ====================================================================== render

/// Renders `page` to an RGB (`n == 3`, no alpha) [`Pixmap`] at `dpi`.
fn render_rgb(page: &Page, dpi: u32) -> Result<pdf_image::pixmap::Pixmap> {
    use pdf_image::pixmap::Colorspace;
    use pdf_render::{render_page, RenderOptions};

    let opts = RenderOptions {
        dpi: Some(dpi),
        colorspace: Colorspace::Rgb,
        alpha: false,
        ..RenderOptions::default()
    };
    Ok(render_page(page.document(), page, &opts)?)
}

/// Dispatches `engine` to a concrete [`OcrEngine`] and recognizes `pix`
/// directly, returning per-word **pixel** bboxes + confidence.
///
/// Only `"paddle"` is supported; anything else yields [`Error::Unsupported`].
/// `recognize` is called directly (never `textpage_ocr`) so confidence and the
/// pixel bbox survive.
fn recognize_words(
    engine: &str,
    pix: &pdf_image::pixmap::Pixmap,
    language: &str,
    dpi: u32,
) -> Result<Vec<OcrWord>> {
    match engine {
        "paddle" => {
            let eng = pdf_ocr::PaddleOcr::new()?;
            Ok(eng.recognize(pix, language, dpi as f32)?)
        }
        other => Err(Error::Unsupported(format!(
            "image-table OCR engine {other:?} is not supported; expected \"paddle\" \
             (the local deterministic PaddleOCR engine)"
        ))),
    }
}

// ===================================================================== banding

/// A 1-D band: an inclusive `[lo, hi]` extent in pixel space.
#[derive(Clone, Copy, Debug)]
struct Band {
    lo: f64,
    hi: f64,
}

/// Clusters words into ordered row bands by vertical center gaps.
///
/// Words are sorted by vertical center `cy`; a new band starts when the next
/// word's `cy` exceeds the current band's running-mean `cy` by more than
/// `gap_ratio * median_height`. Each band's extent is `[min y0, max y1]` of its
/// words.
fn cluster_rows(words: &[OcrWord], gap_ratio: f64) -> Vec<Band> {
    let median_h = median(words.iter().map(|w| w.bbox.normalize().height()));
    let threshold = (gap_ratio * median_h).max(1.0);

    let mut idx: Vec<usize> = (0..words.len()).collect();
    idx.sort_by(|&a, &b| {
        center_y(&words[a])
            .partial_cmp(&center_y(&words[b]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut bands: Vec<Band> = Vec::new();
    let mut run_mean = 0.0_f64;
    let mut run_n = 0usize;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;

    for &i in &idx {
        let w = words[i].bbox.normalize();
        let cy = (w.y0 + w.y1) / 2.0;
        if run_n > 0 && (cy - run_mean) > threshold {
            bands.push(Band { lo, hi });
            run_mean = 0.0;
            run_n = 0;
            lo = f64::INFINITY;
            hi = f64::NEG_INFINITY;
        }
        run_n += 1;
        run_mean += (cy - run_mean) / run_n as f64;
        lo = lo.min(w.y0);
        hi = hi.max(w.y1);
    }
    if run_n > 0 {
        bands.push(Band { lo, hi });
    }
    bands
}

/// Clusters words into ordered column bands by horizontal center gaps.
///
/// Words are sorted by horizontal center `cx`; a new band starts when the next
/// word's `cx` gap exceeds `gap_ratio * median_width`. Each band's extent is
/// `[min x0, max x1]` of its words.
fn cluster_cols(words: &[OcrWord], gap_ratio: f64) -> Vec<Band> {
    let median_w = median(words.iter().map(|w| w.bbox.normalize().width()));
    let threshold = (gap_ratio * median_w).max(1.0);

    let mut idx: Vec<usize> = (0..words.len()).collect();
    idx.sort_by(|&a, &b| {
        center_x(&words[a])
            .partial_cmp(&center_x(&words[b]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut bands: Vec<Band> = Vec::new();
    let mut prev_cx: Option<f64> = None;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;

    for &i in &idx {
        let w = words[i].bbox.normalize();
        let cx = (w.x0 + w.x1) / 2.0;
        if let Some(p) = prev_cx {
            if (cx - p) > threshold {
                bands.push(Band { lo, hi });
                lo = f64::INFINITY;
                hi = f64::NEG_INFINITY;
            }
        }
        lo = lo.min(w.x0);
        hi = hi.max(w.x1);
        prev_cx = Some(cx);
    }
    if prev_cx.is_some() {
        bands.push(Band { lo, hi });
    }
    bands
}

/// Derives `bands.len() + 1` grid-line positions: the outer extents bound the
/// first and last lines; interior lines are the midpoints of consecutive band
/// gaps.
fn grid_lines(bands: &[Band]) -> Vec<f64> {
    let mut lines = Vec::with_capacity(bands.len() + 1);
    lines.push(bands[0].lo);
    for pair in bands.windows(2) {
        lines.push((pair[0].hi + pair[1].lo) / 2.0);
    }
    lines.push(bands[bands.len() - 1].hi);
    lines
}

/// The 0-based slot index whose `[lines[i], lines[i + 1]]` interval contains
/// `v`. Values below the first line clamp to slot `0`, above the last to the
/// last slot, so an off-by-an-epsilon center never drops a word.
fn slot_index(lines: &[f64], v: f64) -> Option<usize> {
    let n = lines.len().checked_sub(1)?;
    if n == 0 {
        return None;
    }
    if v <= lines[0] {
        return Some(0);
    }
    if v >= lines[n] {
        return Some(n - 1);
    }
    for i in 0..n {
        if v >= lines[i] && v < lines[i + 1] {
            return Some(i);
        }
    }
    Some(n - 1)
}

/// The horizontal span (in column slots) for a cell at column `c`: widened when
/// any assigned word's pixel `[x0, x1]` overlaps the interior of a later column
/// slot by more than half that slot's width.
fn max_h_span(
    col_lines: &[f64],
    word_idx: &[usize],
    words: &[OcrWord],
    c: usize,
    col_count: usize,
) -> usize {
    let mut span = 1usize;
    for &i in word_idx {
        let bb = words[i].bbox.normalize();
        let mut last = c;
        for nc in (c + 1)..col_count {
            let lo = col_lines[nc];
            let hi = col_lines[nc + 1];
            let overlap = (bb.x1.min(hi) - bb.x0.max(lo)).max(0.0);
            if overlap > (hi - lo) / 2.0 {
                last = nc;
            } else {
                break;
            }
        }
        span = span.max(last - c + 1);
    }
    span
}

/// The vertical span (in row slots) for a cell at row `r`: widened when any
/// assigned word's pixel `[y0, y1]` overlaps the interior of a later row slot by
/// more than half that slot's height. Spanning rows are rare but cheap to test.
fn max_v_span(
    row_lines: &[f64],
    word_idx: &[usize],
    words: &[OcrWord],
    r: usize,
    row_count: usize,
) -> usize {
    let mut span = 1usize;
    for &i in word_idx {
        let bb = words[i].bbox.normalize();
        let mut last = r;
        for nr in (r + 1)..row_count {
            let lo = row_lines[nr];
            let hi = row_lines[nr + 1];
            let overlap = (bb.y1.min(hi) - bb.y0.max(lo)).max(0.0);
            if overlap > (hi - lo) / 2.0 {
                last = nr;
            } else {
                break;
            }
        }
        span = span.max(last - r + 1);
    }
    span
}

// ======================================================================= color

/// Reads the RGB triple of pixel `(x, y)` straight off the pixmap's interleaved
/// buffer (first three components — alpha, if any, is ignored). `None` out of
/// bounds.
fn rgb_at(pix: &pdf_image::pixmap::Pixmap, x: u32, y: u32) -> Option<Rgb> {
    if x >= pix.width || y >= pix.height {
        return None;
    }
    let off = y as usize * pix.stride + x as usize * pix.n as usize;
    let s = pix.samples();
    Some([s[off], s[off + 1], s[off + 2]])
}

/// Perceptual luminance of an RGB triple (`0.299 r + 0.587 g + 0.114 b`).
fn luminance(c: Rgb) -> f64 {
    0.299 * c[0] as f64 + 0.587 * c[1] as f64 + 0.114 * c[2] as f64
}

/// The modal (most-frequent exact) RGB triple of an iterator of colors, or
/// black when the iterator is empty. Ties break toward the first-seen color, so
/// the result is deterministic.
fn modal_color(colors: impl IntoIterator<Item = Rgb>) -> Rgb {
    let mut counts: HashMap<Rgb, (usize, usize)> = HashMap::new();
    for (seq, c) in colors.into_iter().enumerate() {
        let e = counts.entry(c).or_insert((0, seq));
        e.0 += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| {
            // Higher count wins; on a tie the earlier first-seen color wins.
            a.1 .0
                .cmp(&b.1 .0)
                .then_with(|| b.1 .1.cmp(&a.1 .1))
        })
        .map(|(color, _)| color)
        .unwrap_or([0, 0, 0])
}

/// A deterministic subsample step for a `[lo, hi)` pixel span so large cells
/// stay fast: at least 1, growing so each axis yields roughly `<= 64` samples.
fn step_for(span: f64) -> u32 {
    let span = span.max(1.0) as u32;
    (span / 64).max(1)
}

/// Samples the cell **background**: the modal color of a ring of pixels just
/// inside the cell border (the outer ~12% margin on each side), which avoids the
/// central text and captures the fill even on a busy cell.
fn sample_bg_color(pix: &pdf_image::pixmap::Pixmap, x0: f64, y0: f64, x1: f64, y1: f64) -> Rgb {
    let (ix0, iy0, ix1, iy1) = clamp_rect(pix, x0, y0, x1, y1);
    if ix1 <= ix0 || iy1 <= iy0 {
        return [255, 255, 255];
    }
    let w = ix1 - ix0;
    let h = iy1 - iy0;
    let mx = ((w as f64) * 0.12).round() as u32;
    let my = ((h as f64) * 0.12).round() as u32;
    // The inner rectangle the ring surrounds (text lives here).
    let inner_x0 = ix0 + mx;
    let inner_y0 = iy0 + my;
    let inner_x1 = ix1.saturating_sub(mx);
    let inner_y1 = iy1.saturating_sub(my);

    let sx = step_for(w as f64);
    let sy = step_for(h as f64);
    let mut ring: Vec<Rgb> = Vec::new();
    let mut y = iy0;
    while y < iy1 {
        let mut x = ix0;
        while x < ix1 {
            let in_inner =
                x >= inner_x0 && x < inner_x1 && y >= inner_y0 && y < inner_y1;
            if !in_inner {
                if let Some(c) = rgb_at(pix, x, y) {
                    ring.push(c);
                }
            }
            x += sx;
        }
        y += sy;
    }
    if ring.is_empty() {
        // Degenerate tiny cell: fall back to the whole-cell mode.
        return modal_color(iter_rect(pix, ix0, iy0, ix1, iy1, sx, sy));
    }
    modal_color(ring)
}

/// Samples the cell **text** color: the modal color among the darkest quartile
/// (by luminance) of the cell interior. Falls back to `bg` when the cell has no
/// pixels clearly darker than the background (an empty cell).
///
/// The outer ~12% margin on each side is excluded first (the same inset
/// `sample_bg_color` uses) so a dark cell **border / grid line** — which is not
/// text — never dominates the "darkest" pixels and masquerades as the text
/// color.
fn sample_text_color(
    pix: &pdf_image::pixmap::Pixmap,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    bg: Rgb,
) -> Rgb {
    let (cx0, cy0, cx1, cy1) = clamp_rect(pix, x0, y0, x1, y1);
    if cx1 <= cx0 || cy1 <= cy0 {
        return bg;
    }
    // Inset away from the border (text lives in the interior); fall back to the
    // full cell only when the inset would collapse a tiny cell to nothing.
    let mx = (((cx1 - cx0) as f64) * 0.12).round() as u32;
    let my = (((cy1 - cy0) as f64) * 0.12).round() as u32;
    let mut ix0 = cx0 + mx;
    let mut iy0 = cy0 + my;
    let mut ix1 = cx1.saturating_sub(mx);
    let mut iy1 = cy1.saturating_sub(my);
    if ix1 <= ix0 || iy1 <= iy0 {
        ix0 = cx0;
        iy0 = cy0;
        ix1 = cx1;
        iy1 = cy1;
    }

    let sx = step_for((ix1 - ix0) as f64);
    let sy = step_for((iy1 - iy0) as f64);

    let mut samples: Vec<(f64, Rgb)> = iter_rect(pix, ix0, iy0, ix1, iy1, sx, sy)
        .map(|c| (luminance(c), c))
        .collect();
    if samples.is_empty() {
        return bg;
    }
    samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // The darkest quartile, but only pixels meaningfully darker than the
    // background (else a uniform empty cell would report its own fill).
    let bg_lum = luminance(bg);
    let cut = (samples.len() / 4).max(1);
    let dark: Vec<Rgb> = samples
        .iter()
        .take(cut)
        .filter(|(lum, _)| *lum < bg_lum - 24.0)
        .map(|(_, c)| *c)
        .collect();
    if dark.is_empty() {
        return bg;
    }
    modal_color(dark)
}

/// Clamps a page-pixel rect to integer pixel bounds inside the pixmap, returning
/// `(x0, y0, x1, y1)` with `x0 <= x1`, `y0 <= y1`, all in `[0, width/height]`.
fn clamp_rect(
    pix: &pdf_image::pixmap::Pixmap,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> (u32, u32, u32, u32) {
    let cx0 = x0.min(x1).floor().clamp(0.0, pix.width as f64) as u32;
    let cy0 = y0.min(y1).floor().clamp(0.0, pix.height as f64) as u32;
    let cx1 = x0.max(x1).ceil().clamp(0.0, pix.width as f64) as u32;
    let cy1 = y0.max(y1).ceil().clamp(0.0, pix.height as f64) as u32;
    (cx0, cy0, cx1, cy1)
}

/// Iterates the RGB triples of an integer pixel rect at fixed `(sx, sy)` steps —
/// deterministic, with no randomness.
fn iter_rect(
    pix: &pdf_image::pixmap::Pixmap,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    sx: u32,
    sy: u32,
) -> impl Iterator<Item = Rgb> + '_ {
    let sx = sx.max(1);
    let sy = sy.max(1);
    (y0..y1)
        .step_by(sy as usize)
        .flat_map(move |y| (x0..x1).step_by(sx as usize).map(move |x| (x, y)))
        .filter_map(move |(x, y)| rgb_at(pix, x, y))
}

// ====================================================================== helpers

/// The vertical center of a word's (normalized) pixel bbox.
fn center_y(w: &OcrWord) -> f64 {
    let b = w.bbox.normalize();
    (b.y0 + b.y1) / 2.0
}

/// The horizontal center of a word's (normalized) pixel bbox.
fn center_x(w: &OcrWord) -> f64 {
    let b = w.bbox.normalize();
    (b.x0 + b.x1) / 2.0
}

/// The median of a sequence of finite values, or `0.0` when empty. Used for the
/// row/column clustering thresholds.
fn median(vals: impl IntoIterator<Item = f64>) -> f64 {
    let mut v: Vec<f64> = vals.into_iter().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}
