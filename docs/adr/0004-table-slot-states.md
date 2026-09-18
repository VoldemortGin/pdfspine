# ADR 0004: Explicit table slot states

- Status: Accepted
- Date: 2026-09-17
- Applies to: native, TATR and ONNX `Table` results

## Context

The compatibility text grid returns `None` for empty origins, unavailable text,
structural gaps and merge continuations. The geometry grid also uses `None`
for gaps and continuations. Treating either value as evidence of a merge can
erase ordinary cells, particularly when a blank cell borders a merged cell.

## Decision

Keep `Table.extract()`, `cells`, `spans` and legacy text-source labels unchanged.
Add frozen public `TableCell` and `TableSlot` values exposed by cached
`Table.origin_cells` and `Table.slots` tuples. Coordinates are zero-based;
origin cells retain complete bounding boxes and positive row/column spans.

Each backend supplies a private `cell_records` transport containing only
accepted structural origins `(row, col, row_span, col_span, bbox, text)`.
The new transport has an explicit text contract: a non-whitespace string is
present, `""` is an extracted blank, and `None` means text unavailable. Native
records use the page word list; vision records use actual word availability
and accepted cell text, not the historical `text_source` label. A missing
cell-text result remains unavailable. Whitespace-only results normalize to
`""` only in the new typed contract. Nonblank text is preserved verbatim.

The shared Python adapter validates span bounds and rejects overlapping
origins with `ValueError`. It builds continuation membership exclusively from
those spans, never from missing text or compatibility-grid `None` values.
Every continuation and its origin reference the same `TableCell` object.
Uncovered grid positions are unavailable with no cell or origin. The snapshot
is built in linear grid/coverage time and does not rerun detection or OCR.

`blank` is an extraction fact, not a visual emptiness guarantee. Without usable
words on the page, empty origins are conservatively unavailable. On a page
with usable text elsewhere, an empty result can still reflect text absent
from the text layer; this API does not attempt per-cell visual classification.

## Alternatives and consequences

Changing the old `None` values would break compatibility and still leave callers
to reconstruct ownership. Duplicating state/coverage logic independently in
Rust, TATR and ONNX would risk backend differences. Backend origin transports
retain structural facts while one typed adapter establishes the Python
contract. The existing Rust/PyO3 project profile, Python floor, package layout,
and abi3 baseline remain unchanged.

Public stubs, reference/README documentation and bundled LLM documentation
describe the same contract. Revisit the text contract if a backend gains
reliable per-cell extraction-failure or visual-blank evidence; do not infer
that evidence from empty text alone.
