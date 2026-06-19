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
//! [`liberation_fallback`] returns `None` for those, leaving them a documented
//! residual rather than regressing them.
//!
//! These are *rendering* assets (unlike the mapping data in `data/`): the bytes
//! are real glyph outlines, not numeric facts. Their SIL OFL 1.1 license text
//! and provenance live alongside the files in `fonts/liberation/`. See that
//! directory's `LICENSE` and the crate `NOTICE`.

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
/// Returns `None` for Symbol / ZapfDingbats (not covered by Liberation) and for
/// a name that does not normalize to a standard-14 text family **and** carries
/// no family-distinguishing descriptor flag — i.e. only a genuinely standard or
/// clearly-substitutable font gets a substitute.
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
    fn symbolic_fonts_not_covered() {
        assert_eq!(liberation_fallback("Symbol", 0), None);
        assert_eq!(liberation_fallback("ZapfDingbats", 0), None);
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
