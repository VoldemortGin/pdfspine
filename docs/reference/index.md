# API reference

This reference documents the **actual** public API as implemented today. Methods
that exist but raise `PdfUnsupportedError` are marked; anything not listed here
is either deferred or out of scope (see
[Migrating from PyMuPDF](../guide/migrating-from-pymupdf.md)).

## Top-level (`oxide_pdf`)

```python
import oxide_pdf
```

| Name | Kind | Description |
|---|---|---|
| `open(filename=None, *, stream=None, filetype=None)` | function | Open a document from a path or bytes; no args → new empty PDF. |
| `version` | function | The Rust core version tuple. |
| `__version__` | str | The package version string. |
| `identity_matrix` | function | The identity matrix factory. |
| `Document` | class | A parsed document — see [Document](document.md). |
| `Page` | class | One page — see [Page](page.md). |
| `Pixmap` | class | A raster buffer — see [Pixmap](pixmap.md). |
| `DisplayList` | class | A replayable render — see [Pixmap](pixmap.md). |
| `TextPage` | class | A reusable text-extraction handle. |
| `Annot` | class | A page annotation. |
| `Widget` | class | An AcroForm field widget. |
| `Shape` | class | A reusable drawing canvas for a page. |
| `Table`, `TableFinder` | classes | Detected tables on a page. |
| `Rect`, `IRect`, `Point`, `Matrix`, `Quad` | classes | Geometry — see [Geometry](geometry.md). |

### Constants

`PDF_ENCRYPT_NONE`, `PDF_ENCRYPT_RC4_128`, `PDF_ENCRYPT_AES_128`,
`PDF_ENCRYPT_AES_256`.

### Exceptions

All inherit from `PdfError`:

| Exception | Raised when |
|---|---|
| `PdfError` | Base of the hierarchy. |
| `PdfSyntaxError` | Malformed / corrupt PDF (PyMuPDF `FileDataError`). |
| `PdfPasswordError` | Wrong or missing password. |
| `PdfUnsupportedError` | A known method is not yet implemented / out of scope. |
| `PdfDecodeError` | A stream filter failed to decode. |
| `PdfLimitError` | A safety limit was exceeded. |
| `PdfRedactionError` | A redaction operation failed. |

## Pages in this reference

- [Document](document.md) — open, pages, text, save, edit, metadata, TOC, forms.
- [Page](page.md) — geometry, text, search, render, annotations, drawing.
- [Pixmap](pixmap.md) — raster buffer, `DisplayList`.
- [Geometry](geometry.md) — `Point`, `Rect`, `IRect`, `Matrix`, `Quad`.
