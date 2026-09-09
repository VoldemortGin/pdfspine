# ONNX layout/table backend — first real-model baseline (2026-09-08)

> **Superseded in part.** Sections up to the horizontal rule were measured
> with DocLayout-YOLO, which pdfspine dropped the same day over its AGPL-3.0
> lineage. The current numbers, for PP-DocLayout-L and PP-DocLayoutV3, are in
> [Layout model switched to PP-DocLayout (licence)](#layout-model-switched-to-pp-doclayout-licence--2026-09-08-later-the-same-day)
> at the end of this page.

- Repo/branch: `feat/onnx-vision-backend` @ `1fb964d` (no repository files changed
  by the run itself).
- Purpose: the ONNX backend (`Page.find_layout()`, `Page.get_layout_html()`,
  `find_tables(strategy="vision", backend="onnx")`, documented in
  [`docs/guide/layout-html.md`](guide/layout-html.md)) had never been run against
  real financial-report pages with gold annotations. This is that first run — a
  by-eye + numeric read of three FinTabNet.c pages, not a scored benchmark (no
  evaluation set or scorer exists yet; see
  [ADR 0002](adr/0002-table-structure-backends.md) and
  [PRD-NEXT P3-6](PRD-NEXT.md)).

## Environment

- `onnxruntime` 1.29.0, `providers="auto"` → `CPUExecutionProvider` (no CUDA/CoreML
  in this run).
- Models: `doclayout_yolo_docstructbench_imgsz1024.onnx` (RapidAI RapidLayout
  v1.2.0) + `slanet-plus.onnx` (RapidAI RapidTable v2.0.0), both Apache-2.0,
  located via `PDFSPINE_ONNX_MODELS`.
- Rendering/overlay: `page.get_pixmap(dpi=150)` + Pillow, page coordinates scaled
  by 150/72; `dpi=144` default for the layout/table calls themselves.
- Latency: **3.2–3.7 s/page** for `find_layout()` + `get_layout_html()` +
  `find_tables()` combined, with models already cached in the process.
- Corpus: `conformance/gt/corpus-fintabnet` (FinTabNet.c, CDLA-Permissive). Gold
  `pdf_bbox` / `pdf_table_bbox` are top-left-origin, y-down — the same coordinate
  space as pdfspine's page geometry, so no y-flip is needed (`tables_diff.py`
  already relies on this). Verified by reassembling each gold cell's text from
  `get_text("words")` inside its `pdf_bbox` and comparing to
  `json_text_content`: exact matches on ADBE (107/107), AMP's three tables
  (45/45, 25/25, 63/63); ADI's 107/135 mismatches are dotted leaders ("Revenue .
  . . .") tokenised as separate words, not a coordinate problem.
- Three pages (2 single-table, 1 multi-table with 4-level headers and merged
  cells):
  - `ADBE_2011_page_118` — one table, ruled-ish, single header row.
  - `ADI_2010_page_51` — one income statement, borderless, dotted leaders.
  - `AMP_2015_page_94` — three tables, up to 4-level headers, merged cells.

## Core numbers

| Page | Tables detected / gold | `Table.bbox` IoU | Predicted rows×cols | Gold rows×cols | Unclaimed words |
|---|---|---|---|---|---|
| ADBE_2011_page_118 | 1 / 1 | 0.96 | 16×11 | 16×8 | 0 |
| ADI_2010_page_51 | 1 / 1 | **0.32** | 31×4 | 42×4 | **897** |
| AMP_2015_page_94 T0 | 3 / 3 | 0.93 | 7×12 | 7×9 | 21 (whole page) |
| AMP_2015_page_94 T1 | — | **0.29** | 5×6 | 7×5 | — |
| AMP_2015_page_94 T2 | — | 0.63 | 9×11 | 9×9 | — |

Column counts run high across every table because SLANet-plus frequently
splits a `$` sign into its own predicted column (see
[Issue 2](#issue-2-loses-the-row-label-column-or-splits--into-its-own-column)).
Merged-cell counts (`n_merged`) are similarly unreliable: ADBE predicts 0 vs
3 gold colspan=8 rows, ADI predicts 22 (all wrong) vs 10 gold, AMP T2 predicts
12 vs 6 gold, including a hallucinated `rowspan=4`.

## Three points that were rechecked and confirmed correct

1. **SLANet-plus output order.** `save_infer_model/scale_0.tmp_0` is `[N, L, 8]`
   (bbox), `save_infer_model/scale_1.tmp_0` is `[N, L, 50]` (structure probs).
   `_split_table_outputs` identifies them by last-dimension size (8 vs >8), not
   by positional index, matching RapidTable. The model's `character` metadata
   (48 entries) is element-for-element equal to `SLANET_STRUCTURE_DICT`; decoding
   `[<sos>, *48, <eos>]` = 50 classes matches the output width, and bbox
   coordinates are only taken from `<td`/`<td></td>` tokens up to `<eos>`, as in
   PaddleOCR's `TableLabelDecode`. Bbox denormalization uses
   `scale = max(crop_w, crop_h)` applied to both x and y (one 488×488 padded
   canvas), and the recognized cells land squarely on their text in every
   `vis.png`, including ADI's 324×1122 px crop — confirming this is correct, not
   coincidentally close.
2. **Channel order (BGR/RGB).** DocLayout-YOLO (Ultralytics export) is fed RGB;
   SLANet-plus (Paddle/cv2-trained) is fed BGR via `channel_order="bgr"` (default).
   Swapping to `channel_order="rgb"` for SLANet-plus left ADBE/ADI/AMP-T0/AMP-T2
   structurally unchanged and only shifted AMP-T1 by one row; the model is
   essentially channel-order-insensitive on these pages, so the current default
   is correct and channel order is **not** a lever for improving structure.
3. **NMS.** The DocLayout-YOLO export is YOLOv10 end-to-end (already NMS'd);
   `_nms(iou=0.7)` is a correct second pass (ADBE 12→11, ADI 8→8, AMP 18→17
   boxes). It does **not** catch fully-nested boxes with low IoU but high
   IoB — ADBE block #4/#5 sit entirely inside #3 (IoB=1.0, IoU≈0.41), which no
   IoU threshold removes. Recommendation: add an IoB (intersection over smaller-box
   area) criterion to `_nms`, or dedupe fully-contained same-class blocks in
   `_layout_blocks`.

## Problems found (each with page evidence)

### Issue 1: table detection misses the row-label column

`ADI_2010_page_51` (IoU 0.32, 897 unclaimed words — ~73% of the page),
`AMP_2015_page_94` T1 (IoU 0.29) and T2 (IoU 0.63) all lose the wide row-label
column on the left of a borderless financial table, because DocLayout-YOLO
crops to the numeric block. Consequences cascade: the label text becomes
disconnected `plain text` blocks (or a misclassified `title`), the reading-order
band rule sees the page as two columns and reorders blocks incorrectly (AMP:
T2's intro line ends up before T1 in the HTML), and unclaimed word counts spike.
This is the single most severe and most repeated failure across the three
pages — 3 of 5 tables lose their semantics this way.

### Issue 2: loses the row-label column, or splits "$" into its own column

Independent of Issue 1, predicted column counts run high because a lone `$`
becomes its own column: ADBE predicts 11 columns vs gold's 8 (3 extra `$`
columns; a percentage row is shifted right by two cells because of it); AMP
T0/T2 predict 12/9 and 11/9 for the same reason, plus `"(in billions)"` split
into two cells. ADI's dotted leaders leak into the first numeric cell
(`". $2,761,503"`) because the crop boundary catches part of the leader.

### Issue 3: multi-line row labels are misattributed, merged cells are hallucinated

Two-line row labels ("Gross profit as a percentage of revenue", "Threadneedle
managed assets") have their SLANet-plus cell box hug only the second line; word
assignment (`_assign_words`, nearest-cell-box rule) then gives the first line to
the *previous* row's label cell, producing garbled concatenations like "Gross
profit<br>Gross profit as a" / "percentage<br>of revenue" (seen on ADBE and AMP
T0). Merged-cell prediction is unreliable in both directions: ADBE predicts 0
merges vs 3 gold `colspan=8` section rows; ADI predicts 22 `colspan=2` cells,
all spurious, while the 10 real merges (in the column that Issue 1 cropped out)
are entirely missed; AMP T2 hallucinates `rowspan=4` then `rowspan=2` on
adjacent `$` cells with no basis in the gold layout.

### Issue 4: band reading-order rule misjudges two-column pages

The reading order is downstream damage from Issue 1: once a table's detection
box shrinks to the right-hand numeric block, the band rule treats the page as
two columns and interleaves blocks incorrectly (AMP: `#8`–`#11`, the T1 label
column, "NM Not Meaningful.", and the T2 intro line, are ordered before the T1
table itself). On the one page where the table boxes were accurate (ADBE),
reading order was correct. A recursive XY-cut would not fix this class of
error, because the input blocks are themselves wrong.

## Fix priority

1. **Table detection box expansion (highest priority).** Evidence: Issue 1
   (ADI IoU 0.32/897 unclaimed; AMP T1 IoU 0.29; AMP T2 IoU 0.63) — 3 of 5
   tables across the three pages lose semantics this way, and it is the root
   cause behind the reading-order errors in Issue 4 and most of the unclaimed
   words in Issue 3. Approach (no new model required): seed from the detected
   table's row bands, then grow the box leftward to absorb text lines that are
   y-aligned with the table rows and separated only by whitespace or dotted
   leaders, stopping at the first non-aligned line.
2. **Cell text assignment / column merging.** Evidence: Issue 2 (ADBE 11 vs 8
   columns; AMP T0 12 vs 9; misaligned percentage row) and the multi-line
   label misattribution in Issue 3. Approach, all geometric post-processing:
   (i) merge predicted columns that contain only `$` or blank cells into their
   right-hand numeric neighbor, using the words' x-projection; (ii) assign
   words to row-label cells by row-band y-range rather than nearest-cell-box,
   so a multi-line label's first line is not stolen by the row above; (iii)
   strip standalone `.` leader tokens from cropped cells.
3. **Swap or fine-tune the structure model.** Evidence: Issue 3's 22 spurious
   `colspan=2` cells and the hallucinated `rowspan=4` on ADI/AMP-T2, plus
   ADBE's fully-missed `colspan=8` section rows — SLANet-plus's header/merge
   grouping is unstable on borderless financial tables. Deferred until
   priorities 1–2 land: more than half of the current rows×cols gap traces to
   the wrong detection box and the `$` column split, so a model swap's real
   benefit cannot be measured yet.
4. **Recursive XY-cut reading order (deferred).** ADBE's reading order was
   already correct; ADI's and AMP's disorder are downstream of Issue 1, not a
   band-rule limitation — revisit only after priority 1 lands. The block-label
   misjudgments noted along the way (ADI's "ITEM 8 FINANCIAL STATEMENTS..."
   dropped as `abandon`; AMP's "Total managed asset net flows" mislabeled
   `title`) and the nested-box NMS gap (see [point 3](#three-points-that-were-rechecked-and-confirmed-correct)
   above) are cheaper heuristic fixes worth doing first in `get_layout_html()`.

## Reproducing this run

```bash
pip install "pdfspine[onnx]"
export PDFSPINE_ONNX_MODELS=~/models/pdfspine-onnx   # see docs/guide/layout-html.md Setup
.venv/bin/python conformance/gt/fetch_fintabnet.py   # populates conformance/gt/corpus-fintabnet
.venv/bin/python scripts/onnx_vis.py --out /tmp/onnx-vis
```

`scripts/onnx_vis.py` renders each page (default: the three pages above,
override with `--pages <doc_id> ...`), runs `find_layout()` +
`get_layout_html()` + `find_tables(strategy="vision", backend="onnx")`, and
writes to `--out` (default: the current directory): `<doc_id>.out.html` (the
layout HTML), `<doc_id>.vis.png` (predicted layout blocks colored by label,
plus `Table.bbox`/`detection_bbox`/`recognition_crop_bbox` and cell/merged-cell
overlays), `<doc_id>.gold.png` (FinTabNet.c gold table/cell boxes for
comparison), and `stats.json` (the numbers in the table above, per page). It
depends only on `pdfspine` + Pillow. PNGs and HTML outputs are not committed to
the repository — regenerate them with the command above.

---

## Layout model switched to PP-DocLayout (licence) — 2026-09-08, later the same day

Everything above this line was measured with **DocLayout-YOLO**
(`doclayout_yolo_docstructbench_imgsz1024.onnx`). That model has since been
removed from pdfspine and the numbers above are therefore historical. This
section records why, and re-measures the same three pages with the replacement.

### Why: AGPL-3.0 lineage

pdfspine's whole dependency chain — models included — must be Apache-2.0 or
more permissive. Three independent pieces of evidence put DocLayout-YOLO
outside that line (all checked 2026-09-08):

1. The upstream repository's licence file is the AGPL-3.0 text:
   <https://raw.githubusercontent.com/opendatalab/DocLayout-YOLO/main/LICENSE>.
2. The PyPI package declares AGPL-3.0, and its `pyproject.toml` carries the
   same classifier: <https://pypi.org/pypi/doclayout-yolo/json>. The project is
   a fork of the Ultralytics code base, whose own position is AGPL-3.0 or a
   commercial licence: <https://www.ultralytics.com/license>.
3. The RapidAI ONNX export we were loading embeds `author=Ultralytics,
   license=AGPL-3.0` in its own metadata.

The Apache-2.0 tag on the weights card is not enough to override an upstream
project's LICENSE, its package metadata and the artefact's own stamp. The
model was removed rather than negotiated; see
[ADR 0002](adr/0002-table-structure-backends.md) for the standing rule and
[the model survey](table-structure-models-survey.md) for the full evidence.

### Replacement: PaddleX PP-DocLayout (Apache-2.0)

An RT-DETR detector from PaddleX, ONNX-exported by RapidAI, both Apache-2.0.
Two variants ship behind `vision_options={"layout_variant": ...}`:

| | PP-DocLayout-L | PP-DocLayoutV3 (default) |
|---|---|---|
| File | `pp_doclayout_l.onnx` (123 MB) | `pp_doc_layoutv3.onnx` (124 MB) |
| Input | 640x640 | 800x800 |
| Classes | 23 | 25 (adds `vision_footnote`, `vertical_text` and `reference_content`; splits `formula` into display/inline; drops `table_title` and `chart_title`) |
| Output row | `(cls, score, x0, y0, x1, y1)` | same + a 7th per-box reading-order key |

Inputs are named (`image`, `im_shape`, `scale_factor`), not positional;
preprocessing is a plain resize to the square input with RGB scaled to
`[0, 1]` — no letterbox and no ImageNet mean/std, unlike the previous model —
and the head returns boxes already in original-image pixels because it
recovers the original size internally as `im_shape / scale_factor`. RT-DETR
emits no NMS of its own, so pdfspine runs a **same-class** IoU-0.6 pass (the
old model needed a class-agnostic one). Defaults moved with the model:
`layout_threshold` 0.25 → 0.5 (RapidLayout's own default; below it the head's
duplicate (query, class) pairs dominate), `layout_nms_iou` 0.7 → 0.6, and
`layout_size` now defaults to `None`, meaning "read the model's own input
edge".

The model's 23/25 raw class names are richer than pdfspine's ten downstream
labels, so they are normalised (`text`/`abstract`/`aside_text`/... →
`plain text`, `paragraph_title`/`doc_title` → `title`, `image`/`chart`/`seal`
→ `figure`, and so on). `LayoutBlock.raw_label` keeps the model's own class;
the mapping table is in
[the layout HTML guide](guide/layout-html.md).

### Same three pages, re-measured

Same corpus, same harness (`scripts/onnx_vis.py --variant ...`), same
`onnxruntime` 1.29.0 / `CPUExecutionProvider`, same `dpi=144`.

| Page / table | Gold | DocLayout-YOLO (old) | PP-DocLayout-L | PP-DocLayoutV3 |
|---|---|---|---|---|
| ADBE_2011_page_118 | 16x8 | IoU 0.96, 16x11 | **not detected** (classified `image`) | IoU **0.97**, 16x13 |
| ADI_2010_page_51 | 42x4 | IoU **0.32**, 31x4 | IoU **0.98**, 42x5 (+1 spurious nested box) | IoU **0.99**, 42x5 |
| AMP_2015_page_94 T0 | 7x9 | IoU 0.93, 7x12 | IoU 0.87, 7x11 | IoU **0.94**, 6x13 |
| AMP_2015_page_94 T1 | 7x5 | IoU **0.29**, 5x6 | IoU 0.36, 6x5 | IoU **0.85**, 8x7 |
| AMP_2015_page_94 T2 | 9x9 | IoU 0.63, 9x11 | IoU 0.62, 9x11 | IoU **0.92**, 9x10 |

Unclaimed text-layer words per page (lower is better):

| Page | DocLayout-YOLO | PP-DocLayout-L | PP-DocLayoutV3 |
|---|---|---|---|
| ADBE_2011_page_118 | 0 | 9 (the two title lines) | **0** |
| ADI_2010_page_51 | **897** | **0** | 23 (running head + the two title lines) |
| AMP_2015_page_94 | 21 | 12 | **0** |

Latency is unchanged in practice: 1.6–2.0 s/page for PP-DocLayout-L and
2.5–3.5 s/page for PP-DocLayoutV3 (`find_layout()` + `get_layout_html()` +
`find_tables()` combined, models already cached), against 3.2–3.7 s/page
before.

### What this fixes, and what it does not

- **Issue 1 (table detection crops to the numeric block) is gone with V3.**
  This was the dominant failure — 3 of 5 tables. Every table box on all three
  pages now spans the full table including the wide row-label column, and
  ADI's row count matches gold exactly (42 vs 42) instead of 31. PP-DocLayout-L
  fixes ADI too, but still crops AMP T1/T2.
- **Issue 4 (reading order) is gone with V3**, for a different reason than
  expected: the model emits a monotone reading-order key per box, which
  `_layout_blocks` uses instead of the geometric band rule whenever every
  detection carries one. Verified independently on a genuinely two-column page
  (`fixtures/corpus/cdc-mmwr-7301a1.pdf`): the key walks the left column top to
  bottom, then the right, across a mid-page column change. PP-DocLayout-L has
  no such key and still uses the band rule.
- **Issue 2 (`$` split into its own column) is unchanged** — it lives in
  SLANet-plus, which did not change. Predicted column counts still run high
  (V3: ADBE 13 vs 8, AMP T0 13 vs 9, T1 7 vs 5, T2 10 vs 9).
- **Issue 3 (merged cells) is still unreliable**, but less wildly so: ADI's 22
  spurious `colspan=2` cells are gone (both variants now predict 0 merges
  against 10 gold — a miss rather than a hallucination), ADBE V3 predicts 6
  merges vs 3 gold, AMP T2 4 vs 6 instead of 12 vs 6.

### Two new PP-DocLayout-L-specific problems

1. **A shaded table is classified `image`.** On ADBE the zebra-striped table
   scores 0.83 as `image` and only 0.38 as `table`, so at the 0.5 threshold
   `find_tables()` returns nothing and `get_layout_html()` emits a `<figure>`
   placeholder where the table should be. V3 calls the same region `table` at
   0.95. This is a hard regression against the old model, which scored the
   region 0.96 IoU as a table.
2. **Nested duplicate table boxes.** On ADI, L emits both the correct
   full-width box (0.82) and the old numeric-block-only box (0.93). Their IoU
   is 0.31, well under the 0.6 NMS threshold, so both survive and
   `find_tables()` returns two tables for a one-table page. Suppressing a
   same-class box that is almost entirely contained in another (an IoB rather
   than IoU criterion) would fix it; that is the same gap noted for the old
   model in [point 3](#three-points-that-were-rechecked-and-confirmed-correct)
   above and is still unimplemented.

### Recommendation

**Switch the default to PP-DocLayoutV3.** It is better than PP-DocLayout-L on
every table on all three pages, it is the only variant that finds the ADBE
table at all, it is the only one that never leaves a word unclaimed on two of
the three pages, and its reading-order key removes a whole class of
downstream error. Its costs are a ~40 % higher per-page latency and one
missed running-head block on ADI (23 unclaimed words, which are still emitted
verbatim in `<pre class="unclaimed">` and so are not lost).

The shipped default was flipped to `pp_doclayoutv3` the same day
(`DEFAULT_LAYOUT_VARIANT` in `python/pdfspine/_onnx.py`, plus the documented
defaults); PP-DocLayout-L stays available as the faster optional variant
(`layout_variant="pp_doclayout_l"`).

Revised fix priority for this backend, replacing the list above:

1. **Same-class containment suppression (IoB)** in `_nms` — removes the ADI
   nested-duplicate table and the stacked `table_title` boxes L produces.
   Cheap, and it is the only thing between PP-DocLayout-L and parity with V3
   on ADI.
2. **`$`-column merging and row-band word assignment** (old priority 2,
   unchanged) — now the largest remaining source of the rows x cols gap, since
   the detection box is no longer wrong.
3. **Structure-model swap or fine-tune** (old priority 3, unchanged) — merged
   cells remain unreliable; still deferred until 2 lands.
4. ~~Table detection box expansion~~ — resolved by the model swap.
5. ~~Recursive XY-cut reading order~~ — resolved for V3 by the model's
   reading-order key; still open for PP-DocLayout-L.

### Reproducing this section

```bash
export PDFSPINE_ONNX_MODELS=~/models/pdfspine-onnx   # see docs/guide/layout-html.md Setup
.venv/bin/python scripts/onnx_vis.py --out /tmp/onnx-vis/l  --variant pp_doclayout_l
.venv/bin/python scripts/onnx_vis.py --out /tmp/onnx-vis/v3 --variant pp_doclayoutv3
```

`--variant` was added to `scripts/onnx_vis.py` for this run; the box captions
in `<doc_id>.vis.png` now show the model's raw class name rather than the
normalised label.
