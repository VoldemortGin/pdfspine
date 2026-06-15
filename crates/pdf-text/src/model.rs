//! Output types of the content-stream interpreter (M2b, PRD §8.6).
//!
//! The interpreter emits a flat list of [`PositionedGlyph`]s plus a side
//! inventory of [`ImageRef`]s. Layout grouping (spans/lines/blocks) and the
//! page transform to PyMuPDF device space happen later (M2c/M2d); everything
//! here is in **PDF user space** (origin bottom-left, y up) as produced by the
//! text rendering matrix `Trm` (PRD §8.6.1).

use pdf_core::geom::{Point, Rect};
use smol_str::SmolStr;

/// The writing direction of a glyph (horizontal vs. vertical writing mode).
///
/// v1 implements horizontal writing fully; vertical writing (`wmode 1`) is
/// recognized and tagged but advances are approximated (documented gap — the
/// `/W2`/`/DW2` metrics land with the CJK CMap data, see PRD §8.6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritingDir {
    /// Left-to-right horizontal writing (`wmode 0`, the default).
    Horizontal,
    /// Top-to-bottom vertical writing (`wmode 1`).
    Vertical,
}

/// One painted glyph, positioned in PDF user space (PRD §8.6).
///
/// Coordinates come straight from the text rendering matrix `Trm` (PRD §8.6.1):
/// `origin` is `(0, 0)·Trm` and `bbox` is the axis-aligned envelope of the
/// glyph cell `[0, descent .. advance, ascent]` (in 1000-unit glyph space,
/// scaled by the font size) mapped through `Trm`. Because the envelope is taken
/// *after* the transform, a rotated `Tm` yields a correct axis-aligned box
/// (the `COORD-ROT-*-TRM` contract).
#[derive(Clone, Debug, PartialEq)]
pub struct PositionedGlyph {
    /// The glyph's Unicode string (`/ToUnicode` → encoding+AGL ladder). Empty
    /// when the code is unmapped (still emitted so layout/positions are intact).
    pub unicode: SmolStr,
    /// The raw character code that produced this glyph (1 byte for simple
    /// fonts; the multi-byte value for Type0).
    pub code: u32,
    /// The glyph origin (baseline, left edge) in PDF user space.
    pub origin: Point,
    /// The axis-aligned glyph bounding box in PDF user space.
    pub bbox: Rect,
    /// The resource name the font was referenced under (e.g. `F1`), if known.
    pub font_name: SmolStr,
    /// The text font size `Tfs` in effect.
    pub size: f64,
    /// The current fill color packed as `0x00RRGGBB` sRGB.
    pub color: u32,
    /// The text render mode `Tr` (0 fill … 7 clip; 3 = invisible).
    pub render_mode: u8,
    /// The writing direction in effect.
    pub writing_dir: WritingDir,
    /// The font ascender normalized to a unit font size (`/Ascent ÷ 1000`),
    /// matching PyMuPDF's span `ascender` (PRD §8.6.2, §10.7).
    pub ascender: f64,
    /// The font descender normalized to a unit font size (`/Descent ÷ 1000`,
    /// usually negative), matching PyMuPDF's span `descender`.
    pub descender: f64,
}

impl PositionedGlyph {
    /// Whether this glyph is invisible (render mode 3 — OCR text layers).
    #[must_use]
    pub fn is_invisible(&self) -> bool {
        self.render_mode == 3
    }
}

/// A reference to an image painted on the page (XObject `Do` or inline `BI`),
/// captured for the M2d/M5 image inventory. The bytes are **not** decoded here.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRef {
    /// The XObject resource name (`Do`), or `None` for an inline image.
    pub name: Option<SmolStr>,
    /// `true` when this came from an inline-image (`BI…ID…EI`) operator.
    pub inline: bool,
    /// The image-placement matrix (the CTM at paint time): the unit square is
    /// mapped by this matrix onto the page.
    pub ctm: pdf_core::geom::Matrix,
    /// Declared pixel width (`/Width` / `/W`), if present.
    pub width: Option<u32>,
    /// Declared pixel height (`/Height` / `/H`), if present.
    pub height: Option<u32>,
}

/// One element of a constructed path (PyMuPDF `get_drawings` `items` entry).
///
/// All points are in **PDF user space** (the interpreter's native frame), with
/// the CTM already applied. The serialization to PyMuPDF device space (top-left)
/// happens in the public `get_drawings` layer, mirroring the text path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathItem {
    /// A straight segment `("l", p1, p2)`.
    Line(Point, Point),
    /// A cubic Bézier `("c", p1, p2, p3, p4)` (start, ctrl1, ctrl2, end).
    Curve(Point, Point, Point, Point),
    /// A rectangle `("re", rect)` (axis-aligned in user space).
    Rect(Rect),
}

/// How a constructed path was painted (PyMuPDF drawing `type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintKind {
    /// Stroked only (`type == "s"`).
    Stroke,
    /// Filled only (`type == "f"`).
    Fill,
    /// Filled **and** stroked (`type == "fs"`).
    FillStroke,
}

impl PaintKind {
    /// The PyMuPDF `type` string (`"s"`/`"f"`/`"fs"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PaintKind::Stroke => "s",
            PaintKind::Fill => "f",
            PaintKind::FillStroke => "fs",
        }
    }
}

/// One painted path captured by the interpreter (PyMuPDF `get_drawings` entry).
///
/// Geometry is in **PDF user space** (CTM applied). `rect` is the axis-aligned
/// envelope of the path's points.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawPath {
    /// How the path was painted.
    pub kind: PaintKind,
    /// The path's bounding rect (union of all item points), user space.
    pub rect: Rect,
    /// The stroke color packed as `0x00RRGGBB` sRGB (`None` if not stroked).
    pub color: Option<u32>,
    /// The fill color packed as `0x00RRGGBB` sRGB (`None` if not filled).
    pub fill: Option<u32>,
    /// The stroke line width (user space, post-CTM scaled by `|a|` heuristic —
    /// here the raw `w` operand, matching PyMuPDF's `width`).
    pub width: f64,
    /// The dash pattern string (`"[…] phase"`), empty when solid.
    pub dashes: String,
    /// Whether the (last) sub-path was explicitly closed (`h`/`s`/`b`).
    pub close_path: bool,
    /// Whether an even-odd fill rule was used (`f*`/`B*`/`b*`).
    pub even_odd: bool,
    /// The constructed path items in construction order.
    pub items: Vec<PathItem>,
}

/// The complete result of interpreting a page's content (M2b).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterpretResult {
    /// Every painted glyph, in content (paint) order.
    pub glyphs: Vec<PositionedGlyph>,
    /// Every image painted (XObject or inline), in content order.
    pub images: Vec<ImageRef>,
    /// Every painted vector path, in content order (PRD §8.8 `get_drawings`).
    pub drawings: Vec<DrawPath>,
}

// === M2c — the PyMuPDF-shaped TextPage model =============================
//
// Everything below lives in **PyMuPDF page/device space** (origin top-left, y
// down, `/Rotate` already applied; PRD §8.6.1). The grouping that produces it
// is in `layout.rs`; serialization to text/words/blocks/dict/rawdict is M2d.

/// PyMuPDF span-flag bits (Tier-A documented values; PRD §8.6.2, §8.5).
///
/// `flags = SUPERSCRIPT|ITALIC|SERIF|MONO|BOLD`. Bit 0 (superscript) is a
/// layout-derived property; bits 1–4 are font properties.
pub mod flags {
    /// Bit 0 (value 1): the span sits on a raised baseline (super/subscript).
    pub const SUPERSCRIPT: u32 = 1;
    /// Bit 1 (value 2): italic / oblique.
    pub const ITALIC: u32 = 1 << 1;
    /// Bit 2 (value 4): serifed.
    pub const SERIF: u32 = 1 << 2;
    /// Bit 3 (value 8): monospaced.
    pub const MONO: u32 = 1 << 3;
    /// Bit 4 (value 16): bold.
    pub const BOLD: u32 = 1 << 4;
}

/// The kind of a [`Block`]: a run of text lines, or a placed image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// A text block (PyMuPDF block `type` 0): carries [`Block::lines`].
    Text,
    /// An image block (PyMuPDF block `type` 1): carries [`Block::image`].
    Image,
}

/// One character with its device-space geometry (PyMuPDF rawdict `char`).
#[derive(Clone, Debug, PartialEq)]
pub struct Char {
    /// The glyph origin (baseline, left edge) in device space.
    pub origin: Point,
    /// The axis-aligned glyph bounding box in device space.
    pub bbox: Rect,
    /// The Unicode scalar this glyph maps to (`\u{FFFD}` for unmapped codes,
    /// matching PyMuPDF's replacement behavior).
    pub c: char,
}

/// A contiguous same-style run of characters (PyMuPDF dict/rawdict `span`).
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// The span bounding box (union of its char bboxes), device space.
    pub bbox: Rect,
    /// The font name (resource name, or resolved BaseFont when available).
    pub font: SmolStr,
    /// The font size `Tfs` in effect for the span.
    pub size: f64,
    /// The PyMuPDF span-flag bitfield (see [`flags`]).
    pub flags: u32,
    /// The fill color packed as `0x00RRGGBB` sRGB.
    pub color: u32,
    /// The font ascender normalized to a unit font size (PyMuPDF span
    /// `ascender`; PRD §10.7).
    pub ascender: f64,
    /// The font descender normalized to a unit font size, usually negative
    /// (PyMuPDF span `descender`).
    pub descender: f64,
    /// The origin (baseline, left edge) of the span's first char, device space
    /// (PyMuPDF span `origin`).
    pub origin: Point,
    /// The per-character detail (rawdict-level).
    pub chars: Vec<Char>,
    /// The span text (concatenation of `chars[i].c`), the dict-level field.
    pub text: String,
}

/// One line of text: a baseline-aligned, advance-ordered run of spans
/// (PyMuPDF dict/rawdict `line`).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// The line bounding box (union of its span bboxes), device space.
    pub bbox: Rect,
    /// The writing mode: 0 horizontal, 1 vertical.
    pub wmode: u8,
    /// The writing-direction unit vector `(cos, sin)` (PyMuPDF line `dir`).
    pub dir: (f64, f64),
    /// The spans of this line, in advance order.
    pub spans: Vec<Span>,
    /// Content-order key: the smallest source-glyph (paint) index among this
    /// line's glyphs. Used to order blocks in document/content order, which is
    /// how MuPDF/PyMuPDF sequences its structured-text blocks (PRD §8.6.2).
    pub seq: usize,
}

/// An image placed on the page (PyMuPDF image block), device-space bbox only;
/// pixel data is deferred to M5.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageBlock {
    /// The XObject resource name, or `None` for an inline image.
    pub name: Option<SmolStr>,
    /// Declared pixel width (`/Width` / `/W`), if present.
    pub width: Option<u32>,
    /// Declared pixel height (`/Height` / `/H`), if present.
    pub height: Option<u32>,
}

/// A paragraph-ish grouping of lines, or an image (PyMuPDF dict/rawdict
/// `block`).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The block bounding box (union of its content), device space.
    pub bbox: Rect,
    /// Whether this is a text or image block.
    pub kind: BlockKind,
    /// The lines (empty for an image block), in reading order.
    pub lines: Vec<Line>,
    /// The image payload (`Some` iff [`BlockKind::Image`]).
    pub image: Option<ImageBlock>,
    /// The reading-order block number (PyMuPDF block `number`).
    pub number: usize,
    /// Content-order key: the smallest source-glyph (paint) index among the
    /// block's lines (image blocks default to `usize::MAX`). Drives the
    /// document/content-order block sequencing in [`crate::layout`].
    pub seq: usize,
}

/// The full layout-reconstructed page (PyMuPDF `TextPage`), device space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextPage {
    /// The page width in device space (rotated-page width).
    pub width: f64,
    /// The page height in device space (rotated-page height).
    pub height: f64,
    /// The blocks in reading order.
    pub blocks: Vec<Block>,
}

/// One word produced by the word segmenter: a device-space bbox, its text, and
/// the `(block, line, word)` index triple (PyMuPDF `get_text("words")` shape).
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    /// The word bounding box (union of its char bboxes), device space.
    pub bbox: Rect,
    /// The word text.
    pub text: String,
    /// The owning block's reading-order number.
    pub block_no: usize,
    /// The line index within the block.
    pub line_no: usize,
    /// The word index within the line (resets per line).
    pub word_no: usize,
}
