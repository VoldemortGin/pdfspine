//! Vector drawing — `draw_line` / `draw_rect` / `draw_circle` / `draw_oval` /
//! `draw_bezier` / `draw_polyline` / `draw_curve` and the [`Shape`] builder
//! (PyMuPDF `page.new_shape()`), PRD §8.8.
//!
//! A [`Shape`] accumulates path-construction operators (`m l c re h`), then
//! [`Shape::finish`] sets the paint parameters (stroke/fill color, line width,
//! dashes) and chooses the paint operator (`S` stroke / `f` fill / `B` both),
//! and [`Shape::commit`] emits one balanced `q … Q` chunk and appends it to the
//! page. All input coordinates are in **PyMuPDF top-left page space** and are
//! converted to PDF user space at construction time.
//!
//! Circles/ovals are approximated by **four cubic Béziers** with the standard
//! magic constant κ = 0.5523 (the optimal unit-circle fit), per PRD §8.8.

use pdf_core::error::Result;
use pdf_core::geom::{Point, Rect};
use pdf_core::DocumentStore;

use crate::color::Color;
use crate::content::{fmt_num, PageContent};

/// The cubic-Bézier circle constant: κ = 4/3·(√2 − 1) ≈ 0.5523 (PRD §8.8).
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// A path/paint builder over one page (PyMuPDF `Shape`). Coordinates passed to
/// the `draw_*` methods are in **PyMuPDF top-left page space**; they are mapped
/// to PDF user space immediately, so the accumulated buffer is render-ready.
pub struct Shape<'a> {
    pc: PageContent<'a>,
    /// The path-construction operators accumulated since the last `finish`
    /// (already in user space).
    buf: Vec<u8>,
    /// Finished paint groups (each a balanced `q … Q`), accumulated across
    /// multiple `finish` calls before a single `commit`.
    committed: Vec<u8>,
    /// The last path point emitted (in user space), or `None` after a
    /// `finish`/start. Used to suppress a redundant `m` when a new segment
    /// begins exactly where the previous one ended (PyMuPDF's `last_point`),
    /// so chained primitives (e.g. squiggle's beziers) match fitz operator-for-
    /// operator.
    last: Option<Point>,
}

impl<'a> Shape<'a> {
    /// Opens a shape on the page at zero-based `index`.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`](pdf_core::error::Error::Unsupported) if `index` is
    /// out of range.
    pub fn new(doc: &'a DocumentStore, index: usize) -> Result<Self> {
        Ok(Shape {
            pc: PageContent::new(doc, index)?,
            buf: Vec::new(),
            committed: Vec::new(),
            last: None,
        })
    }

    /// Maps a top-left page point to PDF user space.
    fn u(&self, p: Point) -> Point {
        self.pc.to_user_space(p)
    }

    /// Appends a path-construction operator line.
    fn op(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(b'\n');
    }

    /// Emits a `m` to user-space point `a` unless the path is already there
    /// (PyMuPDF's `last_point` optimization), then records it as the last point.
    fn move_to(&mut self, a: Point) {
        let here = self
            .last
            .map(|l| (l.x - a.x).abs() < 1e-9 && (l.y - a.y).abs() < 1e-9)
            .unwrap_or(false);
        if !here {
            self.op(&format!("{} {} m", fmt_num(a.x), fmt_num(a.y)));
        }
        self.last = Some(a);
    }

    /// A straight segment from `p1` to `p2` (`m … l`). Returns `p2` (PyMuPDF
    /// returns the last point so segments chain).
    pub fn draw_line(&mut self, p1: Point, p2: Point) -> Point {
        let a = self.u(p1);
        let b = self.u(p2);
        self.move_to(a);
        self.op(&format!("{} {} l", fmt_num(b.x), fmt_num(b.y)));
        self.last = Some(b);
        p2
    }

    /// A polyline through `points` (`m` then chained `l`).
    pub fn draw_polyline(&mut self, points: &[Point]) {
        if let Some((first, rest)) = points.split_first() {
            let a = self.u(*first);
            self.move_to(a);
            for p in rest {
                let q = self.u(*p);
                self.op(&format!("{} {} l", fmt_num(q.x), fmt_num(q.y)));
                self.last = Some(q);
            }
        }
    }

    /// Closes the current subpath with `h` (PyMuPDF closes the sector/pie wedge
    /// this way). Resets `last` so the next primitive starts a fresh `m`.
    pub fn close_path(&mut self) {
        self.op("h");
        self.last = None;
    }

    /// An axis-aligned rectangle (`re`). The rect is given in top-left space; in
    /// user space the `re` origin is the lower-left corner with positive
    /// width/height.
    pub fn draw_rect(&mut self, rect: Rect) {
        let ur = self.pc.rect_to_user_space(rect);
        self.op(&format!(
            "{} {} {} {} re",
            fmt_num(ur.x0),
            fmt_num(ur.y0),
            fmt_num(ur.width()),
            fmt_num(ur.height())
        ));
    }

    /// A single cubic Bézier from `p1` to `p4` with control points `p2`, `p3`
    /// (`m … c`).
    pub fn draw_bezier(&mut self, p1: Point, p2: Point, p3: Point, p4: Point) {
        let a = self.u(p1);
        let b = self.u(p2);
        let c = self.u(p3);
        let d = self.u(p4);
        self.move_to(a);
        self.op(&format!(
            "{} {} {} {} {} {} c",
            fmt_num(b.x),
            fmt_num(b.y),
            fmt_num(c.x),
            fmt_num(c.y),
            fmt_num(d.x),
            fmt_num(d.y)
        ));
        self.last = Some(d);
    }

    /// A smooth curve through `points` (a Catmull-Rom-style chain emitted as
    /// cubic Béziers). With two points it degenerates to a line.
    pub fn draw_curve(&mut self, points: &[Point]) {
        if points.len() < 2 {
            return;
        }
        if points.len() == 2 {
            self.draw_line(points[0], points[1]);
            return;
        }
        // Convert to user space, then emit Catmull-Rom → Bézier segments.
        let us: Vec<Point> = points.iter().map(|p| self.u(*p)).collect();
        self.op(&format!("{} {} m", fmt_num(us[0].x), fmt_num(us[0].y)));
        for i in 0..us.len() - 1 {
            let p0 = if i == 0 { us[0] } else { us[i - 1] };
            let p1 = us[i];
            let p2 = us[i + 1];
            let p3 = if i + 2 < us.len() {
                us[i + 2]
            } else {
                us[i + 1]
            };
            let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
            let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);
            self.op(&format!(
                "{} {} {} {} {} {} c",
                fmt_num(c1.x),
                fmt_num(c1.y),
                fmt_num(c2.x),
                fmt_num(c2.y),
                fmt_num(p2.x),
                fmt_num(p2.y)
            ));
        }
    }

    /// An ellipse fitting `rect` (top-left space) as four cubic Béziers (κ),
    /// closed with `h`.
    pub fn draw_oval(&mut self, rect: Rect) {
        let ur = self.pc.rect_to_user_space(rect);
        let cx = (ur.x0 + ur.x1) / 2.0;
        let cy = (ur.y0 + ur.y1) / 2.0;
        let rx = ur.width() / 2.0;
        let ry = ur.height() / 2.0;
        self.emit_ellipse(cx, cy, rx, ry);
    }

    /// A circle of radius `r` centered at `center` (top-left space), as four
    /// cubic Béziers (κ), closed with `h`.
    pub fn draw_circle(&mut self, center: Point, r: f64) {
        let c = self.u(center);
        self.emit_ellipse(c.x, c.y, r, r);
    }

    /// Emits the four-Bézier ellipse path for center `(cx,cy)` and radii
    /// `(rx,ry)` in **user space** (no further conversion), closed with `h`.
    fn emit_ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) {
        let ox = rx * KAPPA;
        let oy = ry * KAPPA;
        // Start at the rightmost point, go counter-clockwise.
        self.op(&format!("{} {} m", fmt_num(cx + rx), fmt_num(cy)));
        self.op(&format!(
            "{} {} {} {} {} {} c",
            fmt_num(cx + rx),
            fmt_num(cy + oy),
            fmt_num(cx + ox),
            fmt_num(cy + ry),
            fmt_num(cx),
            fmt_num(cy + ry)
        ));
        self.op(&format!(
            "{} {} {} {} {} {} c",
            fmt_num(cx - ox),
            fmt_num(cy + ry),
            fmt_num(cx - rx),
            fmt_num(cy + oy),
            fmt_num(cx - rx),
            fmt_num(cy)
        ));
        self.op(&format!(
            "{} {} {} {} {} {} c",
            fmt_num(cx - rx),
            fmt_num(cy - oy),
            fmt_num(cx - ox),
            fmt_num(cy - ry),
            fmt_num(cx),
            fmt_num(cy - ry)
        ));
        self.op(&format!(
            "{} {} {} {} {} {} c",
            fmt_num(cx + ox),
            fmt_num(cy - ry),
            fmt_num(cx + rx),
            fmt_num(cy - oy),
            fmt_num(cx + rx),
            fmt_num(cy)
        ));
        self.op("h");
        // The closed subpath ends the current pen position.
        self.last = None;
    }

    /// Finishes the **current** sub-path group: prepends the graphics-state
    /// operators (line width, dashes, stroke/fill colors) and appends the paint
    /// operator chosen by `(color, fill)`:
    /// - `color` set, `fill` unset → `S` (stroke);
    /// - `fill` set, `color` unset → `f` (fill);
    /// - both set → `B` (fill + stroke);
    /// - neither → `n` (no paint — path discarded but state applied).
    ///
    /// The state + paint are wrapped in a `q … Q` so they don't leak. Returns
    /// `self` for chaining. (PyMuPDF allows multiple `finish` blocks before a
    /// single `commit`.)
    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        &mut self,
        color: Option<Color>,
        fill: Option<Color>,
        width: f64,
        dashes: Option<&str>,
        even_odd: bool,
        close_path: bool,
    ) {
        // Pull out the path constructed since the previous finish/start. A
        // finished group starts a fresh subpath (PyMuPDF resets last_point).
        let path = std::mem::take(&mut self.buf);
        self.last = None;
        let mut block = Vec::new();
        block.extend_from_slice(b"q\n");
        block.extend_from_slice(format!("{} w\n", fmt_num(width)).as_bytes());
        if let Some(d) = dashes {
            block.extend_from_slice(format!("{d} d\n").as_bytes());
        }
        if let Some(sc) = color {
            block.extend_from_slice(format!("{}\n", sc.stroke_op()).as_bytes());
        }
        if let Some(fc) = fill {
            block.extend_from_slice(format!("{}\n", fc.fill_op()).as_bytes());
        }
        block.extend_from_slice(&path);
        if close_path {
            block.extend_from_slice(b"h\n");
        }
        let paint = match (color.is_some(), fill.is_some()) {
            (true, true) => {
                if even_odd {
                    "B*"
                } else {
                    "B"
                }
            }
            (false, true) => {
                if even_odd {
                    "f*"
                } else {
                    "f"
                }
            }
            (true, false) => "S",
            (false, false) => "n",
        };
        block.extend_from_slice(paint.as_bytes());
        block.push(b'\n');
        block.extend_from_slice(b"Q\n");
        // Stash the finished block back into `buf` (committed blocks accumulate).
        // We tag committed blocks by moving them to a separate area: reuse `buf`
        // as the running output, and continue accumulating new path ops after.
        self.committed.extend_from_slice(&block);
    }

    /// Appends the accumulated finished blocks to the page as one content chunk.
    ///
    /// # Errors
    ///
    /// Propagates resolve / ChangeSet errors.
    pub fn commit(self) -> Result<()> {
        // If the caller drew a path but never called `finish`, default to a
        // black stroke at width 1 so the path is visible (PyMuPDF requires an
        // explicit finish, but a forgiving default avoids a silent no-op).
        let mut out = self.committed;
        if !self.buf.is_empty() {
            out.extend_from_slice(b"q\n1 w\n0 0 0 RG\n");
            out.extend_from_slice(&self.buf);
            out.extend_from_slice(b"S\nQ\n");
        }
        if out.is_empty() {
            return Ok(());
        }
        self.pc.append_content(&out)
    }
}

// === one-shot page convenience wrappers (PyMuPDF `page.draw_*`) ===========
//
// Each opens a `Shape`, draws the primitive, finishes with the given paint
// parameters, and commits — the common single-primitive case.

/// `page.draw_line` — a stroked segment.
///
/// # Errors
/// Propagates page-resolve / ChangeSet errors.
pub fn draw_line(
    doc: &DocumentStore,
    page: usize,
    p1: Point,
    p2: Point,
    color: Color,
    width: f64,
) -> Result<()> {
    let mut s = Shape::new(doc, page)?;
    s.draw_line(p1, p2);
    s.finish(Some(color), None, width, None, false, false);
    s.commit()
}

/// `page.draw_rect` — a rectangle (stroke and/or fill).
///
/// # Errors
/// Propagates page-resolve / ChangeSet errors.
pub fn draw_rect(
    doc: &DocumentStore,
    page: usize,
    rect: Rect,
    color: Option<Color>,
    fill: Option<Color>,
    width: f64,
) -> Result<()> {
    let mut s = Shape::new(doc, page)?;
    s.draw_rect(rect);
    s.finish(color, fill, width, None, false, false);
    s.commit()
}

/// `page.draw_circle` — a circle (stroke and/or fill).
///
/// # Errors
/// Propagates page-resolve / ChangeSet errors.
pub fn draw_circle(
    doc: &DocumentStore,
    page: usize,
    center: Point,
    r: f64,
    color: Option<Color>,
    fill: Option<Color>,
    width: f64,
) -> Result<()> {
    let mut s = Shape::new(doc, page)?;
    s.draw_circle(center, r);
    s.finish(color, fill, width, None, false, false);
    s.commit()
}

/// `page.draw_oval` — an ellipse fitting `rect` (stroke and/or fill).
///
/// # Errors
/// Propagates page-resolve / ChangeSet errors.
pub fn draw_oval(
    doc: &DocumentStore,
    page: usize,
    rect: Rect,
    color: Option<Color>,
    fill: Option<Color>,
    width: f64,
) -> Result<()> {
    let mut s = Shape::new(doc, page)?;
    s.draw_oval(rect);
    s.finish(color, fill, width, None, false, false);
    s.commit()
}

/// `page.draw_bezier` — a single cubic Bézier (stroked).
///
/// # Errors
/// Propagates page-resolve / ChangeSet errors.
#[allow(clippy::too_many_arguments)]
pub fn draw_bezier(
    doc: &DocumentStore,
    page: usize,
    p1: Point,
    p2: Point,
    p3: Point,
    p4: Point,
    color: Color,
    width: f64,
) -> Result<()> {
    let mut s = Shape::new(doc, page)?;
    s.draw_bezier(p1, p2, p3, p4);
    s.finish(Some(color), None, width, None, false, false);
    s.commit()
}

/// `page.draw_polyline` — a chained-segment polyline (stroked).
///
/// # Errors
/// Propagates page-resolve / ChangeSet errors.
pub fn draw_polyline(
    doc: &DocumentStore,
    page: usize,
    points: &[Point],
    color: Color,
    width: f64,
) -> Result<()> {
    let mut s = Shape::new(doc, page)?;
    s.draw_polyline(points);
    s.finish(Some(color), None, width, None, false, false);
    s.commit()
}

/// `page.draw_curve` — a smooth curve through `points` (stroked).
///
/// # Errors
/// Propagates page-resolve / ChangeSet errors.
pub fn draw_curve(
    doc: &DocumentStore,
    page: usize,
    points: &[Point],
    color: Color,
    width: f64,
) -> Result<()> {
    let mut s = Shape::new(doc, page)?;
    s.draw_curve(points);
    s.finish(Some(color), None, width, None, false, false);
    s.commit()
}
