//! A standalone Core-14 font handle (PyMuPDF `fitz.Font`) — name, vertical
//! metrics, glyph advances and glyph-name ↔ Unicode helpers over the built-in
//! AFM data (PRD §8.5 / §10.7).
//!
//! This is the pure-Rust analogue of PyMuPDF's `Font` object for the 14 standard
//! typefaces. Metrics (`ascender`, `descender`, `bbox`, advances) are the
//! factual Adobe Core-14 AFM values (1000-unit glyph space normalized to a unit
//! em), not the bundled-font substitutes MuPDF ships; the structure and ratios
//! match. Embedded / user TTFs are out of this handle's scope (a later
//! milestone); constructing from an unknown name falls back to Helvetica so the
//! handle is always usable.

use smol_str::SmolStr;

use crate::glyphlist::{glyph_name_to_unicode, unicode_to_glyph_name};
use crate::std_widths::{standard_font_widths, StandardWidths};
use crate::widths::normalize_standard_font;

/// Canonical Core-14 vertical metrics (Adobe AFM `FontBBox` / `Ascender` /
/// `Descender`), normalized to a unit em (÷1000). One row per standard key.
struct Core14Metrics {
    ascender: f64,
    descender: f64,
    bbox: (f64, f64, f64, f64),
    glyph_count: u32,
    serif: bool,
    bold: bool,
    italic: bool,
    monospace: bool,
}

/// Returns the canonical metrics for one of the 14 standard-font keys.
fn metrics(std_name: &str) -> Core14Metrics {
    // Ascender/Descender/FontBBox are the published Adobe AFM values for the
    // Core-14 typefaces (÷1000). glyph_count is the AFM `C` entry count.
    match std_name {
        "Helvetica" | "Helvetica-Oblique" => Core14Metrics {
            ascender: 0.718,
            descender: -0.207,
            bbox: (-0.166, -0.225, 1.0, 0.931),
            glyph_count: 315,
            serif: false,
            bold: std_name.contains("Bold"),
            italic: std_name.contains("Oblique"),
            monospace: false,
        },
        "Helvetica-Bold" | "Helvetica-BoldOblique" => Core14Metrics {
            ascender: 0.718,
            descender: -0.207,
            bbox: (-0.17, -0.228, 1.003, 0.962),
            glyph_count: 315,
            serif: false,
            bold: true,
            italic: std_name.contains("Oblique"),
            monospace: false,
        },
        "Times-Roman" => Core14Metrics {
            ascender: 0.683,
            descender: -0.217,
            bbox: (-0.168, -0.218, 1.0, 0.898),
            glyph_count: 315,
            serif: true,
            bold: false,
            italic: false,
            monospace: false,
        },
        "Times-Bold" => Core14Metrics {
            ascender: 0.683,
            descender: -0.217,
            bbox: (-0.168, -0.218, 1.0, 0.935),
            glyph_count: 315,
            serif: true,
            bold: true,
            italic: false,
            monospace: false,
        },
        "Times-Italic" => Core14Metrics {
            ascender: 0.683,
            descender: -0.217,
            bbox: (-0.169, -0.217, 1.01, 0.883),
            glyph_count: 315,
            serif: true,
            bold: false,
            italic: true,
            monospace: false,
        },
        "Times-BoldItalic" => Core14Metrics {
            ascender: 0.683,
            descender: -0.217,
            bbox: (-0.2, -0.218, 0.996, 0.921),
            glyph_count: 315,
            serif: true,
            bold: true,
            italic: true,
            monospace: false,
        },
        "Courier" | "Courier-Bold" | "Courier-Oblique" | "Courier-BoldOblique" => Core14Metrics {
            ascender: 0.629,
            descender: -0.157,
            bbox: (-0.023, -0.25, 0.715, 0.805),
            glyph_count: 315,
            serif: false,
            bold: std_name.contains("Bold"),
            italic: std_name.contains("Oblique"),
            monospace: true,
        },
        "Symbol" => Core14Metrics {
            ascender: 1.01,
            descender: -0.293,
            bbox: (-0.18, -0.293, 1.09, 1.01),
            glyph_count: 190,
            serif: false,
            bold: false,
            italic: false,
            monospace: false,
        },
        "ZapfDingbats" => Core14Metrics {
            ascender: 0.82,
            descender: -0.143,
            bbox: (-0.001, -0.143, 0.981, 0.82),
            glyph_count: 202,
            serif: false,
            bold: false,
            italic: false,
            monospace: false,
        },
        // Fallback (unreachable for the 14 keys): Helvetica metrics.
        _ => Core14Metrics {
            ascender: 0.718,
            descender: -0.207,
            bbox: (-0.166, -0.225, 1.0, 0.931),
            glyph_count: 315,
            serif: false,
            bold: false,
            italic: false,
            monospace: false,
        },
    }
}

/// PyMuPDF-style friendly aliases for the 14 standard fonts (`helv`, `tiro`,
/// `cour`, …) → the canonical AFM key. Unknown aliases fall through to
/// [`normalize_standard_font`] then to `Helvetica`.
fn resolve_std_name(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "helv" | "helvetica" => "Helvetica",
        "heit" => "Helvetica-Oblique",
        "hebo" => "Helvetica-Bold",
        "hebi" => "Helvetica-BoldOblique",
        "cour" | "courier" => "Courier",
        "cobo" => "Courier-Bold",
        "coit" => "Courier-Oblique",
        "cobi" => "Courier-BoldOblique",
        "tiro" | "times-roman" | "times" => "Times-Roman",
        "tibo" => "Times-Bold",
        "tiit" => "Times-Italic",
        "tibi" => "Times-BoldItalic",
        "symb" | "symbol" => "Symbol",
        "zadb" | "zapfdingbats" => "ZapfDingbats",
        other => normalize_standard_font(other).unwrap_or("Helvetica"),
    }
}

/// A Core-14 font handle (PyMuPDF `fitz.Font`).
pub struct Font {
    std_name: &'static str,
    metrics: Core14Metrics,
    widths: &'static StandardWidths,
}

impl Font {
    /// Builds a handle for the standard font named `name` (a canonical AFM key
    /// or a PyMuPDF alias such as `"helv"`). Unknown names fall back to
    /// Helvetica so the handle is always usable.
    #[must_use]
    pub fn new(name: &str) -> Self {
        let std_name = resolve_std_name(name);
        let widths = standard_font_widths(std_name).unwrap_or_else(|| {
            standard_font_widths("Helvetica").expect("Helvetica metrics present")
        });
        Font {
            std_name,
            metrics: metrics(std_name),
            widths,
        }
    }

    /// The font's canonical name (e.g. `"Helvetica"`).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.std_name
    }

    /// The ascender, normalized to a unit em (PyMuPDF `Font.ascender`).
    #[must_use]
    pub fn ascender(&self) -> f64 {
        self.metrics.ascender
    }

    /// The descender (usually negative), unit em (PyMuPDF `Font.descender`).
    #[must_use]
    pub fn descender(&self) -> f64 {
        self.metrics.descender
    }

    /// The font bounding box `(x0, y0, x1, y1)`, unit em (PyMuPDF `Font.bbox`).
    #[must_use]
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        self.metrics.bbox
    }

    /// The number of glyphs the font defines (PyMuPDF `Font.glyph_count`).
    #[must_use]
    pub fn glyph_count(&self) -> u32 {
        self.metrics.glyph_count
    }

    /// Whether the font is bold / italic / serifed / monospaced.
    #[must_use]
    pub fn is_bold(&self) -> bool {
        self.metrics.bold
    }
    #[must_use]
    pub fn is_italic(&self) -> bool {
        self.metrics.italic
    }
    #[must_use]
    pub fn is_serif(&self) -> bool {
        self.metrics.serif
    }
    #[must_use]
    pub fn is_monospaced(&self) -> bool {
        self.metrics.monospace
    }

    /// The advance of the glyph for Unicode scalar `cp`, normalized to a unit em
    /// (PyMuPDF `Font.glyph_advance(chr)` — note PyMuPDF keys advances on the
    /// character code, not the glyph id). Returns the font default for scalars
    /// the font has no metric for.
    #[must_use]
    pub fn glyph_advance(&self, cp: u32) -> f64 {
        match char::from_u32(cp) {
            Some(c) => self.widths.advance(c) / 1000.0,
            None => self.widths.default_width() / 1000.0,
        }
    }

    /// Whether the font defines a glyph for Unicode scalar `cp` (PyMuPDF
    /// `Font.has_glyph`). Core-14 coverage is the WinAnsi printable + Latin-1
    /// set; an undefined scalar reports `false`.
    #[must_use]
    pub fn has_glyph(&self, cp: u32) -> bool {
        let Some(c) = char::from_u32(cp) else {
            return false;
        };
        let v = c as u32;
        // Printable ASCII + Latin-1 supplement is what the Core-14 text fonts
        // carry; the pictographic fonts always answer for any nameable scalar.
        (0x20..=0x7E).contains(&v) || (0xA0..=0xFF).contains(&v) || self.std_name == "Symbol"
    }

    /// The total advance of `text` at `fontsize`, in text-space units (PyMuPDF
    /// `Font.text_length`). `Σ advance(ch) · fontsize`.
    #[must_use]
    pub fn text_length(&self, text: &str, fontsize: f64) -> f64 {
        text.chars()
            .map(|c| self.widths.advance(c) / 1000.0 * fontsize)
            .sum()
    }

    /// The per-character advances of `text` at `fontsize` (PyMuPDF
    /// `Font.char_lengths`).
    #[must_use]
    pub fn char_lengths(&self, text: &str, fontsize: f64) -> Vec<f64> {
        text.chars()
            .map(|c| self.widths.advance(c) / 1000.0 * fontsize)
            .collect()
    }

    /// The Unicode scalar for the AGL glyph name `name` (PyMuPDF
    /// `Font.glyph_name_to_unicode`), or `0xFFFD` when unresolvable (matching
    /// PyMuPDF's replacement behavior).
    #[must_use]
    pub fn glyph_name_to_unicode(&self, name: &str) -> u32 {
        glyph_name_to_unicode(name)
            .and_then(|s| s.chars().next())
            .map_or(0xFFFD, |c| c as u32)
    }

    /// The AGL glyph name for Unicode scalar `cp` (PyMuPDF
    /// `Font.unicode_to_glyph_name`), or an empty string when unresolvable.
    #[must_use]
    pub fn unicode_to_glyph_name(&self, cp: u32) -> SmolStr {
        unicode_to_glyph_name(cp).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve() {
        assert_eq!(Font::new("helv").name(), "Helvetica");
        assert_eq!(Font::new("tiro").name(), "Times-Roman");
        assert_eq!(Font::new("cour").name(), "Courier");
        assert_eq!(Font::new("symb").name(), "Symbol");
        assert_eq!(Font::new("Times-Bold").name(), "Times-Bold");
        // Unknown → Helvetica.
        assert_eq!(Font::new("nonesuch").name(), "Helvetica");
    }

    #[test]
    fn metrics_present() {
        let f = Font::new("helv");
        assert!(f.ascender() > 0.0);
        assert!(f.descender() < 0.0);
        assert!(f.glyph_count() > 0);
        let bb = f.bbox();
        assert!(bb.0 < bb.2 && bb.1 < bb.3);
        assert!(!f.is_bold() && !f.is_italic() && !f.is_serif() && !f.is_monospaced());
        assert!(Font::new("cour").is_monospaced());
        assert!(Font::new("tibo").is_bold() && Font::new("tibo").is_serif());
    }

    #[test]
    fn advances_and_lengths() {
        let f = Font::new("helv");
        // Helvetica 'A' AFM width is 667 → 0.667.
        assert!((f.glyph_advance('A' as u32) - 0.667).abs() < 1e-9);
        // text_length scales with fontsize.
        let l = f.text_length("AB", 10.0);
        assert!((l - (0.667 + 0.667) * 10.0).abs() < 1e-6);
        assert_eq!(f.char_lengths("AB", 1.0).len(), 2);
        assert!(f.has_glyph('A' as u32));
        assert!(!f.has_glyph(0x1F600)); // emoji not in Core-14
    }

    #[test]
    fn glyph_name_mappings() {
        let f = Font::new("helv");
        assert_eq!(f.glyph_name_to_unicode("A"), 'A' as u32);
        assert_eq!(f.glyph_name_to_unicode(".notdef"), 0xFFFD);
        assert_eq!(f.unicode_to_glyph_name('A' as u32).as_str(), "A");
        assert_eq!(f.unicode_to_glyph_name(0x00E9).as_str(), "eacute");
    }
}
