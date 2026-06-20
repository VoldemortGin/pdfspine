//! A standalone Core-14 font handle (PyMuPDF `fitz.Font`) — name, vertical
//! metrics, glyph advances and glyph-name ↔ Unicode helpers over the built-in
//! AFM data (PRD §8.5 / §10.7).
//!
//! This is the pure-Rust analogue of PyMuPDF's `Font` object for the 14 standard
//! typefaces. Metrics (`ascender`, `descender`, `bbox`, advances) are the
//! factual Adobe Core-14 AFM values (1000-unit glyph space normalized to a unit
//! em), not the bundled-font substitutes MuPDF ships; the structure and ratios
//! match. Constructing from an unknown name falls back to Helvetica so the
//! handle is always usable.
//!
//! A handle may additionally carry a real **font program** — the bytes of an
//! embedded `/FontFile*` stream, or a user-supplied `fontfile=` / `fontbuffer=`
//! TrueType/OpenType program. When present (see [`Font::from_program`]), the
//! handle parses it with the same `ttf-parser` infrastructure the renderer's
//! `GlyphFont` uses, so [`Font::buffer`] returns the real bytes,
//! [`Font::glyph_bbox`] reports the real per-glyph outline box, and
//! [`Font::valid_codepoints`] reflects the program's actual `cmap` coverage.

use std::sync::Arc;

use smol_str::SmolStr;
use ttf_parser::{name_id, Face, GlyphId};

use crate::encodings::BaseEncoding;
use crate::glyphlist::{glyph_name_to_unicode, unicode_to_glyph_name};
use crate::std_widths::{standard_font_widths, StandardWidths};
use crate::widths::normalize_standard_font;

/// The 14 standard PDF base-font names, in PyMuPDF's exact order
/// (`fitz.Base14_fontnames`). A class/module-level constant.
pub const BASE14_FONTNAMES: [&str; 14] = [
    "Courier",
    "Courier-Oblique",
    "Courier-Bold",
    "Courier-BoldOblique",
    "Helvetica",
    "Helvetica-Oblique",
    "Helvetica-Bold",
    "Helvetica-BoldOblique",
    "Times-Roman",
    "Times-Italic",
    "Times-Bold",
    "Times-BoldItalic",
    "Symbol",
    "ZapfDingbats",
];

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

/// The program's human-readable display name (PyMuPDF `Font.name`).
///
/// PyMuPDF/MuPDF report `"<family> <subfamily>"` (sfnt `name` ids 1 + 2), e.g.
/// `"Liberation Sans Regular"`. This builds that, falling back to the full-name
/// record (id 4) and finally `None` when the program carries no usable name (the
/// caller then keeps the requested font name).
fn face_full_name(face: &Face) -> Option<SmolStr> {
    let pick = |id: u16| -> Option<String> {
        let names = face.names();
        names
            .into_iter()
            .find(|n| n.name_id == id && n.is_unicode())
            .or_else(|| names.into_iter().find(|n| n.name_id == id))
            .and_then(|n| n.to_string())
    };
    if let Some(family) = pick(name_id::FAMILY) {
        let combined = match pick(name_id::SUBFAMILY) {
            Some(sub) if !sub.is_empty() => format!("{family} {sub}"),
            _ => family,
        };
        return Some(SmolStr::new(combined));
    }
    pick(name_id::FULL_NAME).map(SmolStr::new)
}

/// A font handle (PyMuPDF `fitz.Font`).
///
/// Two flavors share one type:
/// - a **Core-14 metrics-only** handle built from a name ([`Font::new`]) — no
///   font program, metrics from the Adobe AFM tables;
/// - a **program-backed** handle built from `/FontFile*` or a user
///   `fontfile=` / `fontbuffer=` program ([`Font::from_program`]) — the parsed
///   outlines drive `buffer` / `glyph_bbox` / `valid_codepoints`, while the
///   Core-14 metric fields still back the name-keyed metric accessors.
pub struct Font {
    std_name: &'static str,
    /// The display name (PyMuPDF `Font.name`): the program's full-name record
    /// when a program is loaded, else the resolved Core-14 key.
    display_name: SmolStr,
    metrics: Core14Metrics,
    widths: &'static StandardWidths,
    /// The embedded / user font-program bytes (`/FontFile*`), parsed on demand.
    /// `None` for a metrics-only Core-14 handle.
    program: Option<Arc<[u8]>>,
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
            display_name: SmolStr::new(std_name),
            metrics: metrics(std_name),
            widths,
            program: None,
        }
    }

    /// Builds a handle that carries a real font **program** (the bytes of an
    /// embedded `/FontFile*` stream, or a user-supplied `fontfile=` /
    /// `fontbuffer=` TrueType/OpenType program).
    ///
    /// The program is parsed with `ttf-parser` (the same path the renderer's
    /// `GlyphFont` uses): on success the handle's display [`name`](Self::name) is
    /// the program's full-name record (falling back to `fallback_name`), and
    /// [`buffer`](Self::buffer) / [`glyph_bbox`](Self::glyph_bbox) /
    /// [`valid_codepoints`](Self::valid_codepoints) are served from the outlines.
    /// The name-keyed metric accessors still come from the Core-14 table chosen by
    /// `fallback_name`, so a program-backed handle remains fully usable.
    ///
    /// Returns `None` when `program` is not a `ttf-parser`-parseable sfnt
    /// (TrueType `FontFile2` / OpenType-CFF `FontFile3`) — e.g. a bare Type1 PFB
    /// `/FontFile`, which has no outline parser here; the caller then falls back
    /// to [`Font::new`].
    #[must_use]
    pub fn from_program(program: &[u8], fallback_name: &str) -> Option<Self> {
        let face = Face::parse(program, 0).ok()?;
        let std_name = resolve_std_name(fallback_name);
        let widths = standard_font_widths(std_name).unwrap_or_else(|| {
            standard_font_widths("Helvetica").expect("Helvetica metrics present")
        });
        // PyMuPDF's `Font.name` for a loaded program is the font's full-name
        // record (name id 4), e.g. "Liberation Sans Regular"; fall back to the
        // requested name when the program carries none.
        let display_name = face_full_name(&face).unwrap_or_else(|| SmolStr::new(fallback_name));
        Some(Font {
            std_name,
            display_name,
            metrics: metrics(std_name),
            widths,
            program: Some(Arc::from(program.to_vec())),
        })
    }

    /// Parses the carried program into a `ttf-parser` [`Face`], or `None` for a
    /// metrics-only handle. Re-parses per call (cheap; only the program-backed
    /// accessors use it).
    fn face(&self) -> Option<Face<'_>> {
        let bytes = self.program.as_ref()?;
        Face::parse(bytes, 0).ok()
    }

    /// The font's name (PyMuPDF `Font.name`): the program's full-name record for
    /// a program-backed handle, else the canonical Core-14 key (e.g.
    /// `"Helvetica"`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.display_name
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

    /// The number of glyphs the font defines (PyMuPDF `Font.glyph_count`). For a
    /// program-backed handle this is the program's real glyph count; otherwise
    /// the Core-14 AFM glyph count.
    #[must_use]
    pub fn glyph_count(&self) -> u32 {
        match self.face() {
            Some(face) => u32::from(face.number_of_glyphs()),
            None => self.metrics.glyph_count,
        }
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
        // A program-backed handle answers from the program's real `cmap`.
        if let Some(face) = self.face() {
            return face.glyph_index(c).is_some();
        }
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

    /// The font's natural built-in encoding: WinAnsi for the text typefaces,
    /// the font's own encoding for the two pictographic families.
    fn base_encoding(&self) -> BaseEncoding {
        match self.std_name {
            "Symbol" => BaseEncoding::Symbol,
            "ZapfDingbats" => BaseEncoding::ZapfDingbats,
            _ => BaseEncoding::WinAnsi,
        }
    }

    /// Whether the font can be used to write text (PyMuPDF `Font.is_writable`).
    /// The Core-14 handles always render text, so this is always `true`.
    #[must_use]
    pub fn is_writable(&self) -> bool {
        true
    }

    /// The embedded font program (`/FontFile*`) bytes (PyMuPDF `Font.buffer`).
    ///
    /// A **program-backed** handle (built from `/FontFile*` or a user
    /// `fontfile=` / `fontbuffer=`) returns the program bytes. A metrics-only
    /// Core-14 handle carries no glyph-outline program (PyMuPDF substitutes a
    /// bundled NimbusSans/Type1 TTF and returns its bytes); with no program to
    /// expose, this returns `None` — a documented deviation.
    #[must_use]
    pub fn buffer(&self) -> Option<&[u8]> {
        self.program.as_deref()
    }

    /// The Unicode codepoints the font's encoding covers, sorted ascending
    /// (PyMuPDF `Font.valid_codepoints` — an array of ints).
    ///
    /// For a **program-backed** handle this is the program's real `cmap`
    /// coverage (every Unicode scalar the font's character map resolves to a
    /// glyph). For a metrics-only Core-14 handle it is derived from the natural
    /// built-in encoding (WinAnsi for text fonts; the font's own encoding for
    /// Symbol/ZapfDingbats), mapping each encoded glyph name back to its AGL
    /// Unicode scalar — an honest subset of PyMuPDF's bundled-cmap set.
    #[must_use]
    pub fn valid_codepoints(&self) -> Vec<u32> {
        if let Some(face) = self.face() {
            let mut cps: Vec<u32> = Vec::new();
            for sub in face.tables().cmap.into_iter().flat_map(|c| c.subtables) {
                if sub.is_unicode() {
                    sub.codepoints(|cp| {
                        if sub.glyph_index(cp).is_some() {
                            cps.push(cp);
                        }
                    });
                }
            }
            cps.sort_unstable();
            cps.dedup();
            return cps;
        }
        let table = self.base_encoding().table();
        let mut cps: Vec<u32> = table
            .iter()
            .filter_map(|slot| slot.as_ref())
            .filter_map(|name| {
                glyph_name_to_unicode(name).and_then(|s| s.chars().next().map(|c| c as u32))
            })
            .collect();
        cps.sort_unstable();
        cps.dedup();
        cps
    }

    /// The glyph bounding box for Unicode scalar `cp` at font size 1, as
    /// `(x0, y0, x1, y1)` in em units (PyMuPDF `Font.glyph_bbox(chr)`).
    ///
    /// A **program-backed** handle reports the glyph's real per-glyph ink box
    /// from the outlines (the program's `cmap` resolves the scalar to a glyph,
    /// whose outline bbox is scaled by `1/units_per_em`); a covered glyph with
    /// an empty outline (e.g. space) reports the empty box. A metrics-only
    /// Core-14 handle has no per-glyph outlines, so it returns the font-level
    /// bounding box for any covered scalar (a documented approximation). Either
    /// way an uncovered scalar reports the empty box.
    #[must_use]
    pub fn glyph_bbox(&self, cp: u32) -> (f64, f64, f64, f64) {
        if let Some(face) = self.face() {
            let Some(c) = char::from_u32(cp) else {
                return (0.0, 0.0, 0.0, 0.0);
            };
            let Some(gid) = face.glyph_index(c) else {
                return (0.0, 0.0, 0.0, 0.0);
            };
            return glyph_outline_bbox(&face, gid);
        }
        if self.has_glyph(cp) {
            self.metrics.bbox
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    }
}

/// The glyph's per-glyph ink box scaled to a unit em (`÷ units_per_em`), as
/// `(x0, y0, x1, y1)`. A glyph with no outline (whitespace) or no reported box
/// yields the empty box.
fn glyph_outline_bbox(face: &Face, gid: GlyphId) -> (f64, f64, f64, f64) {
    let upem = f64::from(face.units_per_em().max(1));
    match face.glyph_bounding_box(gid) {
        Some(bb) => (
            f64::from(bb.x_min) / upem,
            f64::from(bb.y_min) / upem,
            f64::from(bb.x_max) / upem,
            f64::from(bb.y_max) / upem,
        ),
        None => (0.0, 0.0, 0.0, 0.0),
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

    #[test]
    fn base14_fontnames_exact() {
        // Matches PyMuPDF `fitz.Base14_fontnames` exactly, in order.
        assert_eq!(BASE14_FONTNAMES.len(), 14);
        assert_eq!(BASE14_FONTNAMES[0], "Courier");
        assert_eq!(BASE14_FONTNAMES[4], "Helvetica");
        assert_eq!(BASE14_FONTNAMES[8], "Times-Roman");
        assert_eq!(BASE14_FONTNAMES[13], "ZapfDingbats");
    }

    #[test]
    fn is_writable_and_empty_buffer() {
        let f = Font::new("helv");
        assert!(f.is_writable());
        // Metrics-only handle carries no embedded program.
        assert!(f.buffer().is_none());
    }

    #[test]
    fn valid_codepoints_sorted_covering_ascii() {
        let f = Font::new("helv");
        let vc = f.valid_codepoints();
        assert!(!vc.is_empty());
        // Sorted, deduplicated.
        assert!(vc.windows(2).all(|w| w[0] < w[1]));
        assert!(vc.contains(&(' ' as u32)));
        assert!(vc.contains(&('A' as u32)));
        assert!(vc.contains(&0x00E9)); // eacute (WinAnsi)
                                       // Symbol uses its own encoding (no plain ASCII letters).
        let s = Font::new("symb");
        assert!(!s.valid_codepoints().is_empty());
    }

    #[test]
    fn glyph_bbox_covered_vs_uncovered() {
        let f = Font::new("helv");
        // Covered → the font-level bbox (documented approximation).
        assert_eq!(f.glyph_bbox('A' as u32), f.bbox());
        // Uncovered → empty box.
        assert_eq!(f.glyph_bbox(0x1F600), (0.0, 0.0, 0.0, 0.0));
    }

    // ----- program-backed handle (real /FontFile* / user fontfile=) --------

    /// A bundled, license-clean (SIL OFL 1.1) real TrueType program used to
    /// exercise the program-backed path without fetching an external asset.
    const LIBERATION_SANS: &[u8] = include_bytes!("../fonts/liberation/LiberationSans-Regular.ttf");

    #[test]
    fn from_program_carries_real_buffer_and_name() {
        let f = Font::from_program(LIBERATION_SANS, "helv").expect("real sfnt parses");
        // buffer() returns the exact program bytes (no silent Helvetica fallback).
        assert_eq!(f.buffer(), Some(LIBERATION_SANS));
        // name() is the program's full-name record, not the fallback "Helvetica".
        assert_eq!(f.name(), "Liberation Sans Regular");
        // glyph_count() is the program's real count (thousands), not the AFM 315.
        assert!(f.glyph_count() > 1000, "got {}", f.glyph_count());
    }

    #[test]
    fn from_program_glyph_bbox_is_real_per_glyph_outline() {
        let f = Font::from_program(LIBERATION_SANS, "helv").unwrap();
        // 'A' has a real ink box: positive width/height, ascender-ward top.
        let (x0, y0, x1, y1) = f.glyph_bbox('A' as u32);
        assert!(x1 > x0 && y1 > y0, "A bbox {:?}", (x0, y0, x1, y1));
        assert!(y1 > 0.5 && y0.abs() < 0.05, "A sits on the baseline");
        // Distinct glyphs have distinct ink boxes (not a constant font box):
        // 'g' (a descender) reaches below the baseline where 'A' does not.
        let (_, gy0, _, _) = f.glyph_bbox('g' as u32);
        assert!(
            gy0 < y0,
            "descender 'g' dips below 'A' (gy0={gy0}, ay0={y0})"
        );
        // A space glyph has no outline → empty box.
        assert_eq!(f.glyph_bbox(' ' as u32), (0.0, 0.0, 0.0, 0.0));
        // An uncovered scalar → empty box.
        assert_eq!(f.glyph_bbox(0x1F600), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn from_program_valid_codepoints_reflects_real_cmap() {
        let f = Font::from_program(LIBERATION_SANS, "helv").unwrap();
        let vc = f.valid_codepoints();
        // Sorted, de-duplicated, and far broader than the WinAnsi-encoding subset
        // (the bundled cmap covers thousands of scalars).
        assert!(vc.windows(2).all(|w| w[0] < w[1]));
        assert!(vc.len() > 1000, "got {}", vc.len());
        assert!(vc.contains(&('A' as u32)));
        assert!(vc.contains(&0x00E9)); // eacute
                                       // has_glyph agrees with the cmap.
        assert!(f.has_glyph('A' as u32));
        assert!(!f.has_glyph(0x1F600));
    }

    #[test]
    fn from_program_rejects_non_sfnt() {
        // A bare Type1 PFB / random bytes are not ttf-parser-parseable → None,
        // so the caller falls back to Font::new (no panic).
        assert!(Font::from_program(b"%!PS-AdobeFont not an sfnt", "helv").is_none());
        assert!(Font::from_program(&[0u8; 8], "helv").is_none());
    }
}
