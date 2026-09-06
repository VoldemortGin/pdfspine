# Text extraction

pdfspine implements PyMuPDF's full `get_text` family, page search, the reusable
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
| `"layout"` | `str` | Layout-preserving text: lines regrouped with a y tolerance, columns kept as space padding (pdfspine extension). |

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
clip = pdfspine.Rect(0, 0, 300, 400)
snippet = page.get_text("text", clip=clip, sort=True)
```

There is also a document-level convenience:

```python
text = doc.get_page_text(0, "text", sort=True)
```

To export every page as one browser-openable HTML5 document:

```python
html = doc.to_html()
doc.save_html("document.html")  # UTF-8; accepts str and os.PathLike paths
```

The document title comes from PDF metadata when available, then the input
filename, and otherwise defaults to `PDF document`. Page HTML fragments are
preserved in document order.

### Layout-preserving text

`get_text("layout")` — and its tunable form `get_text_layout(...)` — returns
plain text that keeps the page's visual layout, `pdftotext -layout` style. The
words from `get_text("words")` are regrouped into visual lines: a word joins a
line when its vertical center lies within `y_tolerance` points (default 3) of
the line's *anchor* — the center of the line's first word — so sub-point
baseline jitter cannot chain one line into the next. Lines run top-to-bottom,
words left-to-right, and every word is placed on a character grid whose cell is
`char_width` points (default: the median glyph width of the page), so columns
stay aligned as space padding. Vertical gaps wider than a normal line pitch
become blank lines, and `clip` filters words by bbox intersection.

The motivation: `sort=True` orders text by exact `(y, x)`, so a run of words
whose baselines differ by a fraction of a point splits one visual line into
two; layout mode absorbs that jitter inside its `y_tolerance` band.

```python
text = page.get_text("layout")               # default 3 pt tolerance

# Tune the grouping band and the column grid:
text = page.get_text_layout(y_tolerance=2.0, char_width=5.0)
```

### Markdown export

`Page.to_markdown()`, `Document.to_markdown()` and `Document.save_markdown()`
render a page — or a whole document — as Markdown for RAG / LLM pipelines. This
is the PDF → Markdown direction; `markdown_to_pdf()` is the reverse.

```python
Page.to_markdown(*, clip=None, tables=True, table_strategy="lines",
                 heading_levels=3, heading_ratio=1.15, bold_headings=True,
                 emphasis=True, images=False) -> str
Document.to_markdown(pages=None, *, page_separator="\n\n-----\n\n",
                     <same keyword options as Page.to_markdown>) -> str
Document.save_markdown(path, pages=None, *, <same keyword options>) -> None
```

Reading order comes straight from `get_text("dict", sort=True)` — the engine's
column-aware order; nothing is re-ordered here. The renderer then classifies:

- **Headings** — font sizes are clustered to the nearest half point and the
  size carrying the most characters is the *body size*. Every distinct size at
  or above `body_size * heading_ratio` becomes a heading level, largest first,
  capped at `heading_levels` (deeper sizes share the last level). The
  document-level export computes this scale once over all selected pages, so a
  level means the same thing on every page. With `bold_headings`, a block whose
  leading lines are all bold at body size — at most two lines / fifteen words
  and not ending in `.`, `;` or `,` — becomes the next-deeper level.
  Candidates that read like paragraphs (over thirty words, several lines
  ending with a period, fewer than three alphanumeric characters) and
  dotted-leader lines (tables of contents) stay body text.
- **Paragraphs** — the lines of a block joined by spaces, soft hyphenation
  mended, one paragraph per block.
- **Lists** — bullet glyphs (`•` `◦` `▪` …), `-` and `*` become `- item`;
  `1.` / `1)` become `1. item`; letter / roman labels become `- (a) item`;
  wrapped continuation lines join their item and indentation nests up to four
  levels.
- **Tables** — with `tables=True`, every table found by
  `find_tables(strategy=table_strategy)` is rendered through
  `Table.to_markdown()` and replaces the text lines inside its bbox. Grids
  that are not tables — covering nearly the whole page, with fewer than two
  filled cells, or holding a cell of running prose — are ignored and their
  text stays in the flow. Pass `tables=False` to skip detection, or
  `table_strategy="text"` for unruled tables.
- **Inline styles** — with `emphasis`, bold spans become `**bold**`, italic
  `_italic_`, monospace `` `code` ``; a block set entirely in a monospace font
  becomes a fenced code block.
- **Images** — skipped unless `images=True`, which emits
  `![image](page-N-image-K.ext)` placeholders (nothing is written to disk).

`clip` restricts extraction to a rectangle — handy for cutting running headers
and footers. For the document form, `pages` selects and orders the pages and
`page_separator` (a horizontal rule by default) joins the non-empty fragments;
empty pages contribute nothing.

```python
md = page.to_markdown()                       # one page

md = doc.to_markdown()                         # every page, joined
md = doc.to_markdown(pages=[0, 2, 5])          # a subset, in that order
doc.save_markdown("out.md")                    # UTF-8, "\n" newlines
```

**Limitations**

- Unruled tables need `table_strategy="text"` (or the TATR vision backend); the
  default `"lines"` strategy only finds ruled tables.
- List bullets drawn as vector graphics (not glyphs) are not detected.
- Formulas and superscripts are emitted as plain text.
- Images are placeholders only — no files are written.
- Headings are heuristic (font size / bold) and can be tuned via
  `heading_levels`, `heading_ratio` and `bold_headings`.

### Glyph geometry

On top of PyMuPDF's key set, `"dict"`, `"rawdict"`, `"json"` and `"rawjson"`
publish the full rendering geometry of every span and character, so you never
have to reverse-engineer a font size from a bbox or repair a rotated run
yourself. Structured span `size` reports the rendered size; `declared_size`
preserves the original `Tf` operand. Other existing keys keep their meaning.

**Span keys** (`dict`, `rawdict`, `json`, `rawjson`):

| Key | Type | Space | Meaning |
|---|---|---|---|
| `size` | `float` | device | First glyph’s rendered size, equal to `rendered_size`. |
| `declared_size` | `float` | — | The `Tf` operand verbatim, independently preserved. |
| `rendered_size` | `float` | device | `sqrt(\|a·d − b·c\|)` of `matrix` — the size actually painted. |
| `matrix` | 6-tuple `(a, b, c, d, e, f)` | **device** | Render matrix of the span's first glyph (glyph cell → device space). |
| `text_matrix` | 6-tuple | **PDF user** | The first glyph's `Tm`, the raw content-stream value (no page transform). |
| `ctm` | 6-tuple | **PDF user** | The first glyph's CTM, likewise the raw user-space value. |
| `dir` | 2-tuple | device | Unit vector along the baseline. |
| `quad` | 8-tuple `(ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y)` | device | Directional envelope: the extent of the glyph quads along `dir` and its normal, rebuilt as four corners. Equals the `bbox` corners for upright text; hugs the run for rotated or sheared text. |
| `seq` | `int` | — | Painting order: the smallest source-glyph index in the span. |

**Char keys** (`rawdict` / `rawjson` only, in `span["chars"]`):

| Key | Type | Space | Meaning |
|---|---|---|---|
| `matrix` | 6-tuple | **device** | The glyph's render matrix. |
| `quad` | 8-tuple | device | The glyph cell's four true corners — a real parallelogram under rotation or shear; `bbox` is only its axis-aligned envelope. |
| `rendered_size` | `float` | — | `sqrt(\|det\|)` of `matrix`. |
| `seq` | `int` | — | Painting order: the glyph's index in the interpreter output. A synthesized space borrows the preceding real glyph's index, so `seq` is non-decreasing along a span. |
| `synthetic` | `bool` | — | `True` when layout synthesized this char (an inter-word space the PDF never painted, e.g. `TJ` kerning); a space the PDF really drew is `False`. |

**Line keys**: `number` (`int`, the line's reading-order index within the page,
dense `0..n-1`; concatenating lines sorted by `number` reproduces the line order
of `get_text("text")`) and `seq` (`int`, painting order: the smallest
source-glyph index in the line).

**Block keys**: `seq` (`int`, painting order: the smallest line `seq` in the
block). Only text blocks (`type == 0`) carry `seq`; image blocks do not. The
existing `number` (reading-order block index) is unchanged.

`get_text("xml")` now fills `<char quad="...">` with the true quad (value for
value the same as the `rawdict` char `quad`), in the order
`ul.x ul.y ur.x ur.y ll.x ll.y lr.x lr.y`; it used to carry the axis-aligned
corners derived from the bbox. This matches PyMuPDF 1.28.2, which also emits the
real parallelogram.

#### Three coordinate spaces

| Space | Definition | What lives here |
|---|---|---|
| **PDF user space** | y up, origin bottom-left; the space the content-stream operators work in. | `text_matrix`, `ctm`. Deliberately **not** multiplied by the page transform: their job is to map an extracted glyph back to the PDF source (which operators painted it), which is impossible once the page transform is folded in. |
| **pdfspine device space** | y down, origin top-left; page rotation applied. | The existing `bbox` / `origin` / `dir`, and the new `matrix` / `quad` — same frame as `bbox` / `origin`. |
| **Glyph cell (text space)** | The glyph's 1000-unit em space ÷ 1000: `[0, descender .. advance, ascender]` (shifted by the vertical displacement vector `−v` for vertical writing). | `matrix` is the matrix that maps this cell into device space. |

PyMuPDF 1.28.2's text XML keeps unrotated page coordinates when a page has
`/Rotate`, while pdfspine applies its page transform. This is a known coordinate-basis
difference for rotated pages; it does not change the `ul` / `ur` / `ll` / `lr`
corner convention described above.

Note the intentional asymmetry: **`matrix` / `quad` are device space,
`text_matrix` / `ctm` are user space.**

Matrices are 6-tuples `(a, b, c, d, e, f)` in the PDF / PyMuPDF **row-vector**
convention, `(x, y) → (a·x + c·y + e, b·x + d·y + f)`; `m1 * m2` applies `m1`
first, then `m2` (the same as `pdfspine.Matrix`).

#### Invariants

All three hold for spans and chars alike, and each is covered by a test:

1. `(0, 0) · matrix == origin`
2. the axis-aligned envelope of `quad` `== bbox`
3. `matrix == params · text_matrix · ctm · page_transform`, with
   `params = [Tfs·Th, 0, 0, Tfs, 0, Trise]` (`Tfs` = `declared_size`,
   `Th` = horizontal scaling `Tz`/100, `Trise` = `Ts`) and
   `page_transform = page.transformation_matrix` (`[1, 0, 0, -1, 0, 792]` for an
   unrotated 612×792 page).

#### Rendered and declared size

- `rendered_size = sqrt(|a·d − b·c|)` — MuPDF's `fz_matrix_expansion`, which is
  exactly what PyMuPDF reports as the `dict` / `rawdict` span `size`. Pure
  rotation and pure shear leave it unchanged; anisotropic scaling yields the
  geometric mean of the two axes (`Tm 20 0 0 10` → `sqrt(200) ≈ 14.142136`);
  `Tz 50` with `Tf 12` → `sqrt(72) ≈ 8.485281`.
- Degenerate input: a singular or non-finite matrix yields `rendered_size == 0.0`
  (no panic, no NaN / Inf); every published matrix and quad component is finite.
- In all four structured formats, `span["size"] == span["rendered_size"]`.
  `span["declared_size"]` retains the raw `Tf` operand, including its sign.
- Span size represents the **first glyph**, not an average or a guarantee that
  every glyph has that size. Geometry-compatible adjacent glyphs can retain
  small transform differences; cumulative drift within a span can differ from
  PyMuPDF's span grouping. For per-character size use `char["rendered_size"]`
  in `rawdict` / `rawjson`. The 300-document comparison improves size parity
  substantially but does not establish perfect span-by-span parity.
- HTML / XHTML / XML output and pdfspine's `get_texttrace()` retain their
  existing declared-size semantics; this structured-output change does not
  redefine them. PyMuPDF's **texttrace** size instead uses `|(a, b)|`, the x
  basis length, which differs from the determinant definition under `Tz` and
  anisotropic scaling. Do not substitute either trace API for structured size.

#### `seq` (painting order) vs `number` (reading order)

- `seq` is the content stream's painting order (source-glyph index) and exists
  on all four levels — block, line, span, char (the enclosing levels take the
  minimum). Use it to ask "which stroke painted this", for stable sorting, or to
  split a span back into source order.
- `number` is a reading-order index (block index at the block level, page line
  index at the line level).
- Known limitation of `number`: pdfspine's current inter-region ordering key is
  the content stream's painting order, not a purely geometric reading order.
  `number` therefore promises "the index in the order the engine actually emits,
  consistent with `get_text("text")`" — **not** "the ideal geometric reading
  order of the layout". For PDFs whose painting order is scrambled (for example
  interleaved columns), `number` is scrambled with it. Consumers that need a
  strict geometric reading order should sort by bbox themselves or pass
  `sort=True`.

#### Worked numbers

Unrotated 612×792 page (`page_transform = [1, 0, 0, -1, 0, 792]`), a font whose
every code is 500/1000 wide with ascent 800 and descent −200:

- `BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hi) Tj ET` — span: `declared_size == 1.0`,
  `size == rendered_size == 12.0`, `matrix == (12, 0, 0, -12, 100, 92)`,
  `text_matrix == (12, 0, 0, 12, 100, 700)`, `ctm == (1, 0, 0, 1, 0, 0)`,
  `dir == (1, 0)`, `origin == (100, 92)`, `seq == 0`. First char `"H"`:
  `matrix == (12, 0, 0, -12, 100, 92)`,
  `quad == (100, 82.4, 106, 82.4, 100, 94.4, 106, 94.4)`,
  `bbox == (100, 82.4, 106, 94.4)`, `rendered_size == 12.0`, `seq == 0`,
  `synthetic == False`.
- `2 0 0 2 0 0 cm BT /F1 12 Tf 50 350 Td (A) Tj ET` — `declared_size == 12.0`,
  `rendered_size == 24.0`, `ctm == (2, 0, 0, 2, 0, 0)`,
  `text_matrix == (1, 0, 0, 1, 50, 350)`.
- Shear `BT /F1 1 Tf 12 0 6 12 100 700 Tm (A) Tj ET` — the char `quad` is a real
  parallelogram: its left edge has `ll.x − ul.x == −6` (not 0), so the quad is
  not the `bbox` corners; `rendered_size` stays `12.0`.

#### Example

```python
import math
import pdfspine

page = pdfspine.open("in.pdf")[0]
span = page.get_text("rawdict")["blocks"][0]["lines"][0]["spans"][0]
ch = span["chars"][0]

# 1) (0,0) · matrix == origin
m = pdfspine.Matrix(*ch["matrix"])
p = pdfspine.Point(0, 0) * m
assert math.isclose(p.x, ch["origin"][0], abs_tol=1e-9)
assert math.isclose(p.y, ch["origin"][1], abs_tol=1e-9)

# 2) the envelope of quad == bbox
xs, ys = ch["quad"][0::2], ch["quad"][1::2]
env = (min(xs), min(ys), max(xs), max(ys))
assert all(math.isclose(a, b, abs_tol=1e-9) for a, b in zip(env, ch["bbox"]))

# 3) the real font size — no bbox reverse-engineering needed
a, b, c, d = ch["matrix"][:4]
assert math.isclose(ch["rendered_size"], math.sqrt(abs(a * d - b * c)))

# 4) matrix == params · Tm · CTM · page_transform (span level; Tz=100, Ts=0)
tfs = span["declared_size"]
params = pdfspine.Matrix(tfs, 0, 0, tfs, 0, 0)
tm, ctm = pdfspine.Matrix(*span["text_matrix"]), pdfspine.Matrix(*span["ctm"])
composed = params * tm * ctm * page.transformation_matrix
assert all(
    math.isclose(x, y, abs_tol=1e-9)
    for x, y in zip(composed, span["matrix"])
)
```

## Searching

`Page.search_for(needle, *, hit_max=0, quads=False, clip=None, flags=None, textpage=None)`
finds every occurrence of `needle` and returns its geometry:

```python
rects = page.search_for("Total")             # list[Rect]
quads = page.search_for("Total", quads=True)  # list[Quad] (handles rotation)

# Cap the number of hits and restrict to a region:
hits = page.search_for("Total", hit_max=5, clip=pdfspine.Rect(0, 0, 595, 200))
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

`find_tables` accepts native strategies `"lines"` (default), `"lines_strict"`,
and `"text"`, plus the optional TATR vision strategy:

```python
finder = page.find_tables(
    strategy="lines",
    line_max_thickness=3.0,
    snap_tolerance=3.0,
    min_line_length=3.0,
)

# Borderless / complex tables (requires pip install "pdfspine[tatr]"):
finder = page.find_tables(strategy="vision", backend="tatr")
```

TATR predicts regions and structure; cell strings come from pdfspine's native
word coordinates, with built-in OCR used only when the page has no text layer.

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
| `Table.confidence` | `float \| None` | Vision-model confidence; `None` for native strategies. |
| `Table.source` | `str` | Producing backend (`native` or `tatr`). |
| `Table.text_source` | `str` | `pdfspine-native`, `pdfspine-ocr`, or `none`. |
| `Table.metadata` | `dict` | Pinned model revisions, device, and preprocessing metadata. |
| `Table.extract()` | `list[list]` | Cell-text grid (`None` for empty/continuation). |
| `Table.to_markdown()` | `str` | Markdown rendering. |
| `Table.to_html()` | `str` | HTML rendering. |

## Page inventory

```python
fonts = page.get_fonts()        # list of font tuples
images = page.get_images()      # list of image tuples
drawings = page.get_drawings()  # vector drawings (geometry as Point/Rect)
```
