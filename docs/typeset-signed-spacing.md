# Resolved signed character spacing

This unreleased Rust typesetting increment adds explicit condensed cluster gaps.
It does not parse OOXML, infer a LibreOffice saturation constant, or change
consumer versions. The existing `RunStyle.character_spacing` field remains the
single source of spacing. `CharacterSpacing::new` still rejects negative and
nonfinite values; `CharacterSpacing::resolved_signed` accepts finite signed
points. Zero/positive values are equivalent through both constructors, and
paragraphs without a negative value retain their existing layout path.

## Placement and scope

A base scalar and its following recognized combining marks consume one gap,
attributed to the base's style, including across run/style/size boundaries.
This is not full grapheme shaping. Spaces have the existing space advance;
tabs remain tab-stop operations. Soft wraps and paragraph ends omit the final
gap, while explicit hard breaks retain it. There is no requested-spacing clamp.
Glyph script scale does not implicitly scale the gap; text-box autoshrink scales
size, gap and resolved baseline shift once together.

The first signed implementation supports forward cluster advances with finite
arithmetic and a rounding margin. Unsupported font/text-dependent condensation
is diagnosed, not converted to a guessed cap. A final omitted gap does not
require an artificial forward move. Side-by-side mixed-size glyphs use the
rightmost **advance-cell edge**, separately from the condensed pen. Highlights,
underlines, strikeouts, link rectangles, wrapping and measurement retain that
advance-cell coverage. This is the existing font-advance metric, **not actual
outline ink bounds**; italic overhang and full shaping remain outside this work.

## Checked and legacy entry points

`Typesetter::try_layout_flow`, `try_layout_text_box`, `try_measure_blocks`, and
`try_measure_text_box` return `Result<_, SignedSpacingError>`. The error carries
`path: Vec<SpacingPathStep>` (block/row/cell indices), `run_index`, UTF-8
`byte_offset`, and `SignedSpacingReason`. Preflight uses actual font fallback,
effective script size and cluster advances, including nested cells. It collects all affected paragraph diagnostics in one bounded preparation
(no per-error layout retries). A failing flow spacing preflight occurs before
calling `PageProvider` and returns no partial operations. Font resolution may
warm caches and add existing fallback warnings; this is not a full memory-state
transaction or a new guarantee about arbitrary provider geometry.

Text-box layout additionally checks the normal finite proportional scale
interval used by autofit: original, requested and minimum scale, with normal
size/gap and a cancellation margin. This rejects scale underflow before drawing.
Natural text-box measurement remains at scale 1, as before. Flow does not invoke
text-box autofit and has no hidden provider-dependent scale. The validation
scope is signed-spacing arithmetic; unrelated malformed legacy styles retain
their previous behavior.

Existing infallible methods share the preflight. Supported signed input works
normally. For unsupported input, they gather one diagnostic per affected
paragraph, clone the input once, reset only negative gaps in those paragraphs
to zero, and lay out once with that fallback. They do not mutate caller blocks,
drop a run because its gap is too negative, or repeatedly retry the document.
`ExportWarning::SignedSpacingFallback { error }` is readable through existing
`Typesetter::warnings()` and the final `ExportResult.warnings` channel. A caller
requiring exact requested spacing should use `try_*`. This text-preserving gap
fallback does not promise to repair an independently invalid font size, script
geometry or other legacy unusable style.

The warning variant and checked methods are additions to the Rust API only;
they are not new Python fitz-compatible symbols. The warning enum remains
non-exhaustive.

## Evidence and limitations

Focused tests cover constructor compatibility, moderate condensation, all four
typed failures, provider call ordering, multiple/nested paragraph warnings,
caller-input preservation, the large-M/small-i advance-cell extent regression,
combining/CJK source preservation, soft wrapping, scripts/autofit, explicit
controls, and omitted final gaps. The extent test failed before its fix;
it does not test outline overhang. A new authored -1pt uniform Serif fixture
improves local LibreOffice comparison **0.8903 → 0.9868** at 100dpi. The baseline
is the actual prior default-spacing fixture for the same content, representing
an omitted unsupported feature; this is not a DOCX importer test. The new
fixture/ref and license are committed together. All nine prior generated PDFs
remain byte-identical to the base revision.

Small external LibreOffice probes show 12pt saturation near -2pt and 24pt near
-4pt for two Latin faces. We do not implement or generalize that policy.
Same-style run splits agree there, but mixed-style final-gap handling differs
from our explicit cluster rule. CJK requests in that local LO setup produced
missing glyphs and were excluded; engine CJK tests use bundled faces and are
not evidence of CJK LO or Word parity. No Word compatibility, production
consumer migration, or general backward-advance layout is claimed.
