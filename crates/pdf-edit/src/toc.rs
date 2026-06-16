//! Table of contents — `/Outlines` read (`get_toc`) + build (`set_toc`) (PRD §8.9).
//!
//! `get_toc` flattens the outline tree (following First/Next/Parent) into a flat
//! list of `(level, title, page)` rows in document order, computing the page from
//! `/Dest` or a `/A /GoTo` action. `set_toc` builds a correct `/Outlines` tree
//! (Count/First/Last/Next/Prev/Parent, `/Dest` to a page) from such a flat list,
//! **rejecting level jumps** (e.g. 1→3) with a typed error.

use std::collections::HashMap;

use pdf_core::object::Name;
use pdf_core::{Dict, DocumentStore, ObjRef, Object, PdfString, StringKind};

use crate::dest::{page_index_map, resolve_link};

/// One TOC entry: `level` (1-based), `title`, 0-based physical `page` (or `-1`
/// when the destination cannot be resolved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocEntry {
    /// Nesting depth, 1 for top-level.
    pub level: i32,
    /// The entry title.
    pub title: String,
    /// Target physical page (0-based), or `-1` if unresolved.
    pub page: i32,
}

/// One node of the document outline tree (PyMuPDF `Outline`). Mirrors an
/// `/Outlines` item: `title`, the resolved 0-based `page` (or `-1`), an external
/// `uri` (when the action is `/URI`), the open flag, and the `next` sibling /
/// `down` first-child subtrees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineNode {
    /// The entry `/Title`.
    pub title: String,
    /// Target physical page (0-based), or `-1` if unresolved / external.
    pub page: i32,
    /// External URI for a `/URI` action link, else `None`.
    pub uri: Option<String>,
    /// Whether the item is open (`/Count >= 0`).
    pub is_open: bool,
    /// The next sibling subtree, if any.
    pub next: Option<Box<OutlineNode>>,
    /// The first child subtree, if any.
    pub down: Option<Box<OutlineNode>>,
}

/// Reads the document outline as a tree (PyMuPDF `Document.outline`). Returns the
/// first top-level item (with its `next`/`down` chains), or `None` when there is
/// no `/Outlines`.
#[must_use]
pub fn get_outline(doc: &DocumentStore) -> Option<OutlineNode> {
    let catalog = catalog_dict(doc)?;
    let outlines = deref(doc, catalog.get(&Name::new("Outlines"))?);
    let od = outlines.as_dict()?;
    let first = od.get(&Name::new("First")).and_then(Object::as_reference)?;
    let pages = page_index_map(doc);
    build_outline(doc, first, &pages, 0).map(|b| *b)
}

/// Builds the [`OutlineNode`] at `r`, following `/Next` and recursing `/First`.
fn build_outline(
    doc: &DocumentStore,
    r: ObjRef,
    pages: &HashMap<u32, usize>,
    depth: usize,
) -> Option<Box<OutlineNode>> {
    if depth > 200 {
        return None;
    }
    let item = doc.resolve(r).ok()?;
    let d = item.as_dict()?;

    let title = d
        .get(&Name::new("Title"))
        .and_then(Object::as_string)
        .map(|s| decode_text(s.as_bytes()))
        .unwrap_or_default();
    let page = resolve_link(doc, d, pages).map_or(-1, |p| p as i32);
    let uri = outline_uri(doc, d);
    // `/Count` >= 0 (or absent) means open; a negative count means collapsed.
    let is_open = d
        .get(&Name::new("Count"))
        .and_then(Object::as_i64)
        .is_none_or(|c| c >= 0);

    let down = d
        .get(&Name::new("First"))
        .and_then(Object::as_reference)
        .and_then(|c| build_outline(doc, c, pages, depth + 1));
    let next = d
        .get(&Name::new("Next"))
        .and_then(Object::as_reference)
        .and_then(|n| build_outline(doc, n, pages, depth + 1));

    Some(Box::new(OutlineNode {
        title,
        page,
        uri,
        is_open,
        next,
        down,
    }))
}

/// The external URI of an outline item whose `/A` action is `/URI`, if any.
fn outline_uri(doc: &DocumentStore, d: &Dict) -> Option<String> {
    let a = deref(doc, d.get(&Name::new("A"))?);
    let ad = a.as_dict()?;
    if ad
        .get(&Name::new("S"))
        .and_then(Object::as_name)?
        .as_bytes()
        != b"URI"
    {
        return None;
    }
    let uri = ad.get(&Name::new("URI")).and_then(Object::as_string)?;
    Some(String::from_utf8_lossy(uri.as_bytes()).into_owned())
}

/// Reads the document outline as a flat ordered list (PRD §8.9). Empty when there
/// is no `/Outlines`.
#[must_use]
pub fn get_toc(doc: &DocumentStore) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let Some(catalog) = catalog_dict(doc) else {
        return out;
    };
    let Some(outlines) = catalog.get(&Name::new("Outlines")) else {
        return out;
    };
    let outlines = deref(doc, outlines);
    let Some(od) = outlines.as_dict() else {
        return out;
    };
    let Some(first) = od.get(&Name::new("First")).and_then(Object::as_reference) else {
        return out;
    };
    let pages = page_index_map(doc);
    walk_siblings(doc, first, 1, &pages, &mut out, 0);
    out
}

/// Follows the `/Next` chain at one level, recursing into `/First` children.
fn walk_siblings(
    doc: &DocumentStore,
    start: ObjRef,
    level: i32,
    pages: &HashMap<u32, usize>,
    out: &mut Vec<TocEntry>,
    depth: usize,
) {
    if depth > 200 {
        return;
    }
    let mut cur = Some(start);
    let mut guard = 0usize;
    while let Some(r) = cur {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        let Ok(item) = doc.resolve(r) else { break };
        let Some(d) = item.as_dict() else { break };

        let title = d
            .get(&Name::new("Title"))
            .and_then(Object::as_string)
            .map(|s| decode_text(s.as_bytes()))
            .unwrap_or_default();
        let page = resolve_link(doc, d, pages).map_or(-1, |p| p as i32);
        out.push(TocEntry { level, title, page });

        if let Some(child) = d.get(&Name::new("First")).and_then(Object::as_reference) {
            walk_siblings(doc, child, level + 1, pages, out, depth + 1);
        }
        cur = d.get(&Name::new("Next")).and_then(Object::as_reference);
    }
}

/// Builds a fresh `/Outlines` tree from a flat level list and wires it into the
/// catalog (PRD §8.9). An empty list removes `/Outlines`.
///
/// # Errors
///
/// [`pdf_core::Error::InvalidArgument`] on a level jump (a level more than one
/// deeper than its predecessor, or a first entry whose level is not 1) — the
/// document is left unmutated. Propagates object-edit errors.
pub fn set_toc(doc: &DocumentStore, entries: &[TocEntry]) -> pdf_core::Result<()> {
    let root = doc
        .root()
        .ok_or(pdf_core::Error::InvalidArgument("document has no /Root"))?;
    let mut catalog =
        doc.resolve(root)?
            .as_dict()
            .cloned()
            .ok_or(pdf_core::Error::InvalidArgument(
                "/Root is not a dictionary",
            ))?;

    // Validate levels first (no mutation on failure).
    validate_levels(entries)?;

    if entries.is_empty() {
        catalog.remove(&Name::new("Outlines"));
        doc.update_object(root, Object::Dictionary(catalog))?;
        return Ok(());
    }

    let pages = pdf_core::pagetree::page_refs(doc);

    // Pre-allocate an object number for every entry + the /Outlines root, so we
    // can wire Parent/First/Last/Next/Prev refs before filling the dicts.
    let outlines_ref = doc.add_object(Object::Dictionary(Dict::new()))?;
    let mut item_refs: Vec<ObjRef> = Vec::with_capacity(entries.len());
    for _ in entries {
        item_refs.push(doc.add_object(Object::Dictionary(Dict::new()))?);
    }

    // For each entry, find parent (nearest preceding entry with level-1), and the
    // sibling chain at its level.
    let parent_of = compute_parents(entries);

    // children[i] = ordered child indices of entry i; roots = top-level indices.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, _) in entries.iter().enumerate() {
        match parent_of[i] {
            Some(p) => children[p].push(i),
            None => roots.push(i),
        }
    }

    // Fill each item dict.
    for (i, entry) in entries.iter().enumerate() {
        let mut d = Dict::new();
        d.insert(
            Name::new("Title"),
            Object::String(encode_text(&entry.title)),
        );
        // Parent: an item's parent ref, else the /Outlines root.
        let parent_ref = match parent_of[i] {
            Some(p) => item_refs[p],
            None => outlines_ref,
        };
        d.insert(Name::new("Parent"), Object::Reference(parent_ref));

        // Dest → [pageref /XYZ null null null].
        if entry.page >= 0 {
            let idx = entry.page as usize;
            if let Some(pref) = pages.get(idx) {
                d.insert(Name::new("Dest"), make_dest(*pref));
            }
        }

        // Sibling links + child links.
        let siblings = match parent_of[i] {
            Some(p) => &children[p],
            None => &roots,
        };
        let pos = siblings.iter().position(|&x| x == i).unwrap();
        if pos > 0 {
            d.insert(
                Name::new("Prev"),
                Object::Reference(item_refs[siblings[pos - 1]]),
            );
        }
        if pos + 1 < siblings.len() {
            d.insert(
                Name::new("Next"),
                Object::Reference(item_refs[siblings[pos + 1]]),
            );
        }
        if let Some(first_child) = children[i].first() {
            d.insert(
                Name::new("First"),
                Object::Reference(item_refs[*first_child]),
            );
            d.insert(
                Name::new("Last"),
                Object::Reference(item_refs[*children[i].last().unwrap()]),
            );
            // /Count: open count = number of descendants (negative = closed; we
            // emit a positive open count for simplicity, matching PyMuPDF default).
            d.insert(
                Name::new("Count"),
                Object::Integer(descendant_count(i, &children) as i64),
            );
        }
        doc.update_object(item_refs[i], Object::Dictionary(d))?;
    }

    // /Outlines root dict.
    let mut od = Dict::new();
    od.insert(Name::new("Type"), Object::Name(Name::new("Outlines")));
    if let Some(first) = roots.first() {
        od.insert(Name::new("First"), Object::Reference(item_refs[*first]));
        od.insert(
            Name::new("Last"),
            Object::Reference(item_refs[*roots.last().unwrap()]),
        );
    }
    // Root /Count = total number of visible (open) items = all entries here.
    od.insert(Name::new("Count"), Object::Integer(entries.len() as i64));
    doc.update_object(outlines_ref, Object::Dictionary(od))?;

    catalog.insert(Name::new("Outlines"), Object::Reference(outlines_ref));
    doc.update_object(root, Object::Dictionary(catalog))?;
    Ok(())
}

/// Validates the level sequence: first level must be 1, and no level may jump by
/// more than +1 from the previous entry (PRD §12).
fn validate_levels(entries: &[TocEntry]) -> pdf_core::Result<()> {
    let mut prev = 0;
    for (i, e) in entries.iter().enumerate() {
        if e.level < 1 {
            return Err(pdf_core::Error::InvalidArgument("TOC level must be >= 1"));
        }
        if i == 0 {
            if e.level != 1 {
                return Err(pdf_core::Error::InvalidArgument(
                    "first TOC entry must be at level 1",
                ));
            }
        } else if e.level > prev + 1 {
            return Err(pdf_core::Error::InvalidArgument(
                "TOC level jumped by more than one",
            ));
        }
        prev = e.level;
    }
    Ok(())
}

/// For each entry, the index of its parent (nearest preceding entry with level
/// exactly one less), or `None` for a top-level entry.
fn compute_parents(entries: &[TocEntry]) -> Vec<Option<usize>> {
    let mut parents = vec![None; entries.len()];
    // Stack of (level, index) of the current ancestor chain.
    let mut stack: Vec<usize> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        while let Some(&top) = stack.last() {
            if entries[top].level >= e.level {
                stack.pop();
            } else {
                break;
            }
        }
        parents[i] = stack.last().copied();
        stack.push(i);
    }
    parents
}

/// Total descendant count of entry `i` (all nested children, recursively).
fn descendant_count(i: usize, children: &[Vec<usize>]) -> usize {
    let mut n = children[i].len();
    for &c in &children[i] {
        n += descendant_count(c, children);
    }
    n
}

/// `[pageref /XYZ null null null]` explicit destination.
fn make_dest(page: ObjRef) -> Object {
    Object::Array(vec![
        Object::Reference(page),
        Object::Name(Name::new("XYZ")),
        Object::Null,
        Object::Null,
        Object::Null,
    ])
}

fn encode_text(s: &str) -> PdfString {
    if s.is_ascii() {
        PdfString {
            bytes: s.as_bytes().to_vec(),
            kind: StringKind::Literal,
        }
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        PdfString {
            bytes,
            kind: StringKind::Hex,
        }
    }
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
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

fn catalog_dict(doc: &DocumentStore) -> Option<Dict> {
    let root: ObjRef = doc.root()?;
    doc.resolve(root).ok()?.as_dict().cloned()
}
