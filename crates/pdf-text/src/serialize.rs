//! `get_text` serializers + `TEXTFLAGS` (M2d, PRD §8.6.2, §10.7).
//!
//! Turns a [`TextPage`] (device space, produced by [`crate::layout`]) into every
//! PyMuPDF `get_text` output:
//!
//! - [`to_text`] — plain text (`get_text("text")`);
//! - [`to_blocks`] — `(x0,y0,x1,y1,text,block_no,block_type)` tuples;
//! - [`to_words`] — `(x0,y0,x1,y1,word,block_no,line_no,word_no)` tuples;
//! - [`to_dict`] / [`to_json`] — the structured tree (`dict`/`rawdict` and
//!   `json`/`rawjson`), as a neutral [`TextDict`] that M2e converts to Python;
//! - [`to_html`] / [`to_xhtml`] / [`to_xml`] — fitz-shaped valid markup
//!   (Tier-B, PRD §6.1: own goldens, structurally fitz-shaped but not
//!   PyMuPDF-byte-exact);
//! - [`get_textbox`] — text within a clip rect.
//!
//! The `TEXT_*` flag values and the **per-method default flag sets** match
//! PyMuPDF (Tier-A documented facts, PRD §8.6.2); the flags honored in M2d are
//! `PRESERVE_IMAGES` (include/exclude image blocks), `DEHYPHENATE` (join a
//! line-broken hyphenated word in plain text), and `MEDIABOX_CLIP` (drop glyphs
//! whose origin falls outside the page box). Image blocks carry the real encoded
//! pixel bytes + raster header when the caller supplies an [`ImageResolver`] (via
//! [`to_dict_with_images`] / [`to_json_with_images`]) — the same payload as
//! `Document.extract_image`. The plain [`to_dict`] / [`to_json`] entry points
//! have no document handle, so they emit geometry-only image blocks (empty
//! `image`, as fitz does for an unresolvable image).

use pdf_core::geom::{Matrix, Point, Quad, Rect};

use crate::model::{Block, BlockKind, Char, Line, Span, TextPage};
use crate::words::words;

// === TEXTFLAGS (PRD §8.6.2 — Tier-A documented values) ====================

/// PyMuPDF `TEXT_*` text-extraction flag bits (PRD §8.6.2).
///
/// These integer values are the documented PyMuPDF constants; the per-method
/// default sets below combine them. M2d honors `PRESERVE_IMAGES`, `DEHYPHENATE`
/// and `MEDIABOX_CLIP`; the rest are accepted and reserved (ligature/whitespace
/// preservation is already the layout's behavior; `PRESERVE_SPANS`,
/// `INHIBIT_SPACES`, `CID_FOR_UNKNOWN` land with later milestones).
pub mod textflags {
    /// Keep ligatures as single glyphs (e.g. `ﬁ`) instead of expanding them.
    pub const PRESERVE_LIGATURES: u32 = 1;
    /// Keep the source whitespace instead of normalizing runs to one space.
    pub const PRESERVE_WHITESPACE: u32 = 2;
    /// Include image blocks in the structured output.
    pub const PRESERVE_IMAGES: u32 = 4;
    /// Do not synthesize spaces from spatial gaps.
    pub const INHIBIT_SPACES: u32 = 8;
    /// Join a word split by a hyphen across a line break.
    pub const DEHYPHENATE: u32 = 16;
    /// Keep spans even across small style changes (no span merging).
    pub const PRESERVE_SPANS: u32 = 32;
    /// Drop glyphs whose origin falls outside the page (media) box.
    pub const MEDIABOX_CLIP: u32 = 64;
    /// Emit a CID placeholder for codes with no Unicode mapping.
    pub const CID_FOR_UNKNOWN: u32 = 128;
}

/// Per-method default `TEXTFLAGS` sets, pinned to PyMuPDF (PRD §8.6.2).
///
/// `text`/`blocks`/`words` keep ligatures + whitespace and clip to the media box
/// (images off); `dict`/`rawdict`/`json`/`rawjson` add `PRESERVE_IMAGES`;
/// `html`/`xhtml` match the dict set (images on); `xml` is the char-level dump
/// with ligatures + whitespace + media-box clip.
pub mod defaults {
    use super::textflags::*;

    /// `get_text("text")` default flags = `1|2|64 = 67`.
    pub const TEXT: u32 = PRESERVE_LIGATURES | PRESERVE_WHITESPACE | MEDIABOX_CLIP;
    /// `get_text("blocks")` default flags = `67`.
    pub const BLOCKS: u32 = TEXT;
    /// `get_text("words")` default flags = `67`.
    pub const WORDS: u32 = TEXT;
    /// `get_text("dict")` default flags = `1|2|4|64 = 71`.
    pub const DICT: u32 =
        PRESERVE_LIGATURES | PRESERVE_WHITESPACE | PRESERVE_IMAGES | MEDIABOX_CLIP;
    /// `get_text("rawdict")` default flags = `71`.
    pub const RAWDICT: u32 = DICT;
    /// `get_text("json")` default flags = `71`.
    pub const JSON: u32 = DICT;
    /// `get_text("rawjson")` default flags = `71`.
    pub const RAWJSON: u32 = DICT;
    /// `get_text("html")` default flags = `71`.
    pub const HTML: u32 = DICT;
    /// `get_text("xhtml")` default flags = `71`.
    pub const XHTML: u32 = DICT;
    /// `get_text("xml")` default flags = `1|2|64 = 67`.
    pub const XML: u32 = PRESERVE_LIGATURES | PRESERVE_WHITESPACE | MEDIABOX_CLIP;
}

// === tuple shapes (Tier-A, PRD §8.6.2 / §10.7) ============================

/// A flattened affine matrix `(a, b, c, d, e, f)`, the PDF / PyMuPDF row-vector
/// convention: `(x, y) → (a·x + c·y + e, b·x + d·y + f)`. Same shape as the
/// `dict` image block's `transform` and as `pdfspine.Matrix`.
pub type MatrixTuple = (f64, f64, f64, f64, f64, f64);

/// A flattened quadrilateral `(ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y)`
/// — the corner order of `pdf_core::geom::Quad`, of `search_for(quads=True)`
/// and of the `get_text("xml")` `<char quad=…>` attribute.
pub type QuadTuple = (f64, f64, f64, f64, f64, f64, f64, f64);

/// One `get_text("blocks")` tuple: `(x0, y0, x1, y1, text, block_no, type)`
/// where `type` is `0` (text) or `1` (image).
pub type BlockTuple = (f64, f64, f64, f64, String, i32, i32);

/// One `get_text("words")` tuple:
/// `(x0, y0, x1, y1, word, block_no, line_no, word_no)`.
pub type WordTuple = (f64, f64, f64, f64, String, i32, i32, i32);

// === structured tree (neutral; M2e converts to Python) ====================

/// The neutral `dict`/`rawdict` tree (PyMuPDF structured-text shape, PRD §10.7).
///
/// Built by [`to_dict`]; M2e converts this 1:1 into the Python dict PyMuPDF
/// returns. Key names / nesting / types mirror PyMuPDF exactly: tuples are
/// `(f64, …)`, `color` is an `i32`, span carries `text` (dict) **or** `chars`
/// (rawdict).
#[derive(Clone, Debug, PartialEq)]
pub struct TextDict {
    /// Page width in device space.
    pub width: f64,
    /// Page height in device space.
    pub height: f64,
    /// The page's blocks in reading order.
    pub blocks: Vec<DictBlock>,
}

/// A `dict` block: a text block (`type 0`) or an image block (`type 1`).
#[derive(Clone, Debug, PartialEq)]
pub enum DictBlock {
    /// A text block carrying lines.
    Text(DictTextBlock),
    /// An image block carrying placement + encoded pixel bytes/metadata.
    Image(DictImageBlock),
}

/// A `dict` text block (`type` 0).
#[derive(Clone, Debug, PartialEq)]
pub struct DictTextBlock {
    /// The reading-order block number.
    pub number: i32,
    /// The block bounding box `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// Painting order: the smallest source-glyph index among the block's
    /// glyphs. Orders blocks the way the content stream painted them, which is
    /// **not** in general the reading order [`Self::number`] carries.
    pub seq: usize,
    /// The block's lines.
    pub lines: Vec<DictLine>,
}

/// A `dict` line.
#[derive(Clone, Debug, PartialEq)]
pub struct DictLine {
    /// The line bounding box `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// Writing mode: 0 horizontal, 1 vertical.
    pub wmode: i32,
    /// Writing-direction unit vector `(cos, sin)`.
    pub dir: (f64, f64),
    /// The line's index within the page in reading order — the order the lines
    /// leave the engine, i.e. the order their text appears in `get_text("text")`.
    pub number: usize,
    /// Painting order: the smallest source-glyph index among the line's glyphs.
    pub seq: usize,
    /// The line's spans.
    pub spans: Vec<DictSpan>,
}

/// A `dict`/`rawdict` span. Carries `text` in dict mode and `chars` in rawdict
/// mode; the other collection is empty (M2e picks the right field per mode).
#[derive(Clone, Debug, PartialEq)]
pub struct DictSpan {
    /// The font size.
    pub size: f64,
    /// The span-flag bitfield.
    pub flags: i32,
    /// The font name.
    pub font: String,
    /// The fill color as a packed sRGB integer.
    pub color: i32,
    /// The font ascender (unit font size).
    pub ascender: f64,
    /// The font descender (unit font size).
    pub descender: f64,
    /// The span origin `(x, y)` (baseline left).
    pub origin: (f64, f64),
    /// The span bounding box `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// The **declared** font size — the `Tf` operand verbatim, the same value
    /// as [`Self::size`] under an explicit name.
    pub declared_size: f64,
    /// The **rendered** font size, `sqrt(|det|)` of [`Self::matrix`]'s linear
    /// part (MuPDF's `fz_matrix_expansion`). Differs from [`Self::size`]
    /// whenever the scale lives in `Tm` / `cm` / `Tz` rather than in `Tf`.
    pub rendered_size: f64,
    /// The span's render matrix in **device space** (first glyph):
    /// `(0,0)·matrix == origin`.
    pub matrix: MatrixTuple,
    /// The first glyph's text matrix `Tm` — the raw content-stream value in
    /// **PDF user space**, with no page transform folded in.
    pub text_matrix: MatrixTuple,
    /// The first glyph's CTM — likewise the raw **user-space** value.
    pub ctm: MatrixTuple,
    /// The span's baseline direction as a device-space unit vector.
    pub dir: (f64, f64),
    /// The span's directional envelope in device space: the extent of its glyph
    /// quads along [`Self::dir`] and its normal. Equals the `bbox` corners for
    /// upright text; hugs the run for rotated or sheared text.
    pub quad: QuadTuple,
    /// Painting order: the smallest source-glyph index among the span's glyphs.
    pub seq: usize,
    /// The span text (dict mode); empty in rawdict mode.
    pub text: String,
    /// The per-character detail (rawdict mode); empty in dict mode.
    pub chars: Vec<DictChar>,
}

/// A `rawdict` char.
#[derive(Clone, Debug, PartialEq)]
pub struct DictChar {
    /// The glyph origin `(x, y)`.
    pub origin: (f64, f64),
    /// The glyph bounding box `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// The Unicode scalar (single-char string).
    pub c: String,
    /// The glyph's render matrix in **device space**: `(0,0)·matrix == origin`.
    pub matrix: MatrixTuple,
    /// The glyph cell's four true corners in **device space**. `bbox` is this
    /// quad's axis-aligned envelope; the quad keeps the parallelogram a rotated
    /// or sheared `Tm` produces.
    pub quad: QuadTuple,
    /// The **rendered** font size, `sqrt(|det|)` of [`Self::matrix`].
    pub rendered_size: f64,
    /// Painting order: the source glyph's index in the interpreter output. A
    /// synthesized space borrows the preceding real glyph's index, so `seq`
    /// stays non-decreasing along a span.
    pub seq: usize,
    /// `true` when layout synthesized this char (an inter-word space the
    /// content stream never painted); `false` for every glyph the PDF drew.
    pub synthetic: bool,
}

/// A `dict` image block (`type` 1). When a caller supplies an [`ImageResolver`],
/// `image` carries the encoded bytes and the raster header (`ext`, `colorspace`,
/// `bpc`, `xres`/`yres`, `size`) is populated — the same payload as
/// `Document.extract_image`. Without a resolver the block is geometry-only
/// (`image` empty, header zero). All keys are always present so the Python shape
/// is stable (PRD §10.7).
#[derive(Clone, Debug, PartialEq)]
pub struct DictImageBlock {
    /// The reading-order block number.
    pub number: i32,
    /// The image bounding box `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// Pixel width (0 when unknown).
    pub width: i32,
    /// Pixel height (0 when unknown).
    pub height: i32,
    /// Image extension / codec hint (e.g. `"png"`); empty when unresolved.
    pub ext: String,
    /// Colorspace component count (fitz convention: channel count); 0 when
    /// unresolved.
    pub colorspace: i32,
    /// Horizontal resolution in DPI (0 when unresolved).
    pub xres: i32,
    /// Vertical resolution in DPI (0 when unresolved).
    pub yres: i32,
    /// Bits per component (0 when unresolved).
    pub bpc: i32,
    /// The image-placement matrix `(a, b, c, d, e, f)` (device space).
    pub transform: (f64, f64, f64, f64, f64, f64),
    /// Encoded byte size (`image.len()`; 0 when unresolved).
    pub size: i32,
    /// The encoded image bytes (empty when unresolved).
    pub image: Vec<u8>,
}

/// The encoded payload + raster header for one `dict`/`json` image block,
/// resolved by the caller which holds the document (see [`ImageResolver`]). The
/// fields map onto `Document.extract_image`: `ext`/`image`/`bpc`/`width`/`height`
/// are that descriptor verbatim, `colorspace` is its component count.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedImage {
    /// Codec token (`"png"`/`"jpeg"`/`"jpx"`/`"jb2"`/`"ccitt"`), extract-image-sourced.
    pub ext: String,
    /// Colorspace component count (fitz's `colorspace` is the channel count).
    pub colorspace: i32,
    /// Bits per component.
    pub bpc: i32,
    /// Pixel width (from the image XObject's `/Width`).
    pub width: i32,
    /// Pixel height (from the image XObject's `/Height`).
    pub height: i32,
    /// Horizontal resolution in DPI.
    pub xres: i32,
    /// Vertical resolution in DPI.
    pub yres: i32,
    /// The encoded image bytes (same payload as `Document.extract_image`).
    pub image: Vec<u8>,
}

/// Resolves a `dict`/`json` image block's real encoded bytes + raster header.
///
/// Implemented by the caller (`pdf-api`), which holds the document and can map a
/// block's XObject resource name to its bytes. Pure-model callers pass no
/// resolver and get the geometry-only stub. `name` is `None` for an inline image
/// (no resource name), for which the resolver should return `None`.
pub trait ImageResolver {
    /// Resolves the image referenced under XObject resource `name`, or `None`
    /// when it can't be resolved (inline / missing).
    fn resolve(&self, name: Option<&str>) -> Option<ResolvedImage>;
}

// === plain text (PRD §8.6) ================================================

/// Serializes a [`TextPage`] to plain text (`get_text("text")`).
///
/// Words on a line are already separated by spaces in the span text; lines are
/// joined by `\n`, and each block ends with a blank line (PyMuPDF block
/// separation). `DEHYPHENATE` joins a word split by a trailing `-` across a line
/// break; otherwise hyphens are kept verbatim (PyMuPDF default).
#[must_use]
pub fn to_text(tp: &TextPage, flags: u32) -> String {
    let dehyphenate = flags & textflags::DEHYPHENATE != 0;
    let mut out = String::new();
    for block in &tp.blocks {
        if block.kind != BlockKind::Text {
            continue;
        }
        out.push_str(&block_text(block, dehyphenate));
        out.push('\n');
    }
    out
}

/// The text of a single text block: its lines joined with `\n` and a trailing
/// `\n`. With `dehyphenate`, a line ending in `-` is glued to the next line's
/// first word (the hyphen removed).
fn block_text(block: &Block, dehyphenate: bool) -> String {
    let mut out = String::new();
    let n = block.lines.len();
    for (i, line) in block.lines.iter().enumerate() {
        let text = line_text(line);
        if dehyphenate && i + 1 < n && text.ends_with('-') {
            // Drop the trailing hyphen; the next line continues without `\n`.
            out.push_str(text.trim_end_matches('-'));
        } else {
            out.push_str(&text);
            if i + 1 < n {
                out.push('\n');
            }
        }
    }
    out
}

/// The text of one line: its spans concatenated (spans already carry the inter-
/// word spaces from layout).
fn line_text(line: &Line) -> String {
    let mut s = String::new();
    for span in &line.spans {
        s.push_str(&span.text);
    }
    s
}

// === get_textbox (PRD §8.6.2) ============================================

/// Returns the text within `clip` (PyMuPDF `get_textbox`).
///
/// fitz clips **per character**: a char is kept when its bbox overlaps `clip` in
/// both X and Y (strict overlap — a char merely touching a clip edge is out). A
/// line that contributes at least one char yields its kept chars in order; the
/// surviving lines are `\n`-joined. So a clip narrower than a line trims that
/// line's head/tail rather than including or dropping the whole line.
#[must_use]
pub fn get_textbox(tp: &TextPage, clip: Rect) -> String {
    let clip = clip.normalize();
    let mut lines: Vec<String> = Vec::new();
    for block in &tp.blocks {
        if block.kind != BlockKind::Text {
            continue;
        }
        for line in &block.lines {
            let mut kept = String::new();
            for span in &line.spans {
                for ch in &span.chars {
                    if char_overlaps_clip(&ch.bbox.normalize(), &clip) {
                        kept.push(ch.c);
                    }
                }
            }
            if !kept.is_empty() {
                lines.push(kept);
            }
        }
    }
    lines.join("\n")
}

/// Strict bbox overlap of a char against the clip rect (fitz's textbox clip: a
/// char whose bbox only touches a clip edge is excluded).
fn char_overlaps_clip(c: &Rect, clip: &Rect) -> bool {
    c.x0 < clip.x1 && c.x1 > clip.x0 && c.y0 < clip.y1 && c.y1 > clip.y0
}

// === extract_selection (PRD §8.6.2) ======================================

/// One flattened selection char: its bbox + a running `(block, line)` identity,
/// plus the line's baseline (char origin Y) and font size for line resolution.
struct SelChar<'a> {
    c: &'a crate::model::Char,
    line_id: (usize, usize),
    /// The char's baseline Y (device space) — identical between pdfspine and fitz,
    /// unlike the bbox Y (pdfspine uses a tighter glyph box).
    baseline: f64,
    /// The owning span's font size (em), for the baseline-relative line band.
    size: f64,
}

/// Returns the text between two device-space points `a` and `b`, as if dragged
/// with the mouse (PyMuPDF `extractSelection`).
///
/// Chars are flattened in reading order (block → line → char, text blocks only).
/// Each point is resolved to a line by fitz's **baseline-relative line box**
/// (see [`point_line`]): the start point picks the first line, the end point the
/// last line, the point falls within. Within the start/end lines a point picks a
/// char by horizontal position; the inclusive char range `[lo ..= hi]` is emitted
/// with a `\n` at every line boundary (matching MuPDF's selection text). Because
/// the line band is baseline-relative (not bbox-relative), the rule reproduces
/// fitz's behavior despite pdfspine's tighter glyph box.
#[must_use]
pub fn extract_selection(tp: &TextPage, a: Point, b: Point) -> String {
    // Flatten all text chars in reading order.
    let mut chars: Vec<SelChar> = Vec::new();
    for (bi, block) in tp.blocks.iter().enumerate() {
        if block.kind != BlockKind::Text {
            continue;
        }
        for (li, line) in block.lines.iter().enumerate() {
            for span in &line.spans {
                for c in &span.chars {
                    chars.push(SelChar {
                        c,
                        line_id: (bi, li),
                        baseline: c.origin.y,
                        size: span.size,
                    });
                }
            }
        }
    }
    if chars.is_empty() {
        return String::new();
    }

    let start = char_index_for_point(&chars, a, true);
    let end = char_index_for_point(&chars, b, false);
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };

    let mut out = String::new();
    let mut prev_line: Option<(usize, usize)> = None;
    for sc in &chars[lo..=hi] {
        if let Some(pl) = prev_line {
            if pl != sc.line_id {
                out.push('\n');
            }
        }
        out.push(sc.c.c);
        prev_line = Some(sc.line_id);
    }
    out
}

/// The line a selection point falls in, by fitz's baseline-relative line box.
///
/// fitz's selection treats each line as a band `[baseline − 0.875·size,
/// baseline + 0.125·size]` (its full-em line box around the baseline; the two
/// fractions sum to 1 em). A `start` point picks the **first** line whose band
/// bottom is at/after it (`p.y ≤ baseline + 0.125·size`); an `end` point picks
/// the **last** line whose band top is at/before it (`p.y ≥ baseline − 0.875·
/// size`). Points above all text resolve to the first line; below all text to
/// the last (so an end point past the final line keeps that line in full).
fn point_line(chars: &[SelChar], p: Point, is_start: bool) -> (usize, usize) {
    // Lines in reading order, each with its baseline + max size.
    let mut lines: Vec<((usize, usize), f64, f64)> = Vec::new();
    for sc in chars {
        match lines.last_mut() {
            Some((id, _, size)) if *id == sc.line_id => {
                *size = size.max(sc.size);
            }
            _ => lines.push((sc.line_id, sc.baseline, sc.size)),
        }
    }

    if is_start {
        // First line whose band bottom is at/after the point.
        for (id, baseline, size) in &lines {
            if p.y <= baseline + 0.125 * size {
                return *id;
            }
        }
        // Below every line → the last line.
        lines.last().map(|l| l.0).unwrap()
    } else {
        // Last line whose band top is at/before the point.
        let mut chosen = lines.first().map(|l| l.0).unwrap();
        for (id, baseline, size) in &lines {
            if p.y >= baseline - 0.875 * size {
                chosen = *id;
            }
        }
        chosen
    }
}

/// Maps a point to a char index for selection: resolves the point's line via
/// [`point_line`], then within that line picks the char by position.
///
/// fitz snaps to the line edge when the point is vertically outside the line's
/// box (`[baseline − 0.875·size, baseline + 0.125·size]`): a `start` point ABOVE
/// the line selects from its first char (whole line head), an `end` point BELOW
/// the line selects to its last char (whole line tail). When the point is within
/// the line's vertical box, the char is chosen by horizontal position relative to
/// char centers — `start` = first char whose center is at/after the point, `end`
/// = last char whose center is at/before it — so a mid-glyph drag includes the
/// expected text. Points past a line's horizontal extent clamp to its end / start.
fn char_index_for_point(chars: &[SelChar], p: Point, is_start: bool) -> usize {
    let line = point_line(chars, p, is_start);

    // The resolved line's baseline + max font size → its vertical box. The
    // baseline is shared across the line; the size is the largest span size.
    let (baseline, size) = chars
        .iter()
        .filter(|sc| sc.line_id == line)
        .fold((0.0_f64, 0.0_f64), |(_, sz), sc| {
            (sc.baseline, sz.max(sc.size))
        });
    let line_top = baseline - 0.875 * size;
    let line_bottom = baseline + 0.125 * size;

    // First / last char index on the resolved line.
    let first_on_line = chars.iter().position(|sc| sc.line_id == line).unwrap();
    let last_on_line = chars.iter().rposition(|sc| sc.line_id == line).unwrap();

    // Vertically outside the line box → snap to the line edge.
    if is_start && p.y < line_top {
        return first_on_line;
    }
    if !is_start && p.y > line_bottom {
        return last_on_line;
    }

    // Within the box: choose by horizontal position relative to char centers.
    let mut idx = 0usize;
    let mut found_any = false;
    for (i, sc) in chars.iter().enumerate() {
        if sc.line_id != line {
            continue;
        }
        let bb = sc.c.bbox.normalize();
        let center = (bb.x0 + bb.x1) / 2.0;
        if is_start {
            // Start: first char whose center is at/after the point.
            if p.x <= center {
                idx = i;
                found_any = true;
                break;
            }
        } else {
            // End: last char whose center is at/before the point.
            if center <= p.x {
                idx = i;
                found_any = true;
            }
        }
    }
    if !found_any {
        // Start point past the line end → its last char; end point before the
        // line start → its first char.
        idx = if is_start {
            last_on_line
        } else {
            first_on_line
        };
    }
    idx
}

// === blocks (PRD §8.6.2) =================================================

/// Serializes a [`TextPage`] to `get_text("blocks")` tuples. Image blocks are
/// included (type 1) only when `PRESERVE_IMAGES` is set.
#[must_use]
pub fn to_blocks(tp: &TextPage, flags: u32) -> Vec<BlockTuple> {
    let images = flags & textflags::PRESERVE_IMAGES != 0;
    let dehyphenate = flags & textflags::DEHYPHENATE != 0;
    let mut out = Vec::new();
    for block in &tp.blocks {
        let b = block.bbox.normalize();
        match block.kind {
            BlockKind::Text => {
                // Block text ends with a trailing `\n` (PyMuPDF blocks shape).
                let mut text = block_text(block, dehyphenate);
                text.push('\n');
                out.push((b.x0, b.y0, b.x1, b.y1, text, block.number as i32, 0));
            }
            BlockKind::Image => {
                if images {
                    let text = image_block_marker(block);
                    out.push((b.x0, b.y0, b.x1, b.y1, text, block.number as i32, 1));
                }
            }
        }
    }
    out
}

/// A textual marker for an image block in `blocks` output (PyMuPDF emits a short
/// `<image: …>` descriptor; ours notes the placeholder dimensions).
fn image_block_marker(block: &Block) -> String {
    let (w, h) = block
        .image
        .as_ref()
        .map(|i| (i.width.unwrap_or(0), i.height.unwrap_or(0)))
        .unwrap_or((0, 0));
    format!("<image: {w}x{h}>\n")
}

// === words (PRD §10.7) ===================================================

/// Serializes a [`TextPage`] to `get_text("words")` tuples. Image blocks
/// contribute no words.
#[must_use]
pub fn to_words(tp: &TextPage, _flags: u32) -> Vec<WordTuple> {
    words(tp)
        .into_iter()
        .map(|w| {
            let b = w.bbox.normalize();
            (
                b.x0,
                b.y0,
                b.x1,
                b.y1,
                w.text,
                w.block_no as i32,
                w.line_no as i32,
                w.word_no as i32,
            )
        })
        .collect()
}

// === dict / rawdict (PRD §10.7) ==========================================

/// Builds the structured [`TextDict`] (PyMuPDF `dict` when `raw == false`,
/// `rawdict` when `raw == true`). Image blocks are included only when
/// `PRESERVE_IMAGES` is set, and are geometry-only (empty `image`) — use
/// [`to_dict_with_images`] to fill their real encoded bytes.
#[must_use]
pub fn to_dict(tp: &TextPage, raw: bool, flags: u32) -> TextDict {
    to_dict_impl(tp, raw, flags, None)
}

/// Like [`to_dict`] but resolves each image block's real encoded bytes + raster
/// header through `resolver` (implemented by the caller, which holds the
/// document). Used by the `dict`/`rawdict` `get_text` paths.
#[must_use]
pub fn to_dict_with_images(
    tp: &TextPage,
    raw: bool,
    flags: u32,
    resolver: &dyn ImageResolver,
) -> TextDict {
    to_dict_impl(tp, raw, flags, Some(resolver))
}

fn to_dict_impl(
    tp: &TextPage,
    raw: bool,
    flags: u32,
    resolver: Option<&dyn ImageResolver>,
) -> TextDict {
    let images = flags & textflags::PRESERVE_IMAGES != 0;
    let mut blocks = Vec::new();
    for block in &tp.blocks {
        match block.kind {
            BlockKind::Text => blocks.push(DictBlock::Text(text_block(block, raw))),
            BlockKind::Image => {
                if images {
                    blocks.push(DictBlock::Image(image_block(block, resolver)));
                }
            }
        }
    }
    TextDict {
        width: tp.width,
        height: tp.height,
        blocks,
    }
}

fn text_block(block: &Block, raw: bool) -> DictTextBlock {
    let lines = block.lines.iter().map(|l| dict_line(l, raw)).collect();
    DictTextBlock {
        number: block.number as i32,
        bbox: rect_tuple(block.bbox),
        seq: block.seq,
        lines,
    }
}

fn dict_line(line: &Line, raw: bool) -> DictLine {
    let spans = line.spans.iter().map(|s| dict_span(s, raw)).collect();
    DictLine {
        bbox: rect_tuple(line.bbox),
        wmode: line.wmode as i32,
        dir: line.dir,
        number: line.number,
        seq: line.seq,
        spans,
    }
}

fn dict_span(span: &Span, raw: bool) -> DictSpan {
    let (text, chars) = if raw {
        (String::new(), span.chars.iter().map(dict_char).collect())
    } else {
        (span.text.clone(), Vec::new())
    };
    DictSpan {
        size: span.size,
        flags: span.flags as i32,
        font: span.font.to_string(),
        color: span.color as i32,
        ascender: span.ascender,
        descender: span.descender,
        origin: point_tuple(span.origin),
        bbox: rect_tuple(span.bbox),
        declared_size: span.size,
        rendered_size: span.rendered_size,
        matrix: matrix_tuple(&span.matrix),
        text_matrix: matrix_tuple(&span.text_matrix),
        ctm: matrix_tuple(&span.ctm),
        dir: span.dir,
        quad: quad_tuple(&span.quad),
        seq: span.seq,
        text,
        chars,
    }
}

fn dict_char(c: &Char) -> DictChar {
    DictChar {
        origin: point_tuple(c.origin),
        bbox: rect_tuple(c.bbox),
        c: c.c.to_string(),
        matrix: matrix_tuple(&c.matrix),
        quad: quad_tuple(&c.quad),
        rendered_size: c.rendered_size,
        seq: c.seq,
        synthetic: c.synthetic,
    }
}

/// Builds an image block. With a `resolver`, the block's XObject resource name is
/// looked up to fetch the real encoded bytes + raster header (same payload as
/// `Document.extract_image`); otherwise (no document handle, or an inline image
/// with no name) the block is geometry-only with an empty `image`, matching
/// fitz for an unresolvable image.
fn image_block(block: &Block, resolver: Option<&dyn ImageResolver>) -> DictImageBlock {
    let img = block.image.as_ref();
    let b = block.bbox.normalize();
    // 内联图无资源名（name = None）→ 无法回查，落到几何占位分支。
    let name = img.and_then(|i| i.name.as_deref());
    let resolved = resolver.and_then(|r| r.resolve(name));

    // 内容流声明的像素尺寸，作为回退（解析成功时以图像 XObject 为准）。
    let decl_w = img.and_then(|i| i.width).unwrap_or(0) as i32;
    let decl_h = img.and_then(|i| i.height).unwrap_or(0) as i32;

    let (width, height, ext, colorspace, xres, yres, bpc, image) = match resolved {
        Some(r) => (
            if r.width > 0 { r.width } else { decl_w },
            if r.height > 0 { r.height } else { decl_h },
            r.ext,
            r.colorspace,
            r.xres,
            r.yres,
            r.bpc,
            r.image,
        ),
        None => (decl_w, decl_h, String::new(), 0, 0, 0, 0, Vec::new()),
    };
    let size = image.len() as i32;

    DictImageBlock {
        number: block.number as i32,
        bbox: rect_tuple(block.bbox),
        width,
        height,
        ext,
        colorspace,
        xres,
        yres,
        bpc,
        // Placement matrix maps the unit square to the block bbox (device space).
        transform: (b.x1 - b.x0, 0.0, 0.0, b.y1 - b.y0, b.x0, b.y0),
        size,
        image,
    }
}

// === json / rawjson (PRD §8.6.2) =========================================

/// Serializes a [`TextPage`] to the PyMuPDF `json` (`raw == false`) / `rawjson`
/// (`raw == true`) string. Same structure as [`to_dict`]; tuples become arrays,
/// image bytes become a base64 string. Key order is fixed (deterministic).
#[must_use]
pub fn to_json(tp: &TextPage, raw: bool, flags: u32) -> String {
    to_json_impl(tp, raw, flags, None)
}

/// Like [`to_json`] but resolves each image block's real encoded bytes through
/// `resolver` (the `json`/`rawjson` `get_text` paths); the `image` field then
/// carries real base64 instead of an empty string.
#[must_use]
pub fn to_json_with_images(
    tp: &TextPage,
    raw: bool,
    flags: u32,
    resolver: &dyn ImageResolver,
) -> String {
    to_json_impl(tp, raw, flags, Some(resolver))
}

fn to_json_impl(
    tp: &TextPage,
    raw: bool,
    flags: u32,
    resolver: Option<&dyn ImageResolver>,
) -> String {
    let dict = to_dict_impl(tp, raw, flags, resolver);
    let mut s = String::new();
    s.push('{');
    json_kv_num(&mut s, "width", dict.width);
    s.push(',');
    json_kv_num(&mut s, "height", dict.height);
    s.push_str(",\"blocks\":[");
    for (i, block) in dict.blocks.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        json_block(&mut s, block, raw);
    }
    s.push_str("]}");
    s
}

fn json_block(s: &mut String, block: &DictBlock, raw: bool) {
    match block {
        DictBlock::Text(b) => {
            s.push_str("{\"type\":0");
            json_comma_bbox(s, "bbox", b.bbox);
            s.push_str(",\"number\":");
            s.push_str(&b.number.to_string());
            json_comma_raw(s, "seq", &b.seq.to_string());
            s.push_str(",\"lines\":[");
            for (i, line) in b.lines.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                json_line(s, line, raw);
            }
            s.push_str("]}");
        }
        DictBlock::Image(b) => {
            s.push_str("{\"type\":1");
            json_comma_bbox(s, "bbox", b.bbox);
            s.push_str(",\"number\":");
            s.push_str(&b.number.to_string());
            s.push_str(",\"width\":");
            s.push_str(&b.width.to_string());
            s.push_str(",\"height\":");
            s.push_str(&b.height.to_string());
            s.push_str(",\"ext\":");
            json_str(s, &b.ext);
            s.push_str(",\"colorspace\":");
            s.push_str(&b.colorspace.to_string());
            s.push_str(",\"xres\":");
            s.push_str(&b.xres.to_string());
            s.push_str(",\"yres\":");
            s.push_str(&b.yres.to_string());
            s.push_str(",\"bpc\":");
            s.push_str(&b.bpc.to_string());
            s.push_str(",\"transform\":[");
            let t = b.transform;
            for (i, v) in [t.0, t.1, t.2, t.3, t.4, t.5].into_iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&fmt_num(v));
            }
            s.push(']');
            s.push_str(",\"size\":");
            s.push_str(&b.size.to_string());
            s.push_str(",\"image\":");
            json_str(s, &base64_encode(&b.image));
            s.push('}');
        }
    }
}

fn json_line(s: &mut String, line: &DictLine, raw: bool) {
    s.push_str("{\"wmode\":");
    s.push_str(&line.wmode.to_string());
    s.push_str(",\"dir\":[");
    s.push_str(&fmt_num(line.dir.0));
    s.push(',');
    s.push_str(&fmt_num(line.dir.1));
    s.push(']');
    json_comma_bbox(s, "bbox", line.bbox);
    json_comma_raw(s, "number", &line.number.to_string());
    json_comma_raw(s, "seq", &line.seq.to_string());
    s.push_str(",\"spans\":[");
    for (i, span) in line.spans.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        json_span(s, span, raw);
    }
    s.push_str("]}");
}

fn json_span(s: &mut String, span: &DictSpan, raw: bool) {
    s.push_str("{\"size\":");
    s.push_str(&fmt_num(span.size));
    s.push_str(",\"flags\":");
    s.push_str(&span.flags.to_string());
    s.push_str(",\"font\":");
    json_str(s, &span.font);
    s.push_str(",\"color\":");
    s.push_str(&span.color.to_string());
    s.push_str(",\"ascender\":");
    s.push_str(&fmt_num(span.ascender));
    s.push_str(",\"descender\":");
    s.push_str(&fmt_num(span.descender));
    s.push_str(",\"origin\":[");
    s.push_str(&fmt_num(span.origin.0));
    s.push(',');
    s.push_str(&fmt_num(span.origin.1));
    s.push(']');
    json_comma_bbox(s, "bbox", span.bbox);
    json_comma_raw(s, "declared_size", &fmt_num(span.declared_size));
    json_comma_raw(s, "rendered_size", &fmt_num(span.rendered_size));
    json_comma_matrix(s, "matrix", span.matrix);
    json_comma_matrix(s, "text_matrix", span.text_matrix);
    json_comma_matrix(s, "ctm", span.ctm);
    json_comma_nums(s, "dir", &[span.dir.0, span.dir.1]);
    json_comma_quad(s, "quad", span.quad);
    json_comma_raw(s, "seq", &span.seq.to_string());
    if raw {
        s.push_str(",\"chars\":[");
        for (i, ch) in span.chars.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"origin\":[");
            s.push_str(&fmt_num(ch.origin.0));
            s.push(',');
            s.push_str(&fmt_num(ch.origin.1));
            s.push(']');
            json_comma_bbox(s, "bbox", ch.bbox);
            s.push_str(",\"c\":");
            json_str(s, &ch.c);
            json_comma_matrix(s, "matrix", ch.matrix);
            json_comma_quad(s, "quad", ch.quad);
            json_comma_raw(s, "rendered_size", &fmt_num(ch.rendered_size));
            json_comma_raw(s, "seq", &ch.seq.to_string());
            json_comma_raw(s, "synthetic", if ch.synthetic { "true" } else { "false" });
            s.push('}');
        }
        s.push(']');
    } else {
        s.push_str(",\"text\":");
        json_str(s, &span.text);
    }
    s.push('}');
}

fn json_comma_bbox(s: &mut String, key: &str, bbox: (f64, f64, f64, f64)) {
    s.push_str(",\"");
    s.push_str(key);
    s.push_str("\":[");
    s.push_str(&fmt_num(bbox.0));
    s.push(',');
    s.push_str(&fmt_num(bbox.1));
    s.push(',');
    s.push_str(&fmt_num(bbox.2));
    s.push(',');
    s.push_str(&fmt_num(bbox.3));
    s.push(']');
}

/// Writes `,"key":[v, …]` with the shared number formatting.
fn json_comma_nums(s: &mut String, key: &str, vals: &[f64]) {
    s.push_str(",\"");
    s.push_str(key);
    s.push_str("\":[");
    for (i, v) in vals.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&fmt_num(*v));
    }
    s.push(']');
}

/// Writes `,"key":[a,b,c,d,e,f]`.
fn json_comma_matrix(s: &mut String, key: &str, m: MatrixTuple) {
    json_comma_nums(s, key, &[m.0, m.1, m.2, m.3, m.4, m.5]);
}

/// Writes `,"key":[ul.x,ul.y,ur.x,ur.y,ll.x,ll.y,lr.x,lr.y]`.
fn json_comma_quad(s: &mut String, key: &str, q: QuadTuple) {
    json_comma_nums(s, key, &[q.0, q.1, q.2, q.3, q.4, q.5, q.6, q.7]);
}

/// Writes `,"key":<already-rendered JSON literal>`.
fn json_comma_raw(s: &mut String, key: &str, lit: &str) {
    s.push_str(",\"");
    s.push_str(key);
    s.push_str("\":");
    s.push_str(lit);
}

fn json_kv_num(s: &mut String, key: &str, v: f64) {
    s.push('"');
    s.push_str(key);
    s.push_str("\":");
    s.push_str(&fmt_num(v));
}

/// Writes a JSON string literal with the required escapes.
fn json_str(s: &mut String, raw: &str) {
    s.push('"');
    for c in raw.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                s.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

/// Formats an `f64` for JSON: integral values stay integral-looking, otherwise
/// trimmed decimal. Non-finite values become `0` (JSON has no NaN/Inf).
fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let mut s = format!("{v:.6}");
        // Trim trailing zeros but keep at least one decimal digit.
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.push('0');
        }
        s
    }
}

// === base64 (no external dep; standard alphabet) =========================

/// Standard-alphabet base64 (RFC 4648) with `=` padding. Used for the (stubbed)
/// image bytes in `json`/`rawjson`.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18 & 0x3F) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6 & 0x3F) as usize] as char);
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

// === html / xhtml / xml (pdfspine-defined; Tier-B, PRD §6.1) ===============

/// Serializes a [`TextPage`] to **HTML** (`get_text("html")`), shaped to match
/// PyMuPDF's `extractHTML`.
///
/// The page is a single `<div id="page0" style="width:…pt;height:…pt">`; each
/// text *line* is an absolutely-positioned `<p style="top:…pt;left:…pt;
/// line-height:…pt">`; each span a `<span style="font-family:…;font-size:…pt;
/// color:#rrggbb">` (bold/italic added when flagged). Image blocks become an
/// `<img>` with the placement geometry (no data-URI; bytes deferred to M5).
///
/// Not PyMuPDF-byte-exact (deviations: line-`<p>` rather than block-`<p>`; no
/// MuPDF heading promotion; CSS `font-family` is the raw PDF font name; image
/// `<img>` has no `src` data URI), but structurally fitz-shaped, valid, and
/// carrying all text + geometry. Validated against our own goldens (Tier-B).
#[must_use]
pub fn to_html(tp: &TextPage, _flags: u32) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "<div id=\"page0\" style=\"width:{}pt;height:{}pt\">\n",
        fmt_num(tp.width),
        fmt_num(tp.height)
    ));
    for block in &tp.blocks {
        match block.kind {
            BlockKind::Text => html_text_block(&mut s, block, false),
            BlockKind::Image => html_image_block(&mut s, block),
        }
    }
    s.push_str("</div>\n");
    s
}

/// Serializes a [`TextPage`] to **XHTML** (`get_text("xhtml")`), shaped to match
/// PyMuPDF's `extractXHTML`: semantic, reflowable, well-formed markup. The page
/// is `<div id="page0">`; each text line a `<p>` of styled `<span>`s, without
/// absolute positioning. Tier-B; same documented deviations as [`to_html`].
#[must_use]
pub fn to_xhtml(tp: &TextPage, _flags: u32) -> String {
    let mut s = String::new();
    s.push_str("<div id=\"page0\">\n");
    for block in &tp.blocks {
        match block.kind {
            BlockKind::Text => html_text_block(&mut s, block, true),
            BlockKind::Image => html_image_block(&mut s, block),
        }
    }
    s.push_str("</div>\n");
    s
}

fn html_text_block(s: &mut String, block: &Block, semantic: bool) {
    for line in &block.lines {
        let lb = line.bbox.normalize();
        // Line height ≈ the largest span size on the line.
        let lh = line.spans.iter().map(|sp| sp.size).fold(0.0_f64, f64::max);
        if semantic {
            s.push_str("<p>");
        } else {
            s.push_str(&format!(
                "<p style=\"top:{}pt;left:{}pt;line-height:{}pt\">",
                fmt_num(lb.y0),
                fmt_num(lb.x0),
                fmt_num(lh)
            ));
        }
        for span in &line.spans {
            html_span(s, span);
        }
        s.push_str("</p>\n");
    }
}

fn html_span(s: &mut String, span: &Span) {
    let mut style = format!(
        "font-family:{};font-size:{}pt;color:#{:06x}",
        css_font(&span.font),
        fmt_num(span.size),
        span.color & 0x00FF_FFFF
    );
    if span.flags & crate::model::flags::BOLD != 0 {
        style.push_str(";font-weight:bold");
    }
    if span.flags & crate::model::flags::ITALIC != 0 {
        style.push_str(";font-style:italic");
    }
    s.push_str(&format!("<span style=\"{}\">", style));
    html_escape_into(s, &span.text);
    s.push_str("</span>");
}

fn html_image_block(s: &mut String, block: &Block) {
    let b = block.bbox.normalize();
    s.push_str(&format!(
        "<img style=\"top:{}pt;left:{}pt;width:{}pt;height:{}pt\"/>\n",
        fmt_num(b.y0),
        fmt_num(b.x0),
        fmt_num(b.x1 - b.x0),
        fmt_num(b.y1 - b.y0)
    ));
}

/// A CSS-safe font-family token (strips anything past a problematic char).
fn css_font(font: &str) -> String {
    font.replace(['"', ';', '{', '}'], "")
}

/// Serializes a [`TextPage`] to pdfspine-defined **XML** (`get_text("xml")`):
/// the char-level structural dump — `<page>` → `<block>` → `<line>` → `<font>`
/// (span) → `<char>` with bbox attributes. Well-formed; Tier-B (PRD §6.1).
#[must_use]
pub fn to_xml(tp: &TextPage, _flags: u32) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(&format!(
        "<page id=\"page0\" width=\"{}\" height=\"{}\">\n",
        fmt_num(tp.width),
        fmt_num(tp.height)
    ));
    for block in &tp.blocks {
        match block.kind {
            BlockKind::Text => xml_text_block(&mut s, block),
            BlockKind::Image => {
                let b = block.bbox.normalize();
                s.push_str(&format!(
                    "<image bbox=\"{} {} {} {}\"/>\n",
                    fmt_num(b.x0),
                    fmt_num(b.y0),
                    fmt_num(b.x1),
                    fmt_num(b.y1)
                ));
            }
        }
    }
    s.push_str("</page>\n");
    s
}

fn xml_text_block(s: &mut String, block: &Block) {
    let b = block.bbox.normalize();
    s.push_str(&format!(
        "<block bbox=\"{} {} {} {}\" justify=\"unknown\">\n",
        fmt_num(b.x0),
        fmt_num(b.y0),
        fmt_num(b.x1),
        fmt_num(b.y1)
    ));
    for line in &block.lines {
        let lb = line.bbox.normalize();
        s.push_str(&format!(
            "<line bbox=\"{} {} {} {}\" wmode=\"{}\" dir=\"{} {}\" flags=\"0\" text=\"{}\">\n",
            fmt_num(lb.x0),
            fmt_num(lb.y0),
            fmt_num(lb.x1),
            fmt_num(lb.y1),
            line.wmode,
            fmt_num(line.dir.0),
            fmt_num(line.dir.1),
            xml_attr(&line_text(line))
        ));
        for span in &line.spans {
            xml_span(s, span);
        }
        s.push_str("</line>\n");
    }
    s.push_str("</block>\n");
}

fn xml_span(s: &mut String, span: &Span) {
    s.push_str(&format!(
        "<font name=\"{}\" size=\"{}\">\n",
        xml_attr(&span.font),
        fmt_num(span.size)
    ));
    let color = format!("#{:06x}", span.color & 0x00FF_FFFF);
    for ch in &span.chars {
        // The char quad: the glyph cell's four *true* corners in device space,
        // `ul, ur, ll, lr` (PyMuPDF `<char quad=…>`). Rotated or sheared text
        // yields a real parallelogram; for upright text it is the bbox corners.
        let q = &ch.quad;
        s.push_str(&format!(
            "<char quad=\"{} {} {} {} {} {} {} {}\" x=\"{}\" y=\"{}\" bidi=\"0\" color=\"{}\" alpha=\"#ff\" flags=\"{}\" c=\"{}\"/>\n",
            fmt_num(q.ul.x), fmt_num(q.ul.y),
            fmt_num(q.ur.x), fmt_num(q.ur.y),
            fmt_num(q.ll.x), fmt_num(q.ll.y),
            fmt_num(q.lr.x), fmt_num(q.lr.y),
            fmt_num(ch.origin.x),
            fmt_num(ch.origin.y),
            color,
            span.flags,
            xml_attr(&ch.c.to_string())
        ));
    }
    s.push_str("</font>\n");
}

/// Escapes text for HTML/XML element content.
fn html_escape_into(s: &mut String, raw: &str) {
    for c in raw.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            c => s.push(c),
        }
    }
}

/// Escapes a string for use inside an XML/HTML attribute value (double-quoted).
fn xml_attr(raw: &str) -> String {
    let mut s = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            c => s.push(c),
        }
    }
    s
}

// === small helpers =======================================================

fn rect_tuple(r: Rect) -> (f64, f64, f64, f64) {
    let n = r.normalize();
    (n.x0, n.y0, n.x1, n.y1)
}

fn point_tuple(p: Point) -> (f64, f64) {
    (p.x, p.y)
}

fn matrix_tuple(m: &Matrix) -> MatrixTuple {
    (m.a, m.b, m.c, m.d, m.e, m.f)
}

fn quad_tuple(q: &Quad) -> QuadTuple {
    (
        q.ul.x, q.ul.y, q.ur.x, q.ur.y, q.ll.x, q.ll.y, q.lr.x, q.lr.y,
    )
}
