//! Shared content-emission plumbing for `insert_text` / `insert_image` /
//! `draw_*` / `Shape` (PRD §8.8).
//!
//! Everything in this milestone *appends* to a page's `/Contents`:
//!
//! 1. The page's existing content is **wrapped** in a balanced `q … Q` pair so a
//!    newly appended chunk starts from a clean graphics state (the existing
//!    stream may leave the CTM / colors changed). The wrap is achieved by
//!    prepending a `q\n` stream and appending the caller's chunk after a `Q\n`,
//!    turning `/Contents` into an **array** of streams (legal per ISO 32000-1
//!    §7.8.2 — the streams are concatenated as one logical stream at render
//!    time). The original content stream object is left untouched, so existing
//!    text/vector content survives verbatim.
//! 2. New **resources** (a font, an image XObject, an ExtGState, …) are merged
//!    into the page's `/Resources` under the right sub-dictionary, allocating a
//!    fresh, collision-free name.
//!
//! The page leaf is always resolved live through the ChangeSet overlay, so
//! repeated insertions accumulate.

use pdf_core::error::{Error, Result};
use pdf_core::geom::{Point, Rect};
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::pagetree;
use pdf_core::DocumentStore;

/// A handle to one page, used by every content-insertion entry point. It owns no
/// state beyond the document handle and the page leaf reference — page geometry
/// is re-read live so it always reflects prior edits.
pub struct PageContent<'a> {
    pub(crate) doc: &'a DocumentStore,
    pub(crate) leaf: ObjRef,
}

impl<'a> PageContent<'a> {
    /// Opens a content handle on the page at zero-based `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if `index` is out of range.
    pub fn new(doc: &'a DocumentStore, index: usize) -> Result<Self> {
        let leaf = *pagetree::page_refs(doc)
            .get(index)
            .ok_or(Error::Unsupported("page index out of range"))?;
        Ok(PageContent { doc, leaf })
    }

    /// Opens a content handle directly on a page-leaf reference.
    #[must_use]
    pub fn from_leaf(doc: &'a DocumentStore, leaf: ObjRef) -> Self {
        PageContent { doc, leaf }
    }

    /// The page's `/MediaBox` (normalized), used for the top-left → PDF y-up
    /// coordinate conversion.
    #[must_use]
    pub fn mediabox(&self) -> Rect {
        pagetree::mediabox(self.doc, self.leaf)
    }

    /// Converts a PyMuPDF page point (origin top-left, y **down**, MediaBox
    /// relative) to a PDF user-space point (origin bottom-left, y **up**), per
    /// PRD §8.6.1 with `/Rotate == 0` (rotated-page authoring is deferred — the
    /// inserted content is authored in unrotated user space, matching PyMuPDF's
    /// default `insert_text` behavior). With media box `[x0 y0 x1 y1]`:
    /// `x' = x0 + x`, `y' = y1 - y`.
    #[must_use]
    pub fn to_user_space(&self, p: Point) -> Point {
        let mb = self.mediabox();
        Point::new(mb.x0 + p.x, mb.y1 - p.y)
    }

    /// Converts a PyMuPDF page rect (top-left space) to a PDF user-space rect
    /// (bottom-left). The top-left corner maps to the upper edge, so the result
    /// keeps `y0 <= y1`.
    #[must_use]
    pub fn rect_to_user_space(&self, r: Rect) -> Rect {
        let mb = self.mediabox();
        let r = r.normalize();
        Rect::new(mb.x0 + r.x0, mb.y1 - r.y1, mb.x0 + r.x1, mb.y1 - r.y0)
    }

    /// Appends a content chunk to the page, wrapping the existing content in a
    /// balanced `q … Q` so the new chunk starts from a clean state. Idempotent
    /// across repeated calls (each call appends one more stream).
    ///
    /// # Errors
    ///
    /// Propagates ChangeSet-allocation / resolve errors.
    pub fn append_content(&self, chunk: &[u8]) -> Result<()> {
        let mut leaf = self.leaf_dict()?;

        // The new stream object carrying the caller's chunk.
        let new_stream = self
            .doc
            .add_object(Object::Stream(make_stream(chunk.to_vec())))?;

        // Rewrite `/Contents` into `[ <q-guard> <existing…> <Q-guard> <new> ]`.
        // The guards bracket the *existing* content so a leftover CTM/color from
        // the original stream cannot leak into our chunk. An **empty** existing
        // stream (a blank page) carries no state, so it needs no guard — we drop
        // straight to the new chunk, keeping blank-page output clean.
        let existing = leaf.get(&Name::new("Contents")).cloned();
        let mut arr: Vec<Object> = Vec::new();
        let push_guarded = |items: Vec<Object>, arr: &mut Vec<Object>| -> Result<()> {
            let q = self
                .doc
                .add_object(Object::Stream(make_stream(b"q\n".to_vec())))?;
            arr.push(Object::Reference(q));
            arr.extend(items);
            let qq = self
                .doc
                .add_object(Object::Stream(make_stream(b"\nQ\n".to_vec())))?;
            arr.push(Object::Reference(qq));
            Ok(())
        };
        match existing {
            Some(Object::Array(items)) if !items.is_empty() => {
                push_guarded(items, &mut arr)?;
            }
            Some(Object::Reference(r)) if !self.is_empty_content(r) => {
                push_guarded(vec![Object::Reference(r)], &mut arr)?;
            }
            // No existing content / an empty stream / inline stream: nothing to
            // guard — just start the array with our chunk.
            _ => {}
        }
        arr.push(Object::Reference(new_stream));
        leaf.insert(Name::new("Contents"), Object::Array(arr));
        self.doc.update_object(self.leaf, Object::Dictionary(leaf))
    }

    /// Registers `resource` under `/Resources / <category> / <name>`, allocating
    /// a fresh name with the given `prefix` (e.g. `F` for fonts, `Img` for
    /// images, `GS` for ExtGState). Returns the chosen resource name (without the
    /// leading slash). The `/Resources` dict is created if absent.
    ///
    /// # Errors
    ///
    /// Propagates resolve / update errors.
    pub fn add_resource(&self, category: &str, prefix: &str, resource: Object) -> Result<String> {
        let mut leaf = self.leaf_dict()?;

        // Resolve (or create) the page's `/Resources` dict. We materialize a
        // *direct* dict on the leaf so we own it (no risk of mutating a shared
        // indirect resources object).
        let mut resources = match leaf.get(&Name::new("Resources")) {
            Some(Object::Dictionary(d)) => d.clone(),
            Some(Object::Reference(r)) => {
                self.doc.resolve(*r)?.as_dict().cloned().unwrap_or_default()
            }
            _ => Dict::new(),
        };

        // The category sub-dict (e.g. `/Font`, `/XObject`, `/ExtGState`).
        let cat_name = Name::new(category);
        let mut cat = match resources.get(&cat_name) {
            Some(Object::Dictionary(d)) => d.clone(),
            Some(Object::Reference(r)) => {
                self.doc.resolve(*r)?.as_dict().cloned().unwrap_or_default()
            }
            _ => Dict::new(),
        };

        let name = fresh_name(&cat, prefix);
        cat.insert(Name::new(&name), resource);
        resources.insert(cat_name, Object::Dictionary(cat));
        leaf.insert(Name::new("Resources"), Object::Dictionary(resources));
        self.doc
            .update_object(self.leaf, Object::Dictionary(leaf))?;
        Ok(name)
    }

    /// Whether the content stream `r` decodes to an empty / whitespace-only body
    /// (a blank page needs no graphics-state guard). Resolve / decode failures
    /// conservatively report `false` (wrap to be safe).
    fn is_empty_content(&self, r: ObjRef) -> bool {
        let Ok(obj) = self.doc.resolve(r) else {
            return false;
        };
        let Some(stream) = obj.as_stream() else {
            return false;
        };
        match self
            .doc
            .decode_stream(stream)
            .and_then(|o| o.into_decoded())
        {
            Ok(bytes) => bytes.iter().all(u8::is_ascii_whitespace),
            Err(_) => false,
        }
    }

    /// The page leaf dictionary (cloned through the overlay).
    fn leaf_dict(&self) -> Result<Dict> {
        self.doc
            .resolve(self.leaf)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::xref(0, "page leaf is not a dictionary"))
    }
}

/// Builds an *uncompressed* content/data stream with an accurate `/Length`. We
/// keep inserted content uncompressed for transparency / round-trip simplicity;
/// a later `save(deflate=1)` will compress it (PRD §8.7 stream deflation).
pub(crate) fn make_stream(data: Vec<u8>) -> StreamObj {
    StreamObj::new_encoded(
        Dict::from_iter([(Name::new("Length"), Object::Integer(data.len() as i64))]),
        data,
    )
}

/// Allocates a resource name `"{prefix}{n}"` not already present in `dict`,
/// scanning `n = 0, 1, 2, …`.
fn fresh_name(dict: &Dict, prefix: &str) -> String {
    let mut n = 0u32;
    loop {
        let candidate = format!("{prefix}{n}");
        if dict.get(&Name::new(&candidate)).is_none() {
            return candidate;
        }
        n += 1;
    }
}

/// Formats a coordinate / scalar for a content operator: integral values print
/// without a decimal point, others with up to 4 significant fractional digits
/// and no trailing zeros. Non-finite inputs degrade to `0`.
pub(crate) fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let mut s = format!("{v:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// Escapes a string for a PDF literal-string `( … )` content operand: `\`, `(`,
/// `)` are backslash-escaped; control bytes use octal. Bytes ≥ 0x80 are passed
/// through (the caller chooses an encoding that maps them, e.g. WinAnsi).
pub(crate) fn escape_pdf_literal(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    for &b in bytes {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'(' => out.extend_from_slice(b"\\("),
            b')' => out.extend_from_slice(b"\\)"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            _ => out.push(b),
        }
    }
    out
}
