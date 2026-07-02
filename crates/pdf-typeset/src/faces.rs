//! The [`FaceId`] registry (TS-3/TS-4 bookkeeping): mints one [`FaceId`] per
//! distinct resolved face (deduplicated on [`FaceKey`]), parses each face's
//! program **once** per export run (`EmbeddedFont::parse_indexed` at the
//! face's TTC index), and carries the real font metrics that retire the
//! `BASELINE_FACTOR = 0.8` heuristic (PRD §10 TRAP).
//!
//! Measurement and drawing share this registry, so line breaks always match
//! the drawn glyphs (the pdf-markdown invariant, generalized to N faces).

use std::cell::RefCell;
use std::collections::HashMap;

use pdf_edit::EmbeddedFont;
use pdf_fonts::liberation::{liberation_face, LiberationFamily};

use crate::fontres::{FaceKey, FontResolver, ResolvedFace};
use crate::ops::FaceId;

/// Fallback underline position when the font carries no `post` metrics,
/// as a fraction of the em (negative = below the baseline).
const DEFAULT_UNDERLINE_POSITION: f64 = -0.1;
/// Fallback underline / strikeout thickness, × em.
const DEFAULT_LINE_THICKNESS: f64 = 0.05;
/// Fallback strikeout raise above the baseline, × em (the proven
/// pdf-markdown `STRIKE_RAISE`).
const DEFAULT_STRIKEOUT_POSITION: f64 = 0.28;

/// Vertical metrics of one face, as fractions of the em (multiply by the font
/// size in points). Sourced from the face's `hhea` / `post` / `OS/2` tables.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FaceMetrics {
    /// Ascender above the baseline (positive).
    pub ascent: f64,
    /// Descender below the baseline (positive magnitude).
    pub descent: f64,
    /// External leading between consecutive lines (non-negative).
    pub line_gap: f64,
    /// Underline center relative to the baseline (negative = below).
    pub underline_position: f64,
    /// Underline stroke thickness.
    pub underline_thickness: f64,
    /// Strikeout center above the baseline (positive).
    pub strikeout_position: f64,
    /// Strikeout stroke thickness.
    pub strikeout_thickness: f64,
}

impl FaceMetrics {
    /// Reads the metrics of a parsed face, filling gaps with the documented
    /// defaults.
    fn from_face(face: &ttf_parser::Face<'_>) -> Self {
        let upem = f64::from(face.units_per_em());
        let scale = if upem > 0.0 { 1.0 / upem } else { 1.0 };
        let underline = face.underline_metrics();
        let strikeout = face.strikeout_metrics();
        FaceMetrics {
            ascent: f64::from(face.ascender()) * scale,
            descent: f64::from(face.descender()).abs() * scale,
            line_gap: f64::from(face.line_gap()).max(0.0) * scale,
            underline_position: underline.map_or(DEFAULT_UNDERLINE_POSITION, |m| {
                f64::from(m.position) * scale
            }),
            underline_thickness: underline
                .map_or(DEFAULT_LINE_THICKNESS, |m| f64::from(m.thickness) * scale),
            strikeout_position: strikeout.map_or(DEFAULT_STRIKEOUT_POSITION, |m| {
                f64::from(m.position) * scale
            }),
            strikeout_thickness: strikeout
                .map_or(DEFAULT_LINE_THICKNESS, |m| f64::from(m.thickness) * scale),
        }
    }

    /// The natural (single-spaced) line height at `size` points:
    /// ascent + descent + line gap.
    #[must_use]
    pub fn line_height(&self, size: f64) -> f64 {
        (self.ascent + self.descent + self.line_gap) * size
    }
}

/// One registered face: the embeddable font program (parsed once), its
/// metrics, and a per-char glyph-ID memo shared by measurement and emission.
struct Entry {
    font: EmbeddedFont,
    metrics: FaceMetrics,
    gids: RefCell<HashMap<char, u16>>,
}

/// The per-export-run face registry: [`FaceKey`] → [`FaceId`] deduplication
/// plus everything layout ([`FaceRegistry::advance`], [`FaceRegistry::metrics`])
/// and emission ([`FaceRegistry::font`], [`FaceRegistry::gid`]) need.
pub(crate) struct FaceRegistry {
    ids: HashMap<FaceKey, FaceId>,
    faces: Vec<Entry>,
    /// Debug flag forwarded to every `EmbeddedFont` (whole-program embed).
    full_embed: bool,
}

impl FaceRegistry {
    /// An empty registry.
    pub(crate) fn new() -> Self {
        FaceRegistry {
            ids: HashMap::new(),
            faces: Vec::new(),
            full_embed: false,
        }
    }

    /// The number of registered faces.
    pub(crate) fn len(&self) -> usize {
        self.faces.len()
    }

    /// Sets the whole-program-embed debug flag on every registered (and every
    /// future) face.
    pub(crate) fn set_full_embed(&mut self, full_embed: bool) {
        self.full_embed = full_embed;
        for entry in &mut self.faces {
            entry.font.set_full_embed(full_embed);
        }
    }

    /// Interns a resolved face, minting a new [`FaceId`] on first sight.
    /// Total: an unloadable face program (unreachable for resolver-minted
    /// keys) degrades to the bundled Liberation Sans — never an error.
    pub(crate) fn intern(&mut self, resolver: &FontResolver, face: &ResolvedFace) -> FaceId {
        if let Some(&id) = self.ids.get(&face.key()) {
            return id;
        }
        let entry = load(resolver, face, self.full_embed).unwrap_or_else(|| {
            let bytes = liberation_face(LiberationFamily::Sans, false, false);
            let mut font = EmbeddedFont::parse(bytes)
                .expect("bundled Liberation Sans parses (build-time asset invariant)");
            font.set_full_embed(self.full_embed);
            let parsed = ttf_parser::Face::parse(bytes, 0)
                .expect("bundled Liberation Sans parses (build-time asset invariant)");
            Entry {
                metrics: FaceMetrics::from_face(&parsed),
                font,
                gids: RefCell::new(HashMap::new()),
            }
        });
        let id = FaceId(self.faces.len());
        self.faces.push(entry);
        self.ids.insert(face.key(), id);
        id
    }

    /// The embeddable font of one face (for `write_type0`).
    pub(crate) fn font(&self, id: FaceId) -> &EmbeddedFont {
        &self.faces[id.0].font
    }

    /// The vertical metrics of one face.
    pub(crate) fn metrics(&self, id: FaceId) -> &FaceMetrics {
        &self.faces[id.0].metrics
    }

    /// The glyph ID of `ch` on `id` (0 = `.notdef`), memoized.
    pub(crate) fn gid(&self, id: FaceId, ch: char) -> u16 {
        let entry = &self.faces[id.0];
        if let Some(&gid) = entry.gids.borrow().get(&ch) {
            return gid;
        }
        let gid = entry.font.glyph_id(ch);
        entry.gids.borrow_mut().insert(ch, gid);
        gid
    }

    /// The advance of `ch` on `id` at `size` points (`.notdef` advance for
    /// uncovered chars — degrade, never panic).
    pub(crate) fn advance(&self, id: FaceId, ch: char, size: f64) -> f64 {
        self.faces[id.0].font.advance(self.gid(id, ch)) * size / 1000.0
    }
}

/// Loads one resolved face into an [`Entry`]: whole-program bytes from the
/// resolver, `EmbeddedFont` parsed at the face's TTC index, metrics read from
/// the same parse.
fn load(resolver: &FontResolver, face: &ResolvedFace, full_embed: bool) -> Option<Entry> {
    let data = resolver.face_data(face)?;
    let mut font = EmbeddedFont::parse_indexed(&data, face.index).ok()?;
    font.set_full_embed(full_embed);
    let parsed = ttf_parser::Face::parse(&data, face.index).ok()?;
    Some(Entry {
        metrics: FaceMetrics::from_face(&parsed),
        font,
        gids: RefCell::new(HashMap::new()),
    })
}
