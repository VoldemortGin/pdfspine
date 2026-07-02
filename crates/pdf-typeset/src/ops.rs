//! The positioned draw-op IR shared by the layout stages (TS-1).
//!
//! Generalizes `pdf-markdown`'s op vocabulary (PRD §10 落点地图,
//! `layout.rs:88-139`) past its two fixed-enum traps: text ops carry an open
//! [`FaceId`] (the embed registry hands them out, TS-3) **and** a per-op size,
//! so one line may mix faces and sizes; the shape / alpha / clip / transform
//! ops cover the TS-6 preset-geometry needs (`ca`/`CA` ExtGState, `W n`
//! clipping, `q cm … Q` shape transforms).
//!
//! Everything is authored in **top-left page coordinates** (y grows downward);
//! the emitter flips to PDF user space (the pdf-markdown convention,
//! `layout.rs:4-6`). Run decorations (underline / strike / highlight) are
//! materialized by layout into [`Op::Line`] / [`Op::FillRect`] — there are no
//! decoration ops.

use crate::{Matrix, Rgb};

/// An index into the export run's face registry (TS-3): each distinct
/// embedded face (family × style × TTC index) gets one id and one `/FontFile2`
/// per document. Replaces pdf-markdown's fixed 7-variant `Face` enum.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceId(pub usize);

/// One positioned draw operation, in top-left page coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// A single-face, single-size text run at a baseline point.
    Text {
        /// The registered face to draw with.
        face: FaceId,
        /// Font size in points (per-op — lines may mix sizes).
        size: f64,
        /// Fill color.
        color: Rgb,
        /// Baseline start x.
        x: f64,
        /// Baseline y (top-left coords; distance from the page top).
        baseline: f64,
        /// The text to show.
        text: String,
    },
    /// A filled axis-aligned rectangle (`y` is the top edge).
    FillRect {
        /// Left edge.
        x: f64,
        /// Top edge.
        y: f64,
        /// Width.
        w: f64,
        /// Height.
        h: f64,
        /// Fill color.
        color: Rgb,
    },
    /// A stroked axis-aligned rectangle.
    StrokeRect {
        /// Left edge.
        x: f64,
        /// Top edge.
        y: f64,
        /// Width.
        w: f64,
        /// Height.
        h: f64,
        /// Stroke color.
        color: Rgb,
        /// Stroke width in points.
        line_width: f64,
    },
    /// A stroked segment.
    Line {
        /// Start x.
        x1: f64,
        /// Start y.
        y1: f64,
        /// End x.
        x2: f64,
        /// End y.
        y2: f64,
        /// Stroke color.
        color: Rgb,
        /// Stroke width in points.
        width: f64,
    },
    /// A filled circle (list bullets).
    FillCircle {
        /// Center x.
        cx: f64,
        /// Center y.
        cy: f64,
        /// Radius.
        r: f64,
        /// Fill color.
        color: Rgb,
    },
    /// A placed image (`y` is the top edge; `id` indexes the export run's
    /// prepared-image list).
    Image {
        /// Prepared-image index.
        id: usize,
        /// Left edge.
        x: f64,
        /// Top edge.
        y: f64,
        /// Width.
        w: f64,
        /// Height.
        h: f64,
    },
    /// An arbitrary filled and/or stroked path (preset-geometry outlines,
    /// arc→Bézier segments, per-edge table borders…).
    Path {
        /// The subpath segments.
        segs: Vec<PathSeg>,
        /// Fill paint (`None` = no fill).
        fill: Option<Fill>,
        /// Stroke paint (`None` = no stroke).
        stroke: Option<Stroke>,
    },
    /// A `q … Q` group: optional `cm` transform, optional `W n` clip path,
    /// nested ops (shape-level transforms, text rotation, box clipping).
    Group {
        /// Transform applied to the nested ops (`cm`), if any.
        transform: Option<Matrix>,
        /// Clip path (`W n`) applied before the nested ops, if any.
        clip: Option<Vec<PathSeg>>,
        /// The nested ops, in paint order.
        ops: Vec<Op>,
    },
}

/// One path segment, in top-left page coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PathSeg {
    /// Begin a new subpath at a point (`m`).
    MoveTo {
        /// x.
        x: f64,
        /// y.
        y: f64,
    },
    /// A straight segment (`l`).
    LineTo {
        /// End x.
        x: f64,
        /// End y.
        y: f64,
    },
    /// A cubic Bézier segment (`c`).
    CurveTo {
        /// First control point x.
        x1: f64,
        /// First control point y.
        y1: f64,
        /// Second control point x.
        x2: f64,
        /// Second control point y.
        y2: f64,
        /// End x.
        x: f64,
        /// End y.
        y: f64,
    },
    /// Close the current subpath (`h`).
    Close,
}

/// Fill paint for [`Op::Path`].
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    /// Fill color.
    pub color: Rgb,
    /// Constant fill alpha in `0.0..=1.0` (`ca` ExtGState; 1.0 = opaque).
    pub alpha: f64,
    /// Use the even-odd rule (`f*`) instead of nonzero winding (`f`) —
    /// enables donut / frame multi-subpath fills.
    pub even_odd: bool,
}

impl Fill {
    /// An opaque nonzero-winding fill in `color`.
    #[must_use]
    pub fn new(color: Rgb) -> Self {
        Fill {
            color,
            alpha: 1.0,
            even_odd: false,
        }
    }
}

/// Stroke paint for [`Op::Path`].
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// Stroke color.
    pub color: Rgb,
    /// Stroke width in points.
    pub width: f64,
    /// Constant stroke alpha in `0.0..=1.0` (`CA` ExtGState; 1.0 = opaque).
    pub alpha: f64,
    /// Line cap style (`J`).
    pub cap: LineCap,
    /// Line join style (`j`).
    pub join: LineJoin,
    /// Dash pattern (`d`; empty = solid).
    pub dashes: Vec<f64>,
}

impl Stroke {
    /// An opaque solid stroke of `width` points in `color` (butt cap, miter
    /// join).
    #[must_use]
    pub fn new(color: Rgb, width: f64) -> Self {
        Stroke {
            color,
            width,
            alpha: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            dashes: Vec::new(),
        }
    }
}

/// PDF line cap styles (`J` operator values).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    /// Squared-off at the endpoint (default).
    #[default]
    Butt,
    /// Rounded.
    Round,
    /// Squared-off past the endpoint by half the width.
    Square,
}

/// PDF line join styles (`j` operator values).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// Mitered corners (default).
    #[default]
    Miter,
    /// Rounded corners.
    Round,
    /// Beveled corners.
    Bevel,
}

/// The draw ops of one output page, in paint order, plus its geometry (page
/// sizes may vary per docspine section — the `PageProvider` callback, TS-4).
#[derive(Clone, Debug, PartialEq)]
pub struct PageOps {
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// The ops, in paint order.
    pub ops: Vec<Op>,
}
