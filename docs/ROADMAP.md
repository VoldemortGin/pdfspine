# pdfspine — Detailed Roadmap, Milestones, Risks & KPIs

*(Generated as the granular companion to PRD.md §12–§14.)*

I'll produce the requested PRD sections. This is a writing/synthesis task grounded in the research provided — no codebase exploration needed. Let me check whether a relevant skill applies before drafting.

The task is pure PRD synthesis from research. No matching skill (the closest, `to-prd`, publishes to an issue tracker, which isn't requested). I'll write the sections directly.

# PRD: Apache-2.0-Licensed Rust Reimplementation of PyMuPDF
## Sections: Roadmap, Milestones, Risks, Effort, Success Metrics

Working name: **pdfspine** (final name TBD — must avoid "PyMuPDF/MuPDF/fitz/Artifex"). PDF-first; image-docs in v1; rendering of vector PDF pages deferred past v1. Strict TDD. Permissive (Apache-2.0 / MIT / BSD) deps only.

---

## 1. Phased Roadmap & Milestones

Effort is expressed in **agent-week-equivalents (AWE)** — one focused AI-agent working week on one coherent slice, plus relative T-shirt size. These are planning units, not calendar promises; milestones M1/M2/M4 are the long poles. Every exit criterion is a **TEST gate** (the feature is "done" only when the named tests are green in CI), consistent with the TDD methodology: catalogued cases → red → green → harden, with diff-coverage ≥90%, mutation target met, and fuzz-smoke clean as part of Definition of Done.

A standing rule across all milestones: parser/codec code is `#![forbid(unsafe_code)]`, runs under `Limits` (recursion/decompression/zip-bomb/timeout caps), and ships with `cargo-fuzz` targets from the milestone it's introduced. License gate (`cargo-deny`: allow MIT/Apache-2.0/BSD-2/BSD-3/Zlib/ISC/Unicode-DFS/IJG/CC0-test; deny GPL/AGPL/LGPL/MPL/SSPL) and the AGPL-source/fixture provenance lint run on every PR.

---

### M0 — Project Setup / CI / TDD Harness
**Size: S · Effort: 2–3 AWE**

**Goals.** Stand up the eight-crate Cargo workspace (`pdf-core`, `pdf-crypto`, `pdf-fonts`, `pdf-text`, `pdf-edit`, `pdf-image`, `pdf-render`[reserved slot], `pdf-api`, plus `py-bindings`, `pdf-fuzz`, `pdf-testdata`), the full TDD/CI machinery, the clean-room governance, and the foundational geometry layer (which has zero PDF dependencies and is needed by everything).

**Feature set delivered.**
- Workspace + dependency-DAG lint (no sibling-domain-crate cycles; FFI touches only `pdf-api`).
- Geometry: `Matrix`, `Point`, `Rect`, `IRect`, `Quad` + constants (`Identity`, `EMPTY/INFINITE_RECT`, paper sizes) — the full M0 surface from the API inventory.
- CI jobs: `fmt`, `clippy -D warnings`, multi-OS `cargo test`, `cargo-llvm-cov` (diff-coverage gate), `cargo-mutants --in-diff`, `cargo-fuzz` smoke, `cargo-deny`, AGPL-provenance/manifest lint, `test-order-guard` (enforces tests-precede-impl within a milestone PR).
- Test Case Catalog skeleton (`docs/test-case-catalog.md`) and fixture `MANIFEST.toml` with license tags.
- Maturin/PyO3 abi3 build skeleton producing an importable stub wheel on Linux/macOS/Windows.

**Exit criteria (TEST gates).**
- `GEOM-*` unit + property tests green: `concat`/`invert` round-trip within ε, transform-then-inverse identity, rotation 0/90/180/270 exact, `Rect` normalize idempotent, union/intersect commutative, area = |det|.
- CI is red-on-violation for each gate (proven by a deliberately-failing canary PR per gate).
- A stub wheel imports (`python -c "import pdfspine"`) and passes a smoke test on all three OSes.
- `cargo-deny` passes; provenance lint fails a planted AGPL-hashed fixture.

---

### M1 — PDF Parse + Object Model + Filters (+ Encryption read)
**Size: XL · Effort: 12–16 AWE** — *the foundational lift; budget more here than for the entire spec-compliant writer.*

**Goals.** Open and enumerate the pages of nearly all real-world PDFs, including malformed ones. This is where the AGPL clean-room risk and the "works on file A, fails on file B" risk concentrate, so repair tolerance — not happy-path spec compliance — is the design center.

**Feature set delivered.**
- Tokenizer/lexer (all 8 object types, tolerant numeric/string/name parsing, comments, stream EOL handling).
- Object model (`Object` enum, `ObjRef`, flat graph, `Arc<DocumentStore>`, lazy load + interior-mutable cache, mmap-backed source retained for later incremental save).
- Cross-reference: classic xref tables, **xref streams** (PNG/TIFF predictors), **object streams**, trailer/`startxref`, incremental-update `/Prev` chains, hybrid-reference files, linearization read-transparency.
- **Repair subsystem** (the differentiator): full object-scan reconstruction, trailer/Catalog/Pages-tree recovery, `/Length` repair, tolerant fallbacks, decompress-and-scan ObjStms, validation-gate auto-fallback. Strict vs Lenient modes with queryable `Warning`s.
- Filters (decode): FlateDecode (+predictors), LZW (EarlyChange), ASCIIHex, ASCII85, RunLength. Decoders for DCT/CCITT/JBIG2/JPX deferred to M5 (bytes surfaced, marked unsupported).
- Page tree with inheritance resolution (`/Resources`, `/MediaBox`, `/CropBox`, `/Rotate`), boxes, rotation.
- **Encryption (read/open):** Standard Security Handler R2–R6 (RC4-40/128, AES-128 V4, AES-256 R5/R6), crypt filters (`/StmF`/`/StrF`/`/Identity`), per-object vs direct-key layering, ObjStm/Encrypt-dict/ID exemptions, empty-user-password case, SASLprep for R6.
- Low-level xref/object API: `xref_length`, `xref_object`, `xref_stream(_raw)`, `xref_get_key(s)`, `is_stream`, `pdf_catalog`/`pdf_trailer`, type predicates.
- `Document.open` (file/bytes/filetype), `page_count`, `load_page`, `metadata` (read), `needs_pass`/`authenticate`/`permissions`/`is_encrypted`, `Page.bound/rect/rotation/*box`.

**Exit criteria (TEST gates).**
- `LEXER-*`, `XREF-*`, `OBJSTM-*`, `FLATE-*`/`LZW-*`/`ASCII*-*`/`RUNLEN-*`, `REPAIR-*`, `CRYPT-*` catalogs 100% green (incl. the per-filter property round-trips `decode(encode(x))==x` and predictor inverse).
- Corpus gate: ≥99% of the license-clean corpus (GovDocs1 + PDFA + veraPDF + self-generated) **opens and enumerates correct page_count** in Lenient mode; malformed-fixture set repairs-with-warning or returns a typed error — **never panics**.
- Cross-validation: every fixture we re-serialize trivially still passes `qpdf --check` / opens in pikepdf (clean-room oracles).
- Encryption: round-trip decrypt of RC4/AES-128/AES-256(R6) fixtures; wrong password fails cleanly; `/Encrypt` and `/ID` verified never-encrypted; one fixture cross-checked vs `qpdf --decrypt`.
- Fuzz: `fuzz_open`/`fuzz_xref`/`fuzz_repair`/per-filter targets run a clean nightly (no crash/OOM/hang); `pdf-core` parser/filter modules ≥95% line coverage.

---

### M2 — Text Extraction + Search
**Size: XL · Effort: 10–14 AWE** — *the single highest-value capability for most users.*

**Goals.** PyMuPDF-class `get_text(...)` across all output models plus `search_for`, matching PyMuPDF's output *shapes* (keys, tuple arity, coordinate convention) and quality, without copying AGPL output.

**Feature set delivered.**
- Content-stream interpreter (text + graphics-state subset): `q/Q/cm/gs`, `BT/ET`, all text-state and text-showing ops, `Do` form-XObject recursion (depth/cycle capped), inline-image skipping, Trm math, top-left y-down page coordinate flip.
- Font layer for *mapping* (no rasterization): simple-font encodings (Standard/WinAnsi/MacRoman/PDFDoc/Symbol/ZapfDingbats + Differences), Type0/CID (Identity + predefined CJK CMaps), ToUnicode CMap parser, Adobe Glyph List + algorithmic name rules, width arrays (`/Widths`/`/W`/`/DW`/MissingWidth), Core-14 AFM metrics, FontDescriptor flags → span flags.
- Layout reconstruction: glyphs → spans → lines → blocks → words; reading-order/column (XY-cut) detection; sub/superscript flag; rotated/vertical/RTL handling; dehyphenation.
- Serializers (exact shape parity): `text`, `blocks`, `words`, `dict`/`json`, `rawdict`/`rawjson`, `html`, `xhtml`, `xml`; plus `get_textbox`, `get_text_selection`.
- `search_for` (rects/quads, case-insensitive, whole-word, cross-line/hyphenated, overlapping-rect merge, clip, all-hits).
- Page analysis: `get_fonts`, `get_images` (inventory metadata).
- `TEXT_*` flags (ligatures, whitespace, images, inhibit-spaces, dehyphenate, preserve-spans, mediabox-clip, CID-for-unknown).

**Exit criteria (TEST gates).**
- `WORDS-*`, `DICT-*`, `CMAP-*`, `ENCODING-*`, `GLYPHLIST-*`, `WIDTHS-*`, `LAYOUT-*`, `SEARCH-*` catalogs green; cross-mode property (`words` concatenation ≈ `text` whitespace-normalized; bbox well-formedness; monotonic block/line/word counters).
- Serializer conformance harness: emitted JSON/dict key set, nesting, and tuple arity **equal PyMuPDF's documented shape** on the fixture battery (key/shape facts taken from public docs, not AGPL output).
- Golden snapshots (insta) human-validated against visible page + `pdftotext -bbox` geometry within tolerance.
- Quality gate vs permissive oracles: character accuracy ≥0.98 (normalized Levenshtein) vs **pdfminer.six** on the Latin corpus; word-bbox mean IoU ≥0.90; search recall/precision parity on a known-substring set.
- Fuzz `fuzz_cmap`/`fuzz_content_stream`/`fuzz_get_text` clean.

---

### M3 — Edit / Save / Incremental + Page Ops
**Size: XL · Effort: 12–16 AWE**

**Goals.** PyMuPDF-class manipulation: full and incremental save, garbage collection, page operations, and `insert_pdf` merge — all on the copy-on-write overlay established in M1.

**Feature set delivered.**
- Object serializer (canonical syntax; name/string escaping; exact `/Length`; dates) — round-trip-tested.
- Object-edit API: `create/update/delete_object`, `update_stream`, `xref_get_key`/`xref_set_key`, `get_new_xref`, `intern` dedup.
- **Full save**: classic xref **and** xref-stream/object-stream authoring; `/ID` generation; free-list correctness.
- **Garbage collection** levels 1–4 (unreachable sweep → compact/renumber → dedup objects → dedup streams), conservative on identity-bearing objects.
- **Stream deflation**: `deflate`/`deflate_images`/`deflate_fonts`, `use_objstms`; compress-then-encrypt ordering.
- **Incremental save** (append-only, byte-exact prefix, `/Prev` chain) — guards rejecting incremental+garbage/linearization.
- **Save with encryption** (RC4-128, AES-128, AES-256 R6 write).
- Page ops: `new_page`, `insert_page`, `delete_page(s)`, `copy/fullcopy_page`, `move_page`, `select`, box setters, `set_rotation`, n-up; **`insert_pdf`** deep-copy + renumber + dedup-on-merge.
- Metadata write (`set_metadata`, Info+XMP consistency), TOC (`get_toc`/`set_toc`/item edits, outline tree build), named destinations.
- `save`/`ez_save`/`write`/`tobytes`/`save_incremental`/`can_save_incrementally`.

**Exit criteria (TEST gates).**
- `SAVE-*`, `INCR-*`, `GC-*`, `MERGE-*`, `PAGEOPS-*`, `TOC-*`, `META-*` catalogs green; serializer property `parse(serialize(obj))==normalize(obj)`.
- **Incremental byte-exactness**: `out[..orig.len()] == orig`; new xref `/Prev` == prior `startxref`; both revisions reopen and resolve.
- **Round-trip invariants** across all save modes (full / full+GC / incremental / encrypted): page count, live-graph object set, MediaBoxes, extracted text preserved; graph validator finds no dangling/cyclic-broken refs.
- GC levels each proven (orphan dropped / dense renumber / font dedup repoint / identical-stream collapse).
- Merge: A+B page order/count correct, all refs resolve, dedup keeps shared embedded font single; every saved fixture passes `qpdf --check`.
- TOC round-trip equals input (levels/titles/pages, `/Count` signs); level-jump rejected.

---

### M4 — Annotations / Forms / Redaction
**Size: XL · Effort: 12–16 AWE** — *redaction and forms are correctness/security-critical; appearance-stream generation is the hard core.*

**Goals.** Create/edit annotations with portable appearance streams, fill/flatten AcroForms, and perform **destructive** redaction.

**Feature set delivered.**
- Content emission (shared with M3): `insert_text`/`insert_textbox` (Base-14 + full TTF/OTF embedding with `/ToUnicode`; subsetting feature-gated), `insert_image` (JPEG passthrough / Flate / SMask / Indexed), `draw_*` + `Shape` (batched `q…Q`), `insert/update/delete_link`.
- Annotations: full `add_*_annot` family + `Annot` (`update`, colors/opacity/border/flags/info, line-ends), `/AP /N` appearance generation for every subtype, QuadPoints (Acrobat order), delete-with-cleanup.
- **Redaction**: content-stream interpreter-driven glyph removal on any overlap, image PIXELS/NONE modes, line-art modes, content regeneration, overlay fill/text, forced full-rewrite (incremental rejected to prevent byte leaks).
- Forms: AcroForm read (field tree, FQN), set text/checkbox/radio/choice, appearance regeneration (`NeedAppearances=false`), flatten; signatures read-only.
- Embedded files / attachments (`embfile_*`), `bake`, `scrub`.

**Exit criteria (TEST gates).**
- `ANNOT-*`, `FORM-*`, `REDACT-*`, `INSERT-*` catalogs green; each annot subtype reopens with subtype/geometry/`/AP /N` present; `update()` reflects color change in the AP stream.
- **Redaction security gate** (the load-bearing assertions): after `apply_redactions`+full-save, `get_text()` lacks the secret **and** a raw byte-grep of the entire file finds nothing; surviving adjacent text unshifted; PIXELS mode blanks image pixels; incremental-after-redaction rejected/auto-upgraded.
- Forms: set value → `/V` updated and AP contains the value; checkbox on-state discovered from `/AP /N`; radio group `/V` correct; flatten removes `/AcroForm` + widgets, content shows field text. Cross-viewer oracle smoke (`pdftoppm`/`pdfium`) non-blank where expected.
- Embedded-file extract byte-equals original; `/Params/Size` correct.

---

### M5 — Image-Doc Support + Codecs + fitz-Compat Shim Hardening
**Size: L · Effort: 8–11 AWE**

**Goals.** Open images as one-page-per-image Documents, decode the deferred image codecs (so image XObjects and image-docs produce `Pixmap`s — the in-scope "render" path), and harden the `fitz`/`pymupdf` compatibility shim to a measured coverage bar. *(No vector-page rasterization — that is M6.)*

**Feature set delivered.**
- `pdf-image`: PNG/JPEG/TIFF(multi-IFD)/GIF/BMP/WEBP decode behind the `Document`/`Page` traits; `convert_to_pdf`; per-page MediaBox from pixel size + DPI; orientation/colorspace/alpha→SMask/palette→Indexed handling.
- Image codecs (decode): DCTDecode (`zune-jpeg`, CMYK/APP14), CCITTFax (`hayro-ccitt`/`fax`), **JBIG2** (`hayro-jbig2`), **JPEG2000** (`hayro-jpeg2000`) — each fuzzed, resource-capped, treated as untrusted; on save, exotic codecs transcoded to Flate/JPEG.
- `Pixmap` type (samples/save/tobytes/buffer-protocol zero-copy/numpy interop) **produced from decoded images** (not from vector rasterization).
- `extract_image`/`extract_font`/`get_char_widths` (image/font extraction).
- **fitz/pymupdf shim hardening**: pure-Python compat package over `_core`, geometry value types, exception aliasing (`FileDataError`, `EmptyFileError`, …), constants, deprecated-alias coverage, machine-readable `COMPAT.toml`, runtime `PdfUnsupportedError` for known-but-unimplemented methods.

**Exit criteria (TEST gates).**
- `IMGDOC-*`, `DCT-*`, `CCITT-*`, `JBIG2-*`, `JPX-*`, `PIXMAP-*`, `FITZCOMPAT-*` catalogs green.
- Image-doc: PNG→1-page doc with correct MediaBox; JPEG passthrough byte-equal + `/Filter==DCTDecode`; alpha→`/SMask`; palette→`/Indexed`; multi-page TIFF page_count matches IFDs; `convert_to_pdf` outputs pass `qpdf --check`; `get_pixmap` on an image equals decoder output (pixel-equality on fixtures).
- Codec cross-checks: JBIG2 cross-validated between `hayro-jbig2` and `oxidize-pdf`'s decoder; JPX differential-tested vs OpenJPEG on sample images; all codec fuzz targets clean under rss/time caps.
- Compat coverage report ≥ target % `implemented`; behavioral-parity pytest suite green (xfail-tagged for documented deviations); coverage gate blocks any `implemented → missing` regression.

---

### M6 — Rendering (vector page rasterization) — **OUT OF v1 / LATER**
**Size: XXL · Effort: 25–40+ AWE** — *the largest single component; explicitly deferred.*

**Goals (post-v1).** PyMuPDF-class `get_pixmap`/`get_svg_image`/`DisplayList` for arbitrary PDF pages.

**Feature set (deferred).** Full graphics interpreter; font rasterization (`swash`/`fontdue`/`ttf-parser`, Type3 interpreter); color management (`moxcms` ICC, Functions 0/2/3/4); shadings 1–7; transparency groups/blend modes/soft masks; patterns; tiling. Strategy: **depend on the `hayro` interpreter/renderer (Apache-2.0/MIT)** as the "very large leaf" rather than writing a rasterizer from scratch; keep `pdfium-render` as an optional non-default backend feature and as a differential oracle.

**Exit criteria (when scheduled).** Render-to-pixmap perceptual-hash parity vs `pdfium-render`/PyMuPDF (dev oracle) within tolerance on the render corpus; `get_pixmap` calls that currently raise `PdfUnsupportedError` become green; transparency/shading fidelity tracked as a documented long-tail.

---

### Roadmap summary

| Milestone | Slice | Size | AWE | Gating theme |
|---|---|---|---|---|
| M0 | setup/CI/TDD/geometry | S | 2–3 | CI gates provably red-on-violation |
| M1 | parse + objects + filters + crypto-read + **repair** | XL | 12–16 | ≥99% corpus opens; never-panic; ≥95% core cov |
| M2 | text extraction + search | XL | 10–14 | shape parity + ≥0.98 char accuracy vs pdfminer |
| M3 | save/incremental/GC + page ops + merge | XL | 12–16 | byte-exact incremental; `qpdf --check` clean |
| M4 | annotations / forms / redaction | XL | 12–16 | redaction byte-grep gate; AP portability |
| M5 | image-docs + codecs + Pixmap + shim | L | 8–11 | codec cross-checks; compat coverage % |
| **M6** | **vector rendering** | XXL | 25–40+ | **deferred past v1** |

**v1 total (M0–M5): ≈56–76 AWE.** M1–M4 are the long poles and the risk concentration; M5 is de-risked by the `hayro-*` permissive codecs.

---

## 2. Prioritized Feature Matrix (P0–P3 → Milestone)

Priority: **P0** = v1 must-ship core value; **P1** = strongly expected for credible PyMuPDF parity; **P2** = valuable, can trail; **P3** = niche / post-v1.

| Capability (PyMuPDF surface) | Priority | Milestone |
|---|---|---|
| Geometry: Matrix/Point/Rect/IRect/Quad + constants | P0 | M0 |
| `open` (file/bytes/filetype), Document lifecycle, `close` | P0 | M1 |
| xref classic + **xref streams** + **object streams** + trailer | P0 | M1 |
| **Malformed-PDF repair / reconstruction** | P0 | M1 |
| Filters: Flate(+predictors)/LZW/ASCIIHex/ASCII85/RunLength (decode) | P0 | M1 |
| Page tree + inheritance, boxes, rotation; `page_count`/`load_page` | P0 | M1 |
| Encryption **read/open** R2–R6 (RC4/AES-128/AES-256), permissions, `authenticate` | P0 | M1 |
| Low-level xref/object read API (`xref_object`/`xref_stream`/`xref_get_key`) | P1 | M1 |
| `get_text` text/blocks/words/dict/json/rawdict | P0 | M2 |
| `get_text` html/xhtml/xml; `get_textbox`/selection | P1 | M2 |
| `search_for` (rects/quads, flags, clip) | P0 | M2 |
| Fonts for mapping (encodings/Differences/ToUnicode/AGL/CJK CMaps/widths) | P0 | M2 |
| `get_fonts`/`get_images` inventory | P1 | M2 |
| `TextPage` reusable object | P1 | M2 |
| Full `save` (xref + xref/obj-stream authoring); `tobytes`/`write` | P0 | M3 |
| **Incremental save** / `save_incremental` | P0 | M3 |
| Garbage collection 1–4; deflate options | P1 | M3 |
| Encryption **write** (RC4-128/AES-128/AES-256) | P1 | M3 |
| Page ops: new/insert/delete/copy/move/select; box/rotation setters | P0 | M3 |
| **`insert_pdf`** merge (deep-copy + dedup) | P0 | M3 |
| Metadata write (Info+XMP); TOC get/set; named dests | P1 | M3 |
| `insert_text`/`insert_textbox` (Base-14 + TTF embed) | P0 | M4 |
| `insert_image`; `draw_*` + `Shape`; link insert/update/delete | P1 | M4 |
| Annotations: full `add_*_annot` + `Annot` + `/AP` generation | P1 | M4 |
| **`apply_redactions`** (destructive) | P0 | M4 |
| Forms: read + fill + flatten (AcroForm/Widget) | P1 | M4 |
| Embedded files (`embfile_*`); `bake`/`scrub` | P2 | M4 |
| Font subsetting (`subset_fonts`, insert-text subsetting) | P2 | M4/M5 (feature-gated) |
| Image documents (PNG/JPEG/TIFF/GIF/BMP/WEBP) + `convert_to_pdf` | P0 | M5 |
| Image codecs decode: DCT/CCITT/**JBIG2**/**JPX** | P1 | M5 |
| `Pixmap` (from images), buffer-protocol/numpy, save/tobytes | P0 | M5 |
| `extract_image`/`extract_font` | P2 | M5 |
| `fitz`/`pymupdf` compat shim hardening + COMPAT matrix | P0 | M5 |
| **Page rendering** `get_pixmap`/`get_svg_image`/`DisplayList` (vector) | P2→deferred | **M6 (post-v1)** |
| Story/Xml/Archive, `insert_htmlbox`, `convert_to_pdf` (non-image) | P3 | post-v1 |
| OCR (`*_ocr`, `pdfocr_*`), `find_tables`, table finder | P3 | post-v1 |
| OCG/layers, page labels, journalling (undo/redo) | P3 | post-v1 |
| Digital signature **create**; linearization **write** | P3 | post-v1 |
| Non-PDF inputs (XPS/EPUB/MOBI/FB2/CBZ/SVG) | P3 | out of scope |

---

## 3. Risk Register

Likelihood (L) / Impact (I): Low / Med / High.

| # | Risk | L | I | Mitigation |
|---|---|---|---|---|
| R1 | **Malformed-PDF repair gap** — the #1 source of "works on A, fails on B"; the differentiator that separates a toy from a production engine. | High | High | Treat repair as a first-class M1 subsystem (full object scan, Catalog/Pages recovery, `/Length` repair, validation-gate auto-fallback), budget more than the whole writer; gate on ≥99% corpus open in Lenient mode + never-panic fuzz; differential-validate structure with `qpdf`/pikepdf. |
| R2 | **JBIG2 — young permissive decoders + CVE history** (the iMessage zero-click was JBIG2). | Med | Med | Depend on `hayro-jbig2` (MIT/Apache); cross-check vs `oxidize-pdf`'s decoder on a corpus; fuzz hard under rss/time caps; treat as untrusted; **ban** GPL `jbig2dec`. Acceptable to graceful-fail on unsupported sub-features. |
| R3 | **JPEG2000 — historically no permissive pure-Rust codec.** | Med | Med | *Downgraded:* depend on `hayro-jpeg2000` (MIT/Apache, well-tested); optional OpenJPEG (BSD-2) feature fallback for obscure features; differential-test vs OpenJPEG. |
| R4 | **No permissive JBIG2/JPX *encoder*.** | Low | Low | PyMuPDF rarely re-emits these; on save, transcode JBIG2/JPX→Flate/JPEG and document the format change. |
| R5 | **Font subsetting complexity** (rebuild glyf/CFF, remap cmap/CIDToGIDMap, regen widths) — "Very High" effort, the hardest content-insertion item. | Med | Med | Use `allsorts` (Apache-2.0, the only realistic permissive Rust subsetter); ship Base-14 + **full-font embedding** first, land subsetting iteratively behind a feature flag; full font embedding is a correct (if larger) fallback so v1 never blocks on subsetting. |
| R6 | **Encryption edge cases** — R6 Algorithm 2.B hash loop, per-object vs direct-key layering, ObjStm/Encrypt/ID exemptions, SASLprep. A wrong layering corrupts silently. | Med | High | RustCrypto leaf primitives (easy); implement key-derivation strictly from spec; exhaustive `CRYPT-*` catalog incl. the exemption set; cross-check vs Acrobat- and `qpdf`-produced fixtures; fuzz `fuzz_decrypt`. |
| R7 | **fitz 100%-compat gaps** — identical numeric output and full surface are impossible across a different engine. | High | Med | Ship a *behavioral/shape*-compatible shim, not bit-identical; machine-checked `COMPAT.toml` + API-diff coverage badge; tolerance-based golden parity; known-but-unimplemented methods raise `PdfUnsupportedError` (never confusing `AttributeError`); document deviations (rendering deferred, Story/OCR out of scope). |
| R8 | **Redaction security failure** — text selectable under the box / pre-redaction bytes lingering via incremental save = data-leak class bug. | Med | High | Interpreter-driven glyph removal on *any* overlap; force full-rewrite (reject incremental); test gate = whole-file byte-grep finds no secret + `get_text` clean + PIXELS-mode pixel blanking; also redact annot/form text. |
| R9 | **Performance vs C (MuPDF/PDFium).** | Med | Med | Pure-Rust, `forbid(unsafe)`, lazy/mmap parsing, GIL-release (`py.detach`) for parallel Python callers (strictly better than non-thread-safe PyMuPDF); `criterion` benches per op vs pypdf/pypdfium2; accept that vector rendering (deferred) is where C engines lead, and set realistic per-op KPIs rather than "beat C everywhere." |
| R10 | **Clean-room / AGPL contamination** — translating MuPDF C→Rust or vendoring its fixtures carries AGPL into the deliverable. | Low | High | Two-room discipline (API-from-docs, impl-from-ISO-spec + black-box behavior); never put AGPL source in an AI context; provenance logging per module; `cargo-deny` blocks GPL/AGPL/LGPL/MPL/SSPL; fixture `MANIFEST.toml` license lint + AGPL-hash blocklist; PyMuPDF used only as an ephemeral, non-committed dev oracle. |
| R11 | **`hayro` is "experimental"** (no encryption, partial blend modes) — API churn / gaps when leaned on for M5/M6. | Med | Med | Encryption is *our* core anyway; pin versions; keep `pdfium-render` fallback feature until hayro matures; contribute upstream; M5 only needs hayro's *codec leaves*, which are independently stable. |
| R12 | **Untrusted-input attack surface** (parser + codecs are classic CVE vectors). | High | High | `#![forbid(unsafe_code)]` core; `cargo-fuzz` from M1 continuously (+ OSS-Fuzz when public); `Limits` (recursion/decompression-ratio/zip-bomb/object-count/timeout); checked arithmetic + no-panic parsing (clippy `unwrap_used`/`indexing_slicing` = deny in core/fonts/crypto). |
| R13 | **abi3t (free-threaded) tooling immaturity (2026).** | Low | Low | Ship `abi3-py310` stable baseline now; add `abi3t-py3xx` wheel once green in CI; not on the critical path. |
| R14 | **pypdfium2 already covers "permissive + fast render."** | Med | Med | Differentiate on the *intersection nobody owns*: Apache-2.0 + pure-Rust (memory-safe, embeddable, WASM-friendly) + render+extract+edit+generate + fitz-API + Python bindings. Be honest in positioning; target AGPL-blocked PyMuPDF migrants, not raw render speed. |
| R15 | **Scope creep** — PyMuPDF's surface is enormous (Story/Xml, OCR, tables, OCG, signatures, non-PDF inputs). | High | Med | Hard v1 non-goals (§5); P3 items explicitly deferred; `PdfUnsupportedError` with matrix link; milestone exit gated on a 100%-green catalog so unscoped work can't sneak in. |
| R16 | **Effort underestimate on M1/M2/M4 long poles.** | Med | High | AWE ranges carry headroom; M1 repair budgeted above the writer; de-risk M5 via hayro; sequence so M1→M2 deliver standalone value (read+extract) even if later milestones slip. |

---

## 4. Effort Summary

- **Per-milestone AWE** as tabled in §1; **v1 (M0–M5) ≈ 56–76 AWE**; **M6 rendering ≈ 25–40+ AWE, deferred.**
- **Effort concentration:** four XL milestones (M1–M4) hold ~80% of v1 effort. Within them the documented "hardest sub-systems" are: malformed-file repair (M1), encryption R6 correctness (M1/M3), text-extraction font/CMap fallbacks (M2), `insert_pdf` deep-copy/dedup + incremental byte-exactness (M3), the content-stream interpreter shared by redaction + textbox wrapping (M2/M4), redaction correctness (M4), and font subsetting (M4/M5, feature-gated).
- **Staff-first (highest risk):** the content-stream interpreter, `insert_pdf`, incremental-save byte-exactness, AES-256 R6, and the repair subsystem.
- **De-risking levers:** `hayro-*` permissive codecs collapse the historically "impossible" JBIG2/JPX effort; `pdf-writer`/`krilla` and `lopdf` serve as permissive *design references* for the writer; `allsorts` provides the only realistic subsetter. These convert several would-be from-scratch XL efforts into "large leaf" dependencies.

---

## 5. Success Metrics / KPIs

**Correctness & conformance**
- **Open/parse conformance ≥99%** of the license-clean corpus (GovDocs1 + PDFA + veraPDF + self-generated) opens with correct `page_count` in Lenient mode; **0 panics/OOM/hangs** on the entire corpus + malformed set + fuzz (the non-negotiable robustness KPI).
- **Structural validity:** 100% of saved/merged/redacted outputs pass `qpdf --check` (and open in pikepdf) with no errors beyond an allowlist.
- **Encryption:** 100% pass on the R2–R6 round-trip fixture set; exemption-set (Encrypt/ID/ObjStm) verified.

**fitz-API coverage**
- **API coverage ≥85% `implemented`** of in-scope PyMuPDF symbols (machine-counted via the API-diff report), with the rest classified `partial`/`deviates`/`out-of-scope` and never silently `missing`; coverage badge monotonically non-decreasing (CI blocks regressions).
- **Behavioral-parity suite green** (tolerance-based), with every deviation an explicit, documented `xfail`.

**Extraction accuracy (vs permissive references)**
- **Character accuracy ≥0.98** (normalized Levenshtein) vs **pdfminer.six** on the Latin corpus; **≥0.95** on the CJK corpus where ToUnicode/predefined CMaps exist.
- **Word-bbox mean IoU ≥0.90** vs reference; **reading-order Kendall-τ ≥0.9**.
- **Search:** recall/precision parity on a known-substring battery (incl. cross-line/hyphenated).
- **`get_text` shape parity = 100%** (keys/nesting/tuple arity equal PyMuPDF's documented shape).

**Performance targets (vs permissive incumbents; benches via `criterion`)**
- **Parse/open + page enumeration:** competitive with or faster than **pypdf** (pure Python) by a wide margin; **within ~2× of pypdfium2** (C/PDFium) on cold open of large files (lazy/mmap should make us competitive).
- **Text extraction throughput:** **≥ pypdf**, target **within ~1.5–2× of pypdfium2** on the Latin corpus.
- **Save/merge throughput:** competitive with pypdf/pikepdf on split/merge.
- **Concurrency KPI:** linear-ish scaling of multi-document extraction across N Python threads (GIL released) — a capability pypdf/PyMuPDF cannot match safely. (Vector-render speed vs C is explicitly *not* a v1 KPI; rendering is deferred.)

**Fuzz stability**
- **0 reproducible crashes** across all `cargo-fuzz` targets on a clean nightly long run; **caught-mutant ratio ≥80%** in `pdf-core` filters/parsers/geom/serialize; per-target corpora minimized (`cmin`) and persisted; OSS-Fuzz onboarding once public.

**Wheel / platform coverage**
- **abi3-py310 wheels** for: manylinux2014 + musllinux_1_2 (x86_64, aarch64), macOS universal2, Windows x86_64 — **each smoke-imported and pytest-smoked in CI before publish**; optional **abi3t** wheel for free-threaded CPython once green.
- **Wheel self-containedness:** pure-Rust backends only (no system zlib/C linkage); bundled `THIRD-PARTY-LICENSES`; `cargo-deny` license gate green (zero GPL/AGPL/LGPL/MPL/SSPL in the tree).

**Process KPIs (TDD)**
- **Diff-coverage ≥90%** on every merged PR; `pdf-core` parser/filter modules **≥95% line**.
- **100% of a milestone's catalogued test cases green** before milestone close (machine-checked via catalog status + `test-order-guard`).
- **Definition-of-Done gates** (fmt, clippy-deny, coverage, mutation, fuzz-smoke, property/snapshot/python, clean-room manifest) enforced on 100% of PRs.

---

## 6. Out-of-Scope / Non-Goals for v1

Explicitly deferred or excluded; calling any known-but-unimplemented method raises `PdfUnsupportedError`/`NotImplementedError` with a link to the compat matrix (never a bare `AttributeError`):

1. **Vector page rendering / rasterization** — `get_pixmap`/`get_svg_image`/`DisplayList`/`run` for PDF pages (the entire M6 graphics interpreter, font rasterization, shadings, transparency/blend, ICC color management). *Image-doc and image-XObject Pixmaps are in scope; vector-page rasterization is not.*
2. **HTML/CSS layout engine** — `Story`, `Xml` DOM, `Archive`, `insert_htmlbox`; and `convert_to_pdf` for non-image inputs.
3. **OCR** — `get_textpage_ocr`, `pdfocr_save/tobytes`, Tesseract integration.
4. **Table detection** — `find_tables`/`TableFinder`/`make_table`.
5. **Optional content / layers (OCG/OCMD)**, **page labels** beyond basic, and **journalling (undo/redo)**.
6. **Digital signature creation/validation** — signature fields read-only; no signing, no LTV/DSS. (Append-only incremental-save mechanics that *preserve* existing signatures ARE delivered in M3.)
7. **Linearization (Fast Web View) writing** — read-transparent only; `linear=True` returns unsupported.
8. **Non-PDF input formats** — XPS, EPUB, MOBI, FB2, CBZ, SVG, TXT, and the chapter/location reflow model (these are large separate parsers; image docs are the only non-PDF inputs in v1).
9. **Font subsetting on by default** — full-font embedding is the v1 default; subsetting (`allsorts`) ships feature-gated and may trail.
10. **The ~170 `UCDN_SCRIPT_*` complex-shaping IDs / full HarfBuzz shaping** — extraction uses visual order + bidi tagging; full shaping is a rendering-era concern.
11. **Bit-identical fitz output / 100% API parity** — explicit non-goal; we target behavioral + shape compatibility within documented tolerances.
12. **MuPDF-internal low-level surfaces** (`Tools` deep tuning, raw `mupdf.*`) — partial/by-design-different since the engine differs.
