# Table evaluator v2: configuration, coverage and strict cell alignment

This is an evaluator increment, not a new model, a scored backend comparison,
or acceptance of PRD §0 #9 / ADR 0002. No TEDS score is implemented.

Historical `--gold manifest.json` still means native `strategy=lines` against
original source annotations. `--strategy text` and `--strategy vision` retain
historical native-text and default TATR behavior. Explicit backend selection:

```sh
python conformance/gt/tables_diff.py --gold manifest.json \
  --strategy vision --backend onnx \
  --options-json '{"layout_model":"/models/layout.onnx","table_model":"/models/slanet-plus.onnx","ocr_if_no_text":false}' \
  --report report-onnx.md --json result-onnx.json
```

This command requires real existing model files and optional runtime packages.
The evaluator does not install dependencies or download weights. Native strategy
plus a vision backend, unsupported backend, non-object/nonfinite options, and
TATR `local_files_only=false` are rejected. Backend-specific option dataclasses
validate effective defaults inside the worker. A non-default explicit backend
or options hash gets its own default filename; historical default report paths
are preserved. Backend/options flags outside gold/worker mode are rejected.

Both single-shot and persistent workers use `pdfspine.table-worker.v2`, with
request identity, boolean success, table list, runtime metadata and explicit
errors. Invalid JSON/schema/identity, timeout and process failure invalidate the
run rather than producing an empty successful table list. Model metadata comes
from the backend and options actually used, including zero-detection pages:
Python/runtime versions, effective options, actual provider/device, model and
config byte hashes, source module/extension/evaluator hashes. TATR and ONNX
metadata paths are separate. Model-file hashing is cached by path/size/mtime.
No model result was measured to validate this harness change; tests mock vision
runtimes and use existing native worker fixtures.

## Source data and review status

Manifest paths are relative to the manifest directory (absolute historical paths
also remain supported). Entries retain `document_id`, `pdf`, `pdf_page_index`,
`annotation` (original list-of-table annotation JSON), and optional
`pdf_sha256`, `annotation_sha256`. Missing annotation entries are retained in the
requested denominator. Declared selection/review-ledger file references and
hashes are checked. Partition and historical exposure labels are retained.

Only a schema-less manifest explicitly declaring `dataset: "FinTabNet.c"`
selects the historical `source-annotations` compatibility track. Unknown schemas
or missing dataset identity do not default to legacy acceptance. A new draft dataset must set top-level and/or per-entry
`review_status: "unreviewed"`; it can execute successfully and produce diagnostic
metrics, but overall status remains incomplete with no official aggregates. The
financial-drafts.v1 schema requires PDF/annotation/selection/ledger/HTML/cells
hash and explicit review bindings. Dataset and selection identity, unique ledger
structure IDs, per-table source/artifact hashes and exact annotation-table coverage
are validated. Comparable review requires top/entry/table status reviewed and
matching ledger human_review reviewed with reviewer and reviewed_at; AI/machine
checks never substitute. These are explicit recorded assertions, not human review
performed or authenticated by this program. HTML drafts do not silently replace source annotations.

The original GriTS text selection is preserved: truthy `json_text_content`, then
`pdf_text_content`, then empty string, with strip. The report names this legacy
rule. Newly reviewed HTML or a preserve-explicit-empty source track is a separate
future input contract, not an unannounced change to historical scores.

The separate `execution_status` reports success/partial/failed; overall `status`
and `comparable` additionally require complete inputs and the accepted review
track. Missing files,
annotation decoding or declared hash mismatches produce `incomplete`; execution
or protocol failures produce `invalid`. Unreviewed drafts are also incomplete. Official aggregates are null for all
incomplete/invalid runs. A labelled diagnostic aggregate retains completed
pages without silently replacing the requested denominator. Reports record known
requested/missing/evaluated table counts, pages with unknown table counts, and
requested/evaluated/missing page counts. Acceptance command exits nonzero when
not comparable. Unknown source annotation table counts are not invented.

## `cell_span_f1_v1`

GriTS Top/Con remain separate and unchanged for valid predictions. This new
metric is a strict row/column-set diagnostic, not TEDS or GriTS's internal match.

A span contains nonempty sets of nonnegative integer row/column indices. Order
is immaterial (real AMP annotations include `[8,7]`); sorting is canonicalization
only. Duplicates, disconnected sets and overlapping occupied cells are invalid;
no deduplication, gap filling or cell repair occurs. Source arrays are retained.
Evaluator safety limits are 10,000 cells and 1,000,000 bounding grid slots.

Within each bbox-matched table, exact row-set/column-set equality gives one
one-to-one span match. TP is the number of matches; FP is predicted cell count
minus TP; FN is gold cell count minus TP. F1 is `2TP/(2TP+FP+FN)`; both empty is
1, only one empty is 0. A merged cell and an explicitly empty cell each count
once. Content F1 uses those same pairs but counts a TP only for equal text after
NFC + Unicode whitespace collapse/strip. Signs, decimals, parentheses, grouping
commas and currency are never erased or numerically converted. Headers do not
implicitly affect this metric.

Invalid gold topology is an input error. Invalid predicted topology is a quality
failure: that table has zero GriTS Top/Con and zero cell TP; all raw predicted
cells are FP and all gold cells FN. The table remains in the original GriTS
and bbox-match denominators. `invalid_prediction` and its reason are reported;
raw predictions are retained. Unmatched extra tables contribute all raw cells
as FP, including malformed extra tables. The reported cell score is micro over
all page pairs and unmatched tables; historical GriTS's gold-table average stays
unchanged. A protocol-level malformed table/cell object is an execution error,
not valid model topology.

## Still pending

Gold-crop TSR must directly call recognition and prove detector bypass; current
`clip` only filters detected tables and is NOT such a mode. TATR production
postprocessing can still catch certain arithmetic/schema errors and return None;
a strict phase-3 diagnostic seam is needed to distinguish those from genuinely
empty recognition. This increment does not alter `_tatr.py` or `_onnx.py`.

Native words and genuine gold words are different tracks. Existing source
annotations provide cell text/boxes, not authentic per-word boxes; do not split
cell strings into invented gold words. Native OCR fallback is named in effective
options and actual returned table `text_source`; set `ocr_if_no_text=false` for
native-only model experiments. No TableFormer or alternative TATR alias backend
is added, regardless of which weight files happen to exist locally. Forty-page
drafts remain unreviewed until actual verification; no default-model winner or
published TSR comparability is claimed here.
