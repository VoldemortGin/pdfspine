use super::Rect;

/// Page orientation for [`paper_size`] / [`paper_rect`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PaperOrientation {
    /// Portrait: width <= height (the table's natural orientation).
    #[default]
    Portrait,
    /// Landscape: width and height swapped.
    Landscape,
}

/// Paper dimensions in points (1/72 inch), portrait orientation, matching
/// PyMuPDF `paper_size`. Names are matched case-insensitively.
const PAPERS: &[(&str, u32, u32)] = &[
    ("a0", 2384, 3370),
    ("a1", 1684, 2384),
    ("a2", 1191, 1684),
    ("a3", 842, 1191),
    ("a4", 595, 842),
    ("a5", 420, 595),
    ("a6", 298, 420),
    ("b4", 729, 1032),
    ("b5", 516, 729),
    ("letter", 612, 792),
    ("legal", 612, 1008),
    ("tabloid", 792, 1224),
    ("ledger", 1224, 792),
    ("executive", 522, 756),
];

/// Returns the `(width, height)` in points for a named paper size, or `None`
/// if the name is unknown. The name may carry a `-l`/`-landscape` suffix, e.g.
/// `"a4-l"`, to request landscape orientation (PyMuPDF convention).
#[must_use]
pub fn paper_size(name: &str) -> Option<(u32, u32)> {
    let lower = name.trim().to_ascii_lowercase();
    let (base, landscape) = if let Some(stripped) = lower
        .strip_suffix("-l")
        .or_else(|| lower.strip_suffix("-landscape"))
    {
        (stripped, true)
    } else {
        (lower.as_str(), false)
    };

    PAPERS.iter().find(|(n, _, _)| *n == base).map(
        |&(_, w, h)| {
            if landscape {
                (h, w)
            } else {
                (w, h)
            }
        },
    )
}

/// Returns a [`Rect`] at the origin sized for a named paper size, or `None` if
/// the name is unknown (PyMuPDF `paper_rect`).
#[must_use]
pub fn paper_rect(name: &str) -> Option<Rect> {
    paper_size(name).map(|(w, h)| Rect::new(0.0, 0.0, f64::from(w), f64::from(h)))
}

/// Iterates over the known paper sizes as `(name, width, height)` in portrait.
pub fn paper_sizes() -> impl Iterator<Item = (&'static str, u32, u32)> {
    PAPERS.iter().copied()
}
