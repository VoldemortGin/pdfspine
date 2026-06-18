//! Annotations — the `add_*_annot` family, the [`Annot`] handle, and `/AP /N`
//! appearance-stream generation (PRD §8.8 / §12 M4 exit).
//!
//! Every annotation is an indirect dictionary in the page's `/Annots` array with
//! `/Type /Annot`, a `/Subtype`, a `/Rect`, and — the load-bearing portability
//! requirement — a generated `/AP /N` Form XObject so non-Acrobat viewers render
//! it. Appearance streams reuse the operator emitters' conventions from
//! [`crate::drawing`] / [`crate::content`] / [`crate::color`].
//!
//! Coordinate model: callers pass geometry in **PyMuPDF top-left page space**
//! (origin top-left, y **down**). It is converted to PDF user space (y up) via
//! [`PageContent`] exactly as the content-insertion path does, so annotation
//! geometry lines up with inserted text/vector content. The appearance stream's
//! `/BBox` is set to the annotation `/Rect` with `/Matrix` identity, and the
//! stream draws in the rect's own (translated) coordinate frame.

use pdf_core::error::{Error, Result};
use pdf_core::geom::{Matrix, Point, Quad, Rect};
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::pagetree;
use pdf_core::{DocumentStore, PdfString, StringKind};

use crate::color::Color;
use crate::content::{escape_pdf_literal, fmt_num, PageContent};

/// The annotation subtype (`/Subtype`). Mirrors the PyMuPDF / ISO 32000-1 family
/// implemented in M4b. Stored on the dict as the corresponding name.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AnnotType {
    /// Sticky-note `/Text` annotation (icon appearance).
    Text,
    /// `/FreeText` — text drawn directly on the page.
    FreeText,
    /// `/Highlight` text markup.
    Highlight,
    /// `/Underline` text markup.
    Underline,
    /// `/StrikeOut` text markup.
    StrikeOut,
    /// `/Squiggly` text markup.
    Squiggly,
    /// `/Square` (rectangle).
    Square,
    /// `/Circle` (ellipse).
    Circle,
    /// `/Line` segment.
    Line,
    /// `/Polygon` (closed path).
    Polygon,
    /// `/PolyLine` (open path).
    PolyLine,
    /// `/Ink` (free-form strokes).
    Ink,
    /// `/Stamp`.
    Stamp,
    /// `/FileAttachment`.
    FileAttachment,
    /// `/Redact` (created only; applying is M4d).
    Redact,
    /// `/Link` (read-through; created by [`crate::links`]).
    Link,
    /// `/Widget` (form field; read-through, authored in M4c).
    Widget,
    /// An unrecognized subtype.
    Other,
}

impl AnnotType {
    /// The PDF `/Subtype` name for this type.
    #[must_use]
    pub fn pdf_name(self) -> &'static str {
        match self {
            AnnotType::Text => "Text",
            AnnotType::FreeText => "FreeText",
            AnnotType::Highlight => "Highlight",
            AnnotType::Underline => "Underline",
            AnnotType::StrikeOut => "StrikeOut",
            AnnotType::Squiggly => "Squiggly",
            AnnotType::Square => "Square",
            AnnotType::Circle => "Circle",
            AnnotType::Line => "Line",
            AnnotType::Polygon => "Polygon",
            AnnotType::PolyLine => "PolyLine",
            AnnotType::Ink => "Ink",
            AnnotType::Stamp => "Stamp",
            AnnotType::FileAttachment => "FileAttachment",
            AnnotType::Redact => "Redact",
            AnnotType::Link => "Link",
            AnnotType::Widget => "Widget",
            AnnotType::Other => "Annot",
        }
    }

    /// Classifies a `/Subtype` name.
    #[must_use]
    pub fn from_name(name: &[u8]) -> AnnotType {
        match name {
            b"Text" => AnnotType::Text,
            b"FreeText" => AnnotType::FreeText,
            b"Highlight" => AnnotType::Highlight,
            b"Underline" => AnnotType::Underline,
            b"StrikeOut" => AnnotType::StrikeOut,
            b"Squiggly" => AnnotType::Squiggly,
            b"Square" => AnnotType::Square,
            b"Circle" => AnnotType::Circle,
            b"Line" => AnnotType::Line,
            b"Polygon" => AnnotType::Polygon,
            b"PolyLine" => AnnotType::PolyLine,
            b"Ink" => AnnotType::Ink,
            b"Stamp" => AnnotType::Stamp,
            b"FileAttachment" => AnnotType::FileAttachment,
            b"Redact" => AnnotType::Redact,
            b"Link" => AnnotType::Link,
            b"Widget" => AnnotType::Widget,
            _ => AnnotType::Other,
        }
    }
}

/// MuPDF's "infinite rectangle" sentinel, returned by `popup_rect` /
/// `apn_bbox` when the underlying value is absent. These are the exact `int`
/// bounds MuPDF uses (`FZ_MIN_INF_RECT` / `FZ_MAX_INF_RECT`).
pub const FZ_INFINITE_RECT: Rect = Rect::new(
    -2_147_483_648.0,
    -2_147_483_648.0,
    2_147_483_520.0,
    2_147_483_520.0,
);

/// A handle to one annotation: the document, owning page leaf and the
/// annotation's object reference. Property accessors read live through the
/// ChangeSet overlay; mutators rewrite the dict; [`Annot::update`] regenerates
/// `/AP /N`.
pub struct Annot<'a> {
    doc: &'a DocumentStore,
    /// The page-leaf reference (for `/MediaBox` and coordinate conversion).
    leaf: ObjRef,
    /// The annotation object reference.
    obj: ObjRef,
}

impl<'a> Annot<'a> {
    /// Wraps an existing annotation reference known to live on `leaf`.
    #[must_use]
    pub fn from_ref(doc: &'a DocumentStore, leaf: ObjRef, obj: ObjRef) -> Self {
        Annot { doc, leaf, obj }
    }

    /// The annotation object reference (`xref`).
    #[must_use]
    pub fn xref(&self) -> ObjRef {
        self.obj
    }

    /// The annotation dictionary (cloned through the overlay).
    fn dict(&self) -> Result<Dict> {
        self.doc
            .resolve(self.obj)?
            .as_dict()
            .cloned()
            .ok_or(Error::InvalidArgument("annotation is not a dictionary"))
    }

    fn write_dict(&self, d: Dict) -> Result<()> {
        self.doc.update_object(self.obj, Object::Dictionary(d))
    }

    fn page_content(&self) -> PageContent<'a> {
        PageContent::from_leaf(self.doc, self.leaf)
    }

    // --- accessors --------------------------------------------------------

    /// The annotation `/Subtype`.
    #[must_use]
    pub fn annot_type(&self) -> AnnotType {
        self.dict()
            .ok()
            .and_then(|d| {
                d.get(&Name::new("Subtype"))
                    .and_then(Object::as_name)
                    .map(|n| AnnotType::from_name(n.as_bytes()))
            })
            .unwrap_or(AnnotType::Other)
    }

    /// The annotation `/Rect` (PDF user space, normalized).
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.dict()
            .ok()
            .map(|d| read_rect(&d))
            .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0))
    }

    /// The stroke color `/C`, if set.
    #[must_use]
    pub fn color(&self) -> Option<Color> {
        self.dict().ok().and_then(|d| read_color(&d, "C"))
    }

    /// The interior/fill color `/IC`, if set.
    #[must_use]
    pub fn fill_color(&self) -> Option<Color> {
        self.dict().ok().and_then(|d| read_color(&d, "IC"))
    }

    /// The constant opacity `/CA` (default `1.0`).
    #[must_use]
    pub fn opacity(&self) -> f64 {
        self.dict()
            .ok()
            .and_then(|d| d.get(&Name::new("CA")).and_then(Object::as_f64))
            .unwrap_or(1.0)
    }

    /// The border width (from `/BS /W`, falling back to `/Border[2]`).
    #[must_use]
    pub fn border_width(&self) -> f64 {
        self.dict()
            .ok()
            .map(|d| read_border_width(&d))
            .unwrap_or(1.0)
    }

    /// The annotation flags `/F`.
    #[must_use]
    pub fn flags(&self) -> i64 {
        self.dict()
            .ok()
            .and_then(|d| d.get(&Name::new("F")).and_then(Object::as_i64))
            .unwrap_or(0)
    }

    /// The `/Contents` text.
    #[must_use]
    pub fn contents(&self) -> String {
        self.string_key("Contents")
    }

    /// The `/T` title (author).
    #[must_use]
    pub fn title(&self) -> String {
        self.string_key("T")
    }

    /// The `/NM` annotation name.
    #[must_use]
    pub fn name(&self) -> String {
        self.string_key("NM")
    }

    fn string_key(&self, key: &str) -> String {
        self.dict()
            .ok()
            .and_then(|d| {
                d.get(&Name::new(key))
                    .and_then(Object::as_string)
                    .map(decode_text_string)
            })
            .unwrap_or_default()
    }

    /// The `/Vertices` array (Polygon / PolyLine), as user-space points.
    #[must_use]
    pub fn vertices(&self) -> Vec<Point> {
        self.dict()
            .ok()
            .map(|d| read_points(&d, "Vertices"))
            .unwrap_or_default()
    }

    /// The `/QuadPoints` (text markup), grouped into [`Quad`]s (Acrobat order).
    #[must_use]
    pub fn quad_points(&self) -> Vec<Quad> {
        self.dict().ok().map(|d| read_quads(&d)).unwrap_or_default()
    }

    /// Whether an `/AP /N` appearance stream is present and non-empty.
    #[must_use]
    pub fn has_appearance(&self) -> bool {
        self.appearance_ref()
            .and_then(|r| self.doc.resolve(r).ok())
            .and_then(|o| o.as_stream().cloned())
            .and_then(|s| self.doc.decode_stream(&s).ok()?.into_decoded().ok())
            .map(|b| !b.is_empty())
            .unwrap_or(false)
    }

    /// The `/AP /N` Form XObject reference, if present.
    #[must_use]
    pub fn appearance_ref(&self) -> Option<ObjRef> {
        let d = self.dict().ok()?;
        let ap = match d.get(&Name::new("AP"))? {
            Object::Dictionary(ap) => ap.clone(),
            Object::Reference(r) => self.doc.resolve(*r).ok()?.as_dict().cloned()?,
            _ => return None,
        };
        ap.get(&Name::new("N")).and_then(Object::as_reference)
    }

    // --- mutators ---------------------------------------------------------

    /// Sets `/Rect` (PDF user space). Does **not** regenerate `/AP` — call
    /// [`Annot::update`] afterwards.
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_rect(&self, rect: Rect) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(Name::new("Rect"), rect_array(&rect.normalize()));
        self.write_dict(d)
    }

    /// Sets stroke `/C` and/or interior `/IC` color (`None` leaves a key
    /// untouched; pass an explicit value to set it).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_colors(&self, stroke: Option<Color>, fill: Option<Color>) -> Result<()> {
        let mut d = self.dict()?;
        if let Some(c) = stroke {
            d.insert(Name::new("C"), color_array(c));
        }
        if let Some(c) = fill {
            d.insert(Name::new("IC"), color_array(c));
        }
        self.write_dict(d)
    }

    /// Sets the constant opacity `/CA` (clamped to `0.0..=1.0`).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_opacity(&self, opacity: f64) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(Name::new("CA"), Object::Real(opacity.clamp(0.0, 1.0)));
        self.write_dict(d)
    }

    /// Sets the border width (`/BS /W` + a matching `/Border` array).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_border(&self, width: f64) -> Result<()> {
        let mut d = self.dict()?;
        let mut bs = Dict::new();
        bs.insert(Name::new("Type"), Object::Name(Name::new("Border")));
        bs.insert(Name::new("W"), Object::Real(width));
        bs.insert(Name::new("S"), Object::Name(Name::new("S")));
        d.insert(Name::new("BS"), Object::Dictionary(bs));
        d.insert(
            Name::new("Border"),
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(width),
            ]),
        );
        self.write_dict(d)
    }

    /// Sets the annotation flags `/F`.
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_flags(&self, flags: i64) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(Name::new("F"), Object::Integer(flags));
        self.write_dict(d)
    }

    /// Sets the info fields: `/Contents`, `/T` (title), `/NM` (name). Each
    /// `Some` value is written; `None` leaves the key untouched.
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_info(
        &self,
        contents: Option<&str>,
        title: Option<&str>,
        name: Option<&str>,
    ) -> Result<()> {
        let mut d = self.dict()?;
        if let Some(c) = contents {
            d.insert(Name::new("Contents"), text_string(c));
        }
        if let Some(t) = title {
            d.insert(Name::new("T"), text_string(t));
        }
        if let Some(n) = name {
            d.insert(Name::new("NM"), text_string(n));
        }
        self.write_dict(d)
    }

    /// The line-ending styles `/LE` `[start end]`, as PyMuPDF `PDF_ANNOT_LE_*`
    /// integer codes (PyMuPDF `Annot.line_ends`). Defaults to `(0, 0)` when `/LE`
    /// is absent.
    #[must_use]
    pub fn line_ends(&self) -> (i64, i64) {
        let Ok(d) = self.dict() else {
            return (0, 0);
        };
        let Some(a) = d.get(&Name::new("LE")).and_then(Object::as_array) else {
            return (0, 0);
        };
        if a.len() != 2 {
            return (0, 0);
        }
        let code = |o: &Object| o.as_name().map(|n| le_code(n.as_bytes())).unwrap_or(0);
        (code(&a[0]), code(&a[1]))
    }

    /// Sets the line-ending styles `/LE` `[/<start> /<end>]` from PyMuPDF
    /// `PDF_ANNOT_LE_*` integer codes (PyMuPDF `Annot.set_line_ends`).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_line_ends(&self, start: i64, end: i64) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(
            Name::new("LE"),
            Object::Array(vec![
                Object::Name(Name::new(le_name(start))),
                Object::Name(Name::new(le_name(end))),
            ]),
        );
        self.write_dict(d)
    }

    /// The blend mode `/BM` name (PyMuPDF `Annot.blendmode`), `None` if absent.
    #[must_use]
    pub fn blendmode(&self) -> Option<String> {
        self.dict().ok().and_then(|d| {
            d.get(&Name::new("BM"))
                .and_then(Object::as_name)
                .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
        })
    }

    /// Sets the blend mode `/BM` name (PyMuPDF `Annot.set_blendmode`).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_blendmode(&self, mode: &str) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(Name::new("BM"), Object::Name(Name::new(mode)));
        self.write_dict(d)
    }

    /// Sets the icon/appearance name `/Name` (PyMuPDF `Annot.set_name`). This is
    /// the appearance name (e.g. a `/Text` icon or `/Stamp` label), **not** the
    /// `/NM` annotation identifier.
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_name(&self, name: &str) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(Name::new("Name"), Object::Name(Name::new(name)));
        self.write_dict(d)
    }

    /// Whether the `/Open` flag is set (PyMuPDF `Annot.is_open`); default `false`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.dict()
            .ok()
            .and_then(|d| d.get(&Name::new("Open")).and_then(Object::as_bool))
            .unwrap_or(false)
    }

    /// Sets the `/Open` flag (PyMuPDF `Annot.set_open`).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_open(&self, open: bool) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(Name::new("Open"), Object::Boolean(open));
        self.write_dict(d)
    }

    /// The border as `(width, style, dashes)` (backs PyMuPDF `Annot.border`'s
    /// `{width, style, dashes}` dict): width from `/BS /W` (else `/Border[2]`),
    /// style from `/BS /S` (default `"S"`), dashes from `/BS /D` (default empty).
    #[must_use]
    pub fn border(&self) -> (f64, String, Vec<f64>) {
        let width = self.border_width();
        let (mut style, mut dashes) = (String::from("S"), Vec::new());
        if let Ok(d) = self.dict() {
            if let Some(bs) = d.get(&Name::new("BS")).and_then(Object::as_dict) {
                if let Some(s) = bs.get(&Name::new("S")).and_then(Object::as_name) {
                    style = String::from_utf8_lossy(s.as_bytes()).into_owned();
                }
                if let Some(da) = bs.get(&Name::new("D")).and_then(Object::as_array) {
                    dashes = da.iter().filter_map(Object::as_f64).collect();
                }
            }
        }
        (width, style, dashes)
    }

    /// The page leaf's `/MediaBox` height `y1`, used to convert annotation
    /// rectangles between PDF user space (y-up) and PyMuPDF page space (y-down).
    fn page_top(&self) -> f64 {
        pagetree::mediabox(self.doc, self.leaf).normalize().y1
    }

    /// Sets the annotation `/Rotate` value (PyMuPDF `Annot.set_rotation`).
    ///
    /// The value is normalized into `[0, 360)` exactly as fitz does (Euclidean
    /// remainder): `-1 -> 359`, `-90 -> 270`, `360 -> 0`, `450 -> 90`,
    /// `720 -> 0`. The key is always written (never removed).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_rotation(&self, rotate: i64) -> Result<()> {
        let mut d = self.dict()?;
        d.insert(Name::new("Rotate"), Object::Integer(rotate.rem_euclid(360)));
        self.write_dict(d)
    }

    /// The padding deltas `(left, top, right, bottom)` between `/Rect` and the
    /// visible drawing, from `/RD` `[l t r b]` as `(l, t, -r, -b)` (PyMuPDF
    /// `Annot.rect_delta`). `None` when `/RD` is absent.
    #[must_use]
    pub fn rect_delta(&self) -> Option<(f64, f64, f64, f64)> {
        let d = self.dict().ok()?;
        let a = d.get(&Name::new("RD")).and_then(Object::as_array)?;
        if a.len() != 4 {
            return None;
        }
        let v: Vec<f64> = a.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
        Some((v[0], v[1], -v[2], -v[3]))
    }

    /// Whether a `/Popup` entry is present (PyMuPDF `Annot.has_popup`).
    #[must_use]
    pub fn has_popup(&self) -> bool {
        self.dict()
            .ok()
            .map(|d| d.contains_key(&Name::new("Popup")))
            .unwrap_or(false)
    }

    /// The `/Popup` annotation's object number, or `0` if absent (PyMuPDF
    /// `Annot.popup_xref`).
    #[must_use]
    pub fn popup_xref(&self) -> u32 {
        self.popup_ref().map(|r| r.num).unwrap_or(0)
    }

    /// Resolves the `/Popup` reference.
    fn popup_ref(&self) -> Option<ObjRef> {
        self.dict()
            .ok()?
            .get(&Name::new("Popup"))
            .and_then(Object::as_reference)
    }

    /// The `/Popup` annotation's `/Rect` in PyMuPDF page space (y-down), or
    /// `None` if absent (PyMuPDF `Annot.popup_rect`).
    #[must_use]
    pub fn popup_rect(&self) -> Option<Rect> {
        let r = self.popup_ref()?;
        let pd = self.doc.resolve(r).ok()?.as_dict().cloned()?;
        let ur = read_rect(&pd);
        Some(self.user_to_page(ur))
    }

    /// Adds (or replaces) a `/Popup` child annotation covering `rect` (PyMuPDF
    /// page space, y-down). The popup is linked back to this annotation via
    /// `/Parent` and appended to the page's `/Annots`.
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_popup(&self, rect: Rect) -> Result<()> {
        // fitz stores the RAW (non-normalized) /Rect; popup_rect normalizes on
        // READ. Convert page space (y-down) -> user space (y-up) WITHOUT
        // normalizing so an inverted input rect round-trips byte-identically.
        let top = self.page_top();
        let ur = Rect::new(rect.x0, top - rect.y1, rect.x1, top - rect.y0);
        let mut pd = Dict::new();
        pd.insert(Name::new("Type"), Object::Name(Name::new("Annot")));
        pd.insert(Name::new("Subtype"), Object::Name(Name::new("Popup")));
        pd.insert(Name::new("Rect"), rect_array(&ur));
        pd.insert(Name::new("Parent"), Object::Reference(self.obj));
        let popup_ref = match self.popup_ref() {
            Some(r) => {
                self.doc.update_object(r, Object::Dictionary(pd))?;
                r
            }
            None => {
                let r = self.doc.add_object(Object::Dictionary(pd))?;
                append_to_annots(self.doc, self.leaf, r)?;
                r
            }
        };
        let mut d = self.dict()?;
        d.insert(Name::new("Popup"), Object::Reference(popup_ref));
        self.write_dict(d)
    }

    /// Resolves the `/AP /N` Form XObject stream dict, if present.
    fn apn_stream_dict(&self) -> Option<Dict> {
        let r = self.appearance_ref()?;
        self.doc
            .resolve(r)
            .ok()?
            .as_stream()
            .map(|s| s.dict.clone())
    }

    /// The `/AP /N` appearance stream's `/BBox` in PyMuPDF page space (y-down)
    /// (PyMuPDF `Annot.apn_bbox`). When there is no `/AP /N` stream, or it has no
    /// `/BBox`, MuPDF returns its infinite-rect sentinel (the same one
    /// `popup_rect` yields for an absent popup), so we match it.
    #[must_use]
    pub fn apn_bbox(&self) -> Option<Rect> {
        let read = || -> Option<Rect> {
            let sd = self.apn_stream_dict()?;
            let a = sd.get(&Name::new("BBox")).and_then(Object::as_array)?;
            if a.len() != 4 {
                return None;
            }
            let v: Vec<f64> = a.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
            Some(self.user_to_page(Rect::new(v[0], v[1], v[2], v[3])))
        };
        Some(read().unwrap_or(FZ_INFINITE_RECT))
    }

    /// The `/AP /N` appearance stream's `/Matrix`, or `None` (PyMuPDF
    /// `Annot.apn_matrix`). Defaults to the identity matrix when absent.
    #[must_use]
    pub fn apn_matrix(&self) -> Matrix {
        let read = || -> Option<Matrix> {
            let sd = self.apn_stream_dict()?;
            let a = sd.get(&Name::new("Matrix")).and_then(Object::as_array)?;
            if a.len() != 6 {
                return None;
            }
            let v: Vec<f64> = a.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
            Some(Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5]))
        };
        read().unwrap_or(Matrix::IDENTITY)
    }

    /// Sets the `/AP /N` appearance stream's `/BBox` (PyMuPDF
    /// `Annot.set_apn_bbox`), taking a PyMuPDF page-space rect (y-down).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if there is no appearance stream; else
    /// propagates object-edit errors.
    pub fn set_apn_bbox(&self, rect: Rect) -> Result<()> {
        let ur = self.page_to_user(rect).normalize();
        self.mutate_apn(|sd| {
            sd.insert(Name::new("BBox"), rect_array(&ur));
        })
    }

    /// Sets the `/AP /N` appearance stream's `/Matrix` (PyMuPDF
    /// `Annot.set_apn_matrix`).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if there is no appearance stream; else
    /// propagates object-edit errors.
    pub fn set_apn_matrix(&self, m: Matrix) -> Result<()> {
        self.mutate_apn(|sd| {
            sd.insert(
                Name::new("Matrix"),
                Object::Array(vec![
                    Object::Real(m.a),
                    Object::Real(m.b),
                    Object::Real(m.c),
                    Object::Real(m.d),
                    Object::Real(m.e),
                    Object::Real(m.f),
                ]),
            );
        })
    }

    /// Applies `f` to the `/AP /N` stream dict and writes it back.
    fn mutate_apn(&self, f: impl FnOnce(&mut Dict)) -> Result<()> {
        let r = self
            .appearance_ref()
            .ok_or(Error::InvalidArgument("annotation has no /AP /N stream"))?;
        let mut stream = self
            .doc
            .resolve(r)?
            .as_stream()
            .cloned()
            .ok_or(Error::InvalidArgument("/AP /N is not a stream"))?;
        f(&mut stream.dict);
        self.doc.update_object(r, Object::Stream(stream))
    }

    /// The `/Lang` language identifier, or empty (PyMuPDF `Annot.language`).
    ///
    /// DEVIATION (oxide is more faithful): oxide returns the `/Lang` tag verbatim
    /// and `""` for an absent key. PyMuPDF normalizes via MuPDF `fz_text_language`
    /// (a lossy table: `en-US -> en`, `zh-CN -> zh-Hans`) and leaks the *system
    /// locale* for an absent key. We keep the verbatim/deterministic behavior.
    #[must_use]
    pub fn language(&self) -> String {
        self.dict()
            .ok()
            .and_then(|d| {
                d.get(&Name::new("Lang"))
                    .and_then(Object::as_string)
                    .map(decode_text_string)
            })
            .unwrap_or_default()
    }

    /// Sets the `/Lang` language identifier (PyMuPDF `Annot.set_language`). An
    /// empty string removes the key.
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn set_language(&self, lang: &str) -> Result<()> {
        let mut d = self.dict()?;
        if lang.is_empty() {
            d.remove(&Name::new("Lang"));
        } else {
            d.insert(
                Name::new("Lang"),
                Object::String(PdfString::literal(lang.as_bytes().to_vec())),
            );
        }
        self.write_dict(d)
    }

    /// The in-reply-to annotation's object number from `/IRT`, or `0` (PyMuPDF
    /// `Annot.irt_xref`).
    #[must_use]
    pub fn irt_xref(&self) -> u32 {
        self.dict()
            .ok()
            .and_then(|d| d.get(&Name::new("IRT")).and_then(Object::as_reference))
            .map(|r| r.num)
            .unwrap_or(0)
    }

    /// Sets `/IRT` (in-reply-to) to the annotation with object number `xref`
    /// (PyMuPDF `Annot.set_irt_xref`).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if `xref` is not an annotation on this page;
    /// else propagates object-edit errors.
    pub fn set_irt_xref(&self, xref: u32) -> Result<()> {
        let target = annot_refs_on_leaf(self.doc, self.leaf)
            .into_iter()
            .find(|r| r.num == xref)
            .ok_or(Error::InvalidArgument(
                "set_irt_xref: xref is not an annotation on this page",
            ))?;
        let mut d = self.dict()?;
        d.insert(Name::new("IRT"), Object::Reference(target));
        self.write_dict(d)
    }

    /// Deletes every annotation on the page that replies to this one (whose
    /// `/IRT` resolves to this annotation), along with their popups (PyMuPDF
    /// `Annot.delete_responses`).
    ///
    /// # Errors
    /// Propagates object-edit errors.
    pub fn delete_responses(&self) -> Result<()> {
        let me = self.obj.num;
        for r in annot_refs_on_leaf(self.doc, self.leaf) {
            if r == self.obj {
                continue;
            }
            let Some(d) = self.doc.resolve(r).ok().and_then(|o| o.as_dict().cloned()) else {
                continue;
            };
            let is_reply = d
                .get(&Name::new("IRT"))
                .and_then(Object::as_reference)
                .map(|irt| irt.num == me)
                .unwrap_or(false);
            if is_reply {
                if let Some(popup) = d.get(&Name::new("Popup")).and_then(Object::as_reference) {
                    let _ = delete_annot_on_leaf(self.doc, self.leaf, popup);
                }
                delete_annot_on_leaf(self.doc, self.leaf, r)?;
            }
        }
        Ok(())
    }

    /// Sanitizes the `/AP /N` appearance stream by re-emitting its decoded
    /// content wrapped in a balanced `q … Q` graphics-state guard (PyMuPDF
    /// `Annot.clean_contents`). A no-op when there is no appearance stream.
    ///
    /// DEVIATION (kept): oxide produces valid, sanitized output (drops `/Filter`,
    /// balances `q … Q`) but does NOT run MuPDF's token-level minifier/reorderer,
    /// so the bytes differ from mupdf's minimized stream while remaining
    /// equivalently renderable.
    ///
    /// # Errors
    /// Propagates resolve / object-edit errors.
    pub fn clean_contents(&self) -> Result<()> {
        let Some(r) = self.appearance_ref() else {
            return Ok(());
        };
        let stream = self
            .doc
            .resolve(r)?
            .as_stream()
            .cloned()
            .ok_or(Error::InvalidArgument("/AP /N is not a stream"))?;
        let body = self.doc.decode_stream(&stream)?.into_decoded()?;
        let mut cleaned = Vec::with_capacity(body.len() + 4);
        cleaned.extend_from_slice(b"q\n");
        cleaned.extend_from_slice(trim_ascii_ws(&body));
        cleaned.extend_from_slice(b"\nQ");
        let mut dict = stream.dict;
        dict.remove(&Name::new("Filter"));
        dict.remove(&Name::new("DecodeParms"));
        dict.insert(Name::new("Length"), Object::Integer(cleaned.len() as i64));
        self.doc
            .update_object(r, Object::Stream(StreamObj::new_encoded(dict, cleaned)))
    }

    // --- /FileAttachment embedded-file accessors --------------------------

    /// Resolves the `/FS` file-specification dict.
    fn filespec(&self) -> Option<Dict> {
        let d = self.dict().ok()?;
        match d.get(&Name::new("FS"))? {
            Object::Dictionary(fs) => Some(fs.clone()),
            Object::Reference(r) => self.doc.resolve(*r).ok()?.as_dict().cloned(),
            _ => None,
        }
    }

    /// The embedded-file stream reference under `/FS /EF /F`.
    fn ef_stream_ref(&self) -> Option<ObjRef> {
        let fs = self.filespec()?;
        let ef = match fs.get(&Name::new("EF"))? {
            Object::Dictionary(ef) => ef.clone(),
            Object::Reference(r) => self.doc.resolve(*r).ok()?.as_dict().cloned()?,
            _ => return None,
        };
        ef.get(&Name::new("F")).and_then(Object::as_reference)
    }

    /// The embedded file's decoded bytes (PyMuPDF `Annot.get_file`).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if this is not a file attachment; else
    /// propagates resolve / decode errors.
    pub fn get_file(&self) -> Result<Vec<u8>> {
        let r = self
            .ef_stream_ref()
            .ok_or(Error::InvalidArgument("annotation has no embedded file"))?;
        let stream = self
            .doc
            .resolve(r)?
            .as_stream()
            .cloned()
            .ok_or(Error::InvalidArgument("/EF /F is not a stream"))?;
        self.doc.decode_stream(&stream)?.into_decoded()
    }

    /// The file-attachment metadata `(filename, description, length)` (PyMuPDF
    /// `Annot.file_info`). The description defaults to the *filename* when no
    /// `/Desc` is present, matching fitz.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if this is not a file attachment.
    pub fn file_info(&self) -> Result<(String, String, i64)> {
        let fs = self
            .filespec()
            .ok_or(Error::InvalidArgument("annotation has no /FS file spec"))?;
        let str_key = |k: &str| {
            fs.get(&Name::new(k))
                .and_then(Object::as_string)
                .map(decode_text_string)
        };
        let filename = str_key("UF").or_else(|| str_key("F")).unwrap_or_default();
        // fitz defaults the description to the filename when /Desc is absent.
        let desc = str_key("Desc").unwrap_or_else(|| filename.clone());
        let length = self.get_file().map(|b| b.len() as i64).unwrap_or(0);
        Ok((filename, desc, length))
    }

    /// Replaces the embedded file's content (and optionally its filename /
    /// description) (PyMuPDF `Annot.update_file`). `None` fields are left as-is.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if this is not a file attachment; else
    /// propagates object-edit errors.
    pub fn update_file(
        &self,
        buffer: Option<&[u8]>,
        filename: Option<&str>,
        desc: Option<&str>,
    ) -> Result<()> {
        let ef_ref = self
            .ef_stream_ref()
            .ok_or(Error::InvalidArgument("annotation has no embedded file"))?;
        if let Some(bytes) = buffer {
            let mut ef_dict = Dict::new();
            ef_dict.insert(Name::new("Type"), Object::Name(Name::new("EmbeddedFile")));
            ef_dict.insert(Name::new("Length"), Object::Integer(bytes.len() as i64));
            let mut params = Dict::new();
            params.insert(Name::new("Size"), Object::Integer(bytes.len() as i64));
            ef_dict.insert(Name::new("Params"), Object::Dictionary(params));
            self.doc.update_object(
                ef_ref,
                Object::Stream(StreamObj::new_encoded(ef_dict, bytes.to_vec())),
            )?;
        }
        if filename.is_some() || desc.is_some() {
            let fs_ref = match self.dict()?.get(&Name::new("FS")) {
                Some(Object::Reference(r)) => Some(*r),
                _ => None,
            };
            let mut fs = self
                .filespec()
                .ok_or(Error::InvalidArgument("annotation has no /FS file spec"))?;
            if let Some(f) = filename {
                fs.insert(Name::new("F"), text_string(f));
                fs.insert(Name::new("UF"), text_string(f));
            }
            if let Some(de) = desc {
                fs.insert(Name::new("Desc"), text_string(de));
            }
            match fs_ref {
                Some(r) => self.doc.update_object(r, Object::Dictionary(fs))?,
                None => {
                    let mut d = self.dict()?;
                    d.insert(Name::new("FS"), Object::Dictionary(fs));
                    self.write_dict(d)?;
                }
            }
        }
        Ok(())
    }

    /// Converts a PDF user-space rect (y-up) to PyMuPDF page space (y-down).
    fn user_to_page(&self, r: Rect) -> Rect {
        let top = self.page_top();
        Rect::new(r.x0, top - r.y1, r.x1, top - r.y0)
    }

    /// Converts a PyMuPDF page-space rect (y-down) to PDF user space (y-up).
    fn page_to_user(&self, r: Rect) -> Rect {
        let top = self.page_top();
        let r = r.normalize();
        Rect::new(r.x0, top - r.y1, r.x1, top - r.y0)
    }

    /// Regenerates the `/AP /N` appearance stream from the annotation's current
    /// properties (subtype, geometry, colors, opacity, border). This is what
    /// reflects a `set_colors` / `set_opacity` change into the appearance.
    ///
    /// # Errors
    /// Propagates resolve / object-edit errors.
    pub fn update(&self) -> Result<()> {
        let d = self.dict()?;
        let ty = AnnotType::from_name(
            d.get(&Name::new("Subtype"))
                .and_then(Object::as_name)
                .map(Name::as_bytes)
                .unwrap_or(b"Annot"),
        );
        let ap = build_appearance(self.doc, &self.page_content(), ty, &d)?;
        // Allocate (or reuse) the AP /N stream object.
        let n_ref = match self.appearance_ref() {
            Some(r) => {
                self.doc.update_object(r, Object::Stream(ap))?;
                r
            }
            None => self.doc.add_object(Object::Stream(ap))?,
        };
        let mut d = self.dict()?;
        let mut ap_dict = Dict::new();
        ap_dict.insert(Name::new("N"), Object::Reference(n_ref));
        d.insert(Name::new("AP"), Object::Dictionary(ap_dict));
        self.write_dict(d)
    }
}

// === the add_*_annot family ===============================================
//
// Each builder constructs the dict (subtype + geometry + defaults), adds it as
// an indirect object, generates `/AP /N`, and appends the ref to the page's
// `/Annots` array. Returns an [`Annot`] handle.

/// Common annotation builder: creates the dict from a starting `Dict`, registers
/// it, generates `/AP /N`, and links it onto the page.
fn finalize_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    mut d: Dict,
    ty: AnnotType,
) -> Result<Annot<'a>> {
    let leaf = *pagetree::page_refs(doc)
        .get(page)
        .ok_or(Error::Unsupported("page index out of range"))?;
    d.insert(Name::new("Type"), Object::Name(Name::new("Annot")));
    d.insert(Name::new("Subtype"), Object::Name(Name::new(ty.pdf_name())));
    d.entry(Name::new("P")).or_insert(Object::Reference(leaf));
    let obj = doc.add_object(Object::Dictionary(d))?;
    let annot = Annot { doc, leaf, obj };
    annot.update()?;
    append_to_annots(doc, leaf, obj)?;
    Ok(annot)
}

/// Appends `obj` to the page leaf's `/Annots` array (creating it if absent).
fn append_to_annots(doc: &DocumentStore, leaf: ObjRef, obj: ObjRef) -> Result<()> {
    let mut pd = doc
        .resolve(leaf)?
        .as_dict()
        .cloned()
        .ok_or(Error::InvalidArgument("page is not a dictionary"))?;
    let mut annots = match pd.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)?
            .as_array()
            .map(<[Object]>::to_vec)
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    annots.push(Object::Reference(obj));
    pd.insert(Name::new("Annots"), Object::Array(annots));
    doc.update_object(leaf, Object::Dictionary(pd))
}

/// `page.add_text_annot` — a sticky-note `/Text` annotation at `point`
/// (top-left page space) with the given note text and icon name.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_text_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    point: Point,
    text: &str,
    icon: &str,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let p = pc.to_user_space(point);
    // A note icon is conventionally ~18×20 pt anchored at the point.
    let rect = Rect::new(p.x, p.y - 20.0, p.x + 18.0, p.y);
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&rect.normalize()));
    d.insert(Name::new("Contents"), text_string(text));
    let icon = if icon.is_empty() { "Note" } else { icon };
    d.insert(Name::new("Name"), Object::Name(Name::new(icon)));
    d.insert(Name::new("C"), color_array(Color::new(1.0, 1.0, 0.0)));
    let annot = finalize_annot(doc, page, d, AnnotType::Text)?;
    // fitz auto-creates a child /Popup for a sticky-note: a /Subtype /Popup
    // annot with /Parent back-ref, offset to the right of the icon, appended to
    // /Annots and linked via /Popup on the note.
    let r = rect.normalize();
    let popup_rect = Rect::new(r.x1, r.y0 - 100.0, r.x1 + 200.0, r.y1);
    let mut popup = Dict::new();
    popup.insert(Name::new("Type"), Object::Name(Name::new("Annot")));
    popup.insert(Name::new("Subtype"), Object::Name(Name::new("Popup")));
    popup.insert(Name::new("Rect"), rect_array(&popup_rect.normalize()));
    popup.insert(Name::new("Parent"), Object::Reference(annot.obj));
    let popup_ref = doc.add_object(Object::Dictionary(popup))?;
    append_to_annots(doc, annot.leaf, popup_ref)?;
    let mut nd = annot.dict()?;
    nd.insert(Name::new("Popup"), Object::Reference(popup_ref));
    annot.write_dict(nd)?;
    Ok(annot)
}

/// `page.add_freetext_annot` — a `/FreeText` box with text drawn in it.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
#[allow(clippy::too_many_arguments)]
pub fn add_freetext_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    rect: Rect,
    text: &str,
    fontsize: f64,
    color: Color,
    fill: Option<Color>,
    align: i64,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let ur = pc.rect_to_user_space(rect);
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&ur));
    d.insert(Name::new("Contents"), text_string(text));
    // /DA default appearance string (font / size / color) per ISO 32000-1.
    let da = format!(
        "{} {} {} rg /Helv {} Tf",
        fmt_num(color.r.clamp(0.0, 1.0)),
        fmt_num(color.g.clamp(0.0, 1.0)),
        fmt_num(color.b.clamp(0.0, 1.0)),
        fmt_num(fontsize)
    );
    d.insert(Name::new("DA"), text_string(&da));
    d.insert(Name::new("Q"), Object::Integer(align));
    d.insert(Name::new("C"), color_array(color));
    if let Some(f) = fill {
        d.insert(Name::new("IC"), color_array(f));
    }
    finalize_annot(doc, page, d, AnnotType::FreeText)
}

/// Builds a text-markup annotation (`Highlight` / `Underline` / `StrikeOut` /
/// `Squiggly`) from quadpoints in **top-left page space**.
fn add_markup_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    quads: &[Quad],
    ty: AnnotType,
    default_color: Color,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    // Convert each quad's points to user space and compute the bounding rect.
    let mut qp: Vec<f64> = Vec::with_capacity(quads.len() * 8);
    let mut rect = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for q in quads {
        // Acrobat QuadPoints order: (ul, ur, ll, lr) — x1 y1 x2 y2 x3 y3 x4 y4.
        let pts = [
            pc.to_user_space(q.ul),
            pc.to_user_space(q.ur),
            pc.to_user_space(q.ll),
            pc.to_user_space(q.lr),
        ];
        for p in &pts {
            qp.push(p.x);
            qp.push(p.y);
            rect.x0 = rect.x0.min(p.x);
            rect.y0 = rect.y0.min(p.y);
            rect.x1 = rect.x1.max(p.x);
            rect.y1 = rect.y1.max(p.y);
        }
    }
    if quads.is_empty() {
        rect = Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&rect));
    d.insert(
        Name::new("QuadPoints"),
        Object::Array(qp.into_iter().map(Object::Real).collect()),
    );
    d.insert(Name::new("C"), color_array(default_color));
    finalize_annot(doc, page, d, ty)
}

/// `page.add_highlight_annot` — a `/Highlight` over `quads` (yellow default).
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_highlight_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    quads: &[Quad],
) -> Result<Annot<'a>> {
    add_markup_annot(
        doc,
        page,
        quads,
        AnnotType::Highlight,
        Color::new(1.0, 1.0, 0.0),
    )
}

/// `page.add_underline_annot` — an `/Underline` over `quads`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_underline_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    quads: &[Quad],
) -> Result<Annot<'a>> {
    add_markup_annot(doc, page, quads, AnnotType::Underline, Color::BLACK)
}

/// `page.add_strikeout_annot` — a `/StrikeOut` over `quads`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_strikeout_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    quads: &[Quad],
) -> Result<Annot<'a>> {
    add_markup_annot(doc, page, quads, AnnotType::StrikeOut, Color::BLACK)
}

/// `page.add_squiggly_annot` — a `/Squiggly` over `quads`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_squiggly_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    quads: &[Quad],
) -> Result<Annot<'a>> {
    add_markup_annot(doc, page, quads, AnnotType::Squiggly, Color::BLACK)
}

/// `page.add_rect_annot` — a `/Square` annotation fitting `rect`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_rect_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    rect: Rect,
    color: Option<Color>,
    fill: Option<Color>,
) -> Result<Annot<'a>> {
    add_geom_box(doc, page, rect, color, fill, AnnotType::Square)
}

/// `page.add_circle_annot` — a `/Circle` annotation fitting `rect`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_circle_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    rect: Rect,
    color: Option<Color>,
    fill: Option<Color>,
) -> Result<Annot<'a>> {
    add_geom_box(doc, page, rect, color, fill, AnnotType::Circle)
}

fn add_geom_box<'a>(
    doc: &'a DocumentStore,
    page: usize,
    rect: Rect,
    color: Option<Color>,
    fill: Option<Color>,
    ty: AnnotType,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let ur = pc.rect_to_user_space(rect);
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&ur));
    d.insert(Name::new("C"), color_array(color.unwrap_or(Color::BLACK)));
    if let Some(f) = fill {
        d.insert(Name::new("IC"), color_array(f));
    }
    set_default_border(&mut d, 1.0);
    finalize_annot(doc, page, d, ty)
}

/// `page.add_line_annot` — a `/Line` from `p1` to `p2` (top-left page space).
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_line_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    p1: Point,
    p2: Point,
    color: Option<Color>,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let a = pc.to_user_space(p1);
    let b = pc.to_user_space(p2);
    let pad = 4.0;
    let rect = Rect::new(
        a.x.min(b.x) - pad,
        a.y.min(b.y) - pad,
        a.x.max(b.x) + pad,
        a.y.max(b.y) + pad,
    );
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&rect));
    d.insert(
        Name::new("L"),
        Object::Array(vec![
            Object::Real(a.x),
            Object::Real(a.y),
            Object::Real(b.x),
            Object::Real(b.y),
        ]),
    );
    d.insert(Name::new("C"), color_array(color.unwrap_or(Color::BLACK)));
    set_default_border(&mut d, 1.0);
    finalize_annot(doc, page, d, AnnotType::Line)
}

/// `page.add_polygon_annot` — a closed `/Polygon` through `points`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_polygon_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    points: &[Point],
    color: Option<Color>,
    fill: Option<Color>,
) -> Result<Annot<'a>> {
    add_poly(doc, page, points, color, fill, AnnotType::Polygon)
}

/// `page.add_polyline_annot` — an open `/PolyLine` through `points`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_polyline_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    points: &[Point],
    color: Option<Color>,
) -> Result<Annot<'a>> {
    add_poly(doc, page, points, color, None, AnnotType::PolyLine)
}

fn add_poly<'a>(
    doc: &'a DocumentStore,
    page: usize,
    points: &[Point],
    color: Option<Color>,
    fill: Option<Color>,
    ty: AnnotType,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let us: Vec<Point> = points.iter().map(|p| pc.to_user_space(*p)).collect();
    let rect = bounding_rect(&us);
    let mut verts: Vec<Object> = Vec::with_capacity(us.len() * 2);
    for p in &us {
        verts.push(Object::Real(p.x));
        verts.push(Object::Real(p.y));
    }
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&rect));
    d.insert(Name::new("Vertices"), Object::Array(verts));
    d.insert(Name::new("C"), color_array(color.unwrap_or(Color::BLACK)));
    if let Some(f) = fill {
        d.insert(Name::new("IC"), color_array(f));
    }
    set_default_border(&mut d, 1.0);
    finalize_annot(doc, page, d, ty)
}

/// `page.add_ink_annot` — an `/Ink` annotation of free-form `strokes` (each a
/// polyline in top-left page space).
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_ink_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    strokes: &[Vec<Point>],
    color: Option<Color>,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let mut all_user: Vec<Point> = Vec::new();
    let mut ink_list: Vec<Object> = Vec::with_capacity(strokes.len());
    for stroke in strokes {
        let mut path: Vec<Object> = Vec::with_capacity(stroke.len() * 2);
        for p in stroke {
            let u = pc.to_user_space(*p);
            all_user.push(u);
            path.push(Object::Real(u.x));
            path.push(Object::Real(u.y));
        }
        ink_list.push(Object::Array(path));
    }
    let rect = bounding_rect(&all_user);
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&rect));
    d.insert(Name::new("InkList"), Object::Array(ink_list));
    d.insert(Name::new("C"), color_array(color.unwrap_or(Color::BLACK)));
    set_default_border(&mut d, 1.0);
    finalize_annot(doc, page, d, AnnotType::Ink)
}

/// `page.add_stamp_annot` — a `/Stamp` annotation fitting `rect` with a label.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_stamp_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    rect: Rect,
    stamp: &str,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let ur = pc.rect_to_user_space(rect);
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&ur));
    let label = if stamp.is_empty() { "Draft" } else { stamp };
    d.insert(Name::new("Name"), Object::Name(Name::new(label)));
    d.insert(Name::new("Contents"), text_string(label));
    d.insert(Name::new("C"), color_array(Color::new(1.0, 0.0, 0.0)));
    set_default_border(&mut d, 1.0);
    finalize_annot(doc, page, d, AnnotType::Stamp)
}

/// `page.add_file_annot` — a `/FileAttachment` at `point` embedding `bytes` as a
/// file named `filename`.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_file_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    point: Point,
    bytes: &[u8],
    filename: &str,
    desc: Option<&str>,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let p = pc.to_user_space(point);
    let rect = Rect::new(p.x, p.y - 20.0, p.x + 18.0, p.y);

    // The embedded-file stream (/Type /EmbeddedFile) with a /Params /Size.
    let mut ef_dict = Dict::new();
    ef_dict.insert(Name::new("Type"), Object::Name(Name::new("EmbeddedFile")));
    ef_dict.insert(Name::new("Length"), Object::Integer(bytes.len() as i64));
    let mut params = Dict::new();
    params.insert(Name::new("Size"), Object::Integer(bytes.len() as i64));
    ef_dict.insert(Name::new("Params"), Object::Dictionary(params));
    let ef_ref = doc.add_object(Object::Stream(StreamObj::new_encoded(
        ef_dict,
        bytes.to_vec(),
    )))?;

    // The file specification dict (/Type /Filespec) pointing at the EF stream.
    let mut fs = Dict::new();
    fs.insert(Name::new("Type"), Object::Name(Name::new("Filespec")));
    fs.insert(Name::new("F"), text_string(filename));
    fs.insert(Name::new("UF"), text_string(filename));
    // Persist /Desc; file_info defaults the description to the filename when
    // /Desc is absent (matching fitz), so we only write it when given.
    if let Some(de) = desc {
        fs.insert(Name::new("Desc"), text_string(de));
    }
    let mut ef = Dict::new();
    ef.insert(Name::new("F"), Object::Reference(ef_ref));
    ef.insert(Name::new("UF"), Object::Reference(ef_ref));
    fs.insert(Name::new("EF"), Object::Dictionary(ef));
    let fs_ref = doc.add_object(Object::Dictionary(fs))?;

    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&rect.normalize()));
    d.insert(Name::new("FS"), Object::Reference(fs_ref));
    d.insert(Name::new("Name"), Object::Name(Name::new("PushPin")));
    d.insert(Name::new("Contents"), text_string(filename));
    d.insert(Name::new("C"), color_array(Color::new(1.0, 1.0, 0.0)));
    finalize_annot(doc, page, d, AnnotType::FileAttachment)
}

/// `page.add_redact_annot` — a `/Redact` annotation over `rect` (creates the
/// marker only; applying is M4d). Optional fill color and overlay text.
///
/// # Errors
/// Propagates page-resolve / object-edit errors.
pub fn add_redact_annot<'a>(
    doc: &'a DocumentStore,
    page: usize,
    rect: Rect,
    fill: Option<Color>,
    text: Option<&str>,
) -> Result<Annot<'a>> {
    let pc = PageContent::new(doc, page)?;
    let ur = pc.rect_to_user_space(rect);
    let mut d = Dict::new();
    d.insert(Name::new("Rect"), rect_array(&ur));
    // QuadPoints covering the rect (Acrobat order ul,ur,ll,lr).
    d.insert(
        Name::new("QuadPoints"),
        Object::Array(
            [ur.x0, ur.y1, ur.x1, ur.y1, ur.x0, ur.y0, ur.x1, ur.y0]
                .into_iter()
                .map(Object::Real)
                .collect(),
        ),
    );
    let fill = fill.unwrap_or(Color::BLACK);
    d.insert(Name::new("IC"), color_array(fill));
    if let Some(t) = text {
        d.insert(Name::new("OverlayText"), text_string(t));
        d.insert(Name::new("DA"), text_string("0 0 0 rg /Helv 11 Tf"));
    }
    finalize_annot(doc, page, d, AnnotType::Redact)
}

// === CRUD over a page's /Annots ===========================================

/// The annotation references on the page at `index`, in `/Annots` order
/// (all subtypes).
#[must_use]
pub fn annot_refs(doc: &DocumentStore, index: usize) -> Vec<ObjRef> {
    let Some(leaf) = pagetree::page_refs(doc).get(index).copied() else {
        return Vec::new();
    };
    annot_refs_on_leaf(doc, leaf)
}

fn annot_refs_on_leaf(doc: &DocumentStore, leaf: ObjRef) -> Vec<ObjRef> {
    let Ok(pd) = doc.resolve(leaf) else {
        return Vec::new();
    };
    let Some(pd) = pd.as_dict() else {
        return Vec::new();
    };
    let arr = match pd.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    // fitz filters `/Subtype /Popup` out of annotation iteration (popups are
    // reached via the parent annot's `/Popup` ref, not the annot chain).
    arr.iter()
        .filter_map(Object::as_reference)
        .filter(|r| !is_popup(doc, *r))
        .collect()
}

/// Whether the object at `r` is a `/Subtype /Popup` annotation.
fn is_popup(doc: &DocumentStore, r: ObjRef) -> bool {
    doc.resolve(r)
        .ok()
        .and_then(|o| o.as_dict().cloned())
        .and_then(|d| {
            d.get(&Name::new("Subtype"))
                .and_then(Object::as_name)
                .map(|n| n.as_bytes() == b"Popup")
        })
        .unwrap_or(false)
}

/// Annotation handles on the page at `index`, in `/Annots` order.
#[must_use]
pub fn annots(doc: &DocumentStore, index: usize) -> Vec<Annot<'_>> {
    let Some(leaf) = pagetree::page_refs(doc).get(index).copied() else {
        return Vec::new();
    };
    annot_refs_on_leaf(doc, leaf)
        .into_iter()
        .map(|obj| Annot { doc, leaf, obj })
        .collect()
}

/// The first annotation on the page at `index`, if any.
#[must_use]
pub fn first_annot(doc: &DocumentStore, index: usize) -> Option<Annot<'_>> {
    annots(doc, index).into_iter().next()
}

/// The number of annotations on the page at `index`.
#[must_use]
pub fn annot_count(doc: &DocumentStore, index: usize) -> usize {
    annot_refs(doc, index).len()
}

/// The `/NM` names of the annotations on the page at `index` (empty string for
/// annotations with no `/NM`).
#[must_use]
pub fn annot_names(doc: &DocumentStore, index: usize) -> Vec<String> {
    annots(doc, index).iter().map(Annot::name).collect()
}

/// One `(xref, subtype-name, /NM-id)` entry for every object in the page's
/// `/Annots` array (PyMuPDF `Document.page_annot_xrefs`). Unlike
/// [`annot_refs`]/[`annots`], this **includes** `/Subtype /Popup` annotations —
/// fitz's `page_annot_xrefs` dumps the raw `/Annots` array, whereas annotation
/// *iteration* skips popups. `subtype` is the bare `/Subtype` name (e.g.
/// `"Highlight"`, `"Widget"`), `""` if absent; `nm` is the `/NM` string or `""`.
#[must_use]
pub fn annot_entries(doc: &DocumentStore, index: usize) -> Vec<(u32, String, String)> {
    let Some(leaf) = pagetree::page_refs(doc).get(index).copied() else {
        return Vec::new();
    };
    let Ok(pd) = doc.resolve(leaf) else {
        return Vec::new();
    };
    let Some(pd) = pd.as_dict() else {
        return Vec::new();
    };
    let arr = match pd.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)
            .ok()
            .and_then(|o| o.as_array().map(<[Object]>::to_vec))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    arr.iter()
        .filter_map(Object::as_reference)
        .map(|r| {
            let d = doc.resolve(r).ok().and_then(|o| o.as_dict().cloned());
            let subtype = d
                .as_ref()
                .and_then(|d| d.get(&Name::new("Subtype")).and_then(Object::as_name))
                .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
                .unwrap_or_default();
            let nm = d
                .as_ref()
                .and_then(|d| d.get(&Name::new("NM")).and_then(Object::as_string))
                .map(decode_text_string)
                .unwrap_or_default();
            (r.num, subtype, nm)
        })
        .collect()
}

/// Deletes an annotation from the page at `index`: removes it from `/Annots`,
/// frees the object, and frees its `/AP /N` stream and any embedded-file objects.
/// No-op if the annotation is not on the page.
///
/// # Errors
/// [`Error::InvalidArgument`] for an out-of-range page; propagates object-edit
/// errors.
pub fn delete_annot(doc: &DocumentStore, index: usize, annot: ObjRef) -> Result<()> {
    let leaf = *pagetree::page_refs(doc)
        .get(index)
        .ok_or(Error::InvalidArgument("page index out of range"))?;
    delete_annot_on_leaf(doc, leaf, annot)
}

/// Deletes an annotation referenced from a page leaf directly (used when the
/// page index is not at hand, e.g. response/popup cleanup). Mirrors
/// [`delete_annot`].
fn delete_annot_on_leaf(doc: &DocumentStore, leaf: ObjRef, annot: ObjRef) -> Result<()> {
    let mut pd = doc
        .resolve(leaf)?
        .as_dict()
        .cloned()
        .ok_or(Error::InvalidArgument("page is not a dictionary"))?;
    let arr = match pd.get(&Name::new("Annots")) {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => doc
            .resolve(*r)?
            .as_array()
            .map(<[Object]>::to_vec)
            .unwrap_or_default(),
        _ => return Ok(()),
    };
    if !arr
        .iter()
        .any(|o| matches!(o, Object::Reference(r) if r.num == annot.num))
    {
        return Ok(());
    }
    // Free the AP /N stream and embedded-file chain before dropping the annot.
    free_annot_children(doc, annot);
    let filtered: Vec<Object> = arr
        .into_iter()
        .filter(|o| !matches!(o, Object::Reference(r) if r.num == annot.num))
        .collect();
    pd.insert(Name::new("Annots"), Object::Array(filtered));
    doc.update_object(leaf, Object::Dictionary(pd))?;
    doc.delete_object(annot)
}

/// Frees an annotation's owned child objects: the `/AP /N` appearance stream and
/// (for FileAttachment) the `/FS` filespec + `/EF` embedded-file stream. Errors
/// are ignored (best-effort cleanup; the annot dict is removed regardless).
fn free_annot_children(doc: &DocumentStore, annot: ObjRef) {
    let Ok(obj) = doc.resolve(annot) else {
        return;
    };
    let Some(d) = obj.as_dict().cloned() else {
        return;
    };
    // /AP /N stream.
    if let Some(ap) = d.get(&Name::new("AP")) {
        let ap = match ap {
            Object::Dictionary(ap) => Some(ap.clone()),
            Object::Reference(r) => doc.resolve(*r).ok().and_then(|o| o.as_dict().cloned()),
            _ => None,
        };
        if let Some(ap) = ap {
            if let Some(n) = ap.get(&Name::new("N")).and_then(Object::as_reference) {
                let _ = doc.delete_object(n);
            }
        }
    }
    // /FS filespec → /EF embedded file.
    if let Some(fs_ref) = d.get(&Name::new("FS")).and_then(Object::as_reference) {
        if let Ok(fs) = doc.resolve(fs_ref) {
            if let Some(fs) = fs.as_dict() {
                if let Some(ef) = fs.get(&Name::new("EF")).and_then(Object::as_dict) {
                    for v in ef.values() {
                        if let Some(r) = v.as_reference() {
                            let _ = doc.delete_object(r);
                        }
                    }
                }
            }
        }
        let _ = doc.delete_object(fs_ref);
    }
}

// === /AP /N appearance-stream generation ==================================

/// Builds the `/AP /N` Form XObject for an annotation of subtype `ty` with dict
/// `d`. The content draws the annotation inside its `/Rect`; `/BBox` is the rect
/// and `/Matrix` identity, so the appearance maps 1:1 into page space.
fn build_appearance(
    doc: &DocumentStore,
    _pc: &PageContent,
    ty: AnnotType,
    d: &Dict,
) -> Result<StreamObj> {
    let rect = read_rect(d);
    let stroke = read_color(d, "C");
    let fill = read_color(d, "IC");
    let opacity = d
        .get(&Name::new("CA"))
        .and_then(Object::as_f64)
        .unwrap_or(1.0);
    let bw = read_border_width(d);

    let mut body = Vec::new();
    let mut resources = Dict::new();

    match ty {
        AnnotType::Highlight => {
            // Filled quads (yellow default) under a Multiply blend.
            let color = stroke.unwrap_or(Color::new(1.0, 1.0, 0.0));
            let gs = add_multiply_gs(&mut resources);
            body.extend_from_slice(format!("/{gs} gs\n").as_bytes());
            body.extend_from_slice(format!("{}\n", color.fill_op()).as_bytes());
            for q in read_quads(d) {
                emit_quad_fill(&mut body, &q);
            }
        }
        AnnotType::Underline | AnnotType::StrikeOut | AnnotType::Squiggly => {
            let color = stroke.unwrap_or(Color::BLACK);
            body.extend_from_slice(format!("{} w\n", fmt_num(bw.max(1.0))).as_bytes());
            body.extend_from_slice(format!("{}\n", color.stroke_op()).as_bytes());
            for q in read_quads(d) {
                emit_markup_line(&mut body, &q, ty);
            }
        }
        AnnotType::Square => {
            emit_box(&mut body, rect, bw, stroke, fill, false);
        }
        AnnotType::Circle => {
            emit_box(&mut body, rect, bw, stroke, fill, true);
        }
        AnnotType::Line => {
            let color = stroke.unwrap_or(Color::BLACK);
            if let Some(l) = read_line(d) {
                body.extend_from_slice(format!("{} w\n", fmt_num(bw.max(1.0))).as_bytes());
                body.extend_from_slice(format!("{}\n", color.stroke_op()).as_bytes());
                body.extend_from_slice(
                    format!("{} {} m\n", fmt_num(l.0.x), fmt_num(l.0.y)).as_bytes(),
                );
                body.extend_from_slice(
                    format!("{} {} l\nS\n", fmt_num(l.1.x), fmt_num(l.1.y)).as_bytes(),
                );
            }
        }
        AnnotType::Polygon | AnnotType::PolyLine => {
            let color = stroke.unwrap_or(Color::BLACK);
            let pts = read_points(d, "Vertices");
            emit_poly(&mut body, &pts, bw, color, fill, ty == AnnotType::Polygon);
        }
        AnnotType::Ink => {
            let color = stroke.unwrap_or(Color::BLACK);
            body.extend_from_slice(format!("{} w\n", fmt_num(bw.max(1.0))).as_bytes());
            body.extend_from_slice(format!("{}\n", color.stroke_op()).as_bytes());
            body.extend_from_slice(b"1 J\n1 j\n"); // round caps/joins
            for stroke_pts in read_ink_list(d) {
                emit_polyline_path(&mut body, &stroke_pts);
                body.extend_from_slice(b"S\n");
            }
        }
        AnnotType::FreeText => {
            let da_color = stroke.unwrap_or(Color::BLACK);
            let fontsize = read_da_fontsize(d).unwrap_or(11.0);
            let text = read_text_string(d, "Contents");
            emit_freetext(
                &mut body,
                &mut resources,
                rect,
                bw,
                da_color,
                fill,
                &text,
                fontsize,
            );
        }
        AnnotType::Text => {
            emit_note_icon(&mut body, rect, stroke.unwrap_or(Color::new(1.0, 1.0, 0.0)));
        }
        AnnotType::Stamp => {
            let label = read_text_string(d, "Contents");
            let color = stroke.unwrap_or(Color::new(1.0, 0.0, 0.0));
            emit_stamp(&mut body, &mut resources, rect, color, &label);
        }
        AnnotType::Redact => {
            // A Redact marker shows a thin outline of the redaction rect.
            let color = Color::BLACK;
            emit_box(&mut body, rect, 1.0, Some(color), None, false);
        }
        AnnotType::FileAttachment => {
            emit_note_icon(&mut body, rect, stroke.unwrap_or(Color::new(1.0, 1.0, 0.0)));
        }
        AnnotType::Link | AnnotType::Widget | AnnotType::Other => {
            // No appearance (Link/Widget appearances are authored elsewhere).
        }
    }

    // A non-empty fallback so `/AP /N` is never an empty stream (the M4 exit
    // requires a non-empty appearance for every authored subtype).
    if body.is_empty() {
        body.extend_from_slice(b"q\nQ\n");
    }

    let mut dict = Dict::new();
    dict.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    dict.insert(Name::new("Subtype"), Object::Name(Name::new("Form")));
    dict.insert(Name::new("FormType"), Object::Integer(1));
    dict.insert(Name::new("BBox"), rect_array(&rect));
    dict.insert(
        Name::new("Matrix"),
        Object::Array(vec![
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    dict.insert(Name::new("Resources"), Object::Dictionary(resources));
    // `/CA` opacity is carried on the annotation dict and applied by the viewer;
    // for Highlight we additionally fold it into the Multiply ExtGState below.
    let _ = (doc, opacity);
    dict.insert(Name::new("Length"), Object::Integer(body.len() as i64));
    Ok(StreamObj::new_encoded(dict, body))
}

/// Adds a `/ExtGState` with `/BM /Multiply` (+ optional CA/ca) to `resources`,
/// returning its name.
fn add_multiply_gs(resources: &mut Dict) -> String {
    let mut gs = Dict::new();
    gs.insert(Name::new("Type"), Object::Name(Name::new("ExtGState")));
    gs.insert(Name::new("BM"), Object::Name(Name::new("Multiply")));
    let mut extg = match resources.get(&Name::new("ExtGState")) {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dict::new(),
    };
    let name = "GSM";
    extg.insert(Name::new(name), Object::Dictionary(gs));
    resources.insert(Name::new("ExtGState"), Object::Dictionary(extg));
    name.to_string()
}

/// Emits a filled quadrilateral (`m l l l h f`) in user space.
fn emit_quad_fill(body: &mut Vec<u8>, q: &Quad) {
    body.extend_from_slice(
        format!(
            "{} {} m\n{} {} l\n{} {} l\n{} {} l\nh\nf\n",
            fmt_num(q.ul.x),
            fmt_num(q.ul.y),
            fmt_num(q.ur.x),
            fmt_num(q.ur.y),
            fmt_num(q.lr.x),
            fmt_num(q.lr.y),
            fmt_num(q.ll.x),
            fmt_num(q.ll.y),
        )
        .as_bytes(),
    );
}

/// Emits the line(s) for an Underline / StrikeOut / Squiggly across a quad.
fn emit_markup_line(body: &mut Vec<u8>, q: &Quad, ty: AnnotType) {
    // Quad corners (user space): ul/ur top, ll/lr bottom.
    let top_y = q.ul.y.max(q.ur.y);
    let bot_y = q.ll.y.min(q.lr.y);
    let x0 = q.ul.x.min(q.ll.x);
    let x1 = q.ur.x.max(q.lr.x);
    let h = (top_y - bot_y).abs();
    let y = match ty {
        // Underline near the bottom (~1/12 above baseline).
        AnnotType::Underline => bot_y + h * 0.08,
        // StrikeOut through the middle.
        AnnotType::StrikeOut => bot_y + h * 0.5,
        // Squiggly near the bottom (drawn as a zig-zag).
        AnnotType::Squiggly => bot_y + h * 0.08,
        _ => bot_y + h * 0.08,
    };
    if ty == AnnotType::Squiggly {
        // A zig-zag polyline between x0 and x1.
        let amp = (h * 0.06).max(1.0);
        let step = (amp * 2.0).max(2.0);
        body.extend_from_slice(format!("{} {} m\n", fmt_num(x0), fmt_num(y)).as_bytes());
        let mut x = x0;
        let mut up = true;
        while x < x1 {
            x = (x + step).min(x1);
            let yy = if up { y + amp } else { y };
            body.extend_from_slice(format!("{} {} l\n", fmt_num(x), fmt_num(yy)).as_bytes());
            up = !up;
        }
        body.extend_from_slice(b"S\n");
    } else {
        body.extend_from_slice(
            format!(
                "{} {} m\n{} {} l\nS\n",
                fmt_num(x0),
                fmt_num(y),
                fmt_num(x1),
                fmt_num(y)
            )
            .as_bytes(),
        );
    }
}

/// Emits a Square / Circle: stroked (+ optional filled) rect / ellipse inset by
/// the border width.
fn emit_box(
    body: &mut Vec<u8>,
    rect: Rect,
    bw: f64,
    stroke: Option<Color>,
    fill: Option<Color>,
    ellipse: bool,
) {
    let inset = (bw / 2.0).max(0.0);
    let r = Rect::new(
        rect.x0 + inset,
        rect.y0 + inset,
        rect.x1 - inset,
        rect.y1 - inset,
    );
    body.extend_from_slice(format!("{} w\n", fmt_num(bw.max(0.5))).as_bytes());
    if let Some(s) = stroke {
        body.extend_from_slice(format!("{}\n", s.stroke_op()).as_bytes());
    }
    if let Some(f) = fill {
        body.extend_from_slice(format!("{}\n", f.fill_op()).as_bytes());
    }
    if ellipse {
        emit_ellipse_path(body, r);
    } else {
        body.extend_from_slice(
            format!(
                "{} {} {} {} re\n",
                fmt_num(r.x0),
                fmt_num(r.y0),
                fmt_num(r.width()),
                fmt_num(r.height())
            )
            .as_bytes(),
        );
    }
    let paint = match (stroke.is_some(), fill.is_some()) {
        (true, true) => "B",
        (false, true) => "f",
        (true, false) => "S",
        (false, false) => "S",
    };
    body.extend_from_slice(format!("{paint}\n").as_bytes());
}

const KAPPA: f64 = 0.552_284_749_830_793_4;

/// Emits a 4-Bézier ellipse path fitting `r` (no paint operator).
fn emit_ellipse_path(body: &mut Vec<u8>, r: Rect) {
    let cx = (r.x0 + r.x1) / 2.0;
    let cy = (r.y0 + r.y1) / 2.0;
    let rx = r.width() / 2.0;
    let ry = r.height() / 2.0;
    let ox = rx * KAPPA;
    let oy = ry * KAPPA;
    body.extend_from_slice(format!("{} {} m\n", fmt_num(cx + rx), fmt_num(cy)).as_bytes());
    body.extend_from_slice(
        format!(
            "{} {} {} {} {} {} c\n",
            fmt_num(cx + rx),
            fmt_num(cy + oy),
            fmt_num(cx + ox),
            fmt_num(cy + ry),
            fmt_num(cx),
            fmt_num(cy + ry)
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "{} {} {} {} {} {} c\n",
            fmt_num(cx - ox),
            fmt_num(cy + ry),
            fmt_num(cx - rx),
            fmt_num(cy + oy),
            fmt_num(cx - rx),
            fmt_num(cy)
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "{} {} {} {} {} {} c\n",
            fmt_num(cx - rx),
            fmt_num(cy - oy),
            fmt_num(cx - ox),
            fmt_num(cy - ry),
            fmt_num(cx),
            fmt_num(cy - ry)
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "{} {} {} {} {} {} c\n",
            fmt_num(cx + ox),
            fmt_num(cy - ry),
            fmt_num(cx + rx),
            fmt_num(cy - oy),
            fmt_num(cx + rx),
            fmt_num(cy)
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"h\n");
}

/// Emits a polygon / polyline path + paint.
fn emit_poly(
    body: &mut Vec<u8>,
    pts: &[Point],
    bw: f64,
    color: Color,
    fill: Option<Color>,
    closed: bool,
) {
    if pts.is_empty() {
        return;
    }
    body.extend_from_slice(format!("{} w\n", fmt_num(bw.max(1.0))).as_bytes());
    body.extend_from_slice(format!("{}\n", color.stroke_op()).as_bytes());
    if let Some(f) = fill {
        body.extend_from_slice(format!("{}\n", f.fill_op()).as_bytes());
    }
    emit_polyline_path(body, pts);
    if closed {
        body.extend_from_slice(b"h\n");
        let paint = if fill.is_some() { "B" } else { "S" };
        body.extend_from_slice(format!("{paint}\n").as_bytes());
    } else {
        body.extend_from_slice(b"S\n");
    }
}

/// Emits an `m … l …` path (no paint operator) from user-space points.
fn emit_polyline_path(body: &mut Vec<u8>, pts: &[Point]) {
    if let Some((first, rest)) = pts.split_first() {
        body.extend_from_slice(format!("{} {} m\n", fmt_num(first.x), fmt_num(first.y)).as_bytes());
        for p in rest {
            body.extend_from_slice(format!("{} {} l\n", fmt_num(p.x), fmt_num(p.y)).as_bytes());
        }
    }
}

/// Emits a FreeText appearance: a bordered (+ optionally filled) box plus the
/// text rendered with a Base-14 Helvetica.
#[allow(clippy::too_many_arguments)]
fn emit_freetext(
    body: &mut Vec<u8>,
    resources: &mut Dict,
    rect: Rect,
    bw: f64,
    color: Color,
    fill: Option<Color>,
    text: &str,
    fontsize: f64,
) {
    // Box.
    if let Some(f) = fill {
        body.extend_from_slice(format!("{}\n", f.fill_op()).as_bytes());
        body.extend_from_slice(
            format!(
                "{} {} {} {} re\nf\n",
                fmt_num(rect.x0),
                fmt_num(rect.y0),
                fmt_num(rect.width()),
                fmt_num(rect.height())
            )
            .as_bytes(),
        );
    }
    if bw > 0.0 {
        let inset = bw / 2.0;
        body.extend_from_slice(format!("{} w\n0 0 0 RG\n", fmt_num(bw)).as_bytes());
        body.extend_from_slice(
            format!(
                "{} {} {} {} re\nS\n",
                fmt_num(rect.x0 + inset),
                fmt_num(rect.y0 + inset),
                fmt_num(rect.width() - bw),
                fmt_num(rect.height() - bw)
            )
            .as_bytes(),
        );
    }
    // Text (top-left, one line per `\n`).
    register_helv(resources);
    body.extend_from_slice(b"BT\n");
    body.extend_from_slice(format!("/Helv {} Tf\n", fmt_num(fontsize)).as_bytes());
    body.extend_from_slice(format!("{}\n", color.fill_op()).as_bytes());
    let leading = fontsize * 1.2;
    let mut y = rect.y1 - bw - fontsize;
    let pad = bw + 2.0;
    for line in text.split('\n') {
        body.extend_from_slice(
            format!("1 0 0 1 {} {} Tm\n", fmt_num(rect.x0 + pad), fmt_num(y)).as_bytes(),
        );
        body.extend_from_slice(b"(");
        body.extend_from_slice(&escape_pdf_literal(line.as_bytes()));
        body.extend_from_slice(b") Tj\n");
        y -= leading;
    }
    body.extend_from_slice(b"ET\n");
}

/// Emits a sticky-note icon: a small rounded box with a "fold" triangle.
fn emit_note_icon(body: &mut Vec<u8>, rect: Rect, color: Color) {
    body.extend_from_slice(format!("{}\n", color.fill_op()).as_bytes());
    body.extend_from_slice(b"0 0 0 RG\n0.6 w\n");
    body.extend_from_slice(
        format!(
            "{} {} {} {} re\nB\n",
            fmt_num(rect.x0),
            fmt_num(rect.y0),
            fmt_num(rect.width()),
            fmt_num(rect.height())
        )
        .as_bytes(),
    );
    // Three horizontal "text" lines inside.
    let h = rect.height();
    for i in 1..=3 {
        let y = rect.y0 + h * (i as f64) / 4.0;
        body.extend_from_slice(
            format!(
                "{} {} m\n{} {} l\nS\n",
                fmt_num(rect.x0 + 3.0),
                fmt_num(y),
                fmt_num(rect.x1 - 3.0),
                fmt_num(y)
            )
            .as_bytes(),
        );
    }
}

/// Emits a Stamp appearance: a bordered label box with the stamp text.
fn emit_stamp(body: &mut Vec<u8>, resources: &mut Dict, rect: Rect, color: Color, label: &str) {
    body.extend_from_slice(format!("2 w\n{}\n", color.stroke_op()).as_bytes());
    body.extend_from_slice(
        format!(
            "{} {} {} {} re\nS\n",
            fmt_num(rect.x0 + 1.0),
            fmt_num(rect.y0 + 1.0),
            fmt_num(rect.width() - 2.0),
            fmt_num(rect.height() - 2.0)
        )
        .as_bytes(),
    );
    register_helv(resources);
    let fontsize = (rect.height() * 0.5).clamp(6.0, 24.0);
    body.extend_from_slice(b"BT\n");
    body.extend_from_slice(format!("/Helv {} Tf\n", fmt_num(fontsize)).as_bytes());
    body.extend_from_slice(format!("{}\n", color.fill_op()).as_bytes());
    let y = rect.y0 + (rect.height() - fontsize) / 2.0;
    body.extend_from_slice(
        format!("1 0 0 1 {} {} Tm\n", fmt_num(rect.x0 + 4.0), fmt_num(y)).as_bytes(),
    );
    body.extend_from_slice(b"(");
    body.extend_from_slice(&escape_pdf_literal(label.as_bytes()));
    body.extend_from_slice(b") Tj\nET\n");
}

/// Registers a Base-14 Helvetica under `/Resources /Font /Helv`.
fn register_helv(resources: &mut Dict) {
    let mut font = Dict::new();
    font.insert(Name::new("Type"), Object::Name(Name::new("Font")));
    font.insert(Name::new("Subtype"), Object::Name(Name::new("Type1")));
    font.insert(Name::new("BaseFont"), Object::Name(Name::new("Helvetica")));
    font.insert(
        Name::new("Encoding"),
        Object::Name(Name::new("WinAnsiEncoding")),
    );
    let mut fonts = match resources.get(&Name::new("Font")) {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dict::new(),
    };
    fonts.insert(Name::new("Helv"), Object::Dictionary(font));
    resources.insert(Name::new("Font"), Object::Dictionary(fonts));
}

// === dict read/write helpers ==============================================

/// Maps a `/LE` line-ending style name to its PyMuPDF `PDF_ANNOT_LE_*` code.
fn le_code(name: &[u8]) -> i64 {
    match name {
        b"None" => 0,
        b"Square" => 1,
        b"Circle" => 2,
        b"Diamond" => 3,
        b"OpenArrow" => 4,
        b"ClosedArrow" => 5,
        b"Butt" => 6,
        b"ROpenArrow" => 7,
        b"RClosedArrow" => 8,
        b"Slash" => 9,
        _ => 0,
    }
}

/// Maps a PyMuPDF `PDF_ANNOT_LE_*` code back to its `/LE` style name.
fn le_name(code: i64) -> &'static str {
    match code {
        1 => "Square",
        2 => "Circle",
        3 => "Diamond",
        4 => "OpenArrow",
        5 => "ClosedArrow",
        6 => "Butt",
        7 => "ROpenArrow",
        8 => "RClosedArrow",
        9 => "Slash",
        _ => "None",
    }
}

fn rect_array(r: &Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.x0),
        Object::Real(r.y0),
        Object::Real(r.x1),
        Object::Real(r.y1),
    ])
}

/// Trims leading/trailing ASCII whitespace from a byte slice.
fn trim_ascii_ws(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(0);
    let end = b
        .iter()
        .rposition(|c| !c.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &b[start..end.max(start)]
}

fn read_rect(d: &Dict) -> Rect {
    match d.get(&Name::new("Rect")).and_then(Object::as_array) {
        Some(a) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
            Rect::new(v[0], v[1], v[2], v[3]).normalize()
        }
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

fn color_array(c: Color) -> Object {
    Object::Array(vec![
        Object::Real(c.r.clamp(0.0, 1.0)),
        Object::Real(c.g.clamp(0.0, 1.0)),
        Object::Real(c.b.clamp(0.0, 1.0)),
    ])
}

fn read_color(d: &Dict, key: &str) -> Option<Color> {
    let a = d.get(&Name::new(key)).and_then(Object::as_array)?;
    match a.len() {
        3 => Some(Color::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?)),
        1 => {
            let g = a[0].as_f64()?;
            Some(Color::new(g, g, g))
        }
        4 => {
            // CMYK → RGB.
            let (c, m, y, k) = (
                a[0].as_f64()?,
                a[1].as_f64()?,
                a[2].as_f64()?,
                a[3].as_f64()?,
            );
            Some(Color::new(
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
            ))
        }
        _ => None,
    }
}

fn read_border_width(d: &Dict) -> f64 {
    if let Some(bs) = d.get(&Name::new("BS")).and_then(Object::as_dict) {
        if let Some(w) = bs.get(&Name::new("W")).and_then(Object::as_f64) {
            return w;
        }
    }
    if let Some(b) = d.get(&Name::new("Border")).and_then(Object::as_array) {
        if b.len() >= 3 {
            if let Some(w) = b[2].as_f64() {
                return w;
            }
        }
    }
    1.0
}

fn set_default_border(d: &mut Dict, width: f64) {
    let mut bs = Dict::new();
    bs.insert(Name::new("Type"), Object::Name(Name::new("Border")));
    bs.insert(Name::new("W"), Object::Real(width));
    bs.insert(Name::new("S"), Object::Name(Name::new("S")));
    d.insert(Name::new("BS"), Object::Dictionary(bs));
    d.insert(
        Name::new("Border"),
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(width),
        ]),
    );
}

fn read_points(d: &Dict, key: &str) -> Vec<Point> {
    let Some(a) = d.get(&Name::new(key)).and_then(Object::as_array) else {
        return Vec::new();
    };
    a.chunks_exact(2)
        .filter_map(|c| Some(Point::new(c[0].as_f64()?, c[1].as_f64()?)))
        .collect()
}

fn read_quads(d: &Dict) -> Vec<Quad> {
    let Some(a) = d.get(&Name::new("QuadPoints")).and_then(Object::as_array) else {
        return Vec::new();
    };
    a.chunks_exact(8)
        .filter_map(|c| {
            let f = |i: usize| c[i].as_f64();
            Some(Quad {
                ul: Point::new(f(0)?, f(1)?),
                ur: Point::new(f(2)?, f(3)?),
                ll: Point::new(f(4)?, f(5)?),
                lr: Point::new(f(6)?, f(7)?),
            })
        })
        .collect()
}

fn read_line(d: &Dict) -> Option<(Point, Point)> {
    let a = d.get(&Name::new("L")).and_then(Object::as_array)?;
    if a.len() == 4 {
        Some((
            Point::new(a[0].as_f64()?, a[1].as_f64()?),
            Point::new(a[2].as_f64()?, a[3].as_f64()?),
        ))
    } else {
        None
    }
}

fn read_ink_list(d: &Dict) -> Vec<Vec<Point>> {
    let Some(a) = d.get(&Name::new("InkList")).and_then(Object::as_array) else {
        return Vec::new();
    };
    a.iter()
        .filter_map(Object::as_array)
        .map(|stroke| {
            stroke
                .chunks_exact(2)
                .filter_map(|c| Some(Point::new(c[0].as_f64()?, c[1].as_f64()?)))
                .collect()
        })
        .collect()
}

fn read_da_fontsize(d: &Dict) -> Option<f64> {
    let da = d.get(&Name::new("DA")).and_then(Object::as_string)?;
    let s = String::from_utf8_lossy(da.as_bytes());
    // Find a "<size> Tf" token.
    let toks: Vec<&str> = s.split_whitespace().collect();
    for w in toks.windows(2) {
        if w[1] == "Tf" {
            if let Ok(v) = w[0].parse::<f64>() {
                if v > 0.0 {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn read_text_string(d: &Dict, key: &str) -> String {
    d.get(&Name::new(key))
        .and_then(Object::as_string)
        .map(decode_text_string)
        .unwrap_or_default()
}

fn bounding_rect(pts: &[Point]) -> Rect {
    if pts.is_empty() {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    let mut r = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in pts {
        r.x0 = r.x0.min(p.x);
        r.y0 = r.y0.min(p.y);
        r.x1 = r.x1.max(p.x);
        r.y1 = r.y1.max(p.y);
    }
    // Pad slightly so a degenerate (single-point / axis-aligned) path has area.
    Rect::new(r.x0 - 1.0, r.y0 - 1.0, r.x1 + 1.0, r.y1 + 1.0)
}

/// Builds a PDF text string object (`/T`, `/Contents`, …). ASCII strings are
/// emitted as PDFDocEncoding literals; non-ASCII uses UTF-16BE with a BOM.
fn text_string(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(PdfString {
            bytes: s.as_bytes().to_vec(),
            kind: StringKind::Literal,
        })
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(PdfString {
            bytes,
            kind: StringKind::Literal,
        })
    }
}

/// Decodes a PDF text string (UTF-16BE with BOM, else PDFDocEncoding/Latin-1).
fn decode_text_string(s: &PdfString) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let units: Vec<u16> = b[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        b.iter().map(|&c| c as char).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::Limits;

    /// A minimal one-page PDF: catalog, single-page tree, one blank page leaf.
    fn one_page_pdf() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::new();

        offsets.push((1u32, out.len()));
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        offsets.push((2u32, out.len()));
        out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        offsets.push((3u32, out.len()));
        out.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
        );

        let startxref = out.len();
        out.extend_from_slice(b"xref\n0 4\n");
        out.extend_from_slice(b"0000000000 65535 f \n");
        let mut map = std::collections::HashMap::new();
        for (num, off) in &offsets {
            map.insert(*num, *off);
        }
        for num in 1..4u32 {
            out.extend_from_slice(format!("{:010} 00000 n \n", map[&num]).as_bytes());
        }
        out.extend_from_slice(b"trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n");
        out.extend_from_slice(format!("{startxref}\n").as_bytes());
        out.extend_from_slice(b"%%EOF\n");
        out
    }

    /// Builds a doc with a `/Square` annotation and returns its xref.
    fn doc_with_annot() -> DocumentStore {
        DocumentStore::from_bytes(one_page_pdf(), Limits::default()).unwrap()
    }

    fn make_annot(doc: &DocumentStore) -> ObjRef {
        let mut d = Dict::new();
        d.insert(Name::new("Type"), Object::Name(Name::new("Annot")));
        d.insert(Name::new("Subtype"), Object::Name(Name::new("Line")));
        d.insert(
            Name::new("Rect"),
            rect_array(&Rect::new(0.0, 0.0, 100.0, 100.0)),
        );
        doc.add_object(Object::Dictionary(d)).unwrap()
    }

    #[test]
    fn le_code_name_round_trip() {
        let names: [&[u8]; 10] = [
            b"None",
            b"Square",
            b"Circle",
            b"Diamond",
            b"OpenArrow",
            b"ClosedArrow",
            b"Butt",
            b"ROpenArrow",
            b"RClosedArrow",
            b"Slash",
        ];
        for (code, name) in names.iter().enumerate() {
            assert_eq!(le_code(name), code as i64);
            assert_eq!(le_name(code as i64).as_bytes(), *name);
        }
        // Unknown name / code default to 0 / "None".
        assert_eq!(le_code(b"Bogus"), 0);
        assert_eq!(le_name(42), "None");
    }

    #[test]
    fn line_ends_round_trip() {
        let doc = doc_with_annot();
        let leaf = *pagetree::page_refs(&doc).first().unwrap();
        let obj = make_annot(&doc);
        let annot = Annot::from_ref(&doc, leaf, obj);
        assert_eq!(annot.line_ends(), (0, 0));
        annot.set_line_ends(4, 5).unwrap();
        assert_eq!(annot.line_ends(), (4, 5));
        annot.set_line_ends(0, 9).unwrap();
        assert_eq!(annot.line_ends(), (0, 9));
    }

    #[test]
    fn blendmode_round_trip() {
        let doc = doc_with_annot();
        let leaf = *pagetree::page_refs(&doc).first().unwrap();
        let obj = make_annot(&doc);
        let annot = Annot::from_ref(&doc, leaf, obj);
        assert_eq!(annot.blendmode(), None);
        annot.set_blendmode("Multiply").unwrap();
        assert_eq!(annot.blendmode().as_deref(), Some("Multiply"));
    }

    #[test]
    fn open_round_trip() {
        let doc = doc_with_annot();
        let leaf = *pagetree::page_refs(&doc).first().unwrap();
        let obj = make_annot(&doc);
        let annot = Annot::from_ref(&doc, leaf, obj);
        assert!(!annot.is_open());
        annot.set_open(true).unwrap();
        assert!(annot.is_open());
        annot.set_open(false).unwrap();
        assert!(!annot.is_open());
    }

    #[test]
    fn set_name_writes_name_not_nm() {
        let doc = doc_with_annot();
        let leaf = *pagetree::page_refs(&doc).first().unwrap();
        let obj = make_annot(&doc);
        let annot = Annot::from_ref(&doc, leaf, obj);
        annot.set_name("PushPin").unwrap();
        let d = annot.dict().unwrap();
        assert_eq!(
            d.get(&Name::new("Name")).and_then(Object::as_name),
            Some(&Name::new("PushPin"))
        );
        // /NM (the annotation id) is untouched.
        assert!(!d.contains_key(&Name::new("NM")));
    }

    #[test]
    fn border_reports_width_style_dashes() {
        let doc = doc_with_annot();
        let leaf = *pagetree::page_refs(&doc).first().unwrap();
        let obj = make_annot(&doc);
        let annot = Annot::from_ref(&doc, leaf, obj);
        // Default (no /BS): width 1.0, style "S", no dashes.
        let (w, style, dashes) = annot.border();
        assert!((w - 1.0).abs() < 1e-9);
        assert_eq!(style, "S");
        assert!(dashes.is_empty());

        // With a dashed /BS.
        let mut d = annot.dict().unwrap();
        let mut bs = Dict::new();
        bs.insert(Name::new("W"), Object::Real(2.5));
        bs.insert(Name::new("S"), Object::Name(Name::new("D")));
        bs.insert(
            Name::new("D"),
            Object::Array(vec![Object::Integer(3), Object::Real(2.0)]),
        );
        d.insert(Name::new("BS"), Object::Dictionary(bs));
        annot.write_dict(d).unwrap();
        let (w, style, dashes) = annot.border();
        assert!((w - 2.5).abs() < 1e-9);
        assert_eq!(style, "D");
        assert_eq!(dashes, vec![3.0, 2.0]);
    }
}
