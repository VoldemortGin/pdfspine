# Quickstart

This page covers the common workflow end to end: open a PDF, read its text,
search it, render a page to PNG, and save. Every snippet uses the real API.

## Open a document

```python
import oxide_pdf

# From a file path:
doc = oxide_pdf.open("input.pdf")

# Or from in-memory bytes:
with open("input.pdf", "rb") as fh:
    doc = oxide_pdf.open(stream=fh.read())

# Or create a new, empty PDF (no arguments):
new_doc = oxide_pdf.open()
```

`Document` is a context manager and is iterable / indexable like a sequence of
pages:

```python
with oxide_pdf.open("input.pdf") as doc:
    print(f"{doc.page_count} pages, is_pdf={doc.is_pdf}")
    for page in doc:
        print(page.number, page.rect)
    first = doc[0]          # same as doc.load_page(0)
    last = doc[-1]          # negative indexing supported
```

## Extract text

```python
page = doc[0]

plain = page.get_text()              # the default: "text"
words = page.get_text("words")       # list of word tuples with bboxes
blocks = page.get_text("blocks")     # list of block tuples
as_dict = page.get_text("dict")      # nested blocks/lines/spans dict
as_json = page.get_text("json")      # the dict, serialized to a JSON string
as_html = page.get_text("html")      # HTML reconstruction
```

See [Text extraction](text-extraction.md) for every variant and `TextPage`
reuse.

## Search

`search_for` returns the geometry of each hit — `Rect` by default, or `Quad`
when you pass `quads=True` (useful for rotated text):

```python
rects = page.search_for("invoice")           # list[Rect]
quads = page.search_for("total", quads=True)  # list[Quad]

for r in rects:
    print(r)                                  # Rect(x0, y0, x1, y1)
```

## Render to PNG

`get_pixmap` rasterizes a page — text, vector fills and strokes, images, clips,
and shadings. Control resolution with `dpi=` or a `Matrix`:

```python
import oxide_pdf

# 150 DPI render.
pix = page.get_pixmap(dpi=150)
pix.save("page-0.png")

# Equivalent scale via a Matrix (2x = ~144 DPI):
mat = oxide_pdf.Matrix(2, 2)
pix = page.get_pixmap(matrix=mat)

# Grayscale, with an alpha channel:
pix = page.get_pixmap(dpi=150, colorspace="gray", alpha=True)
```

A `Pixmap` carries `width`, `height`, `n`, `stride`, `samples`, and supports the
buffer protocol for zero-copy access — see [Rendering](rendering.md).

## Save

```python
# Full save with garbage collection (0-4) and stream compression.
doc.save("output.pdf", garbage=3, deflate=True)

# The convenience preset (garbage=3, deflate=True):
doc.ez_save("output.pdf")

# Incremental save (append a new revision to the same file):
doc.save("output.pdf", incremental=True)

# Serialize to bytes instead of a file:
data = doc.tobytes(garbage=3, deflate=True)   # alias: doc.write(...)
```

## Close

```python
doc.close()                # or use `with oxide_pdf.open(...) as doc:`
```
