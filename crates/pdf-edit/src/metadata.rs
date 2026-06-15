//! Document metadata write — `/Info` dict + XMP `/Metadata` stream (PRD §8.9).
//!
//! `set_metadata` writes the standard `/Info` text fields; missing/empty values
//! remove the key. The `/Info` reference is wired into the trailer via
//! [`DocumentStore::set_trailer_key`] so it survives a full save even when the
//! original document had no `/Info`. `get/set_xml_metadata` read/replace the
//! catalog `/Metadata` XMP stream.

use pdf_core::object::Name;
use pdf_core::{Dict, DocumentStore, ObjRef, Object, PdfString, StreamData, StreamObj, StringKind};

/// The PyMuPDF `/Info` text keys that `set_metadata` honors, mapped to their PDF
/// dictionary names.
const INFO_KEYS: &[(&str, &str)] = &[
    ("title", "Title"),
    ("author", "Author"),
    ("subject", "Subject"),
    ("keywords", "Keywords"),
    ("creator", "Creator"),
    ("producer", "Producer"),
    ("creationDate", "CreationDate"),
    ("modDate", "ModDate"),
    ("trapped", "Trapped"),
];

/// Writes the `/Info` dictionary from `(pymupdf_key, value)` pairs (PRD §8.9).
///
/// A present non-empty value sets the matching `/Info` key; an empty value
/// removes it. The read-only `format`/`encryption` keys are ignored. The `/Info`
/// reference is created if absent and registered as a trailer key.
///
/// # Errors
///
/// Propagates [`pdf_core::Error`] from the object-edit / trailer-set path.
pub fn set_metadata(doc: &DocumentStore, fields: &[(String, String)]) -> pdf_core::Result<()> {
    // Start from the existing /Info dict (so unmentioned keys are preserved).
    let info_ref = doc.effective_trailer_ref("Info");
    let mut info: Dict = info_ref
        .and_then(|r| doc.resolve(r).ok())
        .and_then(|o| o.as_dict().cloned())
        .unwrap_or_default();

    for (key, pdf_key) in INFO_KEYS {
        if let Some((_, value)) = fields.iter().find(|(k, _)| k == key) {
            let name = Name::new(*pdf_key);
            if value.is_empty() {
                info.remove(&name);
            } else {
                info.insert(name, Object::String(encode_text_string(value)));
            }
        }
    }

    match info_ref {
        Some(r) => doc.update_object(r, Object::Dictionary(info))?,
        None => {
            let r = doc.add_object(Object::Dictionary(info))?;
            doc.set_trailer_key("Info", Object::Reference(r))?;
        }
    }
    Ok(())
}

/// Reads the catalog `/Metadata` XMP stream as a UTF-8 string, or `None` when
/// absent (PRD §8.9).
#[must_use]
pub fn get_xml_metadata(doc: &DocumentStore) -> Option<String> {
    let catalog = catalog_dict(doc)?;
    let meta = catalog.get(&Name::new("Metadata"))?;
    let meta = match meta {
        Object::Reference(r) => doc.resolve(*r).ok()?,
        other => std::sync::Arc::new(other.clone()),
    };
    let stream = meta.as_stream()?;
    let bytes = doc.decode_stream(stream).ok()?.into_decoded().ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Creates or replaces the catalog `/Metadata` XMP stream with `xml` (PRD §8.9).
///
/// # Errors
///
/// Propagates [`pdf_core::Error`] from the object-edit path, or
/// [`pdf_core::Error::InvalidArgument`] if the document has no catalog.
pub fn set_xml_metadata(doc: &DocumentStore, xml: &str) -> pdf_core::Result<()> {
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

    let mut sdict = Dict::new();
    sdict.insert(Name::new("Type"), Object::Name(Name::new("Metadata")));
    sdict.insert(Name::new("Subtype"), Object::Name(Name::new("XML")));
    let body = xml.as_bytes().to_vec();
    sdict.insert(Name::new("Length"), Object::Integer(body.len() as i64));
    let stream = Object::Stream(StreamObj {
        dict: sdict,
        data: StreamData::Decoded(body.into()),
    });

    match catalog.get(&Name::new("Metadata")) {
        Some(Object::Reference(r)) => {
            let r = *r;
            doc.update_object(r, stream)?;
        }
        _ => {
            let r = doc.add_object(stream)?;
            catalog.insert(Name::new("Metadata"), Object::Reference(r));
            doc.update_object(root, Object::Dictionary(catalog))?;
        }
    }
    Ok(())
}

/// Removes the catalog `/Metadata` XMP stream (PyMuPDF `del_xml_metadata`):
/// drops the `/Metadata` catalog key and frees the stream object. A no-op when
/// the document has no XMP metadata.
///
/// # Errors
///
/// Propagates [`pdf_core::Error`] from the object-edit path, or
/// [`pdf_core::Error::InvalidArgument`] if the document has no catalog.
pub fn del_xml_metadata(doc: &DocumentStore) -> pdf_core::Result<()> {
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
    let key = Name::new("Metadata");
    if let Some(Object::Reference(r)) = catalog.get(&key) {
        let r = *r;
        doc.delete_object(r)?;
    }
    if catalog.remove(&key).is_some() {
        doc.update_object(root, Object::Dictionary(catalog))?;
    }
    Ok(())
}

/// Encodes a text string for `/Info`: ASCII → literal PDFDocEncoding; otherwise
/// UTF-16BE with the `FE FF` BOM (PRD §8.9 string encodings).
fn encode_text_string(s: &str) -> PdfString {
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

fn catalog_dict(doc: &DocumentStore) -> Option<Dict> {
    let root: ObjRef = doc.root()?;
    doc.resolve(root).ok()?.as_dict().cloned()
}
