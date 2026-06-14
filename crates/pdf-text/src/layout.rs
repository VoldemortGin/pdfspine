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

/// XY-cut gutter width (fraction of page width / height) required to split a
/// group of blocks into columns / row bands.
const XY_GUTTER_FRAC: f64 = 0.05;

// === public API ===========================================================

/// Builds a [`TextPage`] for a page: runs the interpreter, applies the page
/// transform and groups glyphs (PRD §8.6).
#[must_use]
pub fn build_textpage(doc: &DocumentStore, page: &Page, _limits: &Limits) -> TextPage {
    let Some(page_dict) = page.dict() else {
        return TextPage::default();
    };
    let res: InterpretResult = ContentInterpreter::new(doc).run_page(&page_dict);
    let mediabox = page.mediabox();
    let rotate = page.rotation();
    let glyphs = enrich_glyph_fonts(doc, &page_dict, res.glyphs);
    textpage_from_glyphs(&glyphs, &res.images, mediabox, rotate)
}

/// Builds a [`TextPage`] directly from a glyph list + image inventory in **PDF
/// user space**, a MediaBox and a `/Rotate` value. The unit-test entry point
/// (no document needed).
#[must_use]
pub fn textpage_from_glyphs(
    glyphs: &[PositionedGlyph],
    images: &[ImageRef],
    mediabox: Rect,
    rotate: i32,
) -> TextPage {
    let p = page_transform(mediabox, rotate);
    let (width, height) = page_size(mediabox, rotate);

    // 1. Transform every glyph to device space.
    let dev: Vec<DevGlyph> = glyphs.iter().map(|g| DevGlyph::new(g, &p)).collect();

    // 2/3. lines + spans.
    let lines = group_lines(&dev);

    // 4. blocks (paragraph grouping by vertical proximity / column overlap).
    let mut blocks = group_blocks(lines);

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
        });
    }

    // 5. reading order (XY-cut) + number assignment.
    order_blocks(&mut blocks, width, height);

    TextPage {
        width,
        height,
        blocks,
    }
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
            let line_glyphs: Vec<&DevGlyph> = run.iter().map(|&i| &dev[i]).collect();
            lines.push(build_line(&line_glyphs));
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
/// style (font / size / color / flags) changes.
fn build_line(glyphs: &[&DevGlyph]) -> Line {
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
    }
}

// === block grouping =======================================================

/// Groups lines into paragraph blocks by vertical proximity + horizontal
/// overlap. Lines are assumed top-to-bottom (the order from `group_lines`).
fn group_blocks(lines: Vec<Line>) -> Vec<Block> {
    if lines.is_empty() {
        return Vec::new();
    }

    // Typical line height drives the vertical-gap threshold.
    let typical_h = typical_line_height(&lines);

    let mut blocks: Vec<Block> = Vec::new();
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
            blocks.push(make_text_block(std::mem::take(&mut cur)));
        }
        prev_bottom = Some(line.bbox.y1);
        cur.push(line);
    }
    if !cur.is_empty() {
        blocks.push(make_text_block(cur));
    }
    blocks
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

/// Wraps a run of lines into a text [`Block`] (number assigned later).
fn make_text_block(lines: Vec<Line>) -> Block {
    let mut bbox = Rect::default();
    for l in &lines {
        bbox = bbox.union(&l.bbox);
    }
    Block {
        bbox,
        kind: BlockKind::Text,
        lines,
        image: None,
        number: 0,
    }
}

// === reading order (XY-cut) ===============================================

/// Orders blocks via a recursive XY-cut and assigns sequential numbers
/// (PRD §8.6.2): multi-column pages read column-by-column.
fn order_blocks(blocks: &mut Vec<Block>, width: f64, height: f64) {
    if blocks.len() <= 1 {
        for (i, b) in blocks.iter_mut().enumerate() {
            b.number = i;
        }
        return;
    }
    let idxs: Vec<usize> = (0..blocks.len()).collect();
    let mut ordered: Vec<usize> = Vec::with_capacity(blocks.len());
    xy_cut(blocks, &idxs, width, height, &mut ordered);

    // Rebuild the vector in reading order and number it.
    let mut taken: Vec<Option<Block>> = blocks.drain(..).map(Some).collect();
    for (n, &i) in ordered.iter().enumerate() {
        let mut b = taken[i].take().expect("each block placed once");
        b.number = n;
        blocks.push(b);
    }
}

/// Recursive XY-cut: prefer a **vertical** cut (split into left/right columns)
/// so columns read fully top-to-bottom before moving right; otherwise a
/// **horizontal** cut (top/bottom bands); base case sorts by `(y0, x0)`.
fn xy_cut(blocks: &[Block], idxs: &[usize], width: f64, height: f64, out: &mut Vec<usize>) {
    if idxs.len() <= 1 {
        out.extend_from_slice(idxs);
        return;
    }

    // Try a vertical gutter (columns) first — column-major reading order.
    if let Some(groups) = cut_axis(blocks, idxs, Axis::X, width * XY_GUTTER_FRAC) {
        for g in groups {
            xy_cut(blocks, &g, width, height, out);
        }
        return;
    }
    // Then a horizontal gutter (rows / paragraphs).
    if let Some(groups) = cut_axis(blocks, idxs, Axis::Y, height * XY_GUTTER_FRAC) {
        for g in groups {
            xy_cut(blocks, &g, width, height, out);
        }
        return;
    }
    // No clean cut: stable reading order by (y0, x0).
    let mut sorted = idxs.to_vec();
    sorted.sort_by(|&a, &b| {
        blocks[a]
            .bbox
            .y0
            .total_cmp(&blocks[b].bbox.y0)
            .then(blocks[a].bbox.x0.total_cmp(&blocks[b].bbox.x0))
    });
    out.extend(sorted);
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

/// Attempts to split `idxs` along `axis` wherever a gutter of at least
/// `min_gap` separates consecutive projected intervals. Returns `None` when no
/// gutter splits the set (i.e. a single group). Groups come back in increasing
/// coordinate order along the axis.
fn cut_axis(blocks: &[Block], idxs: &[usize], axis: Axis, min_gap: f64) -> Option<Vec<Vec<usize>>> {
    let interval = |i: usize| -> (f64, f64) {
        let b = &blocks[i].bbox;
        match axis {
            Axis::X => (b.x0, b.x1),
            Axis::Y => (b.y0, b.y1),
        }
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
    if groups.len() <= 1 {
        None
    } else {
        Some(groups)
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
