# pdfspine

**A permissively-licensed, pure-Rust reimplementation of [PyMuPDF](https://pymupdf.readthedocs.io/) (`fitz`) — with a Python API, an `import fitz` compatibility shim, and an `pdfspine` command-line tool.**

pdfspine lets you read, search, extract, edit, and render PDFs from Python with
a PyMuPDF-shaped API — but the entire engine is written in safe Rust and shipped
under **Apache-2.0**. Existing code that does `import fitz` can run unmodified on
the supported subset via the opt-in shim (`pdfspine.install_fitz_shim()`, or
`import pdfspine.fitz as fitz`), while new code can use the idiomatic
`import pdfspine` package directly.

!!! warning "Alpha / work-in-progress"
    pdfspine is **pre-1.0** (`Development Status :: 2 - Pre-Alpha`, version
    `0.0.0`). A large surface of the API is implemented and tested
    (see the [PyMuPDF coverage](#status) below), but real-corpus accuracy
    validation against PyMuPDF is **still in progress**, and the package is
    **not yet published to PyPI**. APIs may change before 1.0. Treat output as
    "verify before you trust it" for production workloads.

## Why pdfspine?

PyMuPDF is excellent, but it is **AGPL-3.0** (or a paid commercial license from
Artifex). That licensing is a non-starter for many closed-source products, SaaS
backends, and permissively-licensed open-source projects.

pdfspine exists to be the **permissively-licensed, drop-in-shaped** alternative:

- **Apache-2.0 throughout.** Every first-party crate is Apache-2.0 — a permissive
  license with an explicit patent grant. The dependency graph is gated by
  `cargo-deny` to exclude GPL / AGPL / LGPL / MPL / SSPL from the shipped wheel.
  License cleanliness is a **CI-enforced, tested property**, not a promise.
- **Pure Rust, no C blob.** Self-contained wheels with no system `zlib` / C
  linkage and no bundled prebuilt engine — the differentiator versus
  pdfium-based wrappers.
- **Memory-safe by construction.** `#![forbid(unsafe_code)]` in all first-party
  crates except the single audited PyO3 FFI chokepoint.
- **`fitz` / `pymupdf`-compatible surface (opt-in).** A compatibility shim aims
  to let existing `import fitz` code run unchanged for the supported subset —
  `import pdfspine.fitz as fitz`, or `pdfspine.install_fitz_shim()` to claim the
  global names (collision-safe by default), with a machine-readable `COMPAT.toml`
  documenting every deviation.
- **Clean-room.** An independent reimplementation: no code, tests, or fixtures
  are derived from MuPDF / PyMuPDF or any AGPL source.

## Feature highlights

- **Open** PDFs from a path or in-memory bytes, including encrypted documents
  (RC4 / AES-128 / AES-256), with `authenticate()` and permission flags.
- **Text extraction** in every PyMuPDF variant: `text`, `words`, `blocks`,
  `dict`, `rawdict`, `json`, `html`, `xhtml`, `xml`.
- **Search** with `search_for`, returning `Rect` or `Quad` hit geometry.
- **Table detection** (`find_tables`) with `to_markdown()` / `to_html()` export.
- **Rendering** any page to a `Pixmap` via `get_pixmap` (text, vector fills,
  strokes, images, clips, shadings), plus `DisplayList` replay and SVG export.
- **Editing**: merge / split / insert pages, insert text / images, draw vectors,
  annotations, AcroForm forms, redaction, metadata, table-of-contents, links.
- **Save** full or **incremental**, with optional garbage collection,
  deflate compression, and encryption.
- **Zero-copy `Pixmap`** via the Python buffer protocol
  (`memoryview(pix)` / `numpy.frombuffer(pix)`).

## Status

The current baseline targets **PyMuPDF 1.24.x**. The machine-readable
`COMPAT.toml` tracks the disposition of every public PyMuPDF symbol:

| Disposition | Count | Meaning |
|---|---:|---|
| Implemented | 647 | Present and does not raise on use |
| Deferred | 56 | Known, planned for a later milestone |
| Out-of-scope | 66 | Intentionally never in v1 |
| **Total baseline** | **769** | **84.1% implemented** |

Anything not yet implemented raises a typed, catchable
`PdfUnsupportedError` (never a bare `AttributeError`), so you always get a clear
signal — see [Migrating from PyMuPDF](guide/migrating-from-pymupdf.md).

## Quick example

```python
import pdfspine  # or, opt-in shim: import pdfspine.fitz as fitz

doc = pdfspine.open("input.pdf")
print(f"{doc.page_count} pages")

page = doc[0]
text = page.get_text()                 # plain text
hits = page.search_for("invoice")      # list[Rect]

pix = page.get_pixmap(dpi=150)         # render to an image
pix.save("page-0.png")

doc.save("output.pdf", garbage=3, deflate=True)
doc.close()
```

Ready to go? Start with the [Installation](guide/installation.md) and
[Quickstart](guide/quickstart.md) guides.
