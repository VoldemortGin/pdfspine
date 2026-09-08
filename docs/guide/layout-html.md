# Layout HTML (ONNX backend)

`Page.find_layout()` and `Page.get_layout_html()` turn one page of a
born-digital PDF into a list of labelled layout regions and into semantic HTML.
They are backed by the opt-in ONNX vision backend (PP-DocLayout for layout,
SLANet-plus for table structure), the same backend that serves
`find_tables(strategy="vision", backend="onnx")`. This page is for people who
call these methods and for maintainers who will extend the backend; the API
details live in [Tables](../reference/tables.md#vision-onnx-pp-doclayout-slanet-plus).

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
- **Permissive licences only.** PP-DocLayout (Baidu PaddlePaddle / PaddleX,
  Apache-2.0) and SLANet-plus (PaddleOCR, Apache-2.0) are used as the
  Apache-2.0 ONNX exports published by RapidAI (PP-DocLayout-L from RapidDoc
  v1.0.0, PP-DocLayoutV3 from RapidLayout v1.2.0, SLANet-plus from RapidTable
  v2.0.0). The YOLO-based detector used before 2026-09-08 was dropped because
  its upstream repository, PyPI package and ONNX metadata all declare
  AGPL-3.0.
  Nothing from MinerU, PyMuPDF, PyMuPDF-Layout or the Docling framework may be
  pulled in.
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
curl -L -o ~/models/pdfspine-onnx/pp_doclayout_l.onnx \
  https://www.modelscope.cn/models/RapidAI/RapidDoc/resolve/v1.0.0/layout/PP-DocLayout-L/pp_doclayout_l.onnx
curl -L -o ~/models/pdfspine-onnx/slanet-plus.onnx \
  https://www.modelscope.cn/models/RapidAI/RapidTable/resolve/v2.0.0/slanet-plus.onnx
export PDFSPINE_ONNX_MODELS=~/models/pdfspine-onnx
```

Optional variant, recommended for financial statements (see
[Layout model variants](#layout-model-variants)):

```bash
curl -L -o ~/models/pdfspine-onnx/pp_doc_layoutv3.onnx \
  https://www.modelscope.cn/models/RapidAI/RapidLayout/resolve/v1.2.0/onnx/pp_doc_layout/pp_doc_layoutv3.onnx
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
    print(block.label, block.raw_label, round(block.score, 2), block.bbox)

html = page.get_layout_html()
tables = page.find_tables(strategy="vision", backend="onnx")
```

Every option of [`OnnxOptions`](../reference/tables.md#onnxoptions) can be
passed as a keyword argument to `find_layout()` / `get_layout_html()`, or as
`vision_options={...}` to `find_tables()`, for example
`page.get_layout_html(dpi=200, layout_threshold=0.4)`.

## Layout model variants

Two PP-DocLayout detectors (both RT-DETR heads) are supported; pick one with
`layout_variant`:

| `layout_variant` | File | Input | Classes | Notes |
|---|---|---|---|---|
| `"pp_doclayout_l"` (default) | `pp_doclayout_l.onnx` (~123 MB) | 640 x 640 | 23 | PP-DocLayout-L; reading order from the geometric band rule |
| `"pp_doclayoutv3"` | `pp_doc_layoutv3.onnx` (~124 MB) | 800 x 800 | 25 | PP-DocLayoutV3; emits a per-box reading-order key that replaces the band rule |
| `"auto"` (the default value) | either | | | picks the variant from the layout model's file name, falling back to PP-DocLayout-L |

```python
blocks = page.find_layout(layout_variant="pp_doclayoutv3")
html = page.get_layout_html(layout_variant="pp_doclayoutv3")
tables = page.find_tables(
    strategy="vision", backend="onnx",
    vision_options={"layout_variant": "pp_doclayoutv3"},
)
```

With `layout_variant="auto"` and `PDFSPINE_ONNX_MODELS` holding only one of
the two files, the variant follows the file present; pass `layout_model=` to
point at a specific file.

**PP-DocLayoutV3 is the recommended variant even though PP-DocLayout-L is the
current default.** On the three FinTabNet.c baseline pages V3 detects every
table with the correct extent (`Table.bbox` IoU 0.85-0.99), while
PP-DocLayout-L misclassifies a shaded table as `image` (so no table is found
on that page) and emits a nested duplicate table box on another; see
[ONNX backend baseline (2026-09-08)](../onnx-backend-baseline-2026-09-08.md)
for the numbers and [Known limitations](#known-limitations).

## What you get

### `find_layout()` → `list[LayoutBlock]`

`LayoutBlock(bbox: Rect, label: str, score: float, raw_label: str)`, with
`bbox` in page points (the same coordinate space as `get_text("words")`),
`label` one of pdfspine's normalised layout classes, and `raw_label` the
PP-DocLayout class the model actually predicted (it equals `label` when no
mapping applies):

| Label | Meaning |
|---|---|
| `title` | section or document heading |
| `plain text` | body paragraph, abstract, reference, aside text, algorithm block, ... |
| `table` | table region (structure recognised by SLANet-plus) |
| `table_caption`, `table_footnote` | text above / below a table, including unit notes |
| `figure`, `figure_caption` | chart, image or seal, and its caption |
| `isolate_formula`, `formula_caption` | display formula and its number |
| `abandon` | running header / footer / page number |

The normalised label is derived from the raw PP-DocLayout class as follows;
anything not listed passes through unchanged.

| PP-DocLayout class | pdfspine label |
|---|---|
| `text`, `abstract`, `content`, `reference`, `reference_content`, `aside_text`, `algorithm`, `vertical_text` | `plain text` |
| `paragraph_title`, `doc_title` | `title` |
| `table` | `table` |
| `table_title` (PP-DocLayout-L only) | `table_caption` |
| `figure_title`, `chart_title` (`chart_title` is PP-DocLayout-L only) | `figure_caption` |
| `image`, `chart`, `seal` | `figure` |
| `footnote` (PP-DocLayout-L) | `table_footnote` (approximation: L has no separate table-footnote class) |
| `vision_footnote` (PP-DocLayoutV3 only) | `table_footnote` |
| `footnote` (PP-DocLayoutV3, a real page footnote) | `plain text`, rendered as `<p class="footnote">` |
| `header`, `footer`, `number`, `header_image`, `footer_image` | `abandon` |
| `formula`, `display_formula`, `inline_formula` | `isolate_formula` |
| `formula_number` | `formula_caption` |

The full class lists, for reference. PP-DocLayout-L (23, in class-id order):
`paragraph_title`, `image`, `text`, `number`, `abstract`, `content`,
`figure_title`, `formula`, `table`, `table_title`, `reference`, `doc_title`,
`footnote`, `header`, `algorithm`, `footer`, `seal`, `chart_title`, `chart`,
`formula_number`, `header_image`, `footer_image`, `aside_text`.
PP-DocLayoutV3 (25, alphabetical): `abstract`, `algorithm`, `aside_text`,
`chart`, `content`, `display_formula`, `doc_title`, `figure_title`, `footer`,
`footer_image`, `footnote`, `formula_number`, `header`, `header_image`,
`image`, `inline_formula`, `number`, `paragraph_title`, `reference`,
`reference_content`, `seal`, `table`, `text`, `vertical_text`,
`vision_footnote`. V3 has neither `table_title` nor `chart_title`: table,
chart and figure captions all share `figure_title`, so on V3 a table caption
arrives as `figure_caption`.

### `get_layout_html()` → `str`

One HTML fragment per block, joined with newlines, in reading order:

| Label | HTML |
|---|---|
| `title` | `<h2>…</h2>` |
| `plain text` | `<p>…</p>` (`<p class="footnote">…</p>` for a PP-DocLayoutV3 page footnote) |
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

With PP-DocLayoutV3 every detected box carries a reading-order key predicted
by the model, and blocks are sorted by that key (ties broken top-to-bottom,
left-to-right). This handles pages whose column layout changes mid-page.

With PP-DocLayout-L (or whenever a box lacks the key) blocks are sorted with a
two-column band rule: a block wider than 60 % of the page, or horizontally
centred, is *full-width* and closes the current band; inside a band, blocks
whose centre is left of the page middle come first, then the right column,
each top-to-bottom. This handles the common single-column and two-column
financial-report layouts.

TODO: replace the band rule with a recursive XY-cut so that three-column pages
and pages whose column layout changes mid-page are ordered correctly on
PP-DocLayout-L too.

## Known limitations

- **`Table.rows` / `Table.cols` are approximations.** SLANet-plus predicts
  cells only. Row and column bands are derived from the union of the cell
  boxes (single-span cells for each index, falling back to any cell touching
  it). `cells`, `spans`, `extract()` and `to_html()` do not depend on them.
- **PP-DocLayout-L can miss or duplicate tables.** On the three FinTabNet.c
  baseline pages the default variant classified a shaded statement table as
  `image`, so `find_tables()` found nothing on that page, and on another page
  it emitted a second, nested table box inside the real one. PP-DocLayoutV3
  showed neither problem; use `layout_variant="pp_doclayoutv3"` for financial
  statements.
- **Table cropping to the numeric block is fixed by PP-DocLayoutV3.** The
  YOLO-based detector used before the model swap often returned only the block
  of numbers and dropped the wide row-label column. PP-DocLayoutV3 detects the full table on
  every baseline page (`Table.bbox` IoU 0.32 → 0.99, 0.29 → 0.85, 0.63 → 0.92;
  unclaimed words 897 → 23 and 21 → 0); PP-DocLayout-L fixes it on the pages
  where it finds the table at all. Numbers in
  [ONNX backend baseline (2026-09-08)](../onnx-backend-baseline-2026-09-08.md).
- **Weak structure on borderless tables.** SLANet-plus was trained mostly on
  ruled or lightly ruled tables; long borderless statements may come back with
  merged or split columns.
- **Models are not yet validated on an evaluation set.** The observations
  above come from a handful of FinTabNet pages, not from a measured score.
- **CoreML crashes.** `CoreMLExecutionProvider` was tried with onnxruntime
  1.29 on macOS and aborted the process ("Error in building plan") while
  loading the models. `providers="auto"` therefore never selects it; use CPU
  on Apple silicon.
- With PP-DocLayout-L the reading order is the band rule above, not an XY-cut.

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
   to: whether PP-DocLayoutV3 should become the default; recursive XY-cut
   reading order for PP-DocLayout-L; a better `rows` / `cols` derivation;
   fine-tuning or swapping the structure model for borderless tables.
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
numbers for both PP-DocLayout variants, the table-detection failures listed
under "Known limitations" above in detail, `"$"`-as-its-own-column and
multi-line row-label misattribution, hallucinated merged cells, and a fix
priority order.
