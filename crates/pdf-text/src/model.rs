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

/// The complete result of interpreting a page's content (M2b).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterpretResult {
    /// Every painted glyph, in content (paint) order.
    pub glyphs: Vec<PositionedGlyph>,
    /// Every image painted (XObject or inline), in content order.
    pub images: Vec<ImageRef>,
}
