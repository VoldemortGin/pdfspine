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

use crate::model::{ImageRef, InterpretResult, PositionedGlyph, WritingDir};
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
}

/// Runs a page (or a form/resources pair) and produces positioned glyphs.
pub struct ContentInterpreter<'a> {
    doc: &'a DocumentStore,
    out: InterpretResult,
}

impl<'a> ContentInterpreter<'a> {
    /// Creates an interpreter bound to a document store.
    #[must_use]
    pub fn new(doc: &'a DocumentStore) -> Self {
        ContentInterpreter {
            doc,
            out: InterpretResult::default(),
        }
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

        for ev in events {
            match ev {
                Event::Operand(o) => ops.push(o),
                Event::InlineImage { params, data: _ } => {
                    self.record_inline_image(&params, gs.ctm);
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
        resources: &Dict,
        font_cache: &mut std::collections::HashMap<SmolStr, Option<CachedFont>>,
        depth: u32,
        visited: &mut HashSet<u32>,
    ) {
        match name {
            // --- graphics state ------------------------------------------
            b"q" => stack.push(gs.clone()),
            b"Q" => {
                if let Some(prev) = stack.pop() {
                    *gs = prev;
                }
            }
            b"cm" => {
                if let Some(m) = matrix_from(ops) {
                    // CTM' = m · CTM (the new matrix premultiplies).
                    gs.ctm = Matrix::concat(&m, &gs.ctm);
                }
            }

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
        let cached = match font_cache.get(&font_name).and_then(Option::as_ref) {
            Some(c) => c,
            None => return,
        };
        // Collect codes up front (the borrow on `cached` ends before mutation).
        let codes: Vec<(u32, u8)> = cached.mapper.iter_codes(bytes).collect();
        for (code, n_bytes) in codes {
            self.emit_glyph(code, n_bytes, &font_name, gs, tm, font_cache);
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

    /// Emits one positioned glyph for `code` and advances `tm`.
    fn emit_glyph(
        &mut self,
        code: u32,
        n_bytes: u8,
        font_name: &SmolStr,
        gs: &GraphicsState,
        tm: &mut Matrix,
        font_cache: &mut std::collections::HashMap<SmolStr, Option<CachedFont>>,
    ) {
        let Some(cached) = font_cache.get(font_name).and_then(Option::as_ref) else {
            return;
        };
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

        self.out.glyphs.push(PositionedGlyph {
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
                self.record_image_xobject(xname, &stream.dict, gs.ctm);
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

    /// Records an Image XObject `Do` into the inventory.
    fn record_image_xobject(&mut self, name: &str, dict: &Dict, ctm: Matrix) {
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
            ctm,
            width,
            height,
        });
    }

    /// Records an inline image (`BI…ID…EI`) into the inventory.
    fn record_inline_image(&mut self, params: &Object, ctm: Matrix) {
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
            ctm,
            width,
            height,
        });
    }
}

// === free helpers =========================================================

/// The `n`-th numeric operand as `f64` (operands are in source order).
fn nth_f64(ops: &[Object], n: usize) -> Option<f64> {
    ops.get(n).and_then(Object::as_f64)
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
