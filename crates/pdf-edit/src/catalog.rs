//! Document `/Catalog` state/meta accessors — the PyMuPDF `Document` "viewer
//! preferences + language + mark-info + XMP-stream-xref" surface (PRD §C
//! batch-5).
//!
//! These are thin, focused readers/writers over the catalog dictionary:
//! `/PageLayout`, `/PageMode`, `/Lang`, `/MarkInfo`, and `/Metadata`. They follow
//! the same `catalog_dict` → mutate → `update_object(root, …)` pattern used by
//! [`crate::metadata`] so writes survive a full save.
//!
//! `/Lang` is normalized exactly like MuPDF's `fz_text_language_from_string` /
//! `fz_string_from_text_language` round-trip (see [`normalize_language`]): the
//! tag is lossily packed to a 2- or 3-letter ISO-639 primary subtag (with the
//! `zh-Hant`/`zh-Hans` special cases), so e.g. `"en-US"` → `"en"`. This matches
//! the bytes fitz stores, not the caller's verbatim input.

use pdf_core::object::Name;
use pdf_core::{Dict, DocumentStore, Object, PdfString, StringKind};

/// The resolved catalog dictionary, if the document has a `/Root` dict.
fn catalog_dict(doc: &DocumentStore) -> Option<Dict> {
    let root = doc.root()?;
    doc.resolve(root).ok()?.as_dict().cloned()
}

/// Reads a catalog name-valued key (e.g. `/PageLayout`) without its leading
/// slash, or `None` when absent.
fn catalog_name(doc: &DocumentStore, key: &str) -> Option<String> {
    let cat = catalog_dict(doc)?;
    cat.get(&Name::new(key))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
}

/// Writes a catalog key to a value produced by `make`, or removes it when `make`
/// is `None`, then persists the catalog.
///
/// # Errors
///
/// [`pdf_core::Error::InvalidArgument`] when the document has no catalog;
/// propagates object-edit errors.
fn set_catalog_key(doc: &DocumentStore, key: &str, value: Option<Object>) -> pdf_core::Result<()> {
    let root = doc
        .root()
        .ok_or(pdf_core::Error::InvalidArgument("document has no /Root"))?;
    let mut cat = doc
        .resolve(root)?
        .as_dict()
        .cloned()
        .ok_or(pdf_core::Error::InvalidArgument(
            "/Root is not a dictionary",
        ))?;
    let name = Name::new(key);
    match value {
        Some(v) => {
            cat.insert(name, v);
        }
        None => {
            cat.remove(&name);
        }
    }
    doc.update_object(root, Object::Dictionary(cat))?;
    Ok(())
}

/// The catalog `/PageLayout` name (PyMuPDF `Document.pagelayout`), defaulting to
/// `"SinglePage"` when absent — the PDF default, which fitz also returns.
#[must_use]
pub fn page_layout(doc: &DocumentStore) -> String {
    catalog_name(doc, "PageLayout").unwrap_or_else(|| "SinglePage".to_string())
}

/// Sets the catalog `/PageLayout` (PyMuPDF `Document.set_pagelayout`).
///
/// # Errors
///
/// See [`set_catalog_key`].
pub fn set_page_layout(doc: &DocumentStore, layout: &str) -> pdf_core::Result<()> {
    set_catalog_key(doc, "PageLayout", Some(Object::Name(Name::new(layout))))
}

/// The catalog `/PageMode` name (PyMuPDF `Document.pagemode`), defaulting to
/// `"UseNone"` when absent — the PDF default, which fitz also returns.
#[must_use]
pub fn page_mode(doc: &DocumentStore) -> String {
    catalog_name(doc, "PageMode").unwrap_or_else(|| "UseNone".to_string())
}

/// Sets the catalog `/PageMode` (PyMuPDF `Document.set_pagemode`).
///
/// # Errors
///
/// See [`set_catalog_key`].
pub fn set_page_mode(doc: &DocumentStore, mode: &str) -> pdf_core::Result<()> {
    set_catalog_key(doc, "PageMode", Some(Object::Name(Name::new(mode))))
}

/// The catalog `/Lang` string (PyMuPDF `Document.language`), or `None` when
/// absent. Returned verbatim — fitz already stored the normalized form.
#[must_use]
pub fn language(doc: &DocumentStore) -> Option<String> {
    let cat = catalog_dict(doc)?;
    cat.get(&Name::new("Lang"))
        .and_then(Object::as_string)
        .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
}

/// Sets the catalog `/Lang` (PyMuPDF `Document.set_language`). The tag is
/// normalized via [`normalize_language`]; an empty/invalid tag removes `/Lang`.
///
/// # Errors
///
/// See [`set_catalog_key`].
pub fn set_language(doc: &DocumentStore, lang: &str) -> pdf_core::Result<()> {
    match normalize_language(lang) {
        Some(norm) => set_catalog_key(
            doc,
            "Lang",
            Some(Object::String(PdfString {
                bytes: norm.into_bytes(),
                kind: StringKind::Literal,
            })),
        ),
        None => set_catalog_key(doc, "Lang", None),
    }
}

/// The catalog `/MarkInfo` dict as `(Marked, UserProperties, Suspects)` booleans
/// when present (PyMuPDF `Document.markinfo`), or `None` when there is no
/// `/MarkInfo` — letting the caller distinguish "no MarkInfo" (`{}`) from "all
/// false".
#[must_use]
pub fn mark_info(doc: &DocumentStore) -> Option<(bool, bool, bool)> {
    let cat = catalog_dict(doc)?;
    let mi = match cat.get(&Name::new("MarkInfo"))? {
        Object::Dictionary(d) => d.clone(),
        Object::Reference(r) => doc.resolve(*r).ok()?.as_dict().cloned()?,
        _ => return None,
    };
    let b = |k: &str| {
        mi.get(&Name::new(k))
            .and_then(Object::as_bool)
            .unwrap_or(false)
    };
    Some((b("Marked"), b("UserProperties"), b("Suspects")))
}

/// Sets the catalog `/MarkInfo` dict from the three booleans (PyMuPDF
/// `Document.set_markinfo`). All three keys are always written, matching fitz.
///
/// # Errors
///
/// See [`set_catalog_key`].
pub fn set_mark_info(
    doc: &DocumentStore,
    marked: bool,
    user_properties: bool,
    suspects: bool,
) -> pdf_core::Result<()> {
    let mut mi = Dict::new();
    mi.insert(Name::new("Marked"), Object::Boolean(marked));
    mi.insert(
        Name::new("UserProperties"),
        Object::Boolean(user_properties),
    );
    mi.insert(Name::new("Suspects"), Object::Boolean(suspects));
    set_catalog_key(doc, "MarkInfo", Some(Object::Dictionary(mi)))
}

/// The object number of the catalog `/Metadata` XMP stream (PyMuPDF
/// `Document.xref_xml_metadata`), or `0` when absent — fitz returns `0`, not
/// `-1`, for a missing `/Metadata`.
#[must_use]
pub fn xref_xml_metadata(doc: &DocumentStore) -> i64 {
    catalog_dict(doc)
        .and_then(|cat| {
            cat.get(&Name::new("Metadata"))
                .and_then(Object::as_reference)
        })
        .map(|r| i64::from(r.num))
        .unwrap_or(0)
}

/// Normalizes a BCP-47 / RFC-3066 language tag exactly like MuPDF's
/// `fz_text_language_from_string` → `fz_string_from_text_language` round-trip.
///
/// MuPDF special-cases the Chinese script/region variants, then otherwise keeps
/// only the first 2–3 ASCII letters of the primary subtag (everything after the
/// first `-` is dropped). A primary subtag of fewer than 2 letters, or a
/// non-letter first character, is invalid and yields `None` (fitz removes
/// `/Lang`).
#[must_use]
pub fn normalize_language(lang: &str) -> Option<String> {
    // Chinese special cases (case-insensitive on the whole tag, per MuPDF).
    let lower = lang.to_ascii_lowercase();
    match lower.as_str() {
        "zh-hant" | "zh-hk" | "zh-mo" | "zh-sg" | "zh-tw" => return Some("zh-Hant".to_string()),
        "zh-hans" | "zh-cn" => return Some("zh-Hans".to_string()),
        _ => {}
    }
    // General ISO-639: up to 3 leading ASCII letters of the primary subtag.
    let mut letters = String::new();
    for c in lang.chars() {
        if c.is_ascii_alphabetic() {
            letters.push(c.to_ascii_lowercase());
            if letters.len() == 3 {
                break;
            }
        } else {
            // First non-letter ends the primary subtag.
            break;
        }
    }
    if letters.len() < 2 {
        None
    } else {
        Some(letters)
    }
}
