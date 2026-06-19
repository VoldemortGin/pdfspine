# PARITY.md — PyMuPDF (`fitz`) Parity Dashboard for `pdfspine`

> **What this file is.** The human-readable **progress dashboard** for the hard goal:
> **"whatever PyMuPDF has, we want."** It rolls up the machine-readable per-symbol disposition matrix in
> [`COMPAT.toml`](COMPAT.toml) into class-by-class and milestone-by-milestone coverage tables, ticks off the
> implemented surface, and calls out the **highest-value remaining work** so future batches can target it.
>
> **Baseline.** PyMuPDF **1.24.x** (1.24.14 / MuPDF 1.24.11) — the pinned baseline in PRD §7 / §9.5.
>
> **Single source of truth.** Per-symbol disposition (`implemented` / `deferred` / `out-of-scope`, plus owning
> group and milestone) lives in **`COMPAT.toml`**, which is generated from `scripts/_compat_catalog.py` and
> guarded in CI (compat-symbol-guard fails if any baseline symbol lacks an entry). **This file is a derived
> view of that matrix** — when the two disagree, `COMPAT.toml` wins and this dashboard is refreshed from it.
>
> **How it is maintained.** This file **complements**:
> - [`COMPAT.toml`](COMPAT.toml) — the authoritative per-symbol matrix (768 symbols + 1 catch-all).
> - [`docs/test-case-catalog.md`](docs/test-case-catalog.md) — the numbered **test IDs**. A symbol is not
>   truly "done" until its disposition is `implemented` in `COMPAT.toml` **and** its catalogued tests pass.
>
> When a batch lands new symbols, regenerate `COMPAT.toml`, then refresh the counts and the "Remaining work"
> section below from it.
>
> **Legend.**
> - **Status:** `implemented` (present in `python/` and does **not** raise `PdfUnsupportedError`) ·
>   `deferred` (known, planned for a later milestone M3–M8 / post-v1) ·
>   `out-of-scope` (intentionally never in v1; raises `PdfUnsupportedError`, per the PRD §7 catch-all + §17.2).
> - **Milestone:** M0 geometry · M1 PDF read core · M2 text extraction · M3 save/edit/merge/metadata ·
>   M4 content creation + annotations + redaction + forms · M5 image docs + codecs + Pixmap + shim ·
>   M6 vector page rendering · M7 SVG / tables / OCG · M8 OCR · `post-v1` · `out-of-scope`.

---

## Summary / Progress Dashboard

> **Snapshot (2026-06-18, after API batches 1–5).** Numbers below are recomputed from the live
> `COMPAT.toml` per-symbol dispositions. `COMPAT.toml [meta]` is always the authoritative live figure;
> the current remaining-work list (the 52 deferred symbols, grouped + prioritized) lives in
> [`docs/PRD-NEXT.md`](docs/PRD-NEXT.md) §3.B.

**Overall: 651 / 769 implemented (84.7% coverage).**

| Disposition | Count | Share |
|---|---:|---:|
| **implemented** | **651** | **84.7%** |
| deferred (planned, later milestone / post-v1) | 52 | 6.8% |
| out-of-scope (raises `PdfUnsupportedError`) | 66 | 8.6% |
| **Total catalogued symbols** | **769** | 100% |

> "Total" counts every PyMuPDF 1.24.x baseline symbol plus the `PdfUnsupportedError` catch-all row. Geometry
> (M0) is fully landed; M1–M5 read/text/edit/forms/annot/Pixmap surfaces are largely landed; M6 vector
> rendering, M7 SVG/tables/OCG, and M8 OCR have their headline paths implemented with long tails deferred.

### Per-class / per-group breakdown

| Class / group | Total | Implemented | Deferred | Out-of-scope | % impl |
|---|---:|---:|---:|---:|---:|
| `Matrix` | 25 | 25 | 0 | 0 | 100% |
| `Point` | 18 | 18 | 0 | 0 | 100% |
| `Rect` | 45 | 45 | 0 | 0 | 100% |
| `IRect` | 25 | 25 | 0 | 0 | 100% |
| `Quad` | 17 | 17 | 0 | 0 | 100% |
| `Document` | 150 | 119 | 17 | 14 | 79% |
| `Page` | 117 | 92 | 23 | 2 | 79% |
| `TextPage` | 17 | 17 | 0 | 0 | 100% |
| `Pixmap` | 43 | 40 | 3 | 0 | 93% |
| `Annot` | 51 | 46 | 1 | 4 | 90% |
| `Widget` | 35 | 28 | 0 | 7 | 80% |
| `Link` | 14 | 14 | 0 | 0 | 100% |
| `Outline` | 11 | 11 | 0 | 0 | 100% |
| `DisplayList` | 5 | 3 | 2 | 0 | 60% |
| `Shape` | 24 | 24 | 0 | 0 | 100% |
| `Font` | 23 | 20 | 2 | 1 | 87% |
| `TextWriter` | 10 | 10 | 0 | 0 | 100% |
| `Story` | 17 | 0 | 0 | 17 | 0% |
| `Xml` | 4 | 0 | 0 | 4 | 0% |
| `Archive` | 5 | 0 | 0 | 5 | 0% |
| `Colorspace` | 6 | 6 | 0 | 0 | 100% |
| `constants` | 43 | 41 | 0 | 2 | 95% |
| Module-level functions | 32 | 28 | 1 | 3 | 88% |
| `Tools` / `TOOLS` | 22 | 12 | 3 | 7 | 55% |
| `exceptions` | 10 | 10 | 0 | 0 | 100% |
| **Total** | **769** | **651** | **52** | **66** | **84.7%** |

### Per-milestone breakdown

> The early-snapshot per-milestone rollup is no longer maintained here: `COMPAT.toml` does not carry a
> per-symbol `milestone` field, so it cannot be recomputed mechanically. Use the **per-class table
> above** (recomputed from the live `COMPAT.toml`) + `docs/PRD-NEXT.md` §3.B for current status. By
> milestone, all of M0–M8's headline paths are landed (geometry, parsing, text, edit/save, annot/forms,
> image-docs/Pixmap, rendering near-parity, SVG/tables/OCG, OCR-via-Tesseract); the 52 deferred are the
> long tails and the 66 out-of-scope are the HTML/CSS story engine + render-era knobs.

---

## Status by class (what is landed)

Fully landed surfaces are summarised; partial surfaces list what is **implemented** vs the notable gaps. The
per-symbol truth (every name, disposition, milestone, note) is in [`COMPAT.toml`](COMPAT.toml).

### Fully landed (100% implemented)

- [x] **`Matrix` / `Point` / `Rect` / `IRect` / `Quad`** — all geometry math, constructors, operators,
  properties, and the geometry constants/aliases (`EMPTY_*`, `INFINITE_*`, `Identity`, `EPSILON`,
  `rect_like` …) — **M0 · implemented**
- [x] **`exceptions`** — full pdfspine-typed hierarchy + PyMuPDF exception-name aliases
  (`PdfError`, `PdfSyntaxError`, `PdfPasswordError`, `PdfUnsupportedError`, `PdfDecodeError`, `PdfLimitError`,
  `PdfRedactionError`, `FileDataError`, `EmptyFileError`, `FileNotFoundError`) — **M1 · implemented**

### Near-complete

- [x] **`Pixmap` (40/43)** — constructors, `save`/`tobytes`, `pil_save`/`pil_tobytes`, all pixel ops
  (`pixel`/`set_pixel`/`set_rect`/`set_alpha`/`clear_with`/`invert_irect`/`shrink`/`copy`/`tint_with`/`gamma_with`),
  metadata (`set_origin`/`set_dpi`/`xres`/`yres`/`digest`), analysis (`color_count`/`color_topusage`/
  `is_monochrome`/`is_unicolor`), all dimension/colorspace props, and `pdfocr_save`/`pdfocr_tobytes`.
  **Gaps (deferred):** `warp`, `samples_ptr`, `__array_interface__` (numpy zero-copy).

### Partial (headline paths landed, long tail deferred)

- [x] **`Page` (92/117)** — text extraction (`get_text` all variants, `get_textpage`, `search_for`, `TEXTFLAGS`,
  OCR textpage), inventory (`get_fonts`/`get_images`/`get_xobjects`/`get_image_info`/`get_image_bbox`/
  `get_image_rects`/`get_drawings`/`get_cdrawings`), the full annotation `add_*`/`delete`/`apply_redactions`
  family, widgets read (`widgets`/`first_widget`), links (`get_links`/`links`/`first_link`/`insert_link`/`delete_link`),
  drawing primitives + `new_shape`, `insert_text`/`insert_textbox`/`insert_image`/`show_pdf_page`,
  rendering (`get_pixmap`/`get_displaylist`/`get_svg_image`), `find_tables`, full box geometry
  (`set_mediabox`/`set_cropbox`/`artbox`/`bleedbox`/`trimbox` + setters), rotation read + `set_rotation` +
  the rotation matrices, `get_contents`/`read_contents`, page labels (`get_label`). Gaps: annot/widget/link
  object loaders, page-level draw convenience, `write_text`/`insert_font`, `remove_rotation`.
- [x] **`Document` (119/150)** — open/lifecycle, save family (`save`/`ez_save`/`save_incremental`/`write`/
  `tobytes`), page ops (`new_page`/`insert_pdf`/`delete_page`/`select`/`fullcopy_page`/`reload_page`/
  `page_xref`/`page_cropbox`), metadata + XMP read/write, TOC get/set, encryption read
  (`needs_pass`/`authenticate`/`permissions`/`is_encrypted`), low-level xref read
  (`xref_length`/`xref_object`/`xref_stream`/`xref_get_key`/`xref_is_stream`) + COS write
  (`update_object`/`update_stream`/`get_new_xref`/`pdf_catalog`/`pdf_trailer`/`xref_get_keys`/…) +
  state/meta (`pagelayout`/`pagemode`/`markinfo`/`language`/`need_appearances`/`get_sigflags`/`name`/…),
  embedded files (`embfile_*`), `bake`/`scrub`/`resolve_link`, forms, OCG read/add/toggle/bind (M7),
  journalling undo/redo (M3), page-label write, OCR export. Gaps: OCG layer object ops, TOC node ops,
  heavy ops (`convert_to_pdf`/`subset`/`insert_file`), `version_count`.
- [x] **`Annot` (46/51)** — `update`, all geometry/colors/opacity/border/flags/info getters+setters,
  `type`/`rect`/`xref`/`vertices`/`has_ap`, line-ends/blendmode/name/open, rotation/popup/apn/file-attach;
  only `get_textbox` deferred (+ 4 out-of-scope).
- [x] **`Shape` (24/24)** — fully landed: all draw primitives (line/rect/circle/oval/bezier/curve/polyline/
  quad/sector/squiggle/zigzag) + `insert_text`/`insert_textbox` + `finish`/`commit` + props.
- [x] **`Widget` (28/35)** — field props + appearance (`/MK`+`/DA`+`/BS`: border/fill/text color+style,
  fontsize/maxlen/format, field_display, is_signed, on_state, reset, rb_parent) + `update`; 7 out-of-scope.
- [x] **`TextPage` (17/17)** — fully landed: `extractText`/TEXT/BLOCKS/WORDS/DICT/JSON/RAWDICT/RAWJSON +
  `extractHTML`/XHTML/XML + `extractSelection`/`extractTextbox`/`search`/`extractIMGINFO` + `rect`/`poolsize`.
- [x] **`DisplayList` (3/5)** — constructor, `get_pixmap`, `rect` (records the render-op stream; replay via
  `get_pixmap`).
- [x] **`constants` (20/43)** — geometry singletons/aliases + encryption-method constants
  (`PDF_ENCRYPT_NONE/RC4_128/AES_128/AES_256`). Remaining enum tables (TEXT_*/PDF_ANNOT_*/…) deferred.
- [x] **Module-level (9/32)** — `open`, `version`, `identity_matrix`, `paper_size`/`paper_rect`/`paper_sizes`,
  `find_tables`, `get_text_length`. Remaining geometry/quad-recover helpers deferred.

### Now landed since the early snapshot (were "not started", now done)

- [x] **`Link` (14/14)**, **`Outline` (11/11)**, **`TextWriter` (10/10)**, **`Colorspace` (6/6)** — the
  value-object / writer / colorspace surfaces are now fully implemented.
- [x] **`Font` (20/23)** — metrics object (`text_length`/`char_lengths`/`glyph_advance`/`has_glyph`/
  `valid_codepoints`/`is_writable`/`Base14_fontnames`/…). Only `glyph_bbox`/`buffer` deferred (pdfspine's
  Font is a metrics-only handle with no embedded program); `css_for_pymupdf_font` is out-of-scope.
- [x] **`Tools` / `TOOLS` (12/22)** — diagnostics/tuning singleton headline paths landed; 3 deferred,
  7 out-of-scope (render-era knobs, raw `mupdf.*` access).

### Out-of-scope (raises `PdfUnsupportedError`)

- [ ] **`Story` / `Xml` / `Archive` (0/26)** — HTML/CSS → PDF layout engine — **entirely out-of-scope** (PRD §3.2 #2).

---

## Remaining work

The authoritative, prioritised list of the **52 deferred** symbols (grouped, with quick-wins flagged)
now lives in **[`docs/PRD-NEXT.md`](docs/PRD-NEXT.md) §3.B** — kept there to avoid two divergent lists.
In brief the deferred set is: **Page (23)** annot/widget/link loaders + page-level draw convenience +
`write_text`/`insert_font`/`remove_rotation`; **Document (17)** OCG/layers + TOC node ops + heavy ops
(`convert_to_pdf`/`subset`/`insert_file`) + `version_count`; **constants (21)** enum tables; **module
helpers (~15)** `recover_*_quad`/glyph-name maps/logging; **Font (2)** `glyph_bbox`/`buffer` (need an
embedded-program handle); plus small tails in Pixmap / Tools / DisplayList / Annot.

The **66 out-of-scope** symbols (raise `PdfUnsupportedError`) are dominated by `Story` / `Xml` /
`Archive` (the HTML/CSS -> PDF layout engine, PRD §3.2 #2) + render-era `Tools` knobs + EPUB
reflow / journalling-persistence + Widget JavaScript hooks.

---

*End of PARITY.md. This is a derived dashboard — `COMPAT.toml [meta]` is the live source of truth and
`docs/PRD-NEXT.md` §3.B is the live remaining-work list. To refresh the tables above, recompute the
per-group counts from `COMPAT.toml`.*
