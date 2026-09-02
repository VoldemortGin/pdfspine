//! Layout reconstruction (M2c, PRD §8.6).
//!
//! Turns the interpreter's flat [`PositionedGlyph`] list (PDF user space) into a
//! PyMuPDF-shaped [`TextPage`] (device space: origin top-left, y down, `/Rotate`
//! applied). The pipeline is:
//!
//! 1. **device transform** — map every glyph through the page transform `P_r`
//!    (PRD §8.6.1) so coordinates are in displayed/rotated device space;
//! 2. **lines** — cluster glyphs by baseline proximity along the writing axis;
//! 3. **spans** — split each line where font / size / color / flags change;
//! 4. **blocks** — group lines by vertical gaps + horizontal overlap;
//! 5. **reading order** — recursive XY-cut so columns read column-by-column.
//!
//! Word segmentation lives in [`crate::words`]; serialization is M2d.

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_core::page::Page;
use pdf_core::{DocumentStore, Limits, Name, Object};
use smol_str::SmolStr;

use crate::interp::ContentInterpreter;
use crate::model::{
    flags, Block, BlockKind, Char, ImageBlock, ImageRef, InterpretResult, Line, PositionedGlyph,
    Span, TextPage, WritingDir,
};

/// Baseline-cluster tolerance as a fraction of font size. Two glyphs land on the
/// same line when their baseline (along the cross-axis) differs by less than
/// this times the larger size — tolerant to small super/subscript rises.
const LINE_TOL_FRAC: f64 = 0.5;
const LINE_BASELINE_EPSILON: f64 = 1e-6;

/// Baseline offset (× the larger size) up to which an orphaned short run is
/// reattached to a horizontally-containing line as a super/subscript. Covers a
/// `Ts`-raised footnote marker `( 1 )` (rise ≈0.6× line height) while staying below
/// normal line spacing (≈1.0×+), so a separate line is never swallowed.
const SUPERSCRIPT_RISE_FRAC: f64 = 0.85;

/// A reattachment-candidate run is at most this many glyph cells wide — a footnote
/// marker / allele subscript is 1–3 chars; a real line is far wider. Keeps the
/// x-containment reattachment from ever merging two genuine lines.
const FRAGMENT_MAX_WIDTH_FRAC: f64 = 3.0;

/// Two independent painted runs on one baseline become separate lines when the
/// true glyph-edge gap reaches this fraction of the device-space font size.
const LINE_RUN_GAP_FRAC: f64 = 0.8;
const LINE_RUN_GAP_EPSILON: f64 = 1e-6;

/// Word-gap threshold as a fraction of the *device-space font size*. A gap
/// between the trailing edge of one glyph and the leading edge of the next that
/// exceeds `size * WORD_GAP_FRAC` is a word break even without a literal space
/// (PRD §8.6.2); `build_line` synthesizes the space there and `words.rs` then
/// splits on it like any other whitespace. Mirrors MuPDF's `SPACE_DIST`
/// (`spacing = gap / size > 0.15`); the size is the per-line median glyph
/// estimate, never the raw cell height, so short descriptor cells cannot shrink
/// the threshold.
const WORD_GAP_FRAC: f64 = 0.15;

/// Widest letter-to-letter gap, as a fraction of the device-space font size,
/// that [`letter_spacing_skip_mask`] still reads as tracking. A letter-spaced
/// heading keeps its letters closer than a word space (≈0.25–0.33 em in text
/// faces — Helvetica's is 0.278); a uniform gap beyond this is a row of
/// single-letter words (map labels, spaced-out initials) and every gap in it
/// stays a word break, as PyMuPDF reads it.
const LETTER_SPACING_MAX_FRAC: f64 = 0.3;

/// Baseline movement, in effective font-size units, below which text remains in
/// the current paragraph regardless of horizontal movement.
const BLOCK_BASELINE_NEAR: f64 = 0.8;

/// Baseline movement, in effective font-size units, above which a new text block
/// starts. The strict boundary is compatibility-significant: a 1.5× step stays
/// in the block, while anything larger starts a new one.
const BLOCK_BASELINE_FAR: f64 = 1.5;

/// A rightward line-start shift larger than this many device-space points marks
/// an indented paragraph when the baseline movement is in the middle band.
const BLOCK_INDENT_EPSILON: f64 = 0.5;

/// Two disjoint regions are treated as side-by-side columns when they overlap
/// by at least this fraction of the shorter region's vertical extent.
const COLUMN_REGION_OVERLAP_FRAC: f64 = 0.5;

/// Minimum evidence required before bypassing XY-cut for a dense table. Tables
/// need same-row cells to remain together; cutting them into vertical regions
/// makes a borderline row step fragment every cell into its own block.
const TABLE_MIN_VISIBLE_LINES: usize = 12;
const TABLE_MIN_DENSE_BASELINES: usize = 3;
const TABLE_MIN_CELLS_PER_BASELINE: usize = 3;
const TABLE_SUPPORT_LINE_FRAC: f64 = 0.65;
const TABLE_DOMINANT_DIR_FRAC: f64 = 0.8;
const TABLE_BASELINE_TOL_FRAC: f64 = 0.1;
const TABLE_BASELINE_TOL_MIN: f64 = 0.5;
const TABLE_BASELINE_TOL_MAX: f64 = 2.0;

/// Minimum empty horizontal-band height used by the recursive page-region cut.
/// This deliberately stays much larger than the paragraph-break threshold: ordinary line
/// spacing may split PyMuPDF text blocks, but it must not split a two-column page
/// into row bands and destroy column-major reading order.
const REGION_BAND_GAP_FRAC: f64 = 1.3;

// === public API ===========================================================

/// Builds a [`TextPage`] for a page: runs the interpreter, applies the page
/// transform and groups glyphs (PRD §8.6).
///
/// Glyphs whose origin falls outside the page **CropBox** are dropped — this is
/// the `TEXT_MEDIABOX_CLIP` behavior in the default `get_text` flag set
/// (`defaults::TEXT`), matching fitz: off-page print-control marks and bleed
/// outside the visible/crop region do not appear in extracted text.
#[must_use]
pub fn build_textpage(doc: &DocumentStore, page: &Page, _limits: &Limits) -> TextPage {
    let Some(page_dict) = page.dict() else {
        return TextPage::default();
    };
    let res: InterpretResult = ContentInterpreter::new(doc).run_page(&page_dict);
    // CropBox is the shared coordinate basis: it drives both the device transform
    // (origin baked out) and the page size, *and* is the out-of-page glyph clip —
    // so digital-text device coords share one origin with render/svg/ocr on pages
    // where CropBox ≠ MediaBox. (On the common CropBox == MediaBox page it is
    // byte-for-byte the old MediaBox behavior.)
    let cropbox = page.cropbox();
    let rotate = page.rotation();
    // Resource-name → resolved `/BaseFont` map (built once per page). The
    // device-transform pass applies it inline, so font-name enrichment costs one
    // pass over the *distinct* fonts instead of one clone per glyph.
    let resolver = build_font_resolver(doc, &page_dict);
    textpage_core(
        &res.glyphs,
        &res.images,
        cropbox,
        rotate,
        Some(cropbox),
        Some(&resolver),
    )
}

/// Builds a [`TextPage`] directly from a glyph list + image inventory in **PDF
/// user space**, a MediaBox and a `/Rotate` value. The unit-test entry point
/// (no document needed). No clipping is applied (see
/// [`textpage_from_glyphs_clipped`] for the CropBox-clip variant).
#[must_use]
pub fn textpage_from_glyphs(
    glyphs: &[PositionedGlyph],
    images: &[ImageRef],
    mediabox: Rect,
    rotate: i32,
) -> TextPage {
    textpage_from_glyphs_clipped(glyphs, images, mediabox, rotate, None)
}

/// Like [`textpage_from_glyphs`], but drops glyphs whose origin falls outside
/// `clip` (a rect in **PDF user space**, e.g. the page CropBox) when `clip` is
/// `Some` — the `TEXT_MEDIABOX_CLIP` behavior. A small epsilon tolerates glyphs
/// sitting exactly on the box edge.
#[must_use]
pub fn textpage_from_glyphs_clipped(
    glyphs: &[PositionedGlyph],
    images: &[ImageRef],
    mediabox: Rect,
    rotate: i32,
    clip: Option<Rect>,
) -> TextPage {
    textpage_core(glyphs, images, mediabox, rotate, clip, None)
}

/// The shared TextPage builder. `resolver`, when present, maps each glyph's
/// resource font name to its resolved `/BaseFont` (font-name enrichment) inline
/// during the device-transform pass, avoiding a separate O(glyphs) pass.
fn textpage_core(
    glyphs: &[PositionedGlyph],
    images: &[ImageRef],
    page_box: Rect,
    rotate: i32,
    clip: Option<Rect>,
    resolver: Option<&FontResolver>,
) -> TextPage {
    let p = page_transform(page_box, rotate);
    let (width, height) = page_size(page_box, rotate);

    // 1. Transform every glyph to device space, dropping out-of-CropBox glyphs.
    // `dir` depends only on the page transform + writing mode (not the glyph), so
    // both vectors are computed once. The font name is resolved (enriched) and
    // its style flags memoized per distinct resource name — both repeat across
    // nearly every glyph, so this is one map lookup per glyph instead of a clone
    // + lowercase scan.
    let clip = clip.map(|c| c.normalize());
    let dir_h = writing_dir_vector(&p, 0);
    let dir_v = writing_dir_vector(&p, 1);
    // Per distinct resource name: (resolved font name, style flags).
    let mut font_cache: std::collections::HashMap<SmolStr, (SmolStr, u32)> =
        std::collections::HashMap::new();
    let mut dev: Vec<DevGlyph> = Vec::with_capacity(glyphs.len());
    for g in glyphs {
        if let Some(c) = clip.as_ref() {
            if !origin_in_clip(g.origin, c) {
                continue;
            }
        }
        let (font, flags) = font_cache
            .entry(g.font_name.clone())
            .or_insert_with(|| {
                let resolved = resolver
                    .and_then(|r| r.resolve(&g.font_name))
                    .unwrap_or_else(|| g.font_name.clone());
                let flags = name_flags(&resolved);
                (resolved, flags)
            })
            .clone();
        dev.push(DevGlyph::new(g, &p, dir_h, dir_v, font, flags));
    }

    // 2/3. lines + spans.
    let lines = group_lines(&dev);

    // 4. blocks — column-aware paragraph grouping: cut the lines into column
    //    regions first (so a paragraph block never straddles two columns), then
    //    group each column's lines into paragraphs by vertical gaps.
    let mut blocks = group_blocks_columned(lines, width, height);

    // image blocks (device-space bbox via the placement CTM → page transform).
    for img in images {
        let bbox = image_bbox(img, &p);
        blocks.push(Block {
            bbox,
            kind: BlockKind::Image,
            lines: Vec::new(),
            image: Some(ImageBlock {
                name: img.name.clone(),
                width: img.width,
                height: img.height,
            }),
            number: 0,
            seq: usize::MAX,
        });
    }

    // 5. reading order + number assignment (content/document order, matching how
    //    MuPDF/PyMuPDF sequences its structured-text blocks).
    order_blocks(&mut blocks);

    TextPage {
        width,
        height,
        blocks,
    }
}

/// Whether a glyph origin (PDF user space) lies within `clip` (the CropBox),
/// with a 1pt slack so glyphs sitting on the edge are kept (matches fitz, which
/// keeps marginal glyphs and only drops clearly off-page ones).
fn origin_in_clip(origin: Point, clip: &Rect) -> bool {
    const SLACK: f64 = 1.0;
    origin.x >= clip.x0 - SLACK
        && origin.x <= clip.x1 + SLACK
        && origin.y >= clip.y0 - SLACK
        && origin.y <= clip.y1 + SLACK
}

// === device / page transform (PRD §8.6.1) ================================

/// The page transform `P_r` (PRD §8.6.1) mapping PDF user space (post-CTM,
/// y-up, page-box-relative) into PyMuPDF device space (top-left, y-down, with
/// `/Rotate` applied). `[a b c d e f]` row-vector form. Basis-agnostic: it bakes
/// out the origin of whatever page box it is given; callers pass the **CropBox**
/// so all extraction channels share one origin.
#[must_use]
pub fn page_transform(page_box: Rect, rotate: i32) -> Matrix {
    let mb = page_box.normalize();
    let (x0, y0, x1, y1) = (mb.x0, mb.y0, mb.x1, mb.y1);
    match normalize_rotate(rotate) {
        90 => Matrix::new(0.0, 1.0, 1.0, 0.0, -y0, -x0),
        180 => Matrix::new(-1.0, 0.0, 0.0, 1.0, x1, -y0),
        270 => Matrix::new(0.0, -1.0, -1.0, 0.0, y1, x1),
        _ => Matrix::new(1.0, 0.0, 0.0, -1.0, -x0, y1),
    }
}

/// The displayed page size `(width, height)` after `/Rotate`: `w×h` for
/// `r ∈ {0,180}`, `h×w` for `r ∈ {90,270}` (PyMuPDF `page.rect`).
#[must_use]
pub fn page_size(page_box: Rect, rotate: i32) -> (f64, f64) {
    let mb = page_box.normalize();
    let (w, h) = (mb.width(), mb.height());
    match normalize_rotate(rotate) {
        90 | 270 => (h, w),
        _ => (w, h),
    }
}

/// Normalizes a raw `/Rotate` to `{0, 90, 180, 270}` (negatives + multiples).
fn normalize_rotate(r: i32) -> i32 {
    r.rem_euclid(360)
}

// === device glyph =========================================================

/// A glyph mapped to device space, with the line axis precomputed.
///
/// `text` holds the glyph's full Unicode mapping (usually one scalar; a ligature
/// like `ﬁ` maps to several). All scalars share the glyph cell geometry.
#[derive(Clone, Debug)]
struct DevGlyph {
    origin: Point,
    bbox: Rect,
    text: SmolStr,
    font: SmolStr,
    size: f64,
    color: u32,
    flags: u32,
    wmode: u8,
    /// Writing-direction unit vector `(cos, sin)` in device space.
    dir: (f64, f64),
    /// Font ascender normalized to a unit font size (PyMuPDF span `ascender`).
    ascender: f64,
    /// Font descender normalized to a unit font size (PyMuPDF span `descender`).
    descender: f64,
}

impl DevGlyph {
    /// Builds a device-space glyph. `dir_h`/`dir_v` are the precomputed
    /// writing-direction vectors for this page transform (horizontal/vertical);
    /// `font` is the resolved (enriched) font name and `flags` its memoized style
    /// flags — all hoisted out of the per-glyph hot path in [`textpage_core`].
    fn new(
        g: &PositionedGlyph,
        p: &Matrix,
        dir_h: (f64, f64),
        dir_v: (f64, f64),
        font: SmolStr,
        flags: u32,
    ) -> Self {
        let origin = g.origin.transform(p);
        let bbox = g.bbox.transform(p).normalize();
        let text = if g.unicode.is_empty() {
            SmolStr::new("\u{FFFD}")
        } else {
            g.unicode.clone()
        };
        let wmode = match g.writing_dir {
            WritingDir::Vertical => 1,
            WritingDir::Horizontal => 0,
        };
        // Device-space writing direction. Vertical writing keeps the page's
        // vertical vector; horizontal writing maps this glyph's own user-space
        // advance (`(1,0)` upright, `(0,±1)` for a rotated `Tm`) through the page
        // transform's linear part — so a rotated table header clusters along its
        // own baseline instead of one glyph per line. Upright text yields exactly
        // `dir_h`, so non-rotated pages are byte-identical to before.
        let dir = if wmode == 1 {
            dir_v
        } else {
            let (ux, uy) = g.advance_dir;
            let dx = p.a * ux + p.c * uy;
            let dy = p.b * ux + p.d * uy;
            let n = (dx * dx + dy * dy).sqrt();
            if n > f64::EPSILON {
                (dx / n, dy / n)
            } else {
                dir_h
            }
        };
        DevGlyph {
            origin,
            bbox,
            text,
            font,
            size: g.size,
            color: g.color,
            flags,
            wmode,
            dir,
            ascender: g.ascender,
            descender: g.descender,
        }
    }

    /// The position along the line's reading axis (advance direction).
    fn along(&self) -> f64 {
        self.dir.0 * self.origin.x + self.dir.1 * self.origin.y
    }

    /// The position along the line's cross axis (the baseline coordinate).
    fn cross(&self) -> f64 {
        // Cross axis is `dir` rotated +90°: (-sin, cos).
        -self.dir.1 * self.origin.x + self.dir.0 * self.origin.y
    }

    /// The glyph cell's `[start, end]` projection onto the reading axis — the
    /// leading and trailing edges in advance order. Projecting the device bbox
    /// (not just the origin) keeps the inter-glyph gap correct for any writing
    /// direction / page rotation, mirroring the device-x gap `words.rs` uses for
    /// horizontal text.
    fn along_span(&self) -> (f64, f64) {
        let b = self.bbox.normalize();
        let (dx, dy) = self.dir;
        // Project all four corners; the extremes are the leading/trailing edges.
        let p = [
            dx * b.x0 + dy * b.y0,
            dx * b.x1 + dy * b.y0,
            dx * b.x0 + dy * b.y1,
            dx * b.x1 + dy * b.y1,
        ];
        let mut lo = p[0];
        let mut hi = p[0];
        for &v in &p[1..] {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        (lo, hi)
    }
}

/// The device-space writing-direction unit vector for a writing mode. We
/// transform the user-space advance direction (x+ for horizontal, y- for
/// vertical writing) through the page transform's linear part and normalize.
/// Falls back to `(1, 0)`. Independent of any individual glyph, so callers
/// compute it once per page transform.
fn writing_dir_vector(p: &Matrix, wmode: u8) -> (f64, f64) {
    // A unit advance step in user space. For horizontal writing the advance is
    // +x; for vertical writing the advance is -y (top-to-bottom).
    let (ux, uy) = if wmode == 1 { (0.0, -1.0) } else { (1.0, 0.0) };
    // Apply only the linear part of the page transform (drop translation).
    let dx = p.a * ux + p.c * uy;
    let dy = p.b * ux + p.d * uy;
    let n = (dx * dx + dy * dy).sqrt();
    if n <= f64::EPSILON {
        return (1.0, 0.0);
    }
    (dx / n, dy / n)
}

/// Font-name heuristics → italic / serif / mono / bold bits.
fn name_flags(name: &str) -> u32 {
    let lower = name.to_ascii_lowercase();
    let mut f = 0u32;
    if lower.contains("bold") || lower.contains("black") || lower.contains("heavy") {
        f |= flags::BOLD;
    }
    if lower.contains("italic") || lower.contains("oblique") {
        f |= flags::ITALIC;
    }
    if lower.contains("mono") || lower.contains("courier") || lower.contains("consol") {
        f |= flags::MONO;
    }
    if lower.contains("times")
        || lower.contains("serif") && !lower.contains("sans")
        || lower.contains("georgia")
        || lower.contains("roman")
        || lower.contains("minion")
        || lower.contains("garamond")
    {
        f |= flags::SERIF;
    }
    f
}

// === line grouping ========================================================

/// Clusters device glyphs into lines by cross-axis (baseline) proximity, then
/// splits each baseline run on column gutters and into spans. Lines are returned
/// in top-to-bottom order.
fn group_lines(dev: &[DevGlyph]) -> Vec<Line> {
    if dev.is_empty() {
        return Vec::new();
    }

    // Cluster by baseline (cross-axis). We keep paint order within a cluster so
    // we can sort by advance afterwards. A simple sweep keyed on cross value is
    // robust for the well-behaved inputs we target; rotated text uses the same
    // cross axis derived from `dir`.
    //
    // Tolerance keys on the larger of the cluster representative's effective
    // size and the candidate's. The size measure is device-space geometry, not
    // the raw `Tf` operand: PDFs may bake scale into either direction (`Tf 1`
    // plus a large CTM, or `Tf 327.68` plus a shrinking CTM). Geometry-first
    // sizing keeps both forms invariant while still joining superscripts.
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut cluster_cross: Vec<f64> = Vec::new();
    // Representative size/dir per cluster, kept in parallel arrays so the hot
    // inner scan never chases the `dev[clusters[ci][0]]` indirection (which is a
    // cache-hostile random access). Iteration order and tie-break are unchanged,
    // so the output is identical.
    let mut cluster_size: Vec<f64> = Vec::new();
    let mut cluster_dir: Vec<(f64, f64)> = Vec::new();

    for (i, g) in dev.iter().enumerate() {
        let cross = g.cross();
        let g_size = dev_glyph_effective_size(g);
        let g_dir = g.dir;
        let mut found = None;
        for ci in 0..cluster_cross.len() {
            let tol = cluster_size[ci].max(g_size).max(1.0) * LINE_TOL_FRAC;
            if (cluster_cross[ci] - cross).abs() <= tol + LINE_BASELINE_EPSILON
                && dir_matches(&cluster_dir[ci], &g_dir)
            {
                found = Some(ci);
                break;
            }
        }
        match found {
            Some(ci) => clusters[ci].push(i),
            None => {
                clusters.push(vec![i]);
                cluster_cross.push(cross);
                cluster_size.push(g_size);
                cluster_dir.push(g_dir);
            }
        }
    }

    // Sort clusters by their cross value (device y-down → top first). For
    // vertical writing the cross axis is x, so this still yields a stable
    // left-to-right column ordering of vertical lines.
    let mut order: Vec<usize> = (0..clusters.len()).collect();
    order.sort_by(|&a, &b| cluster_cross[a].total_cmp(&cluster_cross[b]));

    // Advance-order every cluster's glyphs first, so the gutter detector and the
    // splitter both see runs in reading order.
    let mut runs: Vec<Vec<usize>> = Vec::with_capacity(order.len());
    for ci in order {
        // Move the cluster's index list out (no clone — `clusters` is consumed).
        let mut idxs = std::mem::take(&mut clusters[ci]);
        // Order along the reading axis (advance direction). `along()` is a dot
        // product; sorting recomputes it for every comparison, so precompute it
        // once per glyph and sort the pairs (same ordering, fewer ops).
        if idxs.len() > 1 {
            let mut keyed: Vec<(f64, usize)> = idxs.iter().map(|&i| (dev[i].along(), i)).collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (slot, (_, i)) in idxs.iter_mut().zip(keyed) {
                *slot = i;
            }
        }
        runs.push(idxs);
    }

    // Detect the page's vertical column gutters from a glyph-segment occupancy
    // profile (robust to baselines that *merge* across the gutter; see
    // [`detect_page_gutters`]). These let us split a merged full-width baseline
    // (col-1-line + col-2-line clustered on one baseline) at the exact column
    // boundary even when the inter-column gap is only ~2–3× a word space — too
    // small for the local 4×font-size fallback to catch.
    let body_h = median_glyph_height(dev);
    let gutters = detect_page_gutters(&runs, dev);

    let mut final_runs: Vec<Vec<usize>> = Vec::new();
    for idxs in runs {
        // A single baseline cluster may straddle a column gutter; split it into
        // separate lines at each detected gutter it crosses (the principled cut),
        // or — as a fallback when no gutter applies — at a large along-axis gap.
        for col_run in split_on_gutter(&idxs, dev, &gutters, body_h) {
            // A per-column run can still hold two *distinct* baselines that the
            // content-order baseline sweep over-merged: when one column's line
            // baseline sits between two adjacent lines of the other column (tight
            // CSS line-height), the bridging line pulls both neighbours into one
            // cluster, and the gutter split leaves the two real lines interleaved
            // within a single column piece. Separate any such piece into its
            // distinct baselines so each emitted line is a single physical line —
            // otherwise `build_line`'s advance sort would weave the two baselines
            // character-by-character.
            for run in split_on_baseline(&col_run, dev) {
                final_runs.push(run);
            }
        }
    }
    // Reattach orphaned super/subscript fragment runs onto the line that
    // horizontally contains them. The cluster + gutter + baseline splitters each
    // key on a tight baseline tolerance, so a `Ts`-raised footnote marker `( 1 )`
    // or a clearly-smaller sub/superscript ends up as its own short run — read as a
    // stray line that scrambles both the word (`LNv`→`LN`+`v`) and the reading
    // order. Doing this **after** all splitting means nothing undoes it; requiring
    // x-containment keeps it column-safe (a left-column marker is never inside a
    // right-column line's x-span).
    reattach_fragment_runs(&mut final_runs, dev);
    let mut lines = Vec::with_capacity(final_runs.len());
    for mut run in final_runs {
        if is_rtl_run(&run, dev) {
            reorder_rtl_line(&mut run, dev);
        }
        // Content-order key: the smallest source-glyph index in this run, i.e. the
        // earliest-painted glyph of the line (document order).
        let seq = run.iter().copied().min().unwrap_or(0);
        let line_glyphs: Vec<&DevGlyph> = run.iter().map(|&i| &dev[i]).collect();
        lines.push(build_line(&line_glyphs, seq));
    }
    lines
}

/// Reattaches orphaned super/subscript fragment runs to the line run that
/// horizontally contains them (see the call site for the rationale). A fragment is
/// a short run (≤ [`FRAGMENT_MAX_WIDTH_FRAC`] glyph cells) whose x-centre lies inside
/// a wider run's x-span and whose baseline is within [`SUPERSCRIPT_RISE_FRAC`] of it;
/// it is merged into the nearest such host and the host re-sorted by advance so the
/// marker lands in reading position. Hosts that are themselves fragments-being-moved
/// are excluded, so no chaining. Operates on glyph-index runs in place.
fn reattach_fragment_runs(runs: &mut Vec<Vec<usize>>, dev: &[DevGlyph]) {
    let n = runs.len();
    if n < 2 {
        return;
    }
    // Per-run geometry: x-span, plus the baseline/size/dir of the run's tallest
    // glyph (its most representative body glyph).
    let mut x0 = vec![f64::INFINITY; n];
    let mut x1 = vec![f64::NEG_INFINITY; n];
    let mut cross = vec![0.0_f64; n];
    let mut size = vec![1.0_f64; n];
    let mut dir = vec![(1.0_f64, 0.0_f64); n];
    for (r, run) in runs.iter().enumerate() {
        let mut best_h = -1.0_f64;
        for &i in run {
            let b = dev[i].bbox.normalize();
            if b.x1 > b.x0 {
                x0[r] = x0[r].min(b.x0);
                x1[r] = x1[r].max(b.x1);
            }
            let h = b.height();
            if h > best_h {
                best_h = h;
                cross[r] = dev[i].cross();
                size[r] = dev[i].size.abs().max(h);
                dir[r] = dev[i].dir;
            }
        }
    }
    let mut merge_into: Vec<Option<usize>> = vec![None; n];
    for a in 0..n {
        if runs[a].is_empty() {
            continue;
        }
        let aw = x1[a] - x0[a];
        if aw <= 0.0 || aw > size[a].max(1.0) * FRAGMENT_MAX_WIDTH_FRAC {
            continue;
        }
        let a_mid = (x0[a] + x1[a]) * 0.5;
        let mut best: Option<(usize, f64)> = None;
        for b in 0..n {
            if b == a || runs[b].is_empty() || merge_into[b].is_some() {
                continue;
            }
            // Host is a wider line whose x-span contains the fragment's x-centre.
            if x1[b] - x0[b] <= aw || a_mid < x0[b] || a_mid > x1[b] {
                continue;
            }
            if !dir_matches(&dir[a], &dir[b]) {
                continue;
            }
            // CJK-safety + precision: the fragment must slot into a *horizontal gap*
            // of the host line — no host glyph covers the fragment's x-centre. A
            // footnote marker fills the `( )` gap (true gap); a short CJK column line
            // x-contained in a wider line would x-overlap that line's glyphs (no
            // gap), so it is never pulled in. This is the discriminator that height-
            // ratio could not provide (a `Ts`-raised full-size digit is not shorter).
            if runs[b].iter().any(|&i| {
                let gb = dev[i].bbox.normalize();
                gb.x0 <= a_mid && a_mid <= gb.x1
            }) {
                continue;
            }
            let dy = (cross[a] - cross[b]).abs();
            let rise = size[a].max(size[b]).max(1.0) * SUPERSCRIPT_RISE_FRAC;
            if dy <= rise && best.is_none_or(|(_, bd)| dy < bd) {
                best = Some((b, dy));
            }
        }
        if let Some((b, _)) = best {
            merge_into[a] = Some(b);
        }
    }
    let mut any = false;
    for a in 0..n {
        if let Some(b) = merge_into[a] {
            let moved = std::mem::take(&mut runs[a]);
            runs[b].extend(moved);
            any = true;
        }
    }
    if !any {
        return;
    }
    runs.retain(|r| !r.is_empty());
    // Re-sort every run by advance so a merged marker reads in position (idempotent
    // for runs that received nothing).
    for run in runs.iter_mut() {
        if run.len() > 1 {
            let mut keyed: Vec<(f64, usize)> = run.iter().map(|&i| (dev[i].along(), i)).collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (slot, (_, i)) in run.iter_mut().zip(keyed) {
                *slot = i;
            }
        }
    }
}

/// Separates a column run into its **distinct baselines**, returning each as its
/// own advance-ordered sub-run (top baseline first).
///
/// The content-order baseline sweep ([`group_lines`]) keys each cluster on its
/// seed glyph's cross value and admits any glyph within `LINE_TOL_FRAC × size`.
/// On a tight multi-column layout one column's line baseline can fall *between*
/// two adjacent lines of the neighbouring column; that bridging line is then
/// within tolerance of both, so all three baselines collapse into one cluster.
/// After the gutter split the column piece still carries two real baselines whose
/// glyphs interleave in content-stream order — and a plain advance sort would
/// weave them character-by-character (`is`+`of` → `iosf`).
///
/// We re-cluster the piece purely on the cross axis with single-link gaps: sort
/// by cross, break wherever consecutive cross values jump by more than the line
/// tolerance. Legitimate intra-line variation (super/subscripts, baseline jitter)
/// stays under the tolerance and is never split; genuinely distinct lines (a full
/// line height apart) separate cleanly. Returns the input unchanged when it holds
/// a single baseline (the overwhelmingly common case), so single-column and
/// already-correct lines are untouched.
fn split_on_baseline(idxs: &[usize], dev: &[DevGlyph]) -> Vec<Vec<usize>> {
    if idxs.len() < 2 {
        return vec![idxs.to_vec()];
    }
    // Order by cross (baseline), carrying each glyph's representative size so the
    // gap tolerance keys on the larger of the two adjacent baselines' sizes (same
    // rule as the line sweep). Device y-down, so smaller cross = higher on page.
    let mut keyed: Vec<(f64, f64, usize)> = idxs
        .iter()
        .map(|&i| (dev[i].cross(), dev_glyph_effective_size(&dev[i]), i))
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Find the baseline breakpoints: a jump larger than the line tolerance,
    // keyed (like the line sweep) on the larger of the two adjacent baselines'
    // representative sizes.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = vec![keyed[0].2];
    let mut prev_cross = keyed[0].0;
    let mut prev_size = keyed[0].1;
    for &(cross, size, i) in &keyed[1..] {
        let tol = size.max(prev_size).max(1.0) * LINE_TOL_FRAC;
        if cross - prev_cross > tol + LINE_BASELINE_EPSILON {
            groups.push(std::mem::take(&mut cur));
        }
        cur.push(i);
        prev_cross = cross;
        prev_size = size;
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    if groups.len() == 1 {
        // Single baseline: return the original advance-ordered run unchanged.
        return vec![idxs.to_vec()];
    }
    // Re-sort each distinct baseline by advance so it reads as a whole line.
    for g in &mut groups {
        if g.len() > 1 {
            let mut k: Vec<(f64, usize)> = g.iter().map(|&i| (dev[i].along(), i)).collect();
            k.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (slot, (_, i)) in g.iter_mut().zip(k) {
                *slot = i;
            }
        }
    }
    groups
}

/// Detects the page's vertical **column gutters** (x-midpoints, left→right) from
/// a glyph-occupancy profile, returning an empty vector when the page is not
/// multi-column.
///
/// The robustness comes from a property of PDF text: word spaces are emitted as
/// real space glyphs that fill the inter-word gaps, so *within* a column the
/// glyph occupancy stays high all the way across. Only a genuine inter-column
/// gutter is free of glyphs — even the space glyphs stop at the column edge. So
/// the gutter shows up as a clean near-zero valley in the per-x occupancy
/// histogram, even when many baselines *merge* across the gutter into one cluster
/// (the merged cluster's glyphs still stop at the left column edge and resume at
/// the right column edge, leaving the band empty).
///
/// A gutter is an interior near-empty band, at least `min_gap` wide, with a
/// populated column on each side (so a ragged right margin — empty on its right —
/// is not mistaken for a gutter). A genuinely continuous full-width line (a
/// centered title, a running header) fills its would-be gutter with glyphs and so
/// raises the occupancy there, correctly suppressing a false column; a small
/// tolerance lets a handful of such crossings through without hiding a real
/// gutter. Generalizes to 2, 3, or N columns.
fn detect_page_gutters(runs: &[Vec<usize>], dev: &[DevGlyph]) -> Vec<f64> {
    // Horizontal-writing region bounds + a representative glyph size.
    let mut rx0 = f64::INFINITY;
    let mut rx1 = f64::NEG_INFINITY;
    let mut size_sum = 0.0;
    let mut n_glyphs = 0usize;
    let mut n_lines = 0usize;
    for idxs in runs {
        if idxs.is_empty() || dev[idxs[0]].wmode == 1 {
            continue;
        }
        n_lines += 1;
        for &i in idxs {
            let b = dev[i].bbox.normalize();
            if b.x1 <= b.x0 {
                continue;
            }
            rx0 = rx0.min(b.x0);
            rx1 = rx1.max(b.x1);
            size_sum += dev[i].size.abs();
            n_glyphs += 1;
        }
    }
    if n_lines < 4 || n_glyphs < 16 || rx1 <= rx0 {
        return Vec::new();
    }
    let typ_size = (size_sum / n_glyphs as f64).max(1.0);
    let region_w = (rx1 - rx0).max(1.0);

    // Per-x glyph-occupancy histogram (1pt bins): how many lines have a glyph over
    // each x. Space glyphs count, so a column stays high across its whole width.
    let bin_w = 1.0_f64;
    let nbins = ((region_w / bin_w).ceil() as usize).max(1);
    let bin_of = |x: f64| -> usize {
        (((x - rx0) / bin_w).floor() as isize).clamp(0, nbins as isize - 1) as usize
    };
    let mut occ = vec![0u32; nbins];
    for idxs in runs {
        if idxs.is_empty() || dev[idxs[0]].wmode == 1 {
            continue;
        }
        for &i in idxs {
            let b = dev[i].bbox.normalize();
            if b.x1 <= b.x0 {
                continue;
            }
            let (lo, hi) = (bin_of(b.x0), bin_of(b.x1));
            for c in occ.iter_mut().take(hi + 1).skip(lo) {
                *c += 1;
            }
        }
    }

    // Typical column occupancy = median of the populated bins (robust to gutters
    // and ragged margins).
    let mut nonzero: Vec<u32> = occ.iter().copied().filter(|&v| v > 0).collect();
    if nonzero.is_empty() {
        return Vec::new();
    }
    nonzero.sort_unstable();
    let typical = nonzero[nonzero.len() / 2].max(1);
    // A gutter band is near-empty: ≤ a small fraction of the typical column
    // density (so a few full-width crossings — a title, a footer — are tolerated)
    // but never more than a couple of lines absolutely (a sparse page must not
    // turn every low bin into a gutter).
    let low = (((typical as f64) * 0.15).round() as u32).min(2);
    let side_floor = ((typical as f64) * 0.40).ceil().max(1.0) as u32;
    // A gutter is comfortably wider than inter-letter spacing (~0.4× font size),
    // but narrow letter-size gutters (a few points) still count.
    let min_gap = (typ_size * 0.4).max(2.0);

    // Maximal interior runs of near-empty bins are gutter candidates.
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    let mut run_start: Option<usize> = None;
    for (i, &v) in occ.iter().enumerate() {
        match (v <= low, run_start) {
            (true, None) => run_start = Some(i),
            (false, Some(s)) => {
                candidates.push((s, i));
                run_start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = run_start {
        candidates.push((s, nbins));
    }

    let median_occ = |a: usize, b: usize| -> u32 {
        if a >= b {
            return 0;
        }
        let mut v: Vec<u32> = occ[a..b].to_vec();
        v.sort_unstable();
        v[v.len() / 2]
    };

    let mut gutters: Vec<f64> = Vec::new();
    for (lo_bin, hi_bin) in candidates {
        let lo = (rx0 + lo_bin as f64 * bin_w).max(rx0);
        let hi = (rx0 + hi_bin as f64 * bin_w).min(rx1);
        // Strictly interior + wide enough.
        if lo <= rx0 + 1e-6 || hi >= rx1 - 1e-6 || hi - lo < min_gap {
            continue;
        }
        // A real column on each side. The major (denser) side must be a full
        // column (≥ side_floor); the minor side may be a *sparse* secondary column
        // — overflow text, marginalia, a narrow sidebar — which fitz reads as its
        // own block. A clean interior empty band (occupancy ≤ low across its whole
        // width) with even a few lines of text past it is a genuine column
        // boundary, not a ragged margin: a ragged right margin never forms a clean
        // interior empty band followed by a populated strip, so this does not
        // over-split plain prose (the band there runs to the page edge and is
        // rejected as non-interior above).
        let left_occ = median_occ(0, lo_bin);
        let right_occ = median_occ(hi_bin.min(nbins), nbins);
        let major = left_occ.max(right_occ);
        let minor = left_occ.min(right_occ);
        // A sparse secondary column still needs a minimal real presence (≥ 2 lines)
        // so a one-off stray glyph in the margin never manufactures a column.
        const MINOR_SIDE_FLOOR: u32 = 2;
        if major >= side_floor && minor >= MINOR_SIDE_FLOOR {
            gutters.push((lo + hi) / 2.0);
        }
    }
    gutters.sort_by(f64::total_cmp);
    gutters
}

/// Splits an advance-ordered baseline run into per-column sub-runs. A break is
/// taken wherever the run crosses a detected page column `gutter` (the principled
/// cut), or — as a fallback when no gutter applies — wherever the along-axis gap
/// between consecutive glyph edges reaches the independent-run threshold.
/// Normal inter-word spaces never trigger either rule.
///
/// A large-type heading/title legitimately spans the body's column gutters (e.g.
/// a centered title over a multi-column page); when this cluster's glyphs are
/// clearly taller than the body text it is kept whole and only split on a
/// genuinely huge gap.
fn split_on_gutter(
    idxs: &[usize],
    dev: &[DevGlyph],
    gutters: &[f64],
    body_h: f64,
) -> Vec<Vec<usize>> {
    let cluster_h = run_glyph_height(idxs, dev);
    let is_heading = body_h > 0.0 && cluster_h > body_h * 1.6;
    let gutters: &[f64] = if is_heading { &[] } else { gutters };

    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut prev_end: Option<f64> = None;
    let mut prev_size: Option<f64> = None;
    // The device-x left edge of the previous glyph, so a gutter crossing fires
    // even when a wide glyph's bbox straddles the gutter line. Gutters are
    // device-x midpoints (from [`detect_page_gutters`], over horizontal glyphs),
    // so the crossing test uses device x; the huge-gap fallback uses the reading
    // axis so it still works for rotated text.
    let mut prev_x0: Option<f64> = None;
    for &i in idxs {
        let g = &dev[i];
        // Project the glyph's leading/trailing edges onto the reading axis.
        let (start, end) = g.along_span();
        let effective_size = dev_glyph_effective_size(g);
        let x0 = g.bbox.normalize().x0;
        if let Some(pe) = prev_end {
            let px = prev_x0.unwrap_or(x0);
            // Cut where a detected gutter separates this glyph from the previous
            // one: the previous glyph starts left of the gutter and this glyph
            // starts at/right of it.
            let crosses_gutter = gutters.iter().any(|&gx| px < gx - 0.5 && x0 >= gx - 0.5);
            // Fallback: two independently painted runs with a device-space gap
            // at the compatibility boundary form distinct lines. Use the true
            // projected glyph edges — origin + AABB diagonal overestimates the
            // prior extent and hides this split on CTM-scaled PDFs.
            let gap_size = prev_size.unwrap_or(effective_size).max(effective_size);
            let independent_run_gap =
                start - pe >= gap_size * LINE_RUN_GAP_FRAC - LINE_RUN_GAP_EPSILON;
            if crosses_gutter || independent_run_gap {
                runs.push(std::mem::take(&mut cur));
            }
        }
        cur.push(i);
        prev_end = Some(end);
        prev_size = Some(effective_size);
        prev_x0 = Some(x0);
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

/// Device-space font-size estimate for a positioned glyph. Raw `Tf` can be much
/// smaller *or* larger than the rendered size when reciprocal scale lives in the
/// text matrix / CTM, so axis-aligned cell geometry is authoritative. For an
/// oblique run, however, `bbox` is already an axis-aligned envelope; projecting
/// that envelope again double-counts the glyph's advance extent. Keep the raw
/// size there until the interpreter exposes an exact cell quad.
fn dev_glyph_effective_size(glyph: &DevGlyph) -> f64 {
    let raw = glyph.size.abs();
    let axis_aligned = glyph.dir.0.abs() <= 1e-6 || glyph.dir.1.abs() <= 1e-6;
    if !axis_aligned && raw.is_finite() && raw > f64::EPSILON {
        return raw;
    }

    let normal = (-glyph.dir.1, glyph.dir.0);
    let bbox = glyph.bbox.normalize();
    let projected = if normal.0.abs() >= normal.1.abs() {
        bbox.width()
    } else {
        bbox.height()
    };
    let metric_height = (glyph.ascender - glyph.descender).abs();
    let geometric = if metric_height > f64::EPSILON {
        projected / metric_height
    } else {
        0.0
    };
    if geometric.is_finite() && geometric > f64::EPSILON {
        geometric
    } else {
        raw.max(f64::EPSILON)
    }
}

/// The median glyph cell height over all horizontal-writing glyphs — a robust
/// estimate of the page's body text size. Vertical-writing glyphs are excluded.
fn median_glyph_height(dev: &[DevGlyph]) -> f64 {
    let mut hs: Vec<f64> = dev
        .iter()
        .filter(|g| g.wmode != 1)
        .map(|g| g.bbox.height())
        .filter(|h| *h > 0.0)
        .collect();
    if hs.is_empty() {
        return 0.0;
    }
    hs.sort_by(f64::total_cmp);
    hs[hs.len() / 2]
}

/// The median glyph cell height within one baseline cluster (used to spot a
/// large-type heading that must not be split at a body column gutter).
fn run_glyph_height(idxs: &[usize], dev: &[DevGlyph]) -> f64 {
    let mut hs: Vec<f64> = idxs
        .iter()
        .map(|&i| dev[i].bbox.height())
        .filter(|h| *h > 0.0)
        .collect();
    if hs.is_empty() {
        return 0.0;
    }
    hs.sort_by(f64::total_cmp);
    hs[hs.len() / 2]
}

/// Two writing directions match if their unit vectors are within ~5°.
fn dir_matches(a: &(f64, f64), b: &(f64, f64)) -> bool {
    let dot = a.0 * b.0 + a.1 * b.1;
    dot > 0.996 // cos(5°) ≈ 0.9962
}

/// Detects a predominantly right-to-left run (Hebrew/Arabic blocks).
fn is_rtl_run(idxs: &[usize], dev: &[DevGlyph]) -> bool {
    let mut rtl = 0usize;
    let mut strong = 0usize;
    for &i in idxs {
        for c in dev[i].text.chars() {
            if is_rtl_char(c) {
                rtl += 1;
                strong += 1;
            } else if c.is_alphabetic() {
                strong += 1;
            }
        }
    }
    strong > 0 && rtl * 2 > strong
}

/// Bidi class of a glyph, for the simplified UAX#9 reorder of an RTL line.
#[derive(Clone, Copy, PartialEq)]
enum BidiClass {
    /// Strong right-to-left (Arabic/Hebrew letters).
    Rtl,
    /// Strong left-to-right (Latin/other LTR letters) and European/Arabic
    /// numbers — treated as an LTR run so digit groups read left-to-right.
    Ltr,
    /// Neutral/weak (spaces, punctuation) — resolved from its neighbours.
    Neutral,
}

/// Classifies a glyph by the strong direction of its characters. A glyph carries
/// the base-letter string (post-ToUnicode), usually one char but possibly a
/// ligature; we take the first strongly-directional char, falling back to
/// neutral when the glyph is all whitespace/punctuation.
fn glyph_bidi_class(g: &DevGlyph) -> BidiClass {
    for c in g.text.chars() {
        if is_rtl_char(c) {
            return BidiClass::Rtl;
        }
        if c.is_ascii_digit() || matches!(c as u32, 0x0660..=0x0669 | 0x06F0..=0x06F9) {
            // European digits and Arabic-Indic / extended digits read LTR.
            return BidiClass::Ltr;
        }
        if c.is_alphabetic() {
            return BidiClass::Ltr;
        }
    }
    BidiClass::Neutral
}

/// Reorders one predominantly-RTL line from visual (advance) order into logical
/// order, per a simplified UAX#9.
///
/// The run arrives in advance order (visual left→right). For a right-to-left
/// line the logical order is the whole line reversed, *except* that contiguous
/// strong-LTR sub-runs (Latin words, digit groups like `100` or `1990`) must
/// keep their left-to-right order — a blanket reverse would mangle them
/// (`100`→`001`, `Linux`→`xuniL`). We therefore reverse the whole line, then
/// re-reverse each maximal run of LTR glyphs to restore their internal order.
///
/// Neutrals (spaces, punctuation) are resolved by the rule: a neutral flanked by
/// LTR on both sides joins the LTR run (so `Apple Inc` stays intact); otherwise
/// it stays with the surrounding RTL context. This matches fitz's logical-order
/// output on mixed Arabic+Latin/number lines.
fn reorder_rtl_line(run: &mut [usize], dev: &[DevGlyph]) {
    if run.len() < 2 {
        return;
    }

    // Resolve each glyph's effective direction (LTR vs RTL) in visual order.
    // Neutrals take LTR only when both nearest strong neighbours are LTR; the
    // line base is RTL, so any other neutral defaults to RTL.
    let classes: Vec<BidiClass> = run.iter().map(|&i| glyph_bidi_class(&dev[i])).collect();
    let n = classes.len();
    let mut is_ltr = vec![false; n];
    for (k, &cls) in classes.iter().enumerate() {
        match cls {
            BidiClass::Ltr => is_ltr[k] = true,
            BidiClass::Rtl => is_ltr[k] = false,
            BidiClass::Neutral => {
                let prev = classes[..k]
                    .iter()
                    .rev()
                    .find(|&&c| c != BidiClass::Neutral);
                let next = classes[k + 1..].iter().find(|&&c| c != BidiClass::Neutral);
                is_ltr[k] = prev == Some(&BidiClass::Ltr) && next == Some(&BidiClass::Ltr);
            }
        }
    }

    // Reverse the whole line (visual → RTL logical base order)…
    run.reverse();
    is_ltr.reverse();

    // …then re-reverse each maximal contiguous LTR sub-run so its glyphs read
    // left-to-right again.
    let mut k = 0;
    while k < n {
        if is_ltr[k] {
            let start = k;
            while k < n && is_ltr[k] {
                k += 1;
            }
            run[start..k].reverse();
        } else {
            k += 1;
        }
    }
}

/// Whether a char is in a strong-RTL Unicode block (Hebrew, Arabic).
fn is_rtl_char(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x05FF // Hebrew
        | 0x0600..=0x06FF // Arabic
        | 0x0700..=0x074F // Syriac
        | 0x0750..=0x077F // Arabic Supplement
        | 0x08A0..=0x08FF // Arabic Extended-A
        | 0xFB1D..=0xFB4F // Hebrew presentation forms
        | 0xFB50..=0xFDFF // Arabic presentation forms-A
        | 0xFE70..=0xFEFF // Arabic presentation forms-B
    )
}

/// Builds a [`Line`] from advance-ordered glyphs, splitting into spans where the
/// style (font / size / color / flags) changes. `seq` is the line's content-order
/// key (smallest source-glyph index).
/// Detects **letter-spaced (tracked)** runs and returns a per-glyph mask: `mask[i]`
/// marks ink glyph `i` whose preceding inter-word space must be **suppressed**. Some
/// PDFs render an emphasised heading by tracking every letter apart — "Abschnitt"
/// painted as `A b s c h n i t t` with no literal space glyph, just a uniform
/// per-letter gap wider than the word-gap threshold. `build_line` would synthesize a
/// space at each gap, shattering the word into single-char tokens and tanking text
/// accuracy. A run qualifies only when ≥4 consecutive **single-char** ink glyphs
/// sit at uniform tracking gaps (so initials "J R R" or "x = y" are never
/// touched). The tracking gap is measured between **alphanumeric** neighbours
/// only and must stay below [`LETTER_SPACING_MAX_FRAC`] of the font size (no
/// lower bound: text tracked right at the word-gap threshold keeps a
/// kern-loosened pair together — whole words, not PyMuPDF's "transpor t"): a
/// dot leader `. . . .` or a rule `- - -` is repeated punctuation, not
/// tracking, and must not lend its uniform gap to the letters around it — on a
/// positioned TOC line the leader's dots outnumber the letters, so a line-wide
/// median once read every real word gap as tracking
/// ("Originandscopeofrightofdeduction"); and a uniform gap wider than a word
/// space (map labels `O R E G O N`) is a row of single-letter words, exactly as
/// PyMuPDF reads it. A gap clearly wider than
/// the tracking gap — beyond 1.8× the run's median *and* beyond the median plus
/// the word-gap threshold, e.g. before the number in "Abschnitt 2" — is a real
/// word break and is left unmarked, so bimodal lines split on their wide gaps.
///
/// `size` is the line's device-space font size and `gap_thresh` the word-gap
/// threshold derived from it (both from `build_line`).
fn letter_spacing_skip_mask(glyphs: &[&DevGlyph], size: f64, gap_thresh: f64) -> Vec<bool> {
    let n = glyphs.len();
    let mut mask = vec![false; n];
    let is_space = |g: &DevGlyph| !g.text.is_empty() && g.text.chars().all(is_synth_ws);
    // Ink glyphs (drop literal spaces) in reading order.
    let ink: Vec<usize> = (0..n).filter(|&i| !is_space(glyphs[i])).collect();
    if ink.len() < 4 {
        return mask;
    }
    let single_char = |i: usize| {
        let mut chars = glyphs[i].text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    };
    let single = |i: usize| single_char(i).is_some();
    let letter = |i: usize| single_char(i).is_some_and(char::is_alphanumeric);
    let gap = |k: usize| glyphs[ink[k]].along_span().0 - glyphs[ink[k - 1]].along_span().1;
    let max_track = size * LETTER_SPACING_MAX_FRAC;
    // Maximal runs of consecutive single-char ink glyphs.
    let mut a = 0;
    while a < ink.len() {
        if !single(ink[a]) {
            a += 1;
            continue;
        }
        let mut b = a + 1;
        while b < ink.len() && single(ink[b]) {
            b += 1;
        }
        if b - a >= 4 {
            // Tracking gap = median of the run's letter-to-letter gaps (punctuation
            // contributes none). Only a gap narrower than a word space is tracking;
            // then suppress the space before any glyph whose gap is not clearly
            // wider (a word break). No lower bound: tracking set right at the
            // word-gap threshold (eurlex body text, `0.15 Tc`) must still hold a
            // kern-loosened pair together — "transport", not "transpor t".
            let mut gaps: Vec<f64> = (a + 1..b)
                .filter(|&k| letter(ink[k - 1]) && letter(ink[k]))
                .map(gap)
                .collect();
            if gaps.len() >= 3 {
                gaps.sort_by(f64::total_cmp);
                let med = gaps[gaps.len() / 2].max(0.01);
                if med <= max_track {
                    let wide = (med * 1.8).min(med + gap_thresh);
                    for k in a + 1..b {
                        if gap(k) <= wide {
                            mask[ink[k]] = true;
                        }
                    }
                }
            }
        }
        a = b;
    }
    mask
}

fn build_line(glyphs: &[&DevGlyph], seq: usize) -> Line {
    let wmode = glyphs.first().map_or(0, |g| g.wmode);
    let dir = glyphs.first().map_or((1.0, 0.0), |g| g.dir);

    // Determine the line baseline (median cross) to flag superscripts per glyph.
    let mut crosses: Vec<f64> = glyphs.iter().map(|g| g.cross()).collect();
    crosses.sort_by(f64::total_cmp);
    let baseline = crosses.get(crosses.len() / 2).copied().unwrap_or(0.0);

    // A stable per-line *device-space font size* for the word-gap threshold: the
    // median of the per-glyph size estimates (cell height ÷ (ascent − descent),
    // see `dev_glyph_effective_size`), invariant to whether the text scale lives
    // in the `Tf` operand or the CTM. The raw operand `g.size` is in *text*
    // space, so comparing it against device-space gaps collapses the threshold
    // on PMC/LaTeX PDFs that emit `Tf 1` and bake the scale into the matrix —
    // shattering words and URLs. Keying on the *font size* rather than the raw
    // cell height (MuPDF: `spacing = gap / size`) keeps the threshold stable
    // across descriptors: a legal but short cell (`/Ascent 500 /Descent 0`)
    // otherwise halved it and turned ordinary kerning into word gaps. Falls back
    // to the raw size only for degenerate lines with no finite estimate.
    let eff_size = {
        let mut sizes: Vec<f64> = glyphs
            .iter()
            .map(|g| dev_glyph_effective_size(g))
            .filter(|s| s.is_finite() && *s > 0.0)
            .collect();
        if sizes.is_empty() {
            glyphs.first().map_or(0.0, |g| g.size.abs())
        } else {
            sizes.sort_by(f64::total_cmp);
            sizes[sizes.len() / 2]
        }
    };
    let gap_thresh = eff_size * WORD_GAP_FRAC;

    let mut spans: Vec<Span> = Vec::new();
    let mut line_bbox = Rect::default();
    // Trailing reading-axis edge of the previously emitted glyph + the last char
    // we emitted, so we can synthesize one inter-word space whenever a spatial
    // gap exceeds the word-gap threshold — the layout-stage word-space synthesis
    // that `serialize`/`words` rely on (mirrors [`crate::words`]).
    let mut prev_end: Option<f64> = None;
    let mut prev_char: Option<char> = None;

    // Per-glyph mask: suppress the synthesized word space before a glyph that sits
    // mid letter-spaced (tracked) run, so emphasised headings like "A b s c h n i t t"
    // collapse to "Abschnitt" instead of shattering into single-char tokens.
    let suppress_space_before = letter_spacing_skip_mask(glyphs, eff_size, gap_thresh);

    for (gi, g) in glyphs.iter().enumerate() {
        let mut gflags = g.flags;
        // Superscript: device y-down, so a glyph painted *above* the baseline
        // has a smaller cross value. A meaningful negative shift sets bit0.
        if baseline - g.cross() > g.size.abs() * 0.1 {
            gflags |= flags::SUPERSCRIPT;
        }

        let can_merge = spans.last().is_some_and(|s| {
            s.font == g.font
                && (s.size - g.size).abs() < 1e-6
                && s.color == g.color
                && s.flags == gflags
        });
        // A glyph may carry several Unicode scalars (a ligature); each becomes a
        // `Char` sharing the glyph cell geometry, so no text is dropped.
        let target = if can_merge {
            spans.last_mut().unwrap()
        } else {
            spans.push(Span {
                bbox: g.bbox,
                font: g.font.clone(),
                size: g.size,
                flags: gflags,
                color: g.color,
                ascender: g.ascender,
                descender: g.descender,
                origin: g.origin,
                chars: Vec::new(),
                text: String::new(),
            });
            spans.last_mut().unwrap()
        };

        // Synthesize an inter-word space from a spatial gap wider than the
        // word-gap threshold (same rule as `words.rs`), so text/dict/blocks word
        // boundaries match `get_text("words")` on `TJ`-kerned PDFs that emit no
        // literal space glyph. Skip it when either side is already whitespace, so
        // PDFs that *do* emit real space glyphs are never double-spaced. The
        // space joins this glyph's (new or merged) span so it falls between the
        // two words in document order regardless of a coinciding style change.
        let (lead, end) = g.along_span();
        let first_char = g.text.chars().next();
        if let (Some(pe), Some(pc), Some(fc)) = (prev_end, prev_char, first_char) {
            let gap = lead - pe;
            if gap > gap_thresh
                && !suppress_space_before[gi]
                && !is_synth_ws(pc)
                && !is_synth_ws(fc)
            {
                target.text.push(' ');
                target.chars.push(Char {
                    // A thin zero-width cell at the new word's origin keeps the
                    // rawdict char array consistent without inflating any bbox.
                    origin: g.origin,
                    bbox: Rect::new(g.origin.x, g.bbox.y0, g.origin.x, g.bbox.y1),
                    c: ' ',
                });
                prev_char = Some(' ');
            }
        }

        target.bbox = target.bbox.union(&g.bbox);
        for c in g.text.chars() {
            target.text.push(c);
            target.chars.push(Char {
                origin: g.origin,
                bbox: g.bbox,
                c,
            });
            prev_char = Some(c);
        }
        prev_end = Some(end);
        line_bbox = line_bbox.union(&g.bbox);
    }

    Line {
        bbox: line_bbox,
        wmode,
        dir,
        spans,
        seq,
    }
}

/// Whether a char already counts as whitespace for space synthesis — ASCII
/// whitespace or NBSP. Mirrors `words::is_word_separator` so the layout and the
/// word segmenter agree on what already separates two words (no double space).
fn is_synth_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{00A0}'
}

// === block grouping =======================================================

/// Column-aware paragraph grouping (PRD §8.6.2).
///
/// The previous grouping walked lines strictly top-to-bottom and started a new
/// block whenever the next line failed to x-overlap the current one. On a
/// multi-column page this produced one *single-line* block per column row
/// (left, right, left, right, …) — and a downstream geometric sort then
/// interleaved the columns line-by-line, which is the dominant reading-order
/// divergence vs fitz.
///
/// Instead we first partition the lines into **column regions** with a recursive
/// XY-cut on the line bounding boxes (a vertical gutter splits columns; a
/// horizontal gutter splits stacked regions like a full-width header above a
/// two-column body). Within each leaf region the lines are grouped into
/// paragraph blocks by vertical proximity, so a block never straddles a column.
fn group_blocks_columned(lines: Vec<Line>, width: f64, height: f64) -> Vec<Block> {
    if lines.is_empty() {
        return Vec::new();
    }
    let idxs: Vec<usize> = (0..lines.len()).collect();
    let table_dominant = is_table_dominant(&lines);
    let regions: Vec<Vec<usize>> = if table_dominant {
        vec![idxs]
    } else {
        let mut cut = Vec::new();
        cut_lines(&lines, &idxs, width, height, &mut cut);
        cut
    };

    // A fine paragraph split inside both halves of a multi-column layout would
    // make the later content-order sort interleave the columns line-by-line.
    // Identify only genuinely side-by-side regions here; single-column and
    // vertically stacked regions keep the fine compatibility grouping.
    let region_boxes: Vec<Rect> = regions
        .iter()
        .map(|region| {
            region
                .iter()
                .fold(Rect::default(), |bbox, &i| bbox.union(&lines[i].bbox))
                .normalize()
        })
        .collect();
    let side_by_side: Vec<bool> = region_boxes
        .iter()
        .enumerate()
        .map(|(i, bbox)| {
            region_boxes
                .iter()
                .enumerate()
                .any(|(j, other)| i != j && regions_are_side_by_side(bbox, other))
        })
        .collect();

    // Each ordinary block is one order group. A side-by-side column is an atomic
    // group: its fine-grained blocks stay contiguous and top-to-bottom instead
    // of being interleaved with the neighbouring column by content sequence.
    let mut order_groups: Vec<(usize, Vec<Block>)> = Vec::new();
    // `lines` is consumed region-by-region: move each line out exactly once.
    let mut slots: Vec<Option<Line>> = lines.into_iter().map(Some).collect();
    for (region_index, region) in regions.into_iter().enumerate() {
        // Take the region's lines (top-to-bottom) and split into paragraphs.
        let mut region_lines: Vec<Line> = region
            .iter()
            .map(|&i| slots[i].take().expect("each line placed once"))
            .collect();
        region_lines.sort_by_key(|line| line.seq);
        let mut region_blocks = Vec::new();
        group_region_paragraphs(region_lines, &mut region_blocks);
        if side_by_side[region_index] {
            let order_key = region_blocks
                .iter()
                .map(|block| block.seq)
                .min()
                .unwrap_or(usize::MAX);
            order_groups.push((order_key, region_blocks));
        } else {
            order_groups.extend(
                region_blocks
                    .into_iter()
                    .map(|block| (block.seq, vec![block])),
            );
        }
    }
    order_groups.sort_by_key(|(order_key, _)| *order_key);
    order_groups
        .into_iter()
        .flat_map(|(_, blocks)| blocks)
        .collect()
}

/// Detects a page region dominated by a seeded grid. Three-or-more same-row
/// cells prove that the region is a table; two-cell rows may then support its
/// coverage (for subtotals, spanning headers, and continuation rows). This is
/// deliberately conservative: two-column prose never creates the three-cell
/// seed required to pass the detector.
fn is_table_dominant(lines: &[Line]) -> bool {
    let visible: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line_has_visible_text(line).then_some(index))
        .collect();
    if visible.len() < TABLE_MIN_VISIBLE_LINES {
        return false;
    }

    // Bucket near-identical writing directions, then retain the dominant one so
    // a rotated caption or marginal note cannot manufacture baseline clusters.
    let mut direction_groups: Vec<(u8, (f64, f64), Vec<usize>)> = Vec::new();
    for index in visible.iter().copied() {
        let line = &lines[index];
        if let Some((_, _, members)) = direction_groups.iter_mut().find(|(wmode, dir, _)| {
            *wmode == line.wmode && dir.0 * line.dir.0 + dir.1 * line.dir.1 >= 0.999
        }) {
            members.push(index);
        } else {
            direction_groups.push((line.wmode, line.dir, vec![index]));
        }
    }
    let Some((_, dominant_dir, dominant)) = direction_groups
        .into_iter()
        .max_by_key(|(_, _, members)| members.len())
    else {
        return false;
    };
    if (dominant.len() as f64) < visible.len() as f64 * TABLE_DOMINANT_DIR_FRAC {
        return false;
    }

    let normal = (-dominant_dir.1, dominant_dir.0);
    let mut baselines = Vec::with_capacity(dominant.len());
    let mut sizes = Vec::with_capacity(dominant.len());
    for index in dominant {
        let metrics = paragraph_line_metrics(&lines[index]);
        baselines.push(metrics.baseline_origin.x * normal.0 + metrics.baseline_origin.y * normal.1);
        sizes.push(metrics.effective_size);
    }
    let Some(typical_size) = median_finite(&mut sizes) else {
        return false;
    };
    let tolerance = (typical_size * TABLE_BASELINE_TOL_FRAC)
        .clamp(TABLE_BASELINE_TOL_MIN, TABLE_BASELINE_TOL_MAX);
    baselines.sort_by(f64::total_cmp);

    let mut dense_baselines = 0usize;
    let mut support_lines = 0usize;
    let mut index = 0usize;
    while index < baselines.len() {
        let anchor = baselines[index];
        let mut end = index + 1;
        while end < baselines.len() && baselines[end] - anchor <= tolerance {
            end += 1;
        }
        let cluster_size = end - index;
        if cluster_size >= 2 {
            support_lines += cluster_size;
        }
        if cluster_size >= TABLE_MIN_CELLS_PER_BASELINE {
            dense_baselines += 1;
        }
        index = end;
    }

    dense_baselines >= TABLE_MIN_DENSE_BASELINES
        && support_lines as f64 >= visible.len() as f64 * TABLE_SUPPORT_LINE_FRAC
}

fn line_has_visible_text(line: &Line) -> bool {
    line.spans
        .iter()
        .flat_map(|span| span.chars.iter())
        .any(|ch| !ch.c.is_whitespace())
}

/// Whether two region boxes form horizontally disjoint columns with substantial
/// vertical overlap. Touching edges count as disjoint; glyph bboxes from two
/// columns do not need a rounded-coordinate gap to prove separation.
fn regions_are_side_by_side(a: &Rect, b: &Rect) -> bool {
    let horizontally_disjoint = a.x1 <= b.x0 || b.x1 <= a.x0;
    let vertical_overlap = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let shorter_height = a.height().min(b.height()).max(f64::EPSILON);
    horizontally_disjoint && vertical_overlap >= shorter_height * COLUMN_REGION_OVERLAP_FRAC
}

/// Groups one column region's (y-sorted) lines into paragraph blocks by baseline,
/// writing direction, and paragraph-indent compatibility, appending to `out`.
fn group_region_paragraphs(lines: Vec<Line>, out: &mut Vec<Block>) {
    let mut cur: Vec<Line> = Vec::new();
    let mut prev: Option<ParagraphLineMetrics> = None;
    for line in lines {
        let metrics = paragraph_line_metrics(&line);
        let start_new = match prev {
            None => false,
            Some(prior) => {
                let same_direction = prior.wmode == metrics.wmode
                    && prior.dir.0 * metrics.dir.0 + prior.dir.1 * metrics.dir.1 >= 0.999;
                if !same_direction {
                    true
                } else {
                    let normal = (-metrics.dir.1, metrics.dir.0);
                    let baseline_delta = (
                        metrics.baseline_origin.x - prior.baseline_origin.x,
                        metrics.baseline_origin.y - prior.baseline_origin.y,
                    );
                    let baseline_step = (baseline_delta.0 * normal.0 + baseline_delta.1 * normal.1)
                        .abs()
                        / metrics.effective_size.max(f64::EPSILON);
                    let start_delta = (
                        metrics.start_origin.x - prior.start_origin.x,
                        metrics.start_origin.y - prior.start_origin.y,
                    );
                    let indent = start_delta.0 * metrics.dir.0 + start_delta.1 * metrics.dir.1;
                    baseline_step > BLOCK_BASELINE_FAR
                        || (baseline_step >= BLOCK_BASELINE_NEAR
                            && metrics.wmode == 0
                            && indent > BLOCK_INDENT_EPSILON
                            && !prior.starts_with_bullet)
                }
            }
        };
        if start_new && !cur.is_empty() {
            push_text_blocks(std::mem::take(&mut cur), out);
        }
        prev = Some(metrics);
        cur.push(line);
    }
    if !cur.is_empty() {
        push_text_blocks(cur, out);
    }
}

/// Device-space measurements needed for PyMuPDF-compatible paragraph breaks.
#[derive(Clone, Copy)]
struct ParagraphLineMetrics {
    baseline_origin: Point,
    effective_size: f64,
    start_origin: Point,
    wmode: u8,
    dir: (f64, f64),
    starts_with_bullet: bool,
}

/// Measures a reconstructed line in the same coordinate frame as its glyphs.
/// `Span::size` can be only `1` when the real scale lives in the CTM, so the
/// effective size is recovered from character geometry and font metrics first.
fn paragraph_line_metrics(line: &Line) -> ParagraphLineMetrics {
    let dir = line.dir;
    let normal = (-dir.1, dir.0);
    let dot = |p: Point, axis: (f64, f64)| p.x * axis.0 + p.y * axis.1;

    let mut origins = Vec::new();
    let mut sizes = Vec::new();
    let mut fallback_sizes = Vec::new();
    for span in &line.spans {
        if span.size.is_finite() && span.size.abs() > f64::EPSILON {
            fallback_sizes.push(span.size.abs());
        }
        let metric_height = (span.ascender - span.descender).abs();
        for ch in &span.chars {
            if ch.origin.x.is_finite() && ch.origin.y.is_finite() {
                origins.push(ch.origin);
            }
            if metric_height.is_finite() && metric_height > f64::EPSILON {
                let bbox = ch.bbox.normalize();
                let projected = normal.0.abs() * bbox.width() + normal.1.abs() * bbox.height();
                let size = projected / metric_height;
                if size.is_finite() && size > f64::EPSILON {
                    sizes.push(size);
                }
            }
        }
    }
    if origins.is_empty() {
        origins.extend(
            line.spans
                .iter()
                .map(|span| span.origin)
                .filter(|origin| origin.x.is_finite() && origin.y.is_finite()),
        );
    }

    let fallback_origin = {
        let bbox = line.bbox.normalize();
        Point::new(bbox.x0, bbox.y0)
    };
    let mut baselines: Vec<f64> = origins.iter().map(|origin| dot(*origin, normal)).collect();
    let baseline_value =
        median_finite(&mut baselines).unwrap_or_else(|| dot(fallback_origin, normal));
    let baseline_origin = origins
        .iter()
        .min_by(|left, right| {
            (dot(**left, normal) - baseline_value)
                .abs()
                .total_cmp(&(dot(**right, normal) - baseline_value).abs())
        })
        .copied()
        .unwrap_or(fallback_origin);
    let effective_size = median_finite(&mut sizes)
        .or_else(|| median_finite(&mut fallback_sizes))
        .unwrap_or(1.0)
        .max(f64::EPSILON);
    let start_origin = origins
        .iter()
        .min_by(|left, right| dot(**left, dir).total_cmp(&dot(**right, dir)))
        .copied()
        .unwrap_or(fallback_origin);

    ParagraphLineMetrics {
        baseline_origin,
        effective_size,
        start_origin,
        wmode: line.wmode,
        dir,
        starts_with_bullet: line_starts_with_plausible_bullet(line),
    }
}

/// Returns the upper median of finite values, matching the robust medians used
/// elsewhere in this layout pipeline.
fn median_finite(values: &mut Vec<f64>) -> Option<f64> {
    values.retain(|v| v.is_finite());
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

/// A compact compatibility set for the bullet and numbered-list forms exercised
/// by the public corpus. A bullet suppresses the ordinary first-line-indent
/// paragraph split for its continuation line.
fn line_starts_with_plausible_bullet(line: &Line) -> bool {
    let first = line
        .spans
        .iter()
        .flat_map(|span| span.chars.iter())
        .map(|ch| ch.c)
        .find(|c| !c.is_whitespace());
    first.is_some_and(|c| {
        c.is_ascii_digit()
            || matches!(
                c,
                '*' | '\u{00B7}'
                    | '\u{2022}'
                    | '\u{2023}'
                    | '\u{2043}'
                    | '\u{2219}'
                    | '\u{25CB}'
                    | '\u{25CF}'
                    | '\u{25E6}'
            )
    })
}

/// Recursive XY-cut over **lines** into column / band regions.
///
/// At each node it considers the single widest *empty gutter* on each axis (a
/// coordinate band that no line's projected interval crosses) and cuts on
/// whichever gutter is wider. Cutting on the wider gutter is what lets a
/// full-width header/title — which bridges the column gutter and would otherwise
/// block a vertical cut — be peeled off by a horizontal cut first; the remaining
/// pure multi-column body then yields a clean vertical (column) cut. A vertical
/// gutter is a coverage valley at least `min_x_gut` wide (see [`column_gutter`]);
/// a horizontal gutter must clear ~1.3 typical line heights (so paragraph/band
/// gaps separate, but ordinary inter-line spacing does not). Final document order
/// is decided later by each block's content `seq`.
fn cut_lines(lines: &[Line], idxs: &[usize], width: f64, height: f64, out: &mut Vec<Vec<usize>>) {
    if idxs.len() <= 1 {
        if !idxs.is_empty() {
            out.push(idxs.to_vec());
        }
        return;
    }
    let typ_h = typical_line_height_idx(lines, idxs);
    let min_y_gut = (typ_h * REGION_BAND_GAP_FRAC).max(1.0);

    let column_cut = find_column_cut(lines, idxs, typ_h);
    let best_x_width = column_cut.as_ref().map(|(width, _, _, _)| *width);
    let best_y = widest_y_gutter(lines, idxs, min_y_gut);

    // Cut on the axis whose widest empty gutter is larger. Ties prefer the
    // vertical (column) cut so side-by-side columns separate before bands.
    let prefer_x = match (best_x_width, best_y) {
        (Some(xg), Some((yg, _))) => xg >= yg,
        (Some(_), None) => true,
        _ => false,
    };

    if prefer_x {
        if let Some((_, left, right, spanning)) = column_cut {
            // Once an X-cut has established real columns, do not Y-cut inside a
            // column: a large paragraph gap must not break the column into leaf
            // regions that later interleave by content sequence. Paragraph
            // grouping still performs its fine 1.5x splits inside each atomic
            // column. Nested X-cuts remain supported for three-or-more columns.
            cut_column_subtree(lines, &left, width, height, out);
            if !spanning.is_empty() {
                cut_spanning(lines, &spanning, width, height, out);
            }
            cut_column_subtree(lines, &right, width, height, out);
            return;
        }
    }

    if best_y.is_some() {
        let groups = split_y_bands(lines, idxs, min_y_gut);
        // A horizontal cut that did not actually separate anything (one group)
        // means the region is irreducible — emit it whole to avoid recursion.
        if groups.len() <= 1 {
            out.push(idxs.to_vec());
        } else {
            for g in groups {
                cut_lines(lines, &g, width, height, out);
            }
        }
        return;
    }

    // No clean cut: this region is one column.
    out.push(idxs.to_vec());
}

/// Recurses through X-cuts only after an ancestor has established a column.
/// Keeping every final column in one region makes its paragraph blocks atomic
/// during document-order sorting, even when the column contains large Y gaps.
fn cut_column_subtree(
    lines: &[Line],
    idxs: &[usize],
    width: f64,
    height: f64,
    out: &mut Vec<Vec<usize>>,
) {
    if idxs.len() <= 1 {
        if !idxs.is_empty() {
            out.push(idxs.to_vec());
        }
        return;
    }
    let typ_h = typical_line_height_idx(lines, idxs);
    if let Some((_, left, right, spanning)) = find_column_cut(lines, idxs, typ_h) {
        cut_column_subtree(lines, &left, width, height, out);
        if !spanning.is_empty() {
            cut_spanning(lines, &spanning, width, height, out);
        }
        cut_column_subtree(lines, &right, width, height, out);
    } else {
        out.push(idxs.to_vec());
    }
}

/// 列切割结果：(切割处的间隙宽度, 左侧行索引, 右侧行索引, 跨列行索引)。
type ColumnCut = (f64, Vec<usize>, Vec<usize>, Vec<usize>);

/// Finds and validates one vertical column cut. The returned width is used to
/// choose between competing X/Y cuts; both sides must be substantial so line
/// numbers and other marginal strips do not become columns.
fn find_column_cut(lines: &[Line], idxs: &[usize], typ_h: f64) -> Option<ColumnCut> {
    let region_w = region_width(lines, idxs);
    // A real inter-column gutter is comfortably wider than a word space but on
    // letter-size layouts can be only ≈4% of the region width. Combine a
    // line-height floor with a small region-relative term.
    let min_x_gut = (typ_h * 1.2).max(region_w * 0.03);
    let (gutter_width, at) = column_gutter(lines, idxs, min_x_gut, region_w)?;

    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut spanning = Vec::new();
    for &i in idxs {
        let bbox = lines[i].bbox.normalize();
        if bbox.x1 <= at {
            left.push(i);
        } else if bbox.x0 >= at {
            right.push(i);
        } else {
            spanning.push(i);
        }
    }
    if is_substantial_column(lines, &left, region_w)
        && is_substantial_column(lines, &right, region_w)
    {
        Some((gutter_width, left, right, spanning))
    } else {
        None
    }
}

/// Handles a group of full-width "spanning" lines peeled out of a column cut:
/// they are stacked bands (header line, title, caption, …). A `Y`-cut separates
/// them into bands; each band becomes its own region so it is never merged into a
/// neighbouring column's paragraph block.
fn cut_spanning(
    lines: &[Line],
    idxs: &[usize],
    width: f64,
    height: f64,
    out: &mut Vec<Vec<usize>>,
) {
    if idxs.len() <= 1 {
        if !idxs.is_empty() {
            out.push(idxs.to_vec());
        }
        return;
    }
    let min_y_gut = (typical_line_height_idx(lines, idxs) * REGION_BAND_GAP_FRAC).max(1.0);
    let groups = split_y_bands(lines, idxs, min_y_gut);
    for g in groups {
        // Recurse so a spanning band that itself contains columns (rare) still
        // splits; with one group it just emits that band.
        if g.len() == idxs.len() {
            out.push(g);
        } else {
            cut_lines(lines, &g, width, height, out);
        }
    }
}

/// Finds the column gutter as the widest **coverage valley** on the x-axis: a
/// band that ≤ `tol` lines cross, at least `min_gap` wide, strictly interior to
/// the region. Returns `(valley_width, valley_midpoint)`, or `None`.
///
/// `tol` lets a centered title line, a footer string, or a stray wide line clip
/// across the gutter without hiding it — requiring an entirely empty gutter (as
/// a plain endpoint sweep does) misses real columns on pages with a centered
/// header. Coverage is sampled by sweeping the sorted interval endpoints; the
/// valley is the widest maximal run of x where the active count stays ≤ `tol`.
fn column_gutter(
    lines: &[Line],
    idxs: &[usize],
    min_gap: f64,
    region_w: f64,
) -> Option<(f64, f64)> {
    if idxs.len() < 4 {
        return None;
    }
    // Tolerance: a small fraction of the line count (at least 1), so a handful
    // of header/footer crossings don't fill the gutter.
    let tol = ((idxs.len() as f64) * 0.10).floor().max(1.0) as i64;

    // Region x-bounds (the valley must be interior, not the page margins).
    let mut rx0 = f64::INFINITY;
    let mut rx1 = f64::NEG_INFINITY;
    let mut events: Vec<(f64, i64)> = Vec::with_capacity(idxs.len() * 2);
    for &i in idxs {
        let b = lines[i].bbox.normalize();
        rx0 = rx0.min(b.x0);
        rx1 = rx1.max(b.x1);
        events.push((b.x0, 1)); // coverage starts
        events.push((b.x1, -1)); // coverage ends
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));

    let mut cover: i64 = 0;
    let mut best: Option<(f64, f64)> = None;
    let mut valley_start: Option<f64> = None;
    let mut prev_x = rx0;

    for (x, delta) in events {
        // The interval [prev_x, x) carries the current `cover` count.
        if cover <= tol {
            if valley_start.is_none() {
                valley_start = Some(prev_x);
            }
        } else if let Some(vs) = valley_start.take() {
            consider_valley(vs, prev_x, rx0, rx1, min_gap, &mut best);
        }
        // Apply the event(s) at x.
        cover += delta;
        if cover < 0 {
            cover = 0;
        }
        if cover > tol {
            if let Some(vs) = valley_start.take() {
                consider_valley(vs, x, rx0, rx1, min_gap, &mut best);
            }
        }
        prev_x = x;
    }
    if let Some(vs) = valley_start.take() {
        consider_valley(vs, rx1, rx0, rx1, min_gap, &mut best);
    }
    // Ignore a valley that spans almost the whole region (a near-empty region).
    best.filter(|&(w, _)| w < region_w * 0.95)
}

/// Records a candidate gutter valley `[lo, hi]` if it is interior to `[rx0, rx1]`
/// and at least `min_gap` wide, keeping the widest seen in `best`.
fn consider_valley(
    lo: f64,
    hi: f64,
    rx0: f64,
    rx1: f64,
    min_gap: f64,
    best: &mut Option<(f64, f64)>,
) {
    // Clip to strictly interior: a valley touching the region edge is just the
    // outer margin, not a between-columns gutter.
    let lo = lo.max(rx0);
    let hi = hi.min(rx1);
    if lo <= rx0 + f64::EPSILON || hi >= rx1 - f64::EPSILON {
        return;
    }
    let w = hi - lo;
    if w >= min_gap && best.is_none_or(|(bw, _)| w > bw) {
        *best = Some((w, (lo + hi) / 2.0));
    }
}

/// Whether a candidate column side is a *substantial* column rather than a
/// narrow marginal strip (line numbers, a rule, a single gutter glyph). It must
/// hold at least two lines and span at least ~18% of the region width.
fn is_substantial_column(lines: &[Line], idxs: &[usize], region_w: f64) -> bool {
    if idxs.len() < 2 {
        return false;
    }
    region_width(lines, idxs) >= region_w * 0.18
}

/// The x-extent (max x1 − min x0) covered by a subset of `lines`.
fn region_width(lines: &[Line], idxs: &[usize]) -> f64 {
    let mut x0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    for &i in idxs {
        let b = lines[i].bbox.normalize();
        x0 = x0.min(b.x0);
        x1 = x1.max(b.x1);
    }
    (x1 - x0).max(1.0)
}

/// The widest empty **horizontal** gutter (a y-band no line crosses) that clears
/// `min_gap`, returning its width and the y at which it opens, or `None`.
fn widest_y_gutter(lines: &[Line], idxs: &[usize], min_gap: f64) -> Option<(f64, f64)> {
    let interval = |i: usize| -> (f64, f64) {
        let b = lines[i].bbox.normalize();
        (b.y0, b.y1)
    };
    let mut sorted = idxs.to_vec();
    sorted.sort_by(|&a, &b| interval(a).0.total_cmp(&interval(b).0));

    let mut best: Option<(f64, f64)> = None;
    let mut cur_max = f64::NEG_INFINITY;
    for &i in &sorted {
        let (lo, hi) = interval(i);
        if cur_max.is_finite() {
            let gap = lo - cur_max;
            if gap >= min_gap && best.is_none_or(|(bw, _)| gap > bw) {
                best = Some((gap, cur_max));
            }
        }
        cur_max = cur_max.max(hi);
    }
    best
}

/// Splits line indices into **horizontal bands** wherever a y-gap of at least
/// `min_gap` separates consecutive lines. Groups come back top-to-bottom.
fn split_y_bands(lines: &[Line], idxs: &[usize], min_gap: f64) -> Vec<Vec<usize>> {
    let interval = |i: usize| -> (f64, f64) {
        let b = lines[i].bbox.normalize();
        (b.y0, b.y1)
    };
    let mut sorted = idxs.to_vec();
    sorted.sort_by(|&a, &b| interval(a).0.total_cmp(&interval(b).0));

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_max = f64::NEG_INFINITY;
    for &i in &sorted {
        let (lo, hi) = interval(i);
        if cur.is_empty() {
            cur.push(i);
            cur_max = hi;
            continue;
        }
        if lo - cur_max >= min_gap {
            groups.push(std::mem::take(&mut cur));
            cur.push(i);
            cur_max = hi;
        } else {
            cur.push(i);
            cur_max = cur_max.max(hi);
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

/// Median line height over a subset of `lines` (the band-gap threshold base).
fn typical_line_height_idx(lines: &[Line], idxs: &[usize]) -> f64 {
    let mut hs: Vec<f64> = idxs
        .iter()
        .map(|&i| lines[i].bbox.height())
        .filter(|h| *h > 0.0)
        .collect();
    if hs.is_empty() {
        return 1.0;
    }
    hs.sort_by(f64::total_cmp);
    hs[hs.len() / 2].max(1.0)
}

/// Wraps a run of lines into a text [`Block`] (number assigned later). The
/// block's content-order `seq` is the smallest line `seq` it contains.
fn make_text_block(lines: Vec<Line>) -> Block {
    let mut bbox = Rect::default();
    let mut seq = usize::MAX;
    for l in &lines {
        bbox = bbox.union(&l.bbox);
        seq = seq.min(l.seq);
    }
    Block {
        bbox,
        kind: BlockKind::Text,
        lines,
        image: None,
        number: 0,
        seq,
    }
}

/// Emits a visible block and, when present, a separate trailing whitespace-only
/// run that is horizontally disjoint from the last visible line. Financial-table
/// PDFs sometimes paint such a padding column all the way to the page bottom;
/// keeping it in the visible block inflates the bbox by hundreds of points.
/// Whitespace continuing at the same line start stays in the paragraph, matching
/// MuPDF. Splitting (rather than deleting) preserves rawdict content and the
/// line-containment invariant.
fn push_text_blocks(mut lines: Vec<Line>, out: &mut Vec<Block>) {
    if let Some(last_visible) = lines.iter().rposition(line_has_visible_text) {
        if last_visible + 1 < lines.len() {
            let visible_bbox = lines[last_visible].bbox.normalize();
            let trailing_bbox = lines[last_visible + 1..]
                .iter()
                .fold(Rect::default(), |bbox, line| bbox.union(&line.bbox))
                .normalize();
            let horizontal_overlap =
                visible_bbox.x1.min(trailing_bbox.x1) > visible_bbox.x0.max(trailing_bbox.x0);
            if !horizontal_overlap {
                let trailing_whitespace = lines.split_off(last_visible + 1);
                out.push(make_text_block(lines));
                out.push(make_text_block(trailing_whitespace));
                return;
            }
        }
    }
    out.push(make_text_block(lines));
}

// === reading order ========================================================

/// Assigns sequential numbers to blocks already emitted in document / region
/// reading order (PRD §8.6.2).
///
/// [`group_blocks_columned`] performs the content-sequence sort while retaining
/// side-by-side columns as atomic groups. Image blocks are appended afterwards,
/// matching the prior `seq == usize::MAX` behavior.
fn order_blocks(blocks: &mut [Block]) {
    for (i, b) in blocks.iter_mut().enumerate() {
        b.number = i;
    }
}

// === images ===============================================================

/// The device-space bbox of an image: its placement CTM maps the unit square
/// to user space, then the page transform lands it in device space.
fn image_bbox(img: &ImageRef, p: &Matrix) -> Rect {
    let unit = Rect::new(0.0, 0.0, 1.0, 1.0);
    let user = unit.transform(&img.ctm);
    user.transform(p).normalize()
}

// === font-name enrichment =================================================

/// Resolves a page's resource font names to their `/BaseFont` so span flags can
/// use the real font name (e.g. `Helvetica-Bold`) rather than the resource alias
/// (`F1`). Built once per page; the device-transform pass calls [`Self::resolve`]
/// once per *distinct* resource name (its caller memoizes), so no separate
/// O(glyphs) enrichment pass is needed.
struct FontResolver<'a> {
    doc: &'a DocumentStore,
    /// The page's `/Resources /Font` dict, if present.
    fonts: Option<pdf_core::Dict>,
}

impl<'a> FontResolver<'a> {
    /// Resolves `name`'s `/BaseFont` (tag-stripped). `None` when unresolvable —
    /// the caller keeps the resource name verbatim, matching the prior behavior.
    fn resolve(&self, name: &SmolStr) -> Option<SmolStr> {
        let fonts = self.fonts.as_ref()?;
        let fd = self
            .doc
            .resolve_dict_key(fonts, &Name::new(name.as_str()))
            .ok()
            .flatten()?;
        let fd = fd.as_dict()?;
        base_font_name(self.doc, fd)
    }
}

/// Builds the [`FontResolver`] for a page (resolves `/Resources /Font` once).
fn build_font_resolver<'a>(doc: &'a DocumentStore, page_dict: &pdf_core::Dict) -> FontResolver<'a> {
    let fonts = doc
        .resolve_dict_key(page_dict, &Name::new("Resources"))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
        .and_then(|res| {
            doc.resolve_dict_key(&res, &Name::new("Font"))
                .ok()
                .flatten()
        })
        .and_then(|o| o.as_dict().cloned());
    FontResolver { doc, fonts }
}

/// The `/BaseFont` of a font dict (following a Type0 descendant), tag-stripped
/// (`ABCDEF+Helvetica` → `Helvetica`).
fn base_font_name(doc: &DocumentStore, font: &pdf_core::Dict) -> Option<SmolStr> {
    let direct = font
        .get(&Name::new("BaseFont"))
        .and_then(Object::as_name)
        .and_then(Name::as_str);
    if let Some(n) = direct {
        return Some(strip_subset_tag(n));
    }
    // Type0: descendant carries the BaseFont too, but the parent usually has it.
    let df = doc
        .resolve_dict_key(font, &Name::new("DescendantFonts"))
        .ok()
        .flatten()?;
    let arr = df.as_array()?;
    let first = arr.first()?;
    let d = match first {
        Object::Reference(r) => doc.resolve(*r).ok()?,
        other => std::sync::Arc::new(other.clone()),
    };
    let d = d.as_dict()?;
    let n = d
        .get(&Name::new("BaseFont"))
        .and_then(Object::as_name)
        .and_then(Name::as_str)?;
    Some(strip_subset_tag(n))
}

/// Strips a `ABCDEF+` subset tag from a font name.
fn strip_subset_tag(name: &str) -> SmolStr {
    if let Some((tag, rest)) = name.split_once('+') {
        if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) {
            return SmolStr::new(rest);
        }
    }
    SmolStr::new(name)
}

// === tests ================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WritingDir;

    /// Builds one horizontal-writing glyph cell in PDF user space (origin
    /// bottom-left). `w` is the cell advance width; the ink box is the full cell.
    /// Word spaces are emitted as their own glyphs — exactly as real PDF content
    /// streams do — so the gutter detector sees a glyph-free band only at a true
    /// inter-column gutter (the property the detector relies on).
    fn g(c: &str, ox: f64, oy: f64, w: f64, size: f64) -> PositionedGlyph {
        PositionedGlyph {
            unicode: SmolStr::new(c),
            code: c.chars().next().map_or(0, |ch| ch as u32),
            origin: Point::new(ox, oy),
            bbox: Rect::new(ox, oy - 0.2 * size, ox + w, oy + 0.7 * size),
            font_name: SmolStr::new("Helvetica"),
            size,
            color: 0,
            render_mode: 0,
            writing_dir: WritingDir::Horizontal,
            advance_dir: (1.0, 0.0),
            ascender: 0.7,
            descender: -0.2,
        }
    }

    /// Lays a word (then a trailing space glyph) starting at user-x `x`, baseline
    /// `y`. Returns the x-cursor after the word + space. Each char is ~6pt wide.
    fn word(out: &mut Vec<PositionedGlyph>, text: &str, x: f64, y: f64, size: f64) -> f64 {
        let cw = 6.0;
        let mut cx = x;
        for ch in text.chars() {
            out.push(g(&ch.to_string(), cx, y, cw, size));
            cx += cw;
        }
        // Trailing inter-word space glyph (~3pt), as real PDFs emit.
        out.push(g(" ", cx, y, 3.0, size));
        cx + 3.0
    }

    /// Fills a column line: a keyword word then enough `filler` words (with word
    /// spaces) to cover `[x_start, x_end]`, so the column is a realistic-width
    /// run of glyphs (not a single short word). Returns nothing.
    fn col_line(
        out: &mut Vec<PositionedGlyph>,
        keyword: &str,
        x_start: f64,
        x_end: f64,
        y: f64,
        size: f64,
    ) {
        let mut x = word(out, keyword, x_start, y, size);
        while x < x_end - 20.0 {
            x = word(out, "filler", x, y, size);
        }
    }

    /// The page's emitted block text in reading order (one string per block).
    fn block_texts(tp: &TextPage) -> Vec<String> {
        tp.blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Text)
            .map(|b| {
                b.lines
                    .iter()
                    .flat_map(|l| l.spans.iter())
                    .map(|s| s.text.as_str())
                    .collect::<String>()
                    .replace(' ', "")
            })
            .collect()
    }

    /// The index of the first block whose joined text contains `needle`.
    fn find(texts: &[String], needle: &str) -> Option<usize> {
        texts.iter().position(|t| t.contains(needle))
    }

    // A US-Letter page.
    fn letter() -> Rect {
        Rect::new(0.0, 0.0, 612.0, 792.0)
    }

    /// COMPAT-LINE-GAP-001: independent runs split at exactly 0.8× the
    /// device-space size, not at a threshold derived from raw `Tf 1`.
    #[test]
    fn compat_line_gap_001_device_boundary() {
        let make_page = |gap: f64| {
            let scale = 10.0;
            let width = 6.0;
            let left = scaled_cell("A", 100.0, 700.0, width, scale);
            let right = scaled_cell("B", 100.0 + width + gap, 700.0, width, scale);
            textpage_from_glyphs(&[left, right], &[], letter(), 0)
        };

        let below = make_page(7.999);
        assert_eq!(
            below
                .blocks
                .iter()
                .map(|block| block.lines.len())
                .sum::<usize>(),
            1
        );

        let boundary = make_page(8.0);
        assert_eq!(
            boundary
                .blocks
                .iter()
                .map(|block| block.lines.len())
                .sum::<usize>(),
            2
        );
        assert_eq!(
            boundary
                .blocks
                .iter()
                .filter(|block| block.kind == BlockKind::Text)
                .count(),
            1,
            "same-baseline runs must remain in one block"
        );
    }

    /// COMPAT-LINE-SCALE-001: a huge raw `Tf` counter-scaled down by the CTM
    /// must not make separate physical baselines look like one line.
    #[test]
    fn compat_line_scale_001_large_tf_uses_device_geometry() {
        let mut glyphs = Vec::new();
        word(&mut glyphs, "First", 72.0, 700.0, 10.0);
        word(&mut glyphs, "Second", 72.0, 680.0, 10.0);
        for glyph in &mut glyphs {
            glyph.size = 327.68;
        }

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let line_count = tp
            .blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Text)
            .map(|block| block.lines.len())
            .sum::<usize>();
        assert_eq!(line_count, 2);
        assert_eq!(block_texts(&tp).len(), 2);
    }

    /// COMPAT-LINE-SCALE-002: an oblique glyph bbox is already an axis-aligned
    /// envelope. Re-projecting it onto the run normal must not inflate a 10pt
    /// cell to 15pt and merge two distinct 45-degree baselines six points apart.
    #[test]
    fn compat_line_scale_002_oblique_envelope_does_not_inflate_size() {
        let diagonal = std::f64::consts::FRAC_1_SQRT_2;
        let extent = 15.0 * diagonal;
        let mut first = g("A", 100.0, 700.0, 5.0, 10.0);
        first.advance_dir = (diagonal, diagonal);
        first.bbox = Rect::new(100.0, 700.0, 100.0 + extent, 700.0 + extent);

        let offset = 6.0 * diagonal;
        let mut second = g("B", 100.0 + offset, 700.0 - offset, 5.0, 10.0);
        second.advance_dir = (diagonal, diagonal);
        second.bbox = Rect::new(
            100.0 + offset,
            700.0 - offset,
            100.0 + offset + extent,
            700.0 - offset + extent,
        );

        let tp = textpage_from_glyphs(&[first, second], &[], letter(), 0);
        let line_count = tp
            .blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Text)
            .map(|block| block.lines.len())
            .sum::<usize>();
        assert_eq!(line_count, 2);
    }

    /// COMPAT-BLOCK-001: lines separated by roughly 1.6 line heights form
    /// distinct structured-text blocks, matching MuPDF's block granularity.
    /// The previous 1.3× *bbox-gap* threshold merged an entire densely-spaced
    /// report page into one page-sized block.
    #[test]
    fn compat_block_001_paragraph_gap_splits_blocks() {
        let mut glyphs = Vec::new();
        word(&mut glyphs, "First", 72.0, 700.0, 10.0);
        word(&mut glyphs, "Second", 72.0, 684.0, 10.0);

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let texts = block_texts(&tp);
        assert_eq!(texts.len(), 2, "expected line-level blocks; got {texts:?}");
        assert!(texts[0].contains("First"));
        assert!(texts[1].contains("Second"));
    }

    /// COMPAT-BLOCK-002: ordinary close leading remains one paragraph block.
    #[test]
    fn compat_block_002_close_lines_stay_together() {
        let mut glyphs = Vec::new();
        word(&mut glyphs, "First", 72.0, 700.0, 10.0);
        word(&mut glyphs, "Second", 72.0, 687.0, 10.0);

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let texts = block_texts(&tp);
        assert_eq!(
            texts.len(),
            1,
            "close paragraph split unexpectedly: {texts:?}"
        );
        assert!(texts[0].contains("First") && texts[0].contains("Second"));
    }

    /// COMPAT-BLOCK-003: horizontally separated fragments on one visual row
    /// (the common table-cell shape) stay in one block even though the line
    /// splitter represents them as two lines.
    #[test]
    fn compat_block_003_same_row_cells_stay_together() {
        let mut glyphs = Vec::new();
        word(&mut glyphs, "Label", 72.0, 700.0, 10.0);
        word(&mut glyphs, "Value", 420.0, 700.0, 10.0);

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let texts = block_texts(&tp);
        assert_eq!(
            texts.len(),
            1,
            "same-row cells split unexpectedly: {texts:?}"
        );
        assert!(texts[0].contains("Label") && texts[0].contains("Value"));
    }

    /// COMPAT-BLOCK-004: the 1.5× boundary is inclusive. This guards the exact
    /// compatibility edge instead of merely testing values well to either side.
    #[test]
    fn compat_block_004_baseline_boundary_is_strict() {
        let mut at_boundary = Vec::new();
        word(&mut at_boundary, "First", 72.0, 700.0, 10.0);
        word(&mut at_boundary, "Second", 72.0, 685.0, 10.0);
        assert_eq!(
            block_texts(&textpage_from_glyphs(&at_boundary, &[], letter(), 0)).len(),
            1
        );

        let mut above_boundary = Vec::new();
        word(&mut above_boundary, "First", 72.0, 700.0, 10.0);
        word(&mut above_boundary, "Second", 72.0, 684.99, 10.0);
        assert_eq!(
            block_texts(&textpage_from_glyphs(&above_boundary, &[], letter(), 0)).len(),
            2
        );
    }

    /// COMPAT-BLOCK-005: a right-indented line in the middle baseline band starts
    /// a paragraph, while an indented continuation after a list bullet does not.
    #[test]
    fn compat_block_005_indent_and_bullet_continuation() {
        let mut indented = Vec::new();
        word(&mut indented, "First", 72.0, 700.0, 10.0);
        word(&mut indented, "Second", 92.0, 688.0, 10.0);
        assert_eq!(
            block_texts(&textpage_from_glyphs(&indented, &[], letter(), 0)).len(),
            2
        );

        let mut bullet = Vec::new();
        word(&mut bullet, "• item", 72.0, 700.0, 10.0);
        word(&mut bullet, "continuation", 92.0, 688.0, 10.0);
        assert_eq!(
            block_texts(&textpage_from_glyphs(&bullet, &[], letter(), 0)).len(),
            1
        );
    }

    /// COMPAT-BLOCK-006: when nominal font size is unscaled, character geometry
    /// still supplies the device-space size used by the baseline threshold.
    #[test]
    fn compat_block_006_recovers_effective_size_from_geometry() {
        let mut glyphs = Vec::new();
        word(&mut glyphs, "First", 72.0, 700.0, 10.0);
        word(&mut glyphs, "Second", 72.0, 687.0, 10.0);
        for glyph in &mut glyphs {
            glyph.size = 1.0;
        }

        let texts = block_texts(&textpage_from_glyphs(&glyphs, &[], letter(), 0));
        assert_eq!(texts.len(), 1, "geometry scale was ignored: {texts:?}");
    }

    /// COMPAT-BLOCK-009: whitespace at the same line start remains in the text
    /// block, while a disjoint trailing padding column remains extractable in a
    /// separate block and cannot inflate the visible block's bbox.
    #[test]
    fn compat_block_009_trailing_whitespace_is_separate() {
        let mut aligned = Vec::new();
        word(&mut aligned, "Visible", 72.0, 700.0, 10.0);
        aligned.push(g(" ", 72.0, 687.0, 3.0, 10.0));
        aligned.push(g(" ", 72.0, 674.0, 3.0, 10.0));
        let aligned_page = textpage_from_glyphs(&aligned, &[], letter(), 0);
        assert_eq!(
            aligned_page
                .blocks
                .iter()
                .filter(|block| block.kind == BlockKind::Text)
                .count(),
            1
        );

        let mut disjoint = Vec::new();
        word(&mut disjoint, "Visible", 72.0, 700.0, 10.0);
        disjoint.push(g(" ", 120.0, 687.0, 3.0, 10.0));
        disjoint.push(g(" ", 120.0, 674.0, 3.0, 10.0));

        let tp = textpage_from_glyphs(&disjoint, &[], letter(), 0);
        let text_blocks: Vec<&Block> = tp
            .blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Text)
            .collect();
        assert_eq!(text_blocks.len(), 2);
        assert!(line_has_visible_text(&text_blocks[0].lines[0]));
        assert!(text_blocks[1]
            .lines
            .iter()
            .all(|line| !line_has_visible_text(line)));
        assert!(text_blocks[0].bbox.y1 < text_blocks[1].bbox.y0);
    }

    /// COMPAT-BLOCK-008: a dense financial table is grouped row-wise before
    /// paragraph splitting. A vertical XY-cut would isolate its three columns
    /// and turn a borderline 1.53× row step into one block per cell.
    #[test]
    fn compat_block_008_dense_table_preserves_rows() {
        let mut glyphs = Vec::new();
        for row in 0..5 {
            let baseline = 700.0 - row as f64 * 15.3;
            word(&mut glyphs, &format!("L{row}"), 72.0, baseline, 10.0);
            word(&mut glyphs, &format!("M{row}"), 300.0, baseline - 0.6, 10.0);
            word(&mut glyphs, &format!("R{row}"), 450.0, baseline - 0.6, 10.0);
        }

        let texts = block_texts(&textpage_from_glyphs(&glyphs, &[], letter(), 0));
        assert_eq!(texts.len(), 1, "dense table fragmented: {texts:?}");
        assert!(texts[0].contains("L0") && texts[0].contains("R4"));
    }

    /// COMPAT-BLOCK-010: three-cell rows seed table detection, while ordinary
    /// two-cell subtotal / continuation rows count toward table coverage. The
    /// two-cell rows alone must never be enough to classify two-column prose.
    #[test]
    fn compat_block_010_two_cell_rows_support_seeded_table() {
        let mut glyphs = Vec::new();
        for row in 0..8 {
            let baseline = 700.0 - row as f64 * 15.3;
            word(&mut glyphs, &format!("L{row}"), 72.0, baseline, 10.0);
            word(&mut glyphs, &format!("R{row}"), 450.0, baseline - 0.6, 10.0);
            if row < 3 {
                word(&mut glyphs, &format!("M{row}"), 300.0, baseline - 0.6, 10.0);
            }
        }

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let seeded_lines: Vec<Line> = tp
            .blocks
            .iter()
            .flat_map(|block| block.lines.iter().cloned())
            .collect();
        assert!(is_table_dominant(&seeded_lines));
        let texts = block_texts(&tp);
        assert_eq!(texts.len(), 1, "seeded table fragmented: {texts:?}");
        assert!(texts[0].contains("L0") && texts[0].contains("R7"));

        let mut two_column_prose = Vec::new();
        for row in 0..8 {
            let baseline = 700.0 - row as f64 * 15.3;
            word(&mut two_column_prose, "Left", 72.0, baseline, 10.0);
            word(&mut two_column_prose, "Right", 450.0, baseline, 10.0);
        }
        let prose = textpage_from_glyphs(&two_column_prose, &[], letter(), 0);
        let prose_lines: Vec<Line> = prose
            .blocks
            .iter()
            .flat_map(|block| block.lines.iter().cloned())
            .collect();
        assert!(!is_table_dominant(&prose_lines));
    }

    /// LAYOUT-COLUMN-REGRESSION-001: a two-column body must read column-major
    /// (all of the left column top→bottom, then the right column), NOT row-major
    /// (left-line-1, right-line-1, left-line-2, …). The left column lives in
    /// x∈[40,280], the right in x∈[320,560], with a ~40pt glyph-free gutter. To
    /// make the bug observable even with the bbox-XY-cut, the *first* baseline is
    /// laid as a single full-width cluster (left + right words share one
    /// baseline) — the merged-baseline case that bridges the gutter and used to
    /// defeat column detection.
    #[test]
    fn layout_column_regression_001_two_column_reads_column_major() {
        let size = 10.0;
        let mut glyphs = Vec::new();
        // Six baselines; user y decreases down the page (origin bottom-left).
        let ys = [740.0, 720.0, 700.0, 680.0, 660.0, 640.0];
        let left_words = ["Lone", "Ltwo", "Lthree", "Lfour", "Lfive", "Lsix"];
        let right_words = ["Rone", "Rtwo", "Rthree", "Rfour", "Rfive", "Rsix"];
        for (i, &y) in ys.iter().enumerate() {
            // Left column spans x∈[40,280], right column x∈[320,560], on the SAME
            // baseline — so each row is one merged full-width cluster bridging the
            // gutter (the case that used to defeat column detection).
            col_line(&mut glyphs, left_words[i], 40.0, 280.0, y, size);
            col_line(&mut glyphs, right_words[i], 320.0, 560.0, y, size);
        }

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let texts = block_texts(&tp);
        assert_eq!(
            texts.len(),
            left_words.len() + right_words.len(),
            "1.5× block granularity was lost inside columns: {texts:?}"
        );

        // Every left word must appear before every right word in the block order.
        let last_left = left_words
            .iter()
            .map(|w| find(&texts, w).unwrap_or(usize::MAX))
            .max()
            .unwrap();
        let first_right = right_words
            .iter()
            .map(|w| find(&texts, w).unwrap_or(0))
            .min()
            .unwrap();
        assert!(
            last_left < first_right,
            "expected column-major (all L before all R); got block order {texts:?}"
        );

        // And within each column, the words read top→bottom.
        let l_idx: Vec<usize> = left_words
            .iter()
            .map(|w| find(&texts, w).unwrap())
            .collect();
        assert!(
            l_idx.windows(2).all(|p| p[0] <= p[1]),
            "left column not top→bottom: {l_idx:?} in {texts:?}"
        );
    }

    /// LAYOUT-COLUMN-REGRESSION-002: a full-width header spanning both columns
    /// must read FIRST (in document order at its y), then the two columns
    /// column-major. The header is large continuous type with no gutter gap, so
    /// it must not be split at the body gutter nor merged into a column.
    #[test]
    fn layout_column_regression_002_header_over_two_columns() {
        let body = 10.0;
        let head = 18.0; // clearly larger than body → a heading
        let mut glyphs = Vec::new();
        // Full-width centered header near the top, continuous across the gutter.
        word(
            &mut glyphs,
            "HeaderTitleSpanningWholeWidthAcrossColumns",
            80.0,
            760.0,
            head,
        );
        // Two-column body below.
        let ys = [730.0, 712.0, 694.0, 676.0];
        let left_words = ["Aone", "Atwo", "Athree", "Afour"];
        let right_words = ["Bone", "Btwo", "Bthree", "Bfour"];
        for (i, &y) in ys.iter().enumerate() {
            col_line(&mut glyphs, left_words[i], 40.0, 280.0, y, body);
            col_line(&mut glyphs, right_words[i], 320.0, 560.0, y, body);
        }

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let texts = block_texts(&tp);

        let header = find(&texts, "HeaderTitle").expect("header block present");
        let last_a = left_words
            .iter()
            .map(|w| find(&texts, w).unwrap())
            .max()
            .unwrap();
        let first_b = right_words
            .iter()
            .map(|w| find(&texts, w).unwrap())
            .min()
            .unwrap();
        // Header first, then left column, then right column.
        assert!(
            header < last_a && last_a < first_b,
            "expected header → left col → right col; got {texts:?}"
        );
        // The header block must contain text from BOTH halves (not be split at the
        // gutter, and not be merged into one column).
        assert!(
            texts[header].contains("Header") && texts[header].contains("Columns"),
            "header was shredded at the gutter: {:?}",
            texts[header]
        );
    }

    /// LAYOUT-COLUMN-REGRESSION-003: an ordinary single-column page must NOT be
    /// split into columns by a chance vertical alignment of word breaks (no
    /// glyph-free interior band spans every line), and reads top→bottom.
    #[test]
    fn layout_column_regression_003_single_column_not_split() {
        let size = 10.0;
        let mut glyphs = Vec::new();
        let ys = [740.0, 722.0, 704.0, 686.0, 668.0, 650.0];
        let words = ["Wone", "Wtwo", "Wthree", "Wfour", "Wfive", "Wsix"];
        for (i, &y) in ys.iter().enumerate() {
            // A full-width single-column line: several words across the page with
            // ordinary word spaces (no glyph-free gutter band).
            let mut x = 40.0;
            x = word(&mut glyphs, words[i], x, y, size);
            x = word(&mut glyphs, "filler", x, y, size);
            x = word(&mut glyphs, "filler", x, y, size);
            let _ = word(&mut glyphs, "tail", x, y, size);
        }

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let texts = block_texts(&tp);
        let idx: Vec<usize> = words.iter().map(|w| find(&texts, w).unwrap()).collect();
        // Lines stay in top→bottom document order (no column reshuffle).
        assert!(
            idx.windows(2).all(|p| p[0] <= p[1]),
            "single column reading order disturbed: {idx:?} in {texts:?}"
        );
    }

    /// LAYOUT-COLUMN-REGRESSION-004: two adjacent lines of ONE column whose
    /// baselines bracket a single line of the *other* column (tight CSS
    /// line-height) must each emit as a whole-word line — NOT character-interleaved
    /// (`is`+`of` → `iosf`).
    ///
    /// The content-order baseline sweep keys a cluster on its seed glyph's cross
    /// and admits anything within `LINE_TOL_FRAC × size`. Here the left column's
    /// line baseline sits exactly between the right column's two line baselines, so
    /// the left line bridges them: all three collapse into one cluster. The gutter
    /// split peels the left line off, but the right piece still holds two distinct
    /// baselines whose glyphs arrive row-major (rightLineA-char, rightLineB-char,
    /// …); a plain advance sort would weave them character-by-character. The
    /// baseline-refinement split must separate the right piece into its two real
    /// lines so each reads as whole words.
    #[test]
    fn layout_column_regression_004_bracketing_baselines_no_char_interleave() {
        let size = 12.0;
        let mut glyphs = Vec::new();
        // A realistic two-column page (left x∈[40,280], right x∈[320,560], ~40pt
        // glyph-free gutter) so the page-gutter detector fires (it needs ≥4 lines).
        // Each row's left + right lines share a baseline (the tight-baseline case),
        // EXCEPT the focus row, where the left line's baseline sits exactly between
        // the right column's two lines and bridges them into one content cluster.
        let normal_ys = [560.0, 544.0, 528.0, 512.0];
        for (k, &y) in normal_ys.iter().enumerate() {
            col_line(&mut glyphs, &format!("L{k}word"), 40.0, 280.0, y, size);
            col_line(&mut glyphs, &format!("R{k}word"), 320.0, 560.0, y, size);
        }
        // Focus row: the left line at y=494 brackets the right column's upper
        // (y=500) and lower (y=488) lines — a full line-height (12pt) apart, so once
        // the gutter peels the bridging left line off, the right piece still holds
        // two distinct baselines that arrived row-major and must be un-woven.
        col_line(&mut glyphs, "Leftbridge", 40.0, 280.0, 494.0, size);
        // Right column upper/lower lines, pushed word-alternately so the source
        // order genuinely interleaves the two baselines.
        let upper = ["is", "make", "Twenty", "develope"];
        let lower = ["of", "capric", "character", "years"];
        let mut xu = 320.0;
        let mut xl = 320.0;
        for k in 0..upper.len() {
            xu = word(&mut glyphs, upper[k], xu, 500.0, size);
            xl = word(&mut glyphs, lower[k], xl, 488.0, size);
        }

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        // Join every line's plain text; the bug shows up as fused tokens like
        // "iosf" / "makecapric". A correct split yields each word intact.
        let line_texts: Vec<String> = tp
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Text)
            .flat_map(|b| b.lines.iter())
            .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect();
        let all = line_texts.join("\u{1}");
        // No character interleaving: the upper line's first two words and the lower
        // line's first two words must each survive as whole tokens.
        for w in upper.iter().chain(lower.iter()) {
            assert!(
                all.contains(w),
                "word {w:?} was character-interleaved away; got lines {line_texts:?}"
            );
        }
        // And the canonical fused token must NOT appear on any single line.
        for fused in ["iosf", "makecapric", "ofmake"] {
            assert!(
                !line_texts
                    .iter()
                    .any(|t| t.replace(' ', "").contains(fused)),
                "found char-interleaved token {fused:?} in lines {line_texts:?}"
            );
        }
    }

    /// One glyph cell at user-x `ox`, baseline `oy`, advance/ink width `w`. The
    /// ink box is the full cell, so the inter-glyph gap is `next.ox - (ox + w)`.
    fn cell(c: &str, ox: f64, oy: f64, w: f64, size: f64) -> PositionedGlyph {
        g(c, ox, oy, w, size)
    }

    /// The plain `get_text("text")` of a single-line page (no trailing `\n`),
    /// for asserting synthesized word spaces.
    fn line_text_of(tp: &TextPage) -> String {
        tp.blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Text)
            .flat_map(|b| b.lines.iter())
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect()
    }

    /// A minimal device-space glyph carrying only `text` — enough to exercise
    /// [`reorder_rtl_line`], which keys solely on each glyph's character class.
    /// Indices in the run map 1:1 to `dev`, so positions are implicit.
    fn dg(text: &str) -> DevGlyph {
        DevGlyph {
            origin: Point::new(0.0, 0.0),
            bbox: Rect::new(0.0, 0.0, 1.0, 1.0),
            text: SmolStr::new(text),
            font: SmolStr::new("f"),
            size: 10.0,
            color: 0,
            flags: 0,
            wmode: 0,
            dir: (1.0, 0.0),
            ascender: 0.7,
            descender: -0.2,
        }
    }

    /// Reorders a visual-order char list (as it would be laid out left→right on
    /// the page) into logical order via [`reorder_rtl_line`], returning the
    /// joined string. Each `&str` is one glyph cell.
    fn reorder_visual(visual: &[&str]) -> String {
        let dev: Vec<DevGlyph> = visual.iter().map(|c| dg(c)).collect();
        let mut run: Vec<usize> = (0..dev.len()).collect();
        reorder_rtl_line(&mut run, &dev);
        run.iter().map(|&i| dev[i].text.as_str()).collect()
    }

    /// LAYOUT-BIDI-001: a digit group embedded in an RTL line must keep its
    /// left-to-right order. Visually the Arabic "السعر" sits at the right, then
    /// "100", then "دولار" to its left; in source/logical order it reads
    /// "السعر 100 دولار". A blanket reverse produced "001"; the UAX#9 re-reverse
    /// of the LTR sub-run restores "100".
    #[test]
    fn layout_bidi_001_embedded_digits_stay_ltr() {
        // Visual order (page left→right) = whole line reversed but the digit
        // group "100" kept LTR: دولار-reversed · space · 1 0 0 · space ·
        // السعر-reversed.
        let visual = [
            "ر", "ا", "ل", "و", "د", " ", "1", "0", "0", " ", "ر", "ع", "س", "ل", "ا",
        ];
        let got = reorder_visual(&visual);
        assert_eq!(got, "السعر 100 دولار", "embedded digit group mis-ordered");
    }

    /// LAYOUT-BIDI-002: a Latin word embedded in an RTL line must keep its
    /// left-to-right order ("Linux", not "xuniL").
    #[test]
    fn layout_bidi_002_embedded_latin_word_stays_ltr() {
        // Visual = whole line reversed, "Linux" kept LTR: نظام-reversed · space ·
        // L i n u x · space · أستخدم-reversed.
        let visual = [
            "م", "ا", "ظ", "ن", " ", "L", "i", "n", "u", "x", " ", "م", "د", "خ", "ت", "س", "أ",
        ];
        let got = reorder_visual(&visual);
        // Logical: أستخدم Linux نظام
        assert!(
            got.contains("Linux") && !got.contains("xuniL"),
            "embedded Latin word mis-ordered: {got:?}"
        );
        assert_eq!(got, "أستخدم Linux نظام");
    }

    /// LAYOUT-BIDI-003: a pure-RTL line (no LTR sub-runs) is simply reversed into
    /// logical order, and the single-LTR-glyph case ("3") is a no-op either way.
    #[test]
    fn layout_bidi_003_pure_rtl_and_single_digit() {
        // Pure Arabic word laid out visually (reversed): logical "سلام".
        assert_eq!(reorder_visual(&["م", "ا", "ل", "س"]), "سلام");
        // "رقم 3": visual 3 · space · م ق ر  →  logical "رقم 3".
        assert_eq!(reorder_visual(&["3", " ", "م", "ق", "ر"]), "رقم 3");
    }

    /// LAYOUT-WORDGAP-001: two words placed with a word-sized along-axis gap and
    /// NO space glyph between them must serialize WITH a synthesized space — the
    /// `TJ`-kerned case that previously ran the words together.
    #[test]
    fn layout_wordgap_001_synthesizes_space_on_kern_gap() {
        let size = 10.0;
        let cw = 6.0;
        let mut glyphs = Vec::new();
        // "Hi" then a 4pt gap (> 0.2*10 = 2.0 threshold) then "there", no space.
        let mut x = 100.0;
        for ch in "Hi".chars() {
            glyphs.push(cell(&ch.to_string(), x, 700.0, cw, size));
            x += cw;
        }
        x += 4.0; // word-sized gap, no space glyph emitted
        for ch in "there".chars() {
            glyphs.push(cell(&ch.to_string(), x, 700.0, cw, size));
            x += cw;
        }
        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let txt = line_text_of(&tp);
        assert_eq!(txt, "Hi there", "expected a synthesized space; got {txt:?}");
    }

    /// LAYOUT-WORDGAP-002: a small intra-word kern (below the threshold) must NOT
    /// get a space — tight kerning inside a word stays one token.
    #[test]
    fn layout_wordgap_002_small_kern_no_space() {
        let size = 10.0;
        let cw = 6.0;
        let mut glyphs = Vec::new();
        // "AVA" with a tiny 1pt extra gap (< 0.2*10 = 2.0) before each later char.
        let mut x = 100.0;
        for ch in "AVA".chars() {
            glyphs.push(cell(&ch.to_string(), x, 700.0, cw, size));
            x += cw + 1.0; // sub-threshold kern
        }
        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let txt = line_text_of(&tp);
        assert_eq!(txt, "AVA", "sub-threshold kern must not split; got {txt:?}");
    }

    /// LAYOUT-WORDGAP-003: a real space glyph between two words must NOT be
    /// doubled — a born-digital PDF that emits literal spaces stays single-spaced.
    #[test]
    fn layout_wordgap_003_real_space_not_doubled() {
        let size = 10.0;
        let cw = 6.0;
        let mut glyphs = Vec::new();
        let mut x = 100.0;
        for ch in "Hi".chars() {
            glyphs.push(cell(&ch.to_string(), x, 700.0, cw, size));
            x += cw;
        }
        // A literal space glyph (~3pt), then the next word abutting it. The gap on
        // each side of the space is ~0 so no synthesis fires; even if a side gap
        // were wide, the whitespace neighbor suppresses doubling.
        glyphs.push(cell(" ", x, 700.0, 3.0, size));
        x += 3.0;
        for ch in "there".chars() {
            glyphs.push(cell(&ch.to_string(), x, 700.0, cw, size));
            x += cw;
        }
        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let txt = line_text_of(&tp);
        assert_eq!(
            txt, "Hi there",
            "real space must not be doubled; got {txt:?}"
        );
    }

    /// LAYOUT-WORDGAP-004: a word gap that coincides with a style change (a new
    /// span) must still place the space between the two words in `line_text`
    /// order — the span-boundary case.
    #[test]
    fn layout_wordgap_004_space_across_style_change() {
        let size = 10.0;
        let cw = 6.0;
        let mut glyphs = Vec::new();
        let mut x = 100.0;
        // "Hi" in Helvetica.
        for ch in "Hi".chars() {
            glyphs.push(cell(&ch.to_string(), x, 700.0, cw, size));
            x += cw;
        }
        x += 4.0; // word-sized gap
                  // "there" in a different font (forces a new span at the boundary).
        for ch in "there".chars() {
            let mut gl = cell(&ch.to_string(), x, 700.0, cw, size);
            gl.font_name = SmolStr::new("Times-Bold");
            glyphs.push(gl);
            x += cw;
        }
        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        // Two spans, and the joined line text carries the space between words.
        let txt = line_text_of(&tp);
        assert_eq!(
            txt, "Hi there",
            "space missing across style change; got {txt:?}"
        );
    }

    /// One glyph cell whose `Tf` operand is 1.0 but whose geometry is `scale`×
    /// larger — the PMC/LaTeX case where the text scale lives in the text/CTM
    /// matrix, not the font operand. `ox`/`oy`/`w` are pre-scaled device-ish
    /// user-space coordinates; `size` stays 1.0, mimicking `Tf 1`.
    fn scaled_cell(c: &str, ox: f64, oy: f64, w: f64, scale: f64) -> PositionedGlyph {
        PositionedGlyph {
            unicode: SmolStr::new(c),
            code: c.chars().next().map_or(0, |ch| ch as u32),
            origin: Point::new(ox, oy),
            // Cell height ≈ 0.9*scale (matches `g()`'s 0.7+0.2 cell), so the
            // device-effective size recovers ~`scale` while `size` stays 1.0.
            bbox: Rect::new(ox, oy - 0.2 * scale, ox + w, oy + 0.7 * scale),
            font_name: SmolStr::new("Helvetica"),
            size: 1.0,
            color: 0,
            render_mode: 0,
            writing_dir: WritingDir::Horizontal,
            advance_dir: (1.0, 0.0),
            ascender: 0.7,
            descender: -0.2,
        }
    }

    /// LAYOUT-WORDGAP-005: the scale lives in the CTM, not `Tf`. Every glyph
    /// reports `size = 1.0` (the raw `Tf` operand) while its geometry is rendered
    /// at ~8pt — exactly how PMC/LaTeX PDFs lay out body text. The word-gap
    /// threshold must derive from the *device* glyph size (≈8), not the raw
    /// operand (1.0): with the old `size * 0.2 = 0.2` threshold the tiny ~0.8pt
    /// intra-word kerns here tripped a false split. A whole word with normal
    /// kerns must stay one token; a real ≥1.6pt word gap must still split.
    #[test]
    fn layout_wordgap_005_threshold_uses_device_size_not_tf_operand() {
        let scale = 8.0;
        let cw = scale * 0.6; // ~4.8pt advance per glyph at 8pt
        let intra_kern = scale * 0.1; // ~0.8pt: normal intra-word kern
        let word_gap = scale * 0.5; // ~4.0pt: a real inter-word gap

        let mut glyphs = Vec::new();
        let y = 700.0;
        let mut x = 100.0;
        // "important" with a small intra-word kern between every glyph. Under the
        // old text-space threshold (0.2) each 0.8pt kern would split the word; the
        // device-space threshold (~0.2*8 = 1.6) leaves it intact.
        for ch in "important".chars() {
            glyphs.push(scaled_cell(&ch.to_string(), x, y, cw, scale));
            x += cw + intra_kern;
        }
        // A genuine inter-word gap (no space glyph), then "word".
        x += word_gap;
        for ch in "word".chars() {
            glyphs.push(scaled_cell(&ch.to_string(), x, y, cw, scale));
            x += cw + intra_kern;
        }

        let tp = textpage_from_glyphs(&glyphs, &[], letter(), 0);
        let txt = line_text_of(&tp);
        // No mid-word space inside "important"; exactly one synthesized space at
        // the real word boundary.
        assert_eq!(
            txt, "important word",
            "CTM-scaled threshold mis-fired (should be device-size relative); got {txt:?}"
        );

        // The word segmenter must agree with the layout-stage synthesis.
        let words: Vec<String> = crate::words::words(&tp)
            .into_iter()
            .map(|w| w.text)
            .collect();
        assert_eq!(
            words,
            vec!["important".to_string(), "word".to_string()],
            "get_text(\"words\") disagreed with layout split; got {words:?}"
        );
    }
}
