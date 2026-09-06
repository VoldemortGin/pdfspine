# Changelog

All notable changes to **pdfspine** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

pdfspine is an Apache-2.0-licensed, pure-Rust reimplementation of PyMuPDF
(`fitz`) with PyO3 Python bindings. It is **alpha / pre-1.0**: the core is
feature-complete, but the public API and on-disk formats may still change.

## [Unreleased]

### Added

- `markdown_to_pdf()` now writes clickable **link annotations** (`links=True`,
  default): `[text](https://…)`, `<autolinks>` and `<user@host>` become `/Link`
  annotations with a URI action; `[text](#anchor)` becomes a GoTo destination
  at the target heading's page and top edge (GitHub-style heading slugs,
  `{#id}` heading attributes, percent-encoded fragments; unresolved anchors
  get no annotation).
- `markdown_to_pdf()` now writes the heading hierarchy as the PDF **outline**
  (`toc=True`, default): `Document.get_toc()` reads it back; level jumps such
  as `#` → `###` are normalized to one step per nesting. Both passes reuse the
  `pdf-edit` `insert_link` / `set_toc` writers; a document without links or
  headings (or with both switches off) produces byte-identical output.
- **Layout-preserving text extraction.** `get_text("layout")` and its tunable
  form `Page.get_text_layout()` regroup words into visual lines with a *y
  tolerance* (jitter cannot chain lines) and paint them onto a character grid so
  columns survive as space padding, `pdftotext -layout` style. A
  pdfspine-original extension, outside the fitz-compat surface (`COMPAT.toml`
  unchanged).
- **`pdf-typeset`: PowerPoint-style font-independent line spacing.**
  `LineHeightRule::{FontMetrics, FontIndependent}` plus
  `Typesetter::set_line_height_rule()` / `line_height_rule()`: under
  `FontIndependent` a line is 1.2 × the largest font size on it with the
  baseline 1.0 em below the line top (the PowerPoint rule LibreOffice Impress
  emulates for ppt/pptx), engine-wide — flow, text boxes, table cells and the
  measure API agree. The default (`FontMetrics`, real face metrics) is
  byte-for-byte unchanged. LibreOffice-oracle SSIM of the pptx sample fixture
  0.92 → 0.98 (docx unchanged at 0.98). Crate-level Rust API consumed by
  docspine / pptspine; outside the fitz-compat surface (`COMPAT.toml`
  unchanged).
- **PDF → Markdown export.** `Page.to_markdown()`, `Document.to_markdown()` and
  `Document.save_markdown()` render a page or a whole document as Markdown for
  RAG / LLM pipelines: headings from font-size clustering, lists, GFM tables via
  `find_tables`, inline emphasis, and optional image placeholders. A
  pdfspine-original extension, outside the fitz-compat surface (`COMPAT.toml`
  unchanged); this is the PDF → Markdown direction, the reverse of the existing
  `markdown_to_pdf`.

### Fixed

- **PaddleOCR Latin accuracy 0.839 → 0.990** (CJK 0.989 → 0.993, speed unchanged)
  on the 16-scan CJK+Latin benchmark (`docs/BENCHMARKS.md` §6) by pinning
  `ocrspine` `e810a9c`: the recognizer right-padded each height-48 crop to its
  64 px width bucket with black, which normalizes to `-1.0` and reads as ink to
  the CRNN (the BiLSTM then garbled the tail of Latin lines: `A1938` → `A19w`);
  it now pads with mid-gray (≈`0.0` after normalization, as PaddleOCR's
  `resize_norm_img` does). pdfspine only bridges `Pixmap → ocrspine`, so the
  change is the `rev` bump in `crates/pdf-ocr/Cargo.toml`; the clean-scan Latin
  test that was a strict `xfail` is now a plain regression assertion.

## [0.7.1] — 2026-09-05

### Changed

- **Documentation-only release; PDF processing behavior is unchanged.**
  Synchronize the GitHub README and PyPI project description with the 0.7.0
  glyph-geometry fields, rendered versus declared font sizes, supported Python
  versions and wheel platforms, and current build requirements.
- State the dated text benchmark's aggregate mean differences precisely, retain
  the historical 0.7.0 release-gate counts, and mark the TATR quick-start call
  as optional.
- Clarify which corpus baseline manifests and aggregate summaries are tracked,
  and repair the coordinate-space tables in the English text-extraction guide
  and bundled Chinese API reference.

## [0.7.0] — 2026-09-05

### Added

- **Full glyph rendering geometry through `get_text`.** Structured spans
  (`dict`, `rawdict`, `json`, `rawjson`) expose `declared_size` (the original
  `Tf` operand), `rendered_size`, the device-space render `matrix`, source
  `text_matrix` and `ctm`, baseline `dir`, rotation-aware `quad`, and painting
  order `seq`. Raw characters expose `matrix`, `quad`, `rendered_size`, `seq`,
  and `synthetic`; lines add reading-order `number` and painting-order `seq`,
  and text blocks add `seq`.
- Reproducible corpus, ground-truth, performance, span-boundary, and size-parity
  reports with a trackable 300-document manifest in `conformance/`.

### Changed

- **Structured `span["size"]` now reports rendered font size**, matching the
  `sqrt(abs(det(matrix)))` rule, instead of the declared `Tf` operand. For
  example, `1 Tf` with a 12× text matrix now reports `size == rendered_size ==
  12.0` and `declared_size == 1.0`. Consumers needing the old structured value
  should use `declared_size`. HTML, XHTML, XML font-size attributes, and
  `get_texttrace()` retain their existing declared-size semantics. Span-level
  rendered size represents the first glyph; per-character `rendered_size`
  preserves variation within a span.
- **Visual span boundaries account for affine geometry and baseline changes.**
  Materially different transforms or baselines now split spans, while small
  within-span variation is tolerated. Text, words, and flattened character
  geometry are preserved on the fixed 1,887-page corpus. Some alphabetic and
  leader-dot runs consequently cross additional span boundaries.
- **XML character quads now carry the true glyph corners**, including rotated
  and sheared parallelograms, instead of axis-aligned bounding-box corners.

### Fixed

- SVG glyphs use their actual text rendering matrix, correcting scale and
  orientation under text matrices, CTMs, and horizontal scaling.
- Two-column correlation tables are read row by row.
- Rawdict conversion shares immutable keys and numeric values to reduce the
  memory cost of the expanded geometry without sharing mutable containers.
  Geometry remains unconditional: the measured optimized rawdict path still
  costs about 59% more streamed time and 49% more retained peak RSS than the
  pre-geometry baseline on the documented 118-page sample.

## [0.6.1] — 2026-09-03

### Fixed

- **Words split apart inside a tracked run.** The word-gap test now measures
  the part of the gap that `Tc` (and, on a space glyph, `Tw`) does *not*
  explain, instead of the raw distance between glyph cells. On a line laid out
  with letter-spacing, the tracking is already accounted for, so only the extra
  displacement from a `TJ` kern or an explicit move can open a word boundary.
  Every glyph carries the `Tc`/`Tw` share of its own advance as a user-space
  vector through `Tm · CTM`, so the correction stays exact under rotation,
  skew, `Tz` and vertical writing. On the 33 `0.1499 Tc` pages of the EUR-Lex
  32006L0112 corpus this keeps 61 words whole that PyMuPDF 1.27.2 breaks
  (`property`, `services`, `Definition`, `systems`, `Simplification`,
  `arrangements`, `transport`, …) with no regression in the other direction.
- **A synthesized word space now carries the cell it stands for** — the rect
  spanning the gap between the two glyphs — rather than a zero-width rect at
  the following glyph's origin. `get_text("rawdict")` and `get_text("words")`
  consumers that measure spaces see the same geometry as PyMuPDF.

### Changed

- **The line-level letter-spacing heuristic is gone.** Word gaps are now
  decided per glyph pair from the text state, with no statistics gathered over
  the line. The old mask inferred "this line is tracked" from the median
  measured gap, which misfired on pages that merely use large `TJ` kerns and
  no `Tc` at all — table-of-contents pages came back as
  `Originandscopeofrightofdeduction`. Against PyMuPDF over 300 documents /
  1885 pages, over-splits are unchanged at 334 while under-splits drop from
  394 to 332 (176 → 160 alphabetic).

### Added

- **`TEXT_INHIBIT_SPACES` is implemented.** The flag was exported but had no
  consumer; `get_text(..., flags=pdfspine.TEXT_INHIBIT_SPACES)` now suppresses
  synthesized word spaces and returns only whitespace that the content stream
  actually paints.

## [0.6.0] — 2026-09-02

### Added

- **Optional TATR (Table Transformer) vision table backend.**
  `page.find_tables(strategy="vision", backend="tatr", vision_options=...)`
  detects table regions and structure with Microsoft Table Transformer
  (pinned detection + v1.1-all structure checkpoints). The models only
  predict regions and structure; cell text always comes from pdfspine's
  native word coordinates (built-in OCR only when the page has no text
  layer). `Table` gains `confidence`, `source`, `text_source` and `metadata`
  properties. Ships in the new `[tatr]` extra (Pillow / torch / transformers,
  CPython 3.12–3.14 with OS/arch markers) — `[all]` now includes it; a bare
  install stays ML-free and `import pdfspine` never loads torch. The vendored
  MIT structure post-processing (microsoft/table-transformer @ 16d124f) is
  recorded in `THIRD-PARTY-NOTICES.md`. `conformance/gt/tables_diff.py` gains
  `--strategy vision` with detector P/R/F1 and GriTS (`TATR-001..009`).
- **Glyph-width fallback chain for simple fonts without `/Widths`** (`pdf-fonts`
  mapper, built once at font load): an embedded `/FontFile2` / `/FontFile3`
  program's `hmtx` / charstring advances → Core-14 AFM (now also covering
  WinAnsi 0x80–0x9F high punctuation, StandardEncoding quote glyphs, `fi` /
  `fl`, floating accents) → a `/Flags`-chosen standard substitute
  (FixedPitch → Courier, Serif → Times, else Helvetica; Bold / Italic from
  ForceBold / StemV / ItalicAngle / name) → `/MissingWidth`. Previously any
  non-Core-14 font without `/Widths` got zero-width cells, so per-glyph or
  per-word positioning read as word gaps or line breaks
  (`e x t r a c t i o n`, `Company’ s`). Truncated `/Widths` are deliberately
  not repaired (PyMuPDF parity). `WIDTHS-005..011`, `WORDS-019..022`.

### Fixed

- **Words and text now segment from the same source.** `get_text("words")`
  ran a second, purely spatial split with no letter-spacing awareness, so
  tracked headings (`0.15 Tc`, `3 Tc`) came back as one-letter words while
  `text` / `dict` / `blocks` kept them whole — 893 words-only over-splits in
  the 300-PDF differential. Word boundaries are now by construction identical
  to `to_text` split on whitespace (`WORDS-007..009`).
- **FontDescriptor `/Ascent` / `/Descent` sign normalised and degenerate
  cells rejected** (aligned with MuPDF): real corpora write `/Descent 250`,
  which halved the glyph cell, halved the word-gap threshold and synthesised
  spaces inside mildly kerned words; `rawdict` also reported a positive
  descender. A cell shorter than 0.5 × size falls through to `/FontBBox` and
  then the (800, −200) defaults (`TRM-005..009`, `WORDS-010`).
- **Word-gap threshold keyed on device-space font size instead of cell
  height** (`0.15 × size`, matching MuPDF's `SPACE_DIST`; was `0.2 ×` cell
  height, which collapsed to 0.1 × size for legal short cells like
  `/Ascent 500 /Descent 0`) (`WORDS-011..013`).
- **Letter-spacing mask only suppresses word gaps inside genuinely tracked
  runs.** Dot leaders (`. . . .`) and repeated punctuation no longer set the
  line's median gap, so TOC lines keep their real word gaps
  (`Originandscopeofrightofdeduction` → words); tracking at or above the
  word-gap threshold (EUR-Lex body text `0.15 Tc`) keeps kern-loosened pairs
  inside the word (`transpor t` → `transport`) (`WORDS-014..018`).
- **Never break a line between touching glyphs at a column gutter.** A gutter
  cut now requires real along-axis whitespace, so a one-string title painted
  across a table's columns is no longer shredded one character per line
  (`LIM\nITE\nD`, `Sche\ndule C`, `Mortalit\ny`); genuine two-column bodies
  still split column-major (`LAYOUT-E2E-003..005`).
- **Type3 `/FontMatrix` applied to glyph widths and vertical metrics** (ISO
  32000-1 §9.6.5). pdfTeX bitmap fonts (`FontMatrix [0.01204 …]`) had every
  cell 12× too narrow, turning `text extraction` into a line break or
  letter-by-letter words; a descriptor-less Type3 now derives its cell from
  its own `/FontBBox` (`WIDTHS-012..015`, `TRM-010/011`, `WORDS-023..025`).
- **Phantom whitespace glyphs dropped.** Word-generated letterheads paint
  empty paragraphs as `( ) Tj` on their own baselines; when such a space
  joined a neighbouring line and landed inside a word (`United Stat es`,
  `Washington ,`) it is now discarded unless it was painted in sequence with
  the line — real spaces, kerned-back footnote markers (`( 1 )`) and
  `-0.8 Tc` / `-1.5 Tw` spaces are untouched (`WORDS-026..028`).
- **Encrypted documents: authenticate before building the page list.** The
  page tree was computed eagerly before the empty-password auto-auth, so
  `/Pages` inside an object stream failed to decrypt, the reader silently fell
  back to an object-number scan and `doc[0]` returned the wrong physical page
  (page count still matched). Verified on 446 real-world reports: 94 shifted
  documents → 0, one 0-page document restored to its 14 pages
  (`DOC-CRYPT-004`).
- **PyMuPDF text-extraction parity pass** (b6c027a): `get_text("blocks")`
  segments blocks by baseline step (> 1.5 × effective size starts a block;
  dense table rows stay together) instead of returning page-sized blocks;
  Form XObjects start with the default text state and honour the inherited
  clip; rectangular clips drop boundary padding spaces; transformed-text
  baseline tolerance is measured in device space; `get_text("text",
  sort=True)` now really orders lines by (y, x) (`COMPAT-BLOCK-001..010`,
  `COMPAT-LINE-*`, `COMPAT-CLIP-SPACE-001..005`, `INTERP-FORM-006`,
  `PYTEXT-010`).
- `DOC-CRYPT-001/003` and `CONTENT-BLOCKS-002` re-pinned to the PyMuPDF
  semantics above; clippy / ruff-format drift from b6c027a resolved.

Corpus differential (300 PDFs / 1885 pages, PyMuPDF 1.27.2 oracle): word
over-splits relative to PyMuPDF fell from 1800 to 334 (pure-alpha 1187 → 83)
and under-splits from 1452 to 394; the remaining under-splits are dominated
by cases where PyMuPDF itself breaks a tracked word (`transpor t`) and
pdfspine deliberately keeps it whole. On the 30-document real-corpus
conformance report (`conformance/REPORT.md`), full-document Levenshtein
similarity vs PyMuPDF rose from mean 0.919 / median 0.938 to mean 0.961 /
median 0.993.

### Changed

- **Empty-user-password documents are authenticated on open** (PyMuPDF
  behaviour): `is_encrypted` stays `True`, but `needs_pass` is now `False`
  right after `open()` and `authenticate("")` is no longer required before
  reading pages.
- **Word-gap threshold is `0.15 × device font size`** (was `0.2 ×` cell
  height). Downstream code that tuned around the old, more conservative
  threshold may see additional (correct) word breaks on widely kerned runs.
- **`get_text("words")` is now whitespace-only segmentation** of the laid-out
  line; a run that `text` joins as one word is never split by `words`.
- **Tracked runs keep kern-loosened pairs together** even where PyMuPDF splits
  them (`transport` vs PyMuPDF `transpor t`): whole words win over strict
  oracle parity for this one case.
- `get_text("blocks")` granularity now follows PyMuPDF's line/paragraph
  blocks (see Fixed); consumers that relied on the old page-sized blocks
  will see many more, smaller blocks.
- Fonts without `/Widths` now measure by substitute or embedded metrics
  instead of `/MissingWidth` (default 0): `rawdict` / `words` bboxes for such
  fonts widen from zero-width cells to real advances.

### Security

- **tract bumped to 0.21.17 (`Cargo.lock` only) for RUSTSEC-2026-0217** —
  integer overflow in `tract-nnef`'s NNEF tensor parser (out-of-bounds read
  on model load), reachable through `tract-onnx` ← `ocrspine` ← `pdf-ocr`.
- **`deny.toml` ignores RUSTSEC-2026-0009** (`time` 0.3.41 stack-exhaustion
  DoS): the tract 0.21.16+ bump pins `time < 0.3.42` as a *build-only*
  dependency of `tract-linalg` (liquid templates in its build script); it
  never enters the runtime artifact. The ignore is to be removed once ocrspine
  moves to tract 0.22+.

## [0.5.0] — 2026-07-30

### Changed

- **Breaking: Python floor raised to 3.12.** `requires-python` is now
  `>=3.12` (previously `>=3.11`); CPython 3.11 is no longer a supported
  install target. The extension module still builds against the abi3-py311
  stable ABI (unchanged binary interface) — only the supported interpreter
  floor changed.

### Fixed

- **Table cell text follows visual order for mixed-style spans.**
  `Table.extract()` and `Table.to_markdown()` sorted a cell's words by exact
  bbox-center *y* before *x*, so spans sharing one visual line but differing in
  font size or baseline offset (a raised `New` badge, a lowered `*`) leaked
  their sub-point center differences into the order — e.g.
  `* (Group) Leading Organizational Resilience New` exported as
  `New (Group) Leading Organizational Resilience *`. Cell words are now grouped
  into visual lines by vertical proximity and re-sorted by *x* within each line
  (the grouping `Table.to_html()` already used), so all three exports agree on
  word order (`TABLES-REGR-005`).

### Added

- **Typed page-content API (pdfspine-original extension, not part of the
  fitz-compat surface / COMPAT.toml).** Four new `Page` methods return frozen
  dataclass value objects (new module `pdfspine.models`, re-exported at the
  top level) instead of raw dicts:
  - **`Page.content_blocks(sort=True)`** — the `get_text("dict", sort=...)`
    block sequence as `tuple[TextBlock | ImageBlock, ...]`, same order; image
    blocks keep the original encoded bytes + extension untouched (no OCR, no
    re-encoding; `image=None` when the payload is unavailable).
  - **`Page.link_annotations()`** — the external-URI subset of `get_links()`
    as `tuple[LinkAnnotation, ...]` (`uri` + `from` rect); GoTo/named links
    and malformed entries are skipped, never raising. Named
    `link_annotations` because `Page.links()` is the PyMuPDF-compatible
    `Link` iterator, which is unchanged.
  - **`Page.text_in_rect(rect, *, sort="visual")`** — visually ordered text
    of the spans whose bbox center lies inside `rect`: lines regrouped by
    y-band and ordered `(y0, x0)`, spans ordered by `x0`, a single space
    inserted on a clear horizontal gap, whitespace compressed.
  - **`Page.filled_rectangles(include_white=False)`** — the rectangular fill
    paths of `get_drawings()` (type `"f"`/`"fs"`, all items `("re", Rect)`)
    as `tuple[FilledRectangle, ...]` with the fill color; white fills dropped
    by default.

## [0.4.1] — 2026-07-20

### Added

- **Browser-ready HTML export.** New `Document.to_html()` combines each page's
  `Page.get_text("html")` fragment, in order, into a complete UTF-8 HTML5
  document. The safely escaped title falls back from PDF metadata to the input
  filename and then `PDF document`. `Document.save_html(path)` accepts
  `str` / `os.PathLike`, writes the same document as UTF-8, and follows the
  existing save convention by returning `None`.
- **Parity long-tail — four pure-Python symbols promoted from `deferred` to
  `implemented` (COMPAT.toml).** All expressible over existing pdfspine infra,
  each pinned against the PyMuPDF oracle (`python/tests/test_longtail13.py`):
  - **`Page.remove_rotation`** — bakes `/Rotate` into the content stream as a
    `cm` prefix, swaps the media box for 90°/270°, resets rotation to 0 and
    rewrites annotation / link / widget rects; returns the inverse derotation
    matrix (the identity when already upright). Derotation matrices and page
    geometry match fitz exactly for 0/90/180/270.
  - **`Page.refresh`** — re-syncs the page handle in place via `reload_page`
    (no-op for a parentless page).
  - **`Page.write_text`** — renders one or more `TextWriter` objects onto a
    page (direct draw for a single writer; `show_pdf_page` compose otherwise).
  - **`Document.insert_file`** — inserts an image / PDF source (Pixmap /
    Document / bytes / path) via the `image_to_pdf` + `insert_pdf` pipeline;
    genuinely non-image, non-PDF input raises `PdfUnsupportedError`.
  - COMPAT coverage 88.7% → 89.2% (`deferred` 21 → 17).
- **`Document.FormFonts`** — PyMuPDF read-only property returning the font
  resource key names in `/AcroForm /DR /Font` (e.g. `"Helv"`, without the
  leading slash); an empty / missing dict on a valid PDF → `[]`. Reads only, never
  creates. Promoted from `deferred` to `implemented`, pinned against the oracle in
  `python/tests/test_longtail14.py`. COMPAT coverage → 89.3% (`deferred` 17 → 16).

## [0.4.0] — 2026-07-13

### Added

- **Tab-stop advance in `pdf-typeset` (§10 TS-9 / docspine C-9).** A `\t` now
  advances the pen to the next tab stop instead of collapsing to a single
  space; the interval is Word's `defaultTabStop` (0.5 inch default) and is
  configurable via `Typesetter::set_tab_interval`. Post-tab text lands on the
  stop within 1 pt; justify never widens a tab; auto table-column measurement
  accounts for tab advances.
- **Public text-measurement API in `pdf-typeset` (§10 TS-10).** New
  `Typesetter::measure_blocks(blocks, width, wrap)` and
  `Typesetter::measure_text_box(spec)` report, without emitting a PDF, the
  laid-out line metrics (`LineMetrics { ascent, descent, height }`) plus total
  content height and natural width (`Measurement`) at a fixed width. They share
  the exact measure → wrap → line-box path the emitters run, so the reported
  height equals what `layout_text_box` / box-mode `layout_flow` actually lay
  out — pinned by tests. Unblocks consumer-side sizing: pptspine autofit,
  tables grown to content, docspine cell vertical alignment. The new
  `Measurement` / `LineMetrics` types are `#[non_exhaustive]`.
- **Table cell vertical anchoring in `pdf-typeset` (§10 TS-11).**
  `TableCell` gains `v_align: VAnchor` (default `Top`): a cell's content is
  offset within the finalized, content-driven row height for `Middle` / `Bottom`
  anchoring (docx `tcPr` `vAlign` / pptx cell `anchor`). Anchoring runs after
  the row height is fixed, so it never interferes with content-driven row
  growth.
- **Run-level hyperlinks in `pdf-typeset` (§10 TS-11).** `RunStyle` gains
  `link: Option<String>` (a target URI). The engine accumulates each linked
  run's real laid-out rectangles from the existing flow / text-box layout path
  and emits them as page `/Link` annotations (`/A << /S /URI >>`, borderless).
  Adjacent same-URI fragments merge into one rectangle per line; a run that
  wraps across lines yields one rectangle per line. Links ride the same
  translate / group-transform pipeline, so they land on the real glyphs inside
  boxed and table-cell text. A new `Op::Link` op carries the hot-zones.

### Fixed

- **Encryption read semantics now match PyMuPDF exactly (5 deviations, PRD-NEXT
  §5; re-adjudicated vs the pinned PyMuPDF 1.24.14 oracle).** For an encrypted
  `Document`: `is_encrypted` now means "still locked" (encrypted **and** not yet
  authenticated) and flips to `False` after the empty-password auto-auth or a
  successful `authenticate` — it is no longer permanently truthy just because a
  `/Encrypt` dict is present; `needs_pass` is now the stateless "empty password
  does not unlock" predicate (stays truthy after a real-password authenticate,
  like MuPDF `pdf_needs_password`); `permissions` returns `0` while locked (the
  `/P` flags once unlocked, `-4` unencrypted) instead of the raw `/P`;
  `metadata` returns `None` while locked instead of a decrypted-garbage dict;
  and `metadata["encryption"]` now carries the cipher suffix
  (`"Standard V2 R3 128-bit RC4"`, `"Standard V5 R6 256-bit AES"`) with the
  key-length correct for AES-256. Shaped at the PyMuPDF-compat boundary; the
  lower-level pdf-core/pdf-api explicit-auth Rust contract is unchanged.

## [0.3.0] — 2026-07-04

### Added

- **Shared PDF typesetting engine `pdf-typeset` (workspace crate; Phase A
  complete).** A deterministic, pure-Rust rich-text layout engine living in
  this repo (`crates/pdf-typeset`, with glyph-subsetting extensions in
  `pdf-fonts`): rich-text input model, fontdb-backed system-font resolution
  with a CJK substitution table, TTC face selection, usage-based TrueType
  glyph subsetting, flow layout with text boxes and preset geometry, glyph
  clipping, `srcRect` image cropping, `AtLeast` line spacing, list-label
  extensions, structured degradation warnings (`Custom` kinds), and a
  conformance test gate. It powers the sibling packages' faithful document
  exports — **pptspine 0.2.0 `.pptx → PDF`** and **docspine 0.2.0
  `.docx → PDF`** — which consume it as a pinned git dependency. The engine is
  workspace-internal: the `pdfspine` Python wheel surface is unchanged by this
  release (`markdown_to_pdf()` keeps its own `pdf-markdown` layout path).

## [0.2.0] — 2026-07-02

### Added

- **Markdown → PDF: new top-level `pdfspine.markdown_to_pdf()` (pdfspine
  original extension).** Renders CommonMark + GFM (tables, strikethrough, task
  lists) to a new PDF `Document` through a self-authored, deterministic
  pure-Rust layout engine (new `pdf-markdown` crate; `pulldown-cmark` parses,
  layout and drawing are in-house). This is **not** PyMuPDF's
  `Story` / `insert_htmlbox` HTML-CSS engine and is not part of the fitz-compat
  surface (`COMPAT.toml` does not list it). Covers headings H1–H6, paragraphs
  with bold / italic / inline-code / links (blue text, no annotation in v1),
  nested ordered / unordered / task lists, nested blockquotes, fenced code
  blocks (grey background, Courier), horizontal rules, GFM tables (measured
  column widths, bold header, row-by-row cross-page pagination) and images
  from local paths / `data:` URIs (JPEG passthrough, other formats decoded;
  **no network access ever**). Accepts Markdown text or a
  `.md` / `.markdown` / `.txt` / suffix-less file path (the file's parent
  becomes the image base directory). Options: page size (default A4), margins
  (default 72 pt), body font size (default 11 pt), `font=` (user TTF/OTF
  replacing body + headings) and `cjk_font=` (per-character fallback — without
  it CJK renders as `?`, see the docs); user fonts are embedded once,
  usage-subset. Deterministic: same input + options → identical PDF bytes.

## [0.0.6] — 2026-06-25

### Changed

- **Coordinate basis unified MediaBox → CropBox (corrective breaking).** The
  `page_transform` basis for the digital-text, vector and `get_drawings`
  channels (and, transitively, the line-strategy table finder) is now the
  **CropBox** instead of the MediaBox. On pages where `CropBox ≠ MediaBox` the
  device coordinates of digital text / vector paths / drawings / tables now share
  a single origin with the already-CropBox-based render, SVG and OCR channels,
  eliminating the cross-channel spatial offset. This is a **corrective breaking
  change** for the (uncommon) `CropBox ≠ MediaBox` pages only: extracted digital-
  text device coordinates there shift from a MediaBox basis to a CropBox basis, so
  any downstream consumer that relied on the old MediaBox-based coordinates must
  be updated. Pages with `CropBox == MediaBox` (the overwhelming majority) are
  byte-for-byte unaffected (`cropbox()` returns the MediaBox when `/CropBox` is
  absent). `get_cdrawings` keeps its raw user-space output unchanged.

### Packaging

- OCR models are no longer embedded in the `pdfspine` wheel: the ~28 MB PP-OCRv5
  ONNX weights now ship in the shared `ocrspine-models` data distribution, pulled
  in as a runtime dependency. `pip install pdfspine` still bundles OCR out of the
  box (the models are resolved automatically) while the wheel shrinks by ~28 MB.
  `ocrspine-models` must be published on PyPI before the first `pdfspine` release
  that depends on it.

### Tests

- Added cross-layer alignment tests on `CropBox ≠ MediaBox` pages, each with a
  negative control: digital-text device bbox vs render pixel position share a
  zero-crop-offset origin; `get_drawings` and `find_tables` device coordinates are
  pinned to the same CropBox origin; plus an API round-trip test
  (`derotation_matrix` exactly inverts the extracted bbox, including with
  `/Rotate`). Adjusted the `COORD-ROT-MEDIABOX` case to `COORD-ROT-CROPBOX`
  (CropBox origin baked into the transform).

## [0.1.0] — 2026-06-21

The first public release. The local/dev workspace version is `0.0.0`; the
published wheel's version is set from the `v0.1.0` git tag at build time.

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
  with `TEXTFLAGS`, search, inventory, UAX#9 bidi reordering for RTL/Arabic, and
  Kangxi-radical CJK folding for compatibility ideographs.
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
  `get_pixmap` via a `DisplayList`, and standalone SVG export. Indexed /
  Separation / DeviceN colorspaces and `/Decode` arrays render (pixel-exact vs
  fitz on synthetic cases).
- **Tables & layers:** `find_tables` (line and text strategies) with merged-cell
  detection and `Table.to_html()`; Optional Content Groups (OCG / layers)
  read and write.
- **OCR (`pdf-ocr`):** a pluggable `OcrEngine` with a Tesseract adapter and a
  pure-Rust PaddleOCR engine (PP-OCRv5 via `tract`, with embedded models),
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
  **88.7%**, 682 / 769 of the PyMuPDF 1.24 public API implemented and tested;
  21 deferred, 66 out-of-scope).
- **API parity push (+29 symbols to 88.7%):** a
  Page/Document/Annot/Widget/Shape/TextPage batch added +29 PyMuPDF symbols
  (84.7% → 88.4%), then `Font.buffer` / `Font.glyph_bbox` backed by the real
  `/FontFile*` program (Font class 22/23) brought parity to **88.7%**
  (682/769). See `PARITY.md`.
- **Font fallback & embedded programs:** non-embedded standard-14 fonts now fall
  back to the OFL Liberation families (no more blank body text), and embedded
  **Type1** programs (`/FontFile`, PFB/PFA) rasterize via their charstrings.
- **Full API reference:** the complete public surface is documented via
  mkdocstrings (307/307 symbols).
- **OCR distribution:** the published `pdfspine` wheel compiles OCR in but embeds
  **no models** (lean base wheel); the ~16 MB PP-OCRv5 models ship as a separate
  `pdfspine-ocr-models` data distribution pulled in by the `[ocr]` extra
  (`pip install pdfspine[ocr]`), resolved offline at runtime via
  `PDFSPINE_OCR_MODELS` → companion → in-repo dev fallback (no download).

### Changed

- Renamed the project from `oxide-pdf` (originally `oxipdf`) to **pdfspine**,
  joining the `spine` family of framework-free backend engines.
- Made the `fitz` / `pymupdf` shim opt-in so a default install coexists with a
  real PyMuPDF rather than claiming the global import names.
- Release posture: pdfspine is Python-first; the Rust crates are reserved on
  crates.io only and ship with `publish = false`.

### Fixed

- **Multi-column reading order** — verified at fitz parity against fresh ground
  truth: born-digital column corpus `order` 0.996 (jaccard 0.965, dead-even with
  fitz) and PMC scientific corpus `order` 0.965 mean / 0.995 median (fitz
  0.975 / 0.997). See `docs/BENCHMARKS.md`.
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
- OCR `recognize()` runs its per-box loop with a rayon `par_iter` (indexed
  collect → byte-identical output): **3.49× faster** on a 42-box page (16 cores,
  2858 ms → 819 ms). `rayon` is a feature-gated (`paddle-ocr`) optional dep and
  is not in the lean base wheel.

[Unreleased]: https://github.com/VoldemortGin/pdfspine/compare/v0.7.1...HEAD
[0.7.1]: https://github.com/VoldemortGin/pdfspine/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/VoldemortGin/pdfspine/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/VoldemortGin/pdfspine/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/VoldemortGin/pdfspine/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/VoldemortGin/pdfspine/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/VoldemortGin/pdfspine/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/VoldemortGin/pdfspine/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/VoldemortGin/pdfspine/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/VoldemortGin/pdfspine/compare/v0.1.2...v0.2.0
[0.0.6]: https://github.com/VoldemortGin/pdfspine/compare/v0.0.5...v0.0.6
[0.1.0]: https://github.com/VoldemortGin/pdfspine/releases/tag/v0.1.0
