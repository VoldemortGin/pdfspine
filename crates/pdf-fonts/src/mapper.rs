//! The [`FontMapper`] — the public mapping surface (PRD §8.5).
//!
//! Built from a resolved font dictionary + a `&DocumentStore`, it answers the
//! three questions the M2b text interpreter needs:
//!
//! - [`FontMapper::iter_codes`] — split a show-string into `(code, n_bytes)`
//!   pairs (1 byte for simple fonts; codespace-driven for Type0).
//! - [`FontMapper::to_unicode`] — `code → Unicode` (the resolution ladder:
//!   `/ToUnicode` overrides; else encoding + AGL for simple fonts, or the CID
//!   CMap path for Type0).
//! - [`FontMapper::width`] — `code → advance` in 1000-unit text space.
//!
//! Everything is computed once at construction and stored; the accessors are
//! pure table lookups and never touch the document or panic.

use pdf_core::{Dict, DocumentStore, Name, Object};
use smol_str::SmolStr;

use crate::cmap::{CMap, CodespaceRange};
use crate::encodings::BaseEncoding;
use crate::glyphlist::glyph_name_to_unicode;
use crate::predefined::{self, PredefinedKind};
use crate::widths::{self, CidWidths, SimpleWidths};

/// The broad font-program kind (drives the build path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    /// Type1 / MMType1 / TrueType / Type3 — simple, 1 byte per code.
    Simple,
    /// Type0 composite font (CID-keyed descendant).
    Type0,
}

/// How a Type0 font's `/Encoding` maps codes → CIDs.
#[derive(Clone, Debug)]
enum CidEncoding {
    /// Identity: 2-byte codes, `CID == code`.
    Identity,
    /// A parsed CMap (embedded stream). Carries its own codespace.
    CMap(CMap),
    /// A known-but-unbundled predefined CJK CMap (documented gap): 2-byte
    /// codespace assumed, `CID == code` best-effort, `to_unicode` only via a
    /// font `/ToUnicode`.
    UnbundledPredefined,
}

/// The font-mapping façade (PRD §8.5). Cheap to query; built once.
#[derive(Clone, Debug)]
pub struct FontMapper {
    kind: FontKind,
    /// `/ToUnicode` CMap, if present — overrides every other extraction path.
    to_unicode: Option<CMap>,

    // --- simple-font state ---
    /// Per-code glyph name (encoding + `/Differences`), for the simple path.
    glyph_names: Option<Box<[Option<SmolStr>; 256]>>,
    simple_widths: SimpleWidths,
    /// The normalized Core-14 font key (e.g. `"Helvetica"`), set **only** for a
    /// simple base-14 font that has **no** `/Widths` array. When present,
    /// `width(code)` resolves the glyph's standard AFM advance via the glyph
    /// name. `None` whenever a `/Widths` array is authoritative or the font is
    /// not one of the 14 standard fonts.
    core14: Option<&'static str>,

    // --- Type0 state ---
    cid_encoding: Option<CidEncoding>,
    cid_widths: CidWidths,
    /// CIDToGIDMap stream (Identity when `None`): CID → GID, big-endian u16.
    /// Stored for completeness / future rasterization; mapping uses it to
    /// validate a CID is present but width/unicode key on CID directly.
    cid_to_gid: Option<Vec<u16>>,
}

impl FontMapper {
    /// Builds a [`FontMapper`] from a resolved font dict and the document store
    /// (used to follow references to `/Encoding` dicts, `/ToUnicode` streams,
    /// descendant fonts, embedded CMap streams and `/CIDToGIDMap`).
    ///
    /// Never fails: a malformed or unrecognized font yields a best-effort mapper
    /// (empty tables, notdef widths) rather than an error (PRD §8.5).
    #[must_use]
    pub fn from_dict(font: &Dict, doc: &DocumentStore) -> FontMapper {
        let subtype = name_str(font, "Subtype");
        if subtype.as_deref() == Some("Type0") {
            Self::build_type0(font, doc)
        } else {
            Self::build_simple(font, doc)
        }
    }

    /// The font kind.
    #[must_use]
    pub fn kind(&self) -> FontKind {
        self.kind
    }

    // --- simple fonts -----------------------------------------------------

    fn build_simple(font: &Dict, doc: &DocumentStore) -> FontMapper {
        let base_font = name_str(font, "BaseFont").unwrap_or_default();
        let symbolic_builtin = builtin_symbol_encoding(&base_font);

        // 1. Base encoding selection.
        let base = resolve_base_encoding(font, doc, symbolic_builtin);

        // 2. Dense code → glyph name, then apply `/Differences`.
        let mut names: [Option<SmolStr>; 256] =
            std::array::from_fn(|c| base.glyph_name(c as u8).map(SmolStr::new));
        apply_differences(font, doc, &mut names);

        // 3. Widths: `/Widths` + `/FirstChar` + descriptor `/MissingWidth`.
        let has_widths = has_widths_array(font, doc);
        let simple_widths = build_simple_widths(font, doc);

        // 3b. Core-14 fallback: a standard font *without* a `/Widths` array gets
        // its built-in AFM advances (PRD §6.5 #2). `/Widths` stays authoritative.
        let core14 = if has_widths {
            None
        } else {
            widths::normalize_standard_font(&base_font)
        };

        // 4. `/ToUnicode` (overrides on lookup).
        let to_unicode = load_to_unicode(font, doc);

        FontMapper {
            kind: FontKind::Simple,
            to_unicode,
            glyph_names: Some(Box::new(names)),
            simple_widths,
            core14,
            cid_encoding: None,
            cid_widths: CidWidths::default(),
            cid_to_gid: None,
        }
    }

    // --- Type0 / CID fonts ------------------------------------------------

    fn build_type0(font: &Dict, doc: &DocumentStore) -> FontMapper {
        // `/Encoding`: name (Identity / predefined) or embedded CMap stream.
        let cid_encoding = resolve_cid_encoding(font, doc);

        // Descendant CIDFont (single-element array).
        let descendant = resolve_descendant(font, doc);

        // `/W` + `/DW` widths from the descendant.
        let cid_widths = match &descendant {
            Some(d) => build_cid_widths(d, doc),
            None => CidWidths::default(),
        };

        // CIDToGIDMap (Identity or stream) from the descendant.
        let cid_to_gid = descendant.as_ref().and_then(|d| load_cid_to_gid(d, doc));

        let to_unicode = load_to_unicode(font, doc);

        FontMapper {
            kind: FontKind::Type0,
            to_unicode,
            glyph_names: None,
            simple_widths: SimpleWidths::default(),
            core14: None,
            cid_encoding: Some(cid_encoding),
            cid_widths,
            cid_to_gid,
        }
    }

    // --- code iteration ---------------------------------------------------

    /// Splits `bytes` into `(code, n_bytes)` pairs covering the whole input with
    /// no overlap (PRD §8.5). Simple fonts emit one byte per code; Type0 fonts
    /// use the encoding CMap's codespace ranges (Identity → 2 bytes). A trailing
    /// partial code is consumed as a best-effort single unit (never panics).
    #[must_use]
    pub fn iter_codes<'a>(&'a self, bytes: &'a [u8]) -> CodeIter<'a> {
        CodeIter {
            bytes,
            pos: 0,
            codespace: self.codespace(),
            simple: self.kind == FontKind::Simple,
        }
    }

    /// The codespace ranges driving variable-length decode (empty for simple
    /// fonts; Identity / embedded / default-2-byte for Type0).
    fn codespace(&self) -> &[CodespaceRange] {
        match &self.cid_encoding {
            Some(CidEncoding::CMap(c)) if !c.codespace().is_empty() => c.codespace(),
            // Identity / unbundled / empty embedded → caller defaults to 2 bytes.
            _ => &[],
        }
    }

    // --- to_unicode -------------------------------------------------------

    /// Resolves a character code to its Unicode string (PRD §8.5 ladder).
    ///
    /// `/ToUnicode` always wins. Otherwise: simple fonts use encoding + AGL;
    /// Type0 fonts have no second path here (the CID CMap maps to CIDs, not
    /// Unicode) and return `None` — the documented CJK-without-ToUnicode gap.
    #[must_use]
    pub fn to_unicode(&self, code: u32) -> Option<SmolStr> {
        if let Some(tu) = &self.to_unicode {
            if let Some(s) = tu.to_unicode(code) {
                // A bf value of U+0000 is the "no mapping" sentinel some
                // producers emit; treat it as unmapped.
                if !(s.len() == 1 && s.as_bytes()[0] == 0) {
                    return Some(s);
                }
            }
        }
        match self.kind {
            FontKind::Simple => {
                let name = self.glyph_names.as_ref()?.get(code as usize)?.as_ref()?;
                glyph_name_to_unicode(name)
            }
            FontKind::Type0 => None,
        }
    }

    // --- width ------------------------------------------------------------

    /// The advance width of `code` in 1000-unit text space (PRD §8.5). Always
    /// finite and `>= 0`. An unmapped code yields the appropriate default
    /// (`/MissingWidth` / `/DW` / notdef).
    #[must_use]
    pub fn width(&self, code: u32) -> f64 {
        match self.kind {
            FontKind::Simple => {
                // Core-14 AFM advances apply only when no `/Widths` array is
                // present (otherwise `/Widths` is authoritative). Resolve the
                // code's glyph name, then its standard advance; fall back to the
                // `/MissingWidth`/notdef table when either is unavailable.
                if let Some(std_name) = self.core14 {
                    if let Some(name) = self
                        .glyph_names
                        .as_ref()
                        .and_then(|n| n.get(code as usize))
                        .and_then(Option::as_ref)
                    {
                        if let Some(w) = widths::core14_width(std_name, name) {
                            return w;
                        }
                    }
                }
                self.simple_widths.width(code)
            }
            FontKind::Type0 => {
                let cid = self.code_to_cid(code);
                self.cid_widths.width(cid)
            }
        }
    }

    /// Maps a Type0 character code to a CID via the encoding CMap. For Identity
    /// and unbundled-predefined encodings `CID == code`.
    fn code_to_cid(&self, code: u32) -> u32 {
        match &self.cid_encoding {
            Some(CidEncoding::Identity) | Some(CidEncoding::UnbundledPredefined) | None => code,
            Some(CidEncoding::CMap(c)) => c.cid(code).unwrap_or(code),
        }
    }

    /// The CID for a Type0 character code (exposed for the M2b interpreter /
    /// `get_text` CID fallback). For a simple font this is just `code`.
    #[must_use]
    pub fn cid(&self, code: u32) -> u32 {
        match self.kind {
            FontKind::Simple => code,
            FontKind::Type0 => self.code_to_cid(code),
        }
    }

    /// The GID for a Type0 character code, applying `CIDToGIDMap` (Identity when
    /// absent). For a simple font this is `code`. Useful to M6 rasterization.
    #[must_use]
    pub fn gid(&self, code: u32) -> u32 {
        match self.kind {
            FontKind::Simple => code,
            FontKind::Type0 => {
                let cid = self.code_to_cid(code);
                match &self.cid_to_gid {
                    Some(map) => map.get(cid as usize).copied().map(u32::from).unwrap_or(0),
                    None => cid,
                }
            }
        }
    }
}

/// Iterator over `(code, n_bytes)` produced by [`FontMapper::iter_codes`].
pub struct CodeIter<'a> {
    bytes: &'a [u8],
    pos: usize,
    codespace: &'a [CodespaceRange],
    simple: bool,
}

impl Iterator for CodeIter<'_> {
    type Item = (u32, u8);

    fn next(&mut self) -> Option<(u32, u8)> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        if self.simple {
            let code = u32::from(self.bytes[self.pos]);
            self.pos += 1;
            return Some((code, 1));
        }
        // Type0: pick the byte-length from the codespace by matching a prefix;
        // default to 2 bytes (Identity / empty codespace).
        let rest = &self.bytes[self.pos..];
        let n = match_codespace_len(self.codespace, rest);
        let take = n.min(rest.len()).max(1);
        let mut code: u32 = 0;
        for &b in &rest[..take] {
            code = (code << 8) | u32::from(b);
        }
        self.pos += take;
        Some((code, take as u8))
    }
}

/// Chooses the byte length for the next code given the codespace ranges. The
/// default (no/empty codespace) is 2 bytes (Identity-H/V). When ranges exist,
/// the shortest range whose value bracket contains the prefix wins; if none
/// match, fall back to the shortest declared length (tolerant).
fn match_codespace_len(ranges: &[CodespaceRange], rest: &[u8]) -> usize {
    if ranges.is_empty() {
        return 2;
    }
    // Try each declared length (1..=4) shortest-first; a code of length n is
    // valid if its big-endian value lies within some range of that length.
    let mut lengths: Vec<u8> = ranges.iter().map(|r| r.n_bytes).collect();
    lengths.sort_unstable();
    lengths.dedup();
    for &len in &lengths {
        let len = len as usize;
        if len == 0 || len > rest.len() {
            continue;
        }
        let mut v: u32 = 0;
        for &b in &rest[..len] {
            v = (v << 8) | u32::from(b);
        }
        if ranges
            .iter()
            .any(|r| r.n_bytes as usize == len && v >= r.low && v <= r.high)
        {
            return len;
        }
    }
    // No exact bracket match: use the shortest declared length.
    lengths.first().map(|&l| l as usize).unwrap_or(2)
}

// === construction helpers =================================================

fn name_str(dict: &Dict, key: &str) -> Option<String> {
    match dict.get(&Name::new(key)) {
        Some(Object::Name(n)) => n.as_str().map(str::to_owned),
        _ => None,
    }
}

/// Whether a simple font's base-font name selects the Symbol / ZapfDingbats
/// built-in encoding (their `/Encoding` is usually absent).
fn builtin_symbol_encoding(base_font: &str) -> Option<BaseEncoding> {
    let name = base_font.rsplit('+').next().unwrap_or(base_font);
    let lower = name.to_ascii_lowercase();
    if lower.contains("zapf") || lower.contains("dingbat") {
        Some(BaseEncoding::ZapfDingbats)
    } else if lower.contains("symbol") {
        Some(BaseEncoding::Symbol)
    } else {
        None
    }
}

/// Resolves the simple-font base encoding from `/Encoding` (a name or a dict
/// with `/BaseEncoding`), falling back to the built-in Symbol/ZapfDingbats
/// encoding, then StandardEncoding.
fn resolve_base_encoding(
    font: &Dict,
    doc: &DocumentStore,
    symbolic_builtin: Option<BaseEncoding>,
) -> BaseEncoding {
    let enc = doc
        .resolve_dict_key(font, &Name::new("Encoding"))
        .ok()
        .flatten();
    match enc.as_deref() {
        Some(Object::Name(n)) => BaseEncoding::from_name(n.as_bytes())
            .or(symbolic_builtin)
            .unwrap_or(BaseEncoding::Standard),
        Some(Object::Dictionary(d)) => {
            // `/BaseEncoding` inside the dict, else built-in, else Standard.
            match d.get(&Name::new("BaseEncoding")) {
                Some(Object::Name(n)) => BaseEncoding::from_name(n.as_bytes())
                    .or(symbolic_builtin)
                    .unwrap_or(BaseEncoding::Standard),
                _ => symbolic_builtin.unwrap_or(BaseEncoding::Standard),
            }
        }
        _ => symbolic_builtin.unwrap_or(BaseEncoding::Standard),
    }
}

/// Applies an `/Encoding` dict's `/Differences` array (code → glyph name) over
/// the dense name table. The array is `[ code name name … code name … ]`.
fn apply_differences(font: &Dict, doc: &DocumentStore, names: &mut [Option<SmolStr>; 256]) {
    let Some(enc) = doc
        .resolve_dict_key(font, &Name::new("Encoding"))
        .ok()
        .flatten()
    else {
        return;
    };
    let Some(dict) = enc.as_dict() else { return };
    let Some(diffs) = dict
        .get(&Name::new("Differences"))
        .and_then(Object::as_array)
    else {
        return;
    };
    let mut current: i64 = 0;
    for item in diffs {
        match item {
            Object::Integer(i) => current = *i,
            Object::Name(n) => {
                if (0..256).contains(&current) {
                    if let Some(s) = n.as_str() {
                        names[current as usize] = Some(SmolStr::new(s));
                    }
                }
                current += 1;
            }
            _ => {}
        }
    }
}

/// Whether the font dict carries a usable `/Widths` *array* (after resolving an
/// indirect reference). Drives the Core-14 fallback: a `/Widths` array is always
/// authoritative, so the built-in AFM metrics apply only in its absence.
fn has_widths_array(font: &Dict, doc: &DocumentStore) -> bool {
    doc.resolve_dict_key(font, &Name::new("Widths"))
        .ok()
        .flatten()
        .as_deref()
        .and_then(Object::as_array)
        .is_some()
}

/// Builds the simple-font width table from `/Widths` + `/FirstChar` and the
/// descriptor `/MissingWidth`.
fn build_simple_widths(font: &Dict, doc: &DocumentStore) -> SimpleWidths {
    let first_char = font
        .get(&Name::new("FirstChar"))
        .and_then(Object::as_i64)
        .filter(|v| *v >= 0)
        .map(|v| v as u32)
        .unwrap_or(0);

    let missing = descriptor_missing_width(font, doc);

    let widths_obj = doc
        .resolve_dict_key(font, &Name::new("Widths"))
        .ok()
        .flatten();
    match widths_obj.as_deref().and_then(Object::as_array) {
        Some(arr) => {
            // Resolve any indirect references inside the array.
            let resolved: Vec<Object> = arr
                .iter()
                .map(|o| match o {
                    Object::Reference(r) => doc
                        .resolve(*r)
                        .map(|a| (*a).clone())
                        .unwrap_or(Object::Null),
                    other => other.clone(),
                })
                .collect();
            SimpleWidths::new(first_char, &resolved, missing)
        }
        // No `/Widths`: a MissingWidth-only table. For a base-14 font the
        // Core-14 AFM advances are layered on top in `width()` (see `core14`).
        None => SimpleWidths::new(first_char, &[], missing),
    }
}

/// Reads `/MissingWidth` from the font's `/FontDescriptor` (0 when absent).
fn descriptor_missing_width(font: &Dict, doc: &DocumentStore) -> f64 {
    let Some(desc) = doc
        .resolve_dict_key(font, &Name::new("FontDescriptor"))
        .ok()
        .flatten()
    else {
        return 0.0;
    };
    let Some(d) = desc.as_dict() else { return 0.0 };
    d.get(&Name::new("MissingWidth"))
        .and_then(Object::as_f64)
        .map(widths::sanitize)
        .unwrap_or(0.0)
}

/// Loads and parses a `/ToUnicode` CMap stream, if present.
fn load_to_unicode(font: &Dict, doc: &DocumentStore) -> Option<CMap> {
    let tu = doc
        .resolve_dict_key(font, &Name::new("ToUnicode"))
        .ok()
        .flatten()?;
    let stream = tu.as_stream()?;
    let bytes = doc.decode_stream(stream).ok()?.into_decoded().ok()?;
    let mut no_use = |_: &[u8]| None;
    Some(CMap::parse(&bytes, &mut no_use))
}

/// Resolves a Type0 `/Encoding` into a [`CidEncoding`].
fn resolve_cid_encoding(font: &Dict, doc: &DocumentStore) -> CidEncoding {
    let Some(enc) = doc
        .resolve_dict_key(font, &Name::new("Encoding"))
        .ok()
        .flatten()
    else {
        // Absent `/Encoding` on a Type0 font is malformed; assume Identity.
        return CidEncoding::Identity;
    };
    match enc.as_ref() {
        Object::Name(n) => match predefined::classify(n.as_bytes()) {
            PredefinedKind::Identity => CidEncoding::Identity,
            PredefinedKind::KnownUnbundled => CidEncoding::UnbundledPredefined,
            // Unknown name: best-effort Identity (most are 2-byte).
            PredefinedKind::Unknown => CidEncoding::Identity,
        },
        Object::Stream(s) => {
            if let Ok(decoded) = doc.decode_stream(s).and_then(|o| o.into_decoded()) {
                let mut use_resolver = |name: &[u8]| -> Option<CMap> {
                    // A `usecmap` to Identity is common; resolve it inline.
                    match predefined::classify(name) {
                        PredefinedKind::Identity => Some(identity_cmap()),
                        _ => None,
                    }
                };
                CidEncoding::CMap(CMap::parse(&decoded, &mut use_resolver))
            } else {
                CidEncoding::Identity
            }
        }
        _ => CidEncoding::Identity,
    }
}

/// A synthetic Identity-H CMap (2-byte codespace, `CID == code`) for `usecmap`.
fn identity_cmap() -> CMap {
    // `<0000> <ffff>` codespace; `<0000> <ffff> 0` cidrange.
    let program = b"begincodespacerange <0000> <ffff> endcodespacerange \
                    1 begincidrange <0000> <ffff> 0 endcidrange";
    let mut no_use = |_: &[u8]| None;
    CMap::parse(program, &mut no_use)
}

/// Resolves the single descendant CIDFont dict from `/DescendantFonts`.
fn resolve_descendant(font: &Dict, doc: &DocumentStore) -> Option<Dict> {
    let df = doc
        .resolve_dict_key(font, &Name::new("DescendantFonts"))
        .ok()
        .flatten()?;
    let arr = df.as_array()?;
    let first = arr.first()?;
    let resolved = match first {
        Object::Reference(r) => doc.resolve(*r).ok()?,
        other => std::sync::Arc::new(other.clone()),
    };
    resolved.as_dict().cloned()
}

/// Builds the CID width table from the descendant `/W` + `/DW`.
fn build_cid_widths(descendant: &Dict, doc: &DocumentStore) -> CidWidths {
    let dw = descendant.get(&Name::new("DW")).and_then(Object::as_f64);
    let w = doc
        .resolve_dict_key(descendant, &Name::new("W"))
        .ok()
        .flatten();
    match w.as_deref().and_then(Object::as_array) {
        Some(arr) => {
            let resolved: Vec<Object> = arr
                .iter()
                .map(|o| match o {
                    Object::Reference(r) => doc
                        .resolve(*r)
                        .map(|a| (*a).clone())
                        .unwrap_or(Object::Null),
                    other => other.clone(),
                })
                .collect();
            CidWidths::new(&resolved, dw)
        }
        None => CidWidths::new(&[], dw),
    }
}

/// Loads a `/CIDToGIDMap` stream into a CID→GID table; `None` for Identity (the
/// default) or a `/Identity` name.
fn load_cid_to_gid(descendant: &Dict, doc: &DocumentStore) -> Option<Vec<u16>> {
    let val = doc
        .resolve_dict_key(descendant, &Name::new("CIDToGIDMap"))
        .ok()
        .flatten()?;
    match val.as_ref() {
        Object::Name(_) => None, // `/Identity`
        Object::Stream(s) => {
            let bytes = doc.decode_stream(s).ok()?.into_decoded().ok()?;
            let map: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| (u16::from(c[0]) << 8) | u16::from(c[1]))
                .collect();
            Some(map)
        }
        _ => None,
    }
}
