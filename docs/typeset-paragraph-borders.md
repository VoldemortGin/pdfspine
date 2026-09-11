# Solid paragraph borders

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

Only solid RGB edges are represented. There is no `between` separator, dashed,
double, art, shadow or frame slot, and no OOXML parser or theme-color resolution
is added. In particular, unsupported `between` must not be approximated as two
ordinary paragraph rectangles by a consumer.

Borders participate in vertical layout. A 1pt top border with 4pt space moves
text down 5pt; the bottom extent also contributes to measured height and page
fit. Side edges extend outward from the paragraph text bounds; positive first-line
indent changes text only, while hanging indent extends the border's left bound.
Matching sibling paragraphs (same four optional stroke widths/colors and text
bounds) share outer top/bottom edges. Their ordinary before/after spaces remain
additive. Different per-edge border spaces do not break a group: internal top and
bottom edges/spaces are suppressed, and each paragraph retains its own side
positions. No horizontal step connector is invented when side positions differ.
Different stroke styles, hanging/text bounds or an intervening block end a group.

Each page fragment gets its own top and bottom edges and reserves their extents.
A line too tall for an empty page overflows once, consistent with the existing
engine policy; it cannot cause an empty-page loop. A styled empty paragraph has
its normal line box; a paragraph without a usable run keeps the existing no-line
behavior. Group state is local to each sibling block list, so it does not cross
cells, text boxes, images, tables or explicit page breaks.

Borders reuse `Op::Line`. Shading remains behind text; border lines are emitted
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

Ten focused Rust cases cover validation/defaults, geometry, grouping, page-fit
reserve/oversized input, empty paragraphs, measurement/containers/asymmetric edges, shading with
tracking/decorations/links and font autofit. The typeset readback gate covers five
documents (order/F1 1.0); the render reference gate covers eight pages (minimum
0.9997). Source, probe PDFs, hashes and logs are retained externally under
`/Volumes/ExternalSSD/tmp/typeset-border-readonly/`. Full integration validation is
recorded separately; this document describes the isolated increment.
