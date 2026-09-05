# pdfspine

[![PyPI](https://img.shields.io/pypi/v/pdfspine.svg)](https://pypi.org/project/pdfspine/)

**An Apache-2.0-licensed, pure-Rust reimplementation of [PyMuPDF](https://pymupdf.readthedocs.io/) (`fitz`), with PyO3 Python bindings.**

> 🦴 **Part of the `spine` family — framework-free backend engines, each the spine of a domain:**
> zero framework lock-in, Protocol-ized seams, offline-capable. **pdfspine** is the *PDF spine* (this
> repo); **ragspine** is the *RAG spine* (deterministic dual-channel retrieval + agent orchestration).
>
> **🤖 For AI agents / LLMs:** before using this library, read [`llms.txt`](https://github.com/VoldemortGin/pdfspine/blob/main/llms.txt) (concise index) and [`python/pdfspine/_llms/docs/`](https://github.com/VoldemortGin/pdfspine/tree/main/python/pdfspine/_llms/docs/) (full API / recipes / gotchas); after `pip install` they ship at `site-packages/pdfspine/_llms/`.

> **Status: alpha / pre-1.0, but the core is feature-complete.** pdfspine can
> already parse/repair/decrypt PDFs, extract text & tables, search, edit / merge /
> split / save (incl. byte-exact incremental), encrypt, annotate, fill & flatten
> forms, redact (destructively), open image files as documents, **render pages to
> images**, and **OCR** (Tesseract + a pure-Rust PaddleOCR engine, stronger on CJK).
> **89.3%** (687 / 769) of the PyMuPDF 1.24 public API is implemented and tested
> (climbing), with **1,702 Rust tests + 814 Python tests** passing in the 0.7.0
> release gate. In the dated 58-document benchmark, its aggregate mean text scores
> trail fitz by 0.2–1.4 percentage points (and it beats fitz on Arabic / RTL); rendering
> is at/near parity (SSIM 0.984 mean, 2026-06-21) though ~2× slower than fitz
> (2026-06-16 bench), and the pure-Rust PaddleOCR engine beats fitz on CJK scans
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
  same environment. A machine-readable [`COMPAT.toml`](https://github.com/VoldemortGin/pdfspine/blob/main/COMPAT.toml) documents
  every symbol's status.
- **Memory-safe by construction.** `#![forbid(unsafe_code)]` in every first-party
  crate except the single audited PyO3 FFI chokepoint.
- **Clean-room.** No code, tests, or fixtures derived from MuPDF / PyMuPDF / any
  AGPL source.

## What works today

| Area | Capabilities |
|---|---|
| **Read** | open (file/bytes), **malformed-PDF repair**, encrypted PDFs (RC4 / AES-128 / AES-256, R2–R6) |
| **Text** | `get_text` (`text/words/blocks/dict/rawdict/json/rawjson/html/xhtml/xml`), per-glyph rendering geometry, `search_for`, `TextPage`, fonts/images inventory |
| **Tables** | `find_tables` with merged-cell detection → `extract()` / `to_markdown()` / **`to_html()`**; optional Microsoft TATR vision backend for borderless tables |
| **Edit & save** | full + **byte-exact incremental** save, garbage collection, page insert/delete/copy/move/select, **`insert_pdf`** merge, metadata/XMP, TOC, links, encryption write |
| **Annotate** | all common annotation types with `/AP` appearance streams; AcroForm read / fill / flatten + `Widget`; **destructive redaction** (verified content removal) |
| **Render** | `get_pixmap` (vector + text + image + shadings via a tiny-skia rasterizer), `Pixmap` (buffer-protocol/numpy), `DisplayList`, **`get_svg_image`** |
| **Images** | open PNG/JPEG/TIFF/GIF/BMP/WEBP as documents, `convert_to_pdf`, image-XObject decode (DCT/CCITT/JBIG2/JPX), `extract_image` |
| **Markdown** | `markdown_to_pdf()` — a **pdfspine original extension** (not a PyMuPDF API): CommonMark + GFM tables / strikethrough / task lists → PDF via a deterministic pure-Rust layout engine; local & `data:`-URI images (never the network); optional user TTF via `font=` / `cjk_font=` (CJK) |
| **Layers** | Optional Content Groups read/write (`get_ocgs` / `add_ocg` / `set_layer`) |
| **OCR** | pure-Rust PaddleOCR by default (PP-OCRv5, weights from the shared `ocrspine-models` package, stronger on CJK), with an explicit Tesseract compatibility adapter → searchable-sandwich PDF |
| **CLI** | `pdfspine info / text / render / merge / split / pages / images / toc` |

Planned next: reading-order residuals, Type0/Type3 glyph-rendering edges,
broader CJK coverage. See [`PRD.md`](https://github.com/VoldemortGin/pdfspine/blob/main/PRD.md) / [`docs/ROADMAP.md`](https://github.com/VoldemortGin/pdfspine/blob/main/docs/ROADMAP.md).
Out of scope: digital-signature *creation*.

### Glyph geometry (0.7.0)

The structured `dict` / `rawdict` / `json` / `rawjson` formats expose span
`matrix`, `text_matrix`, `ctm`, `dir`, `quad`, `seq`, `declared_size`, and
`rendered_size`; raw characters also expose `matrix`, `quad`, `rendered_size`,
`seq`, and `synthetic`. The first glyph determines `span["size"]`; it equals
`span["rendered_size"]` and `sqrt(abs(det(matrix)))`, while `declared_size`
preserves the signed PDF `Tf` operand. Use each raw character's `rendered_size`
when glyph sizes vary.

`matrix` / `quad` use the page's device space, while `text_matrix` / `ctm` remain
in PDF user space. HTML / XHTML / XML and `get_texttrace()` retain their existing
declared-size semantics, and rotated-page coordinates and geometry-aware span
boundaries can differ from PyMuPDF. See the [text extraction guide](https://github.com/VoldemortGin/pdfspine/blob/main/docs/guide/text-extraction.md)
for the field contract and the [geometry](https://github.com/VoldemortGin/pdfspine/blob/main/conformance/GLYPH-GEOMETRY-REPORT.md),
[size parity](https://github.com/VoldemortGin/pdfspine/blob/main/conformance/GLYPH-GEOMETRY-SIZE-PARITY-REPORT.md),
[span parity](https://github.com/VoldemortGin/pdfspine/blob/main/conformance/GLYPH-GEOMETRY-SPAN-REPORT.md), and
[performance](https://github.com/VoldemortGin/pdfspine/blob/main/conformance/GLYPH-GEOMETRY-PERFORMANCE-REPORT.md) reports for the
measured limits and cost.

## Install

```bash
pip install pdfspine
```

pdfspine is **on PyPI**. OCR works out of the box: the PP-OCRv5 weights ship in
the shared [`ocrspine-models`](https://pypi.org/project/ocrspine-models/) data
package — a runtime dependency `pip` pulls in automatically — so the wheel itself
stays lean and no longer embeds them. To build from source instead, see
[Build & install](#build--install).

Python **3.12+** is supported. Prebuilt wheels are published for Linux x86-64 /
ARM64, macOS Intel / Apple silicon, and Windows x86-64; an sdist is also available
for other supported environments.

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

# Optional: pip install "pdfspine[tatr]", then prefetch the pinned checkpoints.
# vision_tables = page.find_tables(strategy="vision", backend="tatr")

doc.save("output.pdf", garbage=4, deflate=True)
doc.save_html("output.html")                 # complete UTF-8 HTML5 document

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
committed). See [`docs/BENCHMARKS.md`](https://github.com/VoldemortGin/pdfspine/blob/main/docs/BENCHMARKS.md) and the
[`conformance/gt/`](https://github.com/VoldemortGin/pdfspine/tree/main/conformance/gt/) reports for the dated, reproducible evidence.

- In the dated 58-document benchmark, pdfspine's aggregate mean edit-similarity,
  token-F1, word-set-Jaccard, and reading-order scores trail fitz by **0.2–1.4
  percentage points**. It reaches parity on selected born-digital metrics and
  **beats fitz on Arabic / RTL** (correct bidi reordering).
- **Rendering is at/near parity** with fitz (page-image SSIM **0.984** mean /
  **0.989** median over a 46-document sample, 2026-06-21). Speed, per the dated
  2026-06-16 [`conformance/BENCH.md`](https://github.com/VoldemortGin/pdfspine/blob/main/conformance/BENCH.md):
  open **1.4×** and text extraction **2.7×** faster than fitz; rendering about
  **2× slower** (the from-scratch Rust rasterizer is still young).
- **OCR beats fitz on CJK scans**: the pure-Rust PaddleOCR engine (PP-OCRv5, with
  weights from the shared `ocrspine-models` package) outperforms fitz's OCR path
  on Chinese/Japanese/Korean documents.
- Real-corpus robustness: **open rate 100%**, **0 panics/hangs**, **re-saved files
  100% `qpdf --check`-clean** across the public-domain US-government corpus.

Remaining accuracy work (reading-order residuals, Type0/Type3 glyph-rendering edges,
broader CJK) is tracked in [`docs/PRD-NEXT.md`](https://github.com/VoldemortGin/pdfspine/blob/main/docs/PRD-NEXT.md).

## Build & install

Requirements: Rust (pinned to **1.96.0** by `rust-toolchain.toml`), **Python ≥
3.12**, [maturin](https://www.maturin.rs/) ≥ 1.12,<2. [uv](https://docs.astral.sh/uv/)
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
./ci.sh                                      # complete local/CI/pre-push gate
# Individual focused checks remain available:
python scripts/quality_gate.py --help
python conformance/run_validation.py …       # real-corpus accuracy harness
```

pdfspine is built strictly **test-first** (red → green → refactor → harden); the
per-function test plan is in [`docs/test-case-catalog.md`](https://github.com/VoldemortGin/pdfspine/blob/main/docs/test-case-catalog.md).

## Documentation

Guide + API reference + PyMuPDF migration guide: build the docs site with
`mkdocs serve` (see [`mkdocs.yml`](https://github.com/VoldemortGin/pdfspine/blob/main/mkdocs.yml) / [`docs/`](https://github.com/VoldemortGin/pdfspine/tree/main/docs/)). The
authoritative design lives in [`PRD.md`](https://github.com/VoldemortGin/pdfspine/blob/main/PRD.md).

## License

**Apache-2.0** — see [`LICENSE`](https://github.com/VoldemortGin/pdfspine/blob/main/LICENSE) and [`NOTICE`](https://github.com/VoldemortGin/pdfspine/blob/main/NOTICE). All third-party
dependencies are permissive (MIT / Apache-2.0 / BSD / Zlib / …); the shipped graph
is CI-verified free of copyleft.
