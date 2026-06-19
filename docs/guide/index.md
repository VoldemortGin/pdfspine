# Guide overview

This guide walks through using pdfspine for real work, from installation to the
individual feature areas. Every code block uses the **actual** public API.

## Two import styles

pdfspine ships two equivalent entry points:

```python
import pdfspine               # the native, idiomatic package
# or, the opt-in PyMuPDF compatibility shim:
import pdfspine.fitz as fitz  # no global-name collision
```

Both expose the same `open()`, `Document`, `Page`, `Pixmap`, and geometry
classes. Use `import pdfspine` for new code; for existing PyMuPDF code, the shim
is opt-in (a default install does not claim the global `fitz` / `pymupdf` names,
so it coexists with a real PyMuPDF). To make an unmodified `import fitz` resolve
to the shim, call `pdfspine.install_fitz_shim()` once at startup — see
[Migrating from PyMuPDF](migrating-from-pymupdf.md).

## Where to go next

| Page | What it covers |
|---|---|
| [Installation](installation.md) | Building and installing the wheel (not yet on PyPI). |
| [Quickstart](quickstart.md) | Open, extract, search, render, and save. |
| [Text extraction](text-extraction.md) | `get_text` variants, `search_for`, `TextPage`, tables. |
| [Editing & saving](editing.md) | Merge / split, metadata, TOC, annotations, forms, redaction. |
| [Rendering](rendering.md) | `get_pixmap`, `Pixmap`, `DisplayList`, SVG. |
| [Command-line interface](cli.md) | The planned `pdfspine` CLI. |
| [Migrating from PyMuPDF](migrating-from-pymupdf.md) | Compatibility mapping and gaps. |
| [License](license.md) | Apache-2.0, clean-room note, dependency licenses. |

!!! note "Alpha status"
    pdfspine is pre-1.0 and under active development. The implemented surface is
    substantial but accuracy validation against a real PDF corpus is ongoing.
    Methods that are not yet implemented raise `pdfspine.PdfUnsupportedError`
    with a hint, never a bare `AttributeError`.
