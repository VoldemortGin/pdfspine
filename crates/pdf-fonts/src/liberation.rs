//! Bundled **Liberation** font programs — permissive (SIL OFL 1.1) substitute
//! glyph outlines for the non-embedded standard-14 text families.
//!
//! A PDF may name a standard-14 font (Helvetica / Times / Courier, or the
//! metric-compatible aliases Arial / Times New Roman / Courier New) **without**
//! embedding a `/FontFile*` program. The mapping layer already supplies the
//! advance-width metrics ([`crate::std_widths`]); the only missing piece for
//! rendering is the glyph **outlines**. This module embeds the 12 Liberation
//! text faces (Sans / Serif / Mono × Regular / Bold / Italic / BoldItalic) —
//! which are metric-compatible with Arial / Times New Roman / Courier New, the
//! standard substitutes for the Helvetica / Times / Courier base-14 families —
//! so the renderer can paint real glyphs instead of leaving body text blank.
//!
//! Liberation does **not** cover Symbol or ZapfDingbats (pictographic fonts);
//! those two base-14 fonts are served instead by the bundled **Noto** symbol
//! faces (see [`symbol_faces`] / [`zapf_faces`] and `fonts/symbols/`), so
//! [`liberation_fallback`] returns `None` for them while [`symbolic_fallback`]
//! supplies their outlines.
//!
//! These are *rendering* assets (unlike the mapping data in `data/`): the bytes
//! are real glyph outlines, not numeric facts. Their SIL OFL 1.1 license text
//! and provenance live alongside the files in `fonts/liberation/` and
//! `fonts/symbols/`. See those directories' `LICENSE` and the crate `NOTICE`.

/// The three Liberation text families covering the base-14 text faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiberationFamily {
    /// Liberation **Sans** — metric-compatible with Arial / Helvetica.
    Sans,
    /// Liberation **Serif** — metric-compatible with Times New Roman / Times.
    Serif,
    /// Liberation **Mono** — metric-compatible with Courier New / Courier.
    Mono,
}

const SANS_REGULAR: &[u8] = include_bytes!("../fonts/liberation/LiberationSans-Regular.ttf");
const SANS_BOLD: &[u8] = include_bytes!("../fonts/liberation/LiberationSans-Bold.ttf");
const SANS_ITALIC: &[u8] = include_bytes!("../fonts/liberation/LiberationSans-Italic.ttf");
const SANS_BOLD_ITALIC: &[u8] = include_bytes!("../fonts/liberation/LiberationSans-BoldItalic.ttf");

const SERIF_REGULAR: &[u8] = include_bytes!("../fonts/liberation/LiberationSerif-Regular.ttf");
const SERIF_BOLD: &[u8] = include_bytes!("../fonts/liberation/LiberationSerif-Bold.ttf");
const SERIF_ITALIC: &[u8] = include_bytes!("../fonts/liberation/LiberationSerif-Italic.ttf");
const SERIF_BOLD_ITALIC: &[u8] =
    include_bytes!("../fonts/liberation/LiberationSerif-BoldItalic.ttf");

const MONO_REGULAR: &[u8] = include_bytes!("../fonts/liberation/LiberationMono-Regular.ttf");
const MONO_BOLD: &[u8] = include_bytes!("../fonts/liberation/LiberationMono-Bold.ttf");
const MONO_ITALIC: &[u8] = include_bytes!("../fonts/liberation/LiberationMono-Italic.ttf");
const MONO_BOLD_ITALIC: &[u8] = include_bytes!("../fonts/liberation/LiberationMono-BoldItalic.ttf");

/// The embedded Liberation TrueType program for one `(family, bold, italic)`
/// face. The four style slots are always present, so this never fails.
#[must_use]
pub fn liberation_face(family: LiberationFamily, bold: bool, italic: bool) -> &'static [u8] {
    use LiberationFamily::{Mono, Sans, Serif};
    match (family, bold, italic) {
        (Sans, false, false) => SANS_REGULAR,
        (Sans, true, false) => SANS_BOLD,
        (Sans, false, true) => SANS_ITALIC,
        (Sans, true, true) => SANS_BOLD_ITALIC,
        (Serif, false, false) => SERIF_REGULAR,
        (Serif, true, false) => SERIF_BOLD,
        (Serif, false, true) => SERIF_ITALIC,
        (Serif, true, true) => SERIF_BOLD_ITALIC,
        (Mono, false, false) => MONO_REGULAR,
        (Mono, true, false) => MONO_BOLD,
        (Mono, false, true) => MONO_ITALIC,
        (Mono, true, true) => MONO_BOLD_ITALIC,
    }
}

/// Chooses the Liberation substitute face for a non-embedded standard-14 font.
///
/// `base_font` is the font's `/BaseFont` name (a subset `TAG+` prefix is
/// tolerated). `flags` is the `/FontDescriptor` `/Flags` integer (0 when
/// absent); its serif (bit 2), fixed-pitch (bit 1), italic (bit 7) and
/// force-bold (bit 19) bits refine the choice when the name is ambiguous.
///
/// Returns `None` for Symbol / ZapfDingbats (the *pictographic* fonts, served by
/// the Noto faces via [`symbolic_fallback`], not by Liberation) and for a name
/// that does not normalize to a standard-14 text family **and** carries no
/// family-distinguishing descriptor flag — i.e. only a genuinely standard or
/// clearly-substitutable text font gets a Liberation substitute.
#[must_use]
pub fn liberation_fallback(base_font: &str, flags: u32) -> Option<&'static [u8]> {
    use LiberationFamily::{Mono, Sans, Serif};

    // Descriptor /Flags bits (ISO 32000-1 Table 121, 1-based bit numbers).
    const FLAG_FIXED_PITCH: u32 = 1 << 0; // bit 1
    const FLAG_SERIF: u32 = 1 << 1; // bit 2
    const FLAG_ITALIC: u32 = 1 << 6; // bit 7
    const FLAG_FORCE_BOLD: u32 = 1 << 18; // bit 19

    let normalized = crate::widths::normalize_standard_font(base_font);

    // Symbol / ZapfDingbats are pictographic — Liberation does not cover them.
    if matches!(normalized, Some("Symbol") | Some("ZapfDingbats")) {
        return None;
    }

    // Derive style from the descriptor flags first, then OR in what the
    // (normalized) name encodes, so either source can establish bold/italic.
    let mut bold = flags & FLAG_FORCE_BOLD != 0;
    let mut italic = flags & FLAG_ITALIC != 0;

    let family = match normalized {
        Some(key) => {
            // A standard-14 text key already encodes its family + style.
            if key.contains("Bold") {
                bold = true;
            }
            if key.contains("Italic") || key.contains("Oblique") {
                italic = true;
            }
            if key.starts_with("Courier") {
                Mono
            } else if key.starts_with("Times") {
                Serif
            } else {
                Sans
            }
        }
        // Not a recognized standard-14 name: only substitute when a descriptor
        // flag actually distinguishes the family (serif / fixed-pitch),
        // otherwise leave the font untouched (no blind Sans substitution).
        None => {
            if flags & FLAG_FIXED_PITCH != 0 {
                Mono
            } else if flags & FLAG_SERIF != 0 {
                Serif
            } else {
                return None;
            }
        }
    };

    Some(liberation_face(family, bold, italic))
}

// === Symbol / ZapfDingbats (Noto OFL) ====================================
//
// Liberation has no pictographic glyphs, so the two *symbolic* base-14 fonts use
// bundled **Noto** OFL faces instead. Each is a short fallback chain: a primary
// face plus one supplement carrying the few glyphs the primary lacks. The
// renderer resolves a code's Unicode (via the Symbol / ZapfDingbats built-in
// encoding → glyph name → Unicode tables) against the chain in order. See
// `fonts/symbols/PROVENANCE.md` for the verified repertoire coverage.

/// Noto Sans Math — the Symbol primary (Greek letters + math operators + arrows).
const SYMBOL_PRIMARY: &[u8] = include_bytes!("../fonts/symbols/NotoSansMath-Regular.ttf");
/// Noto Sans Symbols 2 — ZapfDingbats primary; also the Symbol supplement (bullet).
const SYMBOLS2: &[u8] = include_bytes!("../fonts/symbols/NotoSansSymbols2-Regular.ttf");
/// Noto Sans Symbols — the ZapfDingbats supplement (the five `U+271D–U+2721`
/// cross dingbats absent from Noto Sans Symbols 2).
const SYMBOLS1: &[u8] = include_bytes!("../fonts/symbols/NotoSansSymbols-Regular.ttf");

/// The bundled Noto fallback chain for a non-embedded **Symbol** font, in
/// resolution order: Noto Sans Math (Greek + math), then Noto Sans Symbols 2
/// (the `bullet` supplement). A glyph absent from both renders `.notdef`.
#[must_use]
pub fn symbol_faces() -> [&'static [u8]; 2] {
    [SYMBOL_PRIMARY, SYMBOLS2]
}

/// The bundled Noto fallback chain for a non-embedded **ZapfDingbats** font, in
/// resolution order: Noto Sans Symbols 2, then Noto Sans Symbols (the five
/// `cross*` dingbats `U+271D–U+2721`). Together they cover the full repertoire.
#[must_use]
pub fn zapf_faces() -> [&'static [u8]; 2] {
    [SYMBOLS2, SYMBOLS1]
}

/// The bundled permissive **Noto** substitute face chain for a non-embedded
/// *symbolic* standard-14 font (**Symbol** or **ZapfDingbats**), or `None` when
/// `base_font` is neither.
///
/// `base_font` is the font's `/BaseFont` name (a subset `TAG+` prefix is
/// tolerated). The mechanism mirrors [`liberation_fallback`]: the substitute
/// supplies only glyph **outlines**, while advance widths stay authoritative via
/// [`crate::std_widths`]. Glyphs resolve by the character's Unicode (produced by
/// the Symbol / ZapfDingbats built-in encoding tables) against the chain.
#[must_use]
pub fn symbolic_fallback(base_font: &str) -> Option<[&'static [u8]; 2]> {
    match crate::widths::normalize_standard_font(base_font) {
        Some("Symbol") => Some(symbol_faces()),
        Some("ZapfDingbats") => Some(zapf_faces()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_names_map_to_families() {
        // Helvetica / Arial → Sans.
        assert_eq!(liberation_fallback("Helvetica", 0), Some(SANS_REGULAR));
        assert_eq!(liberation_fallback("Arial", 0), Some(SANS_REGULAR));
        // Times → Serif; Courier → Mono.
        assert_eq!(liberation_fallback("Times-Roman", 0), Some(SERIF_REGULAR));
        assert_eq!(liberation_fallback("Courier", 0), Some(MONO_REGULAR));
        assert_eq!(liberation_fallback("TimesNewRoman", 0), Some(SERIF_REGULAR));
        assert_eq!(liberation_fallback("CourierNew", 0), Some(MONO_REGULAR));
    }

    #[test]
    fn style_from_name() {
        assert_eq!(
            liberation_fallback("Helvetica-BoldOblique", 0),
            Some(SANS_BOLD_ITALIC)
        );
        assert_eq!(
            liberation_fallback("Times-BoldItalic", 0),
            Some(SERIF_BOLD_ITALIC)
        );
        assert_eq!(liberation_fallback("Arial-Bold", 0), Some(SANS_BOLD));
    }

    #[test]
    fn subset_tag_is_tolerated() {
        assert_eq!(
            liberation_fallback("ABCDEF+Arial-Italic", 0),
            Some(SANS_ITALIC)
        );
    }

    #[test]
    fn descriptor_flags_refine_style() {
        // Force-bold (bit 19) + italic (bit 7) flags add style even when the
        // name is plain.
        let flags = (1 << 18) | (1 << 6);
        assert_eq!(
            liberation_fallback("Helvetica", flags),
            Some(SANS_BOLD_ITALIC)
        );
    }

    #[test]
    fn unknown_name_uses_family_flags_only() {
        // No standard name, but a serif flag → Serif; fixed-pitch → Mono.
        assert_eq!(liberation_fallback("MyFont", 1 << 1), Some(SERIF_REGULAR));
        assert_eq!(liberation_fallback("MyFont", 1 << 0), Some(MONO_REGULAR));
        // No standard name and no distinguishing flag → no substitute.
        assert_eq!(liberation_fallback("MyFont", 0), None);
    }

    #[test]
    fn symbolic_fonts_not_in_liberation() {
        // Liberation never covers the pictographic fonts...
        assert_eq!(liberation_fallback("Symbol", 0), None);
        assert_eq!(liberation_fallback("ZapfDingbats", 0), None);
    }

    #[test]
    fn symbolic_fonts_use_noto_chain() {
        // ...the Noto fallback supplies them instead, as a primary+supplement chain.
        assert_eq!(
            symbolic_fallback("Symbol"),
            Some([SYMBOL_PRIMARY, SYMBOLS2])
        );
        assert_eq!(
            symbolic_fallback("ZapfDingbats"),
            Some([SYMBOLS2, SYMBOLS1])
        );
        // Subset tag + alias casing tolerated (normalize_standard_font handles it).
        assert_eq!(symbolic_fallback("ABCDEF+Symbol"), Some(symbol_faces()));
        assert_eq!(symbolic_fallback("ZapfDingbatsITC"), Some(zapf_faces()));
        // A Latin family is not a symbolic font.
        assert_eq!(symbolic_fallback("Helvetica"), None);
    }

    #[test]
    fn symbol_faces_are_valid_sfnt() {
        // Every Noto symbol face starts with the TrueType sfnt magic 0x00010000.
        for face in symbol_faces().into_iter().chain(zapf_faces()) {
            assert!(face.len() > 4);
            assert_eq!(&face[0..4], &[0x00, 0x01, 0x00, 0x00]);
        }
    }

    #[test]
    fn faces_are_valid_sfnt() {
        // Every embedded face starts with the TrueType sfnt magic 0x00010000.
        for fam in [
            LiberationFamily::Sans,
            LiberationFamily::Serif,
            LiberationFamily::Mono,
        ] {
            for bold in [false, true] {
                for italic in [false, true] {
                    let bytes = liberation_face(fam, bold, italic);
                    assert!(bytes.len() > 4);
                    assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x00, 0x00]);
                }
            }
        }
    }
}
