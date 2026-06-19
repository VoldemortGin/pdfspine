# Changelog

All notable changes to **pdfspine** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

pdfspine is an Apache-2.0-licensed, pure-Rust reimplementation of PyMuPDF
(`fitz`) with PyO3 Python bindings. It is **alpha / pre-1.0**: the core is
feature-complete, but the public API and on-disk formats may still change.

## [Unreleased]

This section captures the pre-public state of the project, prior to the first
tagged release. The workspace version is `0.0.0` until that tag is cut.

### Added

- **PDF core (`pdf-core`):** lexer/tokenizer, object model and serializer;
  stream filters, predictors and a decode dispatcher; xref machinery with a
  lazy-access `DocumentStore`; malformed-PDF repair/reconstruction; PDF writer
  with full and incremental save, object-edit ChangeSets and garbage collection;
  page tree with a `Document`/`Page` facade.
- **Encryption (`pdf-crypto`):** Standard Security Handler read for revisions
  R2–R6; encrypted-write support.
- **Fonts (`pdf-fonts`):** font mapping (code → Unicode, code → width),
  Core-14 AFM widths, and predefined CJK CMaps for CID → Unicode extraction.
- **Text (`pdf-text`):** content-stream interpreter producing positioned glyphs,
  layout reconstruction into a PyMuPDF-shaped `TextPage`, `get_text` serializers
  with `TEXTFLAGS`, search, inventory, and UAX#9 bidi reordering for RTL/Arabic.
- **Editing (`pdf-edit`):** content insertion with font embedding, the
  annotation family with `/AP` appearance streams, AcroForm forms and the
  `Widget` API, destructive multi-surface redaction, `get_drawings`, page
  operations, `insert_pdf` merge, metadata/TOC/links/PageLabels, and embedded
  files with scrub/bake.
- **Images (`pdf-image`):** DCT / CCITT / JBIG2 / JPX image-XObject decoders,
  `Pixmap`, `get_pixmap`, `extract_image`, an image-document loader and
  `convert_to_pdf`.
- **Rendering (`pdf-render`):** vector path rasterization (fill/stroke/clip/
  blend) on a `Canvas`, text glyph rendering (ttf-parser outlines via
  tiny-skia, including Type3 CharProc recursion and bare-CFF / CID-keyed CFF
  parsing), image compositing, axial/radial shadings, full-page rendering to
  `get_pixmap` via a `DisplayList`, and standalone SVG export.
- **Tables & layers:** `find_tables` (line and text strategies) with merged-cell
  detection and `Table.to_html()`; Optional Content Groups (OCG / layers)
  read and write.
- **OCR (`pdf-ocr`):** a pluggable `OcrEngine` with a Tesseract adapter and a
  pure-Rust PaddleOCR engine (PP-OCRv4 via `tract`, with embedded models),
  Python-selectable, feeding an end-to-end searchable-sandwich PDF pipeline.
  Includes a CJK-scan accuracy benchmark and rotated-text detection.
- **Python API & compat:** PyO3 bindings, module-level constants and helper
  functions, and an **opt-in** `fitz`/`pymupdf` compatibility shim — importable
  as `import pdfspine.fitz as fitz`, or registered under the global `fitz` /
  `pymupdf` names via `pdfspine.install_fitz_shim()`. A default install is
  collision-safe and does not claim the global names.
- **CLI:** `pdfspine info / text / render / merge / split / pages / images / toc`.
- **Conformance harness:** an objective ground-truth accuracy harness
  (`conformance/gt/`) scoring pdfspine vs fitz vs pdfminer against shipped
  ground truth, plus rendering, table-extraction, CJK, multilingual (EUR-Lex),
  GovInfo domain-breadth and GovDocs1 robustness differentials. The `COMPAT.toml`
  disposition matrix and `compat-symbol-guard` track API parity (currently
  **84.1%**, 647 / 769 of the PyMuPDF 1.24 public API implemented and tested).

### Changed

- Renamed the project from `oxide-pdf` (originally `oxipdf`) to **pdfspine**,
  joining the `spine` family of framework-free backend engines.
- Made the `fitz` / `pymupdf` shim opt-in so a default install coexists with a
  real PyMuPDF rather than claiming the global import names.
- Release posture: pdfspine is Python-first; the Rust crates are reserved on
  crates.io only and ship with `publish = false`.

### Fixed

- UAX#9 bidi reordering for RTL lines — Arabic text extraction is now
  byte-perfect and beats fitz on RTL.
- Resolved CID-keyed CFF glyphs via charset (un-blanked CIDFontType0C text).
- Corrected CCITT / JBIG2 1-bpc polarity (un-inverted scanned pages).
- Replaced committed absolute paths under the pre-rename
  `/workspace/pypdf` working tree with repo-relative references so the
  conformance harness resolves corpora after the folder rename.

### Performance

- Cached font programs by `ObjRef`, making rendering ~1.74× faster; open is
  ~1.26× and text extraction ~2.75× faster than fitz in the bundled benchmark.

## [0.1.0] - Unreleased

_Placeholder for the first tagged release. No version has been published yet._

[Unreleased]: https://github.com/pdfspine/pdfspine/compare/HEAD
[0.1.0]: https://github.com/pdfspine/pdfspine/releases/tag/v0.1.0
