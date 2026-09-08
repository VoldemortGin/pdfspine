# Table-structure models: pdfspine status and external survey

- Date: 2026-09-07
- Code baseline: `main` @ `9145f9f` (line numbers below refer to that commit)
- Method: read-only code survey of this repository plus web verification of
  GitHub, Hugging Face, PyPI and arXiv pages. Context7 MCP was **not** available
  in the session, so no vendor documentation was fetched through it; every
  external claim is sourced in §5 with the access date.
- **"Unverified"** marks a statement that comes from a second-hand summary, a
  page that did not state the fact explicitly, or a model card with missing
  fields. Treat those as leads to confirm before they drive a decision, not as
  facts.
- Companion decision record: [ADR 0002 — table-structure backends](adr/0002-table-structure-backends.md).
  Work queue: `docs/PRD-NEXT.md` (Phase 3, P3-6).

This note exists so that a reader (human or LLM) who has not seen the
conversation can answer four questions: what pdfspine does today for table
structure, which external models are candidates, why they are candidates, and
what has to be measured before anything changes.

### Questions this document answers

- Is Docling's TableFormer a TATR fine-tune? Is it publicly released? →
  [§2.0](#20-tableformer-is-not-a-tatr-fine-tune)
- Can TableFormer run without the Docling pipeline, and what inputs does it
  need? → [§2.1](#21-standalone-use)
- Which TATR checkpoints exist and what was each trained on? → [§3.1](#31-official-microsoft-checkpoints-microsofttable-transformer)
- How do we benchmark the structure stage alone (gold-crop TSR-only) and with
  which metrics / data? → [§1.4](#14-tests-and-evaluation-infrastructure),
  [§4](#4-evaluation-resources)
- Why not one pip extra per model? → [ADR 0002, "Model selection"](adr/0002-table-structure-backends.md#model-selection-named-registry-with-pinned-revisions)

---

## 1. pdfspine today

### 1.1 Three table paths

| Path | Public entry | Implementation |
|---|---|---|
| A. Native geometry (`lines`, `lines_strict`, `text`) | `Page.find_tables(strategy=...)` | Rust `crates/pdf-text/src/tables.rs` (`find_tables` @347, `Strategy` @48, `Table` @131, `CellSpan` @116); façade `crates/pdf-api/src/tables.rs:178`; PyO3 in `crates/py-bindings/src/lib.rs` |
| B. TATR vision (`strategy="vision"`, `backend="tatr"`) | same entry, dispatched to Python | `python/pdfspine/_tatr.py:1245 find_tables()` + `python/pdfspine/_tatr_postprocess.py` (port of Microsoft's MIT post-processing) |
| C. Tables inside raster images (OCR) | `Page.find_image_tables(engine=, dpi=, language=, ...)` | `python/pdfspine/document.py:2363`; Rust `crates/py-bindings/src/lib.rs:2484` → `pdf_api::page_find_image_tables` (row/column gap clustering + PaddleOCR) |

Public signature (`python/pdfspine/document.py:2299`):

```python
find_tables(*, strategy="lines", backend=None, vision_options=None,
            line_max_thickness=3.0, snap_tolerance=3.0, min_line_length=3.0,
            clip=None, **_ignored) -> TableFinder
```

The Python `Table` class (`document.py:1484`) wraps both the Rust `_core.Table`
and the Python TATR record, so callers see one shape regardless of path.

### 1.2 TATR backend

- **Checkpoints are hard-coded and pinned** in `_tatr.py:28-31`:
  detection `microsoft/table-transformer-detection` (revision `34669b5e…`),
  structure `microsoft/table-transformer-structure-recognition-v1.1-all`
  (revision `7587a7ef…`). They can be overridden per call through
  `TatrOptions.detection_model` / `TatrOptions.structure_model`, or globally
  through the environment variables `PDFSPINE_TATR_DETECTION_MODEL` /
  `PDFSPINE_TATR_STRUCTURE_MODEL` (`_model_sources` @166). There is no alias
  table; the override value is a raw Hugging Face repo id or local path.
- **Runtime is torch + transformers**, not ONNX (`_TransformersRuntime` @243,
  `AutoModelForObjectDetection`). Pre-processing is done in-house: ImageNet
  normalisation and max-resize to 800 (detection) / 1000 (structure)
  (`_tensor` @367). Device selection is `auto` / `cpu` / `cuda` / `mps`
  (`_resolve_device` @339). `_load_model` @315 carries a `dilation: null` →
  `False` compatibility patch because transformers ≥ 5.14 parses the config
  strictly.
- **Two stages**: `detect()` @412 → `recognize()` @417, with `_make_crop` @680
  applying Microsoft's 10 px crop padding between them. Two pdfspine-specific
  enhancements sit around the crop: `native_line_guidance` (`_native_line_anchors`
  @797 and following; a matching native vector-line outline enlarges the
  detector bbox) and `adaptive_crop` (@924; up to two model-driven context
  expansions when structure objects touch a crop edge). Both record provenance
  in `Table.metadata` (`detection_bbox`, `recognition_crop_bbox`,
  `geometry_source`).
- **Post-processing** (`_tatr_postprocess.py`): `objects_to_table_structures`
  @103, `align_headers` @580, `align_supercells` @630, `nms_supercells` @751,
  `header_supercell_tree` @781, `table_structure_to_cells` @812; the glue that
  turns a structure into a pdfspine `Table` is `_table_from_structure`
  (`_tatr.py:1147`).
- **Cell text comes from pdfspine's native word coordinates** (`_native_words`
  @497). The model never regenerates text; built-in OCR is used only when the
  whole page has no text layer.
- **No backend abstraction.** `document.py:2330` only checks
  `backend not in {None, "tatr"}` and raises `PdfUnsupportedError` otherwise.
  `_tatr.find_tables` has a `_runtime=` injection point for tests, but there is
  no Protocol / ABC that a second backend could implement.
- **Model cache**: `_cached_runtime` @443 keyed by `_RuntimeSpec`, in-process,
  lock-protected; `clear_model_cache` @468.

### 1.3 Extras and weight distribution (`pyproject.toml`)

`[project.optional-dependencies]` (from line 50): `test`, `quality`, `docs`,
`tatr` (line 61: `Pillow>=11,<13` + `torch>=2.6,<3` + `transformers>=5,<6`, each
with the same platform / Python-version markers), `ocr = []` (empty, kept for
backward compatibility), and `all` (line 92, currently identical to `tatr`).
The only base dependency that carries model data is `ocrspine-models>=0.0.1,<0.1`
(the shared PP-OCRv5 ONNX package), so a bare `pip install pdfspine` is
OCR-capable but has no TATR weights.

TATR weights are **neither packaged nor auto-downloaded**: loading defaults to
`local_files_only=True`. Users either point `PDFSPINE_TATR_MODELS=<dir>` at a
directory containing `detection/` and `structure-recognition-v1.1-all/`, or
pass `vision_options={"local_files_only": False}` to let Hugging Face populate
the cache. The two weights total roughly 230 MB. This is documented in
`pyproject.toml:73-76`, `docs/guide/installation.md:34-63` and
`docs/reference/tables.md:6-62`.

### 1.4 Tests and evaluation infrastructure

- Python tests: `python/tests/test_tatr_tables.py`, `test_tatr_units.py`,
  `test_tatr_postprocess.py`, `test_tatr_harness.py`, `test_m7.py`,
  `test_image_table.py`. Rust tests:
  `crates/pdf-text/tests/tables_{lines,text,html,none,regression}.rs`,
  `crates/pdf-api/tests/m7_facade.rs`.
- Main harness: `conformance/gt/tables_diff.py`. Default mode is
  pdfspine-vs-fitz agreement (fitz runs in the `.venv-oracle` subprocess so the
  AGPL package never enters the main environment). `--gold` scores GriTS against
  FinTabNet.c; `--strategy vision` runs through a persistent JSONL worker
  (`_run_jsonl_worker` @319) so both models load once per run. A worker or
  model failure stops the run, writes `status=invalid` with no aggregate score,
  and exits non-zero.
- Metric: **GriTS** only (`conformance/gt/grits.py`, pure-stdlib port of
  Microsoft's `factored_2dmss`, Top + Con variants; the file header records why
  GriTS was chosen over TEDS-Struct). **There is no TEDS implementation** in the
  repository.
- Data: `conformance/gt/corpus-fintabnet/` — FinTabNet.c, fetched by
  `fetch_fintabnet.py` (zip64 HTTP-Range extraction of source PDFs from the HF
  mirror after the original DAX host was decommissioned); **150 pages / 186
  structure-eligible gold tables**. A home-grown `corpus-tables/` (t1..t8,
  scored by `score_tables_gt.py` cell-F1) also exists. There is no PubTables-1M
  subset and no Docling or camelot comparison script.
- Reports: `GT-REPORT-tables-gold.md`, `GT-REPORT-tables-gold-text.md`,
  `GT-REPORT-tables.md`, `docs/BENCHMARKS.md` (tables section around line 116).

### 1.5 Recorded scores and open items (`docs/PRD-NEXT.md`, P3-5)

- `lines`: GriTS_Top **0.073** / GriTS_Con **0.070** (39/150 pages with any
  detection) — parity with fitz, whose default also finds ~0 of these
  borderless financial tables. `text`: GriTS_Top **0.185** / GriTS_Con
  **0.107** (148/150 pages). Both are recall-weighted **end-to-end** numbers:
  full-page detection first, a missed gold table scores 0.
- TATR vision vertical slice landed 2026-08-03; two items remain open:
  1. **No gold-crop TSR-only mode.** Microsoft's published ~0.98 GriTS for
     TATR is measured on gold cropped-table inputs, so the current end-to-end
     numbers cannot be compared with it apples-to-apples.
  2. **Vision score not rerun.** The 150/150 FinTabNet source PDFs were
     recovered during the 2026-09-05 corpus run, but
     `tables_diff.py --gold ... --strategy vision` has not been executed since.
- `docs/reference/tables.md:6-62` is the user-facing description of the vision
  backend; `docs/guide/text-extraction.md:156` tells users that borderless
  tables need `text` or TATR. `docs/adr/` contained only ADR 0001 before this
  survey. The repository had no mention of Docling anywhere.

### 1.6 Sibling repositories

`ocrspine`, `docspine` and `corespine` have zero hits for TATR,
table-transformer, `find_tables`, TableFormer or docling. Table structure
recognition lives only in pdfspine.

---

## 2. Docling TableFormer

### 2.0 TableFormer is not a TATR fine-tune

A question every newcomer asks: *does Docling use its own fine-tuned TATR, and
is it unreleased?* No on both counts.

- **Docling does not use TATR.** TableFormer is IBM's own architecture
  (arXiv 2203.01017, 2022). An encoder reads the table image; a decoder emits
  an HTML-style sequence of structure tokens and, in the same pass, regresses a
  bbox for every cell. It belongs to the **image-to-sequence** family (same
  lineage as EDD and TableMaster). TATR is a **DETR object detector**: rows,
  columns, headers and spanning cells are predicted as boxes, and a geometric
  post-processing step assembles them into a grid.
- **It is publicly released.** Code: `docling-ibm-models`
  (`TFPredictor.multi_table_predict`, §2.1). Weights: Hugging Face
  `docling-project/docling-models`, `fast` and `accurate` variants, licensed
  CDLA-Permissive-2.0 / Apache-2.0 (§2.2). It can be used independently of
  the Docling conversion pipeline.
- **Still unverified:** whether the training recipe of the published HF
  weights matches the paper (the paper's data mix is PubTabNet / FinTabNet /
  TableBank / SynthTabNet).
- **Consequence for pdfspine:** TableFormer is a **second model family**, not
  another TATR checkpoint. Its inputs (page image + text tokens + external
  table bboxes) and outputs (per-cell responses with spans) differ from TATR's
  object lists, so it cannot be dropped into `TatrOptions.structure_model`.
  That is the direct reason ADR 0002 asks for a backend seam before adding it.

### 2.1 Standalone use

TableFormer can be called **without the Docling conversion pipeline**. The
`docling-ibm-models` package exposes `TFPredictor`, whose core method is

```python
predictor.multi_table_predict(
    iocr_page, table_bboxes,
    do_matching=True, correct_overlapping_cells=False, sort_row_col_indexes=True)
```

The caller must supply (1) `iocr_page`: an OpenCV-format page image plus the
OCR / text tokens with their bboxes and the page width/height, and (2)
`table_bboxes`: a list of `[x1, y1, x2, y2]` table regions from an external
detector. The output is one dict per table containing `tf_responses` (per cell:
bbox, row/column spans, header flag, ...). **The model does neither table
detection nor OCR**; both must come from upstream. For pdfspine that upstream is
already there: TATR detection or native-line guidance for bboxes, and native
word coordinates (`_native_words`) for tokens.

Docling's high-level options map onto this: `TableStructureOptions.mode` is
`TableFormerMode.FAST` (faster, lower accuracy) or `ACCURATE` (default; slower,
better on complex tables). `do_cell_matching=True` aligns predicted cells with
the PDF's native text cells; `False` lets the model segment text on its own,
which can go wrong on merged cells. pdfspine's "native words into predicted
cells" design corresponds to `do_cell_matching=True`.

### 2.2 Training data, weights, licence, size

- Training data (unverified, second-hand summary; check arXiv 2203.01017 for
  the exact per-release mix): PubTabNet (509K), **FinTabNet (112K)**, TableBank
  (145K), SynthTabNet (600K synthetic). The presence of FinTabNet is the reason
  TableFormer is a candidate for financial-report tables, where pdfspine's
  default TATR v1.1-all is a general-purpose model.
- Weights: repository moved from `ds4sd/docling-models` to
  `docling-project/docling-models` (the old name redirects). Dual-licensed
  **CDLA-Permissive-2.0 / Apache-2.0**. `model_artifacts/tableformer/` holds
  `fast/` and `accurate/`; `tableformer_accurate.safetensors` is about
  **213 MB** (updated 2025-03-04); the whole `tableformer` directory is about
  **358 MB**.
- Community ONNX build: `asmud/ds4sd-docling-models-onnx` (INT8 weights, FP32
  activations, ONNX Runtime dynamic quantisation). No official MLX variant was
  found; Granite-Docling has MLX support but is a different VLM, not
  TableFormer.
- The torch version required by `docling-ibm-models` was not stated on the
  pages consulted (unverified).

### 2.3 Published TEDS numbers

From the TableFormer paper (arXiv 2203.01017, IBM, 2022):

| Dataset | TableFormer TEDS | Prior SOTA cited in the paper |
|---|---:|---|
| PubTabNet | 96.75 % (simple 98.5 % / complex 95.0 %) | EDD 89.9 %, GTE 93.01 % |
| FinTabNet | 96.8 % | EDD 90.6 % |
| TableBank | 89.6 % | EDD 86.0 % |

The paper contains **no direct comparison with TATR** (the TATR paper is
later). `docling-eval` has PubTabNet / FinTabNet benchmark documents, but the
web pages only describe the scripts; the actual numbers live in downloadable
JSON reports (unverified).

---

## 3. TATR model ecosystem

### 3.1 Official Microsoft checkpoints (`microsoft/table-transformer`)

| Checkpoint | Task | Training set | Notes |
|---|---|---|---|
| `table-transformer-detection` | detection | PubTables-1M | ~110 MB, AP50 0.995; pdfspine's pinned detector |
| `table-transformer-structure-recognition` (v1.0) | structure | PubTables-1M | GriTS-Top 0.9849 |
| `…-structure-recognition-v1.1-pub` | structure | PubTables-1M | |
| `…-structure-recognition-v1.1-fin` | structure | FinTabNet.c | financial-report specialist |
| `…-structure-recognition-v1.1-all` | structure | PubTables-1M + FinTabNet.c | pdfspine's pinned default |

The per-variant GriTS numbers for the v1.1 series are not listed in the README
(unverified; the source is Smock et al. 2023, *"Aligning benchmark datasets for
table structure recognition"*). The README does not state a licence
explicitly; the Hugging Face pages are commonly labelled MIT (unverified).
All three v1.1 variants share the same architecture and therefore the same
runtime that pdfspine already ships.

### 3.2 Community fine-tunes

Only two industry-specific fine-tunes were found, both of limited credibility
and with incomplete model cards (details unverified):

- `apkonsta/table-transformer-detection-ifrs` — detection only, fine-tuned on
  2,359 scanned IFRS financial reports; handles normal and rotated tables,
  no-ruling-line scenes.
- `WANGTINGTING/finetuned-table-transformer-structure-recognition-v2` —
  structure; training data and licence are "More Information Needed".

**No trustworthy Chinese-table-specific TATR fine-tune was found.** The
practical consequence is that "industry adaptation" is more likely to come from
a user's own fine-tune than from a downloadable community checkpoint, so any
model-selection mechanism must accept arbitrary repo ids / local paths.

### 3.3 Other open models (one line each)

- **PaddleOCR SLANet** (PP-StructureV2): image-to-sequence, lightweight,
  ~77 % on PubTabNet, Apache-2.0, ONNX / PaddleX deployment available.
- **UniTable** (`poloclub/unitable`): self-supervised pre-training, unified
  multi-task framework, SOTA claims, MIT-style licence.
- **Surya table**: table module of the Surya OCR suite (not examined in depth).
- **TableMaster**: PP-StructureV1-era image-to-sequence model.

---

## 4. Evaluation resources

### 4.1 Metric implementations

| Metric | Implementation | Notes |
|---|---|---|
| GriTS | `conformance/gt/grits.py` (pdfspine, stdlib-only port) | Top + Con; official reference is `src/grits.py` inside `microsoft/table-transformer`; no standalone `grits` PyPI package exists |
| TEDS | `table-recognition-metric` (PyPI, author SWHL, Apache-2.0, Python 3.6–3.14) | needed to compare with the Docling paper numbers |
| TEDS + layout | `docling-eval` (IBM; CLI batch evaluation over PubTabNet / PubTables-1M / FinTabNet / OmniDocBench) | heavy; pulls in the Docling stack |

### 4.2 Datasets

- **PubTables-1M** (`bsmock/pubtables-1m` on HF): ~947K cropped table
  instances for structure, ~575K pages for detection; no native `datasets`
  integration, download via `wget` of tar.gz archives.
- **FinTabNet**: original release CDLA-Permissive; cleaned version
  `bsmock/FinTabNet.c` on HF. pdfspine already holds a 150-page / 186-table
  slice of it.
- **PubTabNet**: ~568K table images from the PubMed Central OA subset.

### 4.3 Recommendation for a small-scale comparison

Use the FinTabNet.c **test split** (thousands of tables, GB-scale download) or
the PubTabNet val split, both downloadable from the HF repositories above. For
pdfspine specifically, the existing 150 / 186 FinTabNet.c slice is the cheapest
first pass, provided the harness gains a **gold-crop TSR-only mode** so that
structure-stage numbers are comparable with Microsoft's published ~0.98.

---

## 5. Sources (all accessed 2026-09-07)

Docling / TableFormer

- https://docling-project.github.io/docling/usage/advanced_options/
- https://github.com/docling-project/docling-ibm-models
- https://huggingface.co/docling-project/docling-models
- https://huggingface.co/ds4sd/docling-models/discussions/7
- https://arxiv.org/abs/2203.01017
- https://github.com/docling-project/docling-eval/blob/main/docs/PubTabNet_benchmarks.md

TATR

- https://github.com/microsoft/table-transformer
- https://huggingface.co/microsoft/table-transformer-structure-recognition-v1.1-all
- https://huggingface.co/apkonsta/table-transformer-detection-ifrs
- https://huggingface.co/WANGTINGTING/finetuned-table-transformer-structure-recognition-v2

Other models

- https://github.com/poloclub/unitable
- https://huggingface.co/PaddlePaddle/SLANet

Evaluation tools and datasets

- https://pypi.org/project/table-recognition-metric/
- https://github.com/docling-project/docling-eval
- https://huggingface.co/datasets/bsmock/FinTabNet.c
- https://huggingface.co/datasets/bsmock/pubtables-1m
