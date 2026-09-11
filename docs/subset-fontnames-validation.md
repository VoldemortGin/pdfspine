# Dynamic subset-name validation

This is unreleased local development. The switch changes structured presentation
of owned text; it does not change canonical layout or rendering. See
[pymupdf-compat-findings.md](pymupdf-compat-findings.md) for the API normalization,
prefix rules and optional source-matrix fields.

## Functional evidence

- The previously installed baseline rejects the setter as deferred. Public
  regressions cover query/reset/truthiness and failed conversion, Page/DL/extended
  TextPages created under both policies, subsequent source edit/close, Form-local
  font names, distinct resources with equal names, synthetic spaces, trace,
  default-layout restoration and concurrent JSON output during toggling.
- PyMuPDF 1.28.2 subprocess probes establish the affected output methods and raw
  name splitting. HTML/XHTML/XML/text/words are unchanged by the oracle setting.
  Native prefix removal accepts more six-character prefixes than this project's
  existing uppercase-only canonical rule; the latter remains unchanged.
- Existing Rust glyph geometry assertions still require the original Tm/CTM
  tuples under the default policy. Three shared-view unit cases check borrowing,
  split geometry, unknown source matrices, equal name values and empty spans.
- 35 existing corpus inputs, verified against their recorded input hashes, each
  produced identical baseline/candidate hashes for 19 outputs: Page and
  annotation-disabled DisplayList TextPage DICT/RAWDICT/JSON/RAWJSON,
  HTML/XHTML/XML/plain text/words, plus Page texttrace. All **665 comparisons**
  match with the policy disabled. This is not a new text ground-truth evaluation
  or a complete historical 43-document render run.

## Creation and memory cost

The same Rust compiler and actual geometry/string types measured the previous
`c1d3ce7` model declarations and current types on this ARM64 machine:

| Rust type | Previous size | Current size |
|---|---:|---:|
| PositionedGlyph | 344 bytes | 352 bytes |
| Char | 184 bytes | 192 bytes |

Each adds one 8-byte optional Arc pointer. Resolved subset names are shared
within their recorded font context; ordinary fonts do not allocate raw-name
storage. Arc/string allocations and reference-count operations still add work
for subset fonts, even when the display switch is False.

A bounded eight-process probe used 35 documents, one warmup and nine creation
samples per document/mode. It ran ABBA, then reverse-document-order BAAB; the
other development workers paused compilation/heavy tests during both windows.
Timing excludes Python startup, document open and result destruction. DisplayList
figures include both recording and textpage construction, with `annots=0` and
text flags 3. The two windows together took about 11.5 seconds.

| Metric | Page TextPage | DisplayList + TextPage |
|---|---:|---:|
| Median document latency, previous → current | 1.5531 → 1.5758 ms | 2.0798 → 2.0896 ms |
| Median per-document current/previous ratio | 1.0100 | 1.00884 |
| Four adjacent-pair median ratios | 1.0103 / 1.0086 / 1.0173 / 1.0220 | 1.0151 / 0.9991 / 1.0097 / 1.0193 |

The ratios and corpus medians are different statistics. This short single-machine
probe indicates small nonzero added cost; it is not a significance claim or a
universal bound on future documents.

A separate retained-object measurement within each process kept 60 TextPages
from PMC176545 (5,987 characters each). `ps` resident-memory growth ranged from
88.42–89.02 MiB before to 90.64–91.12 MiB after. This includes allocator and process
behavior and is **not** an exact heap-allocation audit. The precise structural
fact is the 8-byte increase per glyph/character record above.

## Reproduction evidence

External artifacts on the development machine are in
`/Volumes/ExternalSSD/tmp/pdfspine-deferred-plan/`: `subset_probe.py`,
`subset-oracle.json`, `subset-prefix-oracle.json`, `subset-spaces.json`,
`subset-corpus.py`, both corpus JSON outputs and comparison, `subset-cost.py`,
eight cost JSON runs and summary, and `subset-sizes/` plus its build log.
The baseline Python package/extension copy and its source/module provenance are
in `subset-baseline/`. It was captured at font-integration checkout `090dc85`,
already containing font registration and warp but no subset-name change; the
font worker confirmed SHA-256 `b5de0c540736d01193a126a6dec7e7d4f97d6722407791a81d71a50e7be77201`
as its final gate binary (retained in the subsequent `f3572f1` merge). Font
registration did not change the model declarations used for the size probe.
This is a local development baseline, not a new published release.
Public regressions and shared-view tests are committed
with the implementation; final integration gates are recorded separately.
