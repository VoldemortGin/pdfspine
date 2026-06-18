//! Page inventory (`get_fonts` / `get_images`) and a reusable text-extraction
//! path (`textpage` / `get_text` / `search`) — free functions over [`Page`].
//!
//! These live in `pdf-api` (not `pdf-core`) because [`Page`] is defined in
//! `pdf-core`; the orphan rule forbids inherent `impl Page` from this crate, so
//! the public surface is free functions the PyO3 layer calls directly.
//!
//! `get_fonts` / `get_images` walk the page `/Resources` and return small structs
//! with public fields ordered to match PyMuPDF's tuples (PRD §7). `textpage`
//! builds the [`pdf_text::TextPage`] model once; `get_text` / `search` accept an
//! optional pre-built `&TextPage` so a caller can extract text **and** search
//! without re-running the interpreter (PRD §9.4).

use std::collections::HashSet;

use pdf_core::geom::{Quad, Rect};
use pdf_core::object::{Dict, Name, Object};
use pdf_core::page::Page;
use pdf_core::{DocumentStore, Limits};

use pdf_text::{defaults, TextDict, TextPage};

// === inventory structs (Tier-A tuple shapes, PRD §7) ======================

/// One `page.get_fonts()` entry. Field order matches the PyMuPDF tuple
/// `(xref, ext, type, basefont, name, encoding, referencer)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontInfo {
    /// The font object's xref number, or `0` if the font dict is direct/inline.
    pub xref: i32,
    /// Embedded-font format hint: `"ttf"` (`/FontFile2`), `"cff"`/`"otf"`
    /// (`/FontFile3`, by `/Subtype`), `"pfb"` (`/FontFile`, Type1), else `"n/a"`.
    pub ext: String,
    /// The font `/Subtype` (e.g. `"Type1"`, `"TrueType"`, `"Type0"`).
    pub type_: String,
    /// The `/BaseFont` name, subset tag (`ABCDEF+`) retained.
    pub basefont: String,
    /// The resource name the font is referenced under (e.g. `"F1"`).
    pub name: String,
    /// The `/Encoding`: a name verbatim, or a dict's `/BaseEncoding`, else empty.
    pub encoding: String,
    /// The xref of the object that references the font — the page object number.
    pub referencer: i32,
}

/// One `page.get_images()` entry. Field order matches the PyMuPDF tuple
/// `(xref, smask, width, height, bpc, colorspace, alt_colorspace, name, filter,
/// referencer)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    /// The image XObject's xref number, or `0` if direct/inline.
    pub xref: i32,
    /// The `/SMask` xref if present, else `0`.
    pub smask: i32,
    /// `/Width`.
    pub width: i32,
    /// `/Height`.
    pub height: i32,
    /// `/BitsPerComponent` (`0` if absent).
    pub bpc: i32,
    /// The colorspace name: a `/ColorSpace` name, or the first array element
    /// name (e.g. `"ICCBased"`, `"Indexed"`), else empty. Indirect `/ColorSpace`
    /// references are resolved (PyMuPDF `get_images[5]`).
    pub colorspace: String,
    /// The ICCBased alternate colorspace if trivially available, else empty.
    pub alt_colorspace: String,
    /// The colorspace component count (1 gray, 3 RGB, 4 CMYK; `0` if unknown),
    /// resolved from the colorspace family — ICCBased reads its `/N` (PyMuPDF's
    /// integer `colorspace` in `get_image_info` / `extractIMGINFO`).
    pub cs_components: i32,
    /// A descriptive colorspace name for `extractIMGINFO`'s `cs-name`, e.g.
    /// `"DeviceRGB"` or `"ICCBased(RGB)"`.
    pub cs_name: String,
    /// The resource name the image is referenced under (e.g. `"Im0"`).
    pub name: String,
    /// The `/Filter` name (or the first filter in an array; empty if none).
    pub filter: String,
    /// The xref of the referencing object — the page object number.
    pub referencer: i32,
}

// === get_fonts ============================================================

/// The page's fonts as PyMuPDF-shaped [`FontInfo`] entries, walking
/// `/Resources /Font` (PyMuPDF `page.get_fonts()`).
///
/// Entries are deduped by xref (a font referenced under two names yields one
/// entry; the first wins) and returned sorted by resource name for a
/// deterministic order. A missing / empty `/Font` dict yields an empty `Vec`.
#[must_use]
pub fn get_fonts(page: &Page) -> Vec<FontInfo> {
    let doc = page.document();
    let referencer = page.obj_ref().num as i32;
    let mut out: Vec<FontInfo> = Vec::new();
    let mut seen_xref: HashSet<i32> = HashSet::new();

    let Some(font_dict) = resource_subdict(doc, page, "Font") else {
        return out;
    };

    // Deterministic order: iterate the resource names sorted.
    let mut entries: Vec<(&Name, &Object)> = font_dict.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    for (res_name, value) in entries {
        let xref = value.as_reference().map(|r| r.num as i32).unwrap_or(0);
        if xref != 0 && !seen_xref.insert(xref) {
            continue; // already reported under another name
        }
        let Some(font) = resolve_value(doc, value).and_then(|o| o.as_dict().cloned()) else {
            continue;
        };
        let Some(name) = res_name.as_str() else {
            continue;
        };
        out.push(build_font_info(
            doc,
            &font,
            xref,
            name.to_string(),
            referencer,
        ));
    }
    out
}

/// Assembles one [`FontInfo`] from a resolved font dict.
fn build_font_info(
    doc: &DocumentStore,
    font: &Dict,
    xref: i32,
    name: String,
    referencer: i32,
) -> FontInfo {
    let type_ = dict_name(font, "Subtype").unwrap_or_default();
    let basefont = dict_name(font, "BaseFont").unwrap_or_default();
    let encoding = encoding_name(doc, font);
    let descriptor = font_descriptor(doc, font, &type_);
    let ext = descriptor
        .as_ref()
        .map(|d| font_ext(doc, d))
        .unwrap_or_else(|| "n/a".to_string());

    FontInfo {
        xref,
        ext,
        type_,
        basefont,
        name,
        encoding,
        referencer,
    }
}

/// The `/Encoding` value as a name: a name verbatim, a dict's `/BaseEncoding`,
/// else empty.
fn encoding_name(doc: &DocumentStore, font: &Dict) -> String {
    match resolve_key(doc, font, "Encoding") {
        Some(obj) => match obj.as_ref() {
            Object::Name(n) => n.as_str().unwrap_or_default().to_string(),
            Object::Dictionary(d) => dict_name(d, "BaseEncoding").unwrap_or_default(),
            _ => String::new(),
        },
        None => String::new(),
    }
}

/// The font's `/FontDescriptor`, following a Type0 font's first
/// `/DescendantFonts` entry when the parent lacks one.
fn font_descriptor(doc: &DocumentStore, font: &Dict, subtype: &str) -> Option<Dict> {
    if let Some(d) = resolve_key(doc, font, "FontDescriptor").and_then(|o| o.as_dict().cloned()) {
        return Some(d);
    }
    if subtype == "Type0" {
        let descendant = resolve_key(doc, font, "DescendantFonts")?;
        let first = descendant.as_array()?.first()?;
        let cid = resolve_value(doc, first)?;
        let cid_dict = cid.as_dict()?;
        return resolve_key(doc, cid_dict, "FontDescriptor").and_then(|o| o.as_dict().cloned());
    }
    None
}

/// The embedded-font format hint from a font descriptor's `/FontFile*` keys.
fn font_ext(doc: &DocumentStore, descriptor: &Dict) -> String {
    if descriptor.contains_key(&Name::new("FontFile")) {
        // /FontFile is a Type1 (PFA/PFB) program.
        return "pfb".to_string();
    }
    if descriptor.contains_key(&Name::new("FontFile2")) {
        return "ttf".to_string();
    }
    if descriptor.contains_key(&Name::new("FontFile3")) {
        // /FontFile3 carries CFF / OpenType — distinguish by the stream /Subtype.
        let sub = resolve_key(doc, descriptor, "FontFile3")
            .and_then(|o| o.as_dict().and_then(|d| dict_name(d, "Subtype")));
        return match sub.as_deref() {
            Some("OpenType") => "otf".to_string(),
            _ => "cff".to_string(),
        };
    }
    "n/a".to_string()
}

// === get_images ===========================================================

/// The page's images as PyMuPDF-shaped [`ImageInfo`] entries, walking
/// `/Resources /XObject` and keeping only `/Subtype /Image` (PyMuPDF
/// `page.get_images()`). Form XObjects are skipped.
///
/// Deduped by xref; sorted by resource name for determinism. A missing / empty
/// `/XObject` dict yields an empty `Vec`.
#[must_use]
pub fn get_images(page: &Page) -> Vec<ImageInfo> {
    let doc = page.document();
    let referencer = page.obj_ref().num as i32;
    let mut out: Vec<ImageInfo> = Vec::new();
    let mut seen_xref: HashSet<i32> = HashSet::new();

    let Some(xobj_dict) = resource_subdict(doc, page, "XObject") else {
        return out;
    };

    let mut entries: Vec<(&Name, &Object)> = xobj_dict.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    for (res_name, value) in entries {
        let xref = value.as_reference().map(|r| r.num as i32).unwrap_or(0);
        if xref != 0 && seen_xref.contains(&xref) {
            continue;
        }
        let Some(obj) = resolve_value(doc, value) else {
            continue;
        };
        // Image XObjects are streams; the dict is addressable via `as_dict`.
        let Some(dict) = obj.as_dict() else {
            continue;
        };
        if dict_name(dict, "Subtype").as_deref() != Some("Image") {
            continue; // skip Form / other XObjects
        }
        let Some(name) = res_name.as_str() else {
            continue;
        };
        if xref != 0 {
            seen_xref.insert(xref);
        }
        out.push(build_image_info(
            doc,
            dict,
            xref,
            name.to_string(),
            referencer,
        ));
    }
    out
}

/// Assembles one [`ImageInfo`] from a resolved image XObject dict.
fn build_image_info(
    doc: &DocumentStore,
    dict: &Dict,
    xref: i32,
    name: String,
    referencer: i32,
) -> ImageInfo {
    let width = dict_int(dict, "Width");
    let height = dict_int(dict, "Height");
    let bpc = dict_int(dict, "BitsPerComponent");
    let cs = resolve_colorspace(doc, dict);
    let filter = filter_name(dict);
    let smask = dict
        .get(&Name::new("SMask"))
        .and_then(Object::as_reference)
        .map(|r| r.num as i32)
        .unwrap_or(0);

    ImageInfo {
        xref,
        smask,
        width,
        height,
        bpc,
        colorspace: cs.name,
        alt_colorspace: cs.alt,
        cs_components: cs.components,
        cs_name: cs.cs_name,
        name,
        filter,
        referencer,
    }
}

/// A resolved image colorspace: its PyMuPDF `get_images` name, the ICCBased
/// alternate (when available), the device component count, and a descriptive
/// `extractIMGINFO` `cs-name`.
struct ResolvedColorspace {
    /// The `get_images[5]` name (e.g. `"DeviceRGB"`, `"ICCBased"`, `"Indexed"`).
    name: String,
    /// The ICCBased alternate device-space name (`get_images[6]`), else empty.
    alt: String,
    /// Component count: 1 gray, 3 RGB, 4 CMYK, else `0`.
    components: i32,
    /// A descriptive name for `extractIMGINFO` (e.g. `"DeviceRGB"`,
    /// `"ICCBased(RGB)"`).
    cs_name: String,
}

/// Resolves an image's `/ColorSpace` (following indirect references) to its
/// PyMuPDF name, component count and `cs-name`.
///
/// A bare device name maps directly (`DeviceRGB` → 3, `DeviceGray` → 1,
/// `DeviceCMYK` → 4). An array form `[/ICCBased <stream>]` resolves the stream's
/// `/N` to the component count and an `Alternate` device name; `[/Indexed base …]`
/// and other special families report their family name with the component count
/// derived from the base where trivially available, else `0`.
fn resolve_colorspace(doc: &DocumentStore, dict: &Dict) -> ResolvedColorspace {
    let Some(cs) = resolve_key(doc, dict, "ColorSpace") else {
        return ResolvedColorspace {
            name: String::new(),
            alt: String::new(),
            components: 0,
            cs_name: String::new(),
        };
    };
    colorspace_from_object(doc, cs.as_ref())
}

/// Resolves a (already dereferenced) colorspace object to its parts.
fn colorspace_from_object(doc: &DocumentStore, cs: &Object) -> ResolvedColorspace {
    match cs {
        Object::Name(n) => {
            let name = n.as_str().unwrap_or_default().to_string();
            let components = device_components(&name);
            ResolvedColorspace {
                cs_name: name.clone(),
                name,
                alt: String::new(),
                components,
            }
        }
        Object::Array(a) => {
            let family = a
                .first()
                .and_then(Object::as_name)
                .and_then(Name::as_str)
                .unwrap_or_default()
                .to_string();
            match family.as_str() {
                "ICCBased" => iccbased_colorspace(doc, a, family),
                // Indexed/Separation/DeviceN palettes index into a base space;
                // each placed sample is a single index component.
                "Indexed" | "I" => ResolvedColorspace {
                    name: "Indexed".to_string(),
                    alt: String::new(),
                    components: 1,
                    cs_name: "Indexed".to_string(),
                },
                _ => {
                    let components = device_components(&family);
                    ResolvedColorspace {
                        cs_name: family.clone(),
                        name: family,
                        alt: String::new(),
                        components,
                    }
                }
            }
        }
        _ => ResolvedColorspace {
            name: String::new(),
            alt: String::new(),
            components: 0,
            cs_name: String::new(),
        },
    }
}

/// Resolves an `[/ICCBased <stream>]` colorspace: the stream's `/N` is the
/// component count and `/Alternate` (when present) the device alternate.
fn iccbased_colorspace(doc: &DocumentStore, arr: &[Object], family: String) -> ResolvedColorspace {
    // The ICC profile stream is the second array element.
    let stream_dict = arr
        .get(1)
        .and_then(|o| resolve_value(doc, o))
        .and_then(|o| o.as_dict().cloned());
    let n = stream_dict.as_ref().map(|d| dict_int(d, "N")).unwrap_or(0);
    // `alt_colorspace` (get_images[6]) is the explicit /Alternate only — left
    // empty when absent, matching PyMuPDF (it does NOT infer from /N here).
    let alt = stream_dict
        .as_ref()
        .and_then(|d| dict_name(d, "Alternate"))
        .unwrap_or_default();
    let device = match n {
        1 => "Gray",
        4 => "CMYK",
        3 => "RGB",
        _ => "",
    };
    let cs_name = if device.is_empty() {
        family.clone()
    } else {
        format!("ICCBased({device})")
    };
    ResolvedColorspace {
        name: family,
        alt,
        components: n,
        cs_name,
    }
}

/// The device component count for a device-colorspace name (`0` if not a known
/// device space).
fn device_components(name: &str) -> i32 {
    match name {
        "DeviceRGB" | "RGB" | "CalRGB" | "Lab" => 3,
        "DeviceGray" | "G" | "Gray" | "CalGray" => 1,
        "DeviceCMYK" | "CMYK" => 4,
        _ => 0,
    }
}

/// The `/Filter` name, or the first filter in an array; empty if none.
fn filter_name(dict: &Dict) -> String {
    match dict.get(&Name::new("Filter")) {
        Some(Object::Name(n)) => n.as_str().unwrap_or_default().to_string(),
        Some(Object::Array(a)) => a
            .first()
            .and_then(Object::as_name)
            .and_then(Name::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

// === get_xobjects =========================================================

/// One `page.get_xobjects()` entry (PyMuPDF tuple
/// `(xref, name, "Form"|"Image", bbox, matrix, referencer)`). `bbox` is the
/// XObject's `/BBox` (Form) or the unit square `[0 0 1 1]` (Image, which has no
/// `/BBox`); `matrix` is the Form `/Matrix` (identity when absent / for Images).
#[derive(Clone, Debug, PartialEq)]
pub struct XObjectInfo {
    /// The XObject's xref number, or `0` if direct/inline.
    pub xref: i32,
    /// The resource name the XObject is referenced under (e.g. `"Fm0"`).
    pub name: String,
    /// `"Form"`, `"Image"`, or the raw `/Subtype` for any other XObject kind.
    pub kind: String,
    /// The `/BBox` (Form) or the unit square (Image), as `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// The Form `/Matrix` as `(a, b, c, d, e, f)` (identity when absent).
    pub matrix: (f64, f64, f64, f64, f64, f64),
    /// The xref of the referencing object — the page object number.
    pub referencer: i32,
}

/// The page's XObjects as [`XObjectInfo`] entries, walking `/Resources /XObject`
/// (PyMuPDF `page.get_xobjects()`). Unlike `get_images`, this keeps **every**
/// XObject (Form *and* Image), mirroring PyMuPDF.
///
/// Deduped by xref; sorted by resource name for determinism. A missing / empty
/// `/XObject` dict yields an empty `Vec`.
#[must_use]
pub fn get_xobjects(page: &Page) -> Vec<XObjectInfo> {
    let doc = page.document();
    let referencer = page.obj_ref().num as i32;
    let mut out: Vec<XObjectInfo> = Vec::new();
    let mut seen_xref: HashSet<i32> = HashSet::new();

    let Some(xobj_dict) = resource_subdict(doc, page, "XObject") else {
        return out;
    };

    let mut entries: Vec<(&Name, &Object)> = xobj_dict.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    for (res_name, value) in entries {
        let xref = value.as_reference().map(|r| r.num as i32).unwrap_or(0);
        if xref != 0 && seen_xref.contains(&xref) {
            continue;
        }
        let Some(obj) = resolve_value(doc, value) else {
            continue;
        };
        let Some(dict) = obj.as_dict() else {
            continue;
        };
        let Some(name) = res_name.as_str() else {
            continue;
        };
        if xref != 0 {
            seen_xref.insert(xref);
        }
        out.push(build_xobject_info(dict, xref, name.to_string(), referencer));
    }
    out
}

/// Assembles one [`XObjectInfo`] from a resolved XObject dict.
fn build_xobject_info(dict: &Dict, xref: i32, name: String, referencer: i32) -> XObjectInfo {
    let kind = dict_name(dict, "Subtype").unwrap_or_default();
    let bbox = dict
        .get(&Name::new("BBox"))
        .and_then(Object::as_array)
        .map(rect_from_array)
        .unwrap_or((0.0, 0.0, 1.0, 1.0));
    let matrix = dict
        .get(&Name::new("Matrix"))
        .and_then(Object::as_array)
        .and_then(matrix_from_array)
        .unwrap_or((1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    XObjectInfo {
        xref,
        name,
        kind,
        bbox,
        matrix,
        referencer,
    }
}

/// A 4-number array as `(x0, y0, x1, y1)`; missing/short arrays degrade to zeros.
fn rect_from_array(a: &[Object]) -> (f64, f64, f64, f64) {
    let n = |i: usize| a.get(i).and_then(Object::as_f64).unwrap_or(0.0);
    (n(0), n(1), n(2), n(3))
}

/// A 6-number array as a matrix tuple; returns `None` when fewer than 6 numbers.
fn matrix_from_array(a: &[Object]) -> Option<(f64, f64, f64, f64, f64, f64)> {
    if a.len() < 6 {
        return None;
    }
    let n = |i: usize| a.get(i).and_then(Object::as_f64).unwrap_or(0.0);
    Some((n(0), n(1), n(2), n(3), n(4), n(5)))
}

// === get_image_rects ======================================================

/// One placement of an image on the page (PyMuPDF `page.get_image_rects` /
/// `get_image_info` subset): the device-space bbox plus the resource name and
/// declared pixel size. One image referenced N times yields N entries.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRect {
    /// The XObject resource name (`Do`), or empty for an inline image.
    pub name: String,
    /// `true` for an inline `BI…ID…EI` image.
    pub inline: bool,
    /// The device-space placement bbox `(x0, y0, x1, y1)` (top-left origin).
    pub bbox: (f64, f64, f64, f64),
    /// The declared pixel width (`/Width`), if present.
    pub width: u32,
    /// The declared pixel height (`/Height`), if present.
    pub height: u32,
}

/// Every image **placement** on `page`, in reading order (PyMuPDF
/// `page.get_image_rects`). Each entry's `bbox` is the device-space rectangle the
/// image occupies, taken from the layout-reconstructed image blocks.
#[must_use]
pub fn get_image_rects(page: &Page) -> Vec<ImageRect> {
    let tp = textpage(page, 0, None);
    tp.blocks
        .iter()
        .filter(|b| b.kind == pdf_text::BlockKind::Image)
        .filter_map(|b| {
            let img = b.image.as_ref()?;
            Some(ImageRect {
                name: img.name.as_ref().map(|s| s.to_string()).unwrap_or_default(),
                inline: img.name.is_none(),
                bbox: (b.bbox.x0, b.bbox.y0, b.bbox.x1, b.bbox.y1),
                width: img.width.unwrap_or(0),
                height: img.height.unwrap_or(0),
            })
        })
        .collect()
}

// === get_image_info / get_image_bbox ======================================

/// One `page.get_image_info()` entry: a per-placement dict merging the
/// device-space `bbox` with the referenced image XObject's metadata (PyMuPDF
/// `page.get_image_info`). Inline images carry an empty `name` and `xref == 0`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageInfoEntry {
    /// The placement index on the page (PyMuPDF `number`).
    pub number: usize,
    /// The image XObject's xref, or `0` for an inline image.
    pub xref: i32,
    /// The XObject resource name (empty for an inline image).
    pub name: String,
    /// `true` for an inline `BI…ID…EI` image.
    pub inline: bool,
    /// The device-space placement bbox `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// The declared pixel width (`/Width`).
    pub width: u32,
    /// The declared pixel height (`/Height`).
    pub height: u32,
    /// `/BitsPerComponent` (`0` if unknown).
    pub bpc: i32,
    /// The colorspace name (e.g. `"DeviceRGB"`, `"ICCBased"`; empty if unknown).
    pub colorspace: String,
    /// The colorspace component count (1/3/4; `0` if unknown), ICCBased-aware.
    pub cs_components: i32,
    /// A descriptive colorspace name for `extractIMGINFO` (`cs-name`).
    pub cs_name: String,
    /// The `/Filter` name (empty if none).
    pub filter: String,
}

/// Every image placement on `page` with merged XObject metadata (PyMuPDF
/// `page.get_image_info`). One image referenced N times yields N entries, each
/// keyed by placement order. Image-XObject metadata (bpc/colorspace/filter/xref)
/// is matched to a placement by resource name.
#[must_use]
pub fn get_image_info(page: &Page) -> Vec<ImageInfoEntry> {
    // Index the XObject image metadata by resource name.
    let metas = get_images(page);
    let meta_by_name: std::collections::HashMap<&str, &ImageInfo> =
        metas.iter().map(|m| (m.name.as_str(), m)).collect();

    get_image_rects(page)
        .into_iter()
        .enumerate()
        .map(|(number, r)| {
            let meta = meta_by_name.get(r.name.as_str());
            ImageInfoEntry {
                number,
                xref: meta.map(|m| m.xref).unwrap_or(0),
                name: r.name,
                inline: r.inline,
                bbox: r.bbox,
                width: r.width,
                height: r.height,
                bpc: meta.map(|m| m.bpc).unwrap_or(0),
                colorspace: meta.map(|m| m.colorspace.clone()).unwrap_or_default(),
                cs_components: meta.map(|m| m.cs_components).unwrap_or(0),
                cs_name: meta.map(|m| m.cs_name.clone()).unwrap_or_default(),
                filter: meta.map(|m| m.filter.clone()).unwrap_or_default(),
            }
        })
        .collect()
}

/// The device-space bbox of the image identified by `name_or_xref` on `page`
/// (PyMuPDF `page.get_image_bbox`). The argument is matched first as a resource
/// name, then (when it parses as an integer) as an xref. Returns the first
/// matching placement's bbox, or `None`.
#[must_use]
pub fn get_image_bbox(page: &Page, name_or_xref: &str) -> Option<(f64, f64, f64, f64)> {
    let info = get_image_info(page);
    // Match by resource name first.
    if let Some(e) = info.iter().find(|e| e.name == name_or_xref) {
        return Some(e.bbox);
    }
    // Then by xref (integer).
    if let Ok(xref) = name_or_xref.parse::<i32>() {
        if let Some(e) = info.iter().find(|e| e.xref == xref && xref != 0) {
            return Some(e.bbox);
        }
    }
    None
}

// === extractIMGINFO =======================================================

/// One `TextPage.extractIMGINFO()` entry: the per-image info dicts PyMuPDF
/// returns for images on the page. Field set matches fitz's key set (`number`,
/// `bbox`, `transform`, `width`, `height`, `colorspace`, `cs-name`, `xres`,
/// `yres`, `bpc`, `size`, `has-mask`).
#[derive(Clone, Debug, PartialEq)]
pub struct ImgInfoEntry {
    /// The placement index on the page (PyMuPDF `number`).
    pub number: usize,
    /// The device-space placement bbox `(x0, y0, x1, y1)`.
    pub bbox: (f64, f64, f64, f64),
    /// The image-to-device transform `(a, b, c, d, e, f)` mapping the unit
    /// image square to the placement bbox (fitz `transform`).
    pub transform: (f64, f64, f64, f64, f64, f64),
    /// The pixel width (`/Width`).
    pub width: u32,
    /// The pixel height (`/Height`).
    pub height: u32,
    /// The colorspace component count (1 gray, 3 RGB, 4 CMYK; `0` if unknown).
    pub colorspace: i32,
    /// The colorspace name (fitz `cs-name`, e.g. `"DeviceRGB"`).
    pub cs_name: String,
    /// Horizontal resolution in DPI (fitz default `96` when not derivable).
    pub xres: i32,
    /// Vertical resolution in DPI (fitz default `96`).
    pub yres: i32,
    /// `/BitsPerComponent` (`0` if unknown).
    pub bpc: i32,
    /// The (compressed) image stream byte size, or `0` if unavailable.
    pub size: usize,
    /// Whether the image carries an `/SMask` (soft mask) — fitz `has-mask`.
    pub has_mask: bool,
}

/// The images on `page` as fitz-shaped [`ImgInfoEntry`] dicts (PyMuPDF
/// `TextPage.extractIMGINFO`). One placement per entry, in reading order. Reuses
/// the layout-reconstructed image blocks ([`get_image_info`]) and enriches each
/// with a unit-square→bbox `transform`, the resolved component-count
/// `colorspace`, and the `/SMask`/stream-size metadata fitz reports. The
/// `colorspace` int and `cs-name` come from the resolved colorspace (indirect
/// references followed, ICCBased `/N` read), matching fitz's `extractIMGINFO`.
#[must_use]
pub fn extract_imginfo(page: &Page) -> Vec<ImgInfoEntry> {
    // XObject metadata (smask / stream length) keyed by xref for enrichment.
    let metas = get_images(page);
    let smask_by_xref: std::collections::HashMap<i32, i32> =
        metas.iter().map(|m| (m.xref, m.smask)).collect();
    let len_by_xref = stream_len_by_xref(page, &metas);

    get_image_info(page)
        .into_iter()
        .map(|e| {
            let (x0, y0, x1, y1) = e.bbox;
            // The descriptive cs-name; fall back to the colorspace family name.
            let cs_name = if e.cs_name.is_empty() {
                e.colorspace.clone()
            } else {
                e.cs_name.clone()
            };
            ImgInfoEntry {
                number: e.number,
                bbox: e.bbox,
                // Map the unit image square to the placement bbox (top-left
                // origin): scale (w, h), translate (x0, y0).
                transform: (x1 - x0, 0.0, 0.0, y1 - y0, x0, y0),
                width: e.width,
                height: e.height,
                colorspace: e.cs_components,
                cs_name,
                xres: 96,
                yres: 96,
                bpc: e.bpc,
                size: *len_by_xref.get(&e.xref).unwrap_or(&0) as usize,
                has_mask: smask_by_xref.get(&e.xref).copied().unwrap_or(0) != 0,
            }
        })
        .collect()
}

/// The `/Length` of each image XObject stream, keyed by xref (best-effort; `0`
/// when the object cannot be resolved). Used to fill fitz's `size` field.
fn stream_len_by_xref(page: &Page, metas: &[ImageInfo]) -> std::collections::HashMap<i32, i32> {
    use pdf_core::object::ObjRef;
    let doc = page.document();
    let mut out = std::collections::HashMap::new();
    for m in metas {
        if m.xref == 0 || out.contains_key(&m.xref) {
            continue;
        }
        let len = doc
            .resolve(ObjRef::new(m.xref as u32, 0))
            .ok()
            .and_then(|o| o.as_stream().map(|s| s.data.len() as i32))
            .unwrap_or(0);
        out.insert(m.xref, len);
    }
    out
}

/// A small stat reflecting the byte footprint of a [`TextPage`]'s structured
/// model (PyMuPDF `TextPage.poolsize`).
///
/// MuPDF's `poolsize` is the size of the `fz_pool` block allocator backing the
/// structured-text page — an internal allocator stat with no portable
/// definition outside MuPDF. We report a deterministic proxy: the in-memory size
/// of the model's blocks/lines/spans/chars (the data MuPDF stores in that pool),
/// so the value scales with page complexity exactly as fitz's does. (Documented
/// deviation: not byte-identical to MuPDF's pool, which over-allocates in
/// chunks.)
#[must_use]
pub fn textpage_poolsize(tp: &TextPage) -> usize {
    use std::mem::size_of;
    let mut total = size_of::<TextPage>();
    for block in &tp.blocks {
        total += size_of::<pdf_text::Block>();
        for line in &block.lines {
            total += size_of::<pdf_text::Line>();
            for span in &line.spans {
                total += size_of::<pdf_text::Span>();
                total += span.text.len();
                total += span.chars.len() * size_of::<pdf_text::Char>();
            }
        }
    }
    total
}

/// The text within `clip` (PyMuPDF `TextPage.extractTextbox`). Delegates to
/// [`pdf_text::get_textbox`] over a pre-built `tp`.
#[must_use]
pub fn extract_textbox(tp: &TextPage, clip: Rect) -> String {
    pdf_text::get_textbox(tp, clip)
}

/// The text between two device-space points `a` and `b`, like a mouse drag
/// (PyMuPDF `TextPage.extractSelection`). Delegates to
/// [`pdf_text::extract_selection`] over a pre-built `tp`.
#[must_use]
pub fn extract_selection(
    tp: &TextPage,
    a: pdf_core::geom::Point,
    b: pdf_core::geom::Point,
) -> String {
    pdf_text::extract_selection(tp, a, b)
}

// === reusable TextPage ====================================================

/// A neutral `get_text` result the PyO3 layer converts to the right Python
/// object. Each variant maps to a PyMuPDF `get_text(opt)` family.
#[derive(Clone, Debug, PartialEq)]
pub enum TextOutput {
    /// `text`/`html`/`xhtml`/`xml`/`json`/`rawjson` — a single string.
    Text(String),
    /// `blocks` — `(x0, y0, x1, y1, text, block_no, type)` tuples.
    Blocks(Vec<pdf_text::BlockTuple>),
    /// `words` — `(x0, y0, x1, y1, word, block_no, line_no, word_no)` tuples.
    Words(Vec<pdf_text::WordTuple>),
    /// `dict`/`rawdict` — the structured tree.
    Dict(TextDict),
    /// Reserved JSON variant (unused: `json`/`rawjson` go through `Text`).
    Json(String),
}

/// Builds the [`TextPage`] model for `page` once (PRD §9.4). `flags`/`clip` are
/// accepted for API symmetry but applied at serialization / search time, not at
/// build time, so the model is reusable across every output.
#[must_use]
pub fn textpage(page: &Page, _flags: u32, _clip: Option<Rect>) -> TextPage {
    let doc = page.document();
    pdf_text::build_textpage(doc, page, &Limits::default())
}

/// Extracts text in the given PyMuPDF `opt` ("text", "html", "xhtml", "xml",
/// "json", "rawjson", "dict", "rawdict", "blocks", "words"), optionally reusing
/// a pre-built `tp` instead of rebuilding the [`TextPage`].
///
/// `flags` overrides the per-method default flag set (PRD §8.6.2) when `Some`.
/// An unknown `opt` falls back to plain `text`.
#[must_use]
pub fn get_text(page: &Page, opt: &str, flags: Option<u32>, tp: Option<&TextPage>) -> TextOutput {
    // Build-or-reuse the model.
    let owned;
    let tp: &TextPage = match tp {
        Some(t) => t,
        None => {
            owned = textpage(page, 0, None);
            &owned
        }
    };

    match opt {
        "blocks" => TextOutput::Blocks(pdf_text::to_blocks(tp, flags.unwrap_or(defaults::BLOCKS))),
        "words" => TextOutput::Words(pdf_text::to_words(tp, flags.unwrap_or(defaults::WORDS))),
        "dict" => TextOutput::Dict(pdf_text::to_dict(
            tp,
            false,
            flags.unwrap_or(defaults::DICT),
        )),
        "rawdict" => TextOutput::Dict(pdf_text::to_dict(
            tp,
            true,
            flags.unwrap_or(defaults::RAWDICT),
        )),
        "json" => TextOutput::Text(pdf_text::to_json(
            tp,
            false,
            flags.unwrap_or(defaults::JSON),
        )),
        "rawjson" => TextOutput::Text(pdf_text::to_json(
            tp,
            true,
            flags.unwrap_or(defaults::RAWJSON),
        )),
        "html" => TextOutput::Text(pdf_text::to_html(tp, flags.unwrap_or(defaults::HTML))),
        "xhtml" => TextOutput::Text(pdf_text::to_xhtml(tp, flags.unwrap_or(defaults::XHTML))),
        "xml" => TextOutput::Text(pdf_text::to_xml(tp, flags.unwrap_or(defaults::XML))),
        // "text" and any unknown option → plain text.
        _ => TextOutput::Text(pdf_text::to_text(tp, flags.unwrap_or(defaults::TEXT))),
    }
}

/// Searches `page` for `needle`, optionally reusing a pre-built `tp`. Returns the
/// hit quads (PyMuPDF `page.search_for`). Delegates to `pdf_text::search`.
#[must_use]
pub fn search(
    page: &Page,
    needle: &str,
    opts: pdf_text::SearchOptions,
    tp: Option<&TextPage>,
) -> Vec<Quad> {
    let owned;
    let tp: &TextPage = match tp {
        Some(t) => t,
        None => {
            owned = textpage(page, 0, opts.clip);
            &owned
        }
    };
    pdf_text::search(tp, needle, opts)
}

// === resource helpers =====================================================

/// The page's `/Resources /<sub>` dictionary, resolving each level. `None` when
/// the page has no resources or the sub-dict is absent.
fn resource_subdict(doc: &DocumentStore, page: &Page, sub: &str) -> Option<Dict> {
    let page_dict = page.dict()?;
    let resources = resolve_key(doc, &page_dict, "Resources")?;
    let res_dict = resources.as_dict()?;
    let sub_obj = resolve_key(doc, res_dict, sub)?;
    sub_obj.as_dict().cloned()
}

/// Resolves `dict[key]` (following a reference) to a non-reference object.
fn resolve_key(doc: &DocumentStore, dict: &Dict, key: &str) -> Option<std::sync::Arc<Object>> {
    doc.resolve_dict_key(dict, &Name::new(key)).ok().flatten()
}

/// Resolves a value that may be a reference to its non-reference object.
fn resolve_value(doc: &DocumentStore, value: &Object) -> Option<std::sync::Arc<Object>> {
    match value {
        Object::Reference(r) => doc.resolve(*r).ok(),
        other => Some(std::sync::Arc::new(other.clone())),
    }
}

/// A dict name value as an owned `String`.
fn dict_name(dict: &Dict, key: &str) -> Option<String> {
    dict.get(&Name::new(key))
        .and_then(Object::as_name)
        .and_then(Name::as_str)
        .map(str::to_string)
}

/// A dict integer value, defaulting to `0` when absent / non-integer.
fn dict_int(dict: &Dict, key: &str) -> i32 {
    dict.get(&Name::new(key))
        .and_then(Object::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(0)
}
