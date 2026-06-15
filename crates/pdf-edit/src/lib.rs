#![forbid(unsafe_code)]
//! `pdf-edit` — page operations, `insert_pdf`, annotations/links/forms, content
//! emission, metadata/TOC, redaction.
//!
//! M3c implements page-tree editing ([`page_ops`]) and document merge / split
//! ([`merge`]). M3d adds metadata write ([`metadata`]), table of contents
//! ([`toc`]), link annotations ([`links`]), named-destination resolution
//! ([`dest`]) and page labels ([`pagelabel`]) — all over a
//! [`pdf_core::DocumentStore`] via its ChangeSet object-edit API.
//! Annotations/forms/redaction (M4b–M4d) land later (PRD §7). M4a adds content
//! insertion: text ([`text`]), images ([`image`]) and vector drawing
//! ([`drawing`]) over the shared content plumbing in [`content`], plus user-TTF
//! font embedding ([`fontfile`]) (PRD §8.8 / §8.5.2).

pub mod annot;
pub mod color;
pub mod content;
pub mod dest;
pub mod drawing;
pub mod drawings;
pub mod embfile;
pub mod fontfile;
pub mod form;
pub mod image;
pub mod links;
pub mod merge;
pub mod metadata;
pub mod ocg;
pub mod page_ops;
pub mod pagelabel;
pub mod redact;
pub mod scrub;
pub mod text;
pub mod toc;

pub use annot::{
    add_circle_annot, add_file_annot, add_freetext_annot, add_highlight_annot, add_ink_annot,
    add_line_annot, add_polygon_annot, add_polyline_annot, add_rect_annot, add_redact_annot,
    add_squiggly_annot, add_stamp_annot, add_strikeout_annot, add_text_annot, add_underline_annot,
    annot_count, annot_names, annot_refs, annots, delete_annot, first_annot, Annot, AnnotType,
};
pub use color::Color;
pub use content::PageContent;
pub use dest::{resolve_link, resolve_named};
pub use drawing::{
    draw_bezier, draw_circle, draw_curve, draw_line, draw_oval, draw_polyline, draw_rect, Shape,
};
pub use drawings::{get_cdrawings, get_drawings, DrawItem, Drawing};
pub use embfile::{
    embfile_add, embfile_count, embfile_del, embfile_get, embfile_info, embfile_names, EmbfileInfo,
};
pub use fontfile::EmbeddedFont;
pub use form::{
    acroform_dict, default_appearance, fill, first_widget, flatten, form_fields, is_form_pdf,
    need_appearances, terminal_field_refs, widget_refs, widgets, Field, FieldType, Widget,
};
pub use image::{insert_image_jpeg, insert_image_rgb};
pub use links::{delete_link, get_links, insert_link, update_link, Link, LinkKind};
pub use merge::{extract_pages, insert_pdf, show_pdf_page, InsertOptions};
pub use metadata::{get_xml_metadata, set_metadata, set_xml_metadata};
pub use ocg::{add_ocg, set_layer, set_layer_state, set_oc};
pub use page_ops::PageEditor;
pub use pagelabel::{get_label, set_labels, LabelSpec};
pub use redact::apply_redactions;
pub use scrub::{bake, scrub, ScrubOptions};
pub use text::{insert_text, insert_textbox, Align, TextOptions};
pub use toc::{get_toc, set_toc, TocEntry};
