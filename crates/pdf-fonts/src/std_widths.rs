//! Core-14 standard-font *advance widths* (built-in table) — the factual AFM
//! `WX` metrics of the 14 standard typefaces named by ISO 32000-1 §9.6.2.2.
//!
//! These are numeric font-advance-width facts (1000-unit em / glyph space), not
//! copyrightable expression. They let `insert_text` place and advance Base-14
//! text without an embedded `/Widths` array. See `data/PROVENANCE.md`
//! ("Core-14 standard advance widths (built-in table)").
//!
//! Layout: each of the 12 *text* fonts (Helvetica / Times / Courier families)
//! is a `[u16; 95]` indexed by `(byte - 0x20)` covering the WinAnsi printable
//! ASCII range U+0020..=U+007E, plus a per-font default width and a small sparse
//! Latin-1 (U+00A0..=U+00FF) overlay keyed by WinAnsi glyph name. The two
//! pictographic fonts (Symbol, ZapfDingbats) use a flat default (rarely used by
//! `insert_text`). Lookup never panics: unmapped chars fall back to the default.

use crate::encodings::BaseEncoding;

/// Index of the first tabled char (U+0020, space) in each `[u16; 95]` array.
const FIRST: u32 = 0x20;
/// Last tabled char (U+007E, `~`).
const LAST: u32 = 0x7E;
/// Average advance used when an entire font is unknown (defensive only).
const FALLBACK_AVG: f64 = 500.0;

/// A static advance-width table for one standard font: the ASCII run, a default
/// width for unmapped codes, and a sparse Latin-1 overlay by glyph name.
#[derive(Debug)]
pub struct StandardWidths {
    /// Widths for U+0020..=U+007E, indexed by `char - 0x20`.
    ascii: &'static [u16; 95],
    /// Default / space-ish width for unmapped chars (never panics).
    default: u16,
    /// Sparse `(glyph_name, width)` overlay for the Latin-1 range
    /// (U+00A0..=U+00FF), resolved via the WinAnsi glyph names. Empty for the
    /// monospaced Courier family (every glyph is `default`).
    latin1: &'static [(&'static str, u16)],
}

impl StandardWidths {
    /// The advance width (1000-unit glyph space) for `ch`, using WinAnsi
    /// mapping. Unmapped chars return [`StandardWidths::default_width`].
    #[must_use]
    pub fn advance(&self, ch: char) -> f64 {
        let cp = ch as u32;
        if (FIRST..=LAST).contains(&cp) {
            return f64::from(self.ascii[(cp - FIRST) as usize]);
        }
        // Latin-1 overlay: resolve the WinAnsi glyph name for this code, then
        // look it up in the per-font sparse table.
        if (0xA0..=0xFF).contains(&cp) {
            if let Some(name) = BaseEncoding::WinAnsi.glyph_name(cp as u8) {
                if let Some(&(_, w)) = self.latin1.iter().find(|&&(n, _)| n == name) {
                    return f64::from(w);
                }
            }
        }
        f64::from(self.default)
    }

    /// The font's default / fallback advance width (1000-unit glyph space).
    #[must_use]
    pub fn default_width(&self) -> f64 {
        f64::from(self.default)
    }
}

/// The built-in advance-width table for one of the 14 canonical standard-font
/// keys (e.g. `"Helvetica"`, `"Times-Roman"`, `"Courier-Bold"`, `"Symbol"`).
/// Returns `None` for an unknown name. To accept friendly `/BaseFont` aliases,
/// run the name through [`crate::widths::normalize_standard_font`] first.
#[must_use]
pub fn standard_font_widths(std_name: &str) -> Option<&'static StandardWidths> {
    Some(match std_name {
        "Helvetica" => &HELVETICA,
        "Helvetica-Bold" => &HELVETICA_BOLD,
        "Helvetica-Oblique" => &HELVETICA_OBLIQUE,
        "Helvetica-BoldOblique" => &HELVETICA_BOLDOBLIQUE,
        "Times-Roman" => &TIMES_ROMAN,
        "Times-Bold" => &TIMES_BOLD,
        "Times-Italic" => &TIMES_ITALIC,
        "Times-BoldItalic" => &TIMES_BOLDITALIC,
        "Courier" | "Courier-Bold" | "Courier-Oblique" | "Courier-BoldOblique" => &COURIER,
        "Symbol" => &SYMBOL,
        "ZapfDingbats" => &ZAPF_DINGBATS,
        _ => return None,
    })
}

/// Total advance of `text` in the standard font `std_name`, scaled to `fontsize`
/// (`Σ advance(ch) * fontsize / 1000`). Unknown fonts approximate with an
/// average width per char; the result is always finite and never panics.
#[must_use]
pub fn string_advance(std_name: &str, text: &str, fontsize: f64) -> f64 {
    let scale = fontsize / 1000.0;
    match standard_font_widths(std_name) {
        Some(w) => text.chars().map(|c| w.advance(c)).sum::<f64>() * scale,
        None => text.chars().count() as f64 * FALLBACK_AVG * scale,
    }
}

// --- Width arrays ---------------------------------------------------------
//
// Each `[u16; 95]` is the standard AFM `WX` value for the WinAnsi-mapped code
// at `index + 0x20` (U+0020..=U+007E). Cross-checked against the anchor values
// in ISO 32000-1 / the Adobe Core14 AFM spec (see PROVENANCE.md).

// Helvetica (anchors: space=278, !=278, A=667, B=667, M=833, W=944, a=556,
// i=222, l=222, m=833, .=278, ,=278, 0..9=556).
#[rustfmt::skip]
static HELVETICA_ASCII: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 0x20..0x2F
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 0x30..0x3F
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // 0x40..0x4F
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // 0x50..0x5F
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 0x60..0x6F
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,       // 0x70..0x7E
];

// Helvetica-Bold (anchors: space=278, A=722).
#[rustfmt::skip]
static HELVETICA_BOLD_ASCII: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, // 0x20..0x2F
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, // 0x30..0x3F
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, // 0x40..0x4F
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, // 0x50..0x5F
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, // 0x60..0x6F
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,       // 0x70..0x7E
];

// Times-Roman (anchors: space=250, A=722, a=444, i=278, M=889, .=250, 0..9=500).
#[rustfmt::skip]
static TIMES_ROMAN_ASCII: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, // 0x20..0x2F
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444, // 0x30..0x3F
    921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722, // 0x40..0x4F
    556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, 333, 278, 333, 469, 500, // 0x50..0x5F
    333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500, // 0x60..0x6F
    500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,       // 0x70..0x7E
];

// Times-Bold (anchors: space=250, A=722).
#[rustfmt::skip]
static TIMES_BOLD_ASCII: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278, // 0x20..0x2F
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, // 0x30..0x3F
    930, 722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667, 944, 722, 778, // 0x40..0x4F
    611, 778, 722, 556, 667, 722, 722, 1000, 722, 722, 667, 333, 278, 333, 581, 500, // 0x50..0x5F
    333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556, 278, 833, 556, 500, // 0x60..0x6F
    556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,       // 0x70..0x7E
];

// Times-Italic (anchors: space=250, A=611).
#[rustfmt::skip]
static TIMES_ITALIC_ASCII: [u16; 95] = [
    250, 333, 420, 500, 500, 833, 778, 214, 333, 333, 500, 675, 250, 333, 250, 278, // 0x20..0x2F
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 675, 675, 675, 500, // 0x30..0x3F
    920, 611, 611, 667, 722, 611, 611, 722, 722, 333, 444, 667, 556, 833, 667, 722, // 0x40..0x4F
    611, 722, 611, 500, 556, 722, 611, 833, 611, 556, 556, 389, 278, 389, 422, 500, // 0x50..0x5F
    333, 500, 500, 444, 500, 444, 278, 500, 500, 278, 278, 444, 278, 722, 500, 500, // 0x60..0x6F
    500, 500, 389, 389, 278, 500, 444, 667, 444, 444, 389, 400, 275, 400, 541,       // 0x70..0x7E
];

// Times-BoldItalic (anchors: space=250, A=667).
#[rustfmt::skip]
static TIMES_BOLDITALIC_ASCII: [u16; 95] = [
    250, 389, 555, 500, 500, 833, 778, 278, 333, 333, 500, 570, 250, 333, 250, 278, // 0x20..0x2F
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, // 0x30..0x3F
    832, 667, 667, 667, 722, 667, 667, 722, 778, 389, 500, 667, 611, 889, 722, 722, // 0x40..0x4F
    611, 722, 667, 556, 611, 722, 667, 889, 667, 611, 611, 333, 278, 333, 570, 500, // 0x50..0x5F
    333, 500, 500, 444, 500, 444, 333, 500, 556, 278, 278, 500, 278, 778, 556, 500, // 0x60..0x6F
    500, 500, 389, 389, 278, 556, 444, 667, 500, 444, 389, 348, 220, 348, 570,       // 0x70..0x7E
];

// Courier family — monospaced: every printable glyph is 600.
#[rustfmt::skip]
static COURIER_ASCII: [u16; 95] = [600; 95];

/// The 14 standard fonts; the 12 text fonts carry full per-font tables.
static HELVETICA: StandardWidths = StandardWidths {
    ascii: &HELVETICA_ASCII,
    default: 278,
    latin1: HELVETICA_LATIN1,
};
static HELVETICA_BOLD: StandardWidths = StandardWidths {
    ascii: &HELVETICA_BOLD_ASCII,
    default: 278,
    latin1: HELVETICA_BOLD_LATIN1,
};
static HELVETICA_OBLIQUE: StandardWidths = StandardWidths {
    ascii: &HELVETICA_ASCII,
    default: 278,
    latin1: HELVETICA_LATIN1,
};
static HELVETICA_BOLDOBLIQUE: StandardWidths = StandardWidths {
    ascii: &HELVETICA_BOLD_ASCII,
    default: 278,
    latin1: HELVETICA_BOLD_LATIN1,
};
static TIMES_ROMAN: StandardWidths = StandardWidths {
    ascii: &TIMES_ROMAN_ASCII,
    default: 250,
    latin1: TIMES_ROMAN_LATIN1,
};
static TIMES_BOLD: StandardWidths = StandardWidths {
    ascii: &TIMES_BOLD_ASCII,
    default: 250,
    latin1: TIMES_BOLD_LATIN1,
};
static TIMES_ITALIC: StandardWidths = StandardWidths {
    ascii: &TIMES_ITALIC_ASCII,
    default: 250,
    latin1: TIMES_ITALIC_LATIN1,
};
static TIMES_BOLDITALIC: StandardWidths = StandardWidths {
    ascii: &TIMES_BOLDITALIC_ASCII,
    default: 250,
    latin1: TIMES_BOLDITALIC_LATIN1,
};
static COURIER: StandardWidths = StandardWidths {
    ascii: &COURIER_ASCII,
    default: 600,
    latin1: &[],
};

// Symbol / ZapfDingbats: flat defaults (rarely used by `insert_text`); see
// PROVENANCE.md. Their ASCII arrays are unused beyond the default but must
// exist; we reuse a flat-default array sized 95.
#[rustfmt::skip]
static SYMBOL_ASCII: [u16; 95] = [600; 95];
#[rustfmt::skip]
static ZAPF_ASCII: [u16; 95] = [788; 95];

static SYMBOL: StandardWidths = StandardWidths {
    ascii: &SYMBOL_ASCII,
    default: 600,
    latin1: &[],
};
static ZAPF_DINGBATS: StandardWidths = StandardWidths {
    ascii: &ZAPF_ASCII,
    default: 788,
    latin1: &[],
};

// --- Latin-1 (U+00A0..=U+00FF) overlays by WinAnsi glyph name -------------
//
// Standard AFM `WX` for the Latin-1 accented/punctuation glyphs of each text
// family. Keyed by WinAnsi glyph name (see `encodings::winansi`); the space
// glyph at 0xA0 maps to the font default so it is omitted here.

static HELVETICA_LATIN1: &[(&str, u16)] = &[
    ("exclamdown", 333),
    ("cent", 556),
    ("sterling", 556),
    ("currency", 556),
    ("yen", 556),
    ("brokenbar", 260),
    ("section", 556),
    ("dieresis", 333),
    ("copyright", 737),
    ("ordfeminine", 370),
    ("guillemotleft", 556),
    ("logicalnot", 584),
    ("hyphen", 333),
    ("registered", 737),
    ("macron", 333),
    ("degree", 400),
    ("plusminus", 584),
    ("twosuperior", 333),
    ("threesuperior", 333),
    ("acute", 333),
    ("mu", 556),
    ("paragraph", 537),
    ("periodcentered", 278),
    ("cedilla", 333),
    ("onesuperior", 333),
    ("ordmasculine", 365),
    ("guillemotright", 556),
    ("onequarter", 834),
    ("onehalf", 834),
    ("threequarters", 834),
    ("questiondown", 611),
    ("Agrave", 667),
    ("Aacute", 667),
    ("Acircumflex", 667),
    ("Atilde", 667),
    ("Adieresis", 667),
    ("Aring", 667),
    ("AE", 1000),
    ("Ccedilla", 722),
    ("Egrave", 667),
    ("Eacute", 667),
    ("Ecircumflex", 667),
    ("Edieresis", 667),
    ("Igrave", 278),
    ("Iacute", 278),
    ("Icircumflex", 278),
    ("Idieresis", 278),
    ("Eth", 722),
    ("Ntilde", 722),
    ("Ograve", 778),
    ("Oacute", 778),
    ("Ocircumflex", 778),
    ("Otilde", 778),
    ("Odieresis", 778),
    ("multiply", 584),
    ("Oslash", 778),
    ("Ugrave", 722),
    ("Uacute", 722),
    ("Ucircumflex", 722),
    ("Udieresis", 722),
    ("Yacute", 667),
    ("Thorn", 667),
    ("germandbls", 611),
    ("agrave", 556),
    ("aacute", 556),
    ("acircumflex", 556),
    ("atilde", 556),
    ("adieresis", 556),
    ("aring", 556),
    ("ae", 889),
    ("ccedilla", 500),
    ("egrave", 556),
    ("eacute", 556),
    ("ecircumflex", 556),
    ("edieresis", 556),
    ("igrave", 278),
    ("iacute", 278),
    ("icircumflex", 278),
    ("idieresis", 278),
    ("eth", 556),
    ("ntilde", 556),
    ("ograve", 556),
    ("oacute", 556),
    ("ocircumflex", 556),
    ("otilde", 556),
    ("odieresis", 556),
    ("divide", 584),
    ("oslash", 611),
    ("ugrave", 556),
    ("uacute", 556),
    ("ucircumflex", 556),
    ("udieresis", 556),
    ("yacute", 500),
    ("thorn", 556),
    ("ydieresis", 500),
];

static HELVETICA_BOLD_LATIN1: &[(&str, u16)] = &[
    ("exclamdown", 333),
    ("cent", 556),
    ("sterling", 556),
    ("currency", 556),
    ("yen", 556),
    ("brokenbar", 280),
    ("section", 556),
    ("dieresis", 333),
    ("copyright", 737),
    ("ordfeminine", 370),
    ("guillemotleft", 556),
    ("logicalnot", 584),
    ("hyphen", 333),
    ("registered", 737),
    ("macron", 333),
    ("degree", 400),
    ("plusminus", 584),
    ("twosuperior", 333),
    ("threesuperior", 333),
    ("acute", 333),
    ("mu", 611),
    ("paragraph", 556),
    ("periodcentered", 278),
    ("cedilla", 333),
    ("onesuperior", 333),
    ("ordmasculine", 365),
    ("guillemotright", 556),
    ("onequarter", 834),
    ("onehalf", 834),
    ("threequarters", 834),
    ("questiondown", 611),
    ("Agrave", 722),
    ("Aacute", 722),
    ("Acircumflex", 722),
    ("Atilde", 722),
    ("Adieresis", 722),
    ("Aring", 722),
    ("AE", 1000),
    ("Ccedilla", 722),
    ("Egrave", 667),
    ("Eacute", 667),
    ("Ecircumflex", 667),
    ("Edieresis", 667),
    ("Igrave", 278),
    ("Iacute", 278),
    ("Icircumflex", 278),
    ("Idieresis", 278),
    ("Eth", 722),
    ("Ntilde", 722),
    ("Ograve", 778),
    ("Oacute", 778),
    ("Ocircumflex", 778),
    ("Otilde", 778),
    ("Odieresis", 778),
    ("multiply", 584),
    ("Oslash", 778),
    ("Ugrave", 722),
    ("Uacute", 722),
    ("Ucircumflex", 722),
    ("Udieresis", 722),
    ("Yacute", 667),
    ("Thorn", 667),
    ("germandbls", 611),
    ("agrave", 556),
    ("aacute", 556),
    ("acircumflex", 556),
    ("atilde", 556),
    ("adieresis", 556),
    ("aring", 556),
    ("ae", 889),
    ("ccedilla", 556),
    ("egrave", 611),
    ("eacute", 611),
    ("ecircumflex", 611),
    ("edieresis", 611),
    ("igrave", 278),
    ("iacute", 278),
    ("icircumflex", 278),
    ("idieresis", 278),
    ("eth", 611),
    ("ntilde", 611),
    ("ograve", 611),
    ("oacute", 611),
    ("ocircumflex", 611),
    ("otilde", 611),
    ("odieresis", 611),
    ("divide", 584),
    ("oslash", 611),
    ("ugrave", 611),
    ("uacute", 611),
    ("ucircumflex", 611),
    ("udieresis", 611),
    ("yacute", 556),
    ("thorn", 611),
    ("ydieresis", 556),
];

static TIMES_ROMAN_LATIN1: &[(&str, u16)] = &[
    ("exclamdown", 333),
    ("cent", 500),
    ("sterling", 500),
    ("currency", 500),
    ("yen", 500),
    ("brokenbar", 200),
    ("section", 500),
    ("dieresis", 333),
    ("copyright", 760),
    ("ordfeminine", 276),
    ("guillemotleft", 500),
    ("logicalnot", 564),
    ("hyphen", 333),
    ("registered", 760),
    ("macron", 333),
    ("degree", 400),
    ("plusminus", 564),
    ("twosuperior", 300),
    ("threesuperior", 300),
    ("acute", 333),
    ("mu", 500),
    ("paragraph", 453),
    ("periodcentered", 250),
    ("cedilla", 333),
    ("onesuperior", 300),
    ("ordmasculine", 310),
    ("guillemotright", 500),
    ("onequarter", 750),
    ("onehalf", 750),
    ("threequarters", 750),
    ("questiondown", 444),
    ("Agrave", 722),
    ("Aacute", 722),
    ("Acircumflex", 722),
    ("Atilde", 722),
    ("Adieresis", 722),
    ("Aring", 722),
    ("AE", 889),
    ("Ccedilla", 667),
    ("Egrave", 611),
    ("Eacute", 611),
    ("Ecircumflex", 611),
    ("Edieresis", 611),
    ("Igrave", 333),
    ("Iacute", 333),
    ("Icircumflex", 333),
    ("Idieresis", 333),
    ("Eth", 722),
    ("Ntilde", 722),
    ("Ograve", 722),
    ("Oacute", 722),
    ("Ocircumflex", 722),
    ("Otilde", 722),
    ("Odieresis", 722),
    ("multiply", 564),
    ("Oslash", 722),
    ("Ugrave", 722),
    ("Uacute", 722),
    ("Ucircumflex", 722),
    ("Udieresis", 722),
    ("Yacute", 722),
    ("Thorn", 556),
    ("germandbls", 500),
    ("agrave", 444),
    ("aacute", 444),
    ("acircumflex", 444),
    ("atilde", 444),
    ("adieresis", 444),
    ("aring", 444),
    ("ae", 667),
    ("ccedilla", 444),
    ("egrave", 444),
    ("eacute", 444),
    ("ecircumflex", 444),
    ("edieresis", 444),
    ("igrave", 278),
    ("iacute", 278),
    ("icircumflex", 278),
    ("idieresis", 278),
    ("eth", 500),
    ("ntilde", 500),
    ("ograve", 500),
    ("oacute", 500),
    ("ocircumflex", 500),
    ("otilde", 500),
    ("odieresis", 500),
    ("divide", 564),
    ("oslash", 500),
    ("ugrave", 500),
    ("uacute", 500),
    ("ucircumflex", 500),
    ("udieresis", 500),
    ("yacute", 500),
    ("thorn", 500),
    ("ydieresis", 500),
];

static TIMES_BOLD_LATIN1: &[(&str, u16)] = &[
    ("exclamdown", 333),
    ("cent", 500),
    ("sterling", 500),
    ("currency", 500),
    ("yen", 500),
    ("brokenbar", 220),
    ("section", 500),
    ("dieresis", 333),
    ("copyright", 747),
    ("ordfeminine", 300),
    ("guillemotleft", 500),
    ("logicalnot", 570),
    ("hyphen", 333),
    ("registered", 747),
    ("macron", 333),
    ("degree", 400),
    ("plusminus", 570),
    ("twosuperior", 300),
    ("threesuperior", 300),
    ("acute", 333),
    ("mu", 556),
    ("paragraph", 540),
    ("periodcentered", 250),
    ("cedilla", 333),
    ("onesuperior", 300),
    ("ordmasculine", 330),
    ("guillemotright", 500),
    ("onequarter", 750),
    ("onehalf", 750),
    ("threequarters", 750),
    ("questiondown", 500),
    ("Agrave", 722),
    ("Aacute", 722),
    ("Acircumflex", 722),
    ("Atilde", 722),
    ("Adieresis", 722),
    ("Aring", 722),
    ("AE", 1000),
    ("Ccedilla", 722),
    ("Egrave", 667),
    ("Eacute", 667),
    ("Ecircumflex", 667),
    ("Edieresis", 667),
    ("Igrave", 389),
    ("Iacute", 389),
    ("Icircumflex", 389),
    ("Idieresis", 389),
    ("Eth", 722),
    ("Ntilde", 722),
    ("Ograve", 778),
    ("Oacute", 778),
    ("Ocircumflex", 778),
    ("Otilde", 778),
    ("Odieresis", 778),
    ("multiply", 570),
    ("Oslash", 778),
    ("Ugrave", 722),
    ("Uacute", 722),
    ("Ucircumflex", 722),
    ("Udieresis", 722),
    ("Yacute", 722),
    ("Thorn", 611),
    ("germandbls", 556),
    ("agrave", 500),
    ("aacute", 500),
    ("acircumflex", 500),
    ("atilde", 500),
    ("adieresis", 500),
    ("aring", 500),
    ("ae", 722),
    ("ccedilla", 444),
    ("egrave", 444),
    ("eacute", 444),
    ("ecircumflex", 444),
    ("edieresis", 444),
    ("igrave", 278),
    ("iacute", 278),
    ("icircumflex", 278),
    ("idieresis", 278),
    ("eth", 500),
    ("ntilde", 556),
    ("ograve", 500),
    ("oacute", 500),
    ("ocircumflex", 500),
    ("otilde", 500),
    ("odieresis", 500),
    ("divide", 570),
    ("oslash", 500),
    ("ugrave", 556),
    ("uacute", 556),
    ("ucircumflex", 556),
    ("udieresis", 556),
    ("yacute", 500),
    ("thorn", 500),
    ("ydieresis", 500),
];

static TIMES_ITALIC_LATIN1: &[(&str, u16)] = &[
    ("exclamdown", 389),
    ("cent", 500),
    ("sterling", 500),
    ("currency", 500),
    ("yen", 500),
    ("brokenbar", 275),
    ("section", 500),
    ("dieresis", 333),
    ("copyright", 760),
    ("ordfeminine", 276),
    ("guillemotleft", 500),
    ("logicalnot", 675),
    ("hyphen", 333),
    ("registered", 760),
    ("macron", 333),
    ("degree", 400),
    ("plusminus", 675),
    ("twosuperior", 300),
    ("threesuperior", 300),
    ("acute", 333),
    ("mu", 500),
    ("paragraph", 523),
    ("periodcentered", 250),
    ("cedilla", 333),
    ("onesuperior", 300),
    ("ordmasculine", 310),
    ("guillemotright", 500),
    ("onequarter", 750),
    ("onehalf", 750),
    ("threequarters", 750),
    ("questiondown", 500),
    ("Agrave", 611),
    ("Aacute", 611),
    ("Acircumflex", 611),
    ("Atilde", 611),
    ("Adieresis", 611),
    ("Aring", 611),
    ("AE", 889),
    ("Ccedilla", 667),
    ("Egrave", 611),
    ("Eacute", 611),
    ("Ecircumflex", 611),
    ("Edieresis", 611),
    ("Igrave", 333),
    ("Iacute", 333),
    ("Icircumflex", 333),
    ("Idieresis", 333),
    ("Eth", 722),
    ("Ntilde", 722),
    ("Ograve", 722),
    ("Oacute", 722),
    ("Ocircumflex", 722),
    ("Otilde", 722),
    ("Odieresis", 722),
    ("multiply", 675),
    ("Oslash", 722),
    ("Ugrave", 722),
    ("Uacute", 722),
    ("Ucircumflex", 722),
    ("Udieresis", 722),
    ("Yacute", 556),
    ("Thorn", 611),
    ("germandbls", 500),
    ("agrave", 444),
    ("aacute", 444),
    ("acircumflex", 444),
    ("atilde", 444),
    ("adieresis", 444),
    ("aring", 444),
    ("ae", 667),
    ("ccedilla", 444),
    ("egrave", 444),
    ("eacute", 444),
    ("ecircumflex", 444),
    ("edieresis", 444),
    ("igrave", 278),
    ("iacute", 278),
    ("icircumflex", 278),
    ("idieresis", 278),
    ("eth", 500),
    ("ntilde", 500),
    ("ograve", 500),
    ("oacute", 500),
    ("ocircumflex", 500),
    ("otilde", 500),
    ("odieresis", 500),
    ("divide", 675),
    ("oslash", 500),
    ("ugrave", 500),
    ("uacute", 500),
    ("ucircumflex", 500),
    ("udieresis", 500),
    ("yacute", 444),
    ("thorn", 500),
    ("ydieresis", 444),
];

static TIMES_BOLDITALIC_LATIN1: &[(&str, u16)] = &[
    ("exclamdown", 389),
    ("cent", 500),
    ("sterling", 500),
    ("currency", 500),
    ("yen", 500),
    ("brokenbar", 220),
    ("section", 500),
    ("dieresis", 333),
    ("copyright", 747),
    ("ordfeminine", 266),
    ("guillemotleft", 500),
    ("logicalnot", 606),
    ("hyphen", 333),
    ("registered", 747),
    ("macron", 333),
    ("degree", 400),
    ("plusminus", 570),
    ("twosuperior", 300),
    ("threesuperior", 300),
    ("acute", 333),
    ("mu", 576),
    ("paragraph", 500),
    ("periodcentered", 250),
    ("cedilla", 333),
    ("onesuperior", 300),
    ("ordmasculine", 300),
    ("guillemotright", 500),
    ("onequarter", 750),
    ("onehalf", 750),
    ("threequarters", 750),
    ("questiondown", 500),
    ("Agrave", 667),
    ("Aacute", 667),
    ("Acircumflex", 667),
    ("Atilde", 667),
    ("Adieresis", 667),
    ("Aring", 667),
    ("AE", 944),
    ("Ccedilla", 667),
    ("Egrave", 667),
    ("Eacute", 667),
    ("Ecircumflex", 667),
    ("Edieresis", 667),
    ("Igrave", 389),
    ("Iacute", 389),
    ("Icircumflex", 389),
    ("Idieresis", 389),
    ("Eth", 722),
    ("Ntilde", 722),
    ("Ograve", 722),
    ("Oacute", 722),
    ("Ocircumflex", 722),
    ("Otilde", 722),
    ("Odieresis", 722),
    ("multiply", 570),
    ("Oslash", 722),
    ("Ugrave", 722),
    ("Uacute", 722),
    ("Ucircumflex", 722),
    ("Udieresis", 722),
    ("Yacute", 611),
    ("Thorn", 611),
    ("germandbls", 500),
    ("agrave", 500),
    ("aacute", 500),
    ("acircumflex", 500),
    ("atilde", 500),
    ("adieresis", 500),
    ("aring", 500),
    ("ae", 722),
    ("ccedilla", 444),
    ("egrave", 444),
    ("eacute", 444),
    ("ecircumflex", 444),
    ("edieresis", 444),
    ("igrave", 278),
    ("iacute", 278),
    ("icircumflex", 278),
    ("idieresis", 278),
    ("eth", 500),
    ("ntilde", 556),
    ("ograve", 500),
    ("oacute", 500),
    ("ocircumflex", 500),
    ("otilde", 500),
    ("odieresis", 500),
    ("divide", 570),
    ("oslash", 500),
    ("ugrave", 556),
    ("uacute", 556),
    ("ucircumflex", 556),
    ("udieresis", 556),
    ("yacute", 444),
    ("thorn", 500),
    ("ydieresis", 444),
];
