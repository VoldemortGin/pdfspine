# PRD-NEXT — Remaining Work Roadmap

> Live to-do list for resuming the oxide-pdf build. Updated 2026-06-18. **This file tracks ONLY what
> is LEFT** — completed work is intentionally not listed here. The record of DONE work lives in:
> git history + commit messages, `docs/BENCHMARKS.md` (accuracy), `COMPAT.toml` / `PARITY.md` (API
> parity), and `conformance/gt/*-REPORT.md` (machine metrics: GT-REPORT, RENDER-REPORT, tables).

## 1. Snapshot (where we are — 2026-06-18)

- **Text extraction:** at fitz parity across born-digital, PMC, EUR-Lex (8 langs), CJK, GovInfo,
  robustness — see `docs/BENCHMARKS.md`.
- **Rendering (`get_pixmap`):** SSIM **0.945 mean / 0.986 median** vs fitz (`conformance/gt/RENDER-REPORT.md`).
- **API parity:** **78.9%** (607/769 in `COMPAT.toml`); **96 symbols still deferred** (§3.B).
- **OCR:** Tesseract adapter shipped; pure-Rust PaddleOCR-via-`tract` **de-risked**, full build pending (§3.A).
- **Gate:** 1343 Rust + 589 pytest green; clippy `-D warnings` clean.

## 2. Harness (reuse, don't rebuild)

Objective ground-truth + differential harness in `conformance/gt/` (scripts committed;
corpora/cache/`*-results.json` gitignored, regenerable):
- `run_gt.py` — scores oxide vs fitz vs pdfminer vs SAME ground truth → `GT-REPORT.md`.
- `score.py` — decomposed metrics (lev/f1/jaccard/order), CJK-aware, NFKC.
- Fetchers: `born_digital.py`, `born_cjk.py`, `pmc_fetch.py`, `fetch_eurlex.py` (8 langs),
  `fetch_govinfo.py`, `fetch_robustness.py` (GovDocs1/SafeDocs).
- `tables_diff.py` — find_tables vs fitz. `render_diff.py` — get_pixmap vs fitz (SSIM).
- Real-corpus differential: `conformance/run_validation.py` + `fetch_corpus.py`.
- Venvs: `.venv` (oxide wheel) is the engine under test; `.venv-oracle` (real fitz 1.27 + pdfminer +
  pypdfium2 + rapidocr) is the GROUND-TRUTH oracle. **In `.venv`, `import fitz` is the oxide SHIM** —
  for true correctness always cross-check against `.venv-oracle`. No oracle output is ever committed.

## 3. Remaining work (priority order)

### A. OCR — full pure-Rust PaddleOCR build (HIGH — de-risked, next up)
De-risk DONE: `tract` (pure Rust) loads + runs all three PP-OCRv4 models (det/rec/cls); route B is GO,
no `ort` fallback needed. Full pipeline NOT yet built. Add a 2nd `OcrEngine` adapter (`PaddleOcr`)
alongside `TesseractCli` — this BEATS fitz on OCR (fitz is Tesseract-only), esp. CJK. Steps:
1. Add `tract-onnx` to `pdf-ocr`; bundle the **dim-fixed** PP-OCRv4 models (det 4.7MB, rec 10.9MB,
   cls 0.6MB, Apache-2.0) + char dict in the wheel. **Gotcha:** tract's ONNX parser rejects
   Paddle2ONNX symbolic dim names — pre-fix input dims to concrete via `onnx` offline (det
   `[1,3,640,640]`, rec `[1,3,48,320]`) + `del graph.value_info[:]`; ship the fixed models.
2. Rust pre-processing (det: resize/pad to 640²+normalize; rec: per-box crop→48×W+normalize),
   DBNet post-process (prob map→binarize→contours→min-area-box→unclip), CTC greedy decode + dict.
3. det→cls→rec→`OcrWord` loop; wire into the existing `integration.rs` pipeline; expose engine choice
   on `get_textpage_ocr` / `pdfocr_save`. Cross-platform is ~free (pure Rust, platform-independent
   `.onnx`, models bundled). Validate recognized text vs GT on real CJK + Latin scan samples.
See the `ocr-upgrade-plan` memory for the full recipe + exact verified output shapes.

### B. API parity coverage — 78.9% → higher (96 deferred)
The monoliths `python/oxide_pdf/document.py` + `crates/py-bindings/src/lib.rs` mean batches that both
touch them run SEQUENTIALLY. New pytest → next `python/tests/test_longtail11.py`. **Always** change
dispositions in `scripts/_compat_catalog.py` then regenerate (`python3 scripts/_compat_catalog.py`) —
**never hand-edit `COMPAT.toml`** — and confirm coverage rises with zero regressions (diff implemented
set vs HEAD) + `compat-symbol-guard.py` exit 0. Adversarially cross-check every symbol vs `.venv-oracle`.

Remaining, by group (largest first):
- **Page (25):** annot/widget/link adders+loaders (add_caret_annot, add_widget, delete_widget,
  load_annot/load_widget/load_links, first_link, links, update_link); page-level draw convenience
  (draw_curve/quad/sector/squiggle/zigzag, cluster_drawings); text (insert_font, write_text,
  set_contents, extend_textpage, run, is_wrapped); language/set_language, remove_rotation, refresh.
- **Document (19):** OCG/layers (add_layer, get_layers, switch_layer, set_layer_ui_config, get_oc,
  get_ocmd, set_ocmd); TOC (set_toc_item, del_toc_item, outline, get_outline_xrefs); heavy ops
  (convert_to_pdf, subset, insert_file, embfile_upd, extract_font, extract_image); FormFonts;
  version_count.
- **Constants (~21):** TEXT_flags, TEXTFLAGS_bundles, TEXT_FONT_flags, TEXT_ALIGN, PDF_ANNOT_types,
  PDF_ANNOT_IS_flags, PDF_ANNOT_LE, PDF_WIDGET_TYPE, PDF_WIDGET_TX_FORMAT, PDF_FIELD_IS_flags,
  PDF_BM_blendmodes, PDF_REDACT_options, STAMP_icons, PDF_BORDER_STYLE, PDF_SIGNATURE_flags,
  ENCRYPT_methods, PERM_flags, PDF_PAGE_LABEL, CS_colorspace, version_info, PDF_TOK_objects.
  → **quick wins** (just expose the enum/dict tables).
- **Module helpers (~15):** recover_quad/recover_char_quad/recover_line_quad/recover_span_quad/
  recover_bbox_quad, planish_line, glyph_name_to_unicode, unicode_to_glyph_name, sRGB_to_rgb,
  sRGB_to_pdf, get_pdf_now, get_pdf_str, get_text_length, ConversionHeader, ConversionTrailer,
  set_messages/message/set_log/log. → mostly **quick wins** (geometry/table helpers).
- **Font (2):** glyph_bbox, buffer — blocked on the Font handle carrying the embedded `/FontFile*`
  program (see §3.F; would also help rendering). **valid_codepoints already shipped (encoding-derived).**
- **Pixmap (3):** __array_interface__, samples_ptr, warp. **Tools (3):** image_profile,
  set_annot_stem, set_subset_fontnames. **DisplayList (2):** get_textpage, run. **Annot (1):**
  get_textbox (fitz reads the annot's own appearance textpage — different semantics; Page.get_textbox
  is the supported surface).

### C. Rendering long tail (MEDIUM — measure each with `render_diff.py`)
- **Bare Type1 PFB/PFA** (`/FontFile`) — not parseable by `ttf-parser`; needs a Type1 charstring
  interpreter (or Type1→CFF). Hits eurlex `32006L0112_ES`, some govdocs. Text stays extractable.
- **Non-embedded standard-14 fonts** (Helvetica/Times/Courier, no embedded program) not rasterized —
  no license-clean substitute bundled. Blanks most govdocs1 body text. (Bundling a metric-compatible
  permissive family — e.g. Liberation/Nimbus — would also unblock Font.glyph_bbox/buffer.)
- **Image/colorspace fidelity** — Indexed/Separation/ICC colorspaces, `/Decode` arrays not yet
  applied in the render path, halftone smoothing. The gross 1-bpc CCITT/JBIG2 inversion is already fixed.
- **Synthetic-bold / heavy display fonts** render slightly heavier than fitz (minor).
Renderer: `crates/pdf-render`; glyph plumbing in `crates/pdf-text` (`interp.rs`, `renderops.rs`).

### D. Extraction breadth (LOWER — text already at parity, diminishing returns)
- **RTL / Arabic (bidi)** — the one untested script class; most likely to surface a real bug. Needs
  bidi-aware GT (visual vs logical order). Born-digital Arabic (Chrome) or UN ODS Arabic PDFs.
- **FinTabNet gold table GT** — validate table *structure* vs human ground truth (not just fitz).
  FinTabNet (IBM, CDLA-Permissive), HF `bsmock/FinTabNet.c`. (Earlier fetch flaky — retry.)
- **Scale robustness** — `fetch_robustness.py` got only 23 (throttled); rerun for thousands of
  GovDocs1/SafeDocs for stronger never-panic + differential evidence.
- **More domains/langs** — DocLayNet (per-cell text GT; official 7.5GB zip ships real PDFs — HF
  mirrors strip them; needs zip64 range-extraction), more EUR-Lex, Japanese.
- **Kangxi-radical fold (CJK polish)** — oxide raw CJK uses radical codepoints (U+2F09 ⼉) where fitz
  folds to canonical ideographs (U+513F 儿). NFKC-equivalent/cosmetic. Small `pdf-fonts` CID→Unicode fix.
- **CI gate** — wire a born-digital `order ≥ 0.95` (+ tables count-agreement) regression gate into CI.

### E. Performance — measure + optimize (UNMEASURED — no speed numbers yet)
Accuracy is measured; **speed is not.** As a Rust lib we should be competitive with / faster than
PyMuPDF and pypdfium2, but this is unproven. Tasks:
- Add a speed benchmark to `conformance/` (open + extract + render time vs fitz/pypdfium2 on the same
  corpus); record in `docs/BENCHMARKS.md`.
- Then optimize hot paths if needed: per-page parallelism (rayon), lazy/streamed object parsing,
  avoiding redundant content-stream re-parse, font-program caching across pages.

### F. Polish / known deviations (LOW — fix when a consumer needs it)
- **Font handle carrying the embedded program** — oxide's `pdf_fonts::Font` is metrics+encoding only;
  carrying the `/FontFile*` bytes would unlock `glyph_bbox` (real per-glyph ink boxes), `buffer`
  (program bytes), and richer `valid_codepoints` (real cmap), AND feed the renderer. A medium refactor
  worth doing once (helps both §3.B Font and §3.C std-14).
- **HTML/XHTML/XML byte-exactness** — currently fitz-STRUCTURED + valid/parseable but not byte-exact
  (CSS font-family = raw PDF name vs fitz's Arial,sans-serif; per-line `<p>` w/o MuPDF heading
  promotion; `<img>` has no data-URI src). Polish only if a consumer needs MuPDF-identical markup.
- **Annot.get_textbox** — deferred (annot-appearance textpage semantics).
- **Page.remove_rotation** — deferred (needs content-stream rewriting).
- **xref_get_keys / pdf_trailer key ORDER** — oxide returns dict keys SORTED (Dict = `BTreeMap`) vs
  fitz's PDF stored order; same keys, benign. A project-wide `BTreeMap`→`IndexMap` swap would match
  fitz exactly but isn't worth it unless required.

## 4. Verify suite (run from repo root before every commit)
```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                      # expect 1343+ passed, 0 failed
source .venv/bin/activate && env -u CONDA_PREFIX maturin develop -q
env -u CONDA_PREFIX python -m pytest python/tests/ -q     # expect 589+ passed
python3 scripts/_compat_catalog.py && python3 scripts/compat-symbol-guard.py   # after any API batch
```
Gotchas: maturin needs `env -u CONDA_PREFIX`. Commit messages: **no backticks** (shell substitutes
them). Only ONE agent rebuilds the wheel at a time; don't run scoring during a wheel rebuild (shared
`.venv`). Subagents must NOT commit (main loop verifies + commits). When a batch agent dies mid-run on
an API error, check the working tree — it usually left coherent, compiling work; verify + finish it
rather than re-running from scratch.

## 5. Pre-public chores + docs upkeep (do last, before going public)
- Folder rename `~/workspace/pypdf` → `oxide-pdf` + recreate `.venv` (FINAL step).
- Reword any historical commit messages with backticks.
- **Keep `PARITY.md` + `docs/BENCHMARKS.md` current** (they drift as batches land — refresh after each).
- Docs site (`docs/guide`, `docs/reference`, `index.md`) completeness pass.
- PyPI publish (`docs/RELEASE-PYPI.md`); optional name trademark.
- Repo stays PRIVATE until everything is done (full parity + accuracy + perf + docs + CLI + OCR), then
  flip to public + push (`gh` authed as VoldemortGin).
