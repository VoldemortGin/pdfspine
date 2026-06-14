//! Vector-path extraction — `page.get_drawings()` / `get_cdrawings()`
//! (PRD §8.8, resolves critique #10).
//!
//! The interpreter ([`pdf_text::ContentInterpreter`]) captures every painted
//! path (`m l c v y re h` construction + `S s f F f* B B* b b* n` paint) into a
//! flat [`pdf_text::DrawPath`] list in **PDF user space** (CTM already applied).
//! This module exposes that list in the PyMuPDF-shaped [`Drawing`] form:
//!
//! - [`get_cdrawings`] is the **raw** variant — geometry stays in PDF user space
//!   (y-up), the interpreter's native frame (PyMuPDF's `get_cdrawings` is the
//!   lower-level / un-postprocessed view).
//! - [`get_drawings`] maps every point through the page transform `P_r` into
//!   PyMuPDF **device space** (top-left, y-down, `/Rotate` applied), exactly as
//!   the text path does, so drawing geometry lines up with `get_text` boxes.
//!
//! Both produce the same `type`/`color`/`fill`/`width`/`dashes`/`close_path`/
//! `even_odd`/`items` shape; only the coordinate frame differs.

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_core::{pagetree, DocumentStore};
use pdf_text::{interpret_page, DrawPath, PaintKind, PathItem};

/// One extracted drawing (PyMuPDF `get_drawings` dict).
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    /// The paint kind (`"s"` stroke / `"f"` fill / `"fs"` both).
    pub kind: PaintKind,
    /// The path bounding rect (in the chosen frame).
    pub rect: Rect,
    /// The stroke color packed `0x00RRGGBB`, if stroked.
    pub color: Option<u32>,
    /// The fill color packed `0x00RRGGBB`, if filled.
    pub fill: Option<u32>,
    /// The stroke line width.
    pub width: f64,
    /// The dash-pattern string (empty when solid).
    pub dashes: String,
    /// Whether the last sub-path was closed.
    pub close_path: bool,
    /// Whether an even-odd fill rule was used.
    pub even_odd: bool,
    /// The path items, in construction order (in the chosen frame).
    pub items: Vec<DrawItem>,
}

impl Drawing {
    /// The PyMuPDF `type` string for this drawing.
    #[must_use]
    pub fn type_str(&self) -> &'static str {
        self.kind.as_str()
    }
}

/// One path item (PyMuPDF `items` tuple), in the chosen coordinate frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawItem {
    /// `("l", p1, p2)` — a straight segment.
    Line(Point, Point),
    /// `("c", p1, p2, p3, p4)` — a cubic Bézier (start, ctrl1, ctrl2, end).
    Curve(Point, Point, Point, Point),
    /// `("re", rect)` — an axis-aligned rectangle.
    Rect(Rect),
}

/// `page.get_cdrawings()` — the raw vector paths of the page at `index`, in
/// **PDF user space** (the interpreter's native y-up frame). Returns an empty
/// vector for a missing page / no vector content (never panics).
#[must_use]
pub fn get_cdrawings(doc: &DocumentStore, index: usize) -> Vec<Drawing> {
    drawings_in_frame(doc, index, None)
}

/// `page.get_drawings()` — the vector paths of the page at `index`, mapped into
/// PyMuPDF **device space** (top-left, y-down, `/Rotate` applied). Returns an
/// empty vector for a missing page / no vector content (never panics).
#[must_use]
pub fn get_drawings(doc: &DocumentStore, index: usize) -> Vec<Drawing> {
    let Some(leaf) = pagetree::page_refs(doc).get(index).copied() else {
        return Vec::new();
    };
    let mb = pagetree::mediabox(doc, leaf);
    let rotate = pagetree::rotation(doc, leaf);
    let pr = pdf_text::page_transform(mb, rotate);
    drawings_in_frame(doc, index, Some(pr))
}

/// Shared core: interpret the page, then convert each [`DrawPath`] (user space)
/// to a [`Drawing`], applying the optional extra `frame` matrix to every point
/// (the device transform for `get_drawings`; `None` keeps user space).
fn drawings_in_frame(doc: &DocumentStore, index: usize, frame: Option<Matrix>) -> Vec<Drawing> {
    let Some(leaf) = pagetree::page_refs(doc).get(index).copied() else {
        return Vec::new();
    };
    let Some(page) = pagetree::page_dict(doc, leaf) else {
        return Vec::new();
    };
    let result = interpret_page(doc, &page);
    result.drawings.iter().map(|p| convert(p, frame)).collect()
}

/// Converts one interpreter [`DrawPath`] into a [`Drawing`], applying `frame`.
fn convert(path: &DrawPath, frame: Option<Matrix>) -> Drawing {
    let tp = |p: Point| match frame {
        Some(m) => p.transform(&m),
        None => p,
    };
    let items: Vec<DrawItem> = path
        .items
        .iter()
        .map(|it| match *it {
            PathItem::Line(a, b) => DrawItem::Line(tp(a), tp(b)),
            PathItem::Curve(a, b, c, d) => DrawItem::Curve(tp(a), tp(b), tp(c), tp(d)),
            PathItem::Rect(r) => {
                // Transform corners and take the axis-aligned envelope.
                let p0 = tp(Point::new(r.x0, r.y0));
                let p1 = tp(Point::new(r.x1, r.y1));
                DrawItem::Rect(Rect::new(p0.x, p0.y, p1.x, p1.y).normalize())
            }
        })
        .collect();
    let rect = {
        let p0 = tp(Point::new(path.rect.x0, path.rect.y0));
        let p1 = tp(Point::new(path.rect.x1, path.rect.y1));
        Rect::new(p0.x, p0.y, p1.x, p1.y).normalize()
    };
    Drawing {
        kind: path.kind,
        rect,
        color: path.color,
        fill: path.fill,
        width: path.width,
        dashes: path.dashes.clone(),
        close_path: path.close_path,
        even_odd: path.even_odd,
        items,
    }
}
