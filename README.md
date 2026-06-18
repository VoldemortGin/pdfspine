# pdfspine

**An Apache-2.0-licensed, pure-Rust reimplementation of [PyMuPDF](https://pymupdf.readthedocs.io/) (`fitz`), with PyO3 Python bindings.**

> **Status: alpha / pre-1.0, but the core is feature-complete.** pdfspine can
> already parse/repair/decrypt PDFs, extract text & tables, search, edit / merge /
> split / save (incl. byte-exact incremental), encrypt, annotate, fill & flatten
> forms, redact (destructively), open image files as documents, and **render
> pages to images**. ~**42%** of the PyMuPDF 1.24 public API is implemented and
> tested (climbing), with **1100+ Rust tests + 260+ Python tests** green.
> Real-world accuracy validation is in progress (see [Accuracy](#accuracy)).
> Not yet on PyPI — [build from source](#build--install) for now.

---

## Why pdfspine?

PyMuPDF is excellent, but it is **AGPL-3.0** (or a commercial license from
Artifex) — a non-starter for many closed-source products, SaaS backends, and
permissively-licensed open-source projects.

pdfspine is a **drop-in-shaped, permissively-licensed (Apache-2.0)** alternative:

- **Apache-2.0 throughout** — permissive, with an explicit patent grant. The
  dependency graph is gated by `cargo-deny` to **exclude GPL / AGPL / LGPL / MPL /
  SSPL** from the shipped wheel. License cleanliness is CI-enforced, not a promise.
- **Pure Rust, no C blob.** Self-contained wheels, no system `zlib`/C linkage, no
  bundled prebuilt engine (the differentiator vs pdfium-based wrappers).
- **`import fitz` compatible.** A compatibility shim lets much existing PyMuPDF
  code run unmodified, with a machine-readable [`COMPAT.toml`](COMPAT.toml)
  documenting every symbol's status.
- **Memory-safe by construction.** `#![forbid(unsafe_code)]` in every first-party
  crate except the single audited PyO3 FFI chokepoint.
- **Clean-room.** No code, tests, or fixtures derived from MuPDF / PyMuPDF / any
  AGPL source.

## What works today

| Area | Capabilities |
|---|---|
| **Read** | open (file/bytes), **malformed-PDF repair**, encrypted PDFs (RC4 / AES-128 / AES-256, R2–R6) |
| **Text** | `get_text` (`text/words/blocks/dict/rawdict/json/html/xhtml/xml`), `search_for`, `TextPage`, fonts/images inventory |
| **Tables** | `find_tables` with merged-cell detection → `extract()` / `to_markdown()` / **`to_html()`** |
| **Edit & save** | full + **byte-exact incremental** save, garbage collection, page insert/delete/copy/move/select, **`insert_pdf`** merge, metadata/XMP, TOC, links, encryption write |
| **Annotate** | all common annotation types with `/AP` appearance streams; AcroForm read / fill / flatten + `Widget`; **destructive redaction** (verified content removal) |
| **Render** | `get_pixmap` (vector + text + image + shadings via a tiny-skia rasterizer), `Pixmap` (buffer-protocol/numpy), `DisplayList`, **`get_svg_image`** |
| **Images** | open PNG/JPEG/TIFF/GIF/BMP/WEBP as documents, `convert_to_pdf`, image-XObject decode (DCT/CCITT/JBIG2/JPX), `extract_image` |
| **Layers** | Optional Content Groups read/write (`get_ocgs` / `add_ocg` / `set_layer`) |
| **CLI** | `pdfspine info / text / render / merge / split / pages / images / toc` |

Planned next: OCR (pluggable engine, Tesseract default), reading-order accuracy
improvements, Type1/Type3 glyph rendering, broader CJK. See [`PRD.md`](PRD.md) /
[`docs/ROADMAP.md`](docs/ROADMAP.md). Out of scope: digital-signature *creation*.

## Quick start

```python
import pdfspine

doc = pdfspine.open("input.pdf")
print(len(doc), "pages", doc.metadata)

page = doc[0]
print(page.get_text())                       # plain text
print(page.search_for("invoice"))            # list[Rect]
page.get_pixmap(dpi=150).save("page1.png")   # render to image

tables = page.find_tables()
for t in tables.tables:
    print(t.to_markdown())                    # or t.to_html() for merged cells

doc.save("output.pdf", garbage=4, deflate=True)
```

Existing PyMuPDF code often runs unchanged via the compat shim:

```python
import fitz                                   # -> pdfspine's fitz shim
doc = fitz.open("input.pdf")
text = doc[0].get_text("dict")
```

Command line:

```bash
pdfspine info report.pdf
pdfspine text report.pdf --pages 1-3 --format json -o out.json
pdfspine render report.pdf --dpi 200 -o images/
pdfspine merge a.pdf b.pdf -o merged.pdf
```

## Accuracy

First real-corpus validation (34 public-domain US-government PDFs, with PyMuPDF as
the differential oracle — see [`conformance/REPORT.md`](conformance/REPORT.md)):

- **Open rate 100%**, **0 panics/hangs**, **re-saved files 100% `qpdf --check`-clean**.
- Text vs PyMuPDF: **word-set overlap (Jaccard) ~0.92–0.97** (we extract the right
  words) — sequence similarity is lower, driven mainly by **multi-column reading
  order**, which is the current focus of an ongoing diff-oracle improvement loop.

This is an early data point (born-digital documents only); scanned / CJK / malformed
corpora come with the OCR and long-tail work.

## Build & install

Requirements: Rust (pinned to **1.96.0** by `rust-toolchain.toml`), **Python ≥
3.11**, [maturin](https://www.maturin.rs/) ≥ 1.7. [uv](https://docs.astral.sh/uv/)
recommended.

```bash
uv venv .venv && source .venv/bin/activate
maturin develop                 # build + install the extension in-place
python -c "import pdfspine; print(pdfspine.__version__)"
# redistributable wheel:
maturin build --release         # -> target/wheels/
```

## Architecture

A Cargo workspace with a strict dependency DAG; the Python bindings touch exactly
one façade crate, and core logic is split into independently testable units.

```
                  py-bindings   (PyO3 cdylib -> pdfspine._core, abi3-py311)
                       │
                       ▼
                    pdf-api      facade / re-exports
        ┌──────────┬───┴────┬──────────┐
        ▼          ▼        ▼          ▼
    pdf-text   pdf-edit  pdf-image  pdf-render
        │          │        │          │
        └────┬─────┘        │     (fonts, text)
             ▼              │
         pdf-fonts ◄────────┘
             ▼
         pdf-core   ◄────────  pdf-crypto
```

| Crate | Responsibility |
|---|---|
| `pdf-core` | object model, lexer/parser, xref, repair, filters, writer, geometry |
| `pdf-crypto` | Standard security handler (RC4 / AES-128 / AES-256) |
| `pdf-fonts` | font mapping (encodings / ToUnicode / CMap / widths) |
| `pdf-text` | content-stream interpreter, `get_text`, search, `find_tables` |
| `pdf-edit` | page ops, merge, annotations / forms, metadata / TOC, redaction, OCG |
| `pdf-image` | image documents, image-XObject codecs, `Pixmap` |
| `pdf-render` | tiny-skia rasterizer → `Pixmap`, `DisplayList`, SVG |
| `pdf-api` | unified ergonomic façade |
| `py-bindings` | PyO3 wrappers → the `_core` extension module |

## Develop / test

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
maturin develop && pytest python/tests       # Python tests
python conformance/run_validation.py …       # real-corpus accuracy harness
```

pdfspine is built strictly **test-first** (red → green → refactor → harden); the
per-function test plan is in [`docs/test-case-catalog.md`](docs/test-case-catalog.md).

## Documentation

Guide + API reference + PyMuPDF migration guide: build the docs site with
`mkdocs serve` (see [`mkdocs.yml`](mkdocs.yml) / [`docs/`](docs/)). The
authoritative design lives in [`PRD.md`](PRD.md).

## License

**Apache-2.0** — see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). All third-party
dependencies are permissive (MIT / Apache-2.0 / BSD / Zlib / …); the shipped graph
is CI-verified free of copyleft.
