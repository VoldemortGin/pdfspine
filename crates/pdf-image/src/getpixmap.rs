//! `get_pixmap` for PDF pages + `extract_image` (PRD §3.3 / §8.10).
//!
//! Per the normative §3.3 rule, `get_pixmap` is in scope in v1 for **image-only
//! PDF pages** (the scanned-document case) and out of scope (deferred to M6) for
//! vector/text pages. This module implements:
//!
//! - the **image-only-page classifier** ([`classify_page`]): tokenizes the
//!   concatenated `/Contents` (resolving Form XObjects, depth-capped) and accepts
//!   a page **iff** it contains only graphics-state / color operators plus one or
//!   more `Do` invocations that each resolve to an image XObject, and **none** of
//!   the text / path-paint / shading / inline-image-with-vector operators;
//! - [`page_pixmap`]: decode the page's image XObject(s) → [`Pixmap`], honoring an
//!   optional scale (matrix/dpi); a vector page yields [`Error::Unsupported`]
//!   (mapped to `PdfUnsupportedError`), an undecodable image yields a typed
//!   decode error (text extraction stays independent — it never calls this path);
//! - [`extract_image`]: the image XObject's `{ext, colorspace, bpc, width,
//!   height, image, …}` descriptor (PyMuPDF `Document.extract_image`).

use std::collections::HashSet;

use pdf_core::{Dict, DocumentStore, Name, ObjRef, Object, StreamObj};

use crate::codecs::{decode_image_stream, pixmap_from_stream, DecodedImage};
use crate::error::{Error, Result};
use crate::pixmap::{Colorspace, Pixmap};

/// Max Form-XObject recursion depth for the classifier (cycle / blow-up guard).
const MAX_FORM_DEPTH: u32 = 16;

/// The classification of a PDF page for the `get_pixmap` image path (§3.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageClass {
    /// An image-only page: the resolved image XObject object-refs drawn by `Do`,
    /// in draw order. The single-image case (`refs.len() == 1`) is the common
    /// scanned page; multiple images are composited onto the page raster.
    ImageOnly {
        /// The image XObject references drawn, in `Do` order.
        refs: Vec<ObjRef>,
    },
    /// A vector / text page — deferred to M6 ([`Error::Unsupported`] from
    /// `get_pixmap`).
    Vector,
}

/// Classifies `page` per the normative §3.3 image-only-page rule.
///
/// Returns [`PageClass::ImageOnly`] with the drawn image refs when the page's
/// operator stream contains only graphics-state / color ops and image `Do`s;
/// [`PageClass::Vector`] otherwise (any text / path / shading / inline-image-
/// with-vector operator, or a `Do` of a non-image XObject).
#[must_use]
pub fn classify_page(doc: &DocumentStore, page: &Dict) -> PageClass {
    let resources = resolve_dict(doc, page, "Resources").unwrap_or_default();
    let content = page_content(doc, page);
    let mut refs: Vec<ObjRef> = Vec::new();
    let mut visited = HashSet::new();
    if scan_image_only(doc, &content, &resources, 0, &mut visited, &mut refs) && !refs.is_empty() {
        PageClass::ImageOnly { refs }
    } else {
        PageClass::Vector
    }
}

/// Decodes `page` to a [`Pixmap`] for the image-only path (§3.3), scaling the
/// output by `scale` (≥0; `1.0` = native image resolution; e.g. `dpi/72`).
///
/// # Errors
///
/// - [`Error::Unsupported`] (`"get_pixmap: vector page"`) for a vector/text page
///   (deferred to M6).
/// - A typed [`Error::Decode`] / [`Error::Unsupported`] for an image-only page
///   whose referenced image fails to decode (the §8.4.1 degradation contract —
///   `get_text` on the same page is unaffected, it never enters this path).
pub fn page_pixmap(doc: &DocumentStore, page: &Dict, scale: f64, alpha: bool) -> Result<Pixmap> {
    let refs = match classify_page(doc, page) {
        PageClass::ImageOnly { refs } => refs,
        PageClass::Vector => return Err(Error::Unsupported("get_pixmap: vector page")),
    };
    // v1 supports the headline single-image scanned-page case directly: decode
    // the (first) image XObject and return its raster. Multi-image compositing
    // onto a page raster with per-image CTMs is the documented partial — we take
    // the first image (the dominant scanned-page layout) and scale it.
    let first = *refs
        .first()
        .ok_or(Error::Unsupported("get_pixmap: no image"))?;
    let stream = resolve_stream(doc, first)?;
    let smask = decode_smask(doc, &stream.dict);

    // Colorspace-aware decode: Indexed palette lookup, Separation/DeviceN tint
    // transform, Lab, and the `/Decode` array are resolved in one place (P3-3).
    let mut pix = pixmap_from_stream(doc, &stream.dict, &stream_raw(doc, &stream)?)?;
    if alpha {
        if let Some((mask, mw, mh)) = smask {
            pix = pix.with_smask_gray(&mask, mw, mh)?;
        } else {
            // Opaque alpha channel requested but no /SMask: add a fully-opaque one.
            let (w, h) = (pix.width, pix.height);
            let opaque = vec![255u8; w as usize * h as usize];
            pix = pix.with_smask_gray(&opaque, w, h)?;
        }
    }
    if (scale - 1.0).abs() > f64::EPSILON {
        pix = resample(&pix, scale)?;
    }
    Ok(pix)
}

/// Nearest-neighbor resamples `pix` by `scale` (>0). Used to honor a
/// `matrix`/`dpi` scale request minimally (PRD §8.10: "scale the output dims";
/// affine clip / rotation is the documented partial, deferred refinement).
fn resample(pix: &Pixmap, scale: f64) -> Result<Pixmap> {
    if scale <= 0.0 || !scale.is_finite() {
        return Err(Error::InvalidArgument("non-positive pixmap scale"));
    }
    let nw = ((pix.width as f64) * scale).round().max(1.0) as u32;
    let nh = ((pix.height as f64) * scale).round().max(1.0) as u32;
    if nw == pix.width && nh == pix.height {
        return Ok(pix.clone());
    }
    crate::codecs::guard_dimensions(nw, nh, "Pixmap")
        .map_err(|_| Error::LimitExceeded("pixmap resample exceeds cap"))?;
    let n = pix.n as usize;
    let src = pix.samples();
    let mut out = Vec::with_capacity(nw as usize * nh as usize * n);
    for y in 0..nh as usize {
        let sy = ((y as f64 + 0.5) / scale).floor() as usize;
        let sy = sy.min(pix.height as usize - 1);
        for x in 0..nw as usize {
            let sx = ((x as f64 + 0.5) / scale).floor() as usize;
            let sx = sx.min(pix.width as usize - 1);
            let base = sy * pix.stride + sx * n;
            out.extend_from_slice(&src[base..base + n]);
        }
    }
    Pixmap::try_new(nw, nh, pix.colorspace, pix.alpha, out)
}

/// A `PyMuPDF`-shaped `extract_image` descriptor for one image XObject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedImage {
    /// File extension / format token (`"png"`, `"jpeg"`, `"jpx"`, `"jb2"`,
    /// `"ccitt"`, or `"png"` for re-encoded raw rasters).
    pub ext: String,
    /// `/ColorSpace` name as a string (`"DeviceRGB"` etc.), best-effort.
    pub colorspace: String,
    /// `/BitsPerComponent`.
    pub bpc: i32,
    /// `/Width`.
    pub width: i32,
    /// `/Height`.
    pub height: i32,
    /// Number of color components.
    pub components: i32,
    /// The `/SMask` xref, or `0`.
    pub smask: i32,
    /// The image payload bytes (codec-native for DCT/JPX/JBIG2/CCITT passthrough;
    /// PNG-encoded for raw/Flate rasters).
    pub image: Vec<u8>,
}

/// Extracts the image XObject at object number `xref` (PyMuPDF
/// `Document.extract_image`).
///
/// For DCT/JPX the codec-native bytes are returned verbatim (the natural file
/// is a JPEG / JP2). For JBIG2/CCITT the still-encoded payload is returned with
/// its codec token. For raw / Flate rasters the samples are decoded and
/// re-encoded as PNG so the bytes are a standalone openable image.
///
/// # Errors
///
/// [`Error::InvalidArgument`] if `xref` is not an image XObject; decode errors
/// propagate for the PNG re-encode path.
pub fn extract_image(doc: &DocumentStore, xref: u32) -> Result<ExtractedImage> {
    let stream = resolve_stream(doc, ObjRef::new(xref, 0))?;
    let dict = &stream.dict;
    if dict_name(dict, "Subtype").as_deref() != Some("Image") {
        return Err(Error::InvalidArgument("xref is not an image XObject"));
    }
    let width = dict_int(doc, dict, "Width", "W");
    let height = dict_int(doc, dict, "Height", "H");
    let bpc = dict_int(doc, dict, "BitsPerComponent", "BPC");
    let colorspace = colorspace_string(doc, dict);
    let smask = dict
        .get(&Name::new("SMask"))
        .and_then(Object::as_reference)
        .map(|r| r.num as i32)
        .unwrap_or(0);

    let raw = stream_raw(doc, &stream)?;
    let filter = terminal_image_filter(doc, dict);
    let (ext, image, components) = match filter.as_deref() {
        Some("DCTDecode") => ("jpeg".to_string(), raw, components_for(&colorspace)),
        Some("JPXDecode") => ("jpx".to_string(), raw, components_for(&colorspace)),
        Some("JBIG2Decode") => ("jb2".to_string(), raw, 1),
        Some("CCITTFaxDecode") => ("ccitt".to_string(), raw, 1),
        _ => {
            // Raw / Flate raster: decode → Pixmap → PNG so the bytes are openable.
            let decoded = decode_image_stream(doc, dict, &raw)?;
            let comps = decoded.components as i32;
            let pix = Pixmap::from_decoded(&decoded)?;
            ("png".to_string(), pix.to_png_bytes()?, comps)
        }
    };

    Ok(ExtractedImage {
        ext,
        colorspace,
        bpc,
        width,
        height,
        components,
        smask,
        image,
    })
}

// === internals ============================================================

/// Concatenates the page's `/Contents` (single stream or array) into one decoded
/// content buffer, joining streams with a newline.
fn page_content(doc: &DocumentStore, page: &Dict) -> Vec<u8> {
    let Some(contents) = doc
        .resolve_dict_key(page, &Name::new("Contents"))
        .ok()
        .flatten()
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match contents.as_ref() {
        Object::Stream(s) => append_decoded(doc, s, &mut out),
        Object::Array(arr) => {
            for item in arr {
                let resolved = match item {
                    Object::Reference(r) => doc.resolve(*r).ok(),
                    other => Some(std::sync::Arc::new(other.clone())),
                };
                if let Some(obj) = resolved {
                    if let Some(s) = obj.as_stream() {
                        if !out.is_empty() {
                            out.push(b'\n');
                        }
                        append_decoded(doc, s, &mut out);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Appends a stream's decoded content bytes to `out` (best-effort; a decode
/// failure leaves `out` unchanged).
fn append_decoded(doc: &DocumentStore, s: &StreamObj, out: &mut Vec<u8>) {
    if let Ok(bytes) = doc.decode_stream(s).and_then(|o| o.into_decoded()) {
        out.extend_from_slice(&bytes);
    }
}

/// Recursively scans `content` for image-only-ness. Returns `false` the moment a
/// disqualifying operator (text/path/shading/inline-image) or a `Do` of a
/// non-image XObject is seen; pushes image refs in `Do` order otherwise.
fn scan_image_only(
    doc: &DocumentStore,
    content: &[u8],
    resources: &Dict,
    depth: u32,
    visited: &mut HashSet<u32>,
    refs: &mut Vec<ObjRef>,
) -> bool {
    let ops = scan_operators(content);
    let xobjects = resolve_dict(doc, resources, "XObject").unwrap_or_default();
    for op in &ops {
        match op {
            Operator::Disqualifying => return false,
            Operator::Allowed => {}
            Operator::Do(name) => {
                let Some(target) = xobjects.get(&Name::new(name)) else {
                    return false; // referenced resource missing → not classifiable
                };
                let Some(xref) = target.as_reference() else {
                    return false; // inline/direct XObject — out of scope here
                };
                let Ok(obj) = doc.resolve(xref) else {
                    return false;
                };
                let Some(xdict) = obj.as_dict() else {
                    return false;
                };
                match dict_name(xdict, "Subtype").as_deref() {
                    Some("Image") => refs.push(xref),
                    Some("Form") => {
                        if depth >= MAX_FORM_DEPTH || !visited.insert(xref.num) {
                            return false; // too deep / cyclic
                        }
                        let Some(form) = obj.as_stream() else {
                            return false;
                        };
                        let Ok(form_content) =
                            doc.decode_stream(form).and_then(|o| o.into_decoded())
                        else {
                            return false;
                        };
                        let form_res = resolve_dict(doc, xdict, "Resources")
                            .unwrap_or_else(|| resources.clone());
                        if !scan_image_only(doc, &form_content, &form_res, depth + 1, visited, refs)
                        {
                            return false;
                        }
                        visited.remove(&xref.num);
                    }
                    _ => return false, // unknown XObject subtype
                }
            }
        }
    }
    true
}

/// A coarse content-operator classification for the image-only-page rule.
enum Operator {
    /// Graphics-state / color operator (allowed): `q Q cm gs cs CS sc scn …`.
    Allowed,
    /// A text / path-paint / shading / inline-image operator → vector page.
    Disqualifying,
    /// `Do` of the named resource.
    Do(String),
}

/// Tokenizes `content` into a flat operator list (operands skipped). String /
/// array / dict / name literals are skipped wholesale so their bytes are never
/// mistaken for operators; an inline image (`BI … ID … EI`) is treated as a
/// single disqualifying operator (it carries vector context per §3.3).
fn scan_operators(content: &[u8]) -> Vec<Operator> {
    let mut ops = Vec::new();
    let mut i = 0usize;
    let n = content.len();
    let mut last_name: Option<String> = None;

    while i < n {
        let b = content[i];
        match b {
            // Whitespace.
            b if is_ws(b) => i += 1,
            // Comment to end of line.
            b'%' => {
                while i < n && content[i] != b'\n' && content[i] != b'\r' {
                    i += 1;
                }
            }
            // Literal string ( ... ) with balanced parens + escapes.
            b'(' => {
                i = skip_literal_string(content, i);
                last_name = None;
            }
            // Hex string < ... > or dict << >>.
            b'<' => {
                if i + 1 < n && content[i + 1] == b'<' {
                    i = skip_dict(content, i);
                } else {
                    i = skip_hex_string(content, i);
                }
                last_name = None;
            }
            // Array [ ... ] — skip; operands only.
            b'[' => {
                i = skip_array(content, i);
                last_name = None;
            }
            b']' | b'>' | b')' | b'}' | b'{' => {
                i += 1;
            }
            // Name /Foo — remember it (the operand `Do` consumes).
            b'/' => {
                let (name, ni) = read_name(content, i);
                last_name = Some(name);
                i = ni;
            }
            // Number / sign / dot — operand, skip the token.
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                i = skip_token(content, i);
                last_name = None;
            }
            // Otherwise a keyword/operator token.
            _ => {
                let (tok, ni) = read_token(content, i);
                i = ni;
                classify_token(&tok, &mut last_name, &mut ops, content, &mut i);
            }
        }
    }
    ops
}

/// Classifies a bare keyword token into an [`Operator`], handling `Do` (consumes
/// the last `/Name`) and the inline-image `BI…EI` block (advances `*i` past it).
fn classify_token(
    tok: &[u8],
    last_name: &mut Option<String>,
    ops: &mut Vec<Operator>,
    content: &[u8],
    i: &mut usize,
) {
    match tok {
        b"Do" => {
            if let Some(name) = last_name.take() {
                ops.push(Operator::Do(name));
            } else {
                ops.push(Operator::Disqualifying);
            }
        }
        b"BI" => {
            // Inline image with vector context (§3.3) → disqualify, skip to EI.
            ops.push(Operator::Disqualifying);
            *i = skip_to_ei(content, *i);
        }
        // Allowed graphics-state / color operators.
        b"q" | b"Q" | b"cm" | b"gs" | b"cs" | b"CS" | b"sc" | b"scn" | b"SC" | b"SCN" | b"g"
        | b"G" | b"rg" | b"RG" | b"k" | b"K" | b"ri" | b"i" | b"j" | b"J" | b"w" | b"M" | b"d"
        | b"BDC" | b"BMC" | b"EMC" | b"DP" | b"MP" => {
            ops.push(Operator::Allowed);
            *last_name = None;
        }
        // Everything else — text (BT/ET/Tj/TJ/'/"/Tf/Td/…), path
        // (m/l/c/v/y/re/h/S/s/f/F/B/b/n/W/W*), shading (sh), … → vector page.
        _ => {
            ops.push(Operator::Disqualifying);
            *last_name = None;
        }
    }
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Reads a bare keyword/operator token (regular chars), returning `(token, next)`.
fn read_token(content: &[u8], start: usize) -> (Vec<u8>, usize) {
    let mut i = start;
    while i < content.len() && !is_ws(content[i]) && !is_delim(content[i]) {
        i += 1;
    }
    (content[start..i].to_vec(), i)
}

/// Skips a numeric/operand token (regular chars), returning the next index.
fn skip_token(content: &[u8], start: usize) -> usize {
    read_token(content, start).1
}

/// Reads a `/Name` token, returning `(name_without_slash, next)`.
fn read_name(content: &[u8], start: usize) -> (String, usize) {
    let mut i = start + 1; // skip '/'
    let begin = i;
    while i < content.len() && !is_ws(content[i]) && !is_delim(content[i]) {
        i += 1;
    }
    (String::from_utf8_lossy(&content[begin..i]).into_owned(), i)
}

/// Skips a literal `( ... )` string (balanced parens, backslash escapes).
fn skip_literal_string(content: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1i32;
    while i < content.len() && depth > 0 {
        match content[i] {
            b'\\' => i += 2,
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// Skips a hex `< ... >` string.
fn skip_hex_string(content: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < content.len() && content[i] != b'>' {
        i += 1;
    }
    (i + 1).min(content.len())
}

/// Skips a `<< ... >>` dictionary (nesting-aware).
fn skip_dict(content: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    let mut depth = 1i32;
    while i < content.len() && depth > 0 {
        if i + 1 < content.len() && content[i] == b'<' && content[i + 1] == b'<' {
            depth += 1;
            i += 2;
        } else if i + 1 < content.len() && content[i] == b'>' && content[i + 1] == b'>' {
            depth -= 1;
            i += 2;
        } else if content[i] == b'(' {
            i = skip_literal_string(content, i);
        } else {
            i += 1;
        }
    }
    i
}

/// Skips an `[ ... ]` array (nesting + string-aware).
fn skip_array(content: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1i32;
    while i < content.len() && depth > 0 {
        match content[i] {
            b'[' => {
                depth += 1;
                i += 1;
            }
            b']' => {
                depth -= 1;
                i += 1;
            }
            b'(' => i = skip_literal_string(content, i),
            _ => i += 1,
        }
    }
    i
}

/// Skips to just past an inline image's `EI` keyword.
fn skip_to_ei(content: &[u8], start: usize) -> usize {
    let mut i = start;
    while i + 1 < content.len() {
        if content[i] == b'E'
            && content[i + 1] == b'I'
            && (i == 0 || is_ws(content[i - 1]))
            && (i + 2 >= content.len() || is_ws(content[i + 2]) || is_delim(content[i + 2]))
        {
            return i + 2;
        }
        i += 1;
    }
    content.len()
}

/// Resolves a `/SMask` to an 8-bit gray plane `(bytes, w, h)`, or `None`.
fn decode_smask(doc: &DocumentStore, dict: &Dict) -> Option<(Vec<u8>, u32, u32)> {
    let r = dict
        .get(&Name::new("SMask"))
        .and_then(Object::as_reference)?;
    let stream = resolve_stream(doc, r).ok()?;
    let raw = stream_raw(doc, &stream).ok()?;
    let decoded: DecodedImage = decode_image_stream(doc, &stream.dict, &raw).ok()?;
    if decoded.components != 1 {
        return None;
    }
    let pix = Pixmap::from_decoded(&decoded).ok()?;
    if pix.colorspace != Colorspace::Gray {
        return None;
    }
    Some((pix.samples().to_vec(), pix.width, pix.height))
}

/// Resolves `xref` to an owned [`StreamObj`].
fn resolve_stream(doc: &DocumentStore, xref: ObjRef) -> Result<StreamObj> {
    let obj = doc.resolve(xref)?;
    obj.as_stream()
        .cloned()
        .ok_or(Error::InvalidArgument("object is not a stream"))
}

/// The raw (still source-/filter-encoded) bytes of a stream.
fn stream_raw(doc: &DocumentStore, stream: &StreamObj) -> Result<Vec<u8>> {
    Ok(doc.stream_raw_bytes(stream)?.to_vec())
}

/// Resolves `dict[key]` to an owned sub-dictionary.
fn resolve_dict(doc: &DocumentStore, dict: &Dict, key: &str) -> Option<Dict> {
    doc.resolve_dict_key(dict, &Name::new(key))
        .ok()
        .flatten()
        .and_then(|o| o.as_dict().cloned())
}

/// A dict `/Name` value as an owned `String`.
fn dict_name(dict: &Dict, key: &str) -> Option<String> {
    dict.get(&Name::new(key))
        .and_then(Object::as_name)
        .and_then(Name::as_str)
        .map(str::to_string)
}

/// A dict integer (trying `key` then the inline-image `abbr`), defaulting to 0.
fn dict_int(doc: &DocumentStore, dict: &Dict, key: &str, abbr: &str) -> i32 {
    crate::codecs::param_i64(doc, dict, key)
        .or_else(|| crate::codecs::param_i64(doc, dict, abbr))
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(0)
}

/// Best-effort `/ColorSpace` name as a string.
fn colorspace_string(doc: &DocumentStore, dict: &Dict) -> String {
    match crate::codecs::resolved(doc, dict, "ColorSpace") {
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

/// Component count implied by a colorspace name (best-effort).
fn components_for(colorspace: &str) -> i32 {
    match colorspace {
        "DeviceGray" | "CalGray" | "G" => 1,
        "DeviceCMYK" | "CMYK" => 4,
        _ => 3,
    }
}

/// The terminal image `/Filter` name (single or last in an array), if any.
fn terminal_image_filter(doc: &DocumentStore, dict: &Dict) -> Option<String> {
    let f = crate::codecs::resolved(doc, dict, "Filter")?;
    let name = match f {
        Object::Name(n) => n.as_str().map(str::to_string),
        Object::Array(a) => a
            .last()
            .and_then(Object::as_name)
            .and_then(Name::as_str)
            .map(str::to_string),
        _ => None,
    }?;
    match name.as_str() {
        "DCTDecode" | "DCT" => Some("DCTDecode".to_string()),
        "JPXDecode" | "JPX" => Some("JPXDecode".to_string()),
        "JBIG2Decode" => Some("JBIG2Decode".to_string()),
        "CCITTFaxDecode" | "CCF" => Some("CCITTFaxDecode".to_string()),
        _ => None,
    }
}
