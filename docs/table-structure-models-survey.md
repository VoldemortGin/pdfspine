# Table-structure models: pdfspine status and external survey

- Date: 2026-09-07; §4 (layout detection models) and its sources added
  2026-09-08
- Code baseline: `main` @ `9145f9f` (line numbers below refer to that commit;
  §4 refers to branch `fix/onnx-layout-ppdoclayout`)
- Method: read-only code survey of this repository plus web verification of
  GitHub, Hugging Face, PyPI and arXiv pages. Context7 MCP was **not** available
  in the session, so no vendor documentation was fetched through it; every
  external claim is sourced in §6 with the access date.
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
  [§5](#5-evaluation-resources)
- Why was DocLayout-YOLO removed from the ONNX backend, and what is the
  evidence? → [§4.1](#41-doclayout-yolo-agpl-30-lineage-withdrawn-2026-09-08)
- Which PP-DocLayout variant should the ONNX backend use? →
  [§4.2](#42-paddlex-pp-doclayout-family), [§4.4](#44-recommendation)
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

## 4. Layout detection models (ONNX backend, added 2026-09-08)

The ONNX backend (`python/pdfspine/_onnx.py`, `backend="onnx"`) is
end-to-end: a page-layout detector finds `table` regions (and every other
block type), SLANet-plus predicts the cell grid of each region, and the
native word layer fills the cells. This section records the layout stage:
why the first detector was withdrawn, what the replacement family looks like,
and which variant the measurements favour. SLANet-plus itself is unchanged
(§3.3).

### 4.1 DocLayout-YOLO: AGPL-3.0 lineage, withdrawn 2026-09-08

The backend first shipped with DocLayout-YOLO
(`doclayout_yolo_docstructbench_imgsz1024.onnx`, RapidAI RapidLayout
v1.2.0). The weights card on Hugging Face is tagged Apache-2.0; the project's
own lineage is not. Three independent pieces of evidence, all checked
2026-09-08:

| # | Evidence | What it says |
|---|---|---|
| 1 | Upstream repository `LICENSE`: https://raw.githubusercontent.com/opendatalab/DocLayout-YOLO/main/LICENSE | The file is the AGPL-3.0 text ("GNU AFFERO GENERAL PUBLIC LICENSE, Version 3, 19 November 2007"). |
| 2 | PyPI package metadata: https://pypi.org/pypi/doclayout-yolo/json | `license: AGPL-3.0`; classifier `License :: OSI Approved :: GNU Affero General Public License v3 or later (AGPLv3+)` (latest 0.0.4). The repository is a fork of Ultralytics and its `pyproject.toml` carries the same classifier. |
| 3 | Ultralytics' own licence position: https://www.ultralytics.com/license | AGPL-3.0 for open use, otherwise a paid Enterprise licence; the page states this applies to "code, models, architectures, training pipelines, or trained/fine-tuned models". DocLayout-YOLO builds on the Ultralytics / YOLOv10 code base. |

Local corroboration: the RapidAI ONNX export we were loading embeds
`author=Ultralytics, license=AGPL-3.0` in its own model metadata.

Conclusion: the Apache-2.0 tag on the weights card does not override the
upstream project's LICENSE, its package metadata and the artefact's own
stamp. Under the family rule (Apache-2.0 or more permissive through the whole
chain; [ADR 0002, "Licence stance"](adr/0002-table-structure-backends.md#licence-stance-apache-20-only-through-the-whole-chain))
the model was **removed from pdfspine** the same day — code, docs and the
`onnx/doclayout-slanet-plus` registry alias — and replaced by PaddleX
PP-DocLayout (§4.2). The DocLayout-YOLO numbers in
`docs/onnx-backend-baseline-2026-09-08.md` are kept as history only.

### 4.2 PaddleX PP-DocLayout family

All members are PaddleX / PaddleOCR layout detectors published by
PaddlePaddle under Apache-2.0, with ONNX exports published by RapidAI
(Apache-2.0, `onnxruntime`, no torch). Head / backbone, class count and input
size are read from each model's `inference.yml` on Hugging Face (`arch`,
`label_list`, `Preprocess.Resize.target_size`) and from the PaddleX model
descriptions; mAP and storage size are PaddleX's published figures. **The mAP
numbers are not mutually comparable**: L / M / S are scored on one self-built
PaddleX evaluation set (the Hugging Face cards say 500 images of Chinese and
English papers, newspapers, research reports and test papers), plus-L on a
different and broader one (1,000 images that add PPT, magazines and
textbooks), and V3 has no published mAP at all. The PaddleOCR 3.x module page
gives yet another size for the same eval set (1,300 images) — treat the
image counts as approximate.

| Model | Head / backbone | Classes | Input | Published mAP(0.5) | Size on disk | Licence |
|---|---|---:|---|---|---|---|
| PP-DocLayout-S | PicoDet-S (`arch: GFL`) | 23 | 480×480 | 70.9 % (PaddleX self-built layout eval set) | 4.834 MB (PaddleX) | Apache-2.0 |
| PP-DocLayout-M | PicoDet-L (`arch: GFL`) | 23 | 640×640 | 75.2 % (same set) | 22.578 MB (PaddleX) | Apache-2.0 |
| PP-DocLayout-L | RT-DETR-L (`arch: DETR`) | 23 | 640×640 | 90.4 % (same set) | 123.76 MB (PaddleX); 123 MB as `pp_doclayout_l.onnx` | Apache-2.0 |
| PP-DocLayout_plus-L | RT-DETR-L (`arch: DETR`) | 20 | 800×800 | 83.2 % (**different**, broader self-built set) | 126.01 MB (PaddleX) | Apache-2.0 |
| PP-DocLayoutV3 | RT-DETR framework with a mask head and a "Global Pointer" reading-order head in the decoder (`arch: DETR`); backbone name **not published**; 33 M parameters per the paper | 25 | 800×800 | **Not published.** PaddleX / PaddleOCR list only latency (23.77 ms, A100) and size. The RT-DocLayout paper reports end-to-end document-parsing scores with a VLM recogniser (OmniDocBench v1.5 overall 94.50; Real5-OmniDocBench six-dimension average 92.46 %), not a standalone layout mAP | 126 MB (PaddleX); 124 MB as `pp_doc_layoutv3.onnx` | Apache-2.0 |

Notes on the rows:

- Lineage: PP-DocLayout-S / M / L are the three models of the PP-DocLayout
  paper (arXiv 2503.17213, March 2025; 23 region types; L on RT-DETR-L).
  PP-DocLayout_plus-L is the PP-StructureV3 default, retrained on a broader
  corpus with a 20-class vocabulary. PP-DocLayoutV3 is the open-source
  release of RT-DocLayout (arXiv 2606.23344, ECCV 2026); the paper says so
  explicitly. It unifies classification, box regression, pixel masks and
  reading-order prediction in one query-based RT-DETR decoder.
- The intermediate PP-DocLayoutV2 (25 classes, 203.8 MB, 81.4 % mAP(0.5) on
  a 1,000-image / 25-class self-built set per the PaddleX layout-analysis
  page) is the nearest published reference point for V3's vocabulary; V3
  itself is only ever compared end-to-end.
- Class vocabularies: S / M / L share one 23-entry list (`paragraph_title`,
  `image`, `text`, `number`, `abstract`, `content`, `figure_title`,
  `formula`, `table`, `table_title`, `reference`, `doc_title`, `footnote`,
  `header`, `algorithm`, `footer`, `seal`, `chart_title`, `chart`,
  `formula_number`, `header_image`, `footer_image`, `aside_text`). plus-L
  drops `table_title`, `chart_title`, `header_image`, `footer_image` and adds
  `reference_content`. V3 has 25 entries, alphabetical upstream: relative
  to L it adds `vision_footnote`, `vertical_text`, `reference_content`,
  splits `formula` into `display_formula` / `inline_formula`, and drops
  `table_title` and `chart_title` (it keeps `header_image` / `footer_image`).
- Input sizes are the fixed square resize in `inference.yml`
  (`keep_ratio: false`); the 640 / 800 figures for L and V3 are also what the
  RapidAI ONNX graphs expect and what `_onnx.py` `LAYOUT_INPUT_SIZES` pins.

### 4.3 What pdfspine ships

Two variants are wired behind `vision_options={"layout_variant": ...}`
(`_onnx.py` `LAYOUT_VARIANTS`), both RapidAI ONNX exports fetched from
ModelScope at pinned revisions:

| Variant | File | Pinned source | Input | Classes | Output row |
|---|---|---|---|---|---|
| `pp_doclayout_l` (default) | `pp_doclayout_l.onnx`, 123 MB | ModelScope `RapidAI/RapidDoc` @ `v1.0.0`, `layout/PP-DocLayout-L/` | 640×640 | 23 | `(class_id, score, x0, y0, x1, y1)` |
| `pp_doclayoutv3` | `pp_doc_layoutv3.onnx`, 124 MB | ModelScope `RapidAI/RapidLayout` @ `v1.2.0`, `onnx/pp_doc_layout/` | 800×800 | 25 | same + 7th column: per-box reading-order key |

Both graphs take three named inputs (`image`, `im_shape`, `scale_factor`)
and return boxes already in original-image pixels, RT-DETR style: no
letterbox, no ImageNet mean / std, and **no built-in NMS**, so pdfspine runs
a same-class IoU pass afterwards. The RapidLayout release notes record
`v1.1.0` as "support PP-DocLayoutV2" and `v1.2.0` as "support
PP-DocLayoutV3"; RapidDoc's README lists PP-DocLayoutV3 as its own default
("自带阅读顺序，支持异形框，默认使用"). SLANet-plus stays
`slanet-plus.onnx` (6.8 MB) from ModelScope `RapidAI/RapidTable` @ `v2.0.0`.
PP-DocLayout-S / M / plus-L have no pdfspine wiring; adding one is a
file-name and label-list entry in `_onnx.py`.

### 4.4 Recommendation

Measured on the same three FinTabNet.c pages as the original baseline
(ADBE_2011_page_118, ADI_2010_page_51, AMP_2015_page_94; details and every
number in
[`docs/onnx-backend-baseline-2026-09-08.md`](onnx-backend-baseline-2026-09-08.md),
"Layout model switched to PP-DocLayout"), **PP-DocLayoutV3 is materially
better than PP-DocLayout-L**. V3 recovers the full table box, including the
wide row-label column, on every table of every page (IoU 0.97 / 0.99 / 0.94 /
0.85 / 0.92 against gold) and leaves no text-layer word unclaimed on two of
the three pages; its per-box reading-order key also replaces the geometric
band rule that misordered two-column pages. PP-DocLayout-L fixes ADI (IoU
0.98) but classifies ADBE's shaded, zebra-striped table as `image` (0.83 vs
0.38 as `table`, so `find_tables()` returns nothing at the 0.5 threshold) and
emits a nested duplicate table box on ADI that IoU-based NMS cannot remove
(IoU 0.31), while still cropping AMP T1 / T2 (IoU 0.36 / 0.62). The cost of
V3 is roughly 40 % more per-page latency (2.5–3.5 s vs 1.6–2.0 s on CPU) and
one missed running-head block. **The shipped default is currently
PP-DocLayout-L** (`DEFAULT_LAYOUT_VARIANT` in `_onnx.py`); the survey's
recommendation is to flip it to V3 once a scored run over the full 150-page
slice confirms the three-page picture.

---

## 5. Evaluation resources

### 5.1 Metric implementations

| Metric | Implementation | Notes |
|---|---|---|
| GriTS | `conformance/gt/grits.py` (pdfspine, stdlib-only port) | Top + Con; official reference is `src/grits.py` inside `microsoft/table-transformer`; no standalone `grits` PyPI package exists |
| TEDS | `table-recognition-metric` (PyPI, author SWHL, Apache-2.0, Python 3.6–3.14) | needed to compare with the Docling paper numbers |
| TEDS + layout | `docling-eval` (IBM; CLI batch evaluation over PubTabNet / PubTables-1M / FinTabNet / OmniDocBench) | heavy; pulls in the Docling stack |

### 5.2 Datasets

- **PubTables-1M** (`bsmock/pubtables-1m` on HF): ~947K cropped table
  instances for structure, ~575K pages for detection; no native `datasets`
  integration, download via `wget` of tar.gz archives.
- **FinTabNet**: original release CDLA-Permissive; cleaned version
  `bsmock/FinTabNet.c` on HF. pdfspine already holds a 150-page / 186-table
  slice of it.
- **PubTabNet**: ~568K table images from the PubMed Central OA subset.

### 5.3 Recommendation for a small-scale comparison

Use the FinTabNet.c **test split** (thousands of tables, GB-scale download) or
the PubTabNet val split, both downloadable from the HF repositories above. For
pdfspine specifically, the existing 150 / 186 FinTabNet.c slice is the cheapest
first pass, provided the harness gains a **gold-crop TSR-only mode** so that
structure-stage numbers are comparable with Microsoft's published ~0.98.

---

## 6. Sources (accessed 2026-09-07 unless marked otherwise)

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

DocLayout-YOLO licence evidence (§4.1; accessed 2026-09-08)

- https://raw.githubusercontent.com/opendatalab/DocLayout-YOLO/main/LICENSE
- https://pypi.org/pypi/doclayout-yolo/json
- https://www.ultralytics.com/license

PP-DocLayout family (§4.2; accessed 2026-09-08)

- https://paddlepaddle.github.io/PaddleX/latest/en/module_usage/tutorials/ocr_modules/layout_detection.html
  (S / M / L / plus-L: mAP, storage size, descriptions, eval-set note)
- https://paddlepaddle.github.io/PaddleX/3.7/en/module_usage/tutorials/ocr_modules/layout_analysis.html
  (V3: latency, storage size, description; V2 reference row)
- https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/docs/version3.x/module_usage/layout_detection.en.md
- https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/docs/version3.x/module_usage/layout_analysis.en.md
- https://huggingface.co/PaddlePaddle/PP-DocLayout-S,
  https://huggingface.co/PaddlePaddle/PP-DocLayout-M,
  https://huggingface.co/PaddlePaddle/PP-DocLayout-L,
  https://huggingface.co/PaddlePaddle/PP-DocLayout_plus-L,
  https://huggingface.co/PaddlePaddle/PP-DocLayoutV3 (model cards: licence,
  eval-set image counts; `raw/main/inference.yml` in each: `arch`,
  `label_list`, `Preprocess.Resize.target_size`)
- https://arxiv.org/abs/2503.17213 (PP-DocLayout paper)
- https://arxiv.org/abs/2606.23344 (RT-DocLayout paper; released as
  PP-DocLayoutV3)
- https://github.com/huggingface/transformers/blob/main/docs/source/en/model_doc/pp_doclayout_v3.md

RapidAI ONNX exports (§4.3; accessed 2026-09-08)

- https://github.com/RapidAI/RapidLayout,
  https://rapidai.github.io/RapidLayout/latest/models/,
  https://github.com/RapidAI/RapidLayout/releases
- https://github.com/RapidAI/RapidDoc
- https://github.com/RapidAI/RapidTable,
  https://github.com/RapidAI/RapidTable/releases/tag/v2.0.0
- ModelScope model pages `RapidAI/RapidDoc`, `RapidAI/RapidLayout`,
  `RapidAI/RapidTable` (the pinned `resolve/<rev>/...` download URLs are in
  `python/pdfspine/_onnx.py`; the pages themselves were unreachable from this
  session on 2026-09-08 — file sizes above are from the downloaded artefacts)
