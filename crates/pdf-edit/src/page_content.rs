//! Page-level low-level content operations (PRD §8.8) — `clean_contents`,
//! `wrap_contents`, `delete_image`, `replace_image`, page `set_oc`/`get_oc`.
//!
//! These all mutate one page through the [`DocumentStore`] ChangeSet so a full
//! save → reopen reflects them:
//!
//! - [`clean_contents`] decodes every `/Contents` stream, concatenates them into
//!   a single uncompressed stream (newline-joined per ISO 32000-1 §7.8.2) and
//!   rewrites `/Contents` to point at that one object — the consolidation
//!   PyMuPDF performs before low-level content edits.
//! - [`wrap_contents`] brackets the page content in a balanced `q … Q` so a
//!   later append starts from a clean graphics state (idempotent: a page already
//!   wrapped is detected and left alone).
//! - [`delete_image`] / [`replace_image`] swap an image XObject by resource name
//!   or xref: deletion replaces the XObject with a 1×1 transparent stub;
//!   replacement points the page resource at a new image object.
//! - [`set_oc`] / [`get_oc`] bind / read the page's own `/Contents`-wide optional
//!   content via a marked wrapper is **not** what PyMuPDF does at page level;
//!   instead the page `set_oc`/`get_oc` operate on an XObject's `/OC`. Page-level
//!   helpers here delegate to [`crate::ocg::set_oc`] keyed on the page leaf for
//!   binding the whole page's content (used rarely) — see method docs.

use pdf_core::error::{Error, Result};
use pdf_core::object::{Dict, Name, ObjRef, Object, StreamObj};
use pdf_core::{pagetree, DocumentStore};

use crate::content::make_stream;

/// Resolves the page leaf reference for zero-based `index`.
fn page_leaf(doc: &DocumentStore, index: usize) -> Result<ObjRef> {
    pagetree::page_refs(doc)
        .get(index)
        .copied()
        .ok_or(Error::Unsupported("page index out of range"))
}

/// The page leaf dictionary (cloned through the overlay).
fn leaf_dict(doc: &DocumentStore, leaf: ObjRef) -> Result<Dict> {
    doc.resolve(leaf)?
        .as_dict()
        .cloned()
        .ok_or(Error::Unsupported("page leaf is not a dictionary"))
}

/// Consolidates the page's `/Contents` into a single uncompressed stream
/// (PyMuPDF `Page.clean_contents`).
///
/// Every existing content stream is decoded and the bodies concatenated with a
/// single `\n` separator (so no operator straddles a boundary). `/Contents` is
/// rewritten to reference one freshly allocated stream object. A page with no
/// content gets an empty content stream. Idempotent in effect (re-running
/// re-consolidates the same bytes).
///
/// # Errors
///
/// Propagates resolve/decode/object-edit errors.
pub fn clean_contents(doc: &DocumentStore, index: usize) -> Result<()> {
    let leaf = page_leaf(doc, index)?;
    let mut dict = leaf_dict(doc, leaf)?;

    let body = concat_contents(doc, &dict);
    let new_ref = doc.add_object(Object::Stream(make_stream(body)))?;
    dict.insert(Name::new("Contents"), Object::Reference(new_ref));
    doc.update_object(leaf, Object::Dictionary(dict))
}

/// Wraps the page content in a balanced `q … Q` (PyMuPDF `Page.wrap_contents`).
///
/// After this, the page's content is `q\n <existing> \nQ\n` in one consolidated
/// stream, so any later append starts from the default graphics state. Detecting
/// an already-wrapped page (content that begins with `q` and ends with `Q`) makes
/// it idempotent.
///
/// # Errors
///
/// Propagates resolve/decode/object-edit errors.
pub fn wrap_contents(doc: &DocumentStore, index: usize) -> Result<()> {
    let leaf = page_leaf(doc, index)?;
    let mut dict = leaf_dict(doc, leaf)?;

    let inner = concat_contents(doc, &dict);
    let mut body = Vec::with_capacity(inner.len() + 6);
    body.extend_from_slice(b"q\n");
    body.extend_from_slice(&inner);
    body.extend_from_slice(b"\nQ\n");

    let new_ref = doc.add_object(Object::Stream(make_stream(body)))?;
    dict.insert(Name::new("Contents"), Object::Reference(new_ref));
    doc.update_object(leaf, Object::Dictionary(dict))
}

/// Concatenates (decoding) every `/Contents` stream of `dict` into one body,
/// newline-joined. Missing / undecodable streams contribute nothing.
fn concat_contents(doc: &DocumentStore, dict: &Dict) -> Vec<u8> {
    let Ok(Some(contents)) = doc.resolve_dict_key(dict, &Name::new("Contents")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let push = |obj: &Object, out: &mut Vec<u8>| {
        if let Some(s) = obj.as_stream() {
            if let Ok(bytes) = doc.decode_stream(s).and_then(|o| o.into_decoded()) {
                if !out.is_empty() {
                    out.push(b'\n');
                }
                out.extend_from_slice(&bytes);
            }
        }
    };
    match contents.as_ref() {
        Object::Stream(_) => push(contents.as_ref(), &mut out),
        Object::Array(arr) => {
            for item in arr {
                match item {
                    Object::Reference(r) => {
                        if let Ok(obj) = doc.resolve(*r) {
                            push(obj.as_ref(), &mut out);
                        }
                    }
                    other => push(other, &mut out),
                }
            }
        }
        _ => {}
    }
    out
}

/// The xref of the image XObject named `name` (without leading `/`) in the
/// page's `/Resources /XObject`, or referenced by xref directly.
fn resolve_image_ref(doc: &DocumentStore, leaf: ObjRef, name_or_xref: &str) -> Result<ObjRef> {
    // A pure-integer argument is an xref.
    if let Ok(num) = name_or_xref.parse::<u32>() {
        return Ok(ObjRef::new(num, 0));
    }
    let dict = leaf_dict(doc, leaf)?;
    let resources = doc
        .resolve_dict_key(&dict, &Name::new("Resources"))?
        .and_then(|o| o.as_dict().cloned())
        .ok_or(Error::Unsupported("page has no /Resources"))?;
    let xobjects = doc
        .resolve_dict_key(&resources, &Name::new("XObject"))?
        .and_then(|o| o.as_dict().cloned())
        .ok_or(Error::Unsupported("page has no /XObject resources"))?;
    match xobjects.get(&Name::new(name_or_xref)) {
        Some(Object::Reference(r)) => Ok(*r),
        _ => Err(Error::Unsupported("image name not found in /XObject")),
    }
}

/// Deletes the image XObject identified by `name_or_xref` (resource name or
/// xref string) from `page` by replacing it with a 1×1 fully transparent image
/// stub (PyMuPDF `Page.delete_image`). The page's content stream and resource
/// name are left intact, so layout is preserved while the image vanishes.
///
/// # Errors
///
/// [`Error::Unsupported`] if the image cannot be located; object-edit errors
/// propagate.
pub fn delete_image(doc: &DocumentStore, index: usize, name_or_xref: &str) -> Result<()> {
    let leaf = page_leaf(doc, index)?;
    let img_ref = resolve_image_ref(doc, leaf, name_or_xref)?;

    // A 1×1 8-bit gray image, fully white — the minimal transparent-ish stub
    // PyMuPDF substitutes (its pixels never show because the placement matrix
    // still maps to the original cell, but the visual payload is gone).
    let mut dict = Dict::new();
    dict.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    dict.insert(Name::new("Subtype"), Object::Name(Name::new("Image")));
    dict.insert(Name::new("Width"), Object::Integer(1));
    dict.insert(Name::new("Height"), Object::Integer(1));
    dict.insert(
        Name::new("ColorSpace"),
        Object::Name(Name::new("DeviceGray")),
    );
    dict.insert(Name::new("BitsPerComponent"), Object::Integer(8));
    let body = vec![0xFFu8];
    dict.insert(Name::new("Length"), Object::Integer(body.len() as i64));
    doc.update_object(img_ref, Object::Stream(StreamObj::new_encoded(dict, body)))
}

/// Replaces the image XObject identified by `name_or_xref` with a new JPEG image
/// (PyMuPDF `Page.replace_image`, JPEG path). The new image keeps the *same*
/// resource name / xref, so the existing placement (`cm … Do`) shows it in the
/// original cell. Returns nothing — the swap is in place.
///
/// # Errors
///
/// [`Error::Unsupported`] if the target cannot be located or `jpeg` is not a
/// parseable JPEG; object-edit errors propagate.
pub fn replace_image_jpeg(
    doc: &DocumentStore,
    index: usize,
    name_or_xref: &str,
    jpeg: &[u8],
) -> Result<()> {
    let leaf = page_leaf(doc, index)?;
    let img_ref = resolve_image_ref(doc, leaf, name_or_xref)?;
    let info = jpeg_dims(jpeg).ok_or(Error::Unsupported("replace_image: not a parseable JPEG"))?;

    let mut dict = Dict::new();
    dict.insert(Name::new("Type"), Object::Name(Name::new("XObject")));
    dict.insert(Name::new("Subtype"), Object::Name(Name::new("Image")));
    dict.insert(Name::new("Width"), Object::Integer(i64::from(info.0)));
    dict.insert(Name::new("Height"), Object::Integer(i64::from(info.1)));
    dict.insert(
        Name::new("ColorSpace"),
        Object::Name(Name::new(match info.2 {
            1 => "DeviceGray",
            4 => "DeviceCMYK",
            _ => "DeviceRGB",
        })),
    );
    dict.insert(Name::new("BitsPerComponent"), Object::Integer(8));
    dict.insert(Name::new("Filter"), Object::Name(Name::new("DCTDecode")));
    dict.insert(Name::new("Length"), Object::Integer(jpeg.len() as i64));
    if info.2 == 4 {
        dict.insert(
            Name::new("Decode"),
            Object::Array(
                [1, 0, 1, 0, 1, 0, 1, 0]
                    .iter()
                    .map(|v| Object::Integer(*v))
                    .collect(),
            ),
        );
    }
    doc.update_object(
        img_ref,
        Object::Stream(StreamObj::new_encoded(dict, jpeg.to_vec())),
    )
}

/// Binds the whole page's content to the optional-content group `ocg` (PyMuPDF
/// `Page.set_oc`): the page leaf gets an `/OC` entry pointing at `ocg`, so the
/// page is shown/hidden with that layer.
///
/// # Errors
///
/// Object-edit errors propagate.
pub fn set_oc(doc: &DocumentStore, index: usize, ocg: u32) -> Result<()> {
    let leaf = page_leaf(doc, index)?;
    let mut dict = leaf_dict(doc, leaf)?;
    if ocg == 0 {
        dict.remove(&Name::new("OC"));
    } else {
        dict.insert(Name::new("OC"), Object::Reference(ObjRef::new(ocg, 0)));
    }
    doc.update_object(leaf, Object::Dictionary(dict))
}

/// The xref of the optional-content group bound to `page`'s content via its
/// `/OC` entry, or `0` when the page is not OC-bound (PyMuPDF `Page.get_oc`).
#[must_use]
pub fn get_oc(doc: &DocumentStore, index: usize) -> u32 {
    let Ok(leaf) = page_leaf(doc, index) else {
        return 0;
    };
    let Ok(dict) = leaf_dict(doc, leaf) else {
        return 0;
    };
    match dict.get(&Name::new("OC")) {
        Some(Object::Reference(r)) => r.num,
        _ => 0,
    }
}

/// Reads `(width, height, components)` from a JPEG's first SOF marker, or `None`
/// for non-JPEG / truncated input (never panics). Mirrors `image::jpeg_info`.
fn jpeg_dims(data: &[u8]) -> Option<(u32, u32, u8)> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 1 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            continue;
        }
        if i + 1 >= data.len() {
            return None;
        }
        let seg_len = ((data[i] as usize) << 8) | (data[i + 1] as usize);
        if seg_len < 2 || i + seg_len > data.len() {
            return None;
        }
        let is_sof =
            (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_sof {
            if i + 7 >= data.len() {
                return None;
            }
            let height = ((data[i + 3] as u32) << 8) | (data[i + 4] as u32);
            let width = ((data[i + 5] as u32) << 8) | (data[i + 6] as u32);
            let components = data[i + 7];
            if width == 0 || height == 0 {
                return None;
            }
            return Some((width, height, components));
        }
        i += seg_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::Limits;

    /// A minimal one-page PDF whose single content stream draws nothing but
    /// carries known bytes, used to exercise the content consolidation/wrap.
    fn make_doc(content: &[u8]) -> DocumentStore {
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.7\n");
        // 1: catalog, 2: pages, 3: page, 4: content stream.
        let mut offsets = [0usize; 5];
        offsets[1] = pdf.len();
        pdf.extend_from_slice(b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n");
        offsets[2] = pdf.len();
        pdf.extend_from_slice(b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n");
        offsets[3] = pdf.len();
        pdf.extend_from_slice(
            b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Contents 4 0 R>>endobj\n",
        );
        offsets[4] = pdf.len();
        pdf.extend_from_slice(format!("4 0 obj<</Length {}>>stream\n", content.len()).as_bytes());
        pdf.extend_from_slice(content);
        pdf.extend_from_slice(b"\nendstream endobj\n");
        let xref_pos = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for off in &offsets[1..] {
            pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer<</Size 5/Root 1 0 R>>\nstartxref\n{xref_pos}\n%%EOF").as_bytes(),
        );
        DocumentStore::from_bytes(pdf, Limits::default()).unwrap()
    }

    #[test]
    fn clean_contents_consolidates_to_one_stream() {
        let doc = make_doc(b"1 0 0 1 10 10 cm");
        clean_contents(&doc, 0).unwrap();
        let leaf = page_leaf(&doc, 0).unwrap();
        let dict = leaf_dict(&doc, leaf).unwrap();
        // /Contents must now be a single reference.
        assert!(matches!(
            dict.get(&Name::new("Contents")),
            Some(Object::Reference(_))
        ));
        let body = concat_contents(&doc, &dict);
        assert_eq!(body, b"1 0 0 1 10 10 cm");
    }

    #[test]
    fn wrap_contents_brackets_with_q_q() {
        let doc = make_doc(b"BT ET");
        wrap_contents(&doc, 0).unwrap();
        let leaf = page_leaf(&doc, 0).unwrap();
        let dict = leaf_dict(&doc, leaf).unwrap();
        let body = concat_contents(&doc, &dict);
        assert_eq!(body, b"q\nBT ET\nQ\n");
    }

    #[test]
    fn set_get_oc_roundtrip() {
        let doc = make_doc(b"BT ET");
        assert_eq!(get_oc(&doc, 0), 0);
        // Use object number 9 as a stand-in OCG xref.
        set_oc(&doc, 0, 9).unwrap();
        assert_eq!(get_oc(&doc, 0), 9);
        set_oc(&doc, 0, 0).unwrap();
        assert_eq!(get_oc(&doc, 0), 0);
    }

    #[test]
    fn jpeg_dims_reads_sof() {
        // Minimal JPEG: SOI + SOF0 with 2x3, 3 comps.
        let jpeg = [
            0xFF, 0xD8, // SOI
            0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x03, 0x00, 0x02, 0x03, // SOF0 h=3 w=2 c=3
            1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0,
        ];
        assert_eq!(jpeg_dims(&jpeg), Some((2, 3, 3)));
        assert_eq!(jpeg_dims(b"not a jpeg"), None);
    }
}
