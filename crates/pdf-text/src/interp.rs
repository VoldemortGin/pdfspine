//! The content-stream interpreter (M2b, PRD §8.6).
//!
//! [`ContentInterpreter`] runs a page's decoded content stream(s) over a
//! graphics-state machine and emits a flat [`InterpretResult`] of positioned
//! glyphs (PDF user space) + an image inventory. It implements the operator
//! subset of PRD §8.6.2: graphics state (`q/Q/cm`), the text object
//! (`BT/ET`), every text-state op (`Tc Tw Tz TL Tf Tr Ts`), positioning
//! (`Td TD Tm T*`), showing (`Tj TJ ' "`), fill-color ops → packed sRGB, `Do`
//! form-XObject recursion (depth-capped + cycle-guarded), and inline images
//! (skipped, captured into the inventory).
//!
//! ## Text rendering matrix (PRD §8.6.1, row-vector convention)
//!
//! ```text
//! params = Matrix(Tfs·Th, 0, 0, Tfs, 0, Trise)
//! Trm    = params · Tm · CTM     // glyph space → user space
//! ```
//!
//! The glyph origin is `(0, 0)·Trm`; the bounding box is the axis-aligned
//! envelope of the glyph cell `[0, descent .. w0, ascent]` (1000-unit glyph
//! space scaled by the font size) transformed by `Trm` — taking the envelope
//! *after* the transform makes a rotated `Tm` produce a correct axis-aligned
//! box. After each glyph the text matrix advances by
//! `tx = ((w0/1000)·Tfs + Tc + Tw_if_space)·Th` (horizontal writing).

use std::collections::HashSet;
use std::sync::Arc;

use pdf_core::geom::{Matrix, Point, Rect};
use pdf_core::{Dict, DocumentStore, Name, Object};
use pdf_fonts::FontMapper;
use smol_str::SmolStr;

use crate::model::{
    DrawPath, ImageRef, InterpretResult, PaintKind, PathItem, PositionedGlyph, WritingDir,
};
use crate::renderops::{ImageOp, RenderOp, ShadingOp, TextRun};
use crate::state::GraphicsState;
use crate::tokenizer::{tokenize, Event};

/// Maximum Form-XObject `Do` recursion depth (PRD §8.6.2 — depth cap 16).
const MAX_FORM_DEPTH: u32 = 16;

/// Default ascent / descent (1000-unit glyph space) when a font has no usable
/// `/FontDescriptor` metrics — typical Latin-text values.
const DEFAULT_ASCENT: f64 = 800.0;
const DEFAULT_DESCENT: f64 = -200.0;

/// Per-font cached data: the mapper plus glyph-cell vertical metrics.
struct CachedFont {
    mapper: FontMapper,
    /// Ascent in 1000-unit glyph space (top of the glyph cell).
    ascent: f64,
    /// Descent in 1000-unit glyph space (bottom of the cell; usually negative).
    descent: f64,
    /// The resolved font dictionary (only needed by the render sink to find the
    /// embedded `/FontFile*` program; cheap `Dict` clone, built once per font).
    dict: Dict,
}

/// Accumulates path-construction operators (`m l c v y re h`) into device-space
/// [`PathItem`]s for the current sub-path(s). Reset after each paint operator
/// (`S s f F f* B B* b b* n`). Points are CTM-transformed at construction time
/// so a `DrawPath` is already in user space (PRD §8.8 `get_drawings`).
#[derive(Default)]
struct CurrentPath {
    /// The completed items of the path (across sub-paths until painted).
    items: Vec<PathItem>,
    /// The current point (user space), updated by `m`/`l`/`c`/`re`.
    current: Option<Point>,
    /// The most recent sub-path start (for `h` close).
    subpath_start: Option<Point>,
    /// Whether any sub-path was closed with `h` since the last paint.
    closed: bool,
    /// Set by `W`/`W*`: the next paint op also intersects the clip with this
    /// path (`Some(even_odd)`). Consumed (and emitted) at the next paint op.
    clip_pending: Option<bool>,
}

impl CurrentPath {
    fn moveto(&mut self, p: Point) {
        self.current = Some(p);
        self.subpath_start = Some(p);
    }

    fn lineto(&mut self, p: Point) {
        if let Some(from) = self.current {
            self.items.push(PathItem::Line(from, p));
        }
        self.current = Some(p);
    }

    fn curveto(&mut self, c1: Point, c2: Point, end: Point) {
        if let Some(from) = self.current {
            self.items.push(PathItem::Curve(from, c1, c2, end));
        }
        self.current = Some(end);
    }

    fn rect(&mut self, r: Rect) {
        self.items.push(PathItem::Rect(r));
        // A `re` sets the current point to its lower-left and starts a sub-path.
        let p = Point::new(r.x0, r.y0);
        self.current = Some(p);
        self.subpath_start = Some(p);
    }

    fn close(&mut self) {
        self.closed = true;
        if let (Some(start), Some(cur)) = (self.subpath_start, self.current) {
            if start != cur {
                self.items.push(PathItem::Line(cur, start));
            }
        }
        self.current = self.subpath_start;
    }

    fn reset(&mut self) {
        self.items.clear();
        self.current = None;
        self.subpath_start = None;
        self.closed = false;
        self.clip_pending = None;
    }

    /// The axis-aligned envelope of all item points (user space).
    fn bounds(&self) -> Rect {
        let mut r = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut acc = |p: Point| {
            r.x0 = r.x0.min(p.x);
            r.y0 = r.y0.min(p.y);
            r.x1 = r.x1.max(p.x);
            r.y1 = r.y1.max(p.y);
        };
        for it in &self.items {
            match *it {
                PathItem::Line(a, b) => {
                    acc(a);
                    acc(b);
                }
                PathItem::Curve(a, b, c, d) => {
                    acc(a);
                    acc(b);
                    acc(c);
                    acc(d);
                }
                PathItem::Rect(rr) => {
                    acc(Point::new(rr.x0, rr.y0));
                    acc(Point::new(rr.x1, rr.y1));
                }
            }
        }
        if self.items.is_empty() {
            Rect::new(0.0, 0.0, 0.0, 0.0)
        } else {
            r.normalize()
        }
    }
}

/// Runs a page (or a form/resources pair) and produces positioned glyphs.
pub struct ContentInterpreter<'a> {
    doc: &'a DocumentStore,
    out: InterpretResult,
    /// When `Some`, the interpreter additionally records an **ordered**
    /// [`RenderOp`] stream (document order, for M6 rendering / `DisplayList`).
    /// `None` for the M2 text-extraction path (zero overhead, identical output).
    render_ops: Option<Vec<RenderOp>>,
}

impl<'a> ContentInterpreter<'a> {
    /// Creates an interpreter bound to a document store (text-extraction mode —
    /// no ordered render-op recording).
    #[must_use]
    pub fn new(doc: &'a DocumentStore) -> Self {
        ContentInterpreter {
            doc,
            out: InterpretResult::default(),
            render_ops: None,
        }
    }

    /// Creates an interpreter that **also** records the ordered [`RenderOp`]
    /// stream (M6 rendering / `DisplayList`). The flat [`InterpretResult`] is
    /// still produced; [`ContentInterpreter::run_page_render`] returns both.
    #[must_use]
    pub fn new_recording(doc: &'a DocumentStore) -> Self {
        ContentInterpreter {
            doc,
            out: InterpretResult::default(),
            render_ops: Some(Vec::new()),
        }
    }

    /// Whether the ordered render-op sink is active.
    fn recording(&self) -> bool {
        self.render_ops.is_some()
    }

    /// Pushes one ordered render op (no-op when not recording).
    fn emit(&mut self, op: RenderOp) {
        if let Some(ops) = self.render_ops.as_mut() {
            ops.push(op);
        }
    }

    /// Runs a page dictionary in **recording** mode, returning the ordered
    /// [`RenderOp`] stream (the M6 render driver / `DisplayList` source).
    #[must_use]
    pub fn run_page_render(mut self, page: &Dict) -> Vec<RenderOp> {
        let content = self.page_content(page);
        let resources = self
            .doc
            .resolve_dict_key(page, &Name::new("Resources"))
            .ok()
            .flatten()
            .and_then(|o| o.as_dict().cloned())
            .unwrap_or_default();
        self.run(
            &content,
            &resources,
            Matrix::IDENTITY,
            0,
            &mut HashSet::new(),
        );
        self.render_ops.take().unwrap_or_default()
    }

    /// Interprets a page dictionary: concatenates its `/Contents` stream(s),
    /// resolves `/Resources`, and runs the interpreter with an identity base
    /// CTM. Returns the positioned glyphs + image inventory (PDF user space).
    #[must_use]
    pub fn run_page(mut self, page: &Dict) -> InterpretResult {
        let content = self.page_content(page);
        let resources = self
            .doc
            .resolve_dict_key(page, &Name::new("Resources"))
            .ok()
            .flatten()
            .and_then(|o| o.as_dict().cloned())
            .unwrap_or_default();
        self.run(
            &content,
            &resources,
            Matrix::IDENTITY,
            0,
            &mut HashSet::new(),
        );
        self.out
    }

    /// Interprets an explicit `(content, resources)` pair under a base CTM —
    /// the testing / form-recursion entry point.
    #[must_use]
    pub fn run_content(
        mut self,
        content: &[u8],
        resources: &Dict,
        base_ctm: Matrix,
    ) -> InterpretResult {
        self.run(content, resources, base_ctm, 0, &mut HashSet::new());
        self.out
    }

    // --- content acquisition ---------------------------------------------

    /// Concatenates a page's `/Contents` (a single stream or an array of
    /// streams) into one decoded byte buffer, joining streams with a single
    /// newline so an operator can't straddle the boundary (PRD §8.6.2).
    fn page_content(&self, page: &Dict) -> Vec<u8> {
        let Some(contents) = self
            .doc
            .resolve_dict_key(page, &Name::new("Contents"))
            .ok()
            .flatten()
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match contents.as_ref() {
            Object::Stream(s) => {
                if let Ok(bytes) = self.doc.decode_stream(s).and_then(|o| o.into_decoded()) {
                    out.extend_from_slice(&bytes);
                }
            }
            Object::Array(arr) => {
                for item in arr {
                    let resolved = match item {
                        Object::Reference(r) => self.doc.resolve(*r).ok(),
                        other => Some(Arc::new(other.clone())),
                    };
                    if let Some(obj) = resolved {
                        if let Some(s) = obj.as_stream() {
                            if let Ok(bytes) =
                                self.doc.decode_stream(s).and_then(|o| o.into_decoded())
                            {
                                if !out.is_empty() {
                                    out.push(b'\n');
                                }
                                out.extend_from_slice(&bytes);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    // --- core interpreter loop -------------------------------------------

    /// Runs one content buffer against a resource dict, base CTM and the given
    /// form-recursion depth. `visited` carries the set of form XObject object
    /// numbers on the current recursion path (cycle guard).
    fn run(
        &mut self,
        content: &[u8],
        resources: &Dict,
        base_ctm: Matrix,
        depth: u32,
        visited: &mut HashSet<u32>,
    ) {
        let events = tokenize(content);

        // Graphics-state stack (q/Q). The top is `gs`.
        let mut gs = GraphicsState::new(base_ctm);
        let mut stack: Vec<GraphicsState> = Vec::new();

        // Text-object matrices (reset at each BT; not on the q/Q stack).
        let mut in_text = false;
        let mut tm = Matrix::IDENTITY;
        let mut tlm = Matrix::IDENTITY;

        // Per-resource font cache (lazy; keyed by resource name).
        let mut font_cache: std::collections::HashMap<SmolStr, Option<CachedFont>> =
            std::collections::HashMap::new();

        // Operand stack for the current operator.
        let mut ops: Vec<Object> = Vec::new();

        // The in-progress vector path (path-construction → paint, PRD §8.8).
        let mut path = CurrentPath::default();

        for ev in events {
            match ev {
                Event::Operand(o) => ops.push(o),
                Event::InlineImage { params, data } => {
                    self.record_inline_image(&params, data, &gs);
                    ops.clear();
                }
                Event::Operator(name) => {
                    self.apply_operator(
                        &name,
                        &mut ops,
                        &mut gs,
                        &mut stack,
                        &mut in_text,
                        &mut tm,
                        &mut tlm,
                        &mut path,
                        resources,
                        &mut font_cache,
                        depth,
                        visited,
                    );
                    ops.clear();
                }
            }
        }
    }

    /// Applies a single operator with its accumulated operands.
    #[allow(clippy::too_many_arguments)]
    fn apply_operator(
        &mut self,
        name: &[u8],
        ops: &mut [Object],
        gs: &mut GraphicsState,
        stack: &mut Vec<GraphicsState>,
        in_text: &mut bool,
        tm: &mut Matrix,
        tlm: &mut Matrix,
        path: &mut CurrentPath,
        resources: &Dict,
        font_cache: &mut std::collections::HashMap<SmolStr, Option<CachedFont>>,
        depth: u32,
        visited: &mut HashSet<u32>,
    ) {
        match name {
            // --- graphics state ------------------------------------------
            b"q" => {
                stack.push(gs.clone());
                if self.recording() {
                    self.emit(RenderOp::Save);
                }
            }
            b"Q" => {
                if let Some(prev) = stack.pop() {
                    *gs = prev;
                }
                if self.recording() {
                    self.emit(RenderOp::Restore);
                }
            }
            b"gs" => {
                // ExtGState: pull constant alpha `ca`/`CA` for the render sink.
                if self.recording() {
                    self.apply_extgstate(ops, gs, resources);
                }
            }
            b"cm" => {
                if let Some(m) = matrix_from(ops) {
                    // CTM' = m · CTM (the new matrix premultiplies).
                    gs.ctm = Matrix::concat(&m, &gs.ctm);
                }
            }
            b"w" => {
                if let Some(v) = nth_f64(ops, 0) {
                    gs.line_width = v;
                }
            }
            b"d" => {
                gs.dashes = format_dash(ops);
            }

            // --- path construction (CTM-applied → user space) ------------
            b"m" => {
                if let (Some(x), Some(y)) = (nth_f64(ops, 0), nth_f64(ops, 1)) {
                    path.moveto(Point::new(x, y).transform(&gs.ctm));
                }
            }
            b"l" => {
                if let (Some(x), Some(y)) = (nth_f64(ops, 0), nth_f64(ops, 1)) {
                    path.lineto(Point::new(x, y).transform(&gs.ctm));
                }
            }
            b"c" => {
                if let Some(v) = six_f64(ops) {
                    path.curveto(
                        Point::new(v[0], v[1]).transform(&gs.ctm),
                        Point::new(v[2], v[3]).transform(&gs.ctm),
                        Point::new(v[4], v[5]).transform(&gs.ctm),
                    );
                }
            }
            b"v" => {
                // `v`: first control point == current point.
                if let Some(v) = four_f64(ops) {
                    let from = path.current.unwrap_or_else(|| Point::new(0.0, 0.0));
                    path.curveto(
                        from,
                        Point::new(v[0], v[1]).transform(&gs.ctm),
                        Point::new(v[2], v[3]).transform(&gs.ctm),
                    );
                }
            }
            b"y" => {
                // `y`: second control point == end point.
                if let Some(v) = four_f64(ops) {
                    let end = Point::new(v[2], v[3]).transform(&gs.ctm);
                    path.curveto(Point::new(v[0], v[1]).transform(&gs.ctm), end, end);
                }
            }
            b"re" => {
                if let Some(v) = four_f64(ops) {
                    // `x y w h re`: build the rect's four corners in user space,
                    // then take the axis-aligned envelope (handles rotated CTMs).
                    let p0 = Point::new(v[0], v[1]).transform(&gs.ctm);
                    let p1 = Point::new(v[0] + v[2], v[1]).transform(&gs.ctm);
                    let p2 = Point::new(v[0] + v[2], v[1] + v[3]).transform(&gs.ctm);
                    let p3 = Point::new(v[0], v[1] + v[3]).transform(&gs.ctm);
                    let mut r = Rect::new(p0.x, p0.y, p0.x, p0.y);
                    for p in [p1, p2, p3] {
                        r.x0 = r.x0.min(p.x);
                        r.y0 = r.y0.min(p.y);
                        r.x1 = r.x1.max(p.x);
                        r.y1 = r.y1.max(p.y);
                    }
                    path.rect(r);
                }
            }
            b"h" => path.close(),

            // --- path painting (emit a DrawPath, then clear the path) ----
            b"S" => self.paint_path(path, gs, PaintKind::Stroke, false),
            b"s" => {
                path.close();
                self.paint_path(path, gs, PaintKind::Stroke, false);
            }
            b"f" | b"F" => self.paint_path(path, gs, PaintKind::Fill, false),
            b"f*" => self.paint_path(path, gs, PaintKind::Fill, true),
            b"B" | b"b" => {
                if name == b"b" {
                    path.close();
                }
                self.paint_path(path, gs, PaintKind::FillStroke, false);
            }
            b"B*" | b"b*" => {
                if name == b"b*" {
                    path.close();
                }
                self.paint_path(path, gs, PaintKind::FillStroke, true);
            }
            b"n" => {
                // `n` may follow `W`/`W*` to apply a clip with no paint.
                if self.recording() {
                    if let Some(eo) = path.clip_pending.take() {
                        if !path.items.is_empty() {
                            self.emit(RenderOp::Clip {
                                items: path.items.clone(),
                                even_odd: eo,
                            });
                        }
                    }
                }
                path.reset();
            }

            // --- clip path (the next paint op also intersects the clip) ---
            b"W" => path.clip_pending = Some(false),
            b"W*" => path.clip_pending = Some(true),

            // --- text object ---------------------------------------------
            b"BT" => {
                *in_text = true;
                *tm = Matrix::IDENTITY;
                *tlm = Matrix::IDENTITY;
            }
            b"ET" => *in_text = false,

            // --- text state ----------------------------------------------
            b"Tc" => {
                if let Some(v) = nth_f64(ops, 0) {
                    gs.text.char_spacing = v;
                }
            }
            b"Tw" => {
                if let Some(v) = nth_f64(ops, 0) {
                    gs.text.word_spacing = v;
                }
            }
            b"Tz" => {
                if let Some(v) = nth_f64(ops, 0) {
                    gs.text.h_scale = v / 100.0;
                }
            }
            b"TL" => {
                if let Some(v) = nth_f64(ops, 0) {
                    gs.text.leading = v;
                }
            }
            b"Ts" => {
                if let Some(v) = nth_f64(ops, 0) {
                    gs.text.rise = v;
                }
            }
            b"Tr" => {
                if let Some(v) = nth_f64(ops, 0) {
                    gs.text.render_mode = (v as i64).clamp(0, 7) as u8;
                }
            }
            b"Tf" => {
                // `/Name size Tf`
                let font = ops.iter().find_map(|o| o.as_name().and_then(Name::as_str));
                if let Some(fname) = font {
                    gs.text.font_name = Some(SmolStr::new(fname));
                }
                if let Some(sz) = nth_f64(ops, 1) {
                    gs.text.font_size = sz;
                }
            }

            // --- text positioning ----------------------------------------
            b"Td" => {
                if let (Some(tx), Some(ty)) = (nth_f64(ops, 0), nth_f64(ops, 1)) {
                    *tlm = Matrix::concat(&Matrix::translate(tx, ty), tlm);
                    *tm = *tlm;
                }
            }
            b"TD" => {
                if let (Some(tx), Some(ty)) = (nth_f64(ops, 0), nth_f64(ops, 1)) {
                    gs.text.leading = -ty;
                    *tlm = Matrix::concat(&Matrix::translate(tx, ty), tlm);
                    *tm = *tlm;
                }
            }
            b"Tm" => {
                if let Some(m) = matrix_from(ops) {
                    *tlm = m;
                    *tm = m;
                }
            }
            b"T*" => {
                let ty = -gs.text.leading;
                *tlm = Matrix::concat(&Matrix::translate(0.0, ty), tlm);
                *tm = *tlm;
            }

            // --- text showing --------------------------------------------
            b"Tj" => {
                if let Some(s) = first_string(ops) {
                    self.show_text(&s, gs, tm, resources, font_cache);
                }
            }
            b"TJ" => {
                if let Some(Object::Array(arr)) = ops.iter().find(|o| matches!(o, Object::Array(_)))
                {
                    let arr = arr.clone();
                    self.show_text_array(&arr, gs, tm, resources, font_cache);
                }
            }
            b"'" => {
                // T* then Tj.
                let ty = -gs.text.leading;
                *tlm = Matrix::concat(&Matrix::translate(0.0, ty), tlm);
                *tm = *tlm;
                if let Some(s) = first_string(ops) {
                    self.show_text(&s, gs, tm, resources, font_cache);
                }
            }
            b"\"" => {
                // `aw ac string "` — set word/char spacing, then '.
                if let Some(aw) = nth_f64(ops, 0) {
                    gs.text.word_spacing = aw;
                }
                if let Some(ac) = nth_f64(ops, 1) {
                    gs.text.char_spacing = ac;
                }
                let ty = -gs.text.leading;
                *tlm = Matrix::concat(&Matrix::translate(0.0, ty), tlm);
                *tm = *tlm;
                if let Some(s) = last_string(ops) {
                    self.show_text(&s, gs, tm, resources, font_cache);
                }
            }

            // --- fill color (→ packed sRGB) ------------------------------
            b"g" => {
                if let Some(gr) = nth_f64(ops, 0) {
                    gs.fill_color = gray_rgb(gr);
                }
            }
            b"rg" => {
                if let (Some(r), Some(gg), Some(b)) =
                    (nth_f64(ops, 0), nth_f64(ops, 1), nth_f64(ops, 2))
                {
                    gs.fill_color = pack_rgb(r, gg, b);
                }
            }
            b"k" => {
                if let (Some(c), Some(m), Some(y), Some(kk)) = (
                    nth_f64(ops, 0),
                    nth_f64(ops, 1),
                    nth_f64(ops, 2),
                    nth_f64(ops, 3),
                ) {
                    gs.fill_color = cmyk_rgb(c, m, y, kk);
                }
            }
            // Stroke color counterparts (recorded but unused for glyph color).
            b"G" => {
                if let Some(gr) = nth_f64(ops, 0) {
                    gs.stroke_color = gray_rgb(gr);
                }
            }
            b"RG" => {
                if let (Some(r), Some(gg), Some(b)) =
                    (nth_f64(ops, 0), nth_f64(ops, 1), nth_f64(ops, 2))
                {
                    gs.stroke_color = pack_rgb(r, gg, b);
                }
            }
            b"K" => {
                if let (Some(c), Some(m), Some(y), Some(kk)) = (
                    nth_f64(ops, 0),
                    nth_f64(ops, 1),
                    nth_f64(ops, 2),
                    nth_f64(ops, 3),
                ) {
                    gs.stroke_color = cmyk_rgb(c, m, y, kk);
                }
            }
            // `cs`/`sc`/`scn` set the fill colorspace/components. We approximate:
            // map 1 comp → gray, 3 → rgb, 4 → cmyk; otherwise leave unchanged.
            b"sc" | b"scn" => {
                let comps: Vec<f64> = ops.iter().filter_map(Object::as_f64).collect();
                if let Some(rgb) = approx_color(&comps) {
                    gs.fill_color = rgb;
                }
            }
            b"SC" | b"SCN" => {
                let comps: Vec<f64> = ops.iter().filter_map(Object::as_f64).collect();
                if let Some(rgb) = approx_color(&comps) {
                    gs.stroke_color = rgb;
                }
            }
            // `cs`/`CS` (colorspace selection) — no-op for color recording.
            b"cs" | b"CS" => {}

            // --- XObjects -------------------------------------------------
            b"Do" => {
                if let Some(xname) = ops.iter().find_map(|o| o.as_name().and_then(Name::as_str)) {
                    self.do_xobject(xname, gs, resources, depth, visited);
                }
            }

            // --- shading (sh) --------------------------------------------
            b"sh" if self.recording() => {
                if let Some(sname) = ops.iter().find_map(|o| o.as_name().and_then(Name::as_str)) {
                    self.do_shading(sname, gs, resources);
                }
            }

            // Everything else: unknown / unhandled operator → skip (tolerant).
            _ => {}
        }
        let _ = in_text; // text ops are valid outside BT in lenient mode too
    }

    // --- text showing -----------------------------------------------------

    /// Shows a literal string: iterate codes, emit a glyph per code, advance Tm.
    fn show_text(
        &mut self,
        bytes: &[u8],
        gs: &GraphicsState,
        tm: &mut Matrix,
        resources: &Dict,
        font_cache: &mut std::collections::HashMap<SmolStr, Option<CachedFont>>,
    ) {
        let Some(font_name) = gs.text.font_name.clone() else {
            return;
        };
        self.ensure_font(&font_name, resources, font_cache);
        // Hold `cached` for the whole show op: `font_cache` is a parameter and
        // `self.out` a field, so the immutable borrow on `cached` and the mutable
        // push into `self.out.glyphs` do not conflict — no per-glyph re-lookup,
        // no up-front `codes` Vec.
        let cached = match font_cache.get(&font_name).and_then(Option::as_ref) {
            Some(c) => c,
            None => return,
        };

        let start = self.out.glyphs.len();
        // Render-op recording captures per-glyph GIDs; the text-extraction path
        // (no recording) skips that allocation entirely.
        let mut gids: Vec<u32> = Vec::new();
        let recording = self.render_ops.is_some();
        for (code, n_bytes) in cached.mapper.iter_codes(bytes) {
            if recording {
                gids.push(cached.mapper.gid(code));
            }
            emit_glyph_into(
                &mut self.out.glyphs,
                cached,
                code,
                n_bytes,
                &font_name,
                gs,
                tm,
            );
        }

        if recording {
            // The glyphs' `origin`/`bbox` already carry the full CTM
            // (Trm = params·Tm·CTM), so the renderer paints with an identity CTM.
            let font_dict = cached.dict.clone();
            let glyphs: Vec<PositionedGlyph> = self.out.glyphs[start..].to_vec();
            if !glyphs.is_empty() {
                self.emit(RenderOp::Text(TextRun {
                    glyphs,
                    gids,
                    font_dict,
                    fill_color: gs.fill_color,
                    stroke_color: gs.stroke_color,
                    fill_alpha: gs.fill_alpha_u8(),
                    render_mode: gs.text.render_mode,
                    stroke_width: gs.line_width,
                    ctm: Matrix::IDENTITY,
                }));
            }
        }
    }

    /// Shows a `TJ` array: strings emit glyphs; numbers displace Tm by
    /// `-adj/1000·Tfs·Th` (horizontal writing).
    fn show_text_array(
        &mut self,
        arr: &[Object],
        gs: &GraphicsState,
        tm: &mut Matrix,
        resources: &Dict,
        font_cache: &mut std::collections::HashMap<SmolStr, Option<CachedFont>>,
    ) {
        for item in arr {
            match item {
                Object::String(s) => {
                    self.show_text(s.as_bytes(), gs, tm, resources, font_cache);
                }
                Object::Integer(_) | Object::Real(_) => {
                    let adj = item.as_f64().unwrap_or(0.0);
                    let tx = -adj / 1000.0 * gs.text.font_size * gs.text.h_scale;
                    *tm = Matrix::concat(&Matrix::translate(tx, 0.0), tm);
                }
                _ => {}
            }
        }
    }

    // --- fonts ------------------------------------------------------------

    /// Ensures `font_name`'s mapper + metrics are in the cache (builds once).
    fn ensure_font(
        &self,
        font_name: &SmolStr,
        resources: &Dict,
        font_cache: &mut std::collections::HashMap<SmolStr, Option<CachedFont>>,
    ) {
        if font_cache.contains_key(font_name) {
            return;
        }
        let cached = self.build_font(font_name, resources);
        font_cache.insert(font_name.clone(), cached);
    }

    /// Resolves `/Resources /Font /<name>` to a font dict and builds a
    /// [`CachedFont`] (mapper + vertical metrics). `None` when unresolvable.
    fn build_font(&self, font_name: &SmolStr, resources: &Dict) -> Option<CachedFont> {
        let fonts = self
            .doc
            .resolve_dict_key(resources, &Name::new("Font"))
            .ok()
            .flatten()?;
        let fonts = fonts.as_dict()?;
        let font_obj = self
            .doc
            .resolve_dict_key(fonts, &Name::new(font_name.as_str()))
            .ok()
            .flatten()?;
        let font_dict = font_obj.as_dict()?;
        let mapper = FontMapper::from_dict(font_dict, self.doc);
        let (ascent, descent) = self.font_vmetrics(font_dict);
        Some(CachedFont {
            mapper,
            ascent,
            descent,
            dict: font_dict.clone(),
        })
    }

    /// Derives glyph-cell vertical metrics (ascent/descent in 1000-unit glyph
    /// space) from the `/FontDescriptor` (`/Ascent`/`/Descent`, else
    /// `/FontBBox` top/bottom), falling back to Latin-text defaults. For a
    /// Type0 font the descriptor lives on the descendant CIDFont.
    fn font_vmetrics(&self, font_dict: &Dict) -> (f64, f64) {
        let desc = self.font_descriptor(font_dict);
        if let Some(d) = desc.as_ref() {
            let asc = d.get(&Name::new("Ascent")).and_then(Object::as_f64);
            let dsc = d.get(&Name::new("Descent")).and_then(Object::as_f64);
            if let (Some(a), Some(de)) = (asc, dsc) {
                if a != 0.0 || de != 0.0 {
                    return (a, de);
                }
            }
            // Fall back to FontBBox [llx lly urx ury] → (ury, lly).
            if let Some(bbox) = d
                .get(&Name::new("FontBBox"))
                .and_then(Object::as_array)
                .filter(|a| a.len() == 4)
            {
                let lly = bbox[1].as_f64();
                let ury = bbox[3].as_f64();
                if let (Some(lly), Some(ury)) = (lly, ury) {
                    if ury != 0.0 || lly != 0.0 {
                        return (ury, lly);
                    }
                }
            }
        }
        (DEFAULT_ASCENT, DEFAULT_DESCENT)
    }

    /// Resolves the `/FontDescriptor`, following the descendant CIDFont for a
    /// Type0 font.
    fn font_descriptor(&self, font_dict: &Dict) -> Option<Dict> {
        // Direct descriptor (simple fonts).
        if let Some(d) = self
            .doc
            .resolve_dict_key(font_dict, &Name::new("FontDescriptor"))
            .ok()
            .flatten()
            .and_then(|o| o.as_dict().cloned())
        {
            return Some(d);
        }
        // Type0: descend into /DescendantFonts[0].
        let df = self
            .doc
            .resolve_dict_key(font_dict, &Name::new("DescendantFonts"))
            .ok()
            .flatten()?;
        let arr = df.as_array()?;
        let first = arr.first()?;
        let descendant = match first {
            Object::Reference(r) => self.doc.resolve(*r).ok()?,
            other => Arc::new(other.clone()),
        };
        let descendant = descendant.as_dict()?;
        self.doc
            .resolve_dict_key(descendant, &Name::new("FontDescriptor"))
            .ok()
            .flatten()
            .and_then(|o| o.as_dict().cloned())
    }

    // --- XObjects ---------------------------------------------------------

    /// Handles `Do`: a Form XObject recurses (with its `/Matrix` and own
    /// `/Resources`); an Image XObject is recorded in the inventory.
    fn do_xobject(
        &mut self,
        xname: &str,
        gs: &GraphicsState,
        resources: &Dict,
        depth: u32,
        visited: &mut HashSet<u32>,
    ) {
        let Some(xobjects) = self
            .doc
            .resolve_dict_key(resources, &Name::new("XObject"))
            .ok()
            .flatten()
        else {
            return;
        };
        let Some(xdict) = xobjects.as_dict() else {
            return;
        };
        // Resolve the named XObject, tracking its object number for the cycle
        // guard (if it is an indirect reference).
        let obj_num = xdict
            .get(&Name::new(xname))
            .and_then(Object::as_reference)
            .map(|r| r.num);
        let Some(xobj) = self
            .doc
            .resolve_dict_key(xdict, &Name::new(xname))
            .ok()
            .flatten()
        else {
            return;
        };
        let Some(stream) = xobj.as_stream() else {
            return;
        };
        let subtype = stream
            .dict
            .get(&Name::new("Subtype"))
            .and_then(Object::as_name)
            .and_then(Name::as_str);

        match subtype {
            Some("Image") => {
                self.record_image_xobject(xname, stream, obj_num, gs);
            }
            Some("Form") | None => {
                // Depth + cycle guards.
                if depth + 1 > MAX_FORM_DEPTH {
                    return;
                }
                if let Some(num) = obj_num {
                    if !visited.insert(num) {
                        return; // cycle
                    }
                }
                // Form /Matrix premultiplies the CTM.
                let form_matrix = stream
                    .dict
                    .get(&Name::new("Matrix"))
                    .and_then(Object::as_array)
                    .and_then(array_to_matrix)
                    .unwrap_or(Matrix::IDENTITY);
                let inner_ctm = Matrix::concat(&form_matrix, &gs.ctm);

                // Form /Resources (fall back to the parent's per spec).
                let form_res = self
                    .doc
                    .resolve_dict_key(&stream.dict, &Name::new("Resources"))
                    .ok()
                    .flatten()
                    .and_then(|o| o.as_dict().cloned())
                    .unwrap_or_else(|| resources.clone());

                if let Ok(bytes) = self
                    .doc
                    .decode_stream(stream)
                    .and_then(|o| o.into_decoded())
                {
                    self.run(&bytes, &form_res, inner_ctm, depth + 1, visited);
                }
                if let Some(num) = obj_num {
                    visited.remove(&num);
                }
            }
            // Other subtypes (PS, …): ignore.
            _ => {}
        }
    }

    /// Records an Image XObject `Do` into the inventory (and, when recording, the
    /// ordered render-op stream with the raw image bytes for decode at replay).
    fn record_image_xobject(
        &mut self,
        name: &str,
        stream: &pdf_core::StreamObj,
        obj_num: Option<u32>,
        gs: &GraphicsState,
    ) {
        let dict = &stream.dict;
        let width = dict
            .get(&Name::new("Width"))
            .and_then(Object::as_i64)
            .and_then(|v| u32::try_from(v).ok());
        let height = dict
            .get(&Name::new("Height"))
            .and_then(Object::as_i64)
            .and_then(|v| u32::try_from(v).ok());
        self.out.images.push(ImageRef {
            name: Some(SmolStr::new(name)),
            inline: false,
            ctm: gs.ctm,
            width,
            height,
        });

        if self.recording() {
            if let Ok(raw) = self.doc.stream_raw_bytes(stream) {
                self.emit(RenderOp::Image(ImageOp {
                    dict: dict.clone(),
                    raw: raw.to_vec(),
                    obj_num,
                    ctm: gs.ctm,
                    fill_color: gs.fill_color,
                    alpha: gs.fill_alpha_u8(),
                }));
            }
        }
    }

    /// Emits a [`DrawPath`] for the current path under paint kind `kind`, then
    /// resets the path. Empty paths emit nothing. Colors come from the graphics
    /// state (stroke for `S`-kinds, fill for `f`-kinds; both for `B`-kinds).
    fn paint_path(
        &mut self,
        path: &mut CurrentPath,
        gs: &GraphicsState,
        kind: PaintKind,
        eo: bool,
    ) {
        if !path.items.is_empty() {
            let (color, fill) = match kind {
                PaintKind::Stroke => (Some(gs.stroke_color), None),
                PaintKind::Fill => (None, Some(gs.fill_color)),
                PaintKind::FillStroke => (Some(gs.stroke_color), Some(gs.fill_color)),
            };
            self.out.drawings.push(DrawPath {
                kind,
                rect: path.bounds(),
                color,
                fill,
                width: gs.line_width,
                dashes: gs.dashes.clone(),
                close_path: path.closed,
                even_odd: eo,
                items: path.items.clone(),
            });

            // Ordered render-op stream (M6): emit fill then stroke in z-order,
            // then any pending clip (W/W* applies *after* the paint).
            if self.recording() {
                let do_fill = matches!(kind, PaintKind::Fill | PaintKind::FillStroke);
                let do_stroke = matches!(kind, PaintKind::Stroke | PaintKind::FillStroke);
                if do_fill {
                    self.emit(RenderOp::Fill {
                        items: path.items.clone(),
                        close: path.closed,
                        color: gs.fill_color,
                        alpha: gs.fill_alpha_u8(),
                        even_odd: eo,
                    });
                }
                if do_stroke {
                    self.emit(RenderOp::Stroke {
                        items: path.items.clone(),
                        close: path.closed,
                        color: gs.stroke_color,
                        alpha: gs.stroke_alpha_u8(),
                        width: gs.line_width,
                        ctm: gs.ctm,
                        dashes: gs.dashes.clone(),
                    });
                }
                if let Some(clip_eo) = path.clip_pending.take() {
                    self.emit(RenderOp::Clip {
                        items: path.items.clone(),
                        even_odd: clip_eo,
                    });
                }
            }
        }
        path.reset();
    }

    /// Records an inline image (`BI…ID…EI`) into the inventory.
    fn record_inline_image(&mut self, params: &Object, data: Vec<u8>, gs: &GraphicsState) {
        let d = params.as_dict();
        let getint = |keys: &[&str]| -> Option<u32> {
            let d = d?;
            for k in keys {
                if let Some(v) = d.get(&Name::new(k)).and_then(Object::as_i64) {
                    return u32::try_from(v).ok();
                }
            }
            None
        };
        let width = getint(&["Width", "W"]);
        let height = getint(&["Height", "H"]);
        self.out.images.push(ImageRef {
            name: None,
            inline: true,
            ctm: gs.ctm,
            width,
            height,
        });

        if self.recording() {
            if let Some(dict) = params.as_dict() {
                self.emit(RenderOp::Image(ImageOp {
                    dict: dict.clone(),
                    raw: data,
                    obj_num: None,
                    ctm: gs.ctm,
                    fill_color: gs.fill_color,
                    alpha: gs.fill_alpha_u8(),
                }));
            }
        }
    }

    // --- ExtGState + shading (render-op recording only) -------------------

    /// Applies a `/gs` ExtGState dict's constant alpha (`/ca`, `/CA`) to the
    /// graphics state for the render sink. Other ExtGState keys (blend mode,
    /// soft masks, …) are a documented deferral.
    fn apply_extgstate(&mut self, ops: &[Object], gs: &mut GraphicsState, resources: &Dict) {
        let Some(gname) = ops.iter().find_map(|o| o.as_name().and_then(Name::as_str)) else {
            return;
        };
        let Some(egs) = self
            .doc
            .resolve_dict_key(resources, &Name::new("ExtGState"))
            .ok()
            .flatten()
        else {
            return;
        };
        let Some(egs) = egs.as_dict() else {
            return;
        };
        let Some(dict) = self
            .doc
            .resolve_dict_key(egs, &Name::new(gname))
            .ok()
            .flatten()
            .and_then(|o| o.as_dict().cloned())
        else {
            return;
        };
        if let Some(ca) = dict.get(&Name::new("ca")).and_then(Object::as_f64) {
            gs.fill_alpha = ca.clamp(0.0, 1.0);
        }
        if let Some(ca) = dict.get(&Name::new("CA")).and_then(Object::as_f64) {
            gs.stroke_alpha = ca.clamp(0.0, 1.0);
        }
    }

    /// Handles `sh`: resolves `/Resources /Shading /<name>` and emits a
    /// [`RenderOp::Shading`] carrying the shading dict for the renderer to parse.
    fn do_shading(&mut self, sname: &str, gs: &GraphicsState, resources: &Dict) {
        let Some(shadings) = self
            .doc
            .resolve_dict_key(resources, &Name::new("Shading"))
            .ok()
            .flatten()
        else {
            return;
        };
        let Some(shadings) = shadings.as_dict() else {
            return;
        };
        // A shading entry may be a dict or a stream (type 4–7). Resolve either.
        let Some(obj) = self
            .doc
            .resolve_dict_key(shadings, &Name::new(sname))
            .ok()
            .flatten()
        else {
            return;
        };
        let dict = match obj.as_ref() {
            Object::Dictionary(d) => d.clone(),
            Object::Stream(s) => s.dict.clone(),
            _ => return,
        };
        self.emit(RenderOp::Shading(ShadingOp {
            dict,
            ctm: gs.ctm,
            alpha: gs.fill_alpha_u8(),
        }));
    }
}

// === free helpers =========================================================

/// Emits one positioned glyph for `code` into `out` and advances `tm`. Free
/// function (not a method) so the caller can hold an immutable borrow of
/// `cached` across the show-op loop while pushing into `out` — avoiding a
/// per-glyph font-cache lookup. Geometry is identical to the previous method.
#[allow(clippy::too_many_arguments)]
fn emit_glyph_into(
    out: &mut Vec<PositionedGlyph>,
    cached: &CachedFont,
    code: u32,
    n_bytes: u8,
    font_name: &SmolStr,
    gs: &GraphicsState,
    tm: &mut Matrix,
) {
    let ts = &gs.text;
    let w0 = cached.mapper.width(code) / 1000.0; // glyph advance, text units
    let unicode = cached.mapper.to_unicode(code).unwrap_or_default();

    // params = [Tfs·Th, 0, 0, Tfs, 0, Trise]
    let params = Matrix::new(
        ts.font_size * ts.h_scale,
        0.0,
        0.0,
        ts.font_size,
        0.0,
        ts.rise,
    );
    // Trm = params · Tm · CTM
    let trm = Matrix::concat(&Matrix::concat(&params, tm), &gs.ctm);

    // Glyph origin = (0,0) · Trm.
    let origin = Point::new(0.0, 0.0).transform(&trm);

    // Glyph cell in 1000-unit glyph space → text space is /1000, then params
    // already carries the size scaling. Build the cell in the *unit* space
    // params operates on: x ∈ [0, w0], y ∈ [descent/1000, ascent/1000].
    let asc = cached.ascent / 1000.0;
    let desc = cached.descent / 1000.0;
    let cell = Rect::new(0.0, desc, w0, asc);
    // Transform the cell by Trm and take the axis-aligned envelope (correct
    // for rotated Tm).
    let bbox = cell.transform(&trm);

    out.push(PositionedGlyph {
        unicode,
        code,
        origin: sanitize_point(origin),
        bbox: sanitize_rect(bbox),
        font_name: font_name.clone(),
        size: ts.font_size,
        color: gs.fill_color,
        render_mode: ts.render_mode,
        writing_dir: WritingDir::Horizontal,
        ascender: asc,
        descender: desc,
    });

    // Advance: tx = ((w0)·Tfs + Tc + Tw_if_space)·Th  (w0 already /1000).
    let is_space = n_bytes == 1 && code == 0x20;
    let tw = if is_space { ts.word_spacing } else { 0.0 };
    let tx = (w0 * ts.font_size + ts.char_spacing + tw) * ts.h_scale;
    *tm = Matrix::concat(&Matrix::translate(tx, 0.0), tm);
}

/// The `n`-th numeric operand as `f64` (operands are in source order).
fn nth_f64(ops: &[Object], n: usize) -> Option<f64> {
    ops.get(n).and_then(Object::as_f64)
}

/// The first six numeric operands as a fixed array (for `c`/`cm`).
fn six_f64(ops: &[Object]) -> Option<[f64; 6]> {
    let nums: Vec<f64> = ops.iter().filter_map(Object::as_f64).collect();
    if nums.len() < 6 {
        return None;
    }
    Some([nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]])
}

/// The first four numeric operands as a fixed array (for `v`/`y`/`re`).
fn four_f64(ops: &[Object]) -> Option<[f64; 4]> {
    let nums: Vec<f64> = ops.iter().filter_map(Object::as_f64).collect();
    if nums.len() < 4 {
        return None;
    }
    Some([nums[0], nums[1], nums[2], nums[3]])
}

/// Formats a `d` dash operator (`[array] phase`) back into a stable string, e.g.
/// `"[3 2] 0"`. An empty array means a solid line → empty string.
fn format_dash(ops: &[Object]) -> String {
    let Some(arr) = ops.iter().find_map(Object::as_array) else {
        return String::new();
    };
    if arr.is_empty() {
        return String::new();
    }
    let phase = ops.iter().rev().find_map(Object::as_f64).unwrap_or(0.0);
    let nums: Vec<String> = arr
        .iter()
        .filter_map(Object::as_f64)
        .map(fmt_dash_num)
        .collect();
    format!("[{}] {}", nums.join(" "), fmt_dash_num(phase))
}

/// Formats a dash scalar without trailing zeros (integral → no point).
fn fmt_dash_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let mut s = format!("{v:.4}");
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        s
    }
}

/// Builds a `Matrix` from the first six numeric operands (`a b c d e f`).
fn matrix_from(ops: &[Object]) -> Option<Matrix> {
    let nums: Vec<f64> = ops.iter().filter_map(Object::as_f64).collect();
    if nums.len() < 6 {
        return None;
    }
    Some(Matrix::new(
        nums[0], nums[1], nums[2], nums[3], nums[4], nums[5],
    ))
}

/// Converts a 6-element PDF array to a `Matrix` (for a form `/Matrix`).
fn array_to_matrix(arr: &[Object]) -> Option<Matrix> {
    if arr.len() < 6 {
        return None;
    }
    let v: Vec<f64> = arr.iter().take(6).filter_map(Object::as_f64).collect();
    if v.len() < 6 {
        return None;
    }
    Some(Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5]))
}

/// The first string operand's bytes (for `Tj`).
fn first_string(ops: &[Object]) -> Option<Vec<u8>> {
    ops.iter()
        .find_map(|o| o.as_string().map(|s| s.as_bytes().to_vec()))
}

/// The last string operand's bytes (for `"`).
fn last_string(ops: &[Object]) -> Option<Vec<u8>> {
    ops.iter()
        .rev()
        .find_map(|o| o.as_string().map(|s| s.as_bytes().to_vec()))
}

/// Packs three 0..=1 channels into `0x00RRGGBB`.
fn pack_rgb(r: f64, g: f64, b: f64) -> u32 {
    let q = |v: f64| -> u32 { (v.clamp(0.0, 1.0) * 255.0).round() as u32 };
    (q(r) << 16) | (q(g) << 8) | q(b)
}

/// Gray → packed sRGB.
fn gray_rgb(g: f64) -> u32 {
    pack_rgb(g, g, g)
}

/// CMYK → packed sRGB (naive conversion; sufficient for span color recording).
fn cmyk_rgb(c: f64, m: f64, y: f64, k: f64) -> u32 {
    let c = c.clamp(0.0, 1.0);
    let m = m.clamp(0.0, 1.0);
    let y = y.clamp(0.0, 1.0);
    let k = k.clamp(0.0, 1.0);
    let r = (1.0 - c) * (1.0 - k);
    let g = (1.0 - m) * (1.0 - k);
    let b = (1.0 - y) * (1.0 - k);
    pack_rgb(r, g, b)
}

/// Approximates a packed sRGB from a generic `sc`/`scn` component list.
fn approx_color(comps: &[f64]) -> Option<u32> {
    match comps.len() {
        1 => Some(gray_rgb(comps[0])),
        3 => Some(pack_rgb(comps[0], comps[1], comps[2])),
        4 => Some(cmyk_rgb(comps[0], comps[1], comps[2], comps[3])),
        _ => None,
    }
}

/// Replaces non-finite coordinates with 0 so a glyph always has a usable point.
fn sanitize_point(p: Point) -> Point {
    Point::new(finite(p.x), finite(p.y))
}

/// Replaces non-finite rect edges with 0 (never emit NaN/Inf — PRD §8.6.2).
fn sanitize_rect(r: Rect) -> Rect {
    Rect::new(finite(r.x0), finite(r.y0), finite(r.x1), finite(r.y1))
}

#[inline]
fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}
