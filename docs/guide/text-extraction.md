# Text extraction

oxide-pdf implements PyMuPDF's full `get_text` family, page search, the reusable
`TextPage` handle, and table detection.

## `get_text` variants

`Page.get_text(option="text", *, clip=None, flags=None, textpage=None, sort=False)`
returns a different native object depending on `option`:

| `option` | Returns | Description |
|---|---|---|
| `"text"` | `str` | Plain text in reading order (the default). |
| `"words"` | `list[tuple]` | One tuple per word, with its bounding box. |
| `"blocks"` | `list[tuple]` | One tuple per text block, with its bounding box. |
| `"dict"` | `dict` | Nested `blocks → lines → spans` structure. |
| `"rawdict"` | `dict` | Like `dict`, but down to per-character detail. |
| `"json"` | `str` | The `dict` structure serialized to JSON. |
| `"rawjson"` | `str` | The `rawdict` structure serialized to JSON. |
| `"html"` | `str` | HTML reconstruction of the page. |
| `"xhtml"` | `str` | XHTML reconstruction. |
| `"xml"` | `str` | Low-level XML with per-glyph geometry. |

```python
page = doc[0]

text = page.get_text()                # "text"
words = page.get_text("words")
blocks = page.get_text("blocks")
data = page.get_text("dict")
html = page.get_text("html")
xml = page.get_text("xml")
```

### Options

- `clip` — a `Rect` (or 4-sequence) limiting extraction to a sub-region.
- `sort` — when `True`, orders blocks top-to-bottom, left-to-right by `(y, x)`.
- `flags` — PyMuPDF text-extraction flag bits.
- `textpage` — reuse a previously built [`TextPage`](#textpage) to avoid
  re-parsing the page.

```python
clip = oxide_pdf.Rect(0, 0, 300, 400)
snippet = page.get_text("text", clip=clip, sort=True)
```

There is also a document-level convenience:

```python
text = doc.get_page_text(0, "text", sort=True)
```

## Searching

`Page.search_for(needle, *, hit_max=0, quads=False, clip=None, flags=None, textpage=None)`
finds every occurrence of `needle` and returns its geometry:

```python
rects = page.search_for("Total")             # list[Rect]
quads = page.search_for("Total", quads=True)  # list[Quad] (handles rotation)

# Cap the number of hits and restrict to a region:
hits = page.search_for("Total", hit_max=5, clip=oxide_pdf.Rect(0, 0, 595, 200))
```

Returning `Quad` geometry is the right choice when text may be rotated or
skewed; the four corner points describe the exact glyph quadrilateral.

## TextPage

When you extract text *and* search the same page, build a `TextPage` once and
pass it back via `textpage=` to avoid re-parsing:

```python
tp = page.get_textpage()                  # optional: flags=, clip=

text = page.get_text("text", textpage=tp)
hits = page.search_for("invoice", textpage=tp)
```

`TextPage` also exposes PyMuPDF's direct extractors:

```python
tp.extractText()       # -> str
tp.extractWORDS()      # -> list[tuple]
tp.extractBLOCKS()     # -> list[tuple]
tp.extractDICT()       # -> dict
tp.extractRAWDICT()    # -> dict
tp.extractJSON()       # -> str
tp.rect                # -> Rect of the page
```

## Tables

`Page.find_tables(...)` detects tables and returns a `TableFinder`:

```python
finder = page.find_tables()               # strategy="lines" by default
print(len(finder), "tables")

for table in finder:                      # also: finder.tables, finder[i]
    print(table.bbox, table.row_count, "x", table.col_count)
    grid = table.extract()                # list[list[str | None]]
    md = table.to_markdown()              # GitHub-Flavored Markdown
    html = table.to_html()                # an HTML <table> string
```

### Strategy

`find_tables` accepts a `strategy` of `"lines"` (default), `"lines_strict"`, or
`"text"`, plus tuning knobs:

```python
finder = page.find_tables(
    strategy="lines",
    line_max_thickness=3.0,
    snap_tolerance=3.0,
    min_line_length=3.0,
)
```

PyMuPDF's `vertical_strategy` / `horizontal_strategy` keyword arguments are also
accepted (a single non-default value selects that strategy).

### Table attributes

| Member | Type | Description |
|---|---|---|
| `Table.bbox` | `Rect` | Bounding box of the table. |
| `Table.row_count` | `int` | Number of rows. |
| `Table.col_count` | `int` | Number of columns. |
| `Table.header` | `list` | Header row cell text (or `[]`). |
| `Table.rows` | `list[float]` | Snapped horizontal grid-line y positions. |
| `Table.cols` | `list[float]` | Snapped vertical grid-line x positions. |
| `Table.cells` | `list[list[Rect | None]]` | Per-slot cell rects (row-major). |
| `Table.spans` | `list[tuple]` | `(row, col, row_span, col_span, Rect)` per merged cell. |
| `Table.extract()` | `list[list]` | Cell-text grid (`None` for empty/continuation). |
| `Table.to_markdown()` | `str` | Markdown rendering. |
| `Table.to_html()` | `str` | HTML rendering. |

## Page inventory

```python
fonts = page.get_fonts()        # list of font tuples
images = page.get_images()      # list of image tuples
drawings = page.get_drawings()  # vector drawings (geometry as Point/Rect)
```
