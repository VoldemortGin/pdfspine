# ADR 0002: Table-structure backends and model selection

- Status: Proposed (becomes Accepted once the FinTabNet.c benchmark in
  "Decision gate" below has been run and recorded)
- Date: 2026-09-07
- Applies to: `Page.find_tables(strategy="vision", ...)`, the
  `python/pdfspine/_tatr.py` backend, the `[project.optional-dependencies]`
  table in `pyproject.toml`, and the `conformance/gt/tables_diff.py` harness
- Background: [docs/table-structure-models-survey.md](../table-structure-models-survey.md)

## Context

Two requests were raised for the table-structure stage of the vision path:

1. **Benchmark before committing.** pdfspine's default structure model is
   Microsoft TATR `structure-recognition-v1.1-all`, a general-purpose model
   trained on PubTables-1M + FinTabNet.c. Docling's TableFormer is trained on
   PubTabNet, FinTabNet, TableBank and SynthTabNet and reports 96.8 % TEDS on
   FinTabNet. Financial reports are a primary pdfspine workload, so the
   structure stage should be measured against TableFormer (and against TATR's
   own `v1.1-fin` variant) before the default is fixed. TableFormer is
   **not** a TATR fine-tune: it is IBM's own publicly released
   image-to-sequence architecture with a different input / output shape
   (see [survey §2.0](../table-structure-models-survey.md#20-tableformer-is-not-a-tatr-fine-tune)),
   which is why adding it means a second backend rather than a second
   checkpoint.
2. **Selectable industry models.** Users want to install pdfspine with one of
   several TATR checkpoints tuned for different industries, chosen at install
   time.

The current code cannot serve either request cleanly:

- There is exactly one vision backend. `python/pdfspine/document.py:2330`
  rejects any `backend` other than `None` / `"tatr"`, and `_tatr.py` has no
  Protocol or ABC that a second backend could implement; the only injection
  point is a test-oriented `_runtime=` argument.
- Checkpoints are pinned constants (`_tatr.py:28-31`). They can be overridden
  only through `TatrOptions.detection_model` / `structure_model` or the
  `PDFSPINE_TATR_*_MODEL` environment variables, with raw repo ids or paths.
- Weights are never shipped with the package. `[tatr]` installs the torch +
  transformers runtime; the user populates a Hugging Face cache or
  `PDFSPINE_TATR_MODELS` by hand.
- The evaluation harness scores end-to-end extraction only. Without a
  gold-crop TSR-only mode, structure-stage numbers cannot be compared with
  Microsoft's published ~0.98 GriTS, and the vision score on the recovered
  150-page FinTabNet.c slice has not been rerun since 2026-08-03.

## Decision

### Backend seam

- Introduce a Python-side Protocol (working name `TableStructureBackend`) that
  separates the two stages so each can be replaced independently:
  **detection** (page image → table bboxes) and **structure** (crop + tokens →
  rows / columns / headers / spanning cells).
- Extend the existing string entry `find_tables(backend=...)` to accept
  `"tatr"` (default, unchanged) and `"tableformer"`. Unknown values keep
  raising `PdfUnsupportedError`.
- The TableFormer backend replaces **only the structure stage**. Detection
  reuses what pdfspine already has (TATR detection or native vector-line
  guidance), and the page's native word coordinates (`_native_words`) are
  passed as the `iocr_page` tokens of
  `docling_ibm_models` `TFPredictor.multi_table_predict(...)`. This is the
  `do_cell_matching=True` semantics of Docling and matches pdfspine's rule
  that models never regenerate text.
- Evidence: TableFormer's predictor requires an external detector and text
  tokens by design (survey §2.1); pdfspine's two-stage `detect()` /
  `recognize()` split (`_tatr.py:412` / `:417`) already has the seam in the
  right place, it is simply not exposed as an interface.
- Affected files: `python/pdfspine/_tatr.py` (split into a shared base and a
  TATR implementation), a new `python/pdfspine/_tableformer.py`,
  `python/pdfspine/document.py` (backend dispatch), public stubs, and
  `docs/reference/tables.md`.
- Revisit trigger: a third backend whose stages do not map onto
  detection / structure (for example an end-to-end image-to-sequence model
  such as SLANet or UniTable); the Protocol then gains an end-to-end variant
  rather than being bypassed.

### Model selection: named registry with pinned revisions

- Model choice is expressed through a **registry of aliases**, each mapped to
  a repo id plus a pinned revision, a licence label and an approximate size:

  | Alias | Backend | Source (pinned) |
  |---|---|---|
  | `tatr/v1.1-all` (default) | tatr | `microsoft/table-transformer-structure-recognition-v1.1-all` @ `7587a7ef…` |
  | `tatr/v1.1-fin` | tatr | `microsoft/table-transformer-structure-recognition-v1.1-fin` |
  | `tatr/v1.1-pub` | tatr | `microsoft/table-transformer-structure-recognition-v1.1-pub` |
  | `tableformer/accurate` | tableformer | `docling-project/docling-models`, `model_artifacts/tableformer/accurate` |
  | `tableformer/fast` | tableformer | `docling-project/docling-models`, `model_artifacts/tableformer/fast` |

  Revisions for the entries not yet pinned are fixed when the backend lands.
- Selection surfaces: `vision_options={"structure_model": "<alias>"}`, the
  existing `PDFSPINE_TATR_STRUCTURE_MODEL` environment variable (generalised
  to accept aliases), and a future `pdfspine models download <alias>` CLI that
  populates the local cache. The detection stage gets the parallel
  `detection_model` key; `tatr/detection` stays the only built-in alias until
  a better detector is measured.
- An alias value that is not in the registry is treated as a raw Hugging Face
  repo id or local directory, exactly as today. This is the intended path for
  users who fine-tune their own industry model.
- Evidence and reasoning:
  - pip extras can only select Python dependencies. They cannot carry weights
    of several hundred MB with their own licences (CDLA-Permissive-2.0 /
    Apache-2.0 for TableFormer; MIT-labelled but not README-stated for TATR,
    survey §3.1), so "choose the model at `pip install` time" is not something
    an extra can deliver on its own.
  - The three TATR v1.1 variants share one architecture and one runtime;
    splitting them into separate extras would create three names for the same
    dependency set.
  - Industry fine-tunes in the community are rare and poorly documented
    (survey §3.2: one IFRS detection model, one structure model with an empty
    card, nothing for Chinese tables). Real industry adaptation will come from
    users' own fine-tunes, so the mechanism must accept arbitrary sources.
- Affected files: `python/pdfspine/_tatr.py` (`_model_sources`), a new
  registry module, `python/pdfspine/cli.py`, `docs/reference/tables.md`,
  `docs/guide/installation.md`.
- Revisit trigger: a checkpoint in the registry changes licence or is removed
  upstream; or the number of aliases grows to the point where a data file
  (TOML) beats a Python dict.

### pip extras by runtime family

- Extras are split by **runtime family**, not by model:
  - `tatr` — the existing torch + transformers + Pillow set (unchanged).
  - `tableformer` — `docling-ibm-models` and the torch version it requires
    (to be pinned when the backend lands; the required torch range is
    unverified in the survey).
  - `tables-all` — union of the two.
  - `all` — includes `tables-all`.
- Weight distribution is **out of scope for this ADR**. If "weights installed
  by `pip`" is later required, the precedent is the `ocrspine-models` data
  package used by the OCR engine: one wheel per model family
  (`pdfspine-models-tatr-fin` or similar), declared as an optional dependency.
  That is recorded here as a follow-up option, not a decision.
- Affected files: `pyproject.toml`, `docs/guide/installation.md`, CI wheel
  matrix.
- Revisit trigger: `docling-ibm-models` and `transformers` stop agreeing on a
  torch range, in which case the two families need separate environments and
  the `tables-all` extra is dropped.

### Decision gate: FinTabNet.c benchmark

The default structure model changes only on measured evidence. The benchmark
that gates this ADR is:

1. **Extend `conformance/gt/tables_diff.py --gold` with a gold-crop TSR-only
   mode**: crop each of the 186 gold tables with its gold bbox, run the
   structure stage alone, score GriTS. This is the only way to compare with
   Microsoft's published ~0.98 GriTS on the same terms. Then run the existing
   end-to-end mode.
2. **Candidates**: `tatr/v1.1-all`, `tatr/v1.1-fin`, `tatr/v1.1-pub`,
   `tableformer/accurate`, `tableformer/fast`.
3. **Metrics**: GriTS_Top and GriTS_Con from `conformance/gt/grits.py`
   (primary); optionally TEDS via `table-recognition-metric` so the numbers can
   be set beside the Docling paper. For every candidate also record per-table
   inference latency and peak memory on CPU, weight size on disk, and licence.
4. **Threshold**: a candidate becomes the recommended (or default)
   configuration for financial-report workloads only if its **structure-stage
   GriTS_Con exceeds `tatr/v1.1-all` by at least +0.02** on the 186-table
   slice. The threshold is provisional and may be adjusted in
   `docs/PRD-NEXT.md` before the run; the adjustment must be recorded there.
   If no candidate clears it, `tatr/v1.1-all` stays the default and the others
   remain opt-in aliases.
5. Results go into `docs/BENCHMARKS.md` and a `GT-REPORT-tables-gold-*.md`
   per candidate; this ADR's status then changes to Accepted with the chosen
   default written into the registry table above.

## Consequences

Positive:

- The default model is chosen on data from the workload pdfspine cares about,
  and the comparison with published numbers becomes apples-to-apples.
- Users can switch structure models per call or per environment, including
  their own fine-tunes, without code changes.
- A second backend forces the seam to exist, so a third one (or a Rust / ONNX
  runtime later) plugs into a defined interface.
- The base wheel stays untouched; every new dependency is opt-in.

Negative:

- One more optional dependency family to test in CI (the `tatr` extra already
  has a platform matrix; `tableformer` doubles it unless the two share a torch
  range).
- A licence and revision table that must be kept current for every alias,
  and surfaced in `THIRD-PARTY-NOTICES.md` when weights are ever shipped.
- `tables_diff.py` grows a second scoring mode and per-candidate report
  variants; the JSONL worker must load different runtimes.
- Until the benchmark runs, the ADR is only a direction; the code should not
  land a TableFormer default ahead of the numbers.

## Alternatives considered

- **One pip extra per model** (`tatr-fin`, `tatr-pub`, `tableformer`, ...).
  Rejected: extras cannot carry weights, so the extra would install the same
  runtime under a different name and the user would still download the model
  separately; the registry solves the actual problem (which checkpoint, which
  revision, which licence).
- **Use the whole Docling pipeline as a backend.** Rejected: it pulls the full
  Docling dependency set, and its layout detection and OCR duplicate what
  pdfspine already owns (native words, TATR detection, built-in OCR).
  `docling-ibm-models` alone gives the structure model without the pipeline.
- **Add TableFormer directly, without an abstraction layer.** Rejected: the
  `backend not in {None, "tatr"}` check would become a two-branch `if`, and
  the third backend would require the refactor anyway, at a time when two
  implementations already depend on the ad-hoc shape.
- **Keep TATR only and tune it (`v1.1-fin` as default).** Not rejected, but
  deferred to the benchmark: if `v1.1-fin` clears the threshold and
  TableFormer does not, this becomes the outcome with the least new code.

## References

- Survey: [docs/table-structure-models-survey.md](../table-structure-models-survey.md)
- Work queue: `docs/PRD-NEXT.md`, Phase 3 item **P3-6** (sub-tasks and status)
- Current vision backend user docs: `docs/reference/tables.md`
- Existing harness and metric: `conformance/gt/tables_diff.py`,
  `conformance/gt/grits.py`
