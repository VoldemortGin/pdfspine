# Financial tables: unreviewed diagnostic baseline — 2026-09-11

**Eight tracks executed all40 pages /60 source tables. These are unreviewed source-annotation diagnostics: every official aggregate is null, comparable=false, and overall status=incomplete.** They are not human-reviewed accuracy, published TSR parity, or an ADR/default-model decision.

The deterministic dataset has10 development pages/15 tables and30 evaluation pages/45 tables, with disjoint issuers. All150 selection candidates were historically exposed; evaluation is not an untouched holdout. See [the dataset contract](financial-eval/README.md) and [the evaluator contract](TABLE-EVAL-V2.md).

## Results (diagnostic only)

| Track | Partition | Pages | Tables | GriTS Top | GriTS Con | Span F1 | Content F1 |
|---|---|---:|---:|---:|---:|---:|---:|
| native-lines-page-e2e | development | 10/10 | 15/15 | 0.1543 | 0.1627 | 0.2220 | 0.0412 |
| native-lines-page-e2e | evaluation | 30/30 | 45/45 | 0.0517 | 0.0547 | 0.1267 | 0.0188 |
| native-text-page-e2e | development | 10/10 | 15/15 | 0.0609 | 0.0574 | 0.1269 | 0.0027 |
| native-text-page-e2e | evaluation | 30/30 | 45/45 | 0.0543 | 0.0456 | 0.1282 | 0.0000 |
| tatr-vision-page-e2e | development | 10/10 | 15/15 | 0.4841 | 0.4615 | 0.5632 | 0.1294 |
| tatr-vision-page-e2e | evaluation | 30/30 | 45/45 | 0.4820 | 0.4309 | 0.4645 | 0.2067 |
| onnx-vision-page-e2e | development | 10/10 | 15/15 | 0.8465 | 0.7628 | 0.6742 | 0.2080 |
| onnx-vision-page-e2e | evaluation | 30/30 | 45/45 | 0.8283 | 0.7438 | 0.7508 | 0.3131 |
| tatr-vision-gold-crop-tsr | development | 10/10 | 15/15 | 0.9450 | 0.9107 | 0.9012 | 0.5769 |
| tatr-vision-gold-crop-tsr | evaluation | 30/30 | 45/45 | 0.9213 | 0.8694 | 0.8734 | 0.5505 |
| onnx-vision-gold-crop-tsr | development | 10/10 | 15/15 | 0.8345 | 0.6919 | 0.7648 | 0.2549 |
| onnx-vision-gold-crop-tsr | evaluation | 30/30 | 45/45 | 0.8808 | 0.7762 | 0.8003 | 0.3369 |

Page-e2e GriTS is recall-weighted over gold tables: missed detections contribute zero, while unmatched extra tables do not directly reduce GriTS. Their raw cells instead count as false positives in span/content F1. Goldcrop supplies table identity/crop and bypasses detection; these are different input tasks. Strict span/content F1 and GriTS use different alignment rules. All eight executions succeeded; the nonzero CLI exit3 means unreviewed/incomplete, not execution failure.

One TATR goldcrop prediction (`V_2009_page_105_0`) contains overlapping cells:135 raw cells versus128 gold cells. It contributes TP0/FP135/FN128 and GriTS0, with raw cells retained. This is a quality failure, not a pipeline exception. No result was silently filtered.

## TableFormer appendix (separate source revision)

Two additional **raw-structure goldcrop** tracks completed the same 40 pages /
60 tables, without altering the earlier six results. These are not the official
Docling matched/compressed output: matching, row/column compression and overlap
correction are all disabled. Native-word assignment follows the documented
shared geometric rule in [TABLE-TSR](TABLE-TSR.md#tableformer-raw-structure-evaluator).

| Track | Partition | Pages | Tables | GriTS Top | GriTS Con | Span F1 | Content F1 |
|---|---|---:|---:|---:|---:|---:|---:|
| tableformer-fast-raw-structure | development | 10/10 | 15/15 | 0.8963 | 0.8707 | 0.8318 | 0.4595 |
| tableformer-fast-raw-structure | evaluation | 30/30 | 45/45 | 0.9302 | 0.8932 | 0.9071 | 0.6355 |
| tableformer-accurate-raw-structure | development | 10/10 | 15/15 | 0.8422 | 0.8223 | 0.8060 | 0.5529 |
| tableformer-accurate-raw-structure | evaluation | 30/30 | 45/45 | 0.9255 | 0.8910 | 0.9083 | 0.6360 |

The new source is `35e5f53568d9e174b00df8fdbc8d47e24ebd986e`. Its native extension
SHA256 is `fe8ba4d7c88df62c88e4defbd2b73d33e7748369e3156351010d91a2b5eaf5d6`,
which includes the later signed typesetting change and differs from the earlier
six-track binary. Independent review verified all 244 frozen file hashes,
203 inherited input artifacts, and **all 60 tables' native-token hashes and
actual crop bounds against earlier TATR goldcrop, for each variant**. This
establishes the observed word/crop agreement, not universal binary equivalence.

Accurate has one invalid prediction: `AMP_2015_page_94_2`, raw cell **index 8**
(zero-based), has top 22.479279 > bottom 20.569140 in crop pixels. The inversion
already exists in the upstream raw normalized prediction; the adapter did not
create it. It was not swapped, discarded or repaired. All 77 raw cells remain
against 63 gold cells: TP0 / FP77 / FN63, GriTS Top/Con zero for this table.
Fast has no invalid predictions. Both executions succeeded; neither pipeline
exceptions nor incomplete requests were converted into quality zeros.

Both variants use the same isolated 38-package hash lock, Docling IBM Models
4.0.2, Torch 2.13.0, torchvision 0.28.0 and OpenCV-headless 4.13.0.92, with CPU,
144 DPI, two threads, OCR=false and socket denial. The four fixed model/config
hashes, runtime source hash, options and lock are recorded in the JSON appendix.
Whole-track observations were 21.637s / 34.919s, not controlled speed comparisons.
No default recommendation follows from these means or the earlier ADBE probes.

To reproduce these two tracks, use the new source revision with the general
command below: BACKEND=tableformer, STRATEGY=vision, MODE=gold-crop-tsr, and the
corresponding JSON options using `${TABLEFORMER_MODEL_ROOT}/{fast,accurate}`.
Install from the indexed frozen requirements.lock in an isolated environment;
verify the exact cached model/config hashes before running. Raw artifacts are
under relocatable `pdfspine-tableformer-diagnostics-35e5f53/`; the JSON appendix
indexes its commands, raw results, frozen source and independent review hashes.
The old six tracks retain their original revisions and archives below. All
human-review, annotation uncertainty and historically-exposed-input limitations
still apply; official aggregates remain null and PRD/ADR decisions stay open.

## Source and evidence boundaries

- Four page-e2e tracks reuse frozen `ec7b199d9ba9704f5077b80e1c0c1ca88e0dfe5f`.
- Two goldcrop tracks were collected at frozen `38cdcba028ba1289ea47f5c47d90e6a7e925261b`. Only the evaluator Python file differs between the runtime snapshots; package/backend/core, models and inputs are byte-identical.
- The original two goldcrop runs stopped explicitly at cropped ADP after14/60. They remain in the first archive and are not overwritten. The [source-extent proof](TABLE-TSR.md#cropped-fintabnet-source-proof) fixes the coordinate contract without changing gold or thresholds.
- All242 frozen files were SHA-checked after collection. The [machine-readable provenance](table-diagnostics-2026-09-11.json) records exact commits, package/core/scorer/model/input hashes, effective options, environments, full-precision partition diagnostics and raw-result hashes.
- Existing pre-phase3 ADBE table payloads compare exactly with reused results: TATR empty list and the entire ONNX table/cell/geometry/text/confidence/internal-metadata payload. Outer runtime/source metadata is separate. This is one-page evidence, not a universal equivalence claim.

The original TATR/ONNX paths use CPU, native words and OCR=false, with fixed verified weights and socket-level Python audit denial during execution. TATR loads both pinned checkpoints but goldcrop executes structure only. ONNX goldcrop loads/executes only its table session. Installed environments differ: TATR torch2.13.0/transformers5.14.1; ONNX Runtime1.29.0; both Python3.12.11/numpy2.5.1/Pillow12.3.0. Elapsed durations in JSON are observations, **not a speed ranking**.

## Gold-review caveats

A separate AI visual scan of ADBE/ADI/AMP (five tables/378 source cells) is not human acceptance or certification of every character. ADBE has seven inherited JSON lost-word boundaries, such as `DigitalMediaSolutions`; AMP has four merged-span questions needing human judgment. ADI empty subtotal labels/dashes were checked. Source annotation/HTML/ledger were not corrected. Content diagnostics therefore include annotation uncertainty. The QA notes hash and external index are included in provenance.

## Reproduce without machine-specific paths

Use two clean checkouts at the exact commits above, and build/install the corresponding pdfspine extension for the local platform. The recorded `.so` SHA identifies this macOS arm64 collection; a different platform build must record its own source/toolchain/binary identity. Restore the selected source assets by the dataset README and verify them against the committed manifest; PDFs/models are not embedded in this report. Do not change review state.

Set `CHECKOUT`, `PY` (an isolated dependency interpreter) and `OUT` for a track. Run its exact configuration from the JSON provenance: `${MODEL_ROOT}`, `${TATR_DETECTION_SNAPSHOT}` and `${TATR_STRUCTURE_SNAPSHOT}` are relocation placeholders, not model revisions. Use `onnxruntime==1.29.0` for ONNX, and the recorded Torch/Transformers versions for TATR. Model bytes must match the JSON hashes. Fixed TATR snapshot revisions are34669b5e93083671f6ccd7aca07d615a79772286 (detection) and7587a7ef111d9dcbf8ac695f1376ab7014340a0c (structure).

```sh
export HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 HF_DATASETS_OFFLINE=1
export OMP_NUM_THREADS=2 MKL_NUM_THREADS=2 PYTHONDONTWRITEBYTECODE=1
export PYTHONPATH="$AUDIT_DIR:$CHECKOUT/python"
"$PY" "$CHECKOUT/conformance/gt/tables_diff.py" \
  --gold "$CHECKOUT/conformance/gt/financial-eval/v1/manifest.json" \
  --pdfspine-python "$PY" --strategy "$STRATEGY" --backend "$BACKEND" \
  --eval-mode "$MODE" --timeout 90 --startup-timeout 240 \
  --options-json "$OPTIONS_JSON" --report "$OUT/diagnostic.md" --json "$OUT/diagnostic.json"
```

Here `STRATEGY` is lines/text for native, vision for models; `MODE` is page-e2e or gold-crop-tsr. Native uses `{}` options. Use the earlier commit for all four page-e2e configurations and the later original commit for the two TATR/ONNX goldcrop configurations. The TableFormer appendix specifies its separate revision. The declared full40/60 manifest is evaluated unchanged; partition summaries are computed afterward by its development/evaluation labels. No subset schema or human-review bypass is used.

Put the following `sitecustomize.py` in the external `AUDIT_DIR` so both controller and spawned workers deny network. Model download/install is a separate setup step before this execution policy.

```python
import sys
def audit(event, args):
    if event in ("socket.connect", "socket.getaddrinfo"):
        raise RuntimeError("offline table diagnostics forbid " + event)
sys.addaudithook(audit)
```

Raw artifacts are externally retained under relocatable archive roots `pdfspine-table-diagnostics-ec7b199/` and `pdfspine-table-diagnostics-38cdcba/`; per-track `diagnostic.json` paths/hashes are indexed in the provenance JSON. On the collection machine these roots are in `/Volumes/ExternalSSD/tmp/`. This location is an evidence index, not a requirement for reproducing the commands. The first archive also retains coordinate overlays and the explicit14/60 failures; the second retains the new complete goldcrop results. No models, PDFs or large prediction payloads are copied into the repository.

PRD§0#9 remains open: human review/corrections, further backend work and any default-model/ADR decision are still pending.
