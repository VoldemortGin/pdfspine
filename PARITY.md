# PARITY.md — PyMuPDF (`fitz`) Parity Dashboard for `oxide-pdf`

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

> **Snapshot.** Numbers below were taken from the `COMPAT.toml` `[meta]` block + per-symbol data at refresh
> time. `COMPAT.toml` is being actively regenerated as batches land, so the implemented count may have been
> **bumped further** since this snapshot — `COMPAT.toml [meta]` is always the live figure.

**Overall: 398 / 769 implemented (51.8% coverage).**

| Disposition | Count | Share |
|---|---:|---:|
| **implemented** | **398** | **51.8%** |
| deferred (planned, later milestone) | 304 | 39.5% |
| out-of-scope (raises `PdfUnsupportedError`) | 67 | 8.7% |
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
| `Document` | 150 | 71 | 65 | 14 | 47% |
| `Page` | 117 | 65 | 50 | 2 | 56% |
| `TextPage` | 17 | 8 | 9 | 0 | 47% |
| `Pixmap` | 43 | 40 | 3 | 0 | 93% |
| `Annot` | 51 | 24 | 23 | 4 | 47% |
| `Widget` | 35 | 11 | 17 | 7 | 31% |
| `Link` | 14 | 0 | 14 | 0 | 0% |
| `Outline` | 11 | 0 | 11 | 0 | 0% |
| `DisplayList` | 5 | 3 | 2 | 0 | 60% |
| `Shape` | 24 | 9 | 15 | 0 | 38% |
| `Font` | 23 | 0 | 22 | 1 | 0% |
| `TextWriter` | 10 | 0 | 10 | 0 | 0% |
| `Story` | 17 | 0 | 0 | 17 | 0% |
| `Xml` | 4 | 0 | 0 | 4 | 0% |
| `Archive` | 5 | 0 | 0 | 5 | 0% |
| `Colorspace` | 6 | 0 | 6 | 0 | 0% |
| `constants` | 43 | 20 | 21 | 2 | 47% |
| Module-level functions | 32 | 7 | 22 | 3 | 22% |
| `Tools` / `TOOLS` | 22 | 0 | 14 | 8 | 0% |
| `exceptions` | 10 | 10 | 0 | 0 | 100% |
| **Total** | **769** | **398** | **304** | **67** | **51.8%** |

### Per-milestone breakdown

| Milestone | Total | Implemented | Deferred | Out-of-scope |
|---|---:|---:|---:|---:|
| **M0** Geometry | 149 | 149 | 0 | 0 |
| **M1** PDF read core | 85 | 42 | 42 | 1 |
| **M2** Text extraction | 77 | 21 | 56 | 0 |
| **M3** Save / edit / merge / metadata | 93 | 33 | 60 | 0 |
| **M4** Content / annot / redaction / forms | 221 | 95 | 119 | 7 |
| **M5** Image docs / codecs / Pixmap / shim | 55 | 38 | 17 | 0 |
| **M6** Vector page rendering | 17 | 5 | 3 | 9 |
| **M7** SVG / tables / OCG | 9 | 9 | 0 | 0 |
| **M8** OCR | 6 | 6 | 0 | 0 |
| **post-v1** | 49 | 0 | 7 | 42 |
| **out-of-scope** | 8 | 0 | 0 | 8 |
| **Total** | **769** | **398** | **304** | **67** |

---

## Status by class (what is landed)

Fully landed surfaces are summarised; partial surfaces list what is **implemented** vs the notable gaps. The
per-symbol truth (every name, disposition, milestone, note) is in [`COMPAT.toml`](COMPAT.toml).

### Fully landed (100% implemented)

- [x] **`Matrix` / `Point` / `Rect` / `IRect` / `Quad`** — all geometry math, constructors, operators,
  properties, and the geometry constants/aliases (`EMPTY_*`, `INFINITE_*`, `Identity`, `EPSILON`,
  `rect_like` …) — **M0 · implemented**
- [x] **`exceptions`** — full oxide-pdf-typed hierarchy + PyMuPDF exception-name aliases
  (`PdfError`, `PdfSyntaxError`, `PdfPasswordError`, `PdfUnsupportedError`, `PdfDecodeError`, `PdfLimitError`,
  `PdfRedactionError`, `FileDataError`, `EmptyFileError`, `FileNotFoundError`) — **M1 · implemented**

### Near-complete

- [x] **`Pixmap` (40/43)** — constructors, `save`/`tobytes`, `pil_save`/`pil_tobytes`, all pixel ops
  (`pixel`/`set_pixel`/`set_rect`/`set_alpha`/`clear_with`/`invert_irect`/`shrink`/`copy`/`tint_with`/`gamma_with`),
  metadata (`set_origin`/`set_dpi`/`xres`/`yres`/`digest`), analysis (`color_count`/`color_topusage`/
  `is_monochrome`/`is_unicolor`), all dimension/colorspace props, and `pdfocr_save`/`pdfocr_tobytes`.
  **Gaps (deferred):** `warp`, `samples_ptr`, `__array_interface__` (numpy zero-copy).

### Partial (headline paths landed, long tail deferred)

- [x] **`Page` (65/117)** — text extraction (`get_text` all variants, `get_textpage`, `search_for`, `TEXTFLAGS`,
  OCR textpage), inventory (`get_fonts`/`get_images`/`get_xobjects`/`get_image_info`/`get_image_bbox`/
  `get_image_rects`/`get_drawings`/`get_cdrawings`), the full annotation `add_*`/`delete`/`apply_redactions`
  family, widgets read (`widgets`/`first_widget`), links (`get_links`/`insert_link`/`delete_link`),
  drawing primitives + `new_shape`, `insert_text`/`insert_textbox`/`insert_image`/`show_pdf_page`,
  rendering (`get_pixmap`/`get_displaylist`/`get_svg_image`), `find_tables`, boxes/rotation read +
  `set_rotation`, `get_contents`/`read_contents`, `get_label`.
- [x] **`Document` (71/150)** — open/lifecycle, save family (`save`/`ez_save`/`save_incremental`/`write`/
  `tobytes`), page ops (`new_page`/`insert_pdf`/`delete_page`/`select`/`fullcopy_page`/`reload_page`/
  `page_xref`/`page_cropbox`), metadata + XMP read/write, TOC get/set, encryption read
  (`needs_pass`/`authenticate`/`permissions`/`is_encrypted`), low-level xref read
  (`xref_length`/`xref_object`/`xref_stream`/`xref_get_key`/`xref_is_stream`), embedded files
  (`embfile_*`), `bake`/`scrub`/`resolve_link`, forms (`form_field_names`/`form_fill`/`form_flatten`),
  OCG read/add/toggle/bind (M7), journalling undo/redo (M3), page-label write, OCR export.
- [x] **`Annot` (24/51)** — `update`, geometry/colors/opacity/border/flags/info getters+setters, `type`/`rect`/
  `xref`/`vertices`/`has_ap`, line-ends/blendmode/name/open getters+setters.
- [x] **`Shape` (9/24)** — `draw_line`/`draw_rect`/`draw_circle`/`draw_oval`/`draw_bezier`/`draw_curve`/
  `draw_polyline` + `finish`/`commit`.
- [x] **`Widget` (11/35)** — `field_name`/`field_label`/`field_value`/`field_type`/`field_type_string`/
  `field_flags`/`rect`/`xref`/`choice_values`/`button_states` + `update`.
- [x] **`TextPage` (8/17)** — `extractText`/`extractTEXT`/`extractBLOCKS`/`extractWORDS`/`extractDICT`/
  `extractJSON`/`extractRAWDICT` + `rect`.
- [x] **`DisplayList` (3/5)** — constructor, `get_pixmap`, `rect` (records the render-op stream; replay via
  `get_pixmap`).
- [x] **`constants` (20/43)** — geometry singletons/aliases + encryption-method constants
  (`PDF_ENCRYPT_NONE/RC4_128/AES_128/AES_256`).
- [x] **Module-level (7/32)** — `open`, `version`, `identity_matrix`, `paper_size`/`paper_rect`/`paper_sizes`,
  `find_tables`.

### Not started (0% implemented)

- [ ] **`Link` (0/14)** — link value-object API (`rect`/`dest`/`uri`/`page`/`is_external`/`border`/`colors`/
  `flags`/`next`/`xref`/`linkDest`) — *deferred · M4*. (Page-level `get_links`/`insert_link`/`delete_link`
  **are** implemented; only the `Link` object wrapper is pending.)
- [ ] **`Outline` (0/11)** — TOC tree-node object (`title`/`dest`/`page`/`uri`/`next`/`down`/…) — *deferred · M3*.
  (`Document.get_toc`/`set_toc` **are** implemented; only the node object is pending.)
- [ ] **`Font` (0/23)** — font metrics object (`text_length`/`char_lengths`/`glyph_advance`/`has_glyph`/
  metrics props/…) — *deferred · M2*; `css_for_pymupdf_font` is *out-of-scope*.
- [ ] **`TextWriter` (0/10)** — styled-text writer (`append`/`appendv`/`fill_textbox`/`write_text`/…) — *deferred · M4*.
- [ ] **`Colorspace` (0/6)** — `Colorspace` object + `csGRAY`/`csRGB`/`csCMYK` — *deferred · M5*.
- [ ] **`Tools` / `TOOLS` (0/22)** — diagnostics/tuning singleton — *deferred (M1/M3/M4)* and
  *out-of-scope* (render-era knobs, raw `mupdf.*` access).
- [ ] **`Story` / `Xml` / `Archive` (0/26)** — HTML/CSS → PDF layout engine — **entirely out-of-scope** (PRD §3.2 #2).

---

## Remaining work

The 371 not-yet-implemented symbols split into **deferred** (304, planned) and **out-of-scope** (67, will raise
`PdfUnsupportedError`). The deferred set is prioritised below so future batches target the highest-value items
first. (Names abbreviated; consult [`COMPAT.toml`](COMPAT.toml) for the exact per-symbol rows.)

### HIGH-VALUE remaining (commonly used; prioritise these)

These are deferred symbols that real PyMuPDF users hit constantly. Landing them moves coverage and real-world
compatibility the most:

- **Text-block / word extraction (M2):** `Page.get_text_blocks`, `Page.get_text_words`, `Page.get_textbox`,
  `Page.get_text_selection`, `TextPage.search`, `TextPage.extractRAWJSON`, `TextPage.extractTextbox`,
  `TextPage.extractSelection` — the convenience shapes most extraction code uses.
- **`Page.get_texttrace` (M2)** — low-level per-glyph trace; valuable for ground-truth / layout analysis.
- **Document page-content helpers (M2):** `Document.get_page_images`, `Document.get_page_fonts`,
  `Document.search_page_for` — common one-call page queries.
- **`Document.subset_fonts` (M5)** — font subsetting; frequently requested for output size.
- **Font extraction / image extraction (M5):** `Document.extract_font`, `Document.extract_image` — staple
  asset-extraction calls.
- **`Font` object (M2, 0/23)** — metrics (`text_length`, `char_lengths`, `glyph_advance`, `has_glyph`,
  ascender/descender/bbox) needed for accurate text placement and `insert_text`/`TextWriter`.
- **`Link` object (M4, 0/14)** — `Link.rect`/`dest`/`uri`/`page`/`is_external`/`linkDest`; pairs with the
  already-implemented page-level link methods.
- **`Outline` object (M3, 0/11)** — TOC tree navigation; pairs with the implemented `get_toc`/`set_toc`.
- **`TextWriter` (M4, 0/10)** — `append`/`fill_textbox`/`write_text`; the modern text-emission API.
- **Remaining `Annot` members (M4):** `Annot.get_text`/`get_textbox`/`get_textpage`, `get_file`/`update_file`/
  `file_info`, popup control (`set_popup`/`popup_rect`/`has_popup`), `apn_bbox`/`apn_matrix`, `next`.
- **Remaining `Widget` members (M4):** colors/border (`border_color`/`fill_color`/`text_color`/`border_style`/
  `border_width`), text style (`text_font`/`text_fontsize`/`text_maxlen`/`text_format`), `reset`/`on_state`/
  `next`, `is_signed`.
- **Widget/annot write ops (M4):** `Page.add_widget`, `Page.load_widget`, `Page.delete_widget`,
  `Page.load_annot`, `Page.add_caret_annot`.
- **`Page.write_text` / `Page.insert_font` / `Page.replace_image` / `Page.delete_image` (M4)** — round out
  content creation.
- **Page box setters & matrices (M1/M3):** `Page.set_mediabox`/`set_cropbox`/`artbox`/`bleedbox`/`trimbox`,
  `transformation_matrix`/`rotation_matrix`/`derotation_matrix`, `Page.xref`, `Page.parent`,
  `Page.remove_rotation`.
- **Page-label read & destinations (M3):** `Document.get_page_labels`/`get_page_numbers`/`get_label`/
  `get_page_label`, `Document.resolve_names`, `Document.outline`.
- **Low-level COS write (M3):** `Document.update_object`, `Document.update_stream`, `Document.get_new_xref`,
  `Document.xref_set_key`, `Document.xref_copy`, `Document.xref_get_keys`, plus type predicates
  (`xref_is_font`/`xref_is_image`/`xref_is_xobject`), `pdf_catalog`/`pdf_trailer`/`is_stream`/
  `page_annot_xrefs`.
- **Document identity/state (M1/M3):** `Document.name`, `Document.is_closed`, `Document.is_dirty`,
  `Document.version_count`, `Document.is_form_pdf` is **done** — but `language`/`set_language`,
  `pagelayout`/`pagemode`/`markinfo` setters remain.
- **`Colorspace` (M5, 0/6)** — needed once codecs/Pixmap colorspace conversion is exercised broadly.

### NICHE / aliases / constants / low-frequency (defer until high-value is done)

- **Reflow / chapter-location model** — `Document.next_location`/`prev_location`/`location_from_page_number`/
  `make_bookmark`/`find_bookmark` (EPUB-class; mostly *out-of-scope*).
- **Journalling persistence & per-op naming** — `Document.journal_start_op`/`stop_op`/`op_name`/`position`/
  `save`/`load` (*out-of-scope*; core undo/redo already implemented).
- **OCG nesting / OCMD** — `Document.add_layer`/`get_layers`/`switch_layer`/`get_oc`/`get_ocmd`/`set_ocmd`,
  `Annot.set_oc`/`get_oc`, `Page.get_oc_items` (*deferred post-v1 / out-of-scope*).
- **`Tools` / `TOOLS` singleton** — diagnostics (`mupdf_warnings`/`mupdf_version`/`fitz_config`), cache knobs
  (`store_*`), id/stem (`gen_id`/`set_annot_stem`); render-era tuning (`set_aa_level`/`set_icc`/…) and raw
  `mupdf.*` access are *out-of-scope*.
- **Constant families** — most enum families are *deferred* until their owning subsystem (text/annot/widget/
  redaction/page-label/colorspace/border/signature) lands; `UCDN_SCRIPT_*` shaping enums are *out-of-scope*.
- **Module helpers** — color/text utilities (`get_text_length`, `sRGB_to_rgb`/`sRGB_to_pdf`,
  `glyph_name_to_unicode`/`unicode_to_glyph_name`, `recover_*_quad`, `planish_line`), HTML/XML export
  scaffolding (`ConversionHeader`/`ConversionTrailer`), logging (`set_messages`/`message`/`set_log`/`log`),
  `get_pdf_now`/`get_pdf_str`, `image_profile`.
- **Vector-render long tail** — `Page.run`, `DisplayList.run`/`get_textpage`, `Annot.get_pixmap`,
  `Page.cluster_drawings`, `Page.get_bboxlog` (M6; *deferred*/*out-of-scope*).
- **Widget JavaScript hooks** — `Widget.script*` (JS stored, never executed; *out-of-scope*).
- **Misc niche** — `Annot.get_sound`, `Annot.set_rotation`, `Annot.irt_xref`/`delete_responses`,
  `Document.get_sigflags`, `Document.convert_to_pdf` (non-image input), `Document.insert_file`,
  `Document.save_snapshot`.
- **`Story` / `Xml` / `Archive`** — full HTML/CSS engine; *out-of-scope* for v1 (PRD §3.2 #2).

---

*End of PARITY.md. This is a derived dashboard — to refresh: regenerate `COMPAT.toml`, re-roll the counts and
the "Remaining work" lists from it, and confirm tests in `docs/test-case-catalog.md` are green.*
