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
//! line positions, plus [`Table::extract`] and [`Table::to_markdown`].

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
    /// The cell rectangles in row-major order. `None` marks a missing cell (a
    /// merged / absent grid slot). Outer `Vec` is rows, inner is columns.
    pub cells: Vec<Vec<Option<Rect>>>,
    /// The detected header: the first row's cell text, when a plausible header
    /// row is present (else empty).
    pub header: Vec<Option<String>>,
}

impl Table {
    /// Extracts the cell text in reading order: `rows[r][c]` is the joined text
    /// of all [`Word`]s whose center lies inside cell `(r, c)`, or `None` for an
    /// empty / missing cell.
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
    vec![Table {
        bbox,
        row_count,
        col_count,
        cols,
        rows,
        cells,
        header: Vec::new(), // populated by `find_tables` (needs the words)
    }]
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

    // Build cells; a text cell always exists (text strategy tiles fully), but we
    // mark a cell None when no word centers inside it.
    let mut cells = Vec::with_capacity(row_count);
    let mut any = false;
    for ri in 0..row_count {
        let mut row = Vec::with_capacity(col_count);
        for ci in 0..col_count {
            let rect = Rect::new(
                col_lines[ci],
                row_lines[ri],
                col_lines[ci + 1],
                row_lines[ri + 1],
            );
            if words.iter().any(|w| rect.contains_point(word_center(w))) {
                any = true;
                row.push(Some(rect));
            } else {
                row.push(None);
            }
        }
        cells.push(row);
    }
    if !any {
        return Vec::new();
    }

    vec![Table {
        bbox,
        row_count,
        col_count,
        cols: col_lines,
        rows: row_lines,
        cells,
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

/// The arithmetic mean of a non-empty slice.
fn mean(vals: &[f64]) -> f64 {
    vals.iter().sum::<f64>() / vals.len() as f64
}
