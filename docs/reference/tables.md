# Tables

`Page.find_tables(...)` returns a `TableFinder` (iterable; `.tables` is the list
of detected `Table`s).

## Vision / Table Transformer

For borderless or visually complex tables, install the optional runtime and use
Microsoft Table Transformer directly:

```bash
pip install "pdfspine[tatr]"
hf download microsoft/table-transformer-detection \
  --revision 34669b5e93083671f6ccd7aca07d615a79772286
hf download microsoft/table-transformer-structure-recognition-v1.1-all \
  --revision 7587a7ef111d9dcbf8ac695f1376ab7014340a0c
```

```python
finder = page.find_tables(strategy="vision", backend="tatr")
for table in finder:
    print(table.confidence, table.extract())
```

The detector proposes tables on the rendered page and the v1.1-all structure
model predicts rows, columns, headers, and spanning cells for each recognition
crop. By default, `native_line_guidance=True` lets a matching table outline from
pdfspine's native vector-line detector enlarge that crop before structure
recognition. This evidence fusion helps when TATR's detector tightly covers the
text but clips an empty outer column; it does not replace the structure model,
and metadata retains `detection_bbox`, `recognition_crop_bbox`, and
`geometry_source`. If no line outline matches, `adaptive_crop=True` can add up to
two rounds of model-driven context when TATR structure objects touch a crop
edge. Set `native_line_guidance=False` for a pure TATR detector-to-structure
pipeline; also set `adaptive_crop=False` to force a single recognition crop.

pdfspine then runs Microsoft's canonical structure post-processing and maps the
page's original PDF words into those cells. The model does not regenerate
financial values; a native value such as `1,234.50` remains that exact token.
When the whole page has no text layer and `ocr_if_no_text=True` (the default),
pdfspine attempts its built-in OCR; if OCR is unavailable, `Table.text_source`
is `none`. A mixed page with any native words does not trigger the page-level
OCR fallback.

Checkpoints are pinned and runtime loading is offline by default. To explicitly
let Hugging Face populate the cache on first use, pass
`vision_options={"local_files_only": False}`. You can instead set
`PDFSPINE_TATR_MODELS` to a directory containing `detection/` and
`structure-recognition-v1.1-all/`. The two weights total roughly 230 MB and are
never included in the base wheel.

The optional runtime supports CPython 3.12–3.14 on glibc/manylinux Linux
(x86_64/aarch64), Apple-silicon macOS, and x86_64 Windows. Intel macOS,
musl/Alpine, Windows ARM64, and Python 3.15+ are not supported by this extra; the
base package remains portable. See [Installation](../guide/installation.md#optional-table-transformer-backend)
for the dependency and Linux install-size details.

Useful `vision_options` include `dpi` (default 144), `device` (`auto`, `cpu`,
`cuda`, or `mps`), detection/structure thresholds, model paths/revisions,
`crop_padding` (Microsoft's default 10 pixels), `native_line_guidance`,
`adaptive_crop`, and `ocr_if_no_text`.
When `clip=` is supplied, vision still performs one full-page detection pass and
returns only tables intersecting that page-space rectangle.

## Vision / ONNX (DocLayout-YOLO + SLANet-plus)

A torch-free alternative to TATR that also exposes page layout. DocLayout-YOLO
detects layout regions (titles, paragraphs, figures, captions, tables, ...) on
the rendered page; SLANet-plus predicts the cell structure of each `table`
region as HTML tokens with `rowspan` / `colspan`; the page's text-layer words
are then assigned to those cells by geometry. Both networks run through
onnxruntime. As with TATR, the models only decide *where* things are: no
character is ever OCR-ed or regenerated, and pdfspine's native word coordinates
are the only source of text. See the [Layout HTML guide](../guide/layout-html.md)
for the end-to-end picture, the reading-order rule, and the current limitations.

```bash
pip install "pdfspine[onnx]"          # onnxruntime, numpy, Pillow
mkdir -p ~/models/pdfspine-onnx
curl -L -o ~/models/pdfspine-onnx/doclayout_yolo_docstructbench_imgsz1024.onnx \
  https://www.modelscope.cn/models/RapidAI/RapidLayout/resolve/v1.2.0/onnx/doclayout/doclayout_yolo_docstructbench_imgsz1024.onnx
curl -L -o ~/models/pdfspine-onnx/slanet-plus.onnx \
  https://www.modelscope.cn/models/RapidAI/RapidTable/resolve/v2.0.0/slanet-plus.onnx
export PDFSPINE_ONNX_MODELS=~/models/pdfspine-onnx
```

(`wget -O <file> <url>` works the same way.) Point `PDFSPINE_ONNX_MODELS` at
the directory holding both files, or pass explicit paths through
`vision_options={"layout_model": ..., "table_model": ...}`. The weights are
never part of the wheel; a missing file raises `PdfUnsupportedError` carrying
the download URL, and a missing runtime raises it with the `pdfspine[onnx]`
hint.

```python
finder = page.find_tables(strategy="vision", backend="onnx")
for table in finder:
    print(table.confidence, table.extract())   # exact text-layer strings
    html = table.to_html()                      # rowspan / colspan preserved
assert all(table.source == "onnx" for table in finder)

blocks = page.find_layout()        # list[LayoutBlock], in reading order
html = page.get_layout_html()      # semantic HTML for the whole page
```

`find_tables(..., backend="onnx")` returns the standard `TableFinder` / `Table`
objects (`extract()`, `to_html()`, `spans`, `cells`, `metadata`);
`backend=None` still selects TATR. `Page.find_layout(**vision_options)` returns
[`LayoutBlock`](#layoutblock)s (page points, DocLayout-YOLO label, score) and
`Page.get_layout_html(**vision_options)` turns the same blocks into HTML
(`<h2>`, `<p>`, `<table>`, `<figure>` placeholders, ...).

**Licence.** Both models are Apache-2.0 exports published by RapidAI:
DocLayout-YOLO (opendatalab, DocStructBench weights, from RapidLayout v1.2.0)
and SLANet-plus (PaddleOCR, from RapidTable v2.0.0). pdfspine's pre/post-
processing follows the `rapid_layout` / `rapid_table` reference code. Note that
the DocLayout-YOLO ONNX file's embedded metadata still carries the Ultralytics
exporter's default `license: AGPL-3.0` stamp (DocLayout-YOLO builds on the
YOLOv10 code base); check that the upstream licence position suits your use
before shipping a product on it.

**Execution providers.** `providers="auto"` (the default) uses
`CUDAExecutionProvider` when the installed onnxruntime offers it (install
`onnxruntime-gpu` instead of `onnxruntime`) and `CPUExecutionProvider`
otherwise. Pass an explicit sequence such as `("CPUExecutionProvider",)` to
force one; an unavailable provider raises `PdfUnsupportedError`. `auto` never
selects `CoreMLExecutionProvider`: with onnxruntime 1.29 on macOS it aborted
the process while building the execution plan, so Apple-silicon machines run
on CPU.

Useful `vision_options` (see [`OnnxOptions`](#onnxoptions) for the full list):
`dpi` (144), `layout_size` (1024, a multiple of 32), `layout_threshold` (0.25),
`layout_nms_iou` (0.7, class-agnostic NMS; `None` disables it), `table_size`
(488), `table_min_score` (0.0; drops tables whose mean structure-token score is
lower), `channel_order` (`"bgr"`, the PaddleOCR convention; `"rgb"` is
available), `crop_padding` (10 pixels), `layout_model` / `table_model`
(explicit paths), `providers`, and the same `ocr_if_no_text` / `ocr_engine` /
`ocr_language` page-level OCR fallback as TATR (it triggers only when the page
has no text layer at all).

SLANet-plus predicts cells only, so `Table.rows` / `Table.cols` are
approximations derived from the union of the cell boxes (single-span cells per
index, falling back to any cell touching it); `cells`, `spans`, `extract()` and
`to_html()` do not depend on them. When `clip=` is supplied, one full-page
layout pass still runs and only tables intersecting that page-space rectangle
are returned, as with TATR.

## TableFinder

::: pdfspine.TableFinder

## Table

::: pdfspine.Table

## ImageTable

`Page.find_image_tables(...)` (a pdfspine extra for scanned / image-only pages)
returns a list of `ImageTable`s made of `ImageTableCell`s.

::: pdfspine.ImageTable

## ImageTableCell

::: pdfspine.ImageTableCell

## OnnxOptions

Validated options for the ONNX backend; pass them as `vision_options={...}`
to `find_tables(strategy="vision", backend="onnx")` or as keyword arguments
to `find_layout()` / `get_layout_html()`.

::: pdfspine.OnnxOptions

## LayoutBlock

::: pdfspine.LayoutBlock
