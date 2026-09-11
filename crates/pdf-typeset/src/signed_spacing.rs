//! Font-dependent preparation for explicitly signed cluster gaps.
use crate::{Block, CharacterSpacing, ExportWarning, Run, TextBoxSpec, Typesetter};
use std::{borrow::Cow, fmt};

/// One step locating a paragraph within the caller's block model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpacingPathStep {
    /// Index in the current block list.
    Block(usize),
    /// Row index inside a table.
    Row(usize),
    /// Cell index inside a row.
    Cell(usize),
}
/// Why a negative-gap paragraph cannot use the bounded forward-placement path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignedSpacingReason {
    /// Effective size/metric or a derived coordinate cannot remain finite.
    NonFiniteEffectiveGeometry,
    /// Advance arithmetic is nonfinite or too close to underflow for stable scaling.
    NonFiniteAdvance,
    /// A cluster plus its gap does not have a safely positive forward advance.
    NonForwardAdvance,
}
/// A font/text-dependent spacing error, before returning any layout operations.
#[derive(Clone, Debug, PartialEq)]
pub struct SignedSpacingError {
    /// Structured path to the paragraph, including nested table cells.
    pub path: Vec<SpacingPathStep>,
    /// Run containing the cluster's base scalar.
    pub run_index: usize,
    /// UTF-8 offset of that base in the run text.
    pub byte_offset: usize,
    /// Machine-readable reason.
    pub reason: SignedSpacingReason,
}
impl fmt::Display for SignedSpacingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported signed spacing at {:?}, run {}, byte {}: {:?}",
            self.path, self.run_index, self.byte_offset, self.reason
        )
    }
}
impl std::error::Error for SignedSpacingError {}

pub(crate) fn has_negative(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(_, runs) => runs
            .iter()
            .any(|r| r.style.character_spacing.points() < 0.0),
        Block::Table(t) => t
            .rows
            .iter()
            .any(|r| r.cells.iter().any(|c| has_negative(&c.blocks))),
        _ => false,
    })
}

struct Cluster {
    run: usize,
    offset: usize,
    advance: f64,
    gap: f64,
}
fn cluster_reason(c: &Cluster) -> Option<SignedSpacingReason> {
    let sum = c.advance + c.gap;
    if !sum.is_finite() || (sum != 0.0 && !sum.is_normal()) {
        return Some(SignedSpacingReason::NonFiniteAdvance);
    }
    // Headroom covers rounding at all proportional autofit scales checked below.
    if c.gap < 0.0 && sum <= (c.advance.abs() + c.gap.abs()) * 64.0 * f64::EPSILON {
        return Some(SignedSpacingReason::NonForwardAdvance);
    }
    None
}
fn paragraph(
    ts: &mut Typesetter,
    runs: &[Run],
    path: &[SpacingPathStep],
    min_scale: f64,
) -> Option<SignedSpacingError> {
    if !runs
        .iter()
        .any(|r| r.style.character_spacing.points() < 0.0)
    {
        return None;
    }
    let error = |run_index, byte_offset, reason| SignedSpacingError {
        path: path.to_vec(),
        run_index,
        byte_offset,
        reason,
    };
    let mut cluster: Option<Cluster> = None;
    let mut bound = 0.0;
    for (ri, run) in runs.iter().enumerate() {
        // Legacy invalid nominal styles retain their pre-existing policy.
        if !run.style.size.is_finite() || run.style.size <= 0.0 {
            continue;
        }
        let Some((size, _)) = crate::flow::run_geometry(ts, run) else {
            return Some(error(
                ri,
                0,
                SignedSpacingReason::NonFiniteEffectiveGeometry,
            ));
        };
        let gap = run.style.character_spacing.points();
        if !(size * min_scale).is_normal() || (gap != 0.0 && !(gap * min_scale).is_normal()) {
            return Some(error(
                ri,
                0,
                SignedSpacingReason::NonFiniteEffectiveGeometry,
            ));
        }
        let base = ts.base_face(&run.style);
        for (offset, ch) in run.text.char_indices() {
            if !unicode_normalization::char::is_combining_mark(ch) {
                if let Some(c) = cluster.take() {
                    if let Some(reason) = cluster_reason(&c) {
                        return Some(error(c.run, c.offset, reason));
                    }
                }
            }
            if ch.is_control() {
                continue;
            }
            let draw = crate::flow::spacing_char(ch);
            let face = ts.char_face(&base, draw);
            let advance = ts.faces().advance(face, draw, size);
            bound += advance.abs() + run.style.character_spacing.points().abs() + size;
            if !advance.is_finite() || !bound.is_finite() || bound > f64::MAX / 4.0 {
                return Some(error(ri, offset, SignedSpacingReason::NonFiniteAdvance));
            }
            if let Some(c) = cluster.as_mut() {
                c.advance += advance;
            } else {
                cluster = Some(Cluster {
                    run: ri,
                    offset,
                    advance,
                    gap: run.style.character_spacing.points(),
                });
            }
        }
    }
    if let Some(mut c) = cluster {
        // Paragraph end has no following cluster: no signed gap is consumed.
        c.gap = 0.0;
        if let Some(reason) = cluster_reason(&c) {
            return Some(error(c.run, c.offset, reason));
        }
    }
    None
}
fn walk(
    ts: &mut Typesetter,
    blocks: &[Block],
    path: &mut Vec<SpacingPathStep>,
    errors: &mut Vec<SignedSpacingError>,
    min_scale: f64,
) {
    for (index, block) in blocks.iter().enumerate() {
        path.push(SpacingPathStep::Block(index));
        match block {
            Block::Paragraph(_, runs) => {
                if let Some(error) = paragraph(ts, runs, path, min_scale) {
                    errors.push(error);
                }
            }
            Block::Table(t) => {
                for (ri, row) in t.rows.iter().enumerate() {
                    path.push(SpacingPathStep::Row(ri));
                    for (ci, cell) in row.cells.iter().enumerate() {
                        path.push(SpacingPathStep::Cell(ci));
                        walk(ts, &cell.blocks, path, errors, min_scale);
                        path.pop();
                    }
                    path.pop();
                }
            }
            _ => {}
        }
        path.pop();
    }
}
pub(crate) fn check(ts: &mut Typesetter, blocks: &[Block]) -> Vec<SignedSpacingError> {
    let mut errors = Vec::new();
    if has_negative(blocks) {
        walk(ts, blocks, &mut Vec::new(), &mut errors, 1.0);
    }
    errors
}
pub(crate) fn check_box(ts: &mut Typesetter, spec: &TextBoxSpec) -> Vec<SignedSpacingError> {
    let mut errors = check(ts, &spec.blocks);
    if has_negative(&spec.blocks) {
        let scale = spec
            .font_scale
            .filter(|s| s.is_finite() && *s > 0.0 && *s <= 1.0)
            .unwrap_or(1.0);
        let minimum = if spec.font_scale.is_some() {
            crate::boxes::MIN_AUTOFIT_SCALE.min(scale)
        } else {
            1.0
        };
        let mut scaled_errors = Vec::new();
        walk(
            ts,
            &spec.blocks,
            &mut Vec::new(),
            &mut scaled_errors,
            minimum,
        );
        for e in scaled_errors {
            if !errors.iter().any(|old| old.path == e.path) {
                errors.push(e);
            }
        }
        // Advance, size and gap scale together. Normal, finite endpoints with a
        // rounding margin exclude cancellation/underflow throughout this interval.
        for s in [scale, minimum] {
            let scaled = crate::boxes::scale_blocks(&spec.blocks, s);
            for e in check(ts, &scaled) {
                if !errors.iter().any(|old| old.path == e.path) {
                    errors.push(e);
                }
            }
        }
    }
    errors
}
fn reset(blocks: &mut [Block], path: &mut Vec<SpacingPathStep>, errors: &[SignedSpacingError]) {
    for (i, block) in blocks.iter_mut().enumerate() {
        path.push(SpacingPathStep::Block(i));
        match block {
            Block::Paragraph(_, runs) if errors.iter().any(|e| e.path == *path) => {
                for run in runs {
                    if run.style.character_spacing.points() < 0.0 {
                        run.style.character_spacing = CharacterSpacing::default();
                    }
                }
            }
            Block::Table(t) => {
                for (ri, row) in t.rows.iter_mut().enumerate() {
                    path.push(SpacingPathStep::Row(ri));
                    for (ci, cell) in row.cells.iter_mut().enumerate() {
                        path.push(SpacingPathStep::Cell(ci));
                        reset(&mut cell.blocks, path, errors);
                        path.pop();
                    }
                    path.pop();
                }
            }
            _ => {}
        }
        path.pop();
    }
}
pub(crate) fn fallback<'a>(
    ts: &mut Typesetter,
    blocks: &'a [Block],
    errors: Vec<SignedSpacingError>,
) -> Cow<'a, [Block]> {
    if errors.is_empty() {
        return Cow::Borrowed(blocks);
    }
    let mut copy = blocks.to_vec();
    fallback_in_place(ts, &mut copy, errors);
    Cow::Owned(copy)
}

pub(crate) fn fallback_in_place(
    ts: &mut Typesetter,
    blocks: &mut [Block],
    errors: Vec<SignedSpacingError>,
) {
    reset(blocks, &mut Vec::new(), &errors);
    ts.warnings.extend(
        errors
            .into_iter()
            .map(|error| ExportWarning::SignedSpacingFallback { error }),
    );
}
