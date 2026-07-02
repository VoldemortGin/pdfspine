//! Flow layout (TS-4, PRD §10 scope (c)): measure → wrap → paginate →
//! positioned draw ops, generalized from the proven `pdf-markdown` layouter.
//!
//! Everything happens in **top-left page coordinates** (y grows downward);
//! the emitter flips to PDF user space. The layouter does its own per-line
//! pagination (`insert_textbox` is deliberately not used — it silently drops
//! overflow, PRD §10 TRAP), so text can never be lost across page breaks.
//!
//! Generalizations past pdf-markdown (its TRAPs, PRD §10):
//!
//! - fragments carry an open [`FaceId`] **and a per-frag size** — one line may
//!   mix faces and sizes, sharing a single baseline computed from the real
//!   font ascent/descent (the `BASELINE_FACTOR = 0.8` heuristic is retired);
//! - `Align::Justify` redistributes inter-word **space fragment widths**
//!   (last / hard-broken lines stay left) — PDF `Tw` cannot implement justify
//!   under Identity-H;
//! - underline / strike / highlight decorations are materialized into
//!   [`Op::Line`] / [`Op::FillRect`] using the face's `post` / `OS/2` metrics;
//! - first-line / hanging / left / right indents (per-line-index wrap widths),
//!   configurable line spacing (multiple / exact) and Word-ish space
//!   before/after (pending-gap model: gaps collapse at a page top);
//! - pagination is driven by a [`PageProvider`] callback (docspine sections
//!   with per-section page geometry), while text boxes and table cells reuse
//!   the same core through [`layout_box_content`] (no pagination, optional
//!   wrap-off where lines break only at hard `\n`).
//!
//! Small documented degradations: tabs lay out as a single space (tab stops
//! are out of v1 scope); consecutive typed spaces are preserved; a paragraph
//! with **no runs** contributes no line box (consumers pass an empty-text run
//! carrying the paragraph-mark style to get Word's empty-paragraph height);
//! CJK-only justified lines stay left-aligned (no inter-character justify).

use crate::faces::FaceRegistry;
use crate::model::{Align, Block, ImageSpec, LineSpacing, ListLabel, Run};
use crate::ops::{FaceId, Op, PageOps};
use crate::warn::ExportWarning;
use crate::{Rgb, Typesetter};

/// Geometric comparison tolerance.
pub(crate) const EPS: f64 = 1e-6;

/// A4 page geometry constants (the [`PageGeom::a4`] default).
const A4_WIDTH_PT: f64 = 595.32;
const A4_HEIGHT_PT: f64 = 841.92;
const A4_MARGIN_PT: f64 = 72.0;

// --- page provider -----------------------------------------------------------

/// The geometry of one output page: size plus content margins, in points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PageGeom {
    /// Page width.
    pub width: f64,
    /// Page height.
    pub height: f64,
    /// Top margin.
    pub margin_top: f64,
    /// Right margin.
    pub margin_right: f64,
    /// Bottom margin.
    pub margin_bottom: f64,
    /// Left margin.
    pub margin_left: f64,
}

impl PageGeom {
    /// A4 portrait with 72 pt margins.
    #[must_use]
    pub fn a4() -> Self {
        PageGeom {
            width: A4_WIDTH_PT,
            height: A4_HEIGHT_PT,
            margin_top: A4_MARGIN_PT,
            margin_right: A4_MARGIN_PT,
            margin_bottom: A4_MARGIN_PT,
            margin_left: A4_MARGIN_PT,
        }
    }

    /// A `width` × `height` page with a uniform `margin`.
    #[must_use]
    pub fn new(width: f64, height: f64, margin: f64) -> Self {
        PageGeom {
            width,
            height,
            margin_top: margin,
            margin_right: margin,
            margin_bottom: margin,
            margin_left: margin,
        }
    }

    /// A defensively sanitized copy: finite positive page size (degrades to
    /// A4), non-negative margins clamped so at least 1 pt of content area
    /// remains in each axis (degrade-never-panic).
    fn sanitized(self) -> Self {
        let mut g = self;
        if !(g.width.is_finite() && g.width > 0.0) {
            g.width = A4_WIDTH_PT;
        }
        if !(g.height.is_finite() && g.height > 0.0) {
            g.height = A4_HEIGHT_PT;
        }
        for m in [
            &mut g.margin_top,
            &mut g.margin_right,
            &mut g.margin_bottom,
            &mut g.margin_left,
        ] {
            if !m.is_finite() || *m < 0.0 {
                *m = 0.0;
            }
        }
        if g.margin_left + g.margin_right + 1.0 > g.width {
            g.margin_left = 0.0;
            g.margin_right = 0.0;
        }
        if g.margin_top + g.margin_bottom + 1.0 > g.height {
            g.margin_top = 0.0;
            g.margin_bottom = 0.0;
        }
        g
    }
}

/// Pagination callback (PRD §10 flow core): the layouter asks for the next
/// page's geometry every time it starts one — docspine sections can vary page
/// size / margins per section; presentations never call this (slides are laid
/// out as text boxes on fixed pages).
pub trait PageProvider {
    /// The geometry of the next page (called once per page, first included).
    fn next_page(&mut self) -> PageGeom;
}

/// The trivial provider: every page has the same geometry.
#[derive(Copy, Clone, Debug)]
pub struct FixedPages {
    geom: PageGeom,
}

impl FixedPages {
    /// A provider repeating `geom` forever.
    #[must_use]
    pub fn new(geom: PageGeom) -> Self {
        FixedPages { geom }
    }
}

impl PageProvider for FixedPages {
    fn next_page(&mut self) -> PageGeom {
        self.geom
    }
}

// --- fragments / tokens -------------------------------------------------------

/// A run of same-face, same-size, same-decoration text (fallback-resolved).
#[derive(Clone)]
pub(crate) struct Frag {
    pub(crate) face: FaceId,
    pub(crate) size: f64,
    pub(crate) color: Rgb,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) highlight: Option<Rgb>,
    pub(crate) text: String,
    pub(crate) width: f64,
    /// A collapsible inter-word space (justify widens these).
    pub(crate) space: bool,
    /// Width was widened by justify (breaks the text-op merge chain).
    pub(crate) stretched: bool,
}

/// One wrapped output line.
pub(crate) struct LineOut {
    pub(crate) frags: Vec<Frag>,
    pub(crate) width: f64,
    /// Ended by an explicit hard break (`\n`) — never justified.
    pub(crate) hard: bool,
}

/// An atomic wrapping unit.
pub(crate) enum Tok {
    /// An unbreakable word (may span fallback faces / sizes). CJK characters
    /// arrive as one-char words so they can break anywhere.
    Word { frags: Vec<Frag>, width: f64 },
    /// An inter-word space (kept as its own fragment for justify).
    Space { frag: Frag },
    /// A hard line break.
    Break,
}

/// Whether `ch` breaks like CJK (no inter-word spaces; break at any char).
fn is_cjk(ch: char) -> bool {
    let cp = ch as u32;
    (0x1100..=0x11FF).contains(&cp)           // Hangul Jamo
        || (0x2E80..=0x9FFF).contains(&cp)    // CJK radicals … unified ideographs
        || (0xAC00..=0xD7AF).contains(&cp)    // Hangul syllables
        || (0xF900..=0xFAFF).contains(&cp)    // compatibility ideographs
        || (0xFF00..=0xFFEF).contains(&cp)    // full/half-width forms
        || (0x20000..=0x3FFFF).contains(&cp) // extension planes
}

/// Appends `frag` to `frags`, merging with the tail when both are non-space
/// and face / size / color / decorations match.
fn push_frag(frags: &mut Vec<Frag>, frag: Frag) {
    if !frag.space {
        if let Some(last) = frags.last_mut() {
            if !last.space
                && !last.stretched
                && last.face == frag.face
                && (last.size - frag.size).abs() < EPS
                && last.color == frag.color
                && last.underline == frag.underline
                && last.strike == frag.strike
                && last.highlight == frag.highlight
            {
                last.text.push_str(&frag.text);
                last.width += frag.width;
                return;
            }
        }
    }
    frags.push(frag);
}

/// Converts styled runs into wrap tokens: per-char font fallback, `\n` hard
/// breaks, tabs as spaces, control chars dropped, CJK chars as one-char words
/// (U+3000 ideographic space stays a full-width breakable char; U+00A0 no-break
/// space stays inside its word).
pub(crate) fn tokens(ts: &mut Typesetter, runs: &[Run]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut word: Vec<Frag> = Vec::new();
    let mut word_w = 0.0f64;

    for run in runs {
        let style = &run.style;
        let size = style.size;
        if !(size.is_finite() && size > 0.0) {
            continue; // degrade: an unusable size renders nothing
        }
        let base = ts.base_face(style);
        for ch in run.text.chars() {
            if ch == '\n' {
                flush_word(&mut out, &mut word, &mut word_w);
                out.push(Tok::Break);
                continue;
            }
            let ch = if ch == '\t' { ' ' } else { ch };
            if ch.is_control() {
                continue;
            }
            let breaks_anywhere = is_cjk(ch);
            if !breaks_anywhere && ch != '\u{a0}' && ch.is_whitespace() {
                flush_word(&mut out, &mut word, &mut word_w);
                let face = ts.char_face(&base, ' ');
                let width = ts.faces().advance(face, ' ', size);
                out.push(Tok::Space {
                    frag: Frag {
                        face,
                        size,
                        color: style.color,
                        underline: style.underline,
                        strike: style.strike,
                        highlight: style.highlight,
                        text: " ".to_string(),
                        width,
                        space: true,
                        stretched: false,
                    },
                });
                continue;
            }
            let face = ts.char_face(&base, ch);
            let width = ts.faces().advance(face, ch, size);
            let frag = Frag {
                face,
                size,
                color: style.color,
                underline: style.underline,
                strike: style.strike,
                highlight: style.highlight,
                text: ch.to_string(),
                width,
                space: false,
                stretched: false,
            };
            if breaks_anywhere {
                flush_word(&mut out, &mut word, &mut word_w);
                out.push(Tok::Word {
                    frags: vec![frag],
                    width,
                });
            } else {
                word_w += width;
                push_frag(&mut word, frag);
            }
        }
        flush_word(&mut out, &mut word, &mut word_w);
    }
    flush_word(&mut out, &mut word, &mut word_w);
    out
}

fn flush_word(out: &mut Vec<Tok>, word: &mut Vec<Frag>, word_w: &mut f64) {
    if !word.is_empty() {
        out.push(Tok::Word {
            frags: std::mem::take(word),
            width: *word_w,
        });
        *word_w = 0.0;
    }
}

/// The widest soft-unbreakable line of `toks`: the max run width between hard
/// breaks (auto table-column sizing).
pub(crate) fn natural_width(toks: &[Tok]) -> f64 {
    let mut max_w: f64 = 0.0;
    let mut cur = 0.0;
    for tok in toks {
        match tok {
            Tok::Break => {
                max_w = max_w.max(cur);
                cur = 0.0;
            }
            Tok::Word { width, .. } => cur += width,
            Tok::Space { frag } => cur += frag.width,
        }
    }
    max_w.max(cur)
}

/// Greedy line breaker with per-line-index widths (`w_first` for line 0 —
/// first-line / hanging indent — then `w_rest`): fills the line, breaking
/// before words; a word wider than the whole line is force-split at character
/// granularity. Trailing spaces are stripped from every line; spaces are
/// dropped only at the start of *soft-wrapped* continuation lines (typed
/// leading spaces at a paragraph start / after `\n` are preserved).
pub(crate) fn wrap(faces: &FaceRegistry, toks: &[Tok], w_first: f64, w_rest: f64) -> Vec<LineOut> {
    let mut lines: Vec<LineOut> = Vec::new();
    let mut cur: Vec<Frag> = Vec::new();
    let mut cur_w = 0.0f64;
    let mut after_soft = false;

    let flush = |cur: &mut Vec<Frag>,
                 cur_w: &mut f64,
                 lines: &mut Vec<LineOut>,
                 keep_empty: bool,
                 hard: bool| {
        while cur.last().is_some_and(|f| f.space) {
            let f = cur.pop().unwrap_or_else(|| unreachable!());
            *cur_w -= f.width;
        }
        if !cur.is_empty() || keep_empty {
            lines.push(LineOut {
                frags: std::mem::take(cur),
                width: *cur_w,
                hard,
            });
        } else {
            cur.clear();
        }
        *cur_w = 0.0;
    };

    for tok in toks {
        let limit = if lines.is_empty() { w_first } else { w_rest };
        match tok {
            Tok::Break => {
                flush(&mut cur, &mut cur_w, &mut lines, true, true);
                after_soft = false;
            }
            Tok::Space { frag } => {
                if cur.is_empty() && after_soft {
                    continue;
                }
                cur_w += frag.width;
                cur.push(frag.clone());
            }
            Tok::Word { frags, width } => {
                if !cur.is_empty() && cur_w + width > limit + EPS {
                    flush(&mut cur, &mut cur_w, &mut lines, false, false);
                    after_soft = true;
                }
                let limit = if lines.is_empty() { w_first } else { w_rest };
                if *width > limit + EPS {
                    // Force-split at character granularity.
                    for frag in frags {
                        for ch in frag.text.chars() {
                            let cw = faces.advance(frag.face, ch, frag.size);
                            let limit = if lines.is_empty() { w_first } else { w_rest };
                            if !cur.is_empty() && cur_w + cw > limit + EPS {
                                flush(&mut cur, &mut cur_w, &mut lines, false, false);
                                after_soft = true;
                            }
                            let mut piece = frag.clone();
                            piece.text = ch.to_string();
                            piece.width = cw;
                            cur_w += cw;
                            push_frag(&mut cur, piece);
                        }
                    }
                } else {
                    for frag in frags {
                        push_frag(&mut cur, frag.clone());
                    }
                    cur_w += width;
                }
            }
        }
    }
    flush(&mut cur, &mut cur_w, &mut lines, false, false);
    lines
}

// --- layout context -----------------------------------------------------------

/// The two layout targets sharing one measure/wrap/emit core (PRD §10):
/// paginated flow (docspine body) and a fixed-width unbounded box (text boxes,
/// table cells) with an optional wrap-off policy.
enum Mode<'p> {
    Paged {
        provider: &'p mut dyn PageProvider,
        geom: PageGeom,
    },
    Boxed {
        width: f64,
        wrap: bool,
    },
}

/// The paginating cursor: current page, y position, pending inter-block gap
/// (collapsed at a page top — Word-ish space before/after), plus the content
/// extent tracking used by text-box overflow detection.
pub(crate) struct Ctx<'t, 'p> {
    pub(crate) ts: &'t mut Typesetter,
    mode: Mode<'p>,
    pub(crate) pages: Vec<PageOps>,
    page: usize,
    pub(crate) y: f64,
    pending: f64,
    pub(crate) max_x: f64,
}

impl Ctx<'_, '_> {
    pub(crate) fn top(&self) -> f64 {
        match &self.mode {
            Mode::Paged { geom, .. } => geom.margin_top,
            Mode::Boxed { .. } => 0.0,
        }
    }

    pub(crate) fn bottom(&self) -> f64 {
        match &self.mode {
            Mode::Paged { geom, .. } => geom.height - geom.margin_bottom,
            Mode::Boxed { .. } => f64::INFINITY,
        }
    }

    pub(crate) fn left(&self) -> f64 {
        match &self.mode {
            Mode::Paged { geom, .. } => geom.margin_left,
            Mode::Boxed { .. } => 0.0,
        }
    }

    pub(crate) fn right(&self) -> f64 {
        match &self.mode {
            Mode::Paged { geom, .. } => geom.width - geom.margin_right,
            Mode::Boxed { width, .. } => *width,
        }
    }

    fn wrap_enabled(&self) -> bool {
        match &self.mode {
            Mode::Paged { .. } => true,
            Mode::Boxed { wrap, .. } => *wrap,
        }
    }

    /// Starts a new page (no-op in box mode).
    fn new_page(&mut self) {
        match &mut self.mode {
            Mode::Paged { provider, geom } => {
                *geom = provider.next_page().sanitized();
                self.pages.push(PageOps {
                    width: geom.width,
                    height: geom.height,
                    ops: Vec::new(),
                });
                self.page = self.pages.len() - 1;
                self.y = geom.margin_top;
                self.pending = 0.0;
            }
            Mode::Boxed { .. } => {}
        }
    }

    /// An explicit `Block::PageBreak` (ignored in box mode, PRD §10 TS-5).
    fn page_break(&mut self) {
        if matches!(self.mode, Mode::Paged { .. }) {
            self.pending = 0.0;
            self.new_page();
        }
    }

    /// Requests `g` of space before the next content (gaps accumulate —
    /// Word adds space-after and the next paragraph's space-before).
    pub(crate) fn gap(&mut self, g: f64) {
        if g.is_finite() && g > 0.0 {
            self.pending += g;
        }
    }

    /// Applies the pending gap (dropped at the top of a page / box).
    pub(crate) fn flush_gap(&mut self) {
        if self.y > self.top() + EPS {
            self.y += self.pending;
        }
        self.pending = 0.0;
    }

    /// Starts a new page if `h` does not fit below the cursor (and the cursor
    /// has left the top margin — content taller than a whole page overflows
    /// rather than looping).
    pub(crate) fn ensure(&mut self, h: f64) {
        if self.y + h > self.bottom() + EPS && self.y > self.top() + EPS {
            self.new_page();
        }
    }

    pub(crate) fn op(&mut self, op: Op) {
        self.pages[self.page].ops.push(op);
    }

    pub(crate) fn extend_ops(&mut self, ops: Vec<Op>) {
        self.pages[self.page].ops.extend(ops);
    }
}

// --- entry points ---------------------------------------------------------------

/// Lays out `blocks` as paginated flow; page geometry comes from `provider`
/// (called once per started page). Always returns at least one page.
pub(crate) fn layout_flow(
    ts: &mut Typesetter,
    blocks: &[Block],
    provider: &mut dyn PageProvider,
) -> Vec<PageOps> {
    let geom = provider.next_page().sanitized();
    let mut ctx = Ctx {
        ts,
        pages: vec![PageOps {
            width: geom.width,
            height: geom.height,
            ops: Vec::new(),
        }],
        page: 0,
        y: geom.margin_top,
        pending: 0.0,
        max_x: 0.0,
        mode: Mode::Paged { provider, geom },
    };
    layout_blocks(&mut ctx, blocks);
    ctx.pages
}

/// Lays out `blocks` into an unbounded box of `width` (origin `(0, 0)`,
/// top-left coords): the shared core behind text boxes and table cells.
/// Returns `(ops, content_height, content_max_x)`; `wrap == false` breaks
/// lines only at hard `\n`.
pub(crate) fn layout_box_content(
    ts: &mut Typesetter,
    blocks: &[Block],
    width: f64,
    wrap: bool,
) -> (Vec<Op>, f64, f64) {
    let width = if width.is_finite() {
        width.max(1.0)
    } else {
        1.0
    };
    let mut ctx = Ctx {
        ts,
        pages: vec![PageOps {
            width,
            height: f64::INFINITY,
            ops: Vec::new(),
        }],
        page: 0,
        y: 0.0,
        pending: 0.0,
        max_x: 0.0,
        mode: Mode::Boxed { width, wrap },
    };
    layout_blocks(&mut ctx, blocks);
    let height = ctx.y;
    let max_x = ctx.max_x;
    let ops = ctx.pages.swap_remove(0).ops;
    (ops, height, max_x)
}

/// Lays out sibling blocks at the context cursor.
pub(crate) fn layout_blocks(ctx: &mut Ctx, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(props, runs) => layout_paragraph(ctx, props, runs),
            Block::Table(spec) => crate::table::layout_table(ctx, spec),
            Block::Image(spec) => layout_image(ctx, spec),
            Block::PageBreak => ctx.page_break(),
        }
    }
}

// --- paragraph -------------------------------------------------------------------

/// Lays out one paragraph under its properties: indents, wrap, alignment
/// (incl. justify), line spacing, list label, per-line pagination.
fn layout_paragraph(ctx: &mut Ctx, props: &crate::model::ParaProps, runs: &[Run]) {
    ctx.gap(props.space_before);
    ctx.flush_gap();

    let left = ctx.left() + props.indent_left.max(0.0);
    let right = (ctx.right() - props.indent_right.max(0.0)).max(left + 1.0);
    let first_extra = if props.first_line_indent.is_finite() {
        props.first_line_indent
    } else {
        0.0
    };
    let first_left = left + first_extra;
    let (w_first, w_rest) = if ctx.wrap_enabled() {
        ((right - first_left).max(1.0), (right - left).max(1.0))
    } else {
        (f64::INFINITY, f64::INFINITY)
    };

    let toks = tokens(ctx.ts, runs);
    let mut lines = wrap(ctx.ts.faces(), &toks, w_first, w_rest);

    // Reference metrics for empty lines (empty paragraph / blank `\n\n` line):
    // the first usable run style (the paragraph-mark style by convention).
    let ref_frag: Option<(FaceId, f64)> = runs
        .iter()
        .find(|r| r.style.size.is_finite() && r.style.size > 0.0)
        .map(|r| {
            let base = ctx.ts.base_face(&r.style);
            (ctx.ts.face_id(&base), r.style.size)
        });
    if lines.is_empty() {
        if ref_frag.is_none() {
            // No runs at all: nothing to size a line box from.
            ctx.gap(props.space_after);
            return;
        }
        lines.push(LineOut {
            frags: Vec::new(),
            width: 0.0,
            hard: false,
        });
    }

    if props.align == Align::Justify {
        let n = lines.len();
        for (i, line) in lines.iter_mut().enumerate() {
            if i + 1 == n || line.hard {
                continue; // last / hard-broken lines stay left
            }
            justify_line(line, if i == 0 { w_first } else { w_rest });
        }
    }

    for (i, line) in lines.iter().enumerate() {
        let (desc, natural) = line_metrics(ctx.ts.faces(), line, ref_frag);
        let lh = match props.spacing {
            LineSpacing::Multiple(m) => natural * if m.is_finite() && m > 0.0 { m } else { 1.0 },
            LineSpacing::Exact(h) => {
                if h.is_finite() && h > 0.0 {
                    h
                } else {
                    natural
                }
            }
        };
        ctx.ensure(lh);
        let baseline = ctx.y + lh - desc;
        let line_left = if i == 0 { first_left } else { left };
        let align_w = right - line_left;
        let offset = match props.align {
            Align::Left | Align::Justify => 0.0,
            Align::Center => (align_w - line.width) / 2.0,
            Align::Right => align_w - line.width,
        };
        let x0 = line_left + offset;
        if i == 0 {
            if let Some(label) = &props.list {
                draw_list_label(ctx, label, runs, first_left, baseline);
            }
        }
        emit_line(ctx, line, x0, baseline);
        ctx.max_x = ctx.max_x.max(x0 + line.width);
        ctx.y += lh;
    }

    ctx.gap(props.space_after);
}

/// Widens the line's space fragments so its width reaches `target` (justify).
fn justify_line(line: &mut LineOut, target: f64) {
    if !target.is_finite() {
        return;
    }
    let nspaces = line.frags.iter().filter(|f| f.space).count();
    if nspaces == 0 {
        return; // single-word / CJK-only lines cannot justify
    }
    let deficit = target - line.width;
    if deficit <= EPS {
        return;
    }
    let extra = deficit / nspaces as f64;
    for frag in &mut line.frags {
        if frag.space {
            frag.width += extra;
            frag.stretched = true;
        }
    }
    line.width = target;
}

/// The line box metrics of one wrapped line: max descent (points below the
/// baseline) and the natural single-spaced line height, both maxed over the
/// line's mixed faces/sizes (so mixed-size lines share one baseline).
fn line_metrics(
    faces: &FaceRegistry,
    line: &LineOut,
    ref_frag: Option<(FaceId, f64)>,
) -> (f64, f64) {
    let mut desc = 0.0f64;
    let mut natural = 0.0f64;
    for frag in &line.frags {
        let m = faces.metrics(frag.face);
        desc = desc.max(m.descent * frag.size);
        natural = natural.max(m.line_height(frag.size));
    }
    if line.frags.is_empty() {
        if let Some((face, size)) = ref_frag {
            let m = faces.metrics(face);
            desc = m.descent * size;
            natural = m.line_height(size);
        }
    }
    (desc, natural)
}

/// Draws a consumer-computed list label right-aligned against the paragraph's
/// first-line text start (`ListLabel::gutter` away), on the first baseline.
/// The label inherits the first run's family / size / color (decorations
/// dropped — marker formatting is the consumer's `numbering.xml` business).
fn draw_list_label(ctx: &mut Ctx, label: &ListLabel, runs: &[Run], first_left: f64, baseline: f64) {
    let Some(first) = runs
        .iter()
        .find(|r| r.style.size.is_finite() && r.style.size > 0.0)
    else {
        return;
    };
    let mut style = first.style.clone();
    style.underline = false;
    style.strike = false;
    style.highlight = None;
    let line = plain_line(ctx.ts, &label.text, &style);
    let x0 = first_left - label.gutter.max(0.0) - line.width;
    emit_line(ctx, &line, x0, baseline);
}

/// Lays a single-line text (no wrapping) out into fragments (list labels).
fn plain_line(ts: &mut Typesetter, text: &str, style: &crate::model::RunStyle) -> LineOut {
    let toks = tokens(ts, &[Run::new(text, style.clone())]);
    let mut lines = wrap(ts.faces(), &toks, f64::INFINITY, f64::INFINITY);
    if lines.is_empty() {
        LineOut {
            frags: Vec::new(),
            width: 0.0,
            hard: true,
        }
    } else {
        lines.swap_remove(0)
    }
}

/// Emits one line at `(x0, baseline)`: highlight rects first, then merged
/// text ops (a widened justify space breaks the merge chain so the following
/// fragment restarts at its exact x), then underline / strike segments from
/// the real face metrics.
fn emit_line(ctx: &mut Ctx, line: &LineOut, x0: f64, baseline: f64) {
    // Pass 1 — highlights behind the text.
    let mut x = x0;
    for frag in &line.frags {
        if let Some(hl) = frag.highlight {
            if frag.width > EPS {
                let m = *ctx.ts.faces().metrics(frag.face);
                ctx.op(Op::FillRect {
                    x,
                    y: baseline - m.ascent * frag.size,
                    w: frag.width,
                    h: (m.ascent + m.descent) * frag.size,
                    color: hl,
                });
            }
        }
        x += frag.width;
    }

    // Pass 2 — text, merged across compatible fragments.
    let mut x = x0;
    let mut cur: Option<(f64, FaceId, f64, Rgb, String)> = None;
    for frag in &line.frags {
        if !frag.text.is_empty() {
            let compatible = cur.as_ref().is_some_and(|(_, face, size, color, _)| {
                *face == frag.face && (*size - frag.size).abs() < EPS && *color == frag.color
            });
            if !compatible {
                flush_text(ctx, &mut cur, baseline);
                cur = Some((x, frag.face, frag.size, frag.color, String::new()));
            }
            if let Some((_, _, _, _, text)) = &mut cur {
                text.push_str(&frag.text);
            }
            if frag.stretched {
                flush_text(ctx, &mut cur, baseline);
            }
        }
        x += frag.width;
    }
    flush_text(ctx, &mut cur, baseline);

    // Pass 3 — decorations over the text.
    let mut x = x0;
    for frag in &line.frags {
        if frag.width > EPS && (frag.underline || frag.strike) {
            let m = *ctx.ts.faces().metrics(frag.face);
            if frag.underline {
                let y = baseline - m.underline_position * frag.size;
                ctx.op(Op::Line {
                    x1: x,
                    y1: y,
                    x2: x + frag.width,
                    y2: y,
                    color: frag.color,
                    width: (m.underline_thickness * frag.size).max(0.1),
                });
            }
            if frag.strike {
                let y = baseline - m.strikeout_position * frag.size;
                ctx.op(Op::Line {
                    x1: x,
                    y1: y,
                    x2: x + frag.width,
                    y2: y,
                    color: frag.color,
                    width: (m.strikeout_thickness * frag.size).max(0.1),
                });
            }
        }
        x += frag.width;
    }
}

/// Flushes the pending merged text op.
fn flush_text(ctx: &mut Ctx, cur: &mut Option<(f64, FaceId, f64, Rgb, String)>, baseline: f64) {
    if let Some((x, face, size, color, text)) = cur.take() {
        if !text.is_empty() {
            ctx.op(Op::Text {
                face,
                size,
                color,
                x,
                baseline,
                text,
            });
        }
    }
}

// --- images ------------------------------------------------------------------------

/// Places a block image at its display size, downscaled proportionally to the
/// available width (and, in paged mode, the page content height) — never
/// upscaled. An undecodable image degrades to a warning (never an error).
fn layout_image(ctx: &mut Ctx, spec: &ImageSpec) {
    ctx.flush_gap();
    if !(spec.width.is_finite() && spec.width > 0.0 && spec.height.is_finite() && spec.height > 0.0)
    {
        ctx.ts.warn(ExportWarning::ImageDropped {
            reason: "non-positive or non-finite display size".to_string(),
        });
        return;
    }
    let Some(id) = ctx.ts.add_image(spec) else {
        return; // warning already recorded
    };
    let avail_w = (ctx.right() - ctx.left()).max(1.0);
    let mut scale = (avail_w / spec.width).min(1.0);
    let content_h = ctx.bottom() - ctx.top();
    if content_h.is_finite() {
        scale = scale.min((content_h / spec.height).min(1.0));
    }
    let (dw, dh) = (spec.width * scale, spec.height * scale);
    ctx.ensure(dh);
    ctx.op(Op::Image {
        id,
        x: ctx.left(),
        y: ctx.y,
        w: dw,
        h: dh,
    });
    ctx.y += dh;
    ctx.max_x = ctx.max_x.max(ctx.left() + dw);
}
