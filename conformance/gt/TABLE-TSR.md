# Gold-crop TSR: native words, detector bypass

`tables_diff.py --gold MANIFEST --strategy vision --backend tatr|onnx
--eval-mode gold-crop-tsr --options-json '{"ocr_if_no_text":false}'` runs one
identity-bearing request per annotation table. Default output names include
`gold-crop-tsr-native-words`; explicit report paths remain available. This is
structure recognition with native words, not published gold-word TSR, detector
accuracy, TEDS, or reviewed financial-table acceptance.

The private backend entry points render the complete display list, crop once,
recognize once and return final raw cells before public Table normalization.
They never call neural detection, native line guidance or adaptive crop expansion.
The existing `Page.find_tables`, float crop, thresholding, public Table records
and default page-e2e evaluator behavior remain unchanged. No public API/catalog
symbol was added.

## Coordinates and words

FinTabNet.c's generator first converts original annotation coordinates into
PyMuPDF page space (`adjust_bbox_coordinates`) and derives `pdf_table_bbox` from
those boxes (`complete_table_grid`). See the
[upstream generator](https://github.com/microsoft/table-transformer/blob/main/scripts/process_fintabnet.py).
The evaluator's `fintabnet-page` request accepts unrotated source pages. For
cropped pages it requires the original annotation `pdf_full_page_bbox`, carried
unchanged as identity-bound `source_page_bbox`: finite zero-origin positive
extent, matching visible CropBox dimensions within 0.001 point, with the crop
inside that extent. Only recognized FinTabNet source tracks supply this proof.
The source coordinates already use visible page space, so mapping is identity;
no CropBox origin is added or subtracted. Missing proof on cropped pages,
rotation, malformed or mismatched extents remain explicit errors before model
startup. General explicit `page-display` callers retain their prior contract.
Historical unrotated full-page requests without proof remain supported. ADBE's source
bbox `[52,420.0411,560.9641,676.8805]` was checked against the actual PDF image and
188 contained native words. Its source crop needs no second PDF bottom-left flip.
General private callers explicitly supply `page-display`: zero-origin visible
page points after Rotate. Four generated Rotate cases with a nonzero CropBox
verify known native text and the actual pixel crop.

Only the new path rounds the pixel crop outward with floor/ceil, then clips
explicit padding (default zero, integer 0–20 pixels) to the page image. Word
coordinates and returned cell geometry use that same actual integer origin.
Metadata records requested point/pixel bounds, actual pixel bounds and size,
rotation, cropped native tokens with IDs/coordinates and their canonical SHA-256.
The existing token-overlap threshold is >= 0.5. Source-native token order is
retained, including its rotation-dependent sort order. OCR is disabled; an
explicit OCR=true request is rejected. No annotation text is fed to recognition.
The mode's padding is separate from the unused e2e `crop_padding` option; e2e
crop/guidance options have no execution effect in this mode.

## Failure and model provenance

A normal empty/low-confidence recognition is a successful empty prediction.
Normal background/special tokens, EOS and padding are not invalid cells. Final
noncontiguous, duplicate or overlapping spans retain every raw cell and receive
zero TP, raw cell count FP, all gold cells FN, and GriTS=0. Bad final geometry
also produces explicit invalid-quality diagnostics. Numeric integer scalar
representations, including NumPy integers and integral floats, are normalized
without truncating fractional indices. No set/deduplication repair occurs.

Strict resource limits match the evaluator: 10,000 cells / 1,000,000 grid slots.
Span products and cumulative occupied space are checked before grid allocation;
over-limit predictions preserve their final-cell count and invalid diagnosis.
The existing 186 structure-eligible source tables have at most 224 cells and
261 grid slots. Invalid or over-limit gold is input-incomplete before worker
startup, preserving the requested denominator. These budgets are resource
safeguards, not quality thresholds.

Assembler/inference exceptions propagate as pipeline failures with phase and
chained original errors; the old public TATR wrapper still returns None for its
historically caught assembler exceptions. The evaluator never turns such a
failure into zero quality. Incomplete inputs/review and execution failures have
null official aggregates; separately labelled partial diagnostics remain visible.
The frozen financial v1 input is unreviewed and cannot become accepted gold by
running this mode.

TATR currently loads both pinned checkpoints, but executes only the structure
model. The report distinguishes loaded and executed roles; structure-only TATR
startup is not claimed. ONNX can run with only its table file/session: no layout
file is required or fingerprinted for TSR. Actual already-cached sessions are
reported as loaded, while only table is executed. Page-e2e still requires both
model role fingerprints. Detection boxes/confidences/metrics are never fabricated
from the supplied crop. Structure confidence and token-assignment confidence,
when available, are named separately.

## Validation scope

Model-free regressions cover both detector traps through real PDF render/crop,
four rotations/nonzero CropBox, fractional crop/padding/half-token inclusion,
empty native text/OCR traps, final raw topology, strict/default assembler errors,
ONNX table-only lazy session and loaded-role reporting, EOS/invalid span limits,
worker mode/crop identity in both protocols, and incomplete/invalid denominators.
The pre-change 97.2680% Python coverage report is a historical measured baseline:
new private Python lines have not been remeasured in that combined profile.
Real model readiness is recorded separately; fake inference tests are not model
quality evidence. No model tuning or dataset quality score is claimed here.


### Frozen implementation and readiness evidence

Implementation source `d98bdf3` passes 127 related tests (five existing live-model
skips) and the full Python suite: **1,492 passed / 68 skipped**. Ruff/format and
project mypy pass. No Rust source changed or native rebuild was needed; the
copied main extension SHA-256 remains
`4f2b3fd6fe6dae8eb4f3eae63d2c804e45c14143118bde99ce9041b08028a331`.

Independent offline CPU smoke on the single ADBE development crop completed:
TATR returned 107 final cells and ONNX 187, each with zero detector calls and
one recognizer call. ONNX's layout path deliberately does not exist and its
only actual session is table/CPU. This establishes executable bypass/readiness,
not extraction quality or comparability to published gold-word scores.

External evidence under `/Volumes/ExternalSSD/tmp/`:
`pdfspine-goldcrop-readiness/` contains each backend's raw stdout, trap trace,
source/binary hashes, exact options and model fingerprints;
`pdfspine-deferred-plan/table-tsr-full-python.log` and
`table-tsr-related-final.log` contain test results; `table-tsr-grid-budget.json`
records the 186-table budget inspection. `table-tsr-adbe-source-box.png` and its
JSON document the actual source-coordinate check. These are separate from the
unreviewed financial dataset and from any future benchmark acceptance report.

## Cropped FinTabNet source proof

The first frozen 40-page diagnostics stopped both goldcrop tracks at ADP after
14/60 results. ADP has MediaBox 612×1008, a visible CropBox 612×918, and original
`pdf_full_page_bbox=[0,0,612,918]`. Its raw source table bounds align with the
actual table and 286 native words; adding or subtracting the raw CropBox offset
misaligns the table. The guard now checks this extent proof instead of requiring
CropBox=MediaBox. The other cropped source pages PXD, EXR, PRU and BSX have the
same validated convention; all 40 selected pages are unrotated. This is source
coordinate validation, not human verification of table text or spans. Original
failed reports and all input hashes remain intact. No production backend,
threshold, text selection, source annotation or ledger is changed.
