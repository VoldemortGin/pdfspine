//! Table-detection facade (M7) — `Page::find_tables` assembles the
//! textpage + device-space words + device-space drawings the
//! [`pdf_text::tables`] finder needs, and surfaces an owned [`TableFinder`] /
//! [`Table`] pair the bindings can hold across the FFI boundary.
//!
//! The orphan rule forbids inherent `impl Page` here (`Page` is `pdf-core`'s),
//! so the entry point is the free function [`page_find_tables`]. The returned
//! [`Table`] owns its source `words` so PyMuPDF's argument-free
//! `table.extract()` / `to_markdown()` / `to_html()` work without the caller
//! re-supplying the word list.

use std::sync::Arc;

use pdf_core::geom::Rect;
use pdf_core::page::Page;

use pdf_text::tables as core_tables;
use pdf_text::Word;

pub use pdf_text::tables::{CellSpan, Strategy};

/// The PyMuPDF-style table-detection request (PRD §7 / M7). Built from the
/// Python kwargs; maps onto [`core_tables::TableOptions`].
#[derive(Clone, Copy, Debug)]
pub struct TableOptions {
    /// The grid-detection strategy.
    pub strategy: Strategy,
    /// Max stroke thickness still treated as a ruling line (device units).
    pub line_max_thickness: f64,
    /// Grid-line snapping tolerance (device units).
    pub snap_tolerance: f64,
    /// Minimum ruling length to count as a grid line (device units).
    pub min_line_length: f64,
}

impl Default for TableOptions {
    fn default() -> Self {
        let d = core_tables::TableOptions::default();
        TableOptions {
            strategy: d.strategy,
            line_max_thickness: d.line_max_thickness,
            snap_tolerance: d.snap_tolerance,
            min_line_length: d.min_line_length,
        }
    }
}

impl TableOptions {
    fn to_core(self) -> core_tables::TableOptions {
        core_tables::TableOptions {
            strategy: self.strategy,
            line_max_thickness: self.line_max_thickness,
            snap_tolerance: self.snap_tolerance,
            min_line_length: self.min_line_length,
        }
    }
}

/// Maps a PyMuPDF strategy string (`"lines"`, `"lines_strict"`, `"text"`) to a
/// [`Strategy`]. Unknown strings fall back to the default (`Lines`), matching
/// PyMuPDF's lenient handling.
#[must_use]
pub fn strategy_from_str(s: &str) -> Strategy {
    match s.to_ascii_lowercase().as_str() {
        "lines_strict" => Strategy::LinesStrict,
        "text" => Strategy::Text,
        // "lines" and anything else → the default grid-from-rulings strategy.
        _ => Strategy::Lines,
    }
}

/// One detected table (PyMuPDF `Table`). Owns the [`pdf_text::tables::Table`]
/// geometry plus the page's device-space `words`, so `extract` / `to_markdown` /
/// `to_html` need no further arguments (the core methods recompute cell text
/// from the words each call).
#[derive(Clone)]
pub struct Table {
    inner: core_tables::Table,
    words: Arc<[Word]>,
}

impl Table {
    /// The table's bounding box in device space (PyMuPDF `Table.bbox`).
    #[must_use]
    pub fn bbox(&self) -> Rect {
        self.inner.bbox
    }

    /// The number of cell rows (PyMuPDF `Table.row_count`).
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.inner.row_count
    }

    /// The number of cell columns (PyMuPDF `Table.col_count`).
    #[must_use]
    pub fn col_count(&self) -> usize {
        self.inner.col_count
    }

    /// The header row's cell text, when a header row was detected (PyMuPDF
    /// `Table.header`); empty otherwise.
    #[must_use]
    pub fn header(&self) -> &[Option<String>] {
        &self.inner.header
    }

    /// The snapped horizontal grid-line y positions, top→bottom (PyMuPDF
    /// `Table.rows` grid coords).
    #[must_use]
    pub fn rows(&self) -> &[f64] {
        &self.inner.rows
    }

    /// The snapped vertical grid-line x positions, left→right (PyMuPDF
    /// `Table.cols` grid coords).
    #[must_use]
    pub fn cols(&self) -> &[f64] {
        &self.inner.cols
    }

    /// The per-slot cell rectangles, row-major; `None` for an absent slot or a
    /// merge-continuation slot (PyMuPDF `Table.cells`).
    #[must_use]
    pub fn cells(&self) -> &[Vec<Option<Rect>>] {
        &self.inner.cells
    }

    /// One [`CellSpan`] per originating (top-left) cell (PyMuPDF `Table.spans`).
    #[must_use]
    pub fn spans(&self) -> &[CellSpan] {
        &self.inner.spans
    }

    /// The cell text grid, row-major; `None` for an empty / continuation slot
    /// (PyMuPDF `Table.extract`).
    #[must_use]
    pub fn extract(&self) -> Vec<Vec<Option<String>>> {
        self.inner.extract(&self.words)
    }

    /// The table as a GitHub-Flavored-Markdown string (PyMuPDF
    /// `Table.to_markdown`).
    #[must_use]
    pub fn to_markdown(&self) -> String {
        self.inner.to_markdown(&self.words)
    }

    /// The table as an HTML `<table>` string, with `colspan`/`rowspan` for
    /// merged cells (pdfspine extra; PyMuPDF has no `to_html`).
    #[must_use]
    pub fn to_html(&self) -> String {
        self.inner.to_html(&self.words)
    }
}

/// A page's detected tables (PyMuPDF `TableFinder`). Owns the [`Table`]s.
#[derive(Clone, Default)]
pub struct TableFinder {
    /// The detected tables, in detection order.
    pub tables: Vec<Table>,
}

impl TableFinder {
    /// The number of detected tables (PyMuPDF `len(finder.tables)`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// Whether no tables were detected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// Detects the tables on `page` under `opts` (PyMuPDF `Page.find_tables`).
///
/// Assembles the inputs the [`pdf_text::tables`] finder needs: the page
/// [`TextPage`](pdf_text::TextPage), its device-space `words`, and — for the
/// line strategies — the page's drawings mapped into device space via
/// [`pdf_text::tables::drawings_to_device`]. The `Text` strategy ignores the
/// drawings. The returned [`Table`]s each carry an `Arc` clone of the word list.
#[must_use]
pub fn page_find_tables(page: &Page, opts: &TableOptions) -> TableFinder {
    let doc = page.document();
    let Some(page_dict) = page.dict() else {
        return TableFinder::default();
    };

    let tp = crate::text::textpage(page, 0, None);
    let words: Arc<[Word]> = Arc::from(pdf_text::words(&tp));

    // Device-space drawings: interpret the page, then map user-space → device.
    let res = pdf_text::interpret_page(doc, &page_dict);
    let pt = pdf_text::page_transform(page.cropbox(), page.rotation());
    let drawings = core_tables::drawings_to_device(&res.drawings, &pt);

    let finder = core_tables::find_tables(&tp, &words, &drawings, &opts.to_core());
    let tables = finder
        .tables
        .into_iter()
        .map(|inner| Table {
            inner,
            words: Arc::clone(&words),
        })
        .collect();
    TableFinder { tables }
}
