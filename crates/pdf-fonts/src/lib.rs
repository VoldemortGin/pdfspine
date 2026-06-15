#![forbid(unsafe_code)]
//! `pdf-fonts` — font *mapping* for oxide-pdf (PRD §8.5): char-code → Unicode and
//! char-code → width. **No rasterization** (that is M6).
//!
//! The public surface is the [`FontMapper`], built from a resolved font
//! dictionary plus a `&DocumentStore`:
//!
//! - [`FontMapper::iter_codes`] walks a show-string into `(code, n_bytes)`
//!   pairs — 1 byte for simple fonts, codespace-driven for Type0.
//! - [`FontMapper::to_unicode`] resolves a code to its Unicode string
//!   (`/ToUnicode` overrides; else encoding + Adobe Glyph List for simple
//!   fonts).
//! - [`FontMapper::width`] returns the glyph advance in 1000-unit text space.
//!
//! Supporting modules are public so the M2b text interpreter and tests can use
//! them directly: [`encodings`] (base encodings), [`glyphlist`] (AGL +
//! algorithmic glyph-name rules), [`cmap`] (the shared CMap parser),
//! [`predefined`] (the predefined-CJK-CMap framework) and [`widths`].
//!
//! ## Bundled data & license provenance (PRD §6.5 #2 / §10.3)
//!
//! - **Adobe Glyph List** (`data/glyphlist.txt`, **BSD-3-Clause**, Adobe) is
//!   embedded verbatim — see `data/PROVENANCE.md` and `data/NOTICE`.
//! - **Core-14 standard advance widths** are a built-in factual-metrics table
//!   ([`std_widths`]): the numeric AFM `WX` values of the 14 standard typefaces
//!   (ISO 32000-1 §9.6.2.2). Numeric metric facts are not copyrightable
//!   expression — see `data/PROVENANCE.md`. These advances feed both
//!   `insert_text` (placement) and **text extraction**: a base-14 simple font
//!   lacking a `/Widths` array resolves each code → glyph name → standard
//!   advance via [`widths::core14_width`] (see [`FontMapper::width`]).

pub mod cmap;
pub mod encodings;
pub mod glyphlist;
pub mod mapper;
pub mod predefined;
pub mod std_widths;
pub mod widths;

pub use cmap::{CMap, CodespaceRange};
pub use encodings::BaseEncoding;
pub use mapper::{CodeIter, FontKind, FontMapper};
pub use std_widths::{standard_font_widths, string_advance, StandardWidths};
