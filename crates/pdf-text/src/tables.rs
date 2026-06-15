//! Table detection / extraction (M7), a pure-Rust reimplementation of PyMuPDF
//! `page.find_tables()`.
//!
//! Two detection strategies, mirroring PyMuPDF:
//!
//! - **`Lines`** — reconstruct the cell grid from the page's vector ruling
//!   lines. Thin/long fills and strokes from the interpreter's drawings are
//!   reduced to horizontal + vertical segments, snapped to a tolerance grid, and
//!   the grid's edge graph is walked to recover cell rectangles.
//! - **`Text`** — when there are no usable rulings, cluster the page [`Word`]s
//!   into columns (x-gaps) and rows (y-gaps) to infer an implicit grid.
//!
//! Everything here works in **PyMuPDF device space** (origin top-left, y down):
//! [`Table`] bboxes/cells and the input [`Word`]s share that frame. Callers that
//! start from [`crate::InterpretResult::drawings`] (PDF user space) must first
//! map the paths through [`crate::page_transform`] — see [`drawings_to_device`].
//!
//! The output mirrors PyMuPDF's `Table`/`TableFinder`: a [`TableFinder`] holds a
//! `Vec<Table>`; each [`Table`] exposes `bbox`, `row_count`, `col_count`, a
//! `cells` grid of `Option<Rect>`, the detected `header`, the snapped `rows`/`cols`
//! line positions, plus [`Table::extract`], [`Table::to_markdown`] and
//! [`Table::to_html`].
//!
//! ## Merged / spanning cells
//!
//! Beyond the regular `Option<Rect>` grid, the detector recognizes **merged
//! cells** (colspan / rowspan). Two adjacent grid slots collapse into one cell
//! when the ruling segment that would separate them is **absent** (`Lines`
//! strategy) or when a single word straddles the boundary between them (`Text`
//! strategy). The merge result is the `spans` list of [`CellSpan`]s — one entry
//! per *originating* cell, carrying its `row_span`/`col_span`. Continuation
//! slots covered by a span are dropped from `cells` (set to `None`) and have no
//! `spans` entry, so they are never double-emitted. [`Table::to_html`] uses this
//! to emit faithful `colspan`/`rowspan`; [`Table::to_markdown`] and
//! [`Table::extract`] degrade gracefully (text in the originating slot, blanks
//! / `None` for continuation slots — see their docs).

use pdf_core::geom::{Matrix, Point, Rect};

use crate::model::{DrawPath, PathItem, TextPage, Word};

/// The cell-grid extraction strategy (PyMuPDF `find_tables(strategy=...)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Strategy {
    /// Build the grid from vector ruling lines only (PyMuPDF `"lines"`).
    #[default]
    Lines,
    /// Like [`Strategy::Lines`] but never falls back to text clustering
    /// (PyMuPDF `"lines_strict"`). Rulings must form the grid on their own.
    LinesStrict,
    /// Infer the grid purely from word alignment (PyMuPDF `"text"`).
    Text,
}

/// Options controlling table detection (a trimmed [`PyMuPDF`]
/// `find_tables` parameter set).
///
/// [`PyMuPDF`]: https://pymupdf.readthedocs.io/en/latest/page.html#Page.find_tables
#[derive(Clone, Copy, Debug)]
pub struct TableOptions {
    /// Which detection strategy to use for both axes.
    pub strategy: Strategy,
    /// Max ruling thickness (device units) for a fill/stroke to count as a line.
    /// Thicker fills are treated as filled boxes, not rules.
    pub line_max_thickness: f64,
    /// Snap tolerance (device units): edges within this distance collapse to one
    /// grid line. Also the column/row clustering gap for the text strategy uses
    /// a multiple of this (see [`TableOptions::text_gap`]).
    pub snap_tolerance: f64,
    /// Min span (device units) a ruling must cover to be considered a real edge
    /// (filters tick marks / underlines that are too short to bound a cell).
    pub min_line_length: f64,
}

impl Default for TableOptions {
    fn default() -> Self {
        TableOptions {
            strategy: Strategy::Lines,
            line_max_thickness: 3.0,
            snap_tolerance: 3.0,
            min_line_length: 3.0,
        }
    }
}

impl TableOptions {
    /// A `TableOptions` with the given strategy and otherwise default tuning.
    #[must_use]
    pub fn with_strategy(strategy: Strategy) -> Self {
        TableOptions {
            strategy,
            ..Self::default()
        }
    }

    /// The column/row clustering gap for the text strategy: a word x/y gap wider
    /// than this starts a new column/row band.
    fn text_gap(&self) -> f64 {
        // Words on the same logical cell never gap by more than a wide space; a
        // gap of several snap-tolerances marks a real column/row boundary.
        (self.snap_tolerance * 2.0).max(3.0)
    }
}

/// One merged (or unit) cell, anchored at its top-left grid slot `(row, col)`
/// and covering `row_span` × `col_span` grid slots. `rect` is the cell's full
/// merged bounding box (device space). A non-merged cell has both spans `1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellSpan {
    /// The originating grid row (0-based, top row first).
    pub row: usize,
    /// The originating grid column (0-based, left column first).
    pub col: usize,
    /// How many grid rows this cell covers (`>= 1`).
    pub row_span: usize,
    /// How many grid columns this cell covers (`>= 1`).
    pub col_span: usize,
    /// The merged cell's bounding box (device space).
    pub rect: Rect,
}

/// One detected table.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// The table's bounding box (device space).
    pub bbox: Rect,
    /// The number of cell rows.
    pub row_count: usize,
    /// The number of cell columns.
    pub col_count: usize,
    /// The snapped vertical grid-line x positions (`col_count + 1` entries when
    /// the grid is regular), left → right.
    pub cols: Vec<f64>,
    /// The snapped horizontal grid-line y positions (`row_count + 1` entries when
    /// regular), top → bottom.
    pub rows: Vec<f64>,
    /// The cell rectangles in row-major order. `None` marks a missing cell (an
    /// absent grid slot) **or** a continuation slot covered by a merged cell
    /// originating elsewhere — the origin slot of a merged cell holds the full
    /// merged [`Rect`]. Outer `Vec` is rows, inner is columns.
    pub cells: Vec<Vec<Option<Rect>>>,
    /// The merged-cell model: one [`CellSpan`] per *originating* cell (the
    /// top-left slot of each cell). Continuation slots have no entry here. A
    /// regular grid yields one unit span (`row_span = col_span = 1`) per present
    /// cell. Use [`Table::span_at`] to look up the originating span for a slot.
    pub spans: Vec<CellSpan>,
    /// The detected header: the first row's cell text, when a plausible header
    /// row is present (else empty).
    pub header: Vec<Option<String>>,
}

impl Table {
    /// The originating [`CellSpan`] anchored at grid slot `(row, col)`, or `None`
    /// when that slot is empty or is a continuation slot covered by a merged
    /// cell that originates elsewhere.
    #[must_use]
    pub fn span_at(&self, row: usize, col: usize) -> Option<&CellSpan> {
        self.spans.iter().find(|s| s.row == row && s.col == col)
    }

    /// Extracts the cell text in reading order: `rows[r][c]` is the joined text
    /// of all [`Word`]s whose center lies inside cell `(r, c)`, or `None` for an
    /// empty / missing cell.
    ///
    /// For **merged cells** the text appears in the originating (top-left) slot;
    /// continuation slots covered by the merge are `None`. This matches
    /// PyMuPDF's `Table.extract`, which likewise reports a merged cell's text in
    /// its first slot and leaves the covered slots empty (it never repeats the
    /// text across the span).
    #[must_use]
    pub fn extract(&self, words: &[Word]) -> Vec<Vec<Option<String>>> {
        self.cells
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.and_then(|rect| cell_text(rect, words)))
                    .collect()
            })
            .collect()
    }

    /// Renders the table as a GitHub-flavored Markdown table. The first row is
    /// used as the header (matching PyMuPDF `Table.to_markdown`); when no real
    /// header was detected a blank header row is emitted so the table is still
    /// well-formed.
    ///
    /// **Lossiness:** Markdown has no notion of merged cells, so a spanning cell
    /// degrades — its text is emitted only in the originating slot and the
    /// continuation slots render as blanks. The column/row count is preserved so
    /// the result stays valid GFM, but colspan/rowspan information is lost. Use
    /// [`Table::to_html`] for a faithful, span-aware rendering.
    #[must_use]
    pub fn to_markdown(&self, words: &[Word]) -> String {
        let grid = self.extract(words);
        if grid.is_empty() || self.col_count == 0 {
            return String::new();
        }
        let cell = |row: &[Option<String>], c: usize| -> String {
            row.get(c)
                .and_then(|o| o.as_deref())
                .unwrap_or("")
                .replace('\n', " ")
                .replace('|', r"\|")
                .trim()
                .to_string()
        };

        let mut out = String::new();
        // Header row (first grid row).
        out.push('|');
        for c in 0..self.col_count {
            out.push(' ');
            out.push_str(&cell(&grid[0], c));
            out.push_str(" |");
        }
        out.push('\n');
        // Separator.
        out.push('|');
        for _ in 0..self.col_count {
            out.push_str(" --- |");
        }
        out.push('\n');
        // Body rows.
        for row in grid.iter().skip(1) {
            out.push('|');
            for c in 0..self.col_count {
                out.push(' ');
                out.push_str(&cell(row, c));
                out.push_str(" |");
            }
            out.push('\n');
        }
        out
    }

    /// Renders the table as a high-fidelity HTML `<table>`.
    ///
    /// Output shape (oxide-defined, own goldens): a `<table>` wrapping one
    /// `<tr>` per grid row. Each *originating* cell is emitted once as a `<td>`
    /// (or `<th>` in the detected header row) with `colspan`/`rowspan`
    /// attributes when it spans more than one grid slot; continuation slots
    /// covered by a span are skipped, so each merged cell appears exactly once.
    /// Empty (absent) slots emit an empty `<td>`/`<th>`.
    ///
    /// Cell text is the [`Word`]s whose center lies inside the cell rect, joined
    /// in reading order with lines separated by `<br>` (multi-line content is
    /// preserved) and `&`, `<`, `>`, `"` HTML-escaped.
    #[must_use]
    pub fn to_html(&self, words: &[Word]) -> String {
        let mut out = String::from("<table>");
        if self.col_count == 0 || self.row_count == 0 {
            out.push_str("</table>");
            return out;
        }
        let has_header = self.header.iter().any(|h| h.is_some());
        for r in 0..self.row_count {
            out.push_str("<tr>");
            let tag = if r == 0 && has_header { "th" } else { "td" };
            for c in 0..self.col_count {
                match self.span_at(r, c) {
                    Some(span) => {
                        out.push('<');
                        out.push_str(tag);
                        if span.col_span > 1 {
                            out.push_str(&format!(" colspan=\"{}\"", span.col_span));
                        }
                        if span.row_span > 1 {
                            out.push_str(&format!(" rowspan=\"{}\"", span.row_span));
                        }
                        out.push('>');
                        if let Some(text) = cell_html_text(span.rect, words) {
                            out.push_str(&text);
                        }
                        out.push_str("</");
                        out.push_str(tag);
                        out.push('>');
                    }
                    None => {
                        // Either an absent slot (emit an empty cell) or a
                        // continuation slot covered by a span (skip entirely).
                        if !self.is_covered(r, c) {
                            out.push('<');
                            out.push_str(tag);
                            out.push_str("></");
                            out.push_str(tag);
                            out.push('>');
                        }
                    }
                }
            }
            out.push_str("</tr>");
        }
        out.push_str("</table>");
        out
    }

    /// Whether grid slot `(row, col)` is a continuation slot covered by a merged
    /// cell that originates at a different slot.
    fn is_covered(&self, row: usize, col: usize) -> bool {
        self.spans.iter().any(|s| {
            row >= s.row
                && row < s.row + s.row_span
                && col >= s.col
                && col < s.col + s.col_span
                && !(row == s.row && col == s.col)
        })
    }
}

/// The result of [`find_tables`]: every detected [`Table`] on the page, in
/// top-to-bottom, left-to-right order (PyMuPDF `TableFinder`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableFinder {
    /// The detected tables.
    pub tables: Vec<Table>,
}

impl TableFinder {
    /// The number of detected tables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// Whether no table was detected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// Detects tables on a page from its layout [`TextPage`] and device-space ruling
/// [`DrawPath`]s.
///
/// `words` are the page words (device space, e.g. from [`crate::words`]);
/// `drawings` are the page's vector paths **already mapped to device space**
/// (see [`drawings_to_device`]). The `Lines`/`LinesStrict` strategies use the
/// drawings; `Text` ignores them. An empty page yields an empty [`TableFinder`].
#[must_use]
pub fn find_tables(
    textpage: &TextPage,
    words: &[Word],
    drawings: &[DrawPath],
    options: &TableOptions,
) -> TableFinder {
    let _ = textpage; // reserved (page size / blocks) — words drive extraction.
    let mut tables = match options.strategy {
        Strategy::Lines => {
            let mut t = detect_lines(drawings, options);
            if t.is_empty() {
                t = detect_text(words, options);
            }
            t
        }
        Strategy::LinesStrict => detect_lines(drawings, options),
        Strategy::Text => detect_text(words, options),
    };
    // Populate each table's header from its first row's cell text (PyMuPDF
    // treats the first row as the header for `to_markdown`).
    for t in &mut tables {
        if let Some(first) = t.cells.first() {
            t.header = first
                .iter()
                .map(|cell| cell.and_then(|rect| cell_text(rect, words)))
                .collect();
        }
    }
    TableFinder { tables }
}

/// Maps the interpreter's user-space [`DrawPath`]s into PyMuPDF device space via
/// the page transform, so they can be fed to [`find_tables`]. Convenience for
/// callers holding raw [`crate::InterpretResult::drawings`].
#[must_use]
pub fn drawings_to_device(drawings: &[DrawPath], page_transform: &Matrix) -> Vec<DrawPath> {
    drawings
        .iter()
        .map(|d| {
            let items = d
                .items
                .iter()
                .map(|it| match *it {
                    PathItem::Line(a, b) => {
                        PathItem::Line(a.transform(page_transform), b.transform(page_transform))
                    }
                    PathItem::Curve(a, b, c, e) => PathItem::Curve(
                        a.transform(page_transform),
                        b.transform(page_transform),
                        c.transform(page_transform),
                        e.transform(page_transform),
                    ),
                    PathItem::Rect(r) => PathItem::Rect(r.transform(page_transform).normalize()),
                })
                .collect();
            DrawPath {
                rect: d.rect.transform(page_transform).normalize(),
                items,
                ..d.clone()
            }
        })
        .collect()
}

// === lines strategy =======================================================

/// An axis-aligned ruling segment after reduction (device space).
#[derive(Clone, Copy, Debug)]
struct Segment {
    /// The constant coordinate (y for a horizontal rule, x for a vertical one).
    pos: f64,
    /// The varying-axis start (min).
    lo: f64,
    /// The varying-axis end (max).
    hi: f64,
}

impl Segment {
    fn len(&self) -> f64 {
        self.hi - self.lo
    }
}

/// Detects tables from ruling lines.
fn detect_lines(drawings: &[DrawPath], opt: &TableOptions) -> Vec<Table> {
    let (h_segs, v_segs) = collect_segments(drawings, opt);
    if h_segs.len() < 2 || v_segs.len() < 2 {
        return Vec::new();
    }

    // Snap the constant coordinates to grid lines.
    let rows = snap_positions(h_segs.iter().map(|s| s.pos), opt.snap_tolerance);
    let cols = snap_positions(v_segs.iter().map(|s| s.pos), opt.snap_tolerance);
    if rows.len() < 2 || cols.len() < 2 {
        return Vec::new();
    }

    // Coverage maps: which horizontal grid lines exist between adjacent col pairs
    // and vice versa. We build cells where all four edges are (partially) ruled.
    let cells = build_cells(&rows, &cols, &h_segs, &v_segs, opt);
    let n_present = cells.iter().flatten().filter(|c| c.is_some()).count();
    if n_present == 0 {
        return Vec::new();
    }

    let bbox = Rect::new(cols[0], rows[0], cols[cols.len() - 1], rows[rows.len() - 1]);
    let row_count = rows.len() - 1;
    let col_count = cols.len() - 1;
    // Build the lattice presence map for span detection: a slot belongs to the
    // table when each side is *sufficiently bounded* — an outer side must carry a
    // ruling, while an interior side may be unruled (that absence is exactly what
    // signals a merge). The strict per-slot `build_cells` grid (all four edges
    // ruled) feeds the regular no-span case; this looser map recovers merged
    // cells whose internal separators were dropped.
    let tol = opt.snap_tolerance;
    let h_at = |y: f64, x0: f64, x1: f64| edge_covered(&h_segs, y, x0, x1, tol);
    let v_at = |x: f64, y0: f64, y1: f64| edge_covered(&v_segs, x, y0, y1, tol);
    let mut presence = vec![vec![false; col_count]; row_count];
    for r in 0..row_count {
        for c in 0..col_count {
            let (y0, y1, x0, x1) = (rows[r], rows[r + 1], cols[c], cols[c + 1]);
            // Outer sides must be ruled; interior sides are always acceptable.
            let top = r > 0 || h_at(y0, x0, x1);
            let bottom = r + 1 < row_count || h_at(y1, x0, x1);
            let left = c > 0 || v_at(x0, y0, y1);
            let right = c + 1 < col_count || v_at(x1, y0, y1);
            // The strictly-ruled slots are always present; otherwise require the
            // outer enclosure so stray interior rules can't invent a whole grid.
            presence[r][c] = cells[r][c].is_some() || (top && bottom && left && right);
        }
    }
    let col_split = |ri: usize, ci: usize| {
        // Is there a vertical rule on grid line `ci+1` over row band `ri`?
        v_at(cols[ci + 1], rows[ri], rows[ri + 1])
    };
    let row_split = |ri: usize, ci: usize| {
        // Is there a horizontal rule on grid line `ri+1` over col band `ci`?
        h_at(rows[ri + 1], cols[ci], cols[ci + 1])
    };
    let (spans, cells) = merge_spans(
        &rows, &cols, &presence, row_count, col_count, col_split, row_split,
    );

    vec![Table {
        bbox,
        row_count,
        col_count,
        cols,
        rows,
        cells,
        spans,
        header: Vec::new(), // populated by `find_tables` (needs the words)
    }]
}

/// Merges adjacent present grid slots into spanning cells.
///
/// `present[r][c]` marks the lattice slots that belong to the table. Two
/// horizontally-adjacent present slots merge when `col_split(r, c)` is `false`
/// (no separating vertical rule); two vertically-adjacent present slots merge
/// when `row_split(r, c)` is `false` (no separating horizontal rule). Returns
/// the originating [`CellSpan`]s plus a rewritten row-major `cells` grid where
/// each origin holds its merged [`Rect`] and continuation/absent slots are
/// `None`.
///
/// The merge is rectangular and greedy in reading order: each not-yet-claimed
/// present slot becomes a span origin, grown right while every row in the
/// candidate band is mergeable across the column boundary, then grown down while
/// every column in the candidate band is mergeable across the row boundary.
#[allow(clippy::too_many_arguments)]
fn merge_spans(
    rows: &[f64],
    cols: &[f64],
    present: &[Vec<bool>],
    row_count: usize,
    col_count: usize,
    col_split: impl Fn(usize, usize) -> bool,
    row_split: impl Fn(usize, usize) -> bool,
) -> (Vec<CellSpan>, Vec<Vec<Option<Rect>>>) {
    let mut claimed = vec![vec![false; col_count]; row_count];
    let mut spans = Vec::new();
    let mut cells: Vec<Vec<Option<Rect>>> = vec![vec![None; col_count]; row_count];

    for r in 0..row_count {
        for c in 0..col_count {
            if claimed[r][c] || !present[r][c] {
                continue;
            }
            // Grow the column span: extend right while the boundary rule is
            // missing and the next slot is present and unclaimed.
            let mut col_span = 1;
            while c + col_span < col_count
                && present[r][c + col_span]
                && !claimed[r][c + col_span]
                && !col_split(r, c + col_span - 1)
            {
                col_span += 1;
            }
            // Grow the row span: extend down while, for every column in the
            // candidate band, the boundary rule is missing and the slot is
            // present and unclaimed.
            let mut row_span = 1;
            'grow_down: while r + row_span < row_count {
                for cc in c..c + col_span {
                    if !present[r + row_span][cc]
                        || claimed[r + row_span][cc]
                        || row_split(r + row_span - 1, cc)
                    {
                        break 'grow_down;
                    }
                }
                row_span += 1;
            }
            for crow in claimed.iter_mut().skip(r).take(row_span) {
                for slot in crow.iter_mut().skip(c).take(col_span) {
                    *slot = true;
                }
            }
            let rect = Rect::new(cols[c], rows[r], cols[c + col_span], rows[r + row_span]);
            cells[r][c] = Some(rect);
            spans.push(CellSpan {
                row: r,
                col: c,
                row_span,
                col_span,
                rect,
            });
        }
    }
    (spans, cells)
}

/// Reduces the page drawings to horizontal + vertical ruling [`Segment`]s.
///
/// A path item contributes a horizontal segment if it is (near) axis-aligned in
/// x and thin in y, and vice versa. Filled rectangles contribute all four of
/// their thin edges (the common "draw a box as a filled rect" idiom) **and**, if
/// the rect itself is a thin bar, the bar as a single rule.
fn collect_segments(drawings: &[DrawPath], opt: &TableOptions) -> (Vec<Segment>, Vec<Segment>) {
    let mut h = Vec::new();
    let mut v = Vec::new();
    let tol = opt.line_max_thickness;

    let mut push_line = |a: Point, b: Point| {
        let dx = (b.x - a.x).abs();
        let dy = (b.y - a.y).abs();
        if dy <= tol && dx > dy {
            // Horizontal segment at y = midpoint.
            h.push(Segment {
                pos: (a.y + b.y) / 2.0,
                lo: a.x.min(b.x),
                hi: a.x.max(b.x),
            });
        } else if dx <= tol && dy > dx {
            // Vertical segment at x = midpoint.
            v.push(Segment {
                pos: (a.x + b.x) / 2.0,
                lo: a.y.min(b.y),
                hi: a.y.max(b.y),
            });
        }
    };

    for d in drawings {
        for it in &d.items {
            match *it {
                PathItem::Line(a, b) => push_line(a, b),
                PathItem::Rect(r) => {
                    let r = r.normalize();
                    if r.height() <= tol && r.width() > r.height() {
                        // A thin horizontal bar → one horizontal rule.
                        push_line(
                            Point::new(r.x0, (r.y0 + r.y1) / 2.0),
                            Point::new(r.x1, (r.y0 + r.y1) / 2.0),
                        );
                    } else if r.width() <= tol && r.height() > r.width() {
                        // A thin vertical bar → one vertical rule.
                        push_line(
                            Point::new((r.x0 + r.x1) / 2.0, r.y0),
                            Point::new((r.x0 + r.x1) / 2.0, r.y1),
                        );
                    } else {
                        // A box outline → its four edges.
                        push_line(Point::new(r.x0, r.y0), Point::new(r.x1, r.y0));
                        push_line(Point::new(r.x0, r.y1), Point::new(r.x1, r.y1));
                        push_line(Point::new(r.x0, r.y0), Point::new(r.x0, r.y1));
                        push_line(Point::new(r.x1, r.y0), Point::new(r.x1, r.y1));
                    }
                }
                PathItem::Curve(..) => {} // curves are not rules
            }
        }
    }

    h.retain(|s| s.len() >= opt.min_line_length);
    v.retain(|s| s.len() >= opt.min_line_length);
    (h, v)
}

/// Clusters a stream of 1-D positions into snapped grid-line coordinates within
/// `tol`, returned sorted ascending.
fn snap_positions(positions: impl Iterator<Item = f64>, tol: f64) -> Vec<f64> {
    let mut vals: Vec<f64> = positions.collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f64> = Vec::new();
    let mut cluster: Vec<f64> = Vec::new();
    for v in vals {
        match cluster.last() {
            Some(&last) if (v - last).abs() <= tol => cluster.push(v),
            _ => {
                if !cluster.is_empty() {
                    out.push(mean(&cluster));
                }
                cluster.clear();
                cluster.push(v);
            }
        }
    }
    if !cluster.is_empty() {
        out.push(mean(&cluster));
    }
    out
}

/// Builds the row-major cell grid: a cell exists where all four bounding edges
/// are at least partially covered by a ruling segment.
fn build_cells(
    rows: &[f64],
    cols: &[f64],
    h_segs: &[Segment],
    v_segs: &[Segment],
    opt: &TableOptions,
) -> Vec<Vec<Option<Rect>>> {
    let tol = opt.snap_tolerance;
    let mut grid = Vec::with_capacity(rows.len().saturating_sub(1));
    for ri in 0..rows.len() - 1 {
        let (y0, y1) = (rows[ri], rows[ri + 1]);
        let mut row = Vec::with_capacity(cols.len().saturating_sub(1));
        for ci in 0..cols.len() - 1 {
            let (x0, x1) = (cols[ci], cols[ci + 1]);
            let top = edge_covered(h_segs, y0, x0, x1, tol);
            let bottom = edge_covered(h_segs, y1, x0, x1, tol);
            let left = edge_covered(v_segs, x0, y0, y1, tol);
            let right = edge_covered(v_segs, x1, y0, y1, tol);
            if top && bottom && left && right {
                row.push(Some(Rect::new(x0, y0, x1, y1)));
            } else {
                row.push(None);
            }
        }
        grid.push(row);
    }
    grid
}

/// Whether some segment at constant `pos` (±tol) spans most of `[lo, hi]`.
fn edge_covered(segs: &[Segment], pos: f64, lo: f64, hi: f64, tol: f64) -> bool {
    let need = (hi - lo) * 0.5; // tolerate gaps: half the span must be covered
    let mut covered = 0.0;
    for s in segs {
        if (s.pos - pos).abs() > tol {
            continue;
        }
        let a = s.lo.max(lo - tol);
        let b = s.hi.min(hi + tol);
        if b > a {
            covered += b - a;
        }
    }
    covered >= need
}

// === text strategy ========================================================

/// Detects a table by clustering words into a column/row grid.
fn detect_text(words: &[Word], opt: &TableOptions) -> Vec<Table> {
    if words.len() < 2 {
        return Vec::new();
    }
    let gap = opt.text_gap();

    // Row bands: cluster words by vertical center, tracking each band's full
    // vertical extent `[min(y0), max(y1)]` so cells are tall enough to contain
    // their word centers.
    let mut rows = cluster_extents(words, center_y, |w| (w.bbox.y0, w.bbox.y1), gap);
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // Column bands: cluster by left edge (x0, stable for left-aligned columns),
    // tracking each band's full horizontal extent `[min(x0), max(x1)]`.
    let mut cols = cluster_extents(words, |w| w.bbox.x0, |w| (w.bbox.x0, w.bbox.x1), gap);
    cols.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // A real grid needs at least 2 columns and 2 rows.
    if rows.len() < 2 || cols.len() < 2 {
        return Vec::new();
    }

    // Build grid-line coordinates: midpoints between adjacent band extents, plus
    // outer margins, so cells fully tile the table bbox.
    let row_lines = band_boundaries(&rows, 1.0);
    let col_lines = band_boundaries(&cols, 1.0);

    let bbox = Rect::new(
        col_lines[0],
        row_lines[0],
        col_lines[col_lines.len() - 1],
        row_lines[row_lines.len() - 1],
    );
    let row_count = row_lines.len() - 1;
    let col_count = col_lines.len() - 1;

    // Presence: a slot is present when at least one word center lies inside it.
    let mut presence = vec![vec![false; col_count]; row_count];
    let mut any = false;
    for (ri, prow) in presence.iter_mut().enumerate() {
        for (ci, slot) in prow.iter_mut().enumerate() {
            let rect = Rect::new(
                col_lines[ci],
                row_lines[ri],
                col_lines[ci + 1],
                row_lines[ri + 1],
            );
            if words.iter().any(|w| rect.contains_point(word_center(w))) {
                any = true;
                *slot = true;
            }
        }
    }
    if !any {
        return Vec::new();
    }

    // Span splits: a boundary between two adjacent slots is considered *missing*
    // (so the slots merge) when a single word's bbox straddles that boundary
    // within the shared band — i.e. one word spans more than one column/row.
    let col_split = |ri: usize, ci: usize| {
        // Vertical boundary x = col_lines[ci+1]; row band rows[ri].
        let x = col_lines[ci + 1];
        let (y0, y1) = (row_lines[ri], row_lines[ri + 1]);
        // Boundary is a real split unless some word straddles it inside the band.
        !words.iter().any(|w| {
            let cy = center_y(w);
            cy > y0
                && cy < y1
                && w.bbox.x0 < x - opt.snap_tolerance
                && w.bbox.x1 > x + opt.snap_tolerance
        })
    };
    let row_split = |ri: usize, ci: usize| {
        // Horizontal boundary y = row_lines[ri+1]; col band cols[ci].
        let y = row_lines[ri + 1];
        let (x0, x1) = (col_lines[ci], col_lines[ci + 1]);
        !words.iter().any(|w| {
            let cx = (w.bbox.x0 + w.bbox.x1) / 2.0;
            cx > x0
                && cx < x1
                && w.bbox.y0 < y - opt.snap_tolerance
                && w.bbox.y1 > y + opt.snap_tolerance
        })
    };

    let (spans, cells) = merge_spans(
        &row_lines, &col_lines, &presence, row_count, col_count, col_split, row_split,
    );

    vec![Table {
        bbox,
        row_count,
        col_count,
        cols: col_lines,
        rows: row_lines,
        cells,
        spans,
        header: Vec::new(),
    }]
}

/// A 1-D band: `(key_min, extent_min, extent_max)`. `key` is the clustering key
/// (sorted on); `extent` is the full member span used for cell boundaries.
type Band = (f64, f64, f64);

/// Clusters words into bands by a 1-D `key`, splitting when consecutive sorted
/// keys gap by more than `gap`. Each band records the full member `extent`
/// (min/max of the per-word `(lo, hi)` extents), so cells span the real word
/// footprint rather than just the clustering key.
fn cluster_extents(
    words: &[Word],
    key: impl Fn(&Word) -> f64,
    extent: impl Fn(&Word) -> (f64, f64),
    gap: f64,
) -> Vec<Band> {
    let mut items: Vec<(f64, f64, f64)> = words
        .iter()
        .map(|w| {
            let (lo, hi) = extent(w);
            (key(w), lo, hi)
        })
        .collect();
    items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut bands: Vec<Band> = Vec::new();
    let mut iter = items.into_iter();
    let Some((k0, lo0, hi0)) = iter.next() else {
        return bands;
    };
    let mut key_min = k0;
    let mut last_key = k0;
    let mut ext_lo = lo0;
    let mut ext_hi = hi0;
    for (k, lo, hi) in iter {
        if k - last_key > gap {
            bands.push((key_min, ext_lo, ext_hi));
            key_min = k;
            ext_lo = lo;
            ext_hi = hi;
        } else {
            ext_lo = ext_lo.min(lo);
            ext_hi = ext_hi.max(hi);
        }
        last_key = k;
    }
    bands.push((key_min, ext_lo, ext_hi));
    bands
}

/// Converts a sorted list of bands into grid-line boundaries: an outer margin
/// before the first / after the last band's extent, and the midpoint between
/// adjacent band extents as the interior boundaries.
fn band_boundaries(bands: &[Band], margin: f64) -> Vec<f64> {
    let mut lines = Vec::with_capacity(bands.len() + 1);
    lines.push(bands[0].1 - margin);
    for w in bands.windows(2) {
        lines.push((w[0].2 + w[1].1) / 2.0);
    }
    lines.push(bands[bands.len() - 1].2 + margin);
    lines
}

// === shared helpers =======================================================

/// The vertical center of a word.
fn center_y(w: &Word) -> f64 {
    (w.bbox.y0 + w.bbox.y1) / 2.0
}

/// The center point of a word.
fn word_center(w: &Word) -> Point {
    Point::new((w.bbox.x0 + w.bbox.x1) / 2.0, (w.bbox.y0 + w.bbox.y1) / 2.0)
}

/// The joined text of all words whose center lies inside `rect`, in reading
/// order (top→bottom, then left→right). `None` when the cell holds no word.
fn cell_text(rect: Rect, words: &[Word]) -> Option<String> {
    let mut hits: Vec<&Word> = words
        .iter()
        .filter(|w| rect.contains_point(word_center(w)))
        .collect();
    if hits.is_empty() {
        return None;
    }
    hits.sort_by(|a, b| {
        let ay = center_y(a);
        let by = center_y(b);
        ay.partial_cmp(&by)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.bbox
                    .x0
                    .partial_cmp(&b.bbox.x0)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    // Join words on the same line by space, different lines by space too
    // (cell text reads as a single string, matching PyMuPDF's cell text).
    let text = hits
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Some(text)
}

/// The HTML-escaped, multi-line cell text for `rect`: words grouped into lines
/// by vertical proximity (a new line starts when a word's center drops below the
/// running line baseline by more than half the line's height), words within a
/// line joined by a space, lines joined by `<br>`. Each word's text is
/// HTML-escaped (`& < > "`). `None` when the cell holds no word.
fn cell_html_text(rect: Rect, words: &[Word]) -> Option<String> {
    let mut hits: Vec<&Word> = words
        .iter()
        .filter(|w| rect.contains_point(word_center(w)))
        .collect();
    if hits.is_empty() {
        return None;
    }
    // Reading order: top→bottom, then left→right.
    hits.sort_by(|a, b| {
        center_y(a)
            .partial_cmp(&center_y(b))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.bbox
                    .x0
                    .partial_cmp(&b.bbox.x0)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    // Group into lines: a new line begins when the next word's vertical center
    // separates from the current line's center by more than half its height.
    let mut lines: Vec<Vec<&Word>> = Vec::new();
    let mut cur_center = center_y(hits[0]);
    let mut cur: Vec<&Word> = Vec::new();
    for w in hits {
        let cy = center_y(w);
        let half_h = (w.bbox.y1 - w.bbox.y0).max(1.0) / 2.0;
        if !cur.is_empty() && (cy - cur_center).abs() > half_h {
            lines.push(std::mem::take(&mut cur));
            cur_center = cy;
        } else if cur.is_empty() {
            cur_center = cy;
        }
        cur.push(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("<br>");
        }
        let mut sorted = line.clone();
        sorted.sort_by(|a, b| {
            a.bbox
                .x0
                .partial_cmp(&b.bbox.x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (j, w) in sorted.iter().enumerate() {
            if j > 0 {
                out.push(' ');
            }
            html_escape_into(&mut out, &w.text);
        }
    }
    Some(out)
}

/// Escapes text for HTML element content (mirrors `serialize.rs` conventions,
/// plus `"` so the output is safe to drop anywhere in cell content).
fn html_escape_into(s: &mut String, raw: &str) {
    for c in raw.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            c => s.push(c),
        }
    }
}

/// The arithmetic mean of a non-empty slice.
fn mean(vals: &[f64]) -> f64 {
    vals.iter().sum::<f64>() / vals.len() as f64
}
