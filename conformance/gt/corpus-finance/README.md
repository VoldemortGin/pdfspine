# `corpus-finance` — hand-annotated financial-report table eval set

This is the corpus that pdfspine's table-structure work is measured against.
The rule it exists to enforce: **every change to table detection or structure
recognition is judged by the number this corpus produces, not by eyeballing a
few pages.**

It has two halves:

1. **A seed set you already have** — the 150-page / 186-table FinTabNet.c slice
   in `conformance/gt/corpus-fintabnet/`. Real financial filings, gold cell
   structure published by Microsoft, permissively licensed. Nobody has to
   annotate anything to start scoring. A 30–50 page recommended subset covering
   every table shape is listed in [`seed-subset.json`](seed-subset.json).
2. **Your own pages** — the insurance financial-report pages you care about,
   annotated by hand and dropped into this directory. FinTabNet is US 10-K
   filings from 2008–2015; it will not tell you how pdfspine does on the
   documents you actually process. This half is the one that matters, and this
   README is the instructions for building it.

Scoring for both halves is `conformance/gt/eval_tables.py`.

## Quick start

```sh
export PDFSPINE_ONNX_MODELS="$HOME/models/pdfspine-onnx"   # only for --backend onnx
P=.venv/bin/python

# Score the FinTabNet seed set (no annotation needed)
$P conformance/gt/eval_tables.py run --backend lines --backend text --backend onnx

# Score your own pages once you have annotated some
$P conformance/gt/eval_tables.py run --corpus conformance/gt/corpus-finance --backend onnx
```

## What is committed and what is not

Only the hand-written, license-clean parts of this directory are tracked:
`README.md`, `seed-subset.json`, `annotate_template.json`. **`pdfs/` and
`annotations/` are gitignored** — source financial documents are of unknown
copyright and must never be committed, and your gold files describe them
closely enough to inherit the problem. Keep both in your own backup.

Everything here is stdlib-only and offline; no annotation tool to install.

## Adding a page

### 1. Drop the PDF in

```
conformance/gt/corpus-finance/pdfs/<document_id>.pdf
```

`<document_id>` is free-form but must be unique, filename-safe, and stable —
it is the key every score is reported under. The convention that works well is
`<issuer>_<year>_<form>_page_<N>`, e.g. `pingan_2024_annual_page_137`. One page
per PDF is easiest; if you keep a multi-page PDF, record the page in the gold
file's `page_index` and name the file after the document, not the page.

### 2. Generate a draft, then correct it

Do not hand-write a gold file from scratch — start from pdfspine's own output
and fix what is wrong. That turns a 20-minute job per page into a 3-minute one.

```sh
$P conformance/gt/eval_tables.py draft-gold \
    --pdf conformance/gt/corpus-finance/pdfs/pingan_2024_annual_page_137.pdf \
    --page 0 --backend onnx
```

This writes `annotations/<stem>_p<N>.gold.json` (the file you edit) plus a
sibling `.gold.html` you can open in a browser to see the predicted grid. Fix
the cells that are wrong, then **set `"_draft": false`**. A file left at
`"_draft": true` is loaded but reported separately, so an uncorrected draft can
never silently inflate a score.

The single most common correction is column count: borderless financial tables
make engines split a `$` sign or a footnote marker into its own column, and
merge or drop header cells that span. Those are exactly the errors the metrics
are meant to catch, so correct them carefully.

### 3. Tag it

Add the shape tags to each table's `tags` list. The vocabulary is fixed —
`conformance/gt/table_gold.py:TAG_VOCABULARY` — and tags drive the per-category
breakdown in the report, which is where regressions actually show up:

| tag | meaning |
|---|---|
| `plain` | ordinary ruled table, no spans, moderate size |
| `borderless` | no ruling lines; structure is implied by whitespace alone |
| `multi-header` | more than one header row, or a header cell spanning columns |
| `spanning` | any cell with `rowspan > 1` or `colspan > 1` |
| `wide` | 8 or more columns |
| `tall` | 20 or more rows |
| `multi-table` | the page carries 2 or more tables |

Two categories from the original brief need a word of explanation:

- **Cross-page tables** (a table continuing onto the next page) are annotated as
  what they physically are: one table per page, each with its own gold file.
  pdfspine's `find_tables` is per-page, so scoring a stitched logical table
  would measure something the engine does not claim to do. Tag both halves
  `plain`/`multi-header` as appropriate and note the continuation in
  `"comment"`; stitching is a separate, later feature with its own metric.
- **Chart pages** (a figure that is not a table) are the negative controls.
  Annotate the page with `"tables": []`. Every table an engine reports there is
  a false positive, and detection precision in the report is where you see it.
  Include a handful — an engine that hallucinates tables on chart pages scores
  well on recall-weighted metrics while being useless in production.

Aim for 30–50 pages, weighted toward what you actually process. A sensible
starting mix: 10 plain, 10 borderless, 8 multi-header, 6 multi-table, 4 chart
pages, plus whatever cross-page cases you have.

## Gold file format

Two spellings are accepted. Both live in `annotations/`.

### `<name>.gold.json` — the precise one

Cells carry geometry, so it supports every metric including cell-alignment F1.
This is what `draft-gold` writes.

```json
{
  "document_id": "pingan_2024_annual_page_137",
  "page_index": 0,
  "_draft": false,
  "tables": [
    {
      "table_id": "pingan_2024_annual_page_137_0",
      "bbox": [52.0, 420.0, 561.0, 676.9],
      "tags": ["borderless", "multi-header"],
      "comment": "continues on page 138",
      "cells": [
        {"row": 0, "col": 0, "rowspan": 2, "colspan": 1,
         "text": "(in millions)", "bbox": [52.0, 420.0, 141.3, 444.9],
         "header": true},
        {"row": 0, "col": 1, "colspan": 3,
         "text": "Year ended 31 December", "bbox": [148.0, 420.0, 400.9, 444.9],
         "header": true}
      ]
    }
  ]
}
```

Per cell: `row`/`col` are the **0-based top-left** position, `rowspan`/`colspan`
default to `1`, `bbox` is `[x0, y0, x1, y1]`, and `header` marks a header cell.
`bbox` is optional — omit it and every metric still works except cell-alignment
F1, which is reported as `null` rather than as a zero.

The loader also accepts the FinTabNet-native spelling, where a cell lists every
index it occupies instead of a span (`"row_nums": [0, 1], "column_nums": [0]`).
Use whichever you prefer; they load identically.

**Coordinates** are PDF points with the **origin at the top-left and y growing
downward** — the same space `pdfspine.Page.find_tables()` returns bboxes in, and
the same one FinTabNet.c uses. No flipping. A page is typically 612 x 792.

### `<name>.gold.html` — the quick one

If you would rather mark structure up as a table than fill in indices, write
plain HTML and let the loader derive the cells:

```html
<table>
  <tr><th rowspan="2">(in millions)</th><th colspan="3">Year ended 31 December</th></tr>
  <tr><th>2024</th><th>2023</th><th>2022</th></tr>
  <tr><td>Gross written premium</td><td>512,308</td><td>486,110</td><td>470,925</td></tr>
</table>
```

`colspan`/`rowspan` are honoured, `<th>` marks headers, multiple `<table>`
elements in one file become multiple tables on the page. There is no geometry,
so cell-alignment F1 is skipped for these; GriTS and TEDS-Struct still score.
Optionally attach geometry with `data-bbox="x0 y0 x1 y1"` on a cell.

Start from `annotate_template.json` if you want a blank to fill in.

## What the score means

`eval_tables.py run` reports three numbers per backend, and they disagree on
purpose — each catches a different failure:

- **GriTS_Top / GriTS_Con** (`conformance/gt/grits.py`) — the canonical
  FinTabNet metric, comparable with published Table-Transformer results. Top
  scores cell topology, Con scores cell content, both with partial credit.
- **TEDS-Struct** (`conformance/gt/table_metrics.py`) — tree edit distance over
  the `<table>/<tr>/<td colspan rowspan>` tree, text ignored. This is the number
  that moves when spans are wrong, and it is the one comparable with the Docling
  and PubTabNet literature.
- **Cell-alignment F1** — predicted cells matched one-to-one against gold cells
  at IoU >= 0.5. The bluntest and most legible: "what fraction of cells landed
  in the right place on the page".

A backend that raises GriTS while dropping cell F1 has learned to guess grids it
cannot actually locate. Watch all three.

Two run modes, and the difference matters:

- `--mode e2e` — the whole pipeline, detection included. This is what a user
  experiences, and a missed table scores 0.
- `--mode gold-crop` — each gold bbox is handed to the structure stage directly,
  so detection is taken out of the picture. This is the only mode whose numbers
  are comparable with the ~0.98 GriTS that Table-Transformer publishes, and it
  is required by the decision gate in
  [`docs/adr/0002-table-structure-backends.md`](../../../docs/adr/0002-table-structure-backends.md).

Track a change by scoring before and after:

```sh
$P conformance/gt/eval_tables.py run --backend onnx --out /tmp/before.json
# ... make the change ...
$P conformance/gt/eval_tables.py run --backend onnx --baseline /tmp/before.json
```

The delta table it prints is the acceptance criterion. Per the ADR, a new
structure model becomes the default only if gold-crop GriTS_Con beats the
incumbent by at least +0.02.
