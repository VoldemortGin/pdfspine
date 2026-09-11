# Explicit paragraph connections (unreleased Rust API)

`pdf-typeset` accepts caller-resolved paragraph boundaries through a read-only
`ParagraphConnections` overlay. It does not interpret OOXML `w:between`, decide
presence/precedence, or change existing consumers. `ParaProps`, the four existing
`try_*` return types (`SignedSpacingError`), and legacy warning/fallback behavior
are unchanged.

```rust
use pdf_typeset::{BlockPathStep as Step, BorderEdge, ParagraphConnection,
                  ParagraphConnections, Rgb};
let connections = ParagraphConnections::new(vec![ParagraphConnection::new(
    vec![Step::Block(0)],
    vec![Step::Block(1)],
    BorderEdge { width: 3.0, color: Rgb::new(0.0, 0.2, 0.8) },
)?]);
// typesetter.try_layout_flow_with_connections(&blocks, &mut pages, &connections)?;
```

The additive methods are `try_layout_flow_with_connections`,
`try_layout_text_box_with_connections`, `try_measure_blocks_with_connections`,
and `try_measure_text_box_with_connections`. Their last argument is the overlay;
they return the old success type inside `Result<_, LayoutError>`. The
non-exhaustive `LayoutError` wraps either the existing `SignedSpacingError` or a
`ConnectionError` with a structural path and non-exhaustive `ConnectionReason`.
An empty overlay delegates immediately to its corresponding old checked method.

## Paths and accepted geometry

`BlockPathStep` is an alias of the existing `SpacingPathStep`; `BlockPath` is a
vector of these steps. A root paragraph is `[Block(i)]`; a paragraph in a table
cell is `[Block(table), Row(r), Cell(c), Block(i)]`, repeated for nested tables.
Every call revalidates paths against its current block tree. There is no cached
preparation token or semantic document identity: callers must update indices
when editing the model. Internal autofit copies retain structure and paths.

Both endpoints must be layout-usable, non-whitespace paragraphs in the same
sibling list. They must be adjacent, or separated by exactly one explicit
`Block::PageBreak`. Images, tables, multiple breaks, and cell boundaries are not
transparent. Repeated incoming targets, malformed/out-of-range paths, and cells
outside their table's declared grid are rejected. Effective horizontal paragraph
boundaries must agree; positive first-line indentation does not move the border,
whereas negative hanging indentation does. Page-dependent clamping is checked
against actual geometry.

The separator has positive finite width and finite RGB components in `[0,1]`.
Its actual PDF-number representation must also remain positive and finite in
f32, preventing accidental zero-width hairlines or raster overflow. It is solid
and has zero additional clearance. Its width is not scaled with text autofit.
No dashed separator, nonzero separator space, double/art border, or automatic
Word/LibreOffice policy is inferred. Each paragraph's ordinary side edges can
retain their existing solid/dashed style, color, width and space.

## One transition shared by measurement and layout

At the explicit A→B boundary, A's final bottom is suppressed in measurement,
page-fit reservation, painting **and** final cursor advance. B's first opening
is the separator, whose width is reserved exactly once. On the same page, the
ordinary positive `A.space_after + B.space_before` gap remains. A's side edges
stop at A's content bottom; B's start at its separator's outer top. They do not
use the ordinary same-style group's side bridge through this gap. Unspecified
boundaries retain ordinary grouping.

On a page break between A and B, A still has no final bottom and B opens with the
separator; ordinary page-top gaps collapse. Internal page fragments within A or
B use ordinary top/bottom edges, consuming the explicit transition only at the
paragraph boundary. Chains may give B both incoming and outgoing roles. The
shared line metrics and closing-reserve plan drive flow, natural measurement,
cell height/vertical anchoring, and textbox autofit. Textbox measurement remains
natural, before autofit, rotation and anchoring.

A connected endpoint moved to a new page **before its first line** is rewrapped
and repositioned once at that geometry. After any text of that endpoint has been
emitted, changing effective page width returns `ChangingParagraphGeometry`.
Equal-width origin changes are supported. This is not arbitrary variable-width
paragraph reflow; unconnected paragraphs keep their old behavior. Existing
unbounded cell measurement and textbox autofit/clip/overflow policies are not
replaced by a new fixed-height container contract.

Static structure/text checks and existing signed-spacing preparation occur
before `PageProvider` is consulted. Page-dependent fit checks can occur later:
for example a 40pt separator + 20pt first line + 1pt closing edge cannot fit a
60pt flow page, while 39 + 20 + 1 can. Failure returns no partial operations, but
does not roll back provider calls, font-cache changes or accumulated warnings.
There is no connection-dropping legacy fallback. Ordinary malformed-style
policies are not broadened into a promise to preserve arbitrary invalid input.

## Validation

Implementation `267961b`, based on `f6d4799`, passed the final whole-repository
five-stage gate: **2,046 Rust / 1,535 Python passed, 68 existing skips**, including
both installed artifacts. The named source/fingerprint/binary/log checkpoint is
in [validation baselines](validation-baselines.md). The smaller counts below
are focused subsets of that validation, not additional tests to add to its total.

- Real same-page red: old layout measured 38pt where the explicit boundary
  requires 39pt (`1 + 14 + 6 + 3 + 14 + 1`). The candidate also checks one separator
  and the two independent side spans.
- Real changing-page red: B retained one wide-page line after moving from a
  300pt page to a 100pt page. It now has the expected five lines at the new left
  margin. Separate cases cover equal-width origin moves and typed rejection of
  an internal width change.
- Twelve focused tests cover validation, single explicit page breaks, full-page
  A without bottom reserve/paint/advance, the 60/61pt opening limit, paragraph
  continuation, table paths/vAlign, clone/autofit, chains, and empty-overlay API
  compatibility. Four explicit old `Result<_, SignedSpacingError>` annotations
  also compile and run against the candidate rlib.
- The crate's 229 tests, all-target/all-feature clippy and rustdoc with warnings
  denied pass. Python documentation coverage is 318/318 (not a new Python API).
- Eleven existing deterministic PDFs are byte-identical to frozen `f6d4799`;
  all twelve prior SSIM reference buffers are preserved. The new generated
  `typeset-lo-connection.pdf`, reference and fixture-license entry are added
  together. The existing and new references pass 13/13 pages at explicit 100dpi
  and SSIM >=0.97 (minimum 0.9991). An initial 150dpi invocation mismatched the
  old reference dimensions; it was corrected without modifying old references.
  Its committed text is `A paragraph. B paragraph.`
- The self-authored incoming 3pt blue-separator sample compares at **0.9992** SSIM
  with LibreOffice 26.8.0.3 at 100dpi using the same pdfspine renderer on both PDFs.
  Identical authored content using the old empty-overlay path scores **0.9607**.
  This is a missing-connection control, not an older binary or a general Word
  compatibility score. The full-page metric is supplemented by explicit edge
  and baseline assertions; no score-driven tuning was performed.

Reproduce the engine PDFs with `cargo run -p pdf-typeset --example
make_typeset_fixtures`; run `cargo test -p pdf-typeset --test
paragraph_connections`. The local-only `conformance/gt/typeset_lo_oracle.py`
builds the matching DOCX with `build_connection_sample` and includes the new pair.
It is advisory and is not a CI dependency. External diagnostic inputs, the
14-case policy investigation and raw red/green logs live in the maintained
machine's `typeset-between-readonly` evidence directory; the public engine
contract is intentionally narrower than that exploratory LO matrix.
