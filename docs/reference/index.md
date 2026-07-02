# API reference

This reference documents the **complete public API** of `pdfspine` — every name
exported from the top-level package (`pdfspine.__all__`). Each class, function
and constant is rendered directly from its docstring, so the reference never
drifts from the implementation.

```python
import pdfspine
```

Methods that exist but raise `PdfUnsupportedError` are documented as such in
their docstrings; see [Migrating from PyMuPDF](../guide/migrating-from-pymupdf.md)
for the deferred / out-of-scope surface.

## How this reference is organised

| Page | Covers |
|---|---|
| [Documents & pages](document.md) | `Document`, `Page` |
| [Text extraction](textpage.md) | `TextPage` |
| [Rendering](pixmap.md) | `Pixmap`, `DisplayList` |
| [Annotations & forms](annotations.md) | `Annot`, `Widget` |
| [Drawing & text](drawing.md) | `Shape`, `Font`, `TextWriter` |
| [Tables](tables.md) | `Table`, `TableFinder`, `ImageTable`, `ImageTableCell` |
| [Navigation](navigation.md) | `Link`, `linkDest`, `Outline` |
| [Color](color.md) | `Colorspace` |
| [Tools](tools.md) | `Tools` |
| [Geometry](geometry.md) | `Rect`, `IRect`, `Point`, `Matrix`, `Quad` |
| [Functions](functions.md) | `open`, `markdown_to_pdf`, `install_fitz_shim`, `identity_matrix`, helpers |
| [Exceptions](exceptions.md) | `PdfError` hierarchy |
| [Constants](constants.md) | `PDF_*` / `TEXT_*` / `STAMP_*` values, colorspace singletons |
