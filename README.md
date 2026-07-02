# pdfspine

[![PyPI](https://img.shields.io/pypi/v/pdfspine.svg)](https://pypi.org/project/pdfspine/)

**An Apache-2.0-licensed, pure-Rust reimplementation of [PyMuPDF](https://pymupdf.readthedocs.io/) (`fitz`), with PyO3 Python bindings.**

> 🦴 **Part of the `spine` family — framework-free backend engines, each the spine of a domain:**
> zero framework lock-in, Protocol-ized seams, offline-capable. **pdfspine** is the *PDF spine* (this
> repo); **ragspine** is the *RAG spine* (deterministic dual-channel retrieval + agent orchestration).
>
> **🤖 For AI agents / LLMs:** before using this library, read [`llms.txt`](llms.txt) (concise index) and [`python/pdfspine/_llms/docs/`](python/pdfspine/_llms/docs/) (full API / recipes / gotchas); after `pip install` they ship at `site-packages/pdfspine/_llms/`.

> **Status: alpha / pre-1.0, but the core is feature-complete.** pdfspine can
> already parse/repair/decrypt PDFs, extract text & tables, search, edit / merge /
> split / save (incl. byte-exact incremental), encrypt, annotate, fill & flatten
> forms, redact (destructively), open image files as documents, **render pages to
> images**, and **OCR** (Tesseract + a pure-Rust PaddleOCR engine, stronger on CJK).
> **88.7%** (682 / 769) of the PyMuPDF 1.24 public API is implemented and tested
> (climbing), with **1349+ Rust tests + 593+ Python tests** green. Text extraction
> is at fitz parity (and beats fitz on Arabic / RTL), rendering is near-parity and
> ~1.74× faster, and the pure-Rust PaddleOCR engine beats fitz on CJK scans
> (see [Accuracy](#accuracy)).
> Now on PyPI: `pip install pdfspine` (see [Install](#install)); or
> [build from source](#build--install).

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
- **`import fitz` compatible (opt-in).** A compatibility shim lets much existing
  PyMuPDF code run unmodified — available as `import pdfspine.fitz as fitz`, or
  registered under the global `fitz` / `pymupdf` names with one call to
  `pdfspine.install_fitz_shim()`. A default install is collision-safe: it does
  **not** claim those global names, so it coexists with a real PyMuPDF in the
  same environment. A machine-readable [`COMPAT.toml`](COMPAT.toml) documents
  every symbol's status.
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
| **Markdown** | `markdown_to_pdf()` — a **pdfspine original extension** (not a PyMuPDF API): CommonMark + GFM tables / strikethrough / task lists → PDF via a deterministic pure-Rust layout engine; local & `data:`-URI images (never the network); optional user TTF via `font=` / `cjk_font=` (CJK) |
| **Layers** | Optional Content Groups read/write (`get_ocgs` / `add_ocg` / `set_layer`) |
| **OCR** | pluggable engine: Tesseract adapter **and** a pure-Rust PaddleOCR engine (PP-OCRv5, weights from the shared `ocrspine-models` package, stronger on CJK) → searchable-sandwich PDF |
| **CLI** | `pdfspine info / text / render / merge / split / pages / images / toc` |

Planned next: reading-order accuracy improvements, Type1/Type3 glyph rendering,
broader CJK coverage. See [`PRD.md`](PRD.md) / [`docs/ROADMAP.md`](docs/ROADMAP.md).
Out of scope: digital-signature *creation*.

## Install

```bash
pip install pdfspine
```

pdfspine is **on PyPI**. OCR works out of the box: the PP-OCRv5 weights ship in
the shared [`ocrspine-models`](https://pypi.org/project/ocrspine-models/) data
package — a runtime dependency `pip` pulls in automatically — so the wheel itself
stays lean and no longer embeds them. To build from source instead, see
[Build & install](#build--install).

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

# Markdown → PDF (pdfspine original extension — not part of the PyMuPDF surface)
pdfspine.markdown_to_pdf("# Title\n\nHello **Markdown**!").save("hello.pdf")
```

Existing PyMuPDF code often runs unchanged via the opt-in compat shim:

```python
import pdfspine.fitz as fitz                  # the shim, no global-name collision
doc = fitz.open("input.pdf")
text = doc[0].get_text("dict")

# Or make the literal `import fitz` resolve to the shim (one-time opt-in):
import pdfspine
pdfspine.install_fitz_shim()
import fitz                                    # now -> pdfspine's fitz shim
```

A default install does **not** claim the global `fitz` / `pymupdf` names, so it
is safe alongside a real PyMuPDF; `install_fitz_shim()` uses `setdefault` and
never clobbers a PyMuPDF you imported first.

Command line:

```bash
pdfspine info report.pdf
pdfspine text report.pdf --pages 1-3 --format json -o out.json
pdfspine render report.pdf --dpi 200 -o images/
pdfspine merge a.pdf b.pdf -o merged.pdf
```

## Accuracy

Validated against an objective ground-truth harness and with PyMuPDF (`fitz`) as
the differential oracle (clean-room: the AGPL oracle is run locally only and never
committed). See [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) and the
[`conformance/gt/`](conformance/gt/) reports for the dated, reproducible evidence.

- **Text extraction is at fitz parity** on born-digital corpora, and **beats fitz
  on Arabic / RTL** (correct bidi reordering).
- **Rendering is near-parity** with fitz (page-image SSIM ~**0.945**) and ~**1.74×
  faster** after a font-cache fix.
- **OCR beats fitz on CJK scans**: the pure-Rust PaddleOCR engine (PP-OCRv5, with
  weights from the shared `ocrspine-models` package) outperforms fitz's OCR path
  on Chinese/Japanese/Korean documents.
- Real-corpus robustness: **open rate 100%**, **0 panics/hangs**, **re-saved files
  100% `qpdf --check`-clean** across the public-domain US-government corpus.

Remaining accuracy work (multi-column reading order, Type1/Type3 glyph rendering,
broader CJK) is tracked in [`docs/PRD-NEXT.md`](docs/PRD-NEXT.md).

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

> **Building from source needs a C/asm compiler.** The bundled pure-Rust
> PaddleOCR engine depends on `tract`, which compiles target-specific assembly
> kernels at build time: a C compiler (`cc`/`clang`) on Linux/macOS, or the MSVC
> Build Tools (incl. `ml64.exe`) on Windows. Prebuilt PyPI wheels need none of
> this. To build a fully C-free library, compile the Rust crates with
> `--no-default-features` (drops the `paddle-ocr` feature). The wheel no longer
> embeds the OCR models — they ship in the shared `ocrspine-models` package (a
> runtime dependency).

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
