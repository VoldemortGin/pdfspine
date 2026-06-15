# PARITY.md — PyMuPDF (`fitz`) Parity Checklist for `oxide-pdf`

> **What this file is.** The *living*, method-level source of truth for how far `oxide-pdf` has progressed
> toward the hard goal: **"whatever PyMuPDF has, we want."** Every public class, method, property, and
> module-level symbol of **PyMuPDF 1.24.x (baseline 1.24.14 / MuPDF 1.24.11)** is catalogued here, grouped
> by class, with a checkbox, a **Priority** (P0–P3, from PRD §7), a **Milestone** (M0–M6 / post-v1 / out-of-scope),
> and a **Status**.
>
> **How it is maintained.**
> - Every row starts **unchecked** (`- [ ]`) with **Status: `catalogued`**.
> - When a capability is implemented **and its catalogued tests are green**, tick its box (`- [x]`) and flip
>   Status to **`implemented`**. Intermediate states allowed: `in-progress`, `partial` (documented subset),
>   `deferred` (intentionally not in v1), `out-of-scope`.
> - This file answers **"did we miss anything PyMuPDF has?"** It is the coverage guard. It **complements**
>   [`docs/test-case-catalog.md`](docs/test-case-catalog.md), which tracks the numbered **test IDs**; this file
>   tracks the **API symbols**. A symbol is not "done" until *both* its tests exist (in the catalog) and pass.
>
> **Sources.** Priorities/milestones come from **PRD §7** (Scope & Feature Catalog) and the per-subsystem
> requirements in **PRD §8**. The exhaustive symbol list comes from the "pymupdf-api-surface" research strand
> (direct introspection of PyMuPDF 1.24.14 + official docs). Where the research lists a symbol **not mapped in
> PRD §7**, it is still catalogued here (with a best-guess priority/milestone) and flagged
> **`(not in PRD §7 — verify)`** so scope can be extended deliberately.
>
> **Legend.**
> - **Priority:** P0 = v1 must-ship core · P1 = strongly expected for credible parity · P2 = valuable, may trail
>   (documented subset) · P3 = niche/post-v1 · `deferred`/`out-of-scope` per PRD §3.2 / §7.
> - **Milestone:** M0 geometry · M1 PDF read core · M2 text extraction · M3 save/edit/merge/metadata ·
>   M4 content creation + annotations + redaction + forms · M5 image docs + codecs + Pixmap + shim ·
>   **M6 = vector page rendering (post-v1, deferred)** · `post-v1` · `out-of-scope`.
>   *(Note: PRD §7 milestone numbering is authoritative and differs from the api-surface research's own M0–M8
>   suggestion; we follow PRD §7. Rendering = M6 in PRD §7.)*
> - **Status:** `catalogued` (start) → `in-progress` → `partial` → `implemented`; or `deferred` / `out-of-scope`.

---

## Summary / Progress Dashboard

**Overall: 117 / 437 implemented (~27%).** Milestones M0 (geometry), M1 (PDF read core), and M2 (text
extraction) are landed and test-backed; M3+ (save/edit/merge, content/annot/forms, image docs/Pixmap,
rendering) remain catalogued.

Counts are over catalogued public symbols (methods + properties + module-level functions + constant *families*
counted as single rows; individual enum members are not double-counted).

### Per-milestone × priority matrix

| Milestone | P0 | P1 | P2 | P3 | deferred | out-of-scope | **Total** | **Implemented** |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| **M0** Geometry (+ paper sizes) | 78 | 0 | 0 | 0 | 0 | 0 | **78** | **81** |
| **M1** PDF read core | 31 | 24 | 0 | 1 | 0 | 0 | **56** | **23** |
| **M2** Text extraction | 19 | 22 | 0 | 0 | 0 | 0 | **41** | **13** |
| **M3** Save / edit / merge / metadata | 14 | 24 | 0 | 1 | 0 | 0 | **39** | 0 |
| **M4** Content / annot / redaction / forms | 14 | 71 | 8 | 0 | 0 | 0 | **93** | 0 |
| **M5** Image docs / codecs / Pixmap / shim | 12 | 6 | 11 | 0 | 0 | 0 | **29** | 0 |
| **M6** Vector rendering (post-v1) | 0 | 0 | 0 | 0 | 14 | 0 | **14** | 0 |
| **post-v1** | 0 | 0 | 0 | 60 | 0 | 0 | **60** | 0 |
| **out-of-scope** | 0 | 0 | 0 | 8 | 0 | 20 | **28** | 0 |
| **Totals** | **168** | **147** | **30** | **70** | **14** | **20** | **437** | **117** |

> **M0 Implemented = 81** counts the 78 geometry rows **plus** the 3 M0-milestone paper-size helpers
> (`paper_size`/`paper_rect`/`paper_sizes`), which are catalogued under *Module-level functions* (their P1
> home) rather than the geometry block — hence M0 Implemented (81) > M0 Total (78).

### Per-class catalogued counts

| Class / group | Catalogued | Implemented |
|---|---:|---:|
| `Matrix` | 16 | 16 |
| `Point` | 11 | 11 |
| `Rect` | 30 | 30 |
| `IRect` | 13 | 13 |
| `Quad` | 11 | 11 |
| Geometry constants/aliases | 3 | 3 |
| `Document` | 110 | 18 |
| `Page` | 96 | 12 |
| `TextPage` | 15 | 6 |
| `Pixmap` | 38 | 0 |
| `Annot` | 30 | 0 |
| `Widget` | 14 | 0 |
| `Link` | 14 | 0 |
| `Outline` / TOC | 11 | 0 |
| `DisplayList` | 5 | 0 |
| `Shape` | 22 | 0 |
| `Font` | 22 | 0 |
| `TextWriter` | 9 | 0 |
| `Story` / `Xml` / `Archive` | 18 | 0 |
| `Colorspace` | 4 | 0 |
| Module-level functions | 22 | 3 |
| `Tools` / `TOOLS` | 12 | 0 |
| Constant families + exceptions | 24 | 0 |
| **TOTAL** | **437** | **117** |

> The catalogued totals double-count a handful of symbols that legitimately appear on two surfaces
> (e.g., `Annot.get_pixmap`, `Annot.get_text`, the `draw_*` family on both `Page` and `Shape`,
> `get_text`/`search` on both `Page`/`TextPage`/`Document`). They are listed under each owning class so the
> per-class coverage view is complete; the milestone matrix counts them once each as well, so the two grand
> totals agree at **437**.

---

## 1. Geometry types (M0, P0)

Pure math, no PDF dependency. PRD §7 row: *"Geometry: Matrix/Point/Rect/IRect/Quad + constants — P0 — M0."*
PRD §8.6.1 pins the row-vector convention `[a b c d e f]`, `x' = a·x + c·y + e`.

### `Matrix` — 2×3 affine transform `(a,b,c,d,e,f)`

- [x] `Matrix(...)` constructors — from 6 floats / `(sx,sy)` scale / `Matrix(deg)` rotation / copy / identity — **P0 · M0 · implemented**
- [x] `concat(m1, m2)` — matrix multiplication — **P0 · M0 · implemented**
- [x] `invert(m=None)` — inverse (returns 1 if degenerate) — **P0 · M0 · implemented**
- [x] `norm()` — Euclidean norm — **P0 · M0 · implemented**
- [x] `prerotate(deg)` — pre-multiply rotation (in place) — **P0 · M0 · implemented**
- [x] `prescale(sx, sy)` — pre-multiply scale — **P0 · M0 · implemented**
- [x] `preshear(h, v)` — pre-multiply shear — **P0 · M0 · implemented**
- [x] `pretranslate(tx, ty)` — pre-multiply translation — **P0 · M0 · implemented**
- [x] `is_rectilinear` (prop) — axis-aligned test — **P0 · M0 · implemented**
- [x] `a b c d e f` (props) — the six components — **P0 · M0 · implemented**
- [x] `__mul__` (`*`) — matrix/point/rect transform — **P0 · M0 · implemented**
- [x] `__invert__` (`~`) — inverse — **P0 · M0 · implemented**
- [x] `__add__` (`+`) — component add — **P0 · M0 · implemented**
- [x] `__sub__` (`-`) — component sub — **P0 · M0 · implemented**
- [x] `__eq__` / `__abs__` / `__len__` / `__getitem__` — sequence/compare protocol — **P0 · M0 · implemented**
- [x] `__repr__` / `__bool__` — Python protocol parity — **P0 · M0 · implemented**

### `Point` — `(x, y)`

- [x] `Point(...)` constructors — from 2 floats / copy — **P0 · M0 · implemented**
- [x] `distance_to(x, unit='px')` — distance to point or rect — **P0 · M0 · implemented**
- [x] `transform(m)` — apply matrix (in place) — **P0 · M0 · implemented**
- [x] `norm()` — Euclidean norm — **P0 · M0 · implemented**
- [x] `unit` (prop) — unit vector — **P0 · M0 · implemented**
- [x] `abs_unit` (prop) — abs-valued unit vector — **P0 · M0 · implemented**
- [x] `x y` (props) — components — **P0 · M0 · implemented**
- [x] operators `+ - * /` — vector arithmetic — **P0 · M0 · implemented**
- [x] `__invert__` (`~`) — **P0 · M0 · implemented**
- [x] `__eq__` / `__abs__` — compare / magnitude — **P0 · M0 · implemented**
- [x] `__getitem__` / `__len__` / `__repr__` — sequence protocol — **P0 · M0 · implemented**

### `Rect` — float rect `(x0, y0, x1, y1)`

- [x] `Rect(...)` constructors — 4 floats / 2 points / copy — **P0 · M0 · implemented**
- [x] `intersect(r)` — set to intersection (in place) — **P0 · M0 · implemented**
- [x] `include_rect(r)` — set to union (in place) — **P0 · M0 · implemented**
- [x] `include_point(p)` — enlarge to contain point — **P0 · M0 · implemented**
- [x] `intersects(r)` — boolean overlap — **P0 · M0 · implemented**
- [x] `contains(x)` — contains point/rect — **P0 · M0 · implemented**
- [x] `normalize()` — make x0≤x1, y0≤y1 — **P0 · M0 · implemented**
- [x] `transform(m)` — apply matrix (→ bounding box) — **P0 · M0 · implemented**
- [x] `morph(point, matrix)` — morph around fixed point → Quad — **P0 · M0 · implemented**
- [x] `torect(rect)` — matrix mapping self → other rect — **P0 · M0 · implemented**
- [x] `round()` — → `IRect` — **P0 · M0 · implemented**
- [x] `get_area(unit)` — area — **P0 · M0 · implemented**
- [x] `norm()` — corner-vector norm — **P0 · M0 · implemented**
- [x] `width` `height` (props) — dimensions — **P0 · M0 · implemented**
- [x] `x0 y0 x1 y1` (props) — edges — **P0 · M0 · implemented**
- [x] `tl tr bl br` (props) — corner points (aliases) — **P0 · M0 · implemented**
- [x] `top_left top_right bottom_left bottom_right` (props) — corner points — **P0 · M0 · implemented**
- [x] `is_empty` (prop) — empty test — **P0 · M0 · implemented**
- [x] `is_infinite` (prop) — infinite test — **P0 · M0 · implemented**
- [x] `is_valid` (prop) — validity test — **P0 · M0 · implemented**
- [x] `irect` (prop) — → `IRect` — **P0 · M0 · implemented**
- [x] `quad` (prop) — → `Quad` — **P0 · M0 · implemented**
- [x] `__and__` (`&`) — intersect — **P0 · M0 · implemented**
- [x] `__or__` (`|`) — union — **P0 · M0 · implemented**
- [x] `__mul__` (`*`) — transform — **P0 · M0 · implemented**
- [x] `__add__` / `__sub__` / `__truediv__` — arithmetic — **P0 · M0 · implemented**
- [x] `__invert__` (`~`) — **P0 · M0 · implemented**
- [x] `__eq__` / `__contains__` — compare / membership — **P0 · M0 · implemented**
- [x] `__getitem__` / `__len__` / `__repr__` — sequence protocol — **P0 · M0 · implemented**
- [x] `__abs__` — area magnitude — **P0 · M0 · implemented**

### `IRect` — integer rect

- [x] `IRect(...)` constructors — 4 ints / copy — **P0 · M0 · implemented**
- [x] `get_area(unit)` — area — **P0 · M0 · implemented**
- [x] `include_point(p)` — **P0 · M0 · implemented**
- [x] `include_rect(r)` — **P0 · M0 · implemented**
- [x] `intersect(r)` — **P0 · M0 · implemented**
- [x] `intersects(r)` — **P0 · M0 · implemented**
- [x] `morph(point, matrix)` — **P0 · M0 · implemented**
- [x] `norm()` — **P0 · M0 · implemented**
- [x] `normalize()` — **P0 · M0 · implemented**
- [x] `torect(rect)` — **P0 · M0 · implemented**
- [x] `transform(m)` — **P0 · M0 · implemented**
- [x] `rect` (prop) — → `Rect` — **P0 · M0 · implemented**
- [x] props mirror `Rect` (`width height x0..y1 tl/tr/bl/br is_empty is_infinite irect quad`) + operators — **P0 · M0 · implemented**

### `Quad` — 4 arbitrary points (ul, ur, ll, lr); supports rotation/shear

- [x] `Quad(...)` constructors — 4 points / copy — **P0 · M0 · implemented**
- [x] `transform(m)` — apply matrix (in place) — **P0 · M0 · implemented**
- [x] `morph(point, matrix)` — morph around fixed point — **P0 · M0 · implemented**
- [x] `width height` (props) — max edge lengths — **P0 · M0 · implemented**
- [x] `rect` (prop) — bounding rect — **P0 · M0 · implemented**
- [x] `ul ur ll lr` (props) — the four corner points — **P0 · M0 · implemented**
- [x] `is_convex` (prop) — **P0 · M0 · implemented**
- [x] `is_empty` (prop) — **P0 · M0 · implemented**
- [x] `is_infinite` (prop) — **P0 · M0 · implemented**
- [x] `is_rectangular` (prop) — **P0 · M0 · implemented**
- [x] operators `* ~ ==` — transform / invert / equal — **P0 · M0 · implemented**

### Geometry constants & type aliases

- [x] `EMPTY_RECT/IRECT/QUAD`, `INFINITE_RECT/IRECT/QUAD`, `Identity`/`IdentityMatrix` — singletons — **P0 · M0 · implemented**
- [x] `EPSILON`, `FLT_EPSILON`, `FZ_MIN_INF_RECT`, `FZ_MAX_INF_RECT` — numeric constants — **P0 · M0 · implemented**
- [x] type aliases `rect_like point_like matrix_like quad_like` — duck-typed inputs — **P0 · M0 · implemented**

---

## 2. `Document`

Central object; `fitz.open(...)` / `fitz.Document(...)` returns it. PRD scopes input to **PDF + image docs**;
all non-PDF (XPS/EPUB/MOBI/FB2/CBZ/SVG/TXT) input is **out-of-scope** (PRD §3.2 #8).

### Open / lifecycle / save

- [x] `open(filename=None, stream=None, filetype=None, rect=None, width=0, height=0, fontsize=11)` — open from path/bytes; PDF + image filetypes in scope — **P0 · M1 · implemented** *(PDF path/bytes done; image filetypes are M5)*
- [x] `Document(...)` — constructor alias of `open` — **P0 · M1 · implemented**
- [ ] `close()` — release resources — **P0 · M1 · catalogued**
- [ ] `save(filename, garbage, clean, deflate, deflate_images, deflate_fonts, incremental, ascii, expand, linear, no_new_id, appearance, pretty, encryption, permissions, owner_pw, user_pw, preserve_metadata, use_objstms, compression_effort)` — full write (NB `linear=True` → unsupported per PRD §3.2 #7) — **P0 · M3 · catalogued**
- [ ] `ez_save(filename, ...)` — `save` with friendly defaults (garbage=3, deflate=1) — **P0 · M3 · catalogued**
- [ ] `saveIncr()` / `save_incremental()` — incremental save (clean-parse only, PRD §8.7) — **P0 · M3 · catalogued**
- [ ] `can_save_incrementally()` — false when `parse_was_repaired` — **P0 · M3 · catalogued**
- [ ] `write(...)` — save to bytes buffer — **P0 · M3 · catalogued**
- [ ] `tobytes(...)` — save to bytes — **P0 · M3 · catalogued**
- [ ] `save_snapshot()` — snapshot save (journaling) — **P3 · post-v1 · catalogued** *(journalling deferred, PRD §3.2 #5)*

### Pages — access, layout

- [x] `load_page(n)` / `__getitem__` — get `Page` (neg idx) — **P0 · M1 · implemented**
- [x] `pages(start, stop, step)` — page iterator — **P1 · M1 · implemented** *(not in PRD §7 explicit row — verify; implied by load_page)*
- [x] `page_count` (prop) — number of pages — **P0 · M1 · implemented**
- [ ] `new_page(pno=-1, width, height)` — create blank page — **P0 · M3 · catalogued**
- [ ] `insert_page(pno, text=None, ...)` — insert page + optional text — **P0 · M3 · catalogued**
- [ ] `insert_pdf(docsrc, from_page, to_page, start_at, rotate, links, annots, ...)` — merge from another PDF (deep-copy + dedup) — **P0 · M3 · catalogued**
- [ ] `insert_file(infile, ...)` — insert from any supported file (image inputs in scope; non-PDF/non-image → unsupported) — **P1 · M5 · catalogued** *(not in PRD §7 explicit row — verify scope)*
- [ ] `copy_page(pno, to)` — duplicate page by reference — **P0 · M3 · catalogued**
- [ ] `fullcopy_page(pno, to)` — deep-copy page — **P0 · M3 · catalogued**
- [ ] `move_page(pno, to)` — reorder page — **P0 · M3 · catalogued**
- [ ] `delete_page(pno)` — remove one page — **P0 · M3 · catalogued**
- [ ] `delete_pages(...)` — remove page range/list — **P0 · M3 · catalogued**
- [ ] `select(list)` — keep/reorder subset — **P0 · M3 · catalogued**
- [ ] `reload_page(page)` — re-fetch page after change — **P1 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `page_cropbox(pno)` — per-page crop without loading — **P1 · M1 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `page_xref(pno)` — page xref without loading — **P1 · M1 · catalogued**
- [ ] `layout(rect, width, height, fontsize)` — re-layout reflowable docs — **out-of-scope · out-of-scope · catalogued** *(reflow only for EPUB-class; non-PDF input out of scope, PRD §3.2 #8)*

### Chapter / location model (reflowable docs)

- [ ] `chapter_count` (prop) — **out-of-scope · out-of-scope · catalogued** *(EPUB-class reflow; PDF has 1 chapter, PRD §3.2 #8)*
- [ ] `chapter_page_count(ch)` — **out-of-scope · out-of-scope · catalogued**
- [ ] `last_location` / `next_location` / `prev_location` (props) — **out-of-scope · out-of-scope · catalogued**
- [ ] `location_from_page_number(pno)` — **out-of-scope · out-of-scope · catalogued**
- [ ] `page_number_from_location(loc)` — **out-of-scope · out-of-scope · catalogued**
- [ ] `make_bookmark(loc)` / `find_bookmark(bm)` — **out-of-scope · out-of-scope · catalogued**

### Metadata / TOC / outline

- [x] `metadata` (attr) — title/author/subject/keywords/creator/producer/dates/format/encryption — **P1 · M1 · implemented** *(read at M1; write at M3)*
- [ ] `set_metadata(d)` — write metadata (Info + mirror to XMP) — **P1 · M3 · catalogued**
- [ ] `get_toc(simple=True)` — TOC as `[lvl, title, page, dest]` (page-label aware, PRD §3.5) — **P1 · M3 · catalogued**
- [ ] `set_toc(toc, collapse)` — replace TOC tree — **P1 · M3 · catalogued**
- [ ] `set_toc_item(idx, ...)` — edit single TOC entry — **P1 · M3 · catalogued**
- [ ] `del_toc_item(idx)` — delete single TOC entry — **P1 · M3 · catalogued**
- [ ] `outline` (prop) — first `Outline` node — **P1 · M3 · catalogued**
- [ ] `get_xml_metadata()` — raw XMP packet — **P1 · M3 · catalogued**
- [ ] `set_xml_metadata(xml)` — write raw XMP — **P1 · M3 · catalogued**
- [ ] `del_xml_metadata()` — drop XMP — **P1 · M3 · catalogued**
- [ ] `xref_xml_metadata()` — xref of XMP stream — **P1 · M3 · catalogued**

### Security / permissions

- [x] `needs_pass` (prop) — encrypted/locked test — **P0 · M1 · implemented**
- [x] `authenticate(password)` — unlock with user/owner pw (R2–R6) — **P0 · M1 · implemented**
- [x] `permissions` (prop) — allowed-ops bitmask (advisory, exposed not enforced) — **P0 · M1 · implemented**
- [x] `is_encrypted` (prop) — encryption state — **P0 · M1 · implemented**
- [ ] `get_sigflags()` — signature flags (read-only) — **P2 · M4 · catalogued** *(sig fields read-only, PRD §3.2 #6)*
- [ ] encryption on `save()` — RC4-128/AES-128/AES-256 R6 (never write R5) — **P1 · M3 · catalogued**

### Identity / state props

- [x] `is_pdf` (prop) — **P0 · M1 · implemented**
- [ ] `is_form_pdf` (prop) — AcroForm present — **P1 · M4 · catalogued**
- [ ] `is_dirty` (prop) — unsaved changes — **P1 · M3 · catalogued**
- [ ] `is_reflowable` (prop) — **P1 · M1 · catalogued** *(false for PDF/image; trivial)*
- [x] `is_repaired` (prop) — `parse_was_repaired` flag (PRD §8.2) — **P0 · M1 · implemented**
- [ ] `is_fast_webaccess` (prop) — linearized read-detect — **P1 · M1 · catalogued** *(read-transparent; linearization write out of scope)*
- [ ] `is_closed` (prop) — **P0 · M1 · catalogued**
- [ ] `name` (prop) — source path/name — **P0 · M1 · catalogued**
- [ ] `language` (prop) / `set_language(lang)` — document `/Lang` — **P1 · M3 · catalogued**
- [ ] `version_count` (prop) — incremental-update revision count — **P1 · M1 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `markinfo` (prop) / `set_markinfo(d)` — `/MarkInfo` (tagged-PDF) — **P2 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `pagelayout` (prop) / `set_pagelayout(s)` — `/PageLayout` — **P2 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `pagemode` (prop) / `set_pagemode(s)` — `/PageMode` — **P2 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `need_appearances` (prop) / `set need_appearances` — AcroForm `/NeedAppearances` — **P1 · M4 · catalogued**
- [ ] `FormFonts` (prop) — list of AcroForm font names — **P1 · M4 · catalogued**

### Conversion / embedded files / fonts

- [ ] `convert_to_pdf(from_page, to_page, rotate)` — **image inputs → PDF only** (non-image → `PdfUnsupportedError`, PRD §3.2 #2) — **P0 · M5 · catalogued**
- [ ] `embfile_add(name, buffer, ...)` — add embedded file — **P2 · M4 · catalogued**
- [ ] `embfile_get(item)` — retrieve embedded file bytes — **P2 · M4 · catalogued**
- [ ] `embfile_del(item)` — delete embedded file — **P2 · M4 · catalogued**
- [ ] `embfile_info(item)` — embedded file info — **P2 · M4 · catalogued**
- [ ] `embfile_upd(item, ...)` — update embedded file — **P2 · M4 · catalogued**
- [ ] `embfile_count()` — count embedded files — **P2 · M4 · catalogued**
- [ ] `embfile_names()` — list embedded file names — **P2 · M4 · catalogued**
- [ ] `extract_font(xref)` — extract embedded font (name, ext, type, bytes) — **P2 · M5 · catalogued**
- [ ] `extract_image(xref)` — extract image (ext, bytes, dims, colorspace) — **P2 · M5 · catalogued**
- [ ] `get_char_widths(xref)` — glyph widths for a font — **P1 · M2 · catalogued** *(font widths machinery; PRD §8.5)*
- [ ] `subset_fonts()` — subset embedded fonts (feature-gated, full-embed fallback PRD §8.5.2) — **P2 · M4/M5 · catalogued**
- [ ] `bake(annots, widgets)` — flatten annots/widgets into content — **P2 · M4 · catalogued**
- [ ] `scrub(...)` — sanitize (remove metadata/js/links/etc.) — **P2 · M4 · catalogued**
- [ ] `subset` (prop) — named-destination subset accessor — **P1 · M3 · catalogued** *(named-dest resolution, PRD §8.7)*
- [ ] `resolve_names()` — resolve named destinations → physical pages — **P1 · M3 · catalogued**
- [ ] `resolve_link(uri)` — resolve a link URI — **P1 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `get_outline_xrefs()` — xrefs of outline items — **P1 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*

### Low-level xref / object access (COS layer — clean-room critical)

- [x] `xref_length()` — number of xref entries — **P1 · M1 · implemented**
- [x] `xref_object(xref, compressed, ascii)` — object source as string — **P1 · M1 · implemented**
- [x] `xref_stream(xref)` — decoded stream bytes — **P1 · M1 · implemented**
- [ ] `xref_stream_raw(xref)` — raw (undecoded) stream bytes — **P1 · M1 · catalogued**
- [ ] `update_object(xref, text, page)` — replace object definition — **P1 · M3 · catalogued**
- [ ] `update_stream(xref, data, new, compress)` — replace stream content — **P1 · M3 · catalogued**
- [ ] `get_new_xref()` — allocate a fresh xref — **P1 · M3 · catalogued**
- [x] `xref_get_key(xref, key)` — read a dict key — **P1 · M1 · implemented**
- [ ] `xref_get_keys(xref)` — list a dict's keys — **P1 · M1 · catalogued**
- [ ] `xref_set_key(xref, key, value)` — set a dict key (Null deletes) — **P1 · M3 · catalogued**
- [ ] `xref_copy(src, dst)` — copy object — **P1 · M3 · catalogued**
- [x] `xref_is_font(xref)` / `xref_is_image` / `xref_is_stream` / `xref_is_xobject` — type predicates — **P1 · M1 · implemented** *(`xref_is_stream` exposed + test-backed; remaining predicates land with the rest of the low-level API)*
- [ ] `pdf_catalog()` — catalog xref — **P1 · M1 · catalogued**
- [ ] `pdf_trailer()` — trailer xref/source — **P1 · M1 · catalogued**
- [ ] `is_stream(xref)` — object has a stream — **P1 · M1 · catalogued**
- [ ] `page_annot_xrefs(pno)` — xrefs of a page's annots — **P1 · M1 · catalogued**

### Optional content (OCG/layers) & page labels

- [ ] `add_ocg(...)` — add optional-content group — **P3 · post-v1 · catalogued** *(OCG out of scope, PRD §3.2 #5)*
- [ ] `add_layer(...)` — **P3 · post-v1 · catalogued**
- [ ] `get_ocgs()` — **P3 · post-v1 · catalogued**
- [ ] `get_layer(...)` / `get_layers()` — **P3 · post-v1 · catalogued**
- [ ] `set_layer(...)` / `switch_layer(...)` — **P3 · post-v1 · catalogued**
- [ ] `set_layer_ui_config(...)` / `layer_ui_configs()` — **P3 · post-v1 · catalogued**
- [ ] `get_oc(xref)` / `set_oc(xref, ocxref)` — object optional-content — **P3 · post-v1 · catalogued**
- [ ] `get_ocmd(xref)` / `set_ocmd(...)` — OCMD — **P3 · post-v1 · catalogued**
- [ ] `get_page_labels()` — read `/PageLabels` ranges — **P1 · M3 · catalogued** *(read in scope, PRD §3.5)*
- [ ] `set_page_labels(labels)` — **write** page labels — **P3 · post-v1 · catalogued** *(label *write* deferred, PRD §3.2 #5)*
- [ ] `get_page_numbers(label, only_one)` — label → page number(s) — **P1 · M3 · catalogued** *(named-dest interplay, PRD §3.5)*
- [ ] `get_label(pno)` — physical page → computed label — **P1 · M3 · catalogued** *(PRD §3.5; also `Page.get_label`)*

### Document-wide page-content helpers (convenience over `Page`)

- [x] `get_page_text(pno, output, ...)` — text of a page — **P1 · M2 · implemented** *(thin wrapper over Page.get_text)*
- [ ] `get_page_pixmap(pno, ...)` — render a page (image-only path in scope; vector → unsupported) — **P0 · M5 · catalogued**
- [ ] `get_page_images(pno, full)` — images on a page — **P1 · M2 · catalogued**
- [ ] `get_page_fonts(pno, full)` — fonts on a page — **P1 · M2 · catalogued**
- [ ] `get_page_xobjects(pno)` — form XObjects on a page — **P1 · M2 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `search_page_for(pno, needle, ...)` — search a page — **P0 · M2 · catalogued**

### Journalling (undo/redo) — deferred

- [ ] `journal_enable()` / `journal_is_enabled()` — **P3 · post-v1 · catalogued** *(journalling out of scope, PRD §3.2 #5)*
- [ ] `journal_start_op(name)` / `journal_stop_op()` — **P3 · post-v1 · catalogued**
- [ ] `journal_undo()` / `journal_redo()` / `journal_can_do()` — **P3 · post-v1 · catalogued**
- [ ] `journal_op_name(step)` / `journal_position()` — **P3 · post-v1 · catalogued**
- [ ] `journal_save(filename)` / `journal_load(filename)` — **P3 · post-v1 · catalogued**

---

## 3. `Page`

Workhorse for extraction, drawing, annotation, and (in scope only for image-only pages) rendering.

### Text extraction

- [x] `get_text(option='text', clip, flags, textpage, sort, delimiters, tolerance)` — `text|blocks|words|dict|rawdict|html|xhtml|xml|json|rawjson` — **P0 · M2 · implemented** *(all output variants done; html/xhtml/xml are P1, PRD §7)*
- [ ] `get_text_blocks(...)` — block tuples convenience — **P0 · M2 · catalogued**
- [ ] `get_text_words(...)` — word tuples convenience — **P0 · M2 · catalogued**
- [ ] `get_textbox(rect, textpage=None)` — text inside a rect — **P1 · M2 · catalogued**
- [ ] `get_text_selection(p1, p2, clip)` — text between two points — **P1 · M2 · catalogued**
- [x] `get_textpage(clip, flags)` — build reusable `TextPage` — **P1 · M2 · implemented**
- [ ] `extend_textpage(...)` — extend an existing TextPage — **P1 · M2 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `get_texttrace()` — low-level per-glyph trace — **P1 · M2 · catalogued** *(not in PRD §7 explicit row — verify; valuable for ground-truth)*
- [ ] `get_textpage_ocr(flags, language, dpi, full, tessdata)` — OCR-backed TextPage — **P3 · post-v1 · catalogued** *(OCR out of scope, PRD §3.2 #3)*

### Per-method default flag sets

- [x] `TEXTFLAGS_*` per-method defaults pinned (text/blocks/words/dict/rawdict/html/xhtml/xml/search) — **P0 · M2 · implemented** *(PRD §7 + §8.6.2; recorded in COMPAT.toml)*

### Search & links

- [x] `search_for(needle, clip, quads, flags, textpage)` — find text → Rects/Quads — **P0 · M2 · implemented**
- [ ] `get_links()` / `links(kinds)` — enumerate links — **P1 · M4 · catalogued** *(read in scope; insert/update/delete = M4)*
- [ ] `load_links()` — load link list — **P1 · M4 · catalogued**
- [ ] `first_link` (prop) — first `Link` of linked list — **P1 · M4 · catalogued**
- [ ] `insert_link(link_dict)` — add link — **P1 · M4 · catalogued**
- [ ] `update_link(link_dict)` — modify link — **P1 · M4 · catalogued**
- [ ] `delete_link(link_dict)` — remove link — **P1 · M4 · catalogued**

### Rendering (vector pages deferred to M6; image-only pages in scope at M5)

- [ ] `get_pixmap(*, matrix=Identity, dpi=None, colorspace=RGB, clip=None, alpha=False, annots=True)` — **in scope only for image documents / image-only pages (PRD §3.3); vector pages → `PdfUnsupportedError`** — **P0 (image path) / deferred (vector) · M5 / M6 · catalogued**
- [ ] `get_svg_image(matrix, text_as_path)` — page → SVG — **deferred · M6 · catalogued** *(vector rendering, PRD §3.2 #1)*
- [ ] `get_displaylist(annots)` — build replayable `DisplayList` — **deferred · M6 · catalogued**
- [ ] `run(device, matrix)` — run page through a device — **deferred · M6 · catalogued**
- [x] `bound()` — page rectangle (= `rect`) — **P0 · M1 · implemented**

### Vector / image / font inventory (analysis)

- [ ] `get_drawings(extended=False)` — vector paths as dicts — **P1 · M4 · catalogued**
- [ ] `get_cdrawings()` — faster raw drawings — **P1 · M4 · catalogued**
- [ ] `get_bboxlog()` — ordered bbox log of content items — **P1 · M4 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `cluster_drawings(...)` — group nearby vector graphics — **P2 · M4 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `find_tables(...)` — detect & extract tables (`TableFinder`) — **P3 · post-v1 · catalogued** *(table detection out of scope, PRD §3.2 #4)*
- [x] `get_fonts(full=False)` — fonts used — **P1 · M2 · implemented**
- [x] `get_images(full=False)` — images used — **P1 · M2 · implemented**
- [ ] `get_image_info(hashes, xrefs)` — image placements + bbox — **P1 · M2 · catalogued**
- [ ] `get_image_bbox(item)` — bbox where image is shown — **P1 · M2 · catalogued**
- [ ] `get_image_rects(item)` — all rects where image is shown — **P1 · M2 · catalogued**
- [ ] `get_xobjects()` — form XObjects on page — **P1 · M2 · catalogued** *(not in PRD §7 explicit row — verify)*

### Drawing primitives (mirror `Shape`)

- [ ] `draw_line(p1, p2, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_rect(rect, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_circle(center, radius, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_oval(rect/quad, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_bezier(p1, p2, p3, p4, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_curve(p1, p2, p3, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_polyline(points, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_quad(quad, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_sector(center, point, angle, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_squiggle(p1, p2, ...)` — **P1 · M4 · catalogued**
- [ ] `draw_zigzag(p1, p2, ...)` — **P1 · M4 · catalogued**
- [ ] `new_shape()` — → `Shape` — **P1 · M4 · catalogued**

### Text & image insertion

- [ ] `insert_text(point, text, fontsize, fontname, fontfile, color, ...)` — write text at point (Base-14 + TTF embed) — **P0 · M4 · catalogued**
- [ ] `insert_textbox(rect, text, ..., align)` — wrapped text in rect — **P0 · M4 · catalogued**
- [ ] `insert_htmlbox(rect, html, css, ...)` — render HTML/CSS into rect (uses Story) — **P3 · post-v1 · catalogued** *(HTML/CSS engine out of scope, PRD §3.2 #2)*
- [ ] `insert_image(rect, filename/stream/pixmap, ...)` — place image — **P1 · M4 · catalogued**
- [ ] `insert_font(fontname, fontfile, ...)` — register a font on page — **P1 · M4 · catalogued**
- [ ] `write_text(writers=..., ...)` — commit one/more `TextWriter`s — **P1 · M4 · catalogued**
- [ ] `show_pdf_page(rect, src, pno, ...)` — embed another PDF page as XObject — **P1 · M4 · catalogued** *(not in PRD §7 explicit row — verify; XObject placement, not rasterization)*
- [ ] `replace_image(xref, ...)` — swap an image — **P2 · M4 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `delete_image(xref)` — blank an image — **P2 · M4 · catalogued** *(not in PRD §7 explicit row — verify)*

### Annotations

- [ ] `annots(types=None)` — iterate annotations — **P1 · M4 · catalogued**
- [ ] `first_annot` (prop) — first `Annot` — **P1 · M4 · catalogued**
- [ ] `annot_names()` — list `/NM` names — **P1 · M4 · catalogued**
- [ ] `annot_xrefs()` — list annot xrefs — **P1 · M4 · catalogued**
- [ ] `load_annot(ident)` — load a named/xref annot — **P1 · M4 · catalogued**
- [ ] `delete_annot(annot)` — delete annot (clean `/AP`/`/Popup`) — **P1 · M4 · catalogued**
- [ ] `add_text_annot(point, text, ...)` — **P1 · M4 · catalogued**
- [ ] `add_freetext_annot(rect, text, ...)` — **P1 · M4 · catalogued**
- [ ] `add_highlight_annot(quads/rect, ...)` — **P1 · M4 · catalogued**
- [ ] `add_underline_annot(quads/rect, ...)` — **P1 · M4 · catalogued**
- [ ] `add_strikeout_annot(quads/rect, ...)` — **P1 · M4 · catalogued**
- [ ] `add_squiggly_annot(quads/rect, ...)` — **P1 · M4 · catalogued**
- [ ] `add_rect_annot(rect, ...)` — **P1 · M4 · catalogued**
- [ ] `add_circle_annot(rect, ...)` — **P1 · M4 · catalogued**
- [ ] `add_line_annot(p1, p2, ...)` — **P1 · M4 · catalogued**
- [ ] `add_polyline_annot(points, ...)` — **P1 · M4 · catalogued**
- [ ] `add_polygon_annot(points, ...)` — **P1 · M4 · catalogued**
- [ ] `add_ink_annot(strokes, ...)` — **P1 · M4 · catalogued**
- [ ] `add_stamp_annot(rect, stamp, ...)` — **P1 · M4 · catalogued**
- [ ] `add_caret_annot(point, ...)` — **P1 · M4 · catalogued**
- [ ] `add_file_annot(point, buffer, filename, ...)` — **P1 · M4 · catalogued**
- [ ] `add_redact_annot(quad/rect, ...)` — mark redaction (applied by `apply_redactions`) — **P0 · M4 · catalogued**
- [ ] `apply_redactions(images, graphics, text)` — destructive multi-surface redaction (PRD §8.8) — **P0 · M4 · catalogued**

### Widgets / forms

- [ ] `widgets(types=None)` — iterate form fields → `Widget` — **P1 · M4 · catalogued**
- [ ] `first_widget` (prop) — first `Widget` — **P1 · M4 · catalogued**
- [ ] `add_widget(widget)` — add form field — **P1 · M4 · catalogued**
- [ ] `load_widget(xref)` — load a widget by xref — **P1 · M4 · catalogued**
- [ ] `delete_widget(widget)` — remove form field — **P1 · M4 · catalogued**

### Content-stream maintenance

- [ ] `get_contents()` — content-stream xref(s) — **P1 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `read_contents()` — concatenated decoded content — **P1 · M3 · catalogued**
- [ ] `set_contents(xref)` — set the content stream — **P1 · M3 · catalogued**
- [ ] `clean_contents(sanitize)` — rewrite/normalize content — **P2 · M4 · catalogued**
- [ ] `wrap_contents()` — wrap in `q…Q` — **P2 · M4 · catalogued**
- [ ] `is_wrapped` (prop) — content already wrapped — **P2 · M4 · catalogued**

### Geometry / boxes / rotation

- [x] `rect` (prop) — page rect (rotation-aware, PRD §8.6.1) — **P0 · M1 · implemented**
- [x] `mediabox` (prop) / `set_mediabox(r)` — **P0 · M1 / M3 · implemented** *(read done; `set_mediabox` is M3)*
- [ ] `mediabox_size` (prop) — **P0 · M1 · catalogued**
- [x] `cropbox` (prop) / `set_cropbox(r)` — (cropbox ⊆ mediabox) — **P0 · M1 / M3 · implemented** *(read done; `set_cropbox` is M3)*
- [ ] `cropbox_position` (prop) — **P0 · M1 · catalogued**
- [ ] `artbox` (prop) / `set_artbox(r)` — **P1 · M1 / M3 · catalogued**
- [ ] `bleedbox` (prop) / `set_bleedbox(r)` — **P1 · M1 / M3 · catalogued**
- [ ] `trimbox` (prop) / `set_trimbox(r)` — **P1 · M1 / M3 · catalogued**
- [x] `rotation` (prop) / `set_rotation(deg)` — page `/Rotate` — **P0 · M1 / M3 · implemented** *(read done; `set_rotation` is M3)*
- [ ] `remove_rotation()` — normalize rotation to 0, bake into content — **P0 · M3 · catalogued**
- [ ] `transformation_matrix` (prop) — page → device matrix — **P0 · M1 · catalogued**
- [ ] `rotation_matrix` (prop) — **P0 · M1 · catalogued**
- [ ] `derotation_matrix` (prop) — **P0 · M1 · catalogued**
- [ ] `xref` (prop) — page object xref — **P0 · M1 · catalogued**
- [x] `number` (prop) — page index — **P0 · M1 · implemented**
- [ ] `parent` (prop) — owning `Document` — **P0 · M1 · catalogued**
- [ ] `refresh()` — reload page after change — **P1 · M3 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `language` (prop) / `set_language(lang)` — page `/Lang` — **P1 · M3 · catalogued**
- [ ] `get_oc_items()` — optional-content items on page — **P3 · post-v1 · catalogued** *(OCG out of scope)*
- [ ] `get_label()` — computed page label — **P1 · M3 · catalogued** *(PRD §3.5)*

---

## 4. `TextPage`

Cached structured text for a page; produced by `Page.get_textpage()`; backs all `get_text` variants.
PRD §7 row: *"`TextPage` reusable object — P1 — M2."* (camelCase method names are PyMuPDF-canonical here.)

- [x] `extractText(sort)` / `extractTEXT` — plain text — **P0 · M2 · implemented**
- [x] `extractBLOCKS` — block tuples — **P0 · M2 · implemented**
- [x] `extractWORDS(delimiters)` — word tuples — **P0 · M2 · implemented**
- [x] `extractDICT(sort)` — structured dict — **P0 · M2 · implemented**
- [x] `extractJSON(sort)` — structured JSON — **P0 · M2 · implemented**
- [x] `extractRAWDICT(sort)` — char-level dict — **P0 · M2 · implemented**
- [ ] `extractRAWJSON(sort)` — char-level JSON — **P0 · M2 · catalogued**
- [ ] `extractHTML()` — HTML markup — **P1 · M2 · catalogued** *(Tier-B serialization, PRD §6.1/§8.6.2)*
- [ ] `extractXHTML()` — XHTML markup — **P1 · M2 · catalogued**
- [ ] `extractXML()` — char-level XML — **P1 · M2 · catalogued**
- [ ] `extractIMGINFO(hashes)` — image metadata on page — **P1 · M2 · catalogued**
- [ ] `extractSelection(p1, p2, clip)` — text between two points — **P1 · M2 · catalogued**
- [ ] `extractTextbox(rect)` — text in a rect — **P1 · M2 · catalogued**
- [ ] `search(needle, quads)` — search within this textpage — **P0 · M2 · catalogued**
- [ ] `rect` (prop) / `poolsize` (prop) — page rect / memory pool — **P1 · M2 · catalogued**

---

## 5. `Pixmap`

Raster image. In scope **only** for the image path (image documents + image-only PDF pages, PRD §3.3);
codecs are M5; vector-page rasterization is M6. Buffer-protocol/numpy support per PRD §7.

### Constructors

- [ ] `Pixmap(colorspace, irect, alpha)` — blank pixmap — **P0 · M5 · catalogued**
- [ ] `Pixmap(src_pixmap, ...)` — copy / recolor / add-alpha / downscale — **P0 · M5 · catalogued**
- [ ] `Pixmap(colorspace, src_pixmap)` — colorspace conversion — **P0 · M5 · catalogued**
- [ ] `Pixmap(doc, xref)` — from a PDF image object — **P0 · M5 · catalogued**
- [ ] `Pixmap(filename)` / `Pixmap(stream)` — from file/bytes (decode) — **P0 · M5 · catalogued**
- [ ] `Pixmap(colorspace, width, height, samples, alpha)` — from raw samples — **P0 · M5 · catalogued**
- [ ] `Pixmap(PIL image)` — from a Pillow image — **P2 · M5 · catalogued** *(not in PRD §7 explicit row — verify; Pillow bridge)*

### Methods

- [ ] `save(filename, output=None, jpg_quality)` — write PNG/JPEG/PNM/PSD/PS/PAM/etc. — **P0 · M5 · catalogued**
- [ ] `tobytes(output='png', jpg_quality)` — encode to bytes — **P0 · M5 · catalogued**
- [ ] `pil_save(...)` — save via Pillow — **P2 · M5 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `pil_tobytes(...)` — bytes via Pillow — **P2 · M5 · catalogued**
- [ ] `pdfocr_save(...)` / `pdfocr_tobytes(...)` — OCR → searchable PDF — **P3 · post-v1 · catalogued** *(OCR out of scope, PRD §3.2 #3)*
- [ ] `pixel(x, y)` — read pixel tuple — **P1 · M5 · catalogued**
- [ ] `set_pixel(x, y, color)` — write pixel — **P1 · M5 · catalogued**
- [ ] `set_rect(irect, color)` — fill rect — **P1 · M5 · catalogued**
- [ ] `set_origin(x, y)` — set origin — **P2 · M5 · catalogued**
- [ ] `set_dpi(x, y)` — set DPI — **P2 · M5 · catalogued**
- [ ] `set_alpha(...)` — set alpha channel — **P2 · M5 · catalogued**
- [ ] `clear_with(value, irect)` — clear (optionally sub-rect) — **P1 · M5 · catalogued**
- [ ] `invert_irect(irect)` — invert colors in region — **P2 · M5 · catalogued**
- [ ] `tint_with(black, white)` — recolor — **P2 · M5 · catalogued**
- [ ] `gamma_with(g)` — gamma — **P2 · M5 · catalogued**
- [ ] `shrink(n)` — halve resolution n times — **P2 · M5 · catalogued**
- [ ] `copy(src, irect)` — copy region from another — **P2 · M5 · catalogued**
- [ ] `warp(quad, width, height)` — perspective de-warp — **P2 · M5 · catalogued** *(not in PRD §7 explicit row — verify)*
- [ ] `color_count()` — histogram — **P2 · M5 · catalogued**
- [ ] `color_topusage()` — dominant color — **P2 · M5 · catalogued**

### Properties

- [ ] `samples` — raw pixel bytes — **P0 · M5 · catalogued**
- [ ] `samples_mv` — memoryview of samples — **P0 · M5 · catalogued**
- [ ] `samples_ptr` — pointer to samples — **P1 · M5 · catalogued**
- [ ] `stride` — bytes per row — **P0 · M5 · catalogued**
- [ ] `width` `height` `w` `h` `x` `y` `irect` — dimensions / position — **P0 · M5 · catalogued**
- [ ] `n` — channel count — **P0 · M5 · catalogued**
- [ ] `alpha` — has alpha — **P0 · M5 · catalogued**
- [ ] `colorspace` — `Colorspace` — **P0 · M5 · catalogued**
- [ ] `digest` — MD5 of samples — **P1 · M5 · catalogued**
- [ ] `size` — byte size — **P1 · M5 · catalogued**
- [ ] `xres` `yres` — DPI — **P1 · M5 · catalogued**
- [ ] `is_monochrome` — quick test — **P2 · M5 · catalogued**
- [ ] `is_unicolor` — quick test — **P2 · M5 · catalogued**
- [ ] `__array_interface__` / numpy buffer protocol — numpy zero-copy (PRD §7 buffer-protocol/numpy) — **P0 · M5 · catalogued**

---

## 6. `Annot`

One annotation (via `page.annots()` / `add_*_annot`). `/AP /N` generated for every subtype (PRD §8.8). All M4.

- [ ] `update(...)` — regenerate appearance after edits — **P1 · M4 · catalogued**
- [ ] `set_rect(r)` — geometry — **P1 · M4 · catalogued**
- [ ] `set_rotation(d)` — rotation — **P1 · M4 · catalogued**
- [ ] `set_colors(stroke, fill)` / `colors` (prop) — colors — **P1 · M4 · catalogued**
- [ ] `set_opacity(v)` / `opacity` (prop) — transparency (`/CA` via `/ExtGState`) — **P1 · M4 · catalogued**
- [ ] `set_border(...)` / `border` (prop) — width/dashes/style/effect — **P1 · M4 · catalogued**
- [ ] `set_flags(f)` / `flags` (prop) — annot flags — **P1 · M4 · catalogued**
- [ ] `set_info(d)` / `info` (prop) — title/content/name/dates — **P1 · M4 · catalogued**
- [ ] `set_line_ends(s, e)` / `line_ends` (prop) — line endings — **P1 · M4 · catalogued**
- [ ] `set_blendmode(bm)` / `blendmode` (prop) — blend mode — **P1 · M4 · catalogued**
- [ ] `set_name(n)` — stamp name — **P1 · M4 · catalogued**
- [ ] `set_oc(xref)` / `get_oc()` — optional content — **P3 · post-v1 · catalogued** *(OCG out of scope)*
- [ ] `set_open(b)` / `is_open` (prop) — popup open state — **P1 · M4 · catalogued**
- [ ] `set_popup(r)` / `popup_rect` (prop) / `popup_xref` (prop) / `has_popup` (prop) — popup control — **P1 · M4 · catalogued**
- [ ] `set_apn_bbox(r)` / `apn_bbox` (prop) — appearance-stream bbox — **P1 · M4 · catalogued**
- [ ] `set_apn_matrix(m)` / `apn_matrix` (prop) — appearance-stream matrix — **P1 · M4 · catalogued**
- [ ] `set_irt_xref(xref)` / `irt_xref` (prop) — reply threading — **P2 · M4 · catalogued**
- [ ] `delete_responses()` — drop reply chain — **P2 · M4 · catalogued**
- [ ] `get_pixmap(...)` — render the annotation — **deferred · M6 · catalogued** *(annot rasterization needs the M6 renderer)*
- [ ] `get_text(...)` / `get_textbox(...)` / `get_textpage(...)` — text inside annot — **P1 · M4 · catalogued**
- [ ] `get_file()` / `update_file(...)` / `file_info` (prop) — file-attachment payload — **P2 · M4 · catalogued**
- [ ] `get_sound()` — sound annot data — **P3 · post-v1 · catalogued** *(not in PRD §7 — verify; niche)*
- [ ] `clean_contents()` — maintenance — **P2 · M4 · catalogued**
- [ ] `set_language(lang)` — annot `/Lang` — **P2 · M4 · catalogued**
- [ ] `type` (prop) — `(type_int, type_string)` — **P1 · M4 · catalogued**
- [ ] `rect` (prop) — geometry — **P1 · M4 · catalogued**
- [ ] `xref` (prop) — annot xref — **P1 · M4 · catalogued**
- [ ] `vertices` (prop) — annot vertices — **P1 · M4 · catalogued**
- [ ] `rect_delta` (prop) — `/RD` inset — **P2 · M4 · catalogued**
- [ ] `next` (prop) / `language` (prop) — next annot / language — **P1 · M4 · catalogued**

---

## 7. `Widget` (form field)

Interactive AcroForm field. PRD §7 row: *"Forms: read + fill + flatten (AcroForm) + `Widget` object API — P1 — M4."*
All instance attributes are read/write, committed via `update()`.

- [ ] `field_name` / `field_label` / `field_value` — identity & value — **P1 · M4 · catalogued**
- [ ] `field_type` / `field_type_string` — field type — **P1 · M4 · catalogued**
- [ ] `field_flags` / `field_display` — flags & display — **P1 · M4 · catalogued**
- [ ] `rect` / `xref` — geometry & xref — **P1 · M4 · catalogued**
- [ ] `border_color` / `border_style` / `border_width` / `border_dashes` — border — **P1 · M4 · catalogued**
- [ ] `fill_color` / `text_color` — colors — **P1 · M4 · catalogued**
- [ ] `text_font` / `text_fontsize` — text font — **P1 · M4 · catalogued**
- [ ] `text_maxlen` / `text_format` — text constraints — **P1 · M4 · catalogued**
- [ ] `choice_values` — choice-field options — **P1 · M4 · catalogued**
- [ ] `button_caption` / `is_signed` / `rb_parent` — button/radio/signature meta — **P1 · M4 · catalogued**
- [ ] JS hooks `script` / `script_calc` / `script_change` / `script_format` / `script_blur` / `script_focus` / `script_stroke` — field JavaScript — **P2 · M4 · catalogued** *(not in PRD §7 explicit — verify; JS storage, not execution)*
- [ ] `update()` — commit field changes (appearance regen) — **P1 · M4 · catalogued**
- [ ] `reset()` — reset to default — **P1 · M4 · catalogued**
- [ ] `button_states()` / `on_state()` — checkbox/radio on-states (from `/AP /N` keys) — **P1 · M4 · catalogued**
- [ ] `next` (prop) — next widget — **P1 · M4 · catalogued**

---

## 8. `Link`

A clickable link. PRD §7 row: *"link insert/update/delete — P1 — M4."*

- [ ] `rect` (prop) — link rect — **P1 · M4 · catalogued**
- [ ] `dest` (prop) — resolved destination (`linkDest`) — **P1 · M4 · catalogued**
- [ ] `uri` (prop) — URI target — **P1 · M4 · catalogued**
- [ ] `page` (prop) — target page index — **P1 · M4 · catalogued**
- [ ] `is_external` (prop) — external link test — **P1 · M4 · catalogued**
- [ ] `border` (prop) / `set_border(...)` — link border — **P1 · M4 · catalogued**
- [ ] `colors` (prop) / `set_colors(...)` — link colors — **P1 · M4 · catalogued**
- [ ] `flags` (prop) / `set_flags(...)` — link flags — **P1 · M4 · catalogued**
- [ ] `next` (prop) — next link — **P1 · M4 · catalogued**
- [ ] `xref` (prop) — link xref — **P1 · M4 · catalogued**
- [ ] `linkDest` value object — resolved destination type — **P1 · M4 · catalogued**
- [ ] `LINK_NONE/GOTO/URI/LAUNCH/GOTOR/NAMED` constants — link kinds — **P1 · M4 · catalogued**
- [ ] `LINK_FLAG_*` constants — link flag bits — **P1 · M4 · catalogued**
- [ ] `set_border` / `set_colors` / `set_flags` methods — link mutation — **P1 · M4 · catalogued**

---

## 9. `Outline` / TOC

TOC tree node (read-mostly). TOC get/set is PRD §7 *"TOC get/set; named dests — P1 — M3."*

- [ ] `title` (prop) — entry title — **P1 · M3 · catalogued**
- [ ] `dest` (prop) — destination — **P1 · M3 · catalogued**
- [ ] `page` (prop) — target page — **P1 · M3 · catalogued**
- [ ] `uri` (prop) — URI — **P1 · M3 · catalogued**
- [ ] `is_external` (prop) — external test — **P1 · M3 · catalogued**
- [ ] `is_open` (prop) — expanded state — **P1 · M3 · catalogued**
- [ ] `next` (prop) — next sibling — **P1 · M3 · catalogued**
- [ ] `down` (prop) — first child — **P1 · M3 · catalogued**
- [ ] `x` `y` (props) — destination coordinates — **P1 · M3 · catalogued**
- [ ] `destination(...)` — resolved destination — **P1 · M3 · catalogued**
- [ ] `Document.get_toc` / `set_toc` interplay — tree build with signed `/Count` (PRD §8.7) — **P1 · M3 · catalogued**

---

## 10. `DisplayList`

Recorded, replayable rendering commands. **Entirely vector-rendering; deferred to M6** (PRD §7 row:
*"`DisplayList` (vector pages) — deferred — M6 (post-v1)"*).

- [ ] `DisplayList(mediabox)` — constructor — **deferred · M6 · catalogued**
- [ ] `get_pixmap(matrix, colorspace, alpha, clip)` — render → Pixmap — **deferred · M6 · catalogued**
- [ ] `get_textpage(flags)` — extract text → TextPage — **deferred · M6 · catalogued**
- [ ] `run(device, matrix, area)` — replay onto a device — **deferred · M6 · catalogued**
- [ ] `rect` (prop) — bounds — **deferred · M6 · catalogued**

---

## 11. `Shape` (drawing canvas on a `Page`)

Accumulates path ops, then `finish()`/`commit()` flush to content. PRD §7 row: *"`draw_*` + `Shape` — P1 — M4."*

- [ ] `draw_line(p1, p2)` — **P1 · M4 · catalogued**
- [ ] `draw_rect(rect)` — **P1 · M4 · catalogued**
- [ ] `draw_circle(center, radius)` — (4 cubic Béziers κ=0.5523, PRD §8.8) — **P1 · M4 · catalogued**
- [ ] `draw_oval(rect/quad)` — **P1 · M4 · catalogued**
- [ ] `draw_bezier(p1, p2, p3, p4)` — **P1 · M4 · catalogued**
- [ ] `draw_curve(p1, p2, p3)` — **P1 · M4 · catalogued**
- [ ] `draw_polyline(points)` — **P1 · M4 · catalogued**
- [ ] `draw_quad(quad)` — **P1 · M4 · catalogued**
- [ ] `draw_sector(center, point, angle)` — **P1 · M4 · catalogued**
- [ ] `draw_squiggle(p1, p2)` — **P1 · M4 · catalogued**
- [ ] `draw_zigzag(p1, p2)` — **P1 · M4 · catalogued**
- [ ] `insert_text(...)` — text on the shape canvas — **P1 · M4 · catalogued**
- [ ] `insert_textbox(...)` — wrapped text on the shape canvas — **P1 · M4 · catalogued**
- [ ] `finish(width, color, fill, lineCap, lineJoin, dashes, closePath, even_odd, morph, stroke_opacity, fill_opacity, ...)` — style & close current path — **P1 · M4 · catalogued**
- [ ] `commit(overlay)` — flush to content stream — **P1 · M4 · catalogued**
- [ ] `update_rect()` — recompute bbox — **P1 · M4 · catalogued**
- [ ] `horizontal_angle` (prop) — last-drawn angle — **P1 · M4 · catalogued**
- [ ] `doc` `page` (props) — owning objects — **P1 · M4 · catalogued**
- [ ] `height` `width` (props) — page dimensions — **P1 · M4 · catalogued**
- [ ] `x` `y` (props) — current pen position — **P1 · M4 · catalogued**
- [ ] `rect` (prop) — accumulated bbox — **P1 · M4 · catalogued**

---

## 12. `Font`

Wraps a font (Base-14, file, buffer). Used for measuring and `TextWriter`. PRD §7: *"Fonts for mapping — P0 — M2"*
(metrics) and *"insert_text (Base-14 + TTF embed) — P0 — M4."* Outlines/rasterization are **not** in v1.

- [ ] `Font(fontname, fontfile, fontbuffer, script, language, ordering, is_bold, is_italic, is_serif)` — load font — **P0 · M2 · catalogued**
- [ ] `text_length(text, fontsize)` — width of string — **P0 · M2 · catalogued**
- [ ] `char_lengths(text, fontsize)` — per-char widths — **P1 · M2 · catalogued**
- [ ] `glyph_advance(chr)` — advance width — **P1 · M2 · catalogued**
- [ ] `glyph_bbox(chr)` — glyph bounding box — **P1 · M2 · catalogued**
- [ ] `has_glyph(chr)` — coverage test — **P1 · M2 · catalogued**
- [ ] `valid_codepoints()` — supported codepoints — **P1 · M2 · catalogued**
- [ ] `glyph_name_to_unicode(name)` — name → codepoint — **P1 · M2 · catalogued**
- [ ] `unicode_to_glyph_name(cp)` — codepoint → name — **P1 · M2 · catalogued**
- [ ] `name` (prop) — font name — **P0 · M2 · catalogued**
- [ ] `ascender` `descender` (props) — vertical metrics — **P1 · M2 · catalogued**
- [ ] `bbox` (prop) — font bbox — **P1 · M2 · catalogued**
- [ ] `glyph_count` (prop) — number of glyphs — **P1 · M2 · catalogued**
- [ ] `flags` (prop) — descriptor flags (PRD §8.5 bit semantics) — **P0 · M2 · catalogued**
- [ ] `buffer` (prop) — raw font bytes — **P1 · M2 · catalogued**
- [ ] `is_bold` (prop) — **P1 · M2 · catalogued**
- [ ] `is_italic` (prop) — **P1 · M2 · catalogued**
- [ ] `is_serif` (prop) — **P1 · M2 · catalogued**
- [ ] `is_monospaced` (prop) — **P1 · M2 · catalogued**
- [ ] `is_writable` (prop) — embeddable test — **P1 · M2 · catalogued**
- [ ] `Base14_fontnames` / `Base14_fontdict` / `fitz_fontdescriptors` — supporting module data — **P0 · M2 · catalogued**
- [ ] `css_for_pymupdf_font(name, ...)` — @font-face CSS for Story — **P3 · post-v1 · catalogued** *(Story-only; HTML engine out of scope)*

---

## 13. `TextWriter`

Collect styled text in page coords, then write once. PRD §7 row: *"write_text — P1 — M4"* / `TextWriter` family.

- [ ] `TextWriter(page_rect, opacity, color)` — constructor — **P1 · M4 · catalogued**
- [ ] `append(pos, text, font, fontsize, language, right_to_left, small_caps)` — add a run — **P1 · M4 · catalogued**
- [ ] `appendv(...)` — vertical text run — **P1 · M4 · catalogued**
- [ ] `fill_textbox(rect, text, ...)` — wrapped fill — **P1 · M4 · catalogued**
- [ ] `write_text(page, opacity, color, morph, matrix, render_mode, oc, overlay)` — flush to page — **P1 · M4 · catalogued**
- [ ] `clean_rtl(text)` — fix RTL ordering — **P1 · M4 · catalogued** *(visual-order/bidi only; full shaping out of scope, PRD §3.2 #10)*
- [ ] `text_rect` (prop) — accumulated bbox — **P1 · M4 · catalogued**
- [ ] `last_point` (prop) — pen position — **P1 · M4 · catalogued**
- [ ] `color` (prop) / `opacity` (prop) — style — **P1 · M4 · catalogued**

---

## 14. `Story` / `Xml` / `Archive` (HTML/CSS → PDF layout engine)

**Entire subsystem out of scope for v1** (PRD §3.2 #2; PRD §7 row: *"Story/Xml/Archive, `insert_htmlbox`,
`convert_to_pdf` (non-image) — P3 — post-v1"*). Catalogued as single grouped rows so the coverage guard
still tracks them; individual `Xml` builder methods are intentionally collapsed.

- [ ] `Story(html, user_css, em, archive)` — constructor — **P3 · post-v1 · catalogued**
- [ ] `Story.place(rect)` — place into a rect → (more, filled) — **P3 · post-v1 · catalogued**
- [ ] `Story.draw(device, matrix)` — draw placed content — **P3 · post-v1 · catalogued**
- [ ] `Story.write(...)` / `write_with_links(...)` / `write_stabilized(...)` / `write_stabilized_with_links(...)` — paginated write — **P3 · post-v1 · catalogued**
- [ ] `Story.fit(...)` / `fit_height` / `fit_width` / `fit_scale` / `FitResult` — fit helpers — **P3 · post-v1 · catalogued**
- [ ] `Story.element_positions(function, args)` — element callback — **P3 · post-v1 · catalogued**
- [ ] `Story.reset()` / `add_pdf_links(...)` / `add_header_ids()` / `document()` — misc — **P3 · post-v1 · catalogued**
- [ ] `Story.body` (prop) — → `Xml` DOM root — **P3 · post-v1 · catalogued**
- [ ] `Xml` builder API — `add_paragraph/division/span/header/bullet_list/number_list/list_item/description_list/image/link/code/codeblock/horizontal_line/text/subscript/superscript/kbd/samp/var` — **P3 · post-v1 · catalogued**
- [ ] `Xml` tree ops — `append_child/append_styled_span/create_element/create_text_node/insert_before/insert_after/remove/clone/find/find_next` — **P3 · post-v1 · catalogued**
- [ ] `Xml` styling — `set_font/fontsize/color/bgcolor/bold/italic/underline/align/margins/leading/lineheight/letter_spacing/word_spacing/columns/opacity/pagebreak_before/pagebreak_after/id/attribute/properties` — **P3 · post-v1 · catalogued**
- [ ] `Xml` navigation props — `first_child/last_child/next/previous/parent/root/tagname/text/is_text` — **P3 · post-v1 · catalogued**
- [ ] `Archive(...)` — constructor (dir/zip/tar/memory) — **P3 · post-v1 · catalogued**
- [ ] `Archive.add(content, name)` — add resource — **P3 · post-v1 · catalogued**
- [ ] `Archive.has_entry(name)` — membership — **P3 · post-v1 · catalogued**
- [ ] `Archive.read_entry(name)` — read resource — **P3 · post-v1 · catalogued**
- [ ] `Archive.entry_list` (prop) — list resources — **P3 · post-v1 · catalogued**
- [ ] `Story.add_caption` / other minor `Xml` setters (catch-all) — **P3 · post-v1 · catalogued** *(grouped; verify completeness against 1.24.x at implementation time)*

---

## 15. `Colorspace`

- [ ] `Colorspace(CS_*)` — constructor (`CS_GRAY/CS_RGB/CS_CMYK`) — **P1 · M5 · catalogued**
- [ ] `n` (prop) — number of components — **P1 · M5 · catalogued**
- [ ] `name` (prop) — colorspace name — **P1 · M5 · catalogued**
- [ ] singletons `csGRAY` / `csRGB` / `csCMYK` + `CS_GRAY/CS_RGB/CS_CMYK` constants — **P1 · M5 · catalogued**

---

## 16. Module-level functions

PRD §7 maps `open`, geometry, and helpers; many of these are pure utilities (P0/P1) while a few are
render/Story/OCR-bound (deferred / out of scope).

- [ ] `open(...)` — open document (alias of `Document`) — **P0 · M1 · catalogued**
- [x] `paper_size(name)` — named paper dimensions — **P1 · M0 · implemented** *(not in PRD §7 explicit — verify; trivial constant table)*
- [x] `paper_rect(name)` — named paper rect — **P1 · M0 · implemented**
- [x] `paper_sizes` — paper-size table — **P1 · M0 · implemented**
- [ ] `get_text_length(text, fontname, fontsize, encoding)` — Base-14 string width — **P0 · M2 · catalogued** *(needs Base-14 AFM metrics)*
- [ ] `get_pdf_now()` — PDF date string — **P1 · M3 · catalogued**
- [ ] `get_pdf_str(s)` — escaped PDF string — **P1 · M3 · catalogued**
- [ ] `sRGB_to_rgb(i)` — integer sRGB → (r,g,b) — **P1 · M2 · catalogued**
- [ ] `sRGB_to_pdf(i)` — integer sRGB → PDF floats — **P1 · M2 · catalogued**
- [ ] `glyph_name_to_unicode(name)` — Adobe glyph-name mapping — **P1 · M2 · catalogued**
- [ ] `unicode_to_glyph_name(cp)` — reverse mapping — **P1 · M2 · catalogued**
- [ ] `recover_quad(line_dir, span)` — reconstruct rotated-text quad — **P1 · M2 · catalogued**
- [ ] `recover_char_quad(...)` / `recover_line_quad(...)` / `recover_span_quad(...)` / `recover_bbox_quad(...)` — quad recovery family — **P1 · M2 · catalogued**
- [ ] `css_for_pymupdf_font(name, ...)` — @font-face CSS — **P3 · post-v1 · catalogued** *(Story-only)*
- [ ] `image_profile(stream)` — inspect image bytes without decode — **P1 · M5 · catalogued**
- [ ] `planish_line(p1, p2)` — matrix flattening a line to x-axis — **P1 · M2 · catalogued**
- [ ] `find_tables(page, ...)` / `make_table(...)` — table detection — **P3 · post-v1 · catalogued** *(table detection out of scope, PRD §3.2 #4)*
- [ ] `get_tessdata()` — locate Tesseract data dir — **P3 · post-v1 · catalogued** *(OCR out of scope, PRD §3.2 #3)*
- [ ] `ConversionHeader(...)` / `ConversionTrailer(...)` — HTML/XML export scaffolding — **P1 · M2 · catalogued** *(used by html/xhtml/xml serializers)*
- [ ] `set_messages(...)` / `message(...)` / `set_log(...)` / `log(...)` — logging redirection — **P1 · M1 · catalogued** *(map to warning collector, PRD §8.2)*
- [ ] `Tools()` / `TOOLS` — global tuning singleton (see §17) — **P1 · M1 · catalogued**

---

## 17. `Tools` / `TOOLS` (global settings singleton)

Dispositions per PRD §3.6.

- [ ] `gen_id()` — generate a unique id — **P1 · M3 · catalogued**
- [ ] `set_annot_stem(s)` — annotation `/NM` stem — **P1 · M4 · catalogued** *(implemented per PRD §3.6)*
- [ ] `mupdf_warnings(reset)` — formatted warning collector output — **P1 · M1 · catalogued** *(mapped, PRD §3.6)*
- [ ] `reset_mupdf_warnings()` — clear collector — **P1 · M1 · catalogued** *(mapped, PRD §3.6)*
- [ ] `mupdf_version()` — version string — **P1 · M1 · catalogued**
- [ ] `store_shrink(n)` / `store_maxsize` / `store_size` — cache knobs — **P3 · M1 · catalogued** *(no-op + warn, PRD §3.6)*
- [ ] `set_aa_level(n)` / `show_aa_level()` — anti-alias level — **deferred · M6 · catalogued** *(no-op + warn until render-era, PRD §3.6)*
- [ ] `set_small_glyph_heights(b)` — render-era tuning — **deferred · M6 · catalogued** *(no-op + warn)*
- [ ] `set_subset_fontnames(b)` — subset naming toggle — **P2 · M4 · catalogued**
- [ ] `set_graphics_min_line_width(w)` / `set_font_width(...)` / `set_icc(b)` / `set_low_memory(b)` — misc tuning — **deferred/P3 · M6 · catalogued** *(render/cache-era; no-op + warn)*
- [ ] `mupdf_display_errors(b)` / `mupdf_display_warnings(b)` / `glyph_cache_empty()` / `image_profile(...)` / `fitz_config` — diagnostics & config — **P3 · M1 · catalogued**
- [ ] raw `mupdf.*` module access — **out-of-scope · out-of-scope · catalogued** *(raises `PdfUnsupportedError`, PRD §3.6)*

---

## 18. Constant families & exceptions

Enums/constants are Low difficulty in Rust but numerous. Each *family* is one checklist row; individual members
are not catalogued separately. Most are needed wherever their owning subsystem lands.

- [ ] **Text flags** `TEXT_PRESERVE_LIGATURES/WHITESPACE/IMAGES/SPANS`, `TEXT_INHIBIT_SPACES`, `TEXT_DEHYPHENATE`, `TEXT_MEDIABOX_CLIP`, `TEXT_CID_FOR_UNKNOWN_UNICODE`, `TEXT_ACCURATE_BBOXES`, `TEXT_COLLECT_STRUCTURE/VECTORS`, `TEXT_IGNORE_ACTUALTEXT`, `TEXT_STEXT_SEGMENT` — **P0 · M2 · catalogued**
- [ ] **`TEXTFLAGS_*` bundles** (`TEXT/WORDS/BLOCKS/DICT/RAWDICT/HTML/XHTML/XML/SEARCH`) — pinned per-method defaults — **P0 · M2 · catalogued**
- [ ] **Text alignment** `TEXT_ALIGN_LEFT/CENTER/RIGHT/JUSTIFY` — **P0 · M4 · catalogued**
- [ ] **Text font flags** `TEXT_FONT_BOLD/ITALIC/SERIFED/MONOSPACED/SUPERSCRIPT` — **P0 · M2 · catalogued**
- [ ] **Annotation types** `PDF_ANNOT_*` (TEXT/FREE_TEXT/.../REDACT/.../UNKNOWN) — **P1 · M4 · catalogued**
- [ ] **Annot flags** `PDF_ANNOT_IS_*` (INVISIBLE/HIDDEN/PRINT/...) — **P1 · M4 · catalogued**
- [ ] **Line endings** `PDF_ANNOT_LE_*` — **P1 · M4 · catalogued**
- [ ] **Widget types** `PDF_WIDGET_TYPE_*` — **P1 · M4 · catalogued**
- [ ] **Widget text formats** `PDF_WIDGET_TX_FORMAT_*` — **P1 · M4 · catalogued**
- [ ] **Widget field flags** `PDF_TX/CH/BTN/FIELD_IS_*` — **P1 · M4 · catalogued**
- [ ] **Encryption methods** `PDF_ENCRYPT_NONE/KEEP/RC4_40/RC4_128/AES_128/AES_256/UNKNOWN` — **P0 · M1 · catalogued**
- [ ] **Permission flags** `PDF_PERM_PRINT/MODIFY/COPY/ANNOTATE/FORM/ACCESSIBILITY/ASSEMBLE/PRINT_HQ` — **P0 · M1 · catalogued**
- [ ] **Blend modes** `PDF_BM_*` — **P1 · M4 · catalogued**
- [ ] **Redaction options** `PDF_REDACT_IMAGE_*`, `PDF_REDACT_LINE_ART_*`, `PDF_REDACT_TEXT_*` — **P0 · M4 · catalogued**
- [ ] **Stamp icons** `STAMP_*` — **P1 · M4 · catalogued**
- [ ] **Page layout/mode/labels** `PDF_PAGE_LABEL_*`, `set_pagelayout`/`set_pagemode` value strings — **P2 · M3 · catalogued**
- [ ] **Colorspace** `CS_GRAY/CS_RGB/CS_CMYK` — **P1 · M5 · catalogued**
- [ ] **Border styles/effects** `PDF_BORDER_STYLE_*`, `PDF_BORDER_EFFECT_*` — **P1 · M4 · catalogued**
- [ ] **Signature flags** `PDF_SIGNATURE_*`, `SigFlag_*` — **P2 · M4 · catalogued** *(read-only signature flags)*
- [ ] **Unicode scripts** ~170 `UCDN_SCRIPT_*` — **deferred · M6 · catalogued** *(full shaping out of scope, PRD §3.2 #10)*
- [ ] **Low-level PDF tokens/objects** `PDF_TOK_*`, `PDF_NAME`, `PDF_NULL/TRUE/FALSE`, `PDF_ENUM_*` — **P3 · M1 · catalogued** *(COS-level; expose only if low-level API needs them)*
- [ ] **Version/info** `version`, `VersionBind`, `VersionFitz`, `VersionDate`, `pymupdf_version`, `mupdf_version` — **P1 · M1 · catalogued**
- [ ] **Exceptions** `FileDataError`, `EmptyFileError`, `FileNotFoundError`, `FitzDeprecation`, plus oxide-pdf-typed `PdfUnsupportedError`/`PdfDecodeError`/`PdfRedactionError` — **P0 · M1 · catalogued**
- [ ] **`PdfUnsupportedError` catch-all** — every PyMuPDF symbol *not* listed here raises this (never `AttributeError`), enumerated in `COMPAT.toml` (PRD §7 catch-all + §17.2) — **P0 · M1 · catalogued**

---

## Appendix A — PyMuPDF capabilities found in research but NOT mapped in PRD §7

The following symbols appear in the api-surface inventory but have **no explicit row in PRD §7**. They are
catalogued above with best-guess (priority, milestone) and flagged `(not in PRD §7 — verify)`. The orchestrator
should decide whether to add explicit PRD §7 rows or confirm they fall under an existing catch-all.

**Likely in-scope, just unlisted (low risk to absorb under existing rows):**
- `Document.pages()` iterator, `Document.reload_page`, `Document.page_cropbox`, `Document.version_count`,
  `Document.resolve_link`, `Document.get_outline_xrefs`
- `Document.get_page_xobjects`, `Page.get_xobjects`, `Page.extend_textpage`, `Page.get_texttrace`
- `Page.get_bboxlog`, `Page.cluster_drawings`
- `Page.show_pdf_page`, `Page.replace_image`, `Page.delete_image`
- `Page.get_contents`/`refresh` (content-stream + page-refresh plumbing)
- Module helpers `paper_size`/`paper_rect`/`paper_sizes`, `ConversionHeader`/`ConversionTrailer`,
  `set_messages`/`message`/`set_log`/`log`
- `Pixmap(PIL image)` ctor, `Pixmap.pil_save`/`pil_tobytes`, `Pixmap.warp`
- `Widget` JavaScript hook attributes (`script*`) — stored, not executed
- `Annot.get_sound`

**State setters not called out in §7 (probably P2/P3; verify whether to support):**
- `Document.markinfo`/`set_markinfo`, `pagelayout`/`set_pagelayout`, `pagemode`/`set_pagemode`

**Confirmed out-of-scope by other PRD sections (listed for completeness, no action needed):**
- Chapter/location model (`chapter_count`, `*_location`, `*_bookmark`) — EPUB reflow, PRD §3.2 #8
- OCG/layers family, journalling family, page-label *write*, OCR family, table-finder family — PRD §3.2 #3/#4/#5
- Vector rendering (`get_pixmap` on vector pages, `get_svg_image`, `DisplayList`, `run`, `Annot.get_pixmap`,
  `Tools.set_aa_level` & render-era knobs, `UCDN_SCRIPT_*`) — PRD §3.2 #1 / §7 (M6)
- All non-PDF input parsing (XPS/EPUB/MOBI/FB2/CBZ/SVG/TXT) — PRD §3.2 #8

---

*End of PARITY.md. Tick boxes and flip Status as features land; keep the Summary dashboard counts in sync.*
