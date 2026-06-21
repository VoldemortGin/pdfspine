//! Glyph advance widths in 1000-unit text space (PRD §8.5; ISO 32000-1 §9.2.4 /
//! §9.7.4.3). All widths are defensively clamped: NaN / negative / absurd values
//! collapse to `0`, and indices never go out of bounds.

use std::collections::HashMap;

use pdf_core::Object;

/// The notdef / fallback width when nothing else is known. Real glyphs use the
/// font's metrics; this is only reached for genuinely unmapped codes.
pub const NOTDEF_WIDTH: f64 = 0.0;

/// An absurd-width ceiling: a single glyph wider than this in 1000-space is
/// taken as corrupt and clamped to 0 (PRD §8.5 defensive contract).
const MAX_SANE_WIDTH: f64 = 100_000.0;

/// Sanitizes a raw width: non-finite, negative, or absurd → `0`.
#[must_use]
pub fn sanitize(w: f64) -> f64 {
    if !w.is_finite() || !(0.0..=MAX_SANE_WIDTH).contains(&w) {
        0.0
    } else {
        w
    }
}

/// Sanitizes a *signed* metric (vertical displacement / position component,
/// which is routinely negative): non-finite or absurd-magnitude → `0`.
#[must_use]
fn sanitize_signed(v: f64) -> f64 {
    if !v.is_finite() || v.abs() > MAX_SANE_WIDTH {
        0.0
    } else {
        v
    }
}

/// Simple-font width table: `/Widths` indexed by `code - /FirstChar`, with a
/// `/MissingWidth` fallback for out-of-range codes (ISO 32000-1 §9.2.4).
#[derive(Clone, Debug, Default)]
pub struct SimpleWidths {
    first_char: u32,
    widths: Vec<f64>,
    missing: f64,
}

impl SimpleWidths {
    /// Builds from already-resolved `/FirstChar`, `/Widths` and `/MissingWidth`.
    /// `widths` is the raw object array; each entry is sanitized.
    #[must_use]
    pub fn new(first_char: u32, widths: &[Object], missing: f64) -> Self {
        let widths = widths
            .iter()
            .map(|o| sanitize(o.as_f64().unwrap_or(0.0)))
            .collect();
        SimpleWidths {
            first_char,
            widths,
            missing: sanitize(missing),
        }
    }

    /// The width for `code`, or the `/MissingWidth` fallback.
    #[must_use]
    pub fn width(&self, code: u32) -> f64 {
        if code < self.first_char {
            return self.missing;
        }
        let idx = (code - self.first_char) as usize;
        match self.widths.get(idx) {
            Some(&w) => w,
            None => self.missing,
        }
    }

    /// The `/MissingWidth` fallback in isolation (used when there is no
    /// `/Widths` array at all but a descriptor `/MissingWidth` exists).
    #[must_use]
    pub fn missing(&self) -> f64 {
        self.missing
    }
}

/// Type0 CID width table from `/W` plus the `/DW` default (ISO 32000-1
/// §9.7.4.3). `/W` has two interleaved forms:
///
/// - `c [w0 w1 …]` — CIDs `c, c+1, …` get `w0, w1, …`.
/// - `c_first c_last w` — CIDs `c_first..=c_last` all get `w`.
#[derive(Clone, Debug)]
pub struct CidWidths {
    individual: HashMap<u32, f64>,
    ranges: Vec<(u32, u32, f64)>,
    dw: f64,
}

impl Default for CidWidths {
    fn default() -> Self {
        CidWidths {
            individual: HashMap::new(),
            ranges: Vec::new(),
            dw: 1000.0,
        }
    }
}

impl CidWidths {
    /// Parses a `/W` array (already resolved to direct objects) with the given
    /// `/DW` default (`None` → the spec default of 1000).
    #[must_use]
    pub fn new(w: &[Object], dw: Option<f64>) -> Self {
        let mut cw = CidWidths {
            dw: dw.map(sanitize).unwrap_or(1000.0),
            ..Default::default()
        };
        let mut i = 0;
        while i < w.len() {
            let Some(c) = w[i].as_f64().map(|v| v as i64) else {
                i += 1;
                continue;
            };
            match w.get(i + 1) {
                // Array form: `c [w0 w1 …]`.
                Some(Object::Array(list)) => {
                    if c >= 0 {
                        let mut cid = c as u32;
                        for item in list {
                            cw.individual
                                .insert(cid, sanitize(item.as_f64().unwrap_or(0.0)));
                            cid = cid.wrapping_add(1);
                        }
                    }
                    i += 2;
                }
                // Range form: `c_first c_last w`.
                Some(obj_last) => {
                    let last = obj_last.as_f64().map(|v| v as i64);
                    let wv = w.get(i + 2).and_then(Object::as_f64);
                    if let (Some(last), Some(wv)) = (last, wv) {
                        if c >= 0 && last >= c {
                            cw.ranges.push((c as u32, last as u32, sanitize(wv)));
                        }
                        i += 3;
                    } else {
                        i += 1;
                    }
                }
                None => i += 1,
            }
        }
        cw
    }

    /// The width for `cid`: individual entry, then a covering range, then `/DW`.
    /// Later ranges win (last-wins, matching the array-append order).
    #[must_use]
    pub fn width(&self, cid: u32) -> f64 {
        if let Some(&w) = self.individual.get(&cid) {
            return w;
        }
        for &(lo, hi, w) in self.ranges.iter().rev() {
            if cid >= lo && cid <= hi {
                return w;
            }
        }
        self.dw
    }

    /// The `/DW` default.
    #[must_use]
    pub fn default_width(&self) -> f64 {
        self.dw
    }
}

/// Type0 vertical metrics from `/W2` plus the `/DW2` default (ISO 32000-1
/// §9.7.4.3). For a CID the vertical glyph carries:
///
/// - a **position vector** `v = (vx, vy)` (1000-unit text space) giving the
///   offset from the horizontal glyph origin to the vertical glyph origin, and
/// - a **vertical displacement** `w1y` (the advance along −y; usually negative).
///
/// `/DW2` is `[vy w1y]` and supplies the defaults `vx = w0/2`, `vy`, `w1y`.
/// `/W2` overrides per CID with two interleaved forms:
///
/// - `c [w1y_0 vx_0 vy_0  w1y_1 vx_1 vy_1 …]` — CIDs `c, c+1, …`.
/// - `c_first c_last w1y vx vy` — CIDs `c_first..=c_last` all share the triple.
#[derive(Clone, Debug)]
pub struct VerticalMetrics {
    /// Per-CID `(w1y, vx, vy)` overrides.
    individual: HashMap<u32, (f64, f64, f64)>,
    /// Range `(lo, hi, w1y, vx, vy)` overrides.
    ranges: Vec<(u32, u32, f64, f64, f64)>,
    /// `/DW2[0]` — the default position-vector y (text space; default 880).
    default_vy: f64,
    /// `/DW2[1]` — the default vertical displacement w1y (default −1000).
    default_w1y: f64,
}

impl Default for VerticalMetrics {
    fn default() -> Self {
        VerticalMetrics {
            individual: HashMap::new(),
            ranges: Vec::new(),
            default_vy: 880.0,
            default_w1y: -1000.0,
        }
    }
}

impl VerticalMetrics {
    /// Parses a `/W2` array (already resolved to direct objects) with the given
    /// `/DW2` pair (`None` → the spec defaults `[880 -1000]`).
    #[must_use]
    pub fn new(w2: &[Object], dw2: Option<(f64, f64)>) -> Self {
        let mut vm = VerticalMetrics::default();
        if let Some((vy, w1y)) = dw2 {
            vm.default_vy = sanitize_signed(vy);
            vm.default_w1y = sanitize_signed(w1y);
        }
        let mut i = 0;
        while i < w2.len() {
            let Some(c) = w2[i].as_f64().map(|v| v as i64) else {
                i += 1;
                continue;
            };
            match w2.get(i + 1) {
                // Array form: `c [w1y vx vy  w1y vx vy …]`.
                Some(Object::Array(list)) => {
                    if c >= 0 {
                        let mut cid = c as u32;
                        for triple in list.chunks_exact(3) {
                            let w1y = sanitize_signed(triple[0].as_f64().unwrap_or(0.0));
                            let vx = sanitize_signed(triple[1].as_f64().unwrap_or(0.0));
                            let vy = sanitize_signed(triple[2].as_f64().unwrap_or(0.0));
                            vm.individual.insert(cid, (w1y, vx, vy));
                            cid = cid.wrapping_add(1);
                        }
                    }
                    i += 2;
                }
                // Range form: `c_first c_last w1y vx vy`.
                Some(obj_last) => {
                    let last = obj_last.as_f64().map(|v| v as i64);
                    let w1y = w2.get(i + 2).and_then(Object::as_f64);
                    let vx = w2.get(i + 3).and_then(Object::as_f64);
                    let vy = w2.get(i + 4).and_then(Object::as_f64);
                    if let (Some(last), Some(w1y), Some(vx), Some(vy)) = (last, w1y, vx, vy) {
                        if c >= 0 && last >= c {
                            vm.ranges.push((
                                c as u32,
                                last as u32,
                                sanitize_signed(w1y),
                                sanitize_signed(vx),
                                sanitize_signed(vy),
                            ));
                        }
                        i += 5;
                    } else {
                        i += 1;
                    }
                }
                None => i += 1,
            }
        }
        vm
    }

    /// The position vector `v = (vx, vy)` for `cid` (1000-unit text space).
    /// `w0` is the CID's horizontal advance, used for the default `vx = w0/2`.
    #[must_use]
    pub fn position(&self, cid: u32, w0: f64) -> (f64, f64) {
        if let Some(&(_, vx, vy)) = self.individual.get(&cid) {
            return (vx, vy);
        }
        for &(lo, hi, _, vx, vy) in self.ranges.iter().rev() {
            if cid >= lo && cid <= hi {
                return (vx, vy);
            }
        }
        (w0 / 2.0, self.default_vy)
    }

    /// The vertical displacement `w1y` for `cid` (1000-unit text space; the
    /// advance along −y, normally negative).
    #[must_use]
    pub fn displacement(&self, cid: u32) -> f64 {
        if let Some(&(w1y, _, _)) = self.individual.get(&cid) {
            return w1y;
        }
        for &(lo, hi, w1y, _, _) in self.ranges.iter().rev() {
            if cid >= lo && cid <= hi {
                return w1y;
            }
        }
        self.default_w1y
    }
}

// --- Core-14 AFM metrics framework (PRD §6.5 #2 / §8.5.2) ------------------

/// Normalizes a `/BaseFont` name to one of the 14 standard font keys, stripping
/// a subset tag (`ABCDEF+`) and matching the documented aliases (`Arial` →
/// Helvetica, etc.). Returns `None` for a non-standard font.
///
/// This is the lookup hook for Core-14 AFM widths: the normalized key feeds
/// [`core14_width`], which resolves against the built-in factual advance-width
/// table ([`crate::std_widths`]). A base-14 simple font lacking a `/Widths`
/// array gets these advances during text extraction.
#[must_use]
pub fn normalize_standard_font(base_font: &str) -> Option<&'static str> {
    // Drop a `TAG+` subset prefix.
    let name = base_font.rsplit('+').next().unwrap_or(base_font);
    // Collapse style fragments to the canonical 14 keys.
    let lower = name.to_ascii_lowercase();
    let is = |needle: &str| lower.contains(needle);

    let serif = is("times") || is("serif");
    let courier = is("courier") || is("mono");
    let symbol = is("symbol");
    let zapf = is("zapf") || is("dingbat");
    let bold = is("bold");
    let italic = is("italic") || is("oblique");

    if symbol {
        return Some("Symbol");
    }
    if zapf {
        return Some("ZapfDingbats");
    }
    if courier {
        return Some(match (bold, italic) {
            (true, true) => "Courier-BoldOblique",
            (true, false) => "Courier-Bold",
            (false, true) => "Courier-Oblique",
            (false, false) => "Courier",
        });
    }
    if serif {
        return Some(match (bold, italic) {
            (true, true) => "Times-BoldItalic",
            (true, false) => "Times-Bold",
            (false, true) => "Times-Italic",
            (false, false) => "Times-Roman",
        });
    }
    // Default sans family (Helvetica / Arial).
    if is("helvetica") || is("arial") || is("sans") {
        return Some(match (bold, italic) {
            (true, true) => "Helvetica-BoldOblique",
            (true, false) => "Helvetica-Bold",
            (false, true) => "Helvetica-Oblique",
            (false, false) => "Helvetica",
        });
    }
    None
}

/// The Core-14 AFM width for `glyph_name` in the normalized standard font
/// `std_name` (one of the 14 canonical keys, e.g. as returned by
/// [`normalize_standard_font`]). Returns `None` for an unknown font key or a
/// glyph name the font has no metric for.
///
/// This resolves against the built-in factual advance-width table
/// ([`crate::std_widths`]). The 12 text fonts carry full per-glyph metrics; the
/// two pictographic fonts (Symbol, ZapfDingbats) report their flat default for
/// any glyph (they are not WinAnsi-named).
#[must_use]
pub fn core14_width(std_name: &str, glyph_name: &str) -> Option<f64> {
    let table = crate::std_widths::standard_font_widths(std_name)?;
    table.glyph_advance(glyph_name).or_else(|| match std_name {
        // Symbol / ZapfDingbats have no WinAnsi glyph names; any glyph they
        // do carry advances at the flat default.
        "Symbol" | "ZapfDingbats" => Some(table.default_width()),
        _ => None,
    })
}
