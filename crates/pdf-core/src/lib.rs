#![forbid(unsafe_code)]
//! `pdf-core` — oxipdf core: object model, lexer/parser, xref (table + stream),
//! trailer, repair, filters, writer, `DocumentStore`. No domain logic.
//!
//! In M0 only the [`geom`] module is implemented (PyMuPDF-compatible geometry
//! value types). Parsing, filters and the writer land in M1+ per PRD §7.

pub mod geom;
