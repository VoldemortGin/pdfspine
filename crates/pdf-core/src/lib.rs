#![forbid(unsafe_code)]
//! `pdf-core` — oxipdf core: object model, lexer/parser, xref (table + stream),
//! trailer, repair, filters, writer, `DocumentStore`. No domain logic.
//!
//! M0 implemented the [`geom`] module (PyMuPDF-compatible geometry value types).
//! M1a adds the [`lexer`] (byte tokenizer), the [`object`] model + parser, the
//! [`serialize`] writer and the core [`Error`] type. M1b adds the [`filters`]
//! codec layer (Flate/LZW/ASCII*/RunLength + predictors + a stream-decode
//! dispatcher) and the [`Limits`] resource ceilings. Xref, encryption and pages
//! land in later M1 units per PRD §7.

pub mod error;
pub mod filters;
pub mod geom;
pub mod lexer;
pub mod limits;
pub mod object;
pub mod serialize;

pub use error::{Error, LimitKind, Result};
pub use filters::{decode_stream, DecodeOutcome};
pub use limits::Limits;
pub use object::{Dict, Name, ObjRef, Object, PdfString, StreamData, StreamObj, StringKind};
