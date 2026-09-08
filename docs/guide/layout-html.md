# Layout HTML (ONNX backend)

`Page.find_layout()` and `Page.get_layout_html()` turn one page of a
born-digital PDF into a list of labelled layout regions and into semantic HTML.
They are backed by the opt-in ONNX vision backend (DocLayout-YOLO for layout,
SLANet-plus for table structure), the same backend that serves
`find_tables(strategy="vision", backend="onnx")`. This page is for people who
call these methods and for maintainers who will extend the backend; the API
details live in [Tables](../reference/tables.md#vision-onnx-doclayout-yolo-slanet-plus).

## Goal

Turn born-digital financial PDFs (annual reports, prospectuses, filings) into
semantic HTML as the first step of a RAG ingestion pipeline: headings become
`<h2>`, paragraphs `<p>`, tables real `<table>` elements with `rowspan` /
`colspan`, captions and unit notes stay attached to their table, and page
furniture (running headers, footers, page numbers) is dropped. The HTML is
meant to be chunked and embedded downstream, not rendered for humans.

## Hard requirements

These are not preferences; a change that violates one of them is wrong even if
it improves a benchmark.

- **Text is 100 % from the PDF text layer.** The models predict *where*
  regions and cells are; every character in the output comes from pdfspine's
  native word coordinates. No OCR and no VLM-generated text is ever used for
  table or paragraph content on a page that has a text layer. (The page-level
  `ocr_if_no_text` fallback exists only for pages with no text layer at all,
  exactly as in the TATR backend.)
- **Permissive licences only.** DocLayout-YOLO and SLANet-plus are Apache-2.0
  exports published by RapidAI (opendatalab DocLayout-YOLO from RapidLayout
  v1.2.0, PaddleOCR SLANet-plus from RapidTable v2.0.0). Nothing from MinerU,
  PyMuPDF, PyMuPDF-Layout or the Docling framework may be pulled in.
- **onnxruntime only.** No torch, no Docling, no PaddlePaddle runtime. The
  `pdfspine[onnx]` extra is `onnxruntime`, `numpy`, `Pillow` and nothing else.
- **Weights are never in the wheel.** Model files are downloaded by the user
  and located through `PDFSPINE_ONNX_MODELS` or explicit paths; a missing file
  raises `PdfUnsupportedError` with the download URL.
- Importing `pdfspine` must stay free of ML imports; the runtime is loaded
  lazily on first use and cached per process.

## Setup

```bash
pip install "pdfspine[onnx]"
mkdir -p ~/models/pdfspine-onnx
curl -L -o ~/models/pdfspine-onnx/doclayout_yolo_docstructbench_imgsz1024.onnx \
  https://www.modelscope.cn/models/RapidAI/RapidLayout/resolve/v1.2.0/onnx/doclayout/doclayout_yolo_docstructbench_imgsz1024.onnx
curl -L -o ~/models/pdfspine-onnx/slanet-plus.onnx \
  https://www.modelscope.cn/models/RapidAI/RapidTable/resolve/v2.0.0/slanet-plus.onnx
export PDFSPINE_ONNX_MODELS=~/models/pdfspine-onnx
```

For CUDA install `onnxruntime-gpu` instead of `onnxruntime`; the default
`providers="auto"` uses `CUDAExecutionProvider` when it is available and CPU
otherwise. Do not request `CoreMLExecutionProvider` (see
[Known limitations](#known-limitations)).

```python
import pdfspine

doc = pdfspine.open("annual-report.pdf")
page = doc[12]

for block in page.find_layout():
    print(block.label, round(block.score, 2), block.bbox)

html = page.get_layout_html()
tables = page.find_tables(strategy="vision", backend="onnx")
```

Every option of [`OnnxOptions`](../reference/tables.md#onnxoptions) can be
passed as a keyword argument to `find_layout()` / `get_layout_html()`, or as
`vision_options={...}` to `find_tables()`, for example
`page.get_layout_html(dpi=200, layout_threshold=0.3)`.

## What you get

### `find_layout()` → `list[LayoutBlock]`

`LayoutBlock(bbox: Rect, label: str, score: float)`, with `bbox` in page
points (the same coordinate space as `get_text("words")`) and `label` one of
the DocLayout-YOLO DocStructBench classes:

| Label | Meaning |
|---|---|
| `title` | section heading |
| `plain text` | body paragraph |
| `table` | table region (structure recognised by SLANet-plus) |
| `table_caption`, `table_footnote` | text above / below a table, including unit notes |
| `figure`, `figure_caption` | chart or image, and its caption |
| `isolate_formula`, `formula_caption` | display formula and its number |
| `abandon` | running header / footer / page number |

### `get_layout_html()` → `str`

One HTML fragment per block, joined with newlines, in reading order:

| Label | HTML |
|---|---|
| `title` | `<h2>…</h2>` |
| `plain text` | `<p>…</p>` |
| `table_caption`, `figure_caption`, `table_footnote` | `<p class="table_caption">…</p>` etc. |
| `table` | `Table.to_html()` (`<thead>`, `rowspan`, `colspan`); when structure recognition finds no cells, `<pre class="table">` with the region's text-layer lines |
| `figure` | `<figure data-bbox="x0 y0 x1 y1"></figure>` placeholder, no text |
| `isolate_formula`, `formula_caption` | `<p class="formula">…</p>` |
| `abandon` | dropped (its words are consumed so they cannot leak into another block) |
| words outside every block | `<pre class="unclaimed">` appended at the end of the page, so text-layer words are never lost |

Text inside a block is the block's text-layer words grouped into visual lines
and HTML-escaped. Tables claim the words of their recognition crop first (their
cells are filled geometrically); every other word belongs to the first block in
reading order whose box contains its centre, so no word is emitted twice.

### Reading order

Blocks are sorted with a two-column band rule: a block wider than 60 % of the
page, or horizontally centred, is *full-width* and closes the current band;
inside a band, blocks whose centre is left of the page middle come first, then
the right column, each top-to-bottom. This handles the common single-column
and two-column financial-report layouts.

TODO: replace the band rule with a recursive XY-cut so that three-column pages
and pages whose column layout changes mid-page are ordered correctly.

## Known limitations

- **`Table.rows` / `Table.cols` are approximations.** SLANet-plus predicts
  cells only. Row and column bands are derived from the union of the cell
  boxes (single-span cells for each index, falling back to any cell touching
  it). `cells`, `spans`, `extract()` and `to_html()` do not depend on them.
- **Table detection may crop to the numeric block.** On financial statements
  DocLayout-YOLO sometimes returns only the block of numbers and misses the
  wide row-label column with dotted leaders. The table is then structurally
  correct but lacks its first column; the labels end up in a neighbouring
  `plain text` block. The layout boxes for the other classes have looked good
  on the pages tried so far.
- **Weak structure on borderless tables.** SLANet-plus was trained mostly on
  ruled or lightly ruled tables; long borderless statements may come back with
  merged or split columns.
- **Models are not yet validated on an evaluation set.** The observations
  above come from a handful of FinTabNet pages, not from a measured score.
- **CoreML crashes.** `CoreMLExecutionProvider` was tried with onnxruntime
  1.29 on macOS and aborted the process ("Error in building plan") while
  loading the models. `providers="auto"` therefore never selects it; use CPU
  on Apple silicon.
- Reading order is the band rule above, not an XY-cut.

## Next steps

1. **Evaluation set.** 30–50 pages from real financial reports (statements,
   notes with unit lines, two-column narrative pages, pages with charts), each
   with hand-written correct HTML in the same tag vocabulary as
   `get_layout_html()`.
2. **Scorer.** TEDS (tree edit distance similarity) on the `<table>` elements
   plus a cell-alignment score against the native words, and a block-level
   reading-order score for the rest of the page. The existing GriTS harness
   in `conformance/gt/` is the natural home.
3. **Decide priorities from the numbers**, in whatever order the scores point
   to: recursive XY-cut reading order; a better `rows` / `cols` derivation;
   the numeric-block cropping (enlarge the detection with native line or text
   evidence, as the TATR backend already does); fine-tuning or swapping the
   structure model for borderless tables.
4. Fold the backend into the `TableStructureBackend` Protocol described in
   [ADR 0002](../adr/0002-table-structure-backends.md) when that refactor
   lands.

## Ingestion-side guidance

These points are outside pdfspine but are the reason the output looks the way
it does.

- **One table is one chunk.** Never split a `<table>` by token count; if it is
  too large for the embedding window, summarise or split by row groups while
  repeating the header row.
- **Keep the context with the table.** The nearest `table_caption`, the
  enclosing `<h2>`, and any `table_footnote` (unit notes such as
  "单位：万元", "in thousands of USD", restatement remarks) belong in the same
  chunk as the table; without them the numbers are meaningless to retrieval.
- **Merge cross-page tables.** A table whose header row equals the header row
  of the last table on the previous page is a continuation; concatenate the
  bodies before chunking.
- **`figure` is only a placeholder.** Crop the region given by `data-bbox`
  from a page render and let a vision-language model describe it; index the
  description, not the pixels.
- **Page images are a fallback for answering, not a retrieval unit.** Attach
  the rendered page to the answer context when a chunk is hit; do not embed
  page images as if they were text chunks.

## Do not

- Do not fill table or paragraph text from OCR or a VLM on pages that have a
  text layer. The text layer is the ground truth; the models only locate it.
- Do not add torch, Docling, `docling-ibm-models`, MinerU, PyMuPDF or
  PyMuPDF-Layout as dependencies of this backend, even as optional extras.
- Do not ship model weights in the wheel or download them implicitly at import
  time.
- Do not let `providers="auto"` pick CoreML until the crash is understood and
  fixed upstream.
- Do not change the tag mapping above without updating the evaluation set's
  hand-written HTML in the same change.

## Known issues

The first by-eye run against real financial-report pages with gold
annotations (three FinTabNet.c pages) is recorded in
[ONNX backend baseline (2026-09-08)](../onnx-backend-baseline-2026-09-08.md):
numbers, the numeric-block cropping failure mode from "Table detection may
crop to the numeric block" above in detail, `"$"`-as-its-own-column and
multi-line row-label misattribution, hallucinated merged cells, and a fix
priority order.
