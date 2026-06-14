//! Link annotations — read / insert / update / delete (PRD §8.9).
//!
//! A link is a page `/Annots` entry with `/Subtype /Link`, a `/Rect` source
//! rectangle, and either a `/A /URI` (external) or a `/Dest` / `/A /GoTo`
//! (internal page) target. The geometry uses `pdf_core::geom::Rect`.

use std::collections::HashMap;

use pdf_core::geom::Rect;
use pdf_core::object::Name;
use pdf_core::{Dict, DocumentStore, ObjRef, Object, PdfString, StringKind};

use crate::dest::{page_index_map, resolve_link};

/// What a link points at.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkKind {
    /// External URI (`/A /URI`).
    Uri(String),
    /// Internal GoTo to a 0-based physical page (`/Dest` or `/A /GoTo`).
    Goto(i32),
    /// A recognized link with no resolvable target.
    None,
}

/// One link annotation on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The source rectangle on the page.
    pub from: Rect,
    /// The target.
    pub kind: LinkKind,
    /// The annotation's object number (for update/delete).
    pub xref: u32,
}

/// Reads the `/Link` annotations of page `index` (PRD §8.9). Empty when the page
/// has no `/Annots` or no links.
#[must_use]
pub fn get_links(doc: &DocumentStore, index: usize) -> Vec<Link> {
    let mut out = Vec::new();
    let Some(page_ref) = pdf_core::pagetree::page_refs(doc).get(index).copied() else {
        return out;
    };
    let Ok(page) = doc.resolve(page_ref) else {
        return out;
    };
    let Some(pd) = page.as_dict() else {
        return out;
    };
    let Some(annots) = pd.get(&Name::new("Annots")) else {
        return out;
    };
    let annots = deref(doc, annots);
    let Some(arr) = annots.as_array() else {
        return out;
    };
    let pages = page_index_map(doc);

    for a in arr {
        let (anum, adict) = match a {
            Object::Reference(r) => match doc.resolve(*r) {
                Ok(o) => match o.as_dict() {
                    Some(d) => (r.num, d.clone()),
                    None => continue,
                },
                Err(_) => continue,
            },
            Object::Dictionary(d) => (0, d.clone()),
            _ => continue,
        };
        let is_link = adict
            .get(&Name::new("Subtype"))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes() == b"Link")
            .unwrap_or(false);
        if !is_link {
            continue;
        }
        let from = rect_from(&adict);
        let kind = link_kind(doc, &adict, &pages);
        out.push(Link {
            from,
            kind,
            xref: anum,
        });
    }
    out
}

/// Inserts a link annotation onto page `index` (PRD §8.9). Returns the new annot
/// object reference.
///
/// # Errors
///
/// [`pdf_core::Error::InvalidArgument`] for an out-of-range page; propagates
/// object-edit errors.
pub fn insert_link(
    doc: &DocumentStore,
    index: usize,
    rect: &Rect,
    kind: &LinkKind,
) -> pdf_core::Result<ObjRef> {
    let page_ref = page_ref_at(doc, index)?;
    let annot = build_link_annot(doc, rect, kind)?;
    let annot_ref = doc.add_object(Object::Dictionary(annot))?;

    let mut pd = doc
        .resolve(page_ref)?
        .as_dict()
        .cloned()
        .ok_or(pdf_core::Error::InvalidArgument("page is not a dictionary"))?;
    let mut annots = match pd.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => match deref(doc, &Object::Reference(*r)) {
            Object::Array(a) => a,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    annots.push(Object::Reference(annot_ref));
    pd.insert(Name::new("Annots"), Object::Array(annots));
    doc.update_object(page_ref, Object::Dictionary(pd))?;
    Ok(annot_ref)
}

/// Updates an existing link annotation's rect and/or target (PRD §8.9).
///
/// # Errors
///
/// Propagates object-edit errors.
pub fn update_link(
    doc: &DocumentStore,
    annot: ObjRef,
    rect: &Rect,
    kind: &LinkKind,
) -> pdf_core::Result<()> {
    let updated = build_link_annot(doc, rect, kind)?;
    doc.update_object(annot, Object::Dictionary(updated))
}

/// Deletes a link annotation from page `index` (PRD §8.9). No-op if the annot is
/// not on the page.
///
/// # Errors
///
/// [`pdf_core::Error::InvalidArgument`] for an out-of-range page; propagates
/// object-edit errors.
pub fn delete_link(doc: &DocumentStore, index: usize, annot: ObjRef) -> pdf_core::Result<()> {
    let page_ref = page_ref_at(doc, index)?;
    let mut pd = doc
        .resolve(page_ref)?
        .as_dict()
        .cloned()
        .ok_or(pdf_core::Error::InvalidArgument("page is not a dictionary"))?;
    if let Some(Object::Array(a)) = pd.get(&Name::new("Annots")).cloned().map(|o| match o {
        Object::Reference(r) => deref(doc, &Object::Reference(r)),
        other => other,
    }) {
        let filtered: Vec<Object> = a
            .into_iter()
            .filter(|o| !matches!(o, Object::Reference(r) if r.num == annot.num))
            .collect();
        pd.insert(Name::new("Annots"), Object::Array(filtered));
        doc.update_object(page_ref, Object::Dictionary(pd))?;
        doc.delete_object(annot)?;
    }
    Ok(())
}

/// Builds a `/Link` annotation dict from a rect + target.
fn build_link_annot(doc: &DocumentStore, rect: &Rect, kind: &LinkKind) -> pdf_core::Result<Dict> {
    let mut d = Dict::new();
    d.insert(Name::new("Type"), Object::Name(Name::new("Annot")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new("Link")));
    d.insert(Name::new("Rect"), rect_array(rect));
    // No visible border by default.
    d.insert(
        Name::new("Border"),
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    match kind {
        LinkKind::Uri(uri) => {
            let mut a = Dict::new();
            a.insert(Name::new("Type"), Object::Name(Name::new("Action")));
            a.insert(Name::new("S"), Object::Name(Name::new("URI")));
            a.insert(
                Name::new("URI"),
                Object::String(PdfString {
                    bytes: uri.as_bytes().to_vec(),
                    kind: StringKind::Literal,
                }),
            );
            d.insert(Name::new("A"), Object::Dictionary(a));
        }
        LinkKind::Goto(page) => {
            if *page >= 0 {
                let pages = pdf_core::pagetree::page_refs(doc);
                if let Some(pref) = pages.get(*page as usize) {
                    d.insert(
                        Name::new("Dest"),
                        Object::Array(vec![
                            Object::Reference(*pref),
                            Object::Name(Name::new("XYZ")),
                            Object::Null,
                            Object::Null,
                            Object::Null,
                        ]),
                    );
                }
            }
        }
        LinkKind::None => {}
    }
    Ok(d)
}

/// Classifies a link annotation's target.
fn link_kind(doc: &DocumentStore, d: &Dict, pages: &HashMap<u32, usize>) -> LinkKind {
    // URI action takes precedence.
    if let Some(a) = d.get(&Name::new("A")) {
        let a = deref(doc, a);
        if let Some(ad) = a.as_dict() {
            let s = ad.get(&Name::new("S")).and_then(Object::as_name);
            if let Some(s) = s {
                if s.as_bytes() == b"URI" {
                    let uri = ad
                        .get(&Name::new("URI"))
                        .and_then(Object::as_string)
                        .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
                        .unwrap_or_default();
                    return LinkKind::Uri(uri);
                }
            }
        }
    }
    match resolve_link(doc, d, pages) {
        Some(p) => LinkKind::Goto(p as i32),
        None => LinkKind::None,
    }
}

fn rect_from(d: &Dict) -> Rect {
    let arr = d.get(&Name::new("Rect")).and_then(Object::as_array);
    if let Some(a) = arr {
        if a.len() == 4 {
            let v: Vec<f64> = a.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
            return Rect::new(v[0], v[1], v[2], v[3]);
        }
    }
    Rect::new(0.0, 0.0, 0.0, 0.0)
}

fn rect_array(r: &Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.x0),
        Object::Real(r.y0),
        Object::Real(r.x1),
        Object::Real(r.y1),
    ])
}

fn page_ref_at(doc: &DocumentStore, index: usize) -> pdf_core::Result<ObjRef> {
    pdf_core::pagetree::page_refs(doc)
        .get(index)
        .copied()
        .ok_or(pdf_core::Error::InvalidArgument("page index out of range"))
}

fn deref(doc: &DocumentStore, obj: &Object) -> Object {
    match obj {
        Object::Reference(r) => doc
            .resolve(*r)
            .map(|a| (*a).clone())
            .unwrap_or(Object::Null),
        other => other.clone(),
    }
}
