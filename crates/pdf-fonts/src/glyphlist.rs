//! Glyph-name → Unicode resolution (PRD §8.5).
//!
//! Two layers, in priority order:
//!
//! 1. The **Adobe Glyph List** (`data/glyphlist.txt`, BSD-3-Clause — see
//!    `data/PROVENANCE.md`): an exact `name → Unicode` table. Parsed once and
//!    cached in a [`OnceLock`].
//! 2. The **algorithmic conventions** (ISO 32000-1 §9.6.6.1 / the AGL spec):
//!    `uniXXXX`, `uXXXXXX`, underscore-joined ligatures, and `.`-suffix
//!    stripping. `cidNN`/`gNN`/`.notdef` and other unresolvable forms yield
//!    `None`.
//!
//! All mappings produce a [`SmolStr`] because a glyph name can map to **more
//! than one** Unicode scalar (the AGL has multi-value entries; underscore
//! ligatures decompose component-by-component).

use std::collections::HashMap;
use std::sync::OnceLock;

use smol_str::SmolStr;

/// The Adobe Glyph List, embedded verbatim (license header retained — this
/// satisfies the BSD-3-Clause source-retention clause; see `data/NOTICE`).
const AGL_TXT: &str = include_str!("../data/glyphlist.txt");

/// The Adobe ZapfDingbats glyph list (`aNN` Dingbat names → Dingbats-block
/// Unicode), same BSD-3-Clause Adobe source/file as the AGL. The `aNN` names do
/// not appear in the AGL, so this is a non-overlapping fallback table.
const ZAPF_TXT: &str = include_str!("../data/zapfdingbats.txt");

/// Parses an AGL-format `name;HHHH[ HHHH …]` table into a name→Unicode map.
fn parse_table(txt: &'static str) -> HashMap<&'static str, SmolStr> {
    let mut map = HashMap::new();
    for line in txt.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, codes)) = line.split_once(';') else {
            continue;
        };
        let mut s = String::new();
        for hex in codes.split_whitespace() {
            if let Some(c) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                s.push(c);
            }
        }
        if !s.is_empty() {
            map.insert(name, SmolStr::new(&s));
        }
    }
    map
}

/// Parsed AGL: glyph name → its Unicode string (may be >1 scalar).
fn agl() -> &'static HashMap<&'static str, SmolStr> {
    static TABLE: OnceLock<HashMap<&'static str, SmolStr>> = OnceLock::new();
    TABLE.get_or_init(|| parse_table(AGL_TXT))
}

/// Parsed ZapfDingbats glyph list (`aNN` → Dingbats Unicode).
fn zapf() -> &'static HashMap<&'static str, SmolStr> {
    static TABLE: OnceLock<HashMap<&'static str, SmolStr>> = OnceLock::new();
    TABLE.get_or_init(|| parse_table(ZAPF_TXT))
}

/// Reverse AGL: Unicode scalar → its canonical AGL glyph name. Built once from
/// the single-scalar AGL entries (multi-scalar / ligature entries are skipped).
/// When several names map to one scalar, the first encountered is kept stable by
/// preferring the shortest name (then lexically smallest) — this picks the
/// canonical short form (`A` over `Alphatonos`-like collisions never occur, but
/// duplicate codepoints in the AGL are resolved deterministically).
fn reverse_agl() -> &'static HashMap<u32, &'static str> {
    static TABLE: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map: HashMap<u32, &'static str> = HashMap::new();
        for (&name, s) in agl() {
            let mut chars = s.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                continue; // skip multi-scalar entries
            };
            let cp = c as u32;
            match map.get(&cp) {
                Some(existing) if (existing.len(), *existing) <= (name.len(), name) => {}
                _ => {
                    map.insert(cp, name);
                }
            }
        }
        map
    })
}

/// Resolves a Unicode scalar `cp` to its AGL glyph name (PyMuPDF
/// `unicode_to_glyph_name`). Falls back to the algorithmic `uniXXXX` form for
/// scalars with no AGL name. Returns `None` only for non-scalar inputs.
#[must_use]
pub fn unicode_to_glyph_name(cp: u32) -> Option<SmolStr> {
    char::from_u32(cp)?;
    if let Some(name) = reverse_agl().get(&cp) {
        return Some(SmolStr::new(name));
    }
    // No AGL name → the canonical algorithmic form.
    Some(SmolStr::new(format!("uni{cp:04X}")))
}

/// Resolves a glyph name to its Unicode string, applying the AGL then the
/// algorithmic conventions. Returns `None` for names that have no defined
/// Unicode meaning (`.notdef`, `cidNN`, `gNN`, unknown names).
#[must_use]
pub fn glyph_name_to_unicode(name: &str) -> Option<SmolStr> {
    if name.is_empty() {
        return None;
    }

    // 1. Exact AGL hit, then the ZapfDingbats `aNN` names.
    if let Some(s) = agl().get(name) {
        return Some(s.clone());
    }
    if let Some(s) = zapf().get(name) {
        return Some(s.clone());
    }

    // `.notdef` and any glyph whose base (before the first `.`) is `notdef`
    // are explicitly unresolved.
    let base = name.split('.').next().unwrap_or(name);
    if base.is_empty() || base == "notdef" {
        return None;
    }

    // 2. Underscore-joined ligature: resolve each component and concatenate.
    //    (`f_f_i` → U+0066 U+0066 U+0069). Only when there really is a `_`.
    if base.contains('_') {
        let mut out = String::new();
        let mut any = false;
        for part in base.split('_') {
            if part.is_empty() {
                continue;
            }
            match component_to_unicode(part) {
                Some(s) => {
                    out.push_str(&s);
                    any = true;
                }
                // A ligature with an unresolvable component is unresolved as a
                // whole (we cannot fabricate a partial glyph mapping).
                None => return None,
            }
        }
        return if any { Some(SmolStr::new(&out)) } else { None };
    }

    // 3. Single component (the `.`-stripped base): AGL again, then uni/u rules.
    component_to_unicode(base)
}

/// Resolves a single ligature component (no `_`), trying the AGL, then the
/// `uniXXXX` / `uXXXXXX` algorithmic forms.
fn component_to_unicode(part: &str) -> Option<SmolStr> {
    if let Some(s) = agl().get(part) {
        return Some(s.clone());
    }
    uni_rule(part)
}

/// The `uniXXXX…` / `uXXXXXX` algorithmic conventions (ISO 32000-1 §9.6.6.1).
///
/// - `uni` followed by one or more groups of exactly 4 hex digits → the
///   concatenation of those BMP scalars (no surrogate or noncharacter halves).
/// - `u` followed by 4–6 hex digits → a single scalar.
fn uni_rule(name: &str) -> Option<SmolStr> {
    if let Some(rest) = name.strip_prefix("uni") {
        if rest.len() >= 4
            && rest.len().is_multiple_of(4)
            && rest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            let mut out = String::new();
            for chunk in rest.as_bytes().chunks(4) {
                let hex = std::str::from_utf8(chunk).ok()?;
                let cp = u32::from_str_radix(hex, 16).ok()?;
                // `uniXXXX` is BMP-only; a lone surrogate is not a scalar.
                let c = char::from_u32(cp)?;
                out.push(c);
            }
            return Some(SmolStr::new(&out));
        }
        return None;
    }
    if let Some(rest) = name.strip_prefix('u') {
        if (4..=6).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_hexdigit()) {
            let cp = u32::from_str_radix(rest, 16).ok()?;
            let c = char::from_u32(cp)?;
            return Some(SmolStr::new(c.to_string()));
        }
    }
    None
}
