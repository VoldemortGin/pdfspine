#![forbid(unsafe_code)]
//! `pdf-typeset` — the shared typesetting engine behind faithful `.docx` /
//! `.pptx` → PDF **export** (PRD §10, Phase A).
//!
//! This is an **original pdfspine extension**: docspine's `doc-render` and
//! pptspine's `ppt-render` build a [`model`] document (styled runs, paragraph
//! properties, tables, text boxes) and this crate resolves fonts, lays the
//! content out and emits PDF ops through `pdf-edit`. It is deliberately not a
//! Story / HTML+CSS engine and does no shaping/kerning/ligatures — advances
//! stay strictly additive per character.
//!
//! # Phase A module map (PRD §10 design sketch)
//!
//! - [`model`] — the layout-ready input IR (TS-1, this crate's public input).
//! - [`warn`] — [`ExportWarning`]: every unsupported-feature degradation is
//!   enumerated, degrade-never-panic (TS-1).
//! - [`ops`] — the positioned draw-op IR shared by the layout stages (TS-1;
//!   generalizes `pdf-markdown`'s op vocabulary with size-carrying text per
//!   [`ops::FaceId`] plus shape / alpha / clip / transform ops).
//! - [`fontres`] — fontdb-backed system-font resolution: folded-name index,
//!   three-platform substitution tables, weight/style query mapping, per-char
//!   fallback chain, bundled Liberation/Noto final fallback (TS-2).
//! - `faces` / `flow` / `boxes` / `preset` / `table` / `emit` — TS-3..TS-6.
//!
//! # Determinism contract
//!
//! Weaker than `pdf-markdown`'s absolute one: same machine + same installed
//! fonts ⇒ identical bytes; cross-machine output may differ (system-font
//! resolution). The [`fontres::FontResolver::without_system_fonts`] constructor
//! (bundled faces only) restores full determinism for tests.

pub mod fontres;
pub mod model;
pub mod ops;
pub mod warn;

// --- re-exported consumer surface (single pdfspine git dep for doc-render /
// --- ppt-render; PRD §10 consumer-wiring precedent) -------------------------
pub use pdf_core::error::{Error, Result};
pub use pdf_core::geom::{Matrix, Point, Rect};
/// The RGB color type used across the input model and op IR (each component in
/// `0.0..=1.0`; `pdf_edit::Color` re-exported under the PRD §10 model name).
pub use pdf_edit::Color as Rgb;

pub use fontres::{FontResolver, Platform, ResolvedFace, Substitutions};
pub use model::{
    Align, Block, BorderEdge, CellBorders, ColumnWidth, ImageSpec, LineSpacing, ListLabel,
    ParaProps, Run, RunStyle, TableCell, TableRow, TableSpec, TextBoxSpec, VAnchor,
};
pub use warn::ExportWarning;

/// The result of one export run: the serialized PDF plus every degradation
/// that occurred while producing it (consumers surface these in Python via
/// `warnings.warn`).
#[derive(Clone, Debug)]
pub struct ExportResult {
    /// The serialized PDF bytes.
    pub pdf: Vec<u8>,
    /// Every unsupported-feature degradation, in occurrence order.
    pub warnings: Vec<ExportWarning>,
}
