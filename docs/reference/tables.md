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
