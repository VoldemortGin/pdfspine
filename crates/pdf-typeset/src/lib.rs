#![forbid(unsafe_code)]
//! `pdf-typeset` — the shared typesetting engine behind faithful `.docx` /
//! `.pptx` → PDF **export** (PRD §10, Phase A).
//!
//! This is an **original pdfspine extension**: docspine's `doc-render` and
//! pptspine's `ppt-render` build a [`model`] document (styled runs, paragraph
//! properties, tables, text boxes) and this crate resolves fonts, lays the
//! content out and emits PDF ops through `pdf-edit`. It is deliberately not a
//! Story / HTML+CSS engine and does no shaping/kerning/ligatures — advances
//! stay strictly additive per character.
//!
//! # Phase A module map (PRD §10 design sketch)
//!
//! - [`model`] — the layout-ready input IR (TS-1, this crate's public input).
//! - [`warn`] — [`ExportWarning`]: every unsupported-feature degradation is
//!   enumerated, degrade-never-panic (TS-1).
//! - [`ops`] — the positioned draw-op IR shared by the layout stages (TS-1;
//!   generalizes `pdf-markdown`'s op vocabulary with size-carrying text per
//!   [`ops::FaceId`] plus shape / alpha / clip / transform ops).
//! - [`fontres`] — fontdb-backed system-font resolution: folded-name index,
//!   three-platform substitution tables, weight/style query mapping, per-char
//!   fallback chain, bundled Liberation/Noto final fallback (TS-2).
//! - `faces` — the [`ops::FaceId`] registry: one id / one parse / one embed
//!   per distinct face per export run, real font metrics (TS-3/TS-4).
//! - [`flow`] — measure → wrap → paginate: mixed faces/sizes on shared
//!   baselines, justify by space redistribution, decorations, indents, line
//!   spacing, [`flow::PageProvider`] pagination (TS-4).
//! - `table` — grid measure (fixed/auto + fair-share shrink), cell layout,
//!   per-edge borders (TS-4).
//! - `boxes` — absolutely-positioned text boxes: [`VAnchor`], wrap-off,
//!   `normAutofit` scaling, rotation, clipping (TS-5).
//! - `emit` — op IR → content streams → deterministic PDF bytes (TS-4).
//! - [`preset`] — pptx autoshape outlines (TS-6).
//!
//! # Driving the engine
//!
//! [`Typesetter`] is the per-export-run facade: consumers lay out flowing
//! blocks ([`Typesetter::layout_flow`]) and/or absolutely-positioned text
//! boxes ([`Typesetter::layout_text_box`], assembling [`PageOps`] themselves
//! for slides), then serialize once via [`Typesetter::emit`], which returns
//! the PDF bytes together with every accumulated [`ExportWarning`].
//!
//! # Determinism contract
//!
//! Weaker than `pdf-markdown`'s absolute one: same machine + same installed
//! fonts ⇒ identical bytes; cross-machine output may differ (system-font
//! resolution). The [`fontres::FontResolver::without_system_fonts`] constructor
//! (bundled faces only) restores full determinism for tests.

mod boxes;
mod emit;
mod faces;
pub mod flow;
pub mod fontres;
pub mod model;
pub mod ops;
pub mod preset;
mod table;
pub mod warn;

use std::collections::{HashMap, HashSet};

// --- re-exported consumer surface (single pdfspine git dep for doc-render /
// --- ppt-render; PRD §10 consumer-wiring precedent) -------------------------
pub use pdf_core::error::{Error, Result};
pub use pdf_core::geom::{Matrix, Point, Rect};
/// The RGB color type used across the input model and op IR (each component in
/// `0.0..=1.0`; `pdf_edit::Color` re-exported under the PRD §10 model name).
pub use pdf_edit::Color as Rgb;

pub use flow::{FixedPages, LineMetrics, Measurement, PageGeom, PageProvider};
pub use fontres::{FontResolver, Platform, ResolvedFace, Substitutions};
pub use model::{
    Align, Block, BorderEdge, CellBorders, ColumnWidth, ImageSpec, LineHeightRule, LineSpacing,
    ListLabel, ParaProps, Run, RunStyle, TableCell, TableRow, TableSpec, TextBoxSpec, VAnchor,
};
pub use ops::{FaceId, Fill, LineCap, LineJoin, Op, PageOps, PathSeg, Stroke};
pub use warn::ExportWarning;

/// Word's default tab-stop interval: 0.5 inch = 36 points.
const DEFAULT_TAB_INTERVAL: f64 = 36.0;

/// The result of one export run: the serialized PDF plus every degradation
/// that occurred while producing it (consumers surface these in Python via
/// `warnings.warn`).
#[derive(Clone, Debug)]
pub struct ExportResult {
    /// The serialized PDF bytes.
    pub pdf: Vec<u8>,
    /// Every unsupported-feature degradation, in occurrence order.
    pub warnings: Vec<ExportWarning>,
}

/// The per-export-run engine: a [`FontResolver`] behind memoized style / char
/// resolution, the face registry (one [`FaceId`] / one parse / one embed per
/// face per document), prepared images, and the deduplicated warning channel.
///
/// One `Typesetter` produces one document: lay out with
/// [`Typesetter::layout_flow`] / [`Typesetter::layout_text_box`] (and place
/// images via [`Typesetter::add_image`] when assembling [`PageOps`] directly),
/// then consume it with [`Typesetter::emit`].
pub struct Typesetter {
    resolver: FontResolver,
    faces: faces::FaceRegistry,
    images: Vec<emit::PreparedImage>,
    warnings: Vec<ExportWarning>,
    /// (family, bold, italic) → resolved base face (memoized so the
    /// substitution / style warnings fire once per style).
    styles: HashMap<(String, bool, bool), ResolvedFace>,
    /// (base face, char) → drawing face (memoized per-char fallback).
    chars: HashMap<(fontres::FaceKey, char), FaceId>,
    /// (family, char) pairs already reported as glyph fallbacks (the TS-4
    /// warning dedup: `resolve_char` fires per occurrence, the layout layer
    /// reports each miss once).
    glyph_warned: HashSet<(String, char)>,
    /// Tab-stop interval in points: a `\t` advances the pen to the next
    /// multiple of this (Word's `defaultTabStop`, 0.5 inch by default).
    tab_interval: f64,
    /// How line-box heights / baselines are derived from the runs on each line
    /// (real face metrics vs PowerPoint's font-independent 1.2-em spacing).
    line_rule: LineHeightRule,
}

impl Typesetter {
    /// An engine over `resolver` (inject a deterministic
    /// [`FontResolver::with_platform`] resolver for tests).
    #[must_use]
    pub fn new(resolver: FontResolver) -> Self {
        Typesetter {
            resolver,
            faces: faces::FaceRegistry::new(),
            images: Vec::new(),
            warnings: Vec::new(),
            styles: HashMap::new(),
            chars: HashMap::new(),
            glyph_warned: HashSet::new(),
            tab_interval: DEFAULT_TAB_INTERVAL,
            line_rule: LineHeightRule::FontMetrics,
        }
    }

    /// An engine over the system fonts (deterministic per font environment
    /// only — the PRD §10 contract).
    #[must_use]
    pub fn with_system_fonts() -> Self {
        Typesetter::new(FontResolver::with_system_fonts())
    }

    /// The underlying font resolver.
    #[must_use]
    pub fn resolver(&self) -> &FontResolver {
        &self.resolver
    }

    /// Mutable resolver access (inject document-embedded fonts via
    /// `add_font_data`, extend the substitution table, …). Configure fonts
    /// **before** laying out — resolution results are memoized per style.
    pub fn resolver_mut(&mut self) -> &mut FontResolver {
        &mut self.resolver
    }

    /// Debug flag: embed whole font programs instead of usage-based glyph
    /// subsets (PRD §10 TS-3 keeps the full embed behind this flag only).
    pub fn set_full_embed(&mut self, full_embed: bool) {
        self.faces.set_full_embed(full_embed);
    }

    /// Sets the tab-stop interval in points (Word's `defaultTabStop`): a `\t`
    /// advances the pen to the next multiple of this. Non-finite / non-positive
    /// values are ignored, keeping the 0.5-inch default.
    pub fn set_tab_interval(&mut self, points: f64) {
        if points.is_finite() && points > 0.0 {
            self.tab_interval = points;
        }
    }

    /// The current tab-stop interval in points (used by the layout core).
    pub(crate) fn tab_interval(&self) -> f64 {
        self.tab_interval
    }

    /// Sets how each line's natural height and baseline are derived from its
    /// runs ([`LineHeightRule`]): real face metrics (Word / Writer, the
    /// default) or PowerPoint's font-independent 1.2-em spacing (pptx /
    /// Impress). Configure this **before** laying out; it applies engine-wide —
    /// to flow, text boxes, table cells and the measure API — so
    /// [`Typesetter::measure_blocks`] / [`Typesetter::measure_text_box`] and the
    /// `layout_*` methods always agree. pptspine sets it once per render.
    pub fn set_line_height_rule(&mut self, rule: LineHeightRule) {
        self.line_rule = rule;
    }

    /// The current line-height rule (used by the layout core).
    #[must_use]
    pub fn line_height_rule(&self) -> LineHeightRule {
        self.line_rule
    }

    /// The warnings accumulated so far (moved into [`ExportResult`] by
    /// [`Typesetter::emit`]).
    #[must_use]
    pub fn warnings(&self) -> &[ExportWarning] {
        &self.warnings
    }

    /// Prepares an image for embedding and returns its id for
    /// [`Op::Image`]. An undecodable image records an
    /// [`ExportWarning::ImageDropped`] and returns `None` (degrade-never-panic).
    pub fn add_image(&mut self, spec: &ImageSpec) -> Option<usize> {
        match emit::prepare_image(&spec.data) {
            Ok(img) => {
                self.images.push(img);
                Some(self.images.len() - 1)
            }
            Err(e) => {
                self.warnings.push(ExportWarning::ImageDropped {
                    reason: e.to_string(),
                });
                None
            }
        }
    }

    /// Lays out `blocks` as paginated flow (docspine body); `pages` supplies
    /// each started page's geometry (per-section page size / margins). Always
    /// returns at least one page.
    pub fn layout_flow(&mut self, blocks: &[Block], pages: &mut dyn PageProvider) -> Vec<PageOps> {
        flow::layout_flow(self, blocks, pages)
    }

    /// Lays out one absolutely-positioned text box (pptx shape text body /
    /// docx text box) and returns its page-coordinate ops, ready to append to
    /// a page's [`PageOps::ops`] (TS-5: vertical anchor, wrap-off,
    /// `normAutofit` scaling, rotation, clipping).
    pub fn layout_text_box(&mut self, spec: &TextBoxSpec) -> Vec<Op> {
        boxes::layout_text_box(self, spec)
    }

    /// Measures `blocks` at a fixed content `width` **without emitting**,
    /// returning per-line metrics and totals (TS-10). It shares the exact
    /// measure → wrap → line-box path the emitters run, so
    /// [`Measurement::height`] equals the content height
    /// [`Typesetter::layout_text_box`] (and box-mode flow) produce for the same
    /// input. `wrap == false` breaks lines only at hard `\n` (the text-box
    /// wrap-off mode); pass `true` for the normal wrap-to-width behaviour.
    ///
    /// 在给定宽度下度量 `blocks`（不产出 PDF），返回每行的 ascent/descent/行高
    /// 与总高度、内容自然宽度。消费方用它做 autofit、表格按内容增高、单元格
    /// 垂直对齐等——度量结果与真实排版逐点一致。
    pub fn measure_blocks(&mut self, blocks: &[Block], width: f64, wrap: bool) -> Measurement {
        flow::measure_box_content(self, blocks, width, wrap)
    }

    /// Measures a [`TextBoxSpec`]'s content at its rect width and wrap mode
    /// (TS-10): the natural line metrics / height at `font_scale` 1.0 — i.e.
    /// **before** any autofit shrinking and independent of vertical anchoring,
    /// rotation and clipping. Consumers use it to drive their own autofit or
    /// grow-to-content boxes; the reported height is what
    /// [`Typesetter::layout_text_box`] anchors within the rect when autofit is
    /// off.
    ///
    /// 按文本框矩形宽度与换行模式度量其内容（`font_scale` 视为 1.0，不含
    /// autofit 缩放、垂直锚定、旋转与裁剪）。
    pub fn measure_text_box(&mut self, spec: &TextBoxSpec) -> Measurement {
        let width = (spec.rect.x1 - spec.rect.x0).abs().max(1.0);
        flow::measure_box_content(self, &spec.blocks, width, spec.wrap)
    }

    /// Serializes the laid-out pages into the final PDF, consuming the engine
    /// (faces, glyph usage and images are document-scoped). Deterministic for
    /// a fixed font environment.
    ///
    /// # Errors
    ///
    /// Propagates `pdf-core` object/write errors (never panics).
    pub fn emit(self, pages: &[PageOps]) -> Result<ExportResult> {
        let pdf = emit::build_pdf(pages, &self.faces, &self.images)?;
        Ok(ExportResult {
            pdf,
            warnings: self.warnings,
        })
    }

    // --- crate-internal resolution plumbing (layout stages) ------------------

    /// Records a degradation.
    pub(crate) fn warn(&mut self, warning: ExportWarning) {
        self.warnings.push(warning);
    }

    /// The face registry (measurement + emission access).
    pub(crate) fn faces(&self) -> &faces::FaceRegistry {
        &self.faces
    }

    /// Resolves a run style to its base face, memoized per
    /// (family, bold, italic) — `FontSubstituted` / `StyleApproximated`
    /// warnings fire exactly once per distinct style.
    pub(crate) fn base_face(&mut self, style: &RunStyle) -> ResolvedFace {
        let key = (style.family.clone(), style.bold, style.italic);
        if let Some(face) = self.styles.get(&key) {
            return face.clone();
        }
        let face =
            self.resolver
                .resolve(&style.family, style.bold, style.italic, &mut self.warnings);
        self.styles.insert(key, face.clone());
        face
    }

    /// Interns a resolved face directly (no per-char fallback) — reference
    /// metrics for empty lines.
    pub(crate) fn face_id(&mut self, base: &ResolvedFace) -> FaceId {
        self.faces.intern(&self.resolver, base)
    }

    /// Resolves one character against `base` through the per-char fallback
    /// chain and interns the winning face. Memoized per (face, char);
    /// `GlyphFallback` warnings are deduplicated per (family, char) — the
    /// TS-4 layout-layer dedup over `resolve_char`'s per-occurrence firing.
    pub(crate) fn char_face(&mut self, base: &ResolvedFace, ch: char) -> FaceId {
        let key = (base.key(), ch);
        if let Some(&id) = self.chars.get(&key) {
            return id;
        }
        let mut local = Vec::new();
        let face = self.resolver.resolve_char(base, ch, &mut local);
        for warning in local {
            match &warning {
                ExportWarning::GlyphFallback { ch, family } => {
                    if self.glyph_warned.insert((family.clone(), *ch)) {
                        self.warnings.push(warning);
                    }
                }
                _ => self.warnings.push(warning),
            }
        }
        let id = self.faces.intern(&self.resolver, &face);
        self.chars.insert(key, id);
        id
    }
}
