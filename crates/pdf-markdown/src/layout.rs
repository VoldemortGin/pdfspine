//! The deterministic block layouter: measure → wrap → paginate → positioned
//! draw ops.
//!
//! Everything happens in **top-left page coordinates** (y grows downward from
//! the top margin); the renderer flips to PDF user space at emission. The
//! layouter does its own per-line pagination — `insert_textbox` is deliberately
//! not used because it silently drops overflow (PRD §9 TRAP) — so text can
//! never be lost across page breaks.
//!
//! All typographic constants live at the top of this module.

use crate::fonts::{Face, FontSet};
use crate::images::PreparedImage;
use crate::model::{Block, CellAlign, Inline, Style};
use crate::Options;

// --- typographic constants (single source of truth) ------------------------

/// Body / heading line height as a multiple of the font size.
const LINE_FACTOR: f64 = 1.4;
/// Code-block line height as a multiple of the code font size.
const CODE_LINE_FACTOR: f64 = 1.3;
/// Baseline offset from the top of a line box, as a multiple of the font size.
const BASELINE_FACTOR: f64 = 0.8;
/// Vertical gap between sibling blocks, × body size.
const BLOCK_GAP_EM: f64 = 0.7;
/// Extra gap before a heading, × heading size.
const HEADING_GAP_BEFORE_EM: f64 = 0.7;
/// Gap after a heading, × heading size.
const HEADING_GAP_AFTER_EM: f64 = 0.35;
/// Heading size ladder (H1..H6), × body size.
const HEADING_SCALE: [f64; 6] = [2.0, 1.5, 1.25, 1.1, 1.0, 0.9];
/// Heading line height, × heading size.
const HEADING_LINE_FACTOR: f64 = 1.25;
/// Gap between list items, × body size.
const ITEM_GAP_EM: f64 = 0.3;
/// Indent of list-item content per nesting level, in points.
const LIST_INDENT: f64 = 22.0;
/// Gap between a list marker's right edge and the item content, in points.
const LIST_MARKER_GAP: f64 = 7.0;
/// Bullet radius, × body size.
const BULLET_RADIUS_EM: f64 = 0.11;
/// Task-list checkbox side, × body size.
const CHECKBOX_EM: f64 = 0.75;
/// Blockquote bar width, in points.
const QUOTE_BAR_WIDTH: f64 = 3.0;
/// Blockquote content indent, in points.
const QUOTE_INDENT: f64 = 14.0;
/// Blockquote bar color.
const QUOTE_BAR_COLOR: Rgb = Rgb(0.62, 0.62, 0.62);
/// Code-block background color.
const CODE_BG: Rgb = Rgb(0.95, 0.95, 0.95);
/// Code-block padding on every side, in points.
const CODE_PAD: f64 = 6.0;
/// Code font size, × body size.
const CODE_SIZE_EM: f64 = 0.9;
/// Table cell padding, in points.
const TABLE_PAD: f64 = 4.0;
/// Table border stroke width, in points.
const TABLE_BORDER_WIDTH: f64 = 0.75;
/// Table border color.
const TABLE_BORDER_COLOR: Rgb = Rgb(0.45, 0.45, 0.45);
/// Table header-row background.
const TABLE_HEAD_BG: Rgb = Rgb(0.92, 0.92, 0.92);
/// Narrowest a table column may shrink, in points.
const MIN_COL_WIDTH: f64 = 24.0;
/// Link text color (v1 renders links as colored text, no annotation).
const LINK_COLOR: Rgb = Rgb(0.05, 0.25, 0.7);
/// Default text color.
const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);
/// Horizontal-rule color / stroke width / block height, in points.
const RULE_COLOR: Rgb = Rgb(0.6, 0.6, 0.6);
const RULE_WIDTH: f64 = 0.75;
const RULE_BLOCK_H: f64 = 6.0;
/// Strikethrough raise above the baseline / stroke width, × font size.
const STRIKE_RAISE: f64 = 0.28;
const STRIKE_WIDTH_EM: f64 = 0.05;
/// Geometric comparison tolerance.
const EPS: f64 = 1e-6;

// --- draw-op IR -------------------------------------------------------------

/// An RGB color, each component in `0.0..=1.0`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Rgb(pub f64, pub f64, pub f64);

/// One positioned draw operation, in top-left page coordinates.
pub(crate) enum Op {
    /// A single-face text run at a baseline point.
    Text {
        face: Face,
        size: f64,
        color: Rgb,
        x: f64,
        baseline: f64,
        text: String,
    },
    /// A filled axis-aligned rectangle (`y` is the top edge).
    FillRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: Rgb,
    },
    /// A stroked axis-aligned rectangle.
    StrokeRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: Rgb,
        line_width: f64,
    },
    /// A stroked segment.
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: Rgb,
        width: f64,
    },
    /// A filled circle (list bullets).
    FillCircle {
        cx: f64,
        cy: f64,
        r: f64,
        color: Rgb,
    },
    /// A placed image (`y` is the top edge; `id` indexes the prepared images).
    Image {
        id: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
}

/// The draw ops of one output page, in paint order.
pub(crate) struct PageOps {
    pub(crate) ops: Vec<Op>,
}

// --- text fragments / tokens ------------------------------------------------

/// A run of same-face, same-decoration text (already fallback-resolved).
#[derive(Clone)]
struct Frag {
    face: Face,
    text: String,
    width: f64,
    color: Rgb,
    strike: bool,
}

/// One wrapped output line.
struct LineOut {
    frags: Vec<Frag>,
    width: f64,
}

/// An atomic wrapping unit.
enum Tok {
    /// An unbreakable word (may span fallback faces). CJK characters arrive as
    /// one-char words so they can break anywhere.
    Word { frags: Vec<Frag>, width: f64 },
    /// A collapsible inter-word space.
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

/// Appends `text` to `frags`, merging with the tail when face/decoration match.
fn push_frag(frags: &mut Vec<Frag>, face: Face, text: &str, width: f64, color: Rgb, strike: bool) {
    if let Some(last) = frags.last_mut() {
        if last.face == face && last.color == color && last.strike == strike {
            last.text.push_str(text);
            last.width += width;
            return;
        }
    }
    frags.push(Frag {
        face,
        text: text.to_string(),
        width,
        color,
        strike,
    });
}

/// Converts inline content into wrap tokens at `size` points. `force_bold`
/// upgrades every run (headings, table headers).
fn tokens(fonts: &FontSet, inlines: &[Inline], size: f64, force_bold: bool) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut word: Vec<Frag> = Vec::new();
    let mut word_w = 0.0;

    for inline in inlines {
        match inline {
            Inline::HardBreak => {
                flush_word(&mut out, &mut word, &mut word_w);
                out.push(Tok::Break);
            }
            Inline::Text { text, style } => {
                let mut style = *style;
                if force_bold {
                    style.bold = true;
                }
                let pref = fonts.face_for(style);
                let color = if style.link { LINK_COLOR } else { BLACK };
                let strike = style.strike;
                let (space_face, space_ch) = fonts.resolve(pref, ' ');
                let space_w = fonts.advance(space_face, space_ch, size);

                for ch in text.chars() {
                    let ch = if ch == '\t' { ' ' } else { ch };
                    if ch.is_control() {
                        continue;
                    }
                    if ch == ' ' || ch.is_whitespace() {
                        flush_word(&mut out, &mut word, &mut word_w);
                        if !matches!(out.last(), Some(Tok::Space { .. }) | None) {
                            out.push(Tok::Space {
                                frag: Frag {
                                    face: space_face,
                                    text: " ".to_string(),
                                    width: space_w,
                                    color,
                                    strike,
                                },
                            });
                        }
                        continue;
                    }
                    let (face, eff) = fonts.resolve(pref, ch);
                    let w = fonts.advance(face, eff, size);
                    if is_cjk(ch) {
                        flush_word(&mut out, &mut word, &mut word_w);
                        out.push(Tok::Word {
                            frags: vec![Frag {
                                face,
                                text: eff.to_string(),
                                width: w,
                                color,
                                strike,
                            }],
                            width: w,
                        });
                    } else {
                        push_frag(
                            &mut word,
                            face,
                            eff.encode_utf8(&mut [0u8; 4]),
                            w,
                            color,
                            strike,
                        );
                        word_w += w;
                    }
                }
                flush_word(&mut out, &mut word, &mut word_w);
            }
        }
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

/// The widest unbreakable-with-soft-wrapping line of `toks` (used for table
/// column sizing): the max run width between hard breaks.
fn natural_width(toks: &[Tok]) -> f64 {
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

/// Greedy line breaker: fills `width`, breaking before words; a word wider than
/// the whole line is force-split at character granularity (long URLs, CJK-free
/// runs). Trailing spaces are stripped from every line.
fn wrap(fonts: &FontSet, toks: &[Tok], width: f64, size: f64) -> Vec<LineOut> {
    let width = width.max(1.0);
    let mut lines: Vec<LineOut> = Vec::new();
    let mut cur: Vec<Frag> = Vec::new();
    let mut cur_w = 0.0;

    let flush =
        |cur: &mut Vec<Frag>, cur_w: &mut f64, lines: &mut Vec<LineOut>, keep_empty: bool| {
            // Strip trailing whitespace fragments.
            while cur.last().is_some_and(|f| f.text.chars().all(|c| c == ' ')) {
                let f = cur.pop().unwrap_or_else(|| unreachable!());
                *cur_w -= f.width;
            }
            if !cur.is_empty() || keep_empty {
                lines.push(LineOut {
                    frags: std::mem::take(cur),
                    width: *cur_w,
                });
            } else {
                cur.clear();
            }
            *cur_w = 0.0;
        };

    for tok in toks {
        match tok {
            Tok::Break => flush(&mut cur, &mut cur_w, &mut lines, true),
            Tok::Space { frag } => {
                if !cur.is_empty() {
                    push_frag(
                        &mut cur,
                        frag.face,
                        &frag.text,
                        frag.width,
                        frag.color,
                        frag.strike,
                    );
                    cur_w += frag.width;
                }
            }
            Tok::Word { frags, width: w } => {
                if !cur.is_empty() && cur_w + w > width + EPS {
                    flush(&mut cur, &mut cur_w, &mut lines, false);
                }
                if *w > width + EPS {
                    // Force-split at character granularity.
                    for frag in frags {
                        for ch in frag.text.chars() {
                            let cw = fonts.advance(frag.face, ch, size);
                            if !cur.is_empty() && cur_w + cw > width + EPS {
                                flush(&mut cur, &mut cur_w, &mut lines, false);
                            }
                            push_frag(
                                &mut cur,
                                frag.face,
                                ch.encode_utf8(&mut [0u8; 4]),
                                cw,
                                frag.color,
                                frag.strike,
                            );
                            cur_w += cw;
                        }
                    }
                } else {
                    for frag in frags {
                        push_frag(
                            &mut cur,
                            frag.face,
                            &frag.text,
                            frag.width,
                            frag.color,
                            frag.strike,
                        );
                    }
                    cur_w += w;
                }
            }
        }
    }
    flush(&mut cur, &mut cur_w, &mut lines, false);
    lines
}

/// Measures `text` on the preferred face (with per-char fallback), at `size`.
fn measure(fonts: &FontSet, pref: Face, text: &str, size: f64) -> f64 {
    text.chars()
        .map(|ch| {
            let (face, eff) = fonts.resolve(pref, ch);
            fonts.advance(face, eff, size)
        })
        .sum()
}

// --- layout context ----------------------------------------------------------

/// The paginating cursor: current page, y position, and pending inter-block gap
/// (dropped at the top of a page so pages never start with dead space).
struct Ctx<'a> {
    fonts: &'a FontSet,
    opts: &'a Options,
    images: &'a [PreparedImage],
    pages: Vec<PageOps>,
    page: usize,
    y: f64,
    pending: f64,
}

impl Ctx<'_> {
    fn top(&self) -> f64 {
        self.opts.margin_top
    }
    fn bottom(&self) -> f64 {
        self.opts.page_height - self.opts.margin_bottom
    }
    fn left(&self) -> f64 {
        self.opts.margin_left
    }
    fn right(&self) -> f64 {
        self.opts.page_width - self.opts.margin_right
    }

    fn new_page(&mut self) {
        self.pages.push(PageOps { ops: Vec::new() });
        self.page = self.pages.len() - 1;
        self.y = self.top();
        self.pending = 0.0;
    }

    /// Requests at least `g` of space before the next block.
    fn gap(&mut self, g: f64) {
        self.pending = self.pending.max(g);
    }

    /// Applies the pending gap (unless at the top of a page).
    fn flush_gap(&mut self) {
        if self.y > self.top() + EPS {
            self.y += self.pending;
        }
        self.pending = 0.0;
    }

    /// Starts a new page if `h` does not fit below the cursor (and the cursor
    /// has left the top margin — content taller than a whole page overflows
    /// rather than looping).
    fn ensure(&mut self, h: f64) {
        if self.y + h > self.bottom() + EPS && self.y > self.top() + EPS {
            self.new_page();
        }
    }

    fn op(&mut self, op: Op) {
        self.pages[self.page].ops.push(op);
    }

    /// Emits wrapped lines at `left` within `width`, paginating per line.
    fn emit_lines(
        &mut self,
        lines: &[LineOut],
        size: f64,
        line_factor: f64,
        left: f64,
        width: f64,
        align: CellAlign,
    ) {
        for line in lines {
            let lh = size * line_factor;
            self.ensure(lh);
            let baseline = self.y + size * BASELINE_FACTOR;
            let mut x = left + align_offset(align, width, line.width);
            for frag in &line.frags {
                self.emit_frag(frag, size, x, baseline);
                x += frag.width;
            }
            self.y += lh;
        }
    }

    /// Emits one fragment (text + optional strikethrough) at a baseline point.
    fn emit_frag(&mut self, frag: &Frag, size: f64, x: f64, baseline: f64) {
        if !frag.text.is_empty() {
            self.op(Op::Text {
                face: frag.face,
                size,
                color: frag.color,
                x,
                baseline,
                text: frag.text.clone(),
            });
        }
        if frag.strike && frag.width > EPS {
            let y = baseline - size * STRIKE_RAISE;
            self.op(Op::Line {
                x1: x,
                y1: y,
                x2: x + frag.width,
                y2: y,
                color: frag.color,
                width: size * STRIKE_WIDTH_EM,
            });
        }
    }
}

fn align_offset(align: CellAlign, width: f64, line_w: f64) -> f64 {
    match align {
        CellAlign::Left => 0.0,
        CellAlign::Center => ((width - line_w) / 2.0).max(0.0),
        CellAlign::Right => (width - line_w).max(0.0),
    }
}

// --- block layout -------------------------------------------------------------

/// Lays out `blocks` and returns the positioned pages (always ≥ 1).
pub(crate) fn layout(
    blocks: &[Block],
    images: &[PreparedImage],
    fonts: &FontSet,
    opts: &Options,
) -> Vec<PageOps> {
    let mut ctx = Ctx {
        fonts,
        opts,
        images,
        pages: vec![PageOps { ops: Vec::new() }],
        page: 0,
        y: opts.margin_top,
        pending: 0.0,
    };
    let (left, right) = (ctx.left(), ctx.right());
    layout_blocks(&mut ctx, blocks, left, right);
    ctx.pages
}

/// Lays out sibling blocks between the absolute x bounds `left..right`.
fn layout_blocks(ctx: &mut Ctx, blocks: &[Block], left: f64, right: f64) {
    let body = ctx.opts.body_font_size;
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => {
                let idx = usize::from(level.saturating_sub(1)).min(5);
                let size = body * HEADING_SCALE[idx];
                ctx.gap(size * HEADING_GAP_BEFORE_EM);
                ctx.flush_gap();
                let toks = tokens(ctx.fonts, inlines, size, true);
                let lines = wrap(ctx.fonts, &toks, right - left, size);
                ctx.emit_lines(
                    &lines,
                    size,
                    HEADING_LINE_FACTOR,
                    left,
                    right - left,
                    CellAlign::Left,
                );
                ctx.gap(size * HEADING_GAP_AFTER_EM);
            }
            Block::Paragraph(inlines) => {
                ctx.flush_gap();
                let toks = tokens(ctx.fonts, inlines, body, false);
                let lines = wrap(ctx.fonts, &toks, right - left, body);
                ctx.emit_lines(
                    &lines,
                    body,
                    LINE_FACTOR,
                    left,
                    right - left,
                    CellAlign::Left,
                );
            }
            Block::Code(text) => layout_code(ctx, text, left, right),
            Block::List {
                ordered,
                start,
                items,
            } => layout_list(ctx, *ordered, *start, items, left, right),
            Block::Quote(children) => layout_quote(ctx, children, left, right),
            Block::Rule => {
                ctx.flush_gap();
                ctx.ensure(RULE_BLOCK_H);
                let y = ctx.y + RULE_BLOCK_H / 2.0;
                ctx.op(Op::Line {
                    x1: left,
                    y1: y,
                    x2: right,
                    y2: y,
                    color: RULE_COLOR,
                    width: RULE_WIDTH,
                });
                ctx.y += RULE_BLOCK_H;
            }
            Block::Table { aligns, head, rows } => {
                layout_table(ctx, aligns, head, rows, left, right)
            }
            Block::Image { id, .. } => layout_image(ctx, *id, left, right),
        }
        ctx.gap(body * BLOCK_GAP_EM);
    }
}

/// Code block: monospace lines over a light-gray background, wrapped at
/// character granularity (newlines preserved), paginated in chunks with the
/// background rectangle repeated per page segment.
fn layout_code(ctx: &mut Ctx, text: &str, left: f64, right: f64) {
    let size = ctx.opts.body_font_size * CODE_SIZE_EM;
    let lh = size * CODE_LINE_FACTOR;
    let inner_w = (right - left - 2.0 * CODE_PAD).max(size);

    // Pre-wrap every source line (tabs → 4 spaces, per-char fallback).
    let mut lines: Vec<Vec<Frag>> = Vec::new();
    for src in text.split('\n') {
        let expanded = src.replace('\t', "    ");
        let mut cur: Vec<Frag> = Vec::new();
        let mut cur_w = 0.0;
        for ch in expanded.chars() {
            if ch.is_control() {
                continue;
            }
            let (face, eff) = ctx.fonts.resolve(Face::Courier, ch);
            let w = ctx.fonts.advance(face, eff, size);
            if !cur.is_empty() && cur_w + w > inner_w + EPS {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
            }
            push_frag(
                &mut cur,
                face,
                eff.encode_utf8(&mut [0u8; 4]),
                w,
                BLACK,
                false,
            );
            cur_w += w;
        }
        lines.push(cur); // keeps blank source lines
    }

    ctx.flush_gap();
    let mut i = 0;
    while i < lines.len() {
        ctx.ensure(2.0 * CODE_PAD + lh);
        let avail = ctx.bottom() - ctx.y - 2.0 * CODE_PAD;
        let fit = (avail / lh).floor();
        let mut k = if fit.is_finite() && fit >= 1.0 {
            fit as usize
        } else {
            1
        };
        k = k.min(lines.len() - i).max(1);

        let chunk_h = k as f64 * lh + 2.0 * CODE_PAD;
        ctx.op(Op::FillRect {
            x: left,
            y: ctx.y,
            w: right - left,
            h: chunk_h,
            color: CODE_BG,
        });
        let mut yy = ctx.y + CODE_PAD;
        for line in &lines[i..i + k] {
            let baseline = yy + size * BASELINE_FACTOR;
            let mut x = left + CODE_PAD;
            for frag in line {
                if !frag.text.is_empty() {
                    ctx.op(Op::Text {
                        face: frag.face,
                        size,
                        color: frag.color,
                        x,
                        baseline,
                        text: frag.text.clone(),
                    });
                }
                x += frag.width;
            }
            yy += lh;
        }
        ctx.y += chunk_h;
        i += k;
        if i < lines.len() {
            ctx.new_page();
        }
    }
}

/// Ordered / unordered / task lists. Markers are drawn in a fixed gutter left
/// of the item content; nesting indents by [`LIST_INDENT`] per level.
fn layout_list(
    ctx: &mut Ctx,
    ordered: bool,
    start: u64,
    items: &[crate::model::ListItem],
    left: f64,
    right: f64,
) {
    let body = ctx.opts.body_font_size;
    let content_left = left + LIST_INDENT;
    for (i, item) in items.iter().enumerate() {
        ctx.flush_gap();
        ctx.ensure(body * LINE_FACTOR);
        let baseline = ctx.y + body * BASELINE_FACTOR;

        if let Some(checked) = item.checkbox {
            let s = body * CHECKBOX_EM;
            let x0 = content_left - LIST_MARKER_GAP - s;
            let y0 = baseline - s;
            ctx.op(Op::StrokeRect {
                x: x0,
                y: y0,
                w: s,
                h: s,
                color: Rgb(0.25, 0.25, 0.25),
                line_width: 0.9,
            });
            if checked {
                let width = (s * 0.14).max(0.7);
                ctx.op(Op::Line {
                    x1: x0 + 0.22 * s,
                    y1: y0 + 0.55 * s,
                    x2: x0 + 0.42 * s,
                    y2: y0 + 0.76 * s,
                    color: BLACK,
                    width,
                });
                ctx.op(Op::Line {
                    x1: x0 + 0.42 * s,
                    y1: y0 + 0.76 * s,
                    x2: x0 + 0.82 * s,
                    y2: y0 + 0.22 * s,
                    color: BLACK,
                    width,
                });
            }
        } else if ordered {
            let marker = format!("{}.", start + i as u64);
            let pref = ctx.fonts.face_for(Style::default());
            let w = measure(ctx.fonts, pref, &marker, body);
            let mut frags = Vec::new();
            for ch in marker.chars() {
                let (face, eff) = ctx.fonts.resolve(pref, ch);
                let cw = ctx.fonts.advance(face, eff, body);
                push_frag(
                    &mut frags,
                    face,
                    eff.encode_utf8(&mut [0u8; 4]),
                    cw,
                    BLACK,
                    false,
                );
            }
            let mut x = content_left - LIST_MARKER_GAP - w;
            for frag in &frags {
                ctx.emit_frag(frag, body, x, baseline);
                x += frag.width;
            }
        } else {
            let r = body * BULLET_RADIUS_EM;
            ctx.op(Op::FillCircle {
                cx: content_left - LIST_MARKER_GAP - r - 1.5,
                cy: baseline - body * 0.30,
                r,
                color: BLACK,
            });
        }

        layout_blocks(ctx, &item.blocks, content_left, right);
        ctx.gap(body * ITEM_GAP_EM);
    }
}

/// Blockquote: children indented with a vertical bar per spanned page segment.
fn layout_quote(ctx: &mut Ctx, children: &[Block], left: f64, right: f64) {
    ctx.flush_gap();
    ctx.ensure(ctx.opts.body_font_size * LINE_FACTOR);
    let start_page = ctx.page;
    let start_y = ctx.y;
    layout_blocks(ctx, children, left + QUOTE_INDENT, right);
    let (end_page, end_y) = (ctx.page, ctx.y);
    let (top, bottom) = (ctx.top(), ctx.bottom());
    for p in start_page..=end_page {
        let y0 = if p == start_page { start_y } else { top };
        let y1 = if p == end_page { end_y } else { bottom };
        if y1 > y0 + EPS {
            ctx.pages[p].ops.push(Op::FillRect {
                x: left,
                y: y0,
                w: QUOTE_BAR_WIDTH,
                h: y1 - y0,
                color: QUOTE_BAR_COLOR,
            });
        }
    }
}

/// Block image, scaled to fit the available width (and the page content
/// height), never upscaled.
fn layout_image(ctx: &mut Ctx, id: usize, left: f64, right: f64) {
    let Some(img) = ctx.images.get(id) else {
        return;
    };
    let (pw, ph) = img.size();
    let (w, h) = (f64::from(pw), f64::from(ph));
    let avail_w = (right - left).max(1.0);
    let content_h = (ctx.bottom() - ctx.top()).max(1.0);
    let scale = (avail_w / w).min(content_h / h).min(1.0);
    let (dw, dh) = (w * scale, h * scale);
    ctx.flush_gap();
    ctx.ensure(dh);
    ctx.op(Op::Image {
        id,
        x: left,
        y: ctx.y,
        w: dw,
        h: dh,
    });
    ctx.y += dh;
}

/// GFM table: content-measured column widths (scaled proportionally when the
/// preferred total overflows), bordered cells, per-cell wrapping, header row
/// bold over a light background. Rows never split across pages; a row taller
/// than a whole page overflows its last page (documented limitation).
fn layout_table(
    ctx: &mut Ctx,
    aligns: &[CellAlign],
    head: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    left: f64,
    right: f64,
) {
    let ncols = aligns.len();
    if ncols == 0 {
        return;
    }
    let body = ctx.opts.body_font_size;
    let lh = body * LINE_FACTOR;
    let avail = (right - left).max(1.0);
    let empty: Vec<Inline> = Vec::new();

    // Preferred column widths from the widest natural cell line.
    let mut pref = vec![MIN_COL_WIDTH; ncols];
    let head_rows: Vec<&[Vec<Inline>]> = if head.is_empty() { vec![] } else { vec![head] };
    for (row, is_head) in head_rows
        .iter()
        .map(|r| (*r, true))
        .chain(rows.iter().map(|r| (r.as_slice(), false)))
    {
        for (c, p) in pref.iter_mut().enumerate().take(ncols) {
            let cell = row.get(c).unwrap_or(&empty);
            let toks = tokens(ctx.fonts, cell, body, is_head);
            *p = p.max(natural_width(&toks) + 2.0 * TABLE_PAD);
        }
    }
    let total: f64 = pref.iter().sum();
    let widths: Vec<f64> = if total > avail {
        // Fair-share shrink: columns whose preference fits their equal share
        // keep it; oversized columns split the remainder evenly. Processing
        // ascending by preference (index-stable) keeps this deterministic.
        let mut order: Vec<usize> = (0..ncols).collect();
        order.sort_by(|&a, &b| {
            pref[a]
                .partial_cmp(&pref[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut widths = vec![0.0; ncols];
        let mut remaining = avail;
        let mut cols_left = ncols;
        for &c in &order {
            let share = remaining / cols_left as f64;
            let w = pref[c].min(share).max(MIN_COL_WIDTH);
            widths[c] = w;
            remaining = (remaining - w).max(0.0);
            cols_left -= 1;
        }
        widths
    } else {
        pref
    };
    let table_w: f64 = widths.iter().sum();

    ctx.flush_gap();
    for (row, is_head) in head_rows
        .iter()
        .map(|r| (*r, true))
        .chain(rows.iter().map(|r| (r.as_slice(), false)))
    {
        // Wrap every cell, derive the row height.
        let mut cell_lines: Vec<Vec<LineOut>> = Vec::with_capacity(ncols);
        let mut nlines = 1usize;
        for (c, w) in widths.iter().enumerate().take(ncols) {
            let cell = row.get(c).unwrap_or(&empty);
            let toks = tokens(ctx.fonts, cell, body, is_head);
            let lines = wrap(ctx.fonts, &toks, w - 2.0 * TABLE_PAD, body);
            nlines = nlines.max(lines.len());
            cell_lines.push(lines);
        }
        let row_h = nlines as f64 * lh + 2.0 * TABLE_PAD;
        ctx.ensure(row_h);
        let y0 = ctx.y;
        if is_head {
            ctx.op(Op::FillRect {
                x: left,
                y: y0,
                w: table_w,
                h: row_h,
                color: TABLE_HEAD_BG,
            });
        }
        let mut x = left;
        for (c, lines) in cell_lines.iter().enumerate() {
            ctx.op(Op::StrokeRect {
                x,
                y: y0,
                w: widths[c],
                h: row_h,
                color: TABLE_BORDER_COLOR,
                line_width: TABLE_BORDER_WIDTH,
            });
            let inner_w = widths[c] - 2.0 * TABLE_PAD;
            let mut yy = y0 + TABLE_PAD;
            for line in lines {
                let baseline = yy + body * BASELINE_FACTOR;
                let mut fx = x + TABLE_PAD + align_offset(aligns[c], inner_w, line.width);
                for frag in &line.frags {
                    ctx.emit_frag(frag, body, fx, baseline);
                    fx += frag.width;
                }
                yy += lh;
            }
            x += widths[c];
        }
        ctx.y += row_h;
    }
}
