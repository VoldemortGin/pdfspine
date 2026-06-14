#![forbid(unsafe_code)]
//! `pdf-edit` — page operations, `insert_pdf`, annotations/links/forms, content
//! emission, metadata/TOC, redaction.
//!
//! M3c implements page-tree editing ([`page_ops`]) and document merge / split
//! ([`merge`]). M3d adds metadata write ([`metadata`]), table of contents
//! ([`toc`]), link annotations ([`links`]), named-destination resolution
//! ([`dest`]) and page labels ([`pagelabel`]) — all over a
//! [`pdf_core::DocumentStore`] via its ChangeSet object-edit API.
//! Annotations/forms/redaction (M4) land later (PRD §7).

pub mod dest;
pub mod links;
pub mod merge;
pub mod metadata;
pub mod page_ops;
pub mod pagelabel;
pub mod toc;

pub use dest::{resolve_link, resolve_named};
pub use links::{delete_link, get_links, insert_link, update_link, Link, LinkKind};
pub use merge::{extract_pages, insert_pdf, InsertOptions};
pub use metadata::{get_xml_metadata, set_metadata, set_xml_metadata};
pub use page_ops::PageEditor;
pub use pagelabel::get_label;
pub use toc::{get_toc, set_toc, TocEntry};
