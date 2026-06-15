# Migrating from PyMuPDF

oxide-pdf is designed so that existing PyMuPDF code can run unmodified for the
supported subset. This page is the honest, no-marketing account of what works,
what differs, and what isn't there yet.

## `import fitz` just works

The compatibility shim maps PyMuPDF's exact names onto oxide-pdf. In most cases
the only change you need is… nothing:

```python
import fitz                       # resolves to the oxide-pdf shim

doc = fitz.open("input.pdf")
page = doc[0]
text = page.get_text()
pix = page.get_pixmap(dpi=150)
pix.save("out.png")
doc.save("out.pdf")
```

`import pymupdf` is supported the same way. For new code, prefer the native
package:

```python
import oxide_pdf
doc = oxide_pdf.open("input.pdf")
```

Both expose the identical `open`, `Document`, `Page`, `Pixmap`, `DisplayList`,
`TextPage`, `Annot`, `Widget`, `Shape`, `Table`, and geometry classes.

## Coverage at a glance

The baseline is **PyMuPDF 1.24.x**. The machine-readable `COMPAT.toml` in the
repository tracks the disposition of every public PyMuPDF symbol:

| Disposition | Count | What it means |
|---|---:|---|
| Implemented | 324 | Works today; does not raise on use. |
| Deferred | 361 | Known and planned for a later milestone. |
| Out-of-scope | 81 | Intentionally never in v1. |
| **Total** | **766** | **~42% implemented** |

!!! warning "Accuracy validation is in progress"
    "Implemented" means the method exists and returns a result of the right
    shape. **Byte-for-byte / pixel-for-pixel agreement with PyMuPDF across a real
    PDF corpus is still being validated.** Verify output on your own documents
    before relying on it.

## How gaps behave

Anything not yet implemented raises a typed, catchable
`oxide_pdf.PdfUnsupportedError` (aliased as `fitz.PdfUnsupportedError`) **with a
hint** — never a bare `AttributeError`. That means you can detect and handle gaps
cleanly:

```python
import oxide_pdf

try:
    doc.some_unimplemented_method()
except oxide_pdf.PdfUnsupportedError as e:
    print("not yet:", e)
```

PyMuPDF exception names are aliased onto the typed hierarchy, so existing
`except` clauses keep working:

| PyMuPDF name | oxide-pdf type |
|---|---|
| `fitz.FileDataError` | `PdfSyntaxError` |
| `fitz.EmptyFileError` | `PdfSyntaxError` |
| `fitz.FileNotFoundError` | built-in `FileNotFoundError` |
| `fitz.mupdf_display_errors` | `PdfError` |

## What is 100% compatible

The **geometry layer** (`Point`, `Rect`, `IRect`, `Matrix`, `Quad`) mirrors
PyMuPDF 1.24.x arithmetic exactly — operators, transforms, inversion,
`morph` / `torect`, quad convexity — as a documented contract. These classes are
also sequences, so `r[0]`, `tuple(r)`, and unpacking all behave like PyMuPDF.

## Feature mapping

| Area | PyMuPDF | oxide-pdf | Status |
|---|---|---|---|
| Open / pages | `fitz.open`, `doc[i]`, `page_count` | same | ✅ Implemented |
| Metadata | `doc.metadata`, `set_metadata` | same | ✅ Implemented |
| XMP | `get_xml_metadata` / `set_xml_metadata` | same | ✅ Implemented |
| Encryption (read) | `authenticate`, `needs_pass`, `permissions` | same | ✅ Implemented |
| Encryption (write) | `save(encryption=...)` | RC4 / AES-128 / AES-256 | ✅ Implemented |
| Text | `get_text("text"/"words"/"blocks"/"dict"/"rawdict"/"json"/"html"/"xhtml"/"xml")` | same | ✅ Implemented |
| Search | `search_for` (rects / quads) | same | ✅ Implemented |
| TextPage | `get_textpage`, `extract*` | same | ✅ Implemented |
| Tables | `find_tables`, `to_markdown` | + `to_html` | ✅ Implemented |
| Render | `get_pixmap` (DPI / matrix / clip / colorspace / alpha) | same | ✅ Implemented |
| DisplayList | `get_displaylist`, `get_pixmap` | same | ✅ Implemented |
| SVG | `get_svg_image` | same | ✅ Implemented |
| Pixmap | save / tobytes / samples / buffer protocol | same | ✅ Implemented |
| Save | `save`, `ez_save`, `tobytes`/`write`, `incremental=` | same | ✅ Implemented |
| Page ops | `new_page`, `delete_page`, `select` | same | ✅ Implemented |
| Merge | `insert_pdf` | same | ✅ Implemented |
| TOC | `get_toc`, `set_toc` | same | ✅ Implemented |
| Links | `get_links`, `insert_link`, `delete_link` | same | ✅ Implemented |
| Annotations | `add_*_annot`, `annots`, `delete_annot` | same | ✅ Implemented |
| Forms | `is_form_pdf`, `widgets`, `form_fill`, `form_flatten` | same | ✅ Implemented |
| Redaction | `add_redact_annot`, `apply_redactions` | same | ✅ Implemented |
| Sanitize | `scrub`, `bake` | same | ✅ Implemented (subset of toggles) |
| Embedded files | `embfile_*` | same | ✅ Implemented |
| OCG / layers | `get_ocgs`, `add_ocg`, `get_layer`, `set_layer`, `set_oc` | same | ✅ Implemented (read + add/toggle/bind) |
| xref read | `xref_object`, `xref_stream`, `xref_get_key`, … | same | ✅ Implemented |

## What differs

- **`scrub` toggles** — the full PyMuPDF toggle set is *accepted*, but only a
  subset (metadata, JavaScript, attached/embedded files, links, XMP) is acted
  on; the rest are no-ops.
- **`insert_image(pixmap=...)`** — not yet supported; pass `stream=` bytes or
  `filename=` instead.
- **Deprecated camelCase aliases** — PyMuPDF's old `getText` / `getPixmap` /
  `setMetadata` style names are provided as aliases where they existed, so legacy
  code keeps working.
- **`to_html()` on tables** — an oxide-pdf extra beyond PyMuPDF.

## What is not yet implemented

These are *deferred* (planned) — they raise `PdfUnsupportedError` today:

- `convert_to_pdf` / image-as-document inputs (milestone M5).
- Page-level word/block helpers (`get_text_words`, `get_text_blocks`,
  `get_textbox`), `get_texttrace` (M2 follow-ups).
- Image-info helpers (`get_image_info`, `get_image_bbox`, `get_image_rects`).
- `show_pdf_page`, `write_text`, `insert_font`, `replace_image`, `delete_image`.
- Page-label read/write, `copy_page` / `move_page` / `delete_pages`.

## Out of scope for v1

Intentionally **never** in v1 (these raise `PdfUnsupportedError`):

- **OCR** (`get_textpage_ocr`).
- **EPUB-class reflow** (`doc.layout`, chapters, locations, bookmarks).
- **HTML/CSS layout** (`insert_htmlbox`).
- **Journalling / undo-redo** (`journal_*`, `save_snapshot`).
- **Full Unicode shaping** (complex scripts).

Consult the repository's `COMPAT.toml` for the authoritative, per-symbol
disposition — it is CI-enforced to stay in sync with the code.
