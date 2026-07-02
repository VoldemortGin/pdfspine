//! The layout-ready input IR (PRD §10 scope (a), TS-1).
//!
//! Consumers (docspine `doc-render` / pptspine `ppt-render`) translate OOXML
//! into this model; all `numbering.xml` / `buChar` intelligence stays on the
//! consumer side — a list paragraph arrives with its **final** label string in
//! [`ListLabel`]. Geometry is in PDF points, colors are [`Rgb`], and text-box
//! rects use top-left page coordinates (the engine y-flips at emission).
//!
//! Structs marked `#[non_exhaustive]` grow over the TS phases: construct them
//! with their `new` constructors, then set the public fields you need.

use crate::Rect;
use crate::Rgb;

/// One styled text run.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The run text (no markup; hard line breaks arrive as `\n` characters
    /// inside the text and are honored by the wrapper).
    pub text: String,
    /// The character formatting of this run.
    pub style: RunStyle,
}

impl Run {
    /// A run of `text` in `style`.
    #[must_use]
    pub fn new(text: impl Into<String>, style: RunStyle) -> Self {
        Run {
            text: text.into(),
            style,
        }
    }
}

/// Per-run character formatting (docx `rPr` / pptx `rPr` subset).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    /// The requested font family (resolved via `fontres`, PRD §10 TS-2).
    pub family: String,
    /// Font size in points.
    pub size: f64,
    /// Bold. A missing bold face substitutes the nearest style — never
    /// synthetic emboldening (locked decision).
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline decoration.
    pub underline: bool,
    /// Strikethrough decoration.
    pub strike: bool,
    /// Text fill color.
    pub color: Rgb,
    /// Optional highlight (drawn as a filled rect behind the run).
    pub highlight: Option<Rgb>,
}

impl RunStyle {
    /// A plain style: `family` at `size` pt, black, no decorations.
    #[must_use]
    pub fn new(family: impl Into<String>, size: f64) -> Self {
        RunStyle {
            family: family.into(),
            size,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: Rgb::BLACK,
            highlight: None,
        }
    }
}

/// Horizontal paragraph alignment. `Justify` redistributes inter-word space
/// (last line left) — PRD §10 TRAP: PDF `Tw` cannot implement it under
/// Identity-H, so the layouter widens space fragments instead.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Left-aligned (default).
    #[default]
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
    /// Justified; the last line stays left-aligned.
    Justify,
}

/// Line spacing (docx `spacing` semantics).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum LineSpacing {
    /// A multiple of the natural line height (docx `lineRule="auto"`).
    Multiple(f64),
    /// An exact line height in points (docx `lineRule="exact"`).
    Exact(f64),
}

impl Default for LineSpacing {
    fn default() -> Self {
        LineSpacing::Multiple(1.0)
    }
}

/// The final, consumer-computed label of a list paragraph (bullet glyph or
/// formatted number — counters / `%1.%2` formats / restarts are consumer
/// business, PRD §10).
#[derive(Clone, Debug, PartialEq)]
pub struct ListLabel {
    /// The label text, drawn right-aligned against the paragraph text start.
    pub text: String,
    /// Gap between the label's right edge and the paragraph text, in points.
    pub gutter: f64,
}

/// Paragraph-level properties (docx `pPr` / pptx `pPr` subset).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct ParaProps {
    /// Horizontal alignment (including justify).
    pub align: Align,
    /// Line spacing rule.
    pub spacing: LineSpacing,
    /// Space before the paragraph, in points (collapses at a page top —
    /// Word-ish semantics inherited from the pdf-markdown pending-gap model).
    pub space_before: f64,
    /// Space after the paragraph, in points.
    pub space_after: f64,
    /// Left indent of the whole paragraph, in points.
    pub indent_left: f64,
    /// Right indent of the whole paragraph, in points.
    pub indent_right: f64,
    /// Extra first-line indent in points; **negative = hanging indent**.
    pub first_line_indent: f64,
    /// List label (present ⇒ this paragraph is a list item).
    pub list: Option<ListLabel>,
}

impl ParaProps {
    /// Default paragraph properties: left-aligned, single spacing, no space
    /// before/after, no indents, not a list item.
    #[must_use]
    pub fn new() -> Self {
        ParaProps {
            align: Align::Left,
            spacing: LineSpacing::default(),
            space_before: 0.0,
            space_after: 0.0,
            indent_left: 0.0,
            indent_right: 0.0,
            first_line_indent: 0.0,
            list: None,
        }
    }
}

impl Default for ParaProps {
    fn default() -> Self {
        ParaProps::new()
    }
}

/// A block-level element, in document order.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A paragraph of styled runs under paragraph properties.
    Paragraph(ParaProps, Vec<Run>),
    /// A table (grid measure / cell layout / per-edge borders, TS-4).
    Table(TableSpec),
    /// A placed image.
    Image(ImageSpec),
    /// An explicit page break (flow layout only; ignored inside text boxes).
    PageBreak,
}

/// One column's width policy in a table grid.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ColumnWidth {
    /// A fixed width in points.
    Fixed(f64),
    /// Measured from content (fair-share shrink when the grid overflows).
    Auto,
}

/// One table border edge.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BorderEdge {
    /// Stroke width in points.
    pub width: f64,
    /// Stroke color.
    pub color: Rgb,
}

/// Per-edge cell borders; `None` edges are not painted.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CellBorders {
    /// Top edge.
    pub top: Option<BorderEdge>,
    /// Right edge.
    pub right: Option<BorderEdge>,
    /// Bottom edge.
    pub bottom: Option<BorderEdge>,
    /// Left edge.
    pub left: Option<BorderEdge>,
}

/// One table cell: nested blocks plus paint properties.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct TableCell {
    /// The cell content (paragraphs, images, nested tables).
    pub blocks: Vec<Block>,
    /// Optional background fill.
    pub fill: Option<Rgb>,
    /// Per-edge borders (painted as 4 line ops, not a stroked rect).
    pub borders: CellBorders,
    /// Inner padding on every side, in points.
    pub padding: f64,
}

impl TableCell {
    /// A cell of `blocks` with no fill, no borders and 0 padding.
    #[must_use]
    pub fn new(blocks: Vec<Block>) -> Self {
        TableCell {
            blocks,
            fill: None,
            borders: CellBorders::default(),
            padding: 0.0,
        }
    }
}

/// One table row.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    /// The row's cells, left to right (one per grid column).
    pub cells: Vec<TableCell>,
    /// Optional minimum row height in points (content may grow it).
    pub min_height: Option<f64>,
}

impl TableRow {
    /// A row of `cells` with content-driven height.
    #[must_use]
    pub fn new(cells: Vec<TableCell>) -> Self {
        TableRow {
            cells,
            min_height: None,
        }
    }
}

/// A table: a column grid plus rows of cells.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct TableSpec {
    /// The column grid, left to right.
    pub columns: Vec<ColumnWidth>,
    /// The rows, top to bottom.
    pub rows: Vec<TableRow>,
}

impl TableSpec {
    /// A table over `columns` with `rows`.
    #[must_use]
    pub fn new(columns: Vec<ColumnWidth>, rows: Vec<TableRow>) -> Self {
        TableSpec { columns, rows }
    }
}

/// A placed image: encoded bytes plus display size.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct ImageSpec {
    /// The encoded image file bytes (JPEG passes through as DCT; PNG / BMP /
    /// GIF / WEBP / TIFF decode via `pdf-image` at emission).
    pub data: Vec<u8>,
    /// Display width in points.
    pub width: f64,
    /// Display height in points.
    pub height: f64,
}

impl ImageSpec {
    /// An image of `data` displayed at `width` × `height` points.
    #[must_use]
    pub fn new(data: Vec<u8>, width: f64, height: f64) -> Self {
        ImageSpec {
            data,
            width,
            height,
        }
    }
}

/// Vertical anchoring of text-box content inside its rect.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum VAnchor {
    /// Content starts at the top (default).
    #[default]
    Top,
    /// Content is vertically centered.
    Middle,
    /// Content ends at the bottom.
    Bottom,
}

/// An absolutely-positioned text box (pptx shape text body / docx text box):
/// a fixed rect that does its own vertical anchoring, autofit and clipping —
/// **not** `insert_textbox`, which silently drops overflow (PRD §10 TRAP).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct TextBoxSpec {
    /// The box rectangle in top-left page coordinates.
    pub rect: Rect,
    /// Vertical anchoring of the laid-out content.
    pub v_anchor: VAnchor,
    /// Word-wrap at the box width (`false` = hard-break lines only).
    pub wrap: bool,
    /// `normAutofit` font scale in `(0, 1]` (`None` = no autofit): all run
    /// sizes multiply by this factor before layout.
    pub font_scale: Option<f64>,
    /// Rotation about the box center, in degrees (counter-clockwise).
    pub rotation_deg: f64,
    /// Clip overflowing content to the rect (emits a `BoxOverflowClipped`
    /// warning when content is lost).
    pub clip: bool,
    /// The box content.
    pub blocks: Vec<Block>,
}

impl TextBoxSpec {
    /// A top-anchored, wrapping, unrotated, unclipped box over `rect`.
    #[must_use]
    pub fn new(rect: Rect, blocks: Vec<Block>) -> Self {
        TextBoxSpec {
            rect,
            v_anchor: VAnchor::Top,
            wrap: true,
            font_scale: None,
            rotation_deg: 0.0,
            clip: false,
            blocks,
        }
    }
}
