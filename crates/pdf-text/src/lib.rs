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
pub mod layout;
pub mod model;
pub mod renderops;
pub mod search;
pub mod serialize;
pub mod state;
pub mod tables;
pub mod tokenizer;
pub mod words;

use pdf_core::geom::Matrix;
use pdf_core::{Dict, DocumentStore};

pub use interp::ContentInterpreter;
pub use layout::{build_textpage, page_size, page_transform, textpage_from_glyphs};
pub use model::{
    flags, Block, BlockKind, Char, DrawPath, ImageBlock, ImageRef, InterpretResult, Line,
    PaintKind, PathItem, PositionedGlyph, Span, TextPage, Word, WritingDir,
};
pub use renderops::{ImageOp, RenderOp, RenderSink, ShadingOp, TextRun};
pub use search::{search, SearchOptions};
pub use serialize::{
    defaults, get_textbox, textflags, to_blocks, to_dict, to_html, to_json, to_text, to_words,
    to_xhtml, to_xml, BlockTuple, DictBlock, DictChar, DictImageBlock, DictLine, DictSpan,
    DictTextBlock, TextDict, WordTuple,
};
pub use words::words;

/// Interprets a page dictionary into positioned glyphs + an image inventory
/// (PDF user space). Convenience wrapper over [`ContentInterpreter::run_page`].
#[must_use]
pub fn interpret_page(doc: &DocumentStore, page: &Dict) -> InterpretResult {
    ContentInterpreter::new(doc).run_page(page)
}

/// Interprets a page dictionary into the **ordered** [`RenderOp`] stream (the M6
/// render driver / `DisplayList` source). Document order is preserved so later
/// drawcalls paint over earlier ones (z-order). Convenience wrapper over
/// [`ContentInterpreter::run_page_render`].
#[must_use]
pub fn interpret_page_render(doc: &DocumentStore, page: &Dict) -> Vec<RenderOp> {
    ContentInterpreter::new_recording(doc).run_page_render(page)
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
