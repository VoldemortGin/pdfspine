#![forbid(unsafe_code)]
//! `pdf-edit` — page operations, `insert_pdf`, annotations/links/forms, content
//! emission, metadata/TOC, redaction.
//!
//! M3c implements page-tree editing ([`page_ops`]) and document merge / split
//! ([`merge`]) over a [`pdf_core::DocumentStore`] via its ChangeSet object-edit
//! API. Annotations/forms/redaction (M4) and metadata/TOC (M3d) land later
//! (PRD §7).

pub mod merge;
pub mod page_ops;

pub use merge::{extract_pages, insert_pdf, InsertOptions};
pub use page_ops::PageEditor;
