#![forbid(unsafe_code)]
//! `pdf-core` — oxipdf core: object model, lexer/parser, xref (table + stream),
//! trailer, repair, filters, writer, `DocumentStore`. No domain logic.
//!
//! M0 implemented the [`geom`] module (PyMuPDF-compatible geometry value types).
//! M1a adds the [`lexer`] (byte tokenizer), the [`object`] model + parser, the
//! [`serialize`] writer and the core [`Error`] type. M1b adds the [`filters`]
//! codec layer (Flate/LZW/ASCII*/RunLength + predictors + a stream-decode
//! dispatcher) and the [`Limits`] resource ceilings. M1c adds the [`source`]
//! backing-bytes abstraction, the [`xref`] cross-reference machinery,
//! [`objstm`] object-stream decoding, name [`interner`]ing and the lazy
//! [`document`]`::DocumentStore`. M1d adds the [`repair`] subsystem
//! (full-file object scan / synthetic xref / trailer reconstruction) wired into
//! the document open path as a fallback, plus the `Strict`/`Lenient`
//! [`repair::ParseMode`]. M1e adds transparent decryption behind the
//! `encryption` feature: the `encrypt` module parses `/Encrypt` into a
//! `pdf-crypto` handler and the `DocumentStore` decrypts strings/streams in
//! `resolve()` once authenticated. Pages land in M1f per PRD §7.

pub mod changeset;
pub mod document;
#[cfg(feature = "encryption")]
pub mod encrypt;
pub mod error;
pub mod filters;
pub mod gc;
pub mod geom;
pub mod interner;
pub mod lexer;
pub mod limits;
pub mod object;
pub mod objstm;
pub mod page;
pub mod pagetree;
pub mod repair;
pub mod serialize;
pub mod source;
pub mod writer;
pub mod xref;

pub use changeset::{Change, ChangeSet};
pub use document::{DocumentStore, Version};
pub use error::{Error, LimitKind, Result};
pub use filters::{decode_stream, DecodeOutcome};
pub use interner::NameInterner;
pub use limits::Limits;
pub use object::{Dict, Name, ObjRef, Object, PdfString, StreamData, StreamObj, StringKind};
pub use page::Page;
pub use repair::{ParseMode, RepairAction, RepairKind, Warning, WarningKind};
pub use source::{MmapMode, Source};
pub use writer::{OnRepaired, SaveOptions, XrefStyle};
pub use xref::{XrefEntry, XrefTable};
