# Page

`oxide_pdf.Page` (PyMuPDF `fitz.Page`) is one page of a `Document`. Obtain one
with `doc[i]` or `doc.load_page(i)`.

## Geometry

| Member | Returns | Description |
|---|---|---|
| `number` | `int` | Zero-based page index. |
| `rect` | `Rect` | Page bound (`CropBox ∩ MediaBox`). |
| `bound()` | `Rect` | Alias for `rect`. |
| `mediabox` | `Rect` | Effective `/MediaBox`. |
| `cropbox` | `Rect` | Effective `/CropBox`. |
| `rotation` | `int` | Normalized rotation ∈ {0, 90, 180, 270}. |
| `set_rotation(rotation)` | `None` | Set the page rotation. |
| `is_image_only` | `bool` | Whether the page is image-only. |

## Text extraction

### `get_text(option="text", *, clip=None, flags=None, textpage=None, sort=False)`

Returns a native object per `option`:

| `option` | Returns |
|---|---|
| `"text"` | `str` |
| `"words"` | `list[tuple]` |
| `"blocks"` | `list[tuple]` |
| `"dict"` / `"rawdict"` | `dict` |
| `"json"` / `"rawjson"` | `str` |
| `"html"` / `"xhtml"` / `"xml"` | `str` |

### `get_textpage(flags=None, clip=None) -> TextPage`

Build a reusable `TextPage`. Pass it back via `textpage=` to `get_text` /
`search_for` to avoid re-parsing.

### `search_for(needle, *, hit_max=0, quads=False, clip=None, flags=None, textpage=None)`

Returns `list[Rect]` (default) or `list[Quad]` (`quads=True`), one per hit.

## Inventory

| Member | Returns | Description |
|---|---|---|
| `get_fonts(full=False)` | `list[tuple]` | Page fonts. |
| `get_images(full=False)` | `list[tuple]` | Page images. |
| `get_drawings()` | `list[dict]` | Vector drawings (geometry as `Point`/`Rect`). |
| `get_cdrawings()` | `list[dict]` | Vector drawings as raw tuples (faster). |

## Rendering

| Member | Returns | Description |
|---|---|---|
| `get_pixmap(*, matrix=None, dpi=None, colorspace=None, alpha=False, clip=None)` | `Pixmap` | Rasterize the page. |
| `get_displaylist()` | `DisplayList` | Record drawcalls for replay. |
| `get_svg_image(matrix=None, *, text_as_path=True)` | `str` | Standalone SVG document string. |

See [Pixmap](pixmap.md) for details.

## Tables

### `find_tables(*, strategy="lines", line_max_thickness=3.0, snap_tolerance=3.0, min_line_length=3.0, clip=None) -> TableFinder`

`strategy` is `"lines"`, `"lines_strict"`, or `"text"`. Returns a `TableFinder`
(iterable; `.tables` is the list, `len()` the count). See
[Text extraction → Tables](../guide/text-extraction.md#tables).

## Links

| Member | Returns | Description |
|---|---|---|
| `get_links()` | `list[dict]` | Link annotations (each `from` is a `Rect`). |
| `insert_link(link)` | `None` | Insert a goto/uri link. |
| `delete_link(link)` | `None` | Delete a link by its xref. |
| `get_label()` | `str` | The page's label under `/PageLabels`. |

## Content insertion

| Member | Returns | Description |
|---|---|---|
| `insert_text(point, text, *, fontname="helv", fontsize=11.0, color=(0,0,0), fontfile=None)` | `int` | Write text; returns lines written. |
| `insert_textbox(rect, text, *, fontname="helv", fontsize=11.0, color=(0,0,0), align=0, fontfile=None)` | `float` | Fill a rect with wrapped text; returns remaining space. |
| `insert_image(rect, *, stream=None, filename=None, width=0, height=0)` | `None` | Place an image (`pixmap=` not yet supported). |

## Vector drawing

| Member | Description |
|---|---|
| `draw_line(p1, p2, *, color=(0,0,0), width=1.0)` | Line segment. |
| `draw_rect(rect, *, color=(0,0,0), fill=None, width=1.0)` | Rectangle. |
| `draw_circle(center, radius, *, color=(0,0,0), fill=None, width=1.0)` | Circle. |
| `draw_oval(rect, *, color=(0,0,0), fill=None, width=1.0)` | Ellipse. |
| `draw_bezier(p1, p2, p3, p4, *, color=(0,0,0), width=1.0)` | Cubic Bézier. |
| `draw_polyline(points, *, color=(0,0,0), width=1.0)` | Connected polyline. |
| `new_shape()` | A reusable `Shape` (draw → `finish` → `commit`). |

## Annotations

Each `add_*` returns an `Annot`:

| Member | Description |
|---|---|
| `add_text_annot(point, text, *, icon="Note")` | Sticky note. |
| `add_freetext_annot(rect, text, *, fontsize=11.0, text_color=(0,0,0), fill_color=None, align=0)` | Free text. |
| `add_highlight_annot(quads)` | Highlight. |
| `add_underline_annot(quads)` | Underline. |
| `add_strikeout_annot(quads)` | Strike-out. |
| `add_squiggly_annot(quads)` | Squiggly underline. |
| `add_rect_annot(rect, *, color=(0,0,0), fill=None)` | Rectangle. |
| `add_circle_annot(rect, *, color=(0,0,0), fill=None)` | Circle/ellipse. |
| `add_line_annot(p1, p2, *, color=(0,0,0))` | Line. |
| `add_polygon_annot(points, *, color=(0,0,0), fill=None)` | Polygon. |
| `add_polyline_annot(points, *, color=(0,0,0))` | Polyline. |
| `add_ink_annot(handwriting, *, color=(0,0,0))` | Free-hand ink (list of strokes). |
| `add_stamp_annot(rect, *, stamp="Approved")` | Rubber stamp. |
| `add_file_annot(point, buffer, filename, ...)` | File attachment. |
| `add_redact_annot(quad, *, text=None, fill=None)` | Redaction mark. |

| Member | Returns | Description |
|---|---|---|
| `annots(types=None)` | iterator of `Annot` | Iterate annotations (optionally filtered by type int). |
| `first_annot` | `Annot \| None` | The first annotation. |
| `annot_xrefs()` | `list[int]` | Annotation xrefs. |
| `annot_names()` | `list[str]` | Annotation `/NM` names. |
| `delete_annot(annot)` | `None` | Delete an annotation (`Annot` or xref int). |
| `apply_redactions(...)` | `int` | Apply pending redactions; returns count. |

### Annot members

`rect`, `type` (`(int, str)`), `xref`, `info`, `colors`, `opacity`, `flags`,
`border`, `vertices`, `has_appearance`, `has_ap()`, and setters
`set_rect`, `set_colors`, `set_opacity`, `set_border`, `set_flags`, `set_info`,
`update()`.

## Forms

| Member | Returns | Description |
|---|---|---|
| `widgets()` | `list[Widget]` | Form-field widgets. |
| `first_widget` | `Widget \| None` | The first widget. |

### Widget members

`rect`, `xref`, `field_type`, `field_type_string`, `field_name`, `field_label`,
`field_value` (read/write), `field_flags`, `choice_values`, `button_states`, and
`update(value=None)`.

## Not yet implemented

Page-level `get_text_words` / `get_text_blocks` / `get_textbox`,
`get_image_info` / `get_image_bbox`, `show_pdf_page`, `write_text`,
`insert_font`, `replace_image`, `delete_image` are deferred and raise
`PdfUnsupportedError`. `insert_htmlbox` is out of scope.
