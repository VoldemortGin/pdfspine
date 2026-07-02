//! [`ExportWarning`] — the degradation channel (PRD §10 locked decision:
//! `ExportResult` carries pdf bytes/ops + `Vec<ExportWarning>`; every
//! unsupported-feature degradation is enumerated, degrade-never-panic).
//!
//! Consumers (docspine `doc-render` / pptspine `ppt-render`) surface these in
//! Python via `warnings.warn`, so every variant has a stable, human-readable
//! [`Display`](std::fmt::Display) rendering.

use std::fmt;

/// One enumerated export degradation. `#[non_exhaustive]`: later TS phases add
/// variants (growth is expected per the PRD §10 model sketch).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum ExportWarning {
    /// The requested font family was not installed; a substitution-table
    /// candidate or the bundled final fallback was used instead.
    FontSubstituted {
        /// The family the document asked for.
        requested: String,
        /// The family actually resolved.
        used: String,
    },
    /// The requested family misses a bold/italic face slot; the nearest
    /// existing face is used as-is (no synthetic bold in v1 — locked).
    StyleApproximated {
        /// The family whose style slot is missing.
        family: String,
        /// Whether bold was requested.
        bold: bool,
        /// Whether italic was requested.
        italic: bool,
    },
    /// A character the resolved face cannot draw fell through the per-char
    /// fallback chain (to another face, or — chain exhausted — to `.notdef`).
    GlyphFallback {
        /// The character that could not be drawn by its requested face.
        ch: char,
        /// The family that lacked the glyph.
        family: String,
    },
    /// An autoshape preset outside the v1 subset degraded to its bounding-box
    /// rectangle (text still laid out on top).
    PresetDegraded {
        /// The `prstGeom` preset name.
        preset: String,
    },
    /// A gradient fill degraded to a representative solid color (gradients are
    /// out for v1 — locked).
    GradientDegraded {
        /// The gradient kind (e.g. `linear`, `radial`).
        kind: String,
    },
    /// Text-box content overflowed a fixed rect with clipping enabled and was
    /// clipped away.
    BoxOverflowClipped {
        /// How far the content overflowed the box, in points.
        overflow_pt: f64,
    },
}

impl fmt::Display for ExportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportWarning::FontSubstituted { requested, used } => {
                write!(f, "font '{requested}' not available; substituted '{used}'")
            }
            ExportWarning::StyleApproximated {
                family,
                bold,
                italic,
            } => write!(
                f,
                "font '{family}' has no {} face; using the nearest style (no synthetic bold)",
                style_name(*bold, *italic)
            ),
            ExportWarning::GlyphFallback { ch, family } => write!(
                f,
                "font '{family}' has no glyph for {ch:?} (U+{:04X}); using fallback",
                u32::from(*ch)
            ),
            ExportWarning::PresetDegraded { preset } => {
                write!(
                    f,
                    "unsupported shape preset '{preset}'; drawn as its bounding box"
                )
            }
            ExportWarning::GradientDegraded { kind } => {
                write!(f, "{kind} gradient fill degraded to a solid color")
            }
            ExportWarning::BoxOverflowClipped { overflow_pt } => {
                write!(f, "text box content clipped ({overflow_pt:.2} pt overflow)")
            }
        }
    }
}

/// Human name of a bold/italic combination for warning text.
fn style_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "bold-italic",
        (true, false) => "bold",
        (false, true) => "italic",
        (false, false) => "regular",
    }
}
