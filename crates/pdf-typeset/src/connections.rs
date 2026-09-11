//! Per-call, structural paragraph-boundary connections. No parser presence policy.
use crate::{Block, BorderEdge, ParaProps, Run, SignedSpacingError, SpacingPathStep, Typesetter};
use std::fmt;

/// A structural step shared with the existing signed-spacing diagnostics.
pub type BlockPathStep = SpacingPathStep;
/// Indices into the current block tree, including table rows and cells.
/// Callers must update these indices when editing or reordering their model.
pub type BlockPath = Vec<BlockPathStep>;

/// One caller-resolved boundary: replace `from`'s final bottom and `to`'s first
/// top by one solid separator at the incoming paragraph. No extra clearance.
#[derive(Clone, Debug, PartialEq)]
pub struct ParagraphConnection {
    from: BlockPath,
    to: BlockPath,
    separator: BorderEdge,
}
impl ParagraphConnection {
    /// Constructs a solid RGB separator. Paths are checked on every layout call.
    ///
    /// # Errors
    /// Rejects nonpositive/nonfinite widths and nonfinite/out-of-range RGB.
    pub fn new(
        from: BlockPath,
        to: BlockPath,
        separator: BorderEdge,
    ) -> Result<Self, ConnectionError> {
        let emitted_width = crate::ops::pdf_scalar(separator.width)
            .parse::<f32>()
            .unwrap_or(0.0);
        if !emitted_width.is_finite()
            || emitted_width <= 0.0
            || !separator.width.is_finite()
            || separator.width <= 0.0
            || [separator.color.r, separator.color.g, separator.color.b]
                .iter()
                .any(|c| !c.is_finite() || !(0.0..=1.0).contains(c))
        {
            return Err(ConnectionError {
                path: to,
                reason: ConnectionReason::InvalidSeparator,
            });
        }
        Ok(Self {
            from,
            to,
            separator,
        })
    }
    /// Source paragraph path in the current model.
    #[must_use]
    pub fn from(&self) -> &[BlockPathStep] {
        &self.from
    }
    /// Incoming paragraph path in the current model.
    #[must_use]
    pub fn to(&self) -> &[BlockPathStep] {
        &self.to
    }
    /// Validated solid stroke; its width is not scaled by text autofit.
    #[must_use]
    pub fn separator(&self) -> BorderEdge {
        self.separator
    }
}

/// Read-only layout overlay, revalidated against the supplied blocks each call.
/// This is not a prepared/cached document or an OOXML border resolver.
///
/// Endpoints must be nonempty, layout-usable sibling paragraphs, directly
/// adjacent or separated by exactly one `Block::PageBreak`, with equal effective
/// horizontal borders. Images, tables and cell boundaries are not transparent.
/// A connection suppresses only the source paragraph's final bottom and replaces
/// only the incoming paragraph's first opening; internal page continuations keep
/// their ordinary borders. Sides keep each paragraph's own style without bridging
/// an explicit boundary's gap. Separators have zero extra clearance and are solid.
///
/// A connected endpoint moved before its first line is rewrapped at the new page
/// geometry. Once any text of that endpoint has been emitted, a change of effective
/// page width returns [`ConnectionReason::ChangingParagraphGeometry`]; equal-width
/// origin changes are supported. This does not add arbitrary variable-width
/// paragraph reflow to the existing engine. Unconnected paragraphs keep the old
/// behavior even when another paragraph in the same call is connected.
/// Static checks precede page-provider calls. Page-dependent errors can occur
/// after those calls; no partial operations are returned and provider side effects
/// are not rolled back. Font caches and warnings may be warmed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphConnections {
    connections: Vec<ParagraphConnection>,
}
impl ParagraphConnections {
    /// Owns the explicit connections. Structural validation occurs at use time.
    #[must_use]
    pub fn new(connections: Vec<ParagraphConnection>) -> Self {
        Self { connections }
    }
    /// All requested boundaries, in caller order.
    #[must_use]
    pub fn connections(&self) -> &[ParagraphConnection] {
        &self.connections
    }
    /// Empty overlays delegate directly to the existing checked layout APIs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

/// Why a checked paragraph connection cannot be represented.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionReason {
    /// Separator width or RGB is outside its finite supported range.
    InvalidSeparator,
    /// A structural step or index does not resolve in the current block tree.
    InvalidPath,
    /// A resolved endpoint is not a paragraph.
    NotParagraph,
    /// Two separators target the same incoming paragraph.
    DuplicateIncoming,
    /// Endpoints belong to different sibling lists (including different cells).
    DifferentContainer,
    /// Endpoints are not consecutive, optionally separated by one PageBreak.
    NotAdjacent,
    /// Effective left/right paragraph boundaries are unequal or nonfinite.
    HorizontalBounds,
    /// An endpoint has no non-whitespace text with layout-usable run geometry.
    NoUsableText,
    /// A page-dependent opening/line/closing requirement cannot fit safely.
    UnusableGeometry,
    /// A connected paragraph has already emitted text before its page width changes.
    ChangingParagraphGeometry,
}
/// A connection diagnostic bound to a path in the supplied model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionError {
    /// Offending endpoint (incoming endpoint for boundary-wide failures).
    pub path: BlockPath,
    /// Machine-readable cause.
    pub reason: ConnectionReason,
}
impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported paragraph connection at {:?}: {:?}",
            self.path, self.reason
        )
    }
}
impl std::error::Error for ConnectionError {}

/// Errors of the additive checked connection APIs. Existing checked methods
/// continue to return `SignedSpacingError` directly.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutError {
    /// Existing caller-resolved signed-spacing preparation failed.
    SignedSpacing(SignedSpacingError),
    /// Structural or geometry validation of a connection failed.
    Connection(ConnectionError),
}
impl From<SignedSpacingError> for LayoutError {
    fn from(error: SignedSpacingError) -> Self {
        Self::SignedSpacing(error)
    }
}
impl From<ConnectionError> for LayoutError {
    fn from(error: ConnectionError) -> Self {
        Self::Connection(error)
    }
}
impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedSpacing(e) => e.fmt(f),
            Self::Connection(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::SignedSpacing(e) => e,
            Self::Connection(e) => e,
        })
    }
}

struct Endpoint<'a> {
    props: &'a ParaProps,
    runs: &'a [Run],
    siblings: &'a [Block],
    index: usize,
}
fn resolve<'a>(
    blocks: &'a [Block],
    path: &[BlockPathStep],
) -> Result<Endpoint<'a>, ConnectionReason> {
    let Some((BlockPathStep::Block(index), tail)) = path.split_first() else {
        return Err(ConnectionReason::InvalidPath);
    };
    let block = blocks.get(*index).ok_or(ConnectionReason::InvalidPath)?;
    if tail.is_empty() {
        return match block {
            Block::Paragraph(props, runs) => Ok(Endpoint {
                props,
                runs,
                siblings: blocks,
                index: *index,
            }),
            _ => Err(ConnectionReason::NotParagraph),
        };
    }
    let (Block::Table(table), [BlockPathStep::Row(row), BlockPathStep::Cell(cell), rest @ ..]) =
        (block, tail)
    else {
        return Err(ConnectionReason::InvalidPath);
    };
    let cell = table
        .rows
        .get(*row)
        .and_then(|r| r.cells.get(*cell))
        .ok_or(ConnectionReason::InvalidPath)?;
    // Table layout ignores cells beyond its declared columns: do not accept an
    // association that would pass preflight and then never be consumed.
    if let [_, BlockPathStep::Cell(c), ..] = tail {
        if *c >= table.columns.len() {
            return Err(ConnectionReason::InvalidPath);
        }
    }
    resolve(&cell.blocks, rest)
}

pub(crate) fn validate(
    ts: &mut Typesetter,
    blocks: &[Block],
    overlay: &ParagraphConnections,
) -> Result<(), ConnectionError> {
    for (i, c) in overlay.connections.iter().enumerate() {
        let error = |reason| ConnectionError {
            path: c.to.clone(),
            reason,
        };
        if overlay.connections[..i]
            .iter()
            .any(|previous| previous.to == c.to)
        {
            return Err(error(ConnectionReason::DuplicateIncoming));
        }
        let a = resolve(blocks, &c.from).map_err(|reason| ConnectionError {
            path: c.from.clone(),
            reason,
        })?;
        let b = resolve(blocks, &c.to).map_err(error)?;
        if c.from[..c.from.len() - 1] != c.to[..c.to.len() - 1] {
            return Err(error(ConnectionReason::DifferentContainer));
        }
        if b.index != a.index + 1
            && !(b.index == a.index + 2
                && matches!(a.siblings.get(a.index + 1), Some(Block::PageBreak)))
        {
            return Err(error(ConnectionReason::NotAdjacent));
        }
        let bounds = |p: &ParaProps| (crate::flow::border_left(p), p.indent_right.max(0.0));
        let (left, right) = bounds(a.props);
        if !left.is_finite() || !right.is_finite() || bounds(a.props) != bounds(b.props) {
            return Err(error(ConnectionReason::HorizontalBounds));
        }
        for (endpoint, path) in [(&a, &c.from), (&b, &c.to)] {
            if !endpoint.runs.iter().any(|r| {
                r.text
                    .chars()
                    .any(|ch| !ch.is_whitespace() && !ch.is_control())
                    && crate::flow::run_geometry(ts, r).is_some()
            }) {
                return Err(ConnectionError {
                    path: path.clone(),
                    reason: ConnectionReason::NoUsableText,
                });
            }
        }
    }
    Ok(())
}

/// Internal transition for one paragraph, built from structural indices for
/// this invocation (also used unchanged with structure-preserving autofit clones).
#[derive(Clone, Debug, Default)]
pub(crate) struct Transition {
    pub(crate) incoming: Option<crate::ParagraphBorder>,
    pub(crate) outgoing: bool,
    path: BlockPath,
    // (left text inset A/B, shared border-left inset, shared right inset).
    bounds: Vec<(f64, f64, f64, f64)>,
}
impl Transition {
    pub(crate) fn active(&self) -> bool {
        self.incoming.is_some() || self.outgoing
    }
    pub(crate) fn error(&self) -> ConnectionError {
        ConnectionError {
            path: self.path.clone(),
            reason: ConnectionReason::UnusableGeometry,
        }
    }
    pub(crate) fn changing_geometry(&self) -> ConnectionError {
        ConnectionError {
            path: self.path.clone(),
            reason: ConnectionReason::ChangingParagraphGeometry,
        }
    }
    pub(crate) fn check_width(&self, width: f64) -> Result<(), ConnectionError> {
        for &(a, b, left, right) in &self.bounds {
            let ar = (width - right).max(a + 1.0);
            let br = (width - right).max(b + 1.0);
            if !ar.is_finite() || ar != br || ar <= left {
                return Err(ConnectionError {
                    path: self.path.clone(),
                    reason: ConnectionReason::HorizontalBounds,
                });
            }
        }
        Ok(())
    }
}
impl ParagraphConnections {
    pub(crate) fn transition(
        &self,
        blocks: &[Block],
        prefix: &[BlockPathStep],
        index: usize,
    ) -> Transition {
        let mut path = prefix.to_vec();
        path.push(BlockPathStep::Block(index));
        let mut result = Transition {
            path: path.clone(),
            ..Transition::default()
        };
        for c in &self.connections {
            if c.from != path && c.to != path {
                continue;
            }
            // The whole overlay was validated before entering layout. Structure
            // remains unchanged in internal autofit copies.
            let a = resolve(blocks, &c.from[prefix.len()..]).expect("validated local from path");
            let b = resolve(blocks, &c.to[prefix.len()..]).expect("validated local to path");
            result.bounds.push((
                a.props.indent_left.max(0.0),
                b.props.indent_left.max(0.0),
                crate::flow::border_left(a.props),
                a.props.indent_right.max(0.0),
            ));
            if c.to == path {
                result.incoming = Some(
                    crate::ParagraphBorder::new(c.separator, 0.0).expect("validated separator"),
                );
            }
            if c.from == path {
                result.outgoing = true;
            }
        }
        result
    }
    pub(crate) fn contains_boundary(
        &self,
        prefix: &[BlockPathStep],
        from: usize,
        to: usize,
    ) -> bool {
        self.connections.iter().any(|c| {
            c.from.len() == prefix.len() + 1
                && c.from.starts_with(prefix)
                && c.from.last() == Some(&BlockPathStep::Block(from))
                && c.to.last() == Some(&BlockPathStep::Block(to))
        })
    }
    pub(crate) fn has_descendant(&self, prefix: &[BlockPathStep]) -> bool {
        self.connections.iter().any(|c| c.to.starts_with(prefix))
    }
}
