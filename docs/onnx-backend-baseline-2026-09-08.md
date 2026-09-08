# ONNX layout/table backend — first real-model baseline (2026-09-08)

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
