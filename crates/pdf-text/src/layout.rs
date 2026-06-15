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

/// Minimum vertical gap (as a fraction of the typical line height) that starts a
/// new block. Lines closer than this fall into one paragraph block.
const BLOCK_GAP_FRAC: f64 = 1.3;

/// Minimum horizontal overlap fraction for two lines to be considered part of
/// the same column during block grouping.
const BLOCK_OVERLAP_FRAC: f64 = 0.1;

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
    let mediabox = page.mediabox();
    let rotate = page.rotation();
    let clip = page.cropbox();
    let glyphs = enrich_glyph_fonts(doc, &page_dict, res.glyphs);
    textpage_from_glyphs_clipped(&glyphs, &res.images, mediabox, rotate, Some(clip))
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
    let p = page_transform(mediabox, rotate);
    let (width, height) = page_size(mediabox, rotate);

    // 1. Transform every glyph to device space, dropping out-of-CropBox glyphs.
    let clip = clip.map(|c| c.normalize());
    let dev: Vec<DevGlyph> = glyphs
        .iter()
        .filter(|g| clip.is_none_or(|c| origin_in_clip(g.origin, &c)))
        .map(|g| DevGlyph::new(g, &p))
        .collect();

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
/// y-up, MediaBox-relative) into PyMuPDF device space (top-left, y-down, with
/// `/Rotate` applied). `[a b c d e f]` row-vector form.
#[must_use]
pub fn page_transform(mediabox: Rect, rotate: i32) -> Matrix {
    let mb = mediabox.normalize();
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
pub fn page_size(mediabox: Rect, rotate: i32) -> (f64, f64) {
    let mb = mediabox.normalize();
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
    fn new(g: &PositionedGlyph, p: &Matrix) -> Self {
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
        let dir = writing_dir_vector(g, p, wmode);
        let flags = style_flags(g);
        DevGlyph {
            origin,
            bbox,
            text,
            font: g.font_name.clone(),
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
}

/// The device-space writing-direction unit vector. We transform the user-space
/// advance direction (x+ for horizontal, y- for vertical writing) through the
/// page transform's linear part and normalize. Falls back to `(1, 0)`.
fn writing_dir_vector(g: &PositionedGlyph, p: &Matrix, wmode: u8) -> (f64, f64) {
    // A unit advance step in user space, derived from the glyph bbox if we have
    // width, else the canonical axis. For horizontal writing the advance is +x;
    // for vertical writing the advance is -y (top-to-bottom).
    let (ux, uy) = if wmode == 1 { (0.0, -1.0) } else { (1.0, 0.0) };
    // Apply only the linear part of the page transform (drop translation).
    let dx = p.a * ux + p.c * uy;
    let dy = p.b * ux + p.d * uy;
    let n = (dx * dx + dy * dy).sqrt();
    if n <= f64::EPSILON {
        return (1.0, 0.0);
    }
    let _ = g;
    (dx / n, dy / n)
}

/// Derives the font-property span flags (italic/serif/mono/bold) from the font
/// name (PRD §8.6.2, §8.5). The superscript bit is layout-derived and added in
/// `build_line` once the line baseline is known.
fn style_flags(g: &PositionedGlyph) -> u32 {
    name_flags(&g.font_name)
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
    // Tolerance keys on the *larger* of the cluster representative's size and
    // the candidate glyph's size so a smaller super/subscript glyph still joins
    // the main baseline.
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut cluster_cross: Vec<f64> = Vec::new();

    for (i, g) in dev.iter().enumerate() {
        let cross = g.cross();
        let mut found = None;
        for (ci, cc) in cluster_cross.iter().enumerate() {
            let rep = &dev[clusters[ci][0]];
            let tol = rep.size.abs().max(g.size.abs()).max(1.0) * LINE_TOL_FRAC;
            if (cc - cross).abs() <= tol && dir_matches(&rep.dir, &g.dir) {
                found = Some(ci);
                break;
            }
        }
        match found {
            Some(ci) => clusters[ci].push(i),
            None => {
                clusters.push(vec![i]);
                cluster_cross.push(cross);
            }
        }
    }

    // Sort clusters by their cross value (device y-down → top first). For
    // vertical writing the cross axis is x, so this still yields a stable
    // left-to-right column ordering of vertical lines.
    let mut order: Vec<usize> = (0..clusters.len()).collect();
    order.sort_by(|&a, &b| cluster_cross[a].total_cmp(&cluster_cross[b]));

    let mut lines = Vec::new();
    for ci in order {
        let mut idxs = clusters[ci].clone();
        // Order along the reading axis (advance direction).
        idxs.sort_by(|&a, &b| dev[a].along().total_cmp(&dev[b].along()));
        // A single baseline cluster may straddle a column gutter; split it into
        // separate lines wherever a large along-axis gap appears so a line never
        // crosses a column boundary.
        for run in split_on_gutter(&idxs, dev) {
            let mut run = run;
            if is_rtl_run(&run, dev) {
                run.reverse();
            }
            // Content-order key: the smallest source-glyph index in this run,
            // i.e. the earliest-painted glyph of the line (document order).
            let seq = run.iter().copied().min().unwrap_or(0);
            let line_glyphs: Vec<&DevGlyph> = run.iter().map(|&i| &dev[i]).collect();
            lines.push(build_line(&line_glyphs, seq));
        }
    }
    lines
}

/// Splits an advance-ordered baseline run into sub-runs wherever the along-axis
/// gap between consecutive glyphs exceeds a generous multiple of the font size
/// (a column gutter). Normal inter-word spaces never trigger this.
fn split_on_gutter(idxs: &[usize], dev: &[DevGlyph]) -> Vec<Vec<usize>> {
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut prev_end: Option<f64> = None;
    for &i in idxs {
        let g = &dev[i];
        // Project the glyph's leading/trailing edges onto the reading axis.
        let start = g.along();
        let extent = (g.bbox.width().hypot(g.bbox.height())).max(g.size.abs());
        let gutter = g.size.abs().max(1.0) * 4.0; // ≫ a normal space
        if let Some(pe) = prev_end {
            if start - pe > gutter {
                runs.push(std::mem::take(&mut cur));
            }
        }
        cur.push(i);
        prev_end = Some(start + extent);
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
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
fn build_line(glyphs: &[&DevGlyph], seq: usize) -> Line {
    let wmode = glyphs.first().map_or(0, |g| g.wmode);
    let dir = glyphs.first().map_or((1.0, 0.0), |g| g.dir);

    // Determine the line baseline (median cross) to flag superscripts per glyph.
    let mut crosses: Vec<f64> = glyphs.iter().map(|g| g.cross()).collect();
    crosses.sort_by(f64::total_cmp);
    let baseline = crosses.get(crosses.len() / 2).copied().unwrap_or(0.0);

    let mut spans: Vec<Span> = Vec::new();
    let mut line_bbox = Rect::default();

    for g in glyphs {
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
        target.bbox = target.bbox.union(&g.bbox);
        for c in g.text.chars() {
            target.text.push(c);
            target.chars.push(Char {
                origin: g.origin,
                bbox: g.bbox,
                c,
            });
        }
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
    // Typical line height (computed over all lines) drives the paragraph-gap
    // threshold uniformly across regions.
    let typical_h = typical_line_height(&lines);

    let idxs: Vec<usize> = (0..lines.len()).collect();
    let mut regions: Vec<Vec<usize>> = Vec::new();
    cut_lines(&lines, &idxs, width, height, &mut regions);

    let mut blocks: Vec<Block> = Vec::new();
    // `lines` is consumed region-by-region: move each line out exactly once.
    let mut slots: Vec<Option<Line>> = lines.into_iter().map(Some).collect();
    for region in regions {
        // Take the region's lines (top-to-bottom) and split into paragraphs.
        let mut region_lines: Vec<Line> = region
            .iter()
            .map(|&i| slots[i].take().expect("each line placed once"))
            .collect();
        region_lines.sort_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
        group_region_paragraphs(region_lines, typical_h, &mut blocks);
    }
    blocks
}

/// Groups one column region's (y-sorted) lines into paragraph blocks by vertical
/// proximity + horizontal overlap, appending to `out`.
fn group_region_paragraphs(lines: Vec<Line>, typical_h: f64, out: &mut Vec<Block>) {
    let mut cur: Vec<Line> = Vec::new();
    let mut prev_bottom: Option<f64> = None;
    for line in lines {
        let top = line.bbox.y0;
        let start_new = match prev_bottom {
            None => false,
            Some(pb) => {
                let gap = top - pb;
                gap > typical_h * BLOCK_GAP_FRAC || !overlaps_block(&cur, &line)
            }
        };
        if start_new && !cur.is_empty() {
            out.push(make_text_block(std::mem::take(&mut cur)));
        }
        prev_bottom = Some(line.bbox.y1);
        cur.push(line);
    }
    if !cur.is_empty() {
        out.push(make_text_block(cur));
    }
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
    let min_y_gut = (typ_h * BLOCK_GAP_FRAC).max(1.0);

    // Column gutters are probed over **narrow** lines only, so a full-width
    // header/title that bridges the gutter does not defeat column detection.
    let region_w = region_width(lines, idxs);
    // A real inter-column gutter is comfortably wider than a word space but on
    // letter-size multi-column layouts is only ≈4% of the region width (≈22pt
    // observed) — well under the 5% page-width rule used previously. Use a
    // line-height floor (a gutter exceeds ~1.2 line heights) and a small
    // region-relative term; an empty vertical band this wide that no narrow line
    // crosses across the whole region does not occur in ordinary single-column
    // justified text, so this does not over-split.
    let min_x_gut = (typ_h * 1.2).max(region_w * 0.03);
    // Column gutter via a coverage-profile valley that tolerates a few
    // crossings: a centered title line or a footer string can clip across the
    // gutter without filling it, so requiring *zero* crossings (a plain empty
    // gutter) misses the column break. A valley whose crossing count stays at or
    // below `tol` over a band ≥ `min_x_gut` wide is treated as the gutter.
    let best_x = column_gutter(lines, idxs, min_x_gut, region_w);
    let best_y = widest_y_gutter(lines, idxs, min_y_gut);

    // Validate a candidate column cut: partition into left / right / straddling
    // lines at the gutter midpoint, and accept only when **both** sides are
    // substantial columns. A narrow marginal strip — e.g. a column of line
    // numbers beside legal text — is not a real second column and must not be
    // split off (fitz keeps the number with its line in content order).
    let column_cut = best_x.and_then(|(_, at)| {
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut spanning = Vec::new();
        for &i in idxs {
            let b = lines[i].bbox.normalize();
            if b.x1 <= at {
                left.push(i);
            } else if b.x0 >= at {
                right.push(i);
            } else {
                spanning.push(i);
            }
        }
        if is_substantial_column(lines, &left, region_w)
            && is_substantial_column(lines, &right, region_w)
        {
            Some((left, right, spanning))
        } else {
            None
        }
    });

    // Cut on the axis whose widest empty gutter is larger. Ties prefer the
    // vertical (column) cut so side-by-side columns separate before bands.
    let prefer_x = match (best_x, best_y) {
        _ if column_cut.is_none() => false,
        (Some((xg, _)), Some((yg, _))) => xg >= yg,
        (Some(_), None) => true,
        _ => false,
    };

    if prefer_x {
        if let Some((left, right, spanning)) = column_cut {
            // Recurse each side; spanning lines form their own (band-cut) region.
            cut_lines(lines, &left, width, height, out);
            if !spanning.is_empty() {
                cut_spanning(lines, &spanning, width, height, out);
            }
            cut_lines(lines, &right, width, height, out);
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
    let min_y_gut = (typical_line_height_idx(lines, idxs) * BLOCK_GAP_FRAC).max(1.0);
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

/// The median line height (a robust "typical" measure for gap thresholds).
fn typical_line_height(lines: &[Line]) -> f64 {
    let mut hs: Vec<f64> = lines
        .iter()
        .map(|l| l.bbox.height())
        .filter(|h| *h > 0.0)
        .collect();
    if hs.is_empty() {
        return 1.0;
    }
    hs.sort_by(f64::total_cmp);
    hs[hs.len() / 2].max(1.0)
}

/// Whether `line` horizontally overlaps the current block enough to belong to
/// the same column (uses the block's running x-extent).
fn overlaps_block(cur: &[Line], line: &Line) -> bool {
    if cur.is_empty() {
        return true;
    }
    let mut bx0 = f64::INFINITY;
    let mut bx1 = f64::NEG_INFINITY;
    for l in cur {
        bx0 = bx0.min(l.bbox.x0);
        bx1 = bx1.max(l.bbox.x1);
    }
    let lo = bx0.max(line.bbox.x0);
    let hi = bx1.min(line.bbox.x1);
    let overlap = (hi - lo).max(0.0);
    let min_w = (bx1 - bx0).min(line.bbox.width()).max(1.0);
    overlap >= min_w * BLOCK_OVERLAP_FRAC
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

// === reading order ========================================================

/// Orders blocks in **document / content order** and assigns sequential numbers
/// (PRD §8.6.2).
///
/// MuPDF/PyMuPDF emit structured-text blocks in the order its content device
/// encountered them (content order), *not* a geometric top-to-bottom sort — a
/// pure geometric reordering of a page's blocks diverges sharply from fitz's
/// default `get_text` sequence. Since our interpreter already walks the content
/// stream in paint order, ordering blocks by their content-order `seq` (smallest
/// source-glyph index) reproduces fitz's block sequence closely. Column grouping
/// (in [`group_blocks_columned`]) guarantees a block's lines come from one
/// column, so `seq` ordering keeps each column contiguous instead of
/// interleaving columns line-by-line. The sort is **stable**, so image blocks
/// (`seq == usize::MAX`) sort to the end while equal-`seq` blocks keep their
/// relative position.
fn order_blocks(blocks: &mut [Block]) {
    blocks.sort_by_key(|b| b.seq);
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

/// Replaces each glyph's resource font name with the resolved `/BaseFont` when
/// the page's `/Resources /Font` dict provides one — so span flags can use the
/// real font name (e.g. `Helvetica-Bold`) rather than the resource alias (`F1`).
/// Falls back to the resource name when unresolvable.
fn enrich_glyph_fonts(
    doc: &DocumentStore,
    page_dict: &pdf_core::Dict,
    mut glyphs: Vec<PositionedGlyph>,
) -> Vec<PositionedGlyph> {
    use std::collections::HashMap;
    let mut cache: HashMap<SmolStr, Option<SmolStr>> = HashMap::new();
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

    for g in &mut glyphs {
        let resolved = cache.entry(g.font_name.clone()).or_insert_with(|| {
            let fonts = fonts.as_ref()?;
            let fd = doc
                .resolve_dict_key(fonts, &Name::new(g.font_name.as_str()))
                .ok()
                .flatten()?;
            let fd = fd.as_dict()?;
            base_font_name(doc, fd)
        });
        if let Some(base) = resolved {
            g.font_name = base.clone();
        }
    }
    glyphs
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
