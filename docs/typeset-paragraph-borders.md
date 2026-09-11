# Paragraph borders

This unreleased increment adds native paragraph borders to `pdf-typeset` flow,
text boxes and table cells. It does not change consumer dependency pins or the
existing table-cell border API. Some docspine exports already use a one-cell
wrapper fallback; native paragraph support avoids requiring that wrapper in a
future consumer migration, which is separate work.

`ParaProps.borders` is `Option<Box<ParagraphBorders>>`, default `None` (no extra
allocation). Each optional top/right/bottom/left edge is a `ParagraphBorder`
constructed from `BorderEdge { width, color }` and text-to-border space in points.
The constructor rejects nonfinite/nonpositive widths, negative/nonfinite space,
and an overflowing combined edge extent. Private edge fields preserve validation;
`stroke()` and `space()` return values. An empty edge set paints nothing.

Solid RGB edges and explicit two-length dashed RGB edges are represented.
There is no `between` separator, double, art, shadow or frame slot, and no OOXML parser or theme-color resolution
is added. In particular, unsupported `between` must not be approximated as two
ordinary paragraph rectangles by a consumer.

Borders participate in vertical layout. A 1pt top border with 4pt space moves
text down 5pt; the bottom extent also contributes to measured height and page
fit. Side edges extend outward from the paragraph text bounds; positive first-line
indent changes text only, while hanging indent extends the border's left bound.
Matching sibling paragraphs (same four optional stroke widths/colors/dash pairs and text
bounds) share outer top/bottom edges. Their ordinary before/after spaces remain
additive. Different per-edge border spaces do not break a group: internal top and
bottom edges/spaces are suppressed, and each paragraph retains its own side
positions. No horizontal step connector is invented when side positions differ.
Different stroke styles, hanging/text bounds or an intervening block end a group.

Each page fragment gets its own top and bottom edges and reserves their extents.
For groups with different bottom extents, bounded lookahead uses actual wrapped
line heights and paragraph gaps to choose a valid page-closing edge. This avoids
premature breaks caused by reserving an internal edge that a following paragraph
will suppress. Future-paragraph wrapping is replanned when the current page width
changes; default-off and uniform-bottom groups do not need this prediction.
A 42pt group with a suppressed 50pt internal bottom fits the remaining 60pt in
both the regression and LO. The paired true-split case restores the first page's
50pt bottom extent and the next page's top edge, with all text inside the fragments.
A line too tall for an empty page overflows once, consistent with the existing
engine policy; it cannot cause an empty-page loop. A styled empty paragraph has
its normal line box; a paragraph without a usable run keeps the existing no-line
behavior. Group state is local to each sibling block list, so it does not cross
cells, text boxes, images, tables or explicit page breaks.

Solid borders retain the original `Op::Line` path and serialized bytes.
Explicit dashed borders use independent two-point `Op::Path` strokes. Shading remains behind text; border lines are emitted
after the paragraph group's fills/text. Textbox anchoring and cell row growth use
the bordered measured height. Font autofit shrinks font/tracking advances but
retains paragraph geometry, including border dimensions. Explicit textbox clipping
continues to clip outward side strokes, and rotations/transforms use the existing
op-group path. This does not alter the semantics of clipping or cell borders.

## Evidence

Real LibreOffice 26.8.0.3 probes establish single/adjacent paragraphs, differing
stroke color, first-line/hanging indents, additive paragraph spacing, asymmetric
per-edge spaces, styled empty paragraphs, table cells, a VML textbox and a
three-page paragraph. Some LO PDFs duplicate stroke commands; comparisons use
geometry/pixels rather than raw op counts. An early probe with duplicate
`w:spacing` XML elements was discarded and replaced by legal single-element
probes. This is LO evidence, not an assertion that every Word border option has
been verified.

The deterministic `typeset-lo-border.pdf` matches the DOCX authored independently
by `typeset_lo_oracle.py`. Against this border DOCX, the previous borderless engine
scores **0.9527**, and the new engine **0.9836** at 100dpi. Existing DOCX/PPTX/shading/
tracking comparisons remain **0.9822 / 0.9780 / 0.9868 / 0.9749**. All six prior
engine PDF fixtures remain byte-identical. Only the new fixture, its license/hash
manifest entry, readback entry and SSIM reference are added; old references are
unchanged. LO stays local-only and advisory, not a CI dependency.

Fourteen focused Rust cases cover validation/defaults, geometry, grouping, page-fit
reserve/oversized input, empty paragraphs, measurement/containers/asymmetric edges, shading with
tracking/decorations/links and font autofit. The typeset readback gate covers five
documents (order/F1 1.0); the render reference gate covers eight pages (minimum
0.9997). Source, probe PDFs, hashes and logs are retained externally under
`/Volumes/ExternalSSD/tmp/typeset-border-readonly/`. Full integration validation is
recorded separately; this document describes the isolated increment.

## Explicit caller-resolved dash pairs (unreleased)

`ParagraphBorder::new(stroke, space)?.with_dash(8.0, 2.5)?` sets on/off
lengths in points; `dash()` returns `Some([on, off])`. The default is `None`.
Both lengths must be finite and positive **after the existing PDF scalar formatter**
(four decimal places where needed), represent positive finite `f32` values, and
have a finite `f32` cycle sum. Values rounding to zero and unrenderable large
cycles are rejected. This validates unit-scale strokes; arbitrary extreme caller
transforms are not guaranteed. No guessed epsilon or OOXML enum conversion is used.

Each emitted edge starts at phase zero with butt caps. Phase restarts for every
paragraph side and page fragment; it is not carried through corners or joined
siblings. Dash pairs participate in sibling grouping; spaces retain the existing
rules above. Dash dimensions, like solid border widths, stay fixed during font
autofit. Measurement, reserve, clipping and container placement are unchanged.

Self-authored DOCX probes rendered by LibreOffice 26.8.0.3 expose `[8 2.5] 0`
for `dashed` at both 1pt and 3pt stroke widths, and `[3 1] 0` for `dashSmallGap`
at 1pt, with butt caps. These observations do not define an automatic mapping.
The new explicit 8/2.5pt fixture scores 0.9803 against the independent dashed
DOCX at 100dpi (the old solid fixture scores 0.9801). This advisory whole-page
comparison is not proof of matching corner/endpoint phase or Word behavior.
Independent pixel tests check on/off interiors under the declared phase-zero
contract; default solid output keeps all ten prior generated PDFs byte-identical.

The deterministic fixture and local LO generator are committed; probe variants,
vector output, versions, input/output SHA256s and red/green logs are retained at
`/Volumes/ExternalSSD/tmp/typeset-dash-probe/`. Regenerate the engine fixture with
`cargo run -p pdf-typeset --example make_typeset_fixtures`; the existing
`conformance/gt/typeset_lo_oracle.py` command now includes the dashed DOCX pair.
Run `cargo test -p pdf-typeset --test paragraph_dash` for geometry, grouping,
page fragments, measurement, autofit, table-cell and pixel regressions.

Isolated validation passes 217 Rust tests (including seven new dash cases),
all-target/all-feature crate clippy, rustdoc with warnings denied, 9/9 readback
PDFs and 12/12 render-reference pages (minimum SSIM 0.9991). The initial reference
gate reported a missing new reference because its filename included `.pdf`;
correcting only that new filename produced the passing run. Old references were
not refreshed. These focused results are not a new complete workspace gate.
