//! Document sanitization — `scrub` (remove sensitive data) and `bake` (flatten
//! interactive content into static page content) (PRD §8.8, PyMuPDF parity).
//!
//! [`scrub`] is a conservative, PyMuPDF-style cleaner that strips the most common
//! carriers of leaked or executable data: the `/Info` dictionary and XMP
//! `/Metadata`, document-level JavaScript (`/OpenAction`, catalog `/AA`, the
//! `/Names /JavaScript` name-tree), embedded files (`/Names /EmbeddedFiles`) and,
//! optionally, every `/Link` annotation. It operates over the catalog and trailer
//! only; it is idempotent (a second run over an already-scrubbed document is a
//! no-op) and never panics on degenerate dictionaries — an absent key is skipped
//! silently.
//!
//! Documented limitations (deliberately conservative for this milestone):
//! - JavaScript removal targets only the **document-level** carriers above. It
//!   does not chase per-object `/AA` additional-action dictionaries on individual
//!   annotations or form fields, nor `/JS` actions embedded in link/annotation
//!   `/A` action dictionaries.
//! - It does not rewrite or sanitize page content streams, and does not touch
//!   object-stream / xref-stream metadata.
//!
//! [`bake`] flattens interactive content into the page content streams: widgets
//! (form fields) via the existing [`crate::form::flatten`], and non-widget
//! annotations by drawing each annotation's `/AP /N` appearance as a Form XObject
//! `Do` at its `/Rect`, then removing the annotation from the page `/Annots`.

use std::collections::HashSet;

use pdf_core::error::{Error, Result};
use pdf_core::geom::Rect;
use pdf_core::object::{Dict, Name, ObjRef, Object};
use pdf_core::pagetree;
use pdf_core::DocumentStore;

use crate::content::PageContent;
use crate::form;
use crate::metadata::set_metadata;

/// Which classes of sensitive data [`scrub`] removes. Construct with
/// [`ScrubOptions::default`] (the PyMuPDF-aligned defaults) and toggle fields.
#[derive(Clone, Copy, Debug)]
pub struct ScrubOptions {
    /// Remove the trailer `/Info` dictionary fields and the catalog `/Metadata`
    /// (XMP) stream. Default `true`.
    pub metadata: bool,
    /// Remove document-level JavaScript: catalog `/OpenAction`, catalog `/AA`
    /// (additional actions) and the `/Names /JavaScript` name-tree. Default
    /// `true`.
    pub javascript: bool,
    /// Remove embedded/attached files: the `/Names /EmbeddedFiles` name-tree.
    /// Default `true`.
    pub attached_files: bool,
    /// Remove every `/Link` annotation from every page's `/Annots`. Default
    /// `false`.
    pub remove_links: bool,
    /// Remove the catalog `/Metadata` (XMP) stream. Subsumed by `metadata`; kept
    /// as a separate toggle for parity. Default `true`.
    pub xml_metadata: bool,
}

impl Default for ScrubOptions {
    fn default() -> Self {
        ScrubOptions {
            metadata: true,
            javascript: true,
            attached_files: true,
            remove_links: false,
            xml_metadata: true,
        }
    }
}

/// Removes sensitive data from `doc` per the enabled [`ScrubOptions`] (PRD §8.8).
///
/// Conservative and idempotent: absent keys are skipped silently, a second run is
/// a no-op, and degenerate dictionaries never panic. See the module docs for the
/// exact scope and documented limitations.
///
/// # Errors
///
/// Propagates [`pdf_core::Error`] from the object-edit / trailer-set path.
pub fn scrub(doc: &DocumentStore, opts: &ScrubOptions) -> Result<()> {
    if opts.metadata {
        clear_info(doc)?;
        remove_xml_metadata(doc)?;
    }
    if opts.xml_metadata {
        remove_xml_metadata(doc)?;
    }
    if opts.javascript {
        remove_javascript(doc)?;
    }
    if opts.attached_files {
        remove_embedded_files(doc)?;
    }
    if opts.remove_links {
        remove_links(doc)?;
    }
    Ok(())
}

/// Empties the trailer `/Info`: removes every known text field via
/// [`set_metadata`] (empty value removes the key) and drops the trailer `/Info`
/// reference so a reopened document carries no document information.
fn clear_info(doc: &DocumentStore) -> Result<()> {
    // Only act when an /Info actually exists (keeps scrub idempotent and avoids
    // creating an empty /Info on a doc that never had one).
    if doc.effective_trailer_ref("Info").is_none() {
        return Ok(());
    }
    // Remove every known field (empty value => key removed), then detach /Info.
    let empties: Vec<(String, String)> = INFO_FIELD_KEYS
        .iter()
        .map(|k| ((*k).to_string(), String::new()))
        .collect();
    set_metadata(doc, &empties)?;
    doc.set_trailer_key("Info", Object::Null)
}

/// The PyMuPDF `/Info` field keys understood by [`set_metadata`] (mirrors the
/// private `INFO_KEYS` table in [`crate::metadata`]).
const INFO_FIELD_KEYS: &[&str] = &[
    "title",
    "author",
    "subject",
    "keywords",
    "creator",
    "producer",
    "creationDate",
    "modDate",
    "trapped",
];

/// Removes the catalog `/Metadata` (XMP) key and frees the stream if indirect.
fn remove_xml_metadata(doc: &DocumentStore) -> Result<()> {
    let Some(root) = doc.root() else {
        return Ok(());
    };
    let Some(mut catalog) = catalog_dict(doc) else {
        return Ok(());
    };
    match catalog.remove(&Name::new("Metadata")) {
        Some(removed) => {
            if let Some(r) = removed.as_reference() {
                let _ = doc.delete_object(r);
            }
            doc.update_object(root, Object::Dictionary(catalog))
        }
        None => Ok(()),
    }
}

/// Removes document-level JavaScript carriers from the catalog: `/OpenAction`,
/// `/AA` and the `/Names /JavaScript` name-tree.
fn remove_javascript(doc: &DocumentStore) -> Result<()> {
    let Some(root) = doc.root() else {
        return Ok(());
    };
    let Some(mut catalog) = catalog_dict(doc) else {
        return Ok(());
    };
    let mut changed = false;
    if catalog.remove(&Name::new("OpenAction")).is_some() {
        changed = true;
    }
    if catalog.remove(&Name::new("AA")).is_some() {
        changed = true;
    }
    changed |= remove_names_entry(doc, &mut catalog, "JavaScript")?;
    if changed {
        doc.update_object(root, Object::Dictionary(catalog))?;
    }
    Ok(())
}

/// Removes the embedded-files name-tree (`/Names /EmbeddedFiles`) from the
/// catalog.
fn remove_embedded_files(doc: &DocumentStore) -> Result<()> {
    let Some(root) = doc.root() else {
        return Ok(());
    };
    let Some(mut catalog) = catalog_dict(doc) else {
        return Ok(());
    };
    if remove_names_entry(doc, &mut catalog, "EmbeddedFiles")? {
        doc.update_object(root, Object::Dictionary(catalog))?;
    }
    Ok(())
}

/// Removes `key` from the catalog `/Names` dictionary, writing the updated
/// `/Names` back in place (resolving it if indirect). Returns whether the
/// catalog dict itself was mutated (i.e. the caller must persist it). Returns
/// `false` when there was nothing to remove.
fn remove_names_entry(doc: &DocumentStore, catalog: &mut Dict, key: &str) -> Result<bool> {
    match catalog.get(&Name::new("Names")).cloned() {
        // Indirect /Names: mutate and update the referenced object in place; the
        // catalog dict is unchanged, so it need not be re-persisted by the caller.
        Some(Object::Reference(r)) => {
            let mut names = match doc.resolve(r).ok().and_then(|o| o.as_dict().cloned()) {
                Some(d) => d,
                None => return Ok(false),
            };
            if names.remove(&Name::new(key)).is_some() {
                doc.update_object(r, Object::Dictionary(names))?;
            }
            Ok(false)
        }
        // Direct /Names dict: mutate it on the catalog; caller persists.
        Some(Object::Dictionary(mut names)) => {
            if names.remove(&Name::new(key)).is_some() {
                catalog.insert(Name::new("Names"), Object::Dictionary(names));
                Ok(true)
            } else {
                Ok(false)
            }
        }
        _ => Ok(false),
    }
}

/// Removes every `/Link`-subtype annotation from each page's `/Annots`.
fn remove_links(doc: &DocumentStore) -> Result<()> {
    for &leaf in &pagetree::page_refs(doc) {
        let Some(mut pd) = doc.resolve(leaf).ok().and_then(|o| o.as_dict().cloned()) else {
            continue;
        };
        let arr = match annots_array(doc, &pd) {
            Some(a) => a,
            None => continue,
        };
        let filtered: Vec<Object> = arr.into_iter().filter(|o| !is_link_ref(doc, o)).collect();
        let had_links = match pd.get(&Name::new("Annots")) {
            Some(Object::Array(a)) => a.len() != filtered.len(),
            // Indirect /Annots: compare against the resolved length.
            Some(Object::Reference(_)) => true,
            _ => false,
        };
        if !had_links {
            continue;
        }
        if filtered.is_empty() {
            pd.remove(&Name::new("Annots"));
        } else {
            pd.insert(Name::new("Annots"), Object::Array(filtered));
        }
        doc.update_object(leaf, Object::Dictionary(pd))?;
    }
    Ok(())
}

/// Whether an `/Annots` entry resolves to a dict with `/Subtype /Link`.
fn is_link_ref(doc: &DocumentStore, entry: &Object) -> bool {
    let dict = match entry {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(r) => doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned()),
        _ => None,
    };
    dict.map(|d| {
        d.get(&Name::new("Subtype"))
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"Link")
    })
    .unwrap_or(false)
}

// === bake() ===============================================================

/// Flattens interactive content into the page content streams (PRD §8.8).
///
/// - `widgets`: delegates to [`crate::form::flatten`], baking each widget's
///   current `/AP /N` appearance into page content as a Form XObject `Do`,
///   removing the `/Widget` annotations and deleting the catalog `/AcroForm`.
/// - `annots`: for every **non-widget** annotation carrying an `/AP /N`
///   appearance stream, draws that appearance as a Form XObject `Do` mapped into
///   the annotation's `/Rect`, then removes the annotation from the page
///   `/Annots`. Annotations without a usable `/AP /N` are left in place (nothing
///   to bake). The appearance-placement mapping (form `/BBox` → annot `/Rect`)
///   mirrors the widget path in [`crate::form`].
///
/// When both flags are `false` this is a no-op. When `widgets` is requested the
/// widget pass runs first (so the form is fully resolved before the generic
/// annotation pass).
///
/// # Errors
///
/// Propagates resolve / object-edit errors from the flatten and content-append
/// paths.
pub fn bake(doc: &DocumentStore, annots: bool, widgets: bool) -> Result<()> {
    if widgets {
        form::flatten(doc)?;
    }
    if annots {
        bake_annots(doc)?;
    }
    Ok(())
}

/// Bakes every non-widget annotation with an `/AP /N` stream into page content,
/// then removes those annotations from the page.
fn bake_annots(doc: &DocumentStore) -> Result<()> {
    let pages = pagetree::page_refs(doc);
    for (index, &leaf) in pages.iter().enumerate() {
        let Some(pd) = doc.resolve(leaf).ok().and_then(|o| o.as_dict().cloned()) else {
            continue;
        };
        let Some(arr) = annots_array(doc, &pd) else {
            continue;
        };
        // Collect the annotation refs to bake (non-widget, has /AP /N stream).
        let candidates: Vec<ObjRef> = arr
            .iter()
            .filter_map(Object::as_reference)
            .filter(|&r| !is_widget(doc, r) && ap_n_stream(doc, r).is_some())
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let pc = PageContent::new(doc, index)?;
        let mut baked: HashSet<u32> = HashSet::new();
        for &r in &candidates {
            if let Some(n_ref) = draw_annot_ap(doc, &pc, r)? {
                baked.insert(n_ref.num);
            }
        }
        // Remove the baked annotations from /Annots (re-resolve the leaf: the
        // content append above rewrote the page dict).
        remove_annots_from_leaf(doc, leaf, &candidates)?;
        // Free the baked annotation objects, keeping the appearance streams now
        // referenced by page content alive.
        for &r in &candidates {
            free_annot(doc, r, &baked);
        }
    }
    Ok(())
}

/// Draws an annotation's `/AP /N` appearance stream into the page content at its
/// `/Rect`, as a Form XObject `Do`. Returns the appearance reference on success
/// (so the caller keeps it alive). `Ok(None)` when there is no usable appearance.
fn draw_annot_ap(doc: &DocumentStore, pc: &PageContent, annot: ObjRef) -> Result<Option<ObjRef>> {
    let Some(ad) = doc.resolve(annot).ok().and_then(|o| o.as_dict().cloned()) else {
        return Ok(None);
    };
    let Some(n_ref) = ap_n_stream(doc, annot) else {
        return Ok(None);
    };
    let rect = read_rect(&ad, "Rect").normalize();
    let n_obj = doc.resolve(n_ref)?;
    let n_dict = n_obj
        .as_stream()
        .map(|s| s.dict.clone())
        .unwrap_or_default();
    let bbox = read_rect(&n_dict, "BBox");
    let cm = bbox_to_rect_cm(bbox, rect);
    let name = pc.add_resource("XObject", "Fm", Object::Reference(n_ref))?;
    let chunk = format!(
        "q\n{} {} {} {} {} {} cm\n/{name} Do\nQ\n",
        fmt_num(cm[0]),
        fmt_num(cm[1]),
        fmt_num(cm[2]),
        fmt_num(cm[3]),
        fmt_num(cm[4]),
        fmt_num(cm[5]),
    );
    pc.append_content(chunk.as_bytes())?;
    Ok(Some(n_ref))
}

/// The annotation's `/AP /N` *stream* reference (text/markup appearances), or
/// `None` when `/N` is absent or a state sub-dictionary (button-style).
fn ap_n_stream(doc: &DocumentStore, annot: ObjRef) -> Option<ObjRef> {
    let d = doc.resolve(annot).ok()?.as_dict().cloned()?;
    let ap = match d.get(&Name::new("AP"))? {
        Object::Dictionary(ap) => ap.clone(),
        Object::Reference(r) => doc.resolve(*r).ok()?.as_dict().cloned()?,
        _ => return None,
    };
    let n_ref = ap.get(&Name::new("N")).and_then(Object::as_reference)?;
    // Confirm it resolves to a stream (not a state sub-dict reference).
    doc.resolve(n_ref).ok()?.as_stream().map(|_| n_ref)
}

/// Removes the given annotation references from a page leaf's `/Annots`.
fn remove_annots_from_leaf(doc: &DocumentStore, leaf: ObjRef, drop: &[ObjRef]) -> Result<()> {
    let mut pd = doc
        .resolve(leaf)?
        .as_dict()
        .cloned()
        .ok_or(Error::InvalidArgument("page is not a dictionary"))?;
    let Some(arr) = annots_array(doc, &pd) else {
        return Ok(());
    };
    let drop_nums: HashSet<u32> = drop.iter().map(|r| r.num).collect();
    let filtered: Vec<Object> = arr
        .into_iter()
        .filter(|o| !matches!(o, Object::Reference(r) if drop_nums.contains(&r.num)))
        .collect();
    if filtered.is_empty() {
        pd.remove(&Name::new("Annots"));
    } else {
        pd.insert(Name::new("Annots"), Object::Array(filtered));
    }
    doc.update_object(leaf, Object::Dictionary(pd))
}

/// Frees an annotation object and its `/AP /N` stream (best-effort), keeping any
/// appearance reference whose object number is in `keep` (baked into content).
fn free_annot(doc: &DocumentStore, annot: ObjRef, keep: &HashSet<u32>) {
    if let Some(n_ref) = ap_n_stream(doc, annot) {
        if !keep.contains(&n_ref.num) {
            let _ = doc.delete_object(n_ref);
        }
    }
    let _ = doc.delete_object(annot);
}

// === shared helpers =======================================================

/// The catalog dictionary, resolved through the overlay (or `None`).
fn catalog_dict(doc: &DocumentStore) -> Option<Dict> {
    let root = doc.root()?;
    doc.resolve(root).ok()?.as_dict().cloned()
}

/// Resolves a page dict's `/Annots` to an owned array of entries (direct or
/// indirect), or `None` when absent / not an array.
fn annots_array(doc: &DocumentStore, page: &Dict) -> Option<Vec<Object>> {
    match page.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => Some(a.clone()),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec)),
        _ => None,
    }
}

/// Whether a reference resolves to a `/Subtype /Widget` annotation.
fn is_widget(doc: &DocumentStore, r: ObjRef) -> bool {
    doc.resolve(r)
        .ok()
        .and_then(|o| o.as_dict().cloned())
        .and_then(|d| {
            d.get(&Name::new("Subtype"))
                .and_then(Object::as_name)
                .map(|n| n.as_bytes() == b"Widget")
        })
        .unwrap_or(false)
}

/// Reads a `[x0 y0 x1 y1]` rect from `key` (zero rect when absent/malformed).
fn read_rect(d: &Dict, key: &str) -> Rect {
    match d.get(&Name::new(key)).and_then(Object::as_array) {
        Some(a) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
            Rect::new(v[0], v[1], v[2], v[3])
        }
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

/// The CTM placing a form whose `/BBox` is `bbox` into the target `rect`. A
/// degenerate BBox falls back to a pure translation to the rect corner.
fn bbox_to_rect_cm(bbox: Rect, rect: Rect) -> [f64; 6] {
    let bw = bbox.width();
    let bh = bbox.height();
    if bw.abs() < 1e-6 || bh.abs() < 1e-6 {
        return [1.0, 0.0, 0.0, 1.0, rect.x0, rect.y0];
    }
    let sx = rect.width() / bw;
    let sy = rect.height() / bh;
    let e = rect.x0 - sx * bbox.x0;
    let f = rect.y0 - sy * bbox.y0;
    [sx, 0.0, 0.0, sy, e, f]
}

/// Formats an `f64` for a content-stream operand (trims trailing zeros).
fn fmt_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.4}");
        let s = s.trim_end_matches('0');
        s.trim_end_matches('.').to_string()
    }
}
