//! Exact-name page font registration and reuse by text insertion.

use pdf_core::error::{Error, Result};
use pdf_core::object::{Dict, Name, ObjRef, Object};
use pdf_core::{pagetree, DocumentStore};
use pdf_fonts::widths::normalize_standard_font;

use crate::content::PageContent;
use crate::fontfile::EmbeddedFont;
use crate::text::{base14_font_object, checked_base14};

/// Supported registration options. Nondefault modes fail before creating objects.
#[derive(Default)]
pub struct FontRegistrationOptions<'a> {
    /// Raw standalone TrueType program, when registering a non-Core14 name.
    pub program: Option<&'a [u8]>,
    /// Simple-font mode (unsupported for new registrations).
    pub set_simple: bool,
    /// Writing mode; only horizontal zero is supported.
    pub wmode: i32,
    /// Encoding selector; only the default zero is supported.
    pub encoding: i32,
}

/// Whether the name denotes a supported Core14 alias.
#[must_use]
pub fn is_core14(name: &str) -> bool {
    checked_base14(name).is_some()
}

fn valid_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name
            .chars()
            .any(|ch| ch.is_control() || " ()<>[]{}/%#".contains(ch))
    {
        return Err(Error::Unsupported("insert_font: invalid resource name"));
    }
    Ok(())
}

fn font_entry(pc: &PageContent<'_>, name: &str) -> Result<Option<Object>> {
    let Some(resources) = pagetree::resources(pc.doc, pc.leaf) else {
        return Ok(None);
    };
    let Some(fonts) = pc.doc.resolve_dict_key(&resources, &Name::new("Font"))? else {
        return Ok(None);
    };
    Ok(fonts
        .as_dict()
        .and_then(|fonts| fonts.get(&Name::new(name)))
        .cloned())
}

fn resolve_dict(doc: &DocumentStore, value: &Object) -> Result<Dict> {
    let resolved = match value {
        Object::Reference(reference) => doc.resolve(*reference)?,
        value => std::sync::Arc::new(value.clone()),
    };
    resolved.as_dict().cloned().ok_or(Error::Unsupported(
        "insert_font: font resource is not a dictionary",
    ))
}

fn install(pc: &PageContent<'_>, name: &str, font: ObjRef) -> Result<()> {
    let mut leaf = resolve_dict(pc.doc, &Object::Reference(pc.leaf))?;
    let mut resources = pagetree::resources(pc.doc, pc.leaf).unwrap_or_default();
    let mut fonts = pc
        .doc
        .resolve_dict_key(&resources, &Name::new("Font"))?
        .and_then(|value| value.as_dict().cloned())
        .unwrap_or_default();
    fonts.insert(Name::new(name), Object::Reference(font));
    resources.insert(Name::new("Font"), Object::Dictionary(fonts));
    leaf.insert(Name::new("Resources"), Object::Dictionary(resources));
    pc.doc.update_object(pc.leaf, Object::Dictionary(leaf))
}

/// Returns the existing exact-name font xref, promoting a direct dictionary.
///
/// # Errors
/// Propagates invalid page/resource or document mutation errors.
pub fn existing_font(doc: &DocumentStore, page_index: usize, name: &str) -> Result<Option<u32>> {
    valid_name(name)?;
    let pc = PageContent::new(doc, page_index)?;
    let Some(entry) = font_entry(&pc, name)? else {
        return Ok(None);
    };
    let dictionary = resolve_dict(doc, &entry)?;
    if let Object::Reference(reference) = entry {
        return Ok(Some(reference.num));
    }
    let reference = doc.add_object(Object::Dictionary(dictionary))?;
    install(&pc, name, reference)?;
    Ok(Some(reference.num))
}

fn identical_core14(pc: &PageContent<'_>, expected: &Object) -> Result<Option<ObjRef>> {
    let Some(resources) = pagetree::resources(pc.doc, pc.leaf) else {
        return Ok(None);
    };
    let Some(fonts) = pc.doc.resolve_dict_key(&resources, &Name::new("Font"))? else {
        return Ok(None);
    };
    if let Some(fonts) = fonts.as_dict() {
        for value in fonts.values() {
            if let Object::Reference(reference) = value {
                if pc.doc.resolve(*reference)?.as_ref() == expected {
                    return Ok(Some(*reference));
                }
            }
        }
    }
    Ok(None)
}

/// Registers an exact page resource name without writing page content.
///
/// # Errors
/// Unsupported modes/formats and malformed sources are rejected before allocation.
pub fn insert_font(
    doc: &DocumentStore,
    page_index: usize,
    name: &str,
    options: &FontRegistrationOptions<'_>,
) -> Result<u32> {
    if let Some(xref) = existing_font(doc, page_index, name)? {
        return Ok(xref);
    }
    if options.set_simple || options.wmode != 0 || options.encoding != 0 {
        return Err(Error::Unsupported(
            "insert_font: only default horizontal composite registration is supported",
        ));
    }
    let pc = PageContent::new(doc, page_index)?;
    let font_ref = if let Some(base14) = checked_base14(name) {
        let object = base14_font_object(base14);
        if let Some(reference) = identical_core14(&pc, &object)? {
            reference
        } else {
            doc.add_object(object)?
        }
    } else {
        let program = options
            .program
            .ok_or(Error::Unsupported("insert_font: need font file or buffer"))?;
        let font = EmbeddedFont::for_registration(program)?;
        let used = font.registration_map()?;
        font.write_registered_type0(doc, &used)?
    };
    install(&pc, name, font_ref)?;
    Ok(font_ref.num)
}

pub(crate) enum WritingFont {
    Simple(Box<[f64; 256]>),
    Composite(std::collections::BTreeMap<char, (u16, f64)>),
}

impl WritingFont {
    pub(crate) fn advance(&self, text: &str, size: f64) -> Result<f64> {
        match self {
            Self::Simple(widths) => Ok(crate::text::winansi_bytes(text)
                .iter()
                .map(|&code| widths[usize::from(code)] * size / 1000.0)
                .sum()),
            Self::Composite(codes) => text
                .chars()
                .map(|ch| {
                    codes
                        .get(&ch)
                        .map(|(_, width)| width * size / 1000.0)
                        .ok_or(Error::Unsupported(
                            "insert_text: character is absent from registered font mapping",
                        ))
                })
                .sum(),
        }
    }

    pub(crate) fn encode(&self, text: &str) -> Result<Vec<u8>> {
        match self {
            Self::Simple(_) => {
                let escaped = crate::content::escape_pdf_literal(&crate::text::winansi_bytes(text));
                let mut show = vec![b'('];
                show.extend_from_slice(&escaped);
                show.push(b')');
                Ok(show)
            }
            Self::Composite(codes) => {
                let mut show = String::from("<");
                for ch in text.chars() {
                    let (code, _) = codes.get(&ch).ok_or(Error::Unsupported(
                        "insert_text: character is absent from registered font mapping",
                    ))?;
                    show.push_str(&format!("{code:04X}"));
                }
                show.push('>');
                Ok(show.into_bytes())
            }
        }
    }
}

/// Resolves actual PDF encoding/width resources, including after save/reopen.
pub(crate) fn writing_font(pc: &PageContent<'_>, name: &str) -> Result<Option<WritingFont>> {
    let Some(entry) = font_entry(pc, name)? else {
        return Ok(None);
    };
    valid_name(name)?;
    let font = resolve_dict(pc.doc, &entry)?;
    let subtype = font
        .get(&Name::new("Subtype"))
        .and_then(Object::as_name)
        .and_then(|n| n.as_str());
    if subtype == Some("Type1") {
        let _base = font
            .get(&Name::new("BaseFont"))
            .and_then(Object::as_name)
            .and_then(|name| name.as_str())
            .and_then(normalize_standard_font)
            .ok_or(Error::Unsupported(
                "insert_text: named Type1 font is not supported",
            ))?;
        let encoding = pc.doc.resolve_dict_key(&font, &Name::new("Encoding"))?;
        if encoding.as_ref().is_some_and(|e| {
            !e.is_null() && e.as_name().and_then(|n| n.as_str()) != Some("WinAnsiEncoding")
        }) {
            return Err(Error::Unsupported(
                "insert_text: named custom encoding is not supported",
            ));
        }
        let mapper = pdf_fonts::FontMapper::from_dict(&font, pc.doc);
        let widths = std::array::from_fn(|code| mapper.width(code as u32));
        return Ok(Some(WritingFont::Simple(Box::new(widths))));
    }
    if subtype != Some("Type0")
        || font
            .get(&Name::new("Encoding"))
            .and_then(Object::as_name)
            .and_then(|n| n.as_str())
            != Some("Identity-H")
    {
        return Err(Error::Unsupported(
            "insert_text: named font requires unsupported encoding",
        ));
    }
    let descendants = pc
        .doc
        .resolve_dict_key(&font, &Name::new("DescendantFonts"))?
        .ok_or(Error::Unsupported(
            "insert_text: named font has no descendant",
        ))?;
    let descendant = descendants
        .as_array()
        .and_then(|a| a.first())
        .ok_or(Error::Unsupported(
            "insert_text: named font has no descendant",
        ))?;
    let cid = resolve_dict(pc.doc, descendant)?;
    if cid
        .get(&Name::new("Subtype"))
        .and_then(Object::as_name)
        .and_then(|n| n.as_str())
        != Some("CIDFontType2")
    {
        return Err(Error::Unsupported(
            "insert_text: named font requires unsupported glyph mapping",
        ));
    }
    let descriptor = pc
        .doc
        .resolve_dict_key(&cid, &Name::new("FontDescriptor"))?
        .ok_or(Error::Unsupported(
            "insert_text: named font has no descriptor",
        ))?;
    let descriptor = descriptor
        .as_dict()
        .ok_or(Error::Unsupported("insert_text: invalid descriptor"))?;
    let program = pc
        .doc
        .resolve_dict_key(descriptor, &Name::new("FontFile2"))?
        .ok_or(Error::Unsupported(
            "insert_text: named font has no TrueType program",
        ))?;
    let stream = program
        .as_stream()
        .ok_or(Error::Unsupported("insert_text: invalid font program"))?;
    let bytes = pc.doc.decode_stream(stream)?.into_decoded()?;
    let parsed = EmbeddedFont::for_registration(&bytes)?;
    let unicode = pc
        .doc
        .resolve_dict_key(&font, &Name::new("ToUnicode"))?
        .ok_or(Error::Unsupported(
            "insert_text: named font has no Unicode mapping",
        ))?;
    let unicode = unicode
        .as_stream()
        .ok_or(Error::Unsupported("insert_text: invalid Unicode mapping"))?;
    let unicode = pc.doc.decode_stream(unicode)?.into_decoded()?;
    let cmap = pdf_fonts::cmap::CMap::parse(&unicode, &mut |_| None);
    let mapper = pdf_fonts::FontMapper::from_dict(&font, pc.doc);
    let mut codes = std::collections::BTreeMap::new();
    for (code, text) in cmap.unicode_mappings(u32::from(u16::MAX)) {
        let mut chars = text.chars();
        if let Some(ch) = chars.next() {
            if chars.next().is_none()
                && ch != '\0'
                && mapper.gid(code) != 0
                && mapper.gid(code) < u32::from(parsed.num_glyphs())
            {
                codes.entry(ch).or_insert((code as u16, mapper.width(code)));
            }
        }
    }
    if codes.is_empty() {
        return Err(Error::Unsupported(
            "insert_text: no supported Unicode codes in named font",
        ));
    }
    Ok(Some(WritingFont::Composite(codes)))
}
