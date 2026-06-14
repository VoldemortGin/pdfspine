#![forbid(unsafe_code)]
//! `pdf-text` — content-stream interpreter (text), `TextPage`, all `get_text`
//! output formats, and `search_for`.
//!
//! M2b implements the **content-stream interpreter** (PRD §8.6): the
//! [`ContentInterpreter`] runs a page's decoded content stream(s) over a
//! graphics-state machine and emits a flat [`InterpretResult`] of positioned
//! glyphs (in PDF user space) plus an image inventory. Layout grouping
//! (spans/lines/blocks, M2c) and the `get_text` serializers / page transform
//! (M2d) build on top of this list.
//!
//! Entry points:
//! - [`interpret_page`] — run a page dictionary (resolves `/Contents` +
//!   `/Resources`).
//! - [`interpret_content`] — run an explicit `(content, resources, ctm)` triple
//!   (the form-recursion / testing entry point).

pub mod interp;
pub mod model;
pub mod state;
pub mod tokenizer;

use pdf_core::geom::Matrix;
use pdf_core::{Dict, DocumentStore};

pub use interp::ContentInterpreter;
pub use model::{ImageRef, InterpretResult, PositionedGlyph, WritingDir};

/// Interprets a page dictionary into positioned glyphs + an image inventory
/// (PDF user space). Convenience wrapper over [`ContentInterpreter::run_page`].
#[must_use]
pub fn interpret_page(doc: &DocumentStore, page: &Dict) -> InterpretResult {
    ContentInterpreter::new(doc).run_page(page)
}

/// Interprets an explicit content buffer + resource dict under `base_ctm`.
/// Convenience wrapper over [`ContentInterpreter::run_content`].
#[must_use]
pub fn interpret_content(
    doc: &DocumentStore,
    content: &[u8],
    resources: &Dict,
    base_ctm: Matrix,
) -> InterpretResult {
    ContentInterpreter::new(doc).run_content(content, resources, base_ctm)
}
