# PRD-NEXT — Remaining Work Roadmap (code-verified)

> **What this file is.** The single live to-do list for resuming the pdfspine build, **re-verified against
> the actual source on 2026-06-19** and re-prioritized around the public release. It tracks ONLY what is
> LEFT; the record of DONE work lives in git history + commit messages, `docs/BENCHMARKS.md` (accuracy),
> `COMPAT.toml` / `PARITY.md` (API parity), and `conformance/gt/*-REPORT.md` (machine metrics).
>
> **How priorities are set.** The plan pivots on the first public PyPI tag: **Phase 0** = must-fix-before-tag
> blockers · **Phase 1** = pre-launch quality (cheap, high-credibility) · **Phase 2** = launch parity push ·
> **Phase 3** = post-launch correctness · **Phase 4** = post-launch capability. **§3 is the correction log**
> for where the *previous* A–F framing of this doc was wrong vs the code — read it first if you remember the
> old structure.
>
> **Single source of truth.** Per-symbol disposition lives in **`COMPAT.toml`**, generated from
> `scripts/_compat_catalog.py` (guarded in CI). **Never hand-edit `COMPAT.toml`** — change dispositions in
> the catalog and regenerate.

## 1. Snapshot (verified 2026-06-19)

- **Gate:** 1349 Rust tests + 593 pytest fns floor (locally 658 pytest passed / 1 skipped / 8 xfailed) green ·
  clippy `-D warnings` clean on both lean and OCR variants · 0 real `TODO`/`unimplemented!`/`panic!` in
  `crates/` (4 guarded `unreachable!`) · full 7-job CI matrix over 3 OSes × 4 Pythons + an OIDC
  trusted-publishing `release.yml`. (The only local failures — 3 Rust + 7 pytest OCR tests — stem from a
  broken local tesseract/leptonica install, an env defect, not code; a clean machine still meets the
  1349/593 floors.)
- **API parity:** **651 / 769 implemented (84.7%)** — consistent across `COMPAT.toml`, README, and PARITY.md.
  52 deferred · 66 out-of-scope. **Most of the 52 deferred are achievable WITHOUT** the Font refactor → parity
  can still reach **~89–90%**.
- **Text extraction:** at fitz parity for **single-column** born-digital / CJK / EUR-Lex / GovInfo. **NOT at
  parity for multi-column** — committed GT shows PMC journals at order 0.08–0.44 vs fitz ~0.99, born-digital
  0/6 match-or-exceed (see §3 C4, Phase 3). Arabic/bidi beats fitz (logical order, UAX#9 reorder).
- **Rendering (`get_pixmap`):** SSIM **0.945 mean / 0.986 median** vs fitz — but the 3 worst pages
  (0.527/0.541/0.558) are all **blank non-embedded standard-14 body text** (Phase 1's top fix).
- **OCR:** Tesseract adapter + pure-Rust PaddleOCR (PP-OCRv4 via `tract`) both shipped, Python-selectable,
  scanned→searchable proven end-to-end, beats fitz on CJK. Wheel-bloat **resolved** and the publishing path
  is **decided + implemented** (P0-5r): the published `pdfspine` wheel compiles OCR in but embeds **no
  models** (~7 MB compressed; the `cargo` build default stays lean), and the ~16 MB models ship as a separate
  `pdfspine-ocr-models` data distribution the `[ocr]` extra pulls in (`pip install pdfspine[ocr]`); models
  resolve at runtime `PDFSPINE_OCR_MODELS` → companion → in-repo dev fallback (offline, no download). The
  per-box `recognize()` loop is still sequential (→ P4-3).
- **Performance:** pdfspine **beats fitz on open (1.26×)** and **text (2.75×)** — the ops users actually
  call; render is 1.74× faster than before but still ~2.3× slower than pypdfium2 (a C engine, not the parity
  target). **All render-perf work is deferred** — see §5 "Deferred for v1".

## 2. Harness (reuse, don't rebuild)

Objective ground-truth + differential harness in `conformance/gt/` (scripts committed; corpora/cache/
`*-results.json` gitignored, regenerable):
- `run_gt.py` — scores pdfspine vs fitz vs pdfminer vs SAME ground truth → `GT-REPORT.md`.
- `score.py` — decomposed metrics (lev/f1/jaccard/order), CJK-aware, NFKC.
- Fetchers: `born_digital.py`, `born_cjk.py`, `born_arabic.py`, `pmc_fetch.py`, `fetch_eurlex.py` (8 langs),
  `fetch_govinfo.py`, `fetch_robustness.py` (GovDocs1/SafeDocs, zip64 range-extraction).
- `tables_diff.py` — find_tables vs fitz (currently fitz-**agreement** only, no gold GT — see Phase 3).
  `render_diff.py` — get_pixmap vs fitz (SSIM).
- Real-corpus differential: `conformance/run_validation.py` + `fetch_corpus.py`.
- **Venvs:** `.venv` (pdfspine wheel) is the engine under test; `.venv-oracle` (real fitz 1.27 + pdfminer +
  pypdfium2 + rapidocr) is the GROUND-TRUTH oracle. **In `.venv`, `import fitz` is the pdfspine SHIM** — for
  true correctness always cross-check against `.venv-oracle`. No oracle output is ever committed. **Caveat:**
  `.venv-oracle` is absent on this machine, so local oracle cross-checks are currently unavailable — re-create
  it per this section to restore them.
- **Reproducibility debt — resolved in P0-6:** the 7 committed manifests/reports no longer hard-code
  `/workspace/pypdf` (de-absolutized to repo-relative; `run_gt.py` resolves corpora after a rename). **Minor
  residual (→ P0-6r):** the gitignored/regenerable corpus manifests (`conformance/gt/corpus-*/manifest.json`)
  still embed absolute paths — `fetch_corpus.py` should emit manifest-relative pdf paths so regenerations stay
  rename-proof.

## 3. Correction log — where the previous A–F framing was wrong

This doc used to be organized as priority areas A (OCR) / B (parity) / C (rendering) / D (extraction) /
E (perf) / F (polish) with HIGH/MEDIUM/LOW labels. A code-verified survey found that framing materially
mis-stated the work. The phased plan in §4 replaces it. Key corrections (all carry `file:line` evidence):

| # | Old framing said | Verified reality | Where |
|---|---|---|---|
| C1 | (silent) | **Version hard-pinned `0.0.0`**; no tag→version in `release.yml`. A tagged release ships a `0.0.0` wheel. **Hard blocker.** **✅ FIXED in Phase 0 (P0-1).** | `pyproject.toml:8`, `Cargo.toml:24`, `crates/py-bindings/src/lib.rs:33`, `.github/workflows/release.yml:48` |
| C2 | §7: "always `PdfUnsupportedError`, never `AttributeError`" | **~50 of 56 deferred symbols raise bare `AttributeError`.** `_UNIMPLEMENTED_*` maps cover only 2; the guard never checks runtime behavior. **✅ FIXED in Phase 0 (P0-2)** for the 40 Page/Document deferred symbols; 12 deferred members on non-subclassable `_core` types still `AttributeError` (→ P0-2r). | `python/pdfspine/document.py:37,2276,3700`, `scripts/compat-symbol-guard.py` |
| C3 | §F: Font-program refactor is the keystone that unblocks std-14 rendering | **False.** Renderer builds its own `GlyphFont` from `/FontFile*`; never consults `pdf_fonts::Font`. Std-14 fix = bundle a fallback family (independent). Refactor payoff = +2 API symbols only. | `crates/pdf-render/src/render.rs:497,528`, `crates/pdf-fonts/src/font.rs` |
| C4 | §D: "text already at parity, diminishing returns" | **Overstated.** PMC 5/12 docs order 0.08–0.44 vs fitz ~0.99; born-digital 0/6 match-or-exceed. Parity holds **single-column only**. | `conformance/gt/GT-REPORT-pmc.md`, `GT-REPORT-born.md` |
| C5 | (an earlier draft claimed a 648-vs-647 count drift) | **No drift** — that "648" was a `grep` artifact counting the comment legend line. COMPAT body, `[meta]`, README, and PARITY are all consistent at **647 / 84.1%**. No doc-number change needed. | `COMPAT.toml:31`, `README.md:14`, `PARITY.md:40` |
| C6 | 4 symbols listed deferred | `Page.links`, `Page.first_link`, `Document.outline`, `Document.extract_image` are **implemented + live** (missed by `_reconcile_batch34`). Free +4. **✅ FIXED in Phase 0 (P0-3)** — flipped + COMPAT regenerated to 651 / 84.7%. | `python/pdfspine/document.py:1513,1518,3206,3038` |
| C7 | §3.B `convert_to_pdf` = "heavy ops" / stub | Rust impl **finished** (`imagedoc::convert_to_pdf`, `image_to_pdf`); just **unexposed** in py-bindings. Image-input case is a small binding task. | `crates/pdf-image/src/imagedoc.rs:137`, `crates/pdf-api/src/image.rs:206` |
| C8 | §E render-perf TODOs (glyph cache, q/Q clones, rayon, JPEG2000) | Mostly mis-stated: outline-Path **and** ObjRef program caches **exist**; paints split-borrow (only `Canvas::save` clones); `get_pixmap` **already releases the GIL** (cross-page parallelism works today); JPEG2000 is **already a wired codec** (upstream-bound). | `crates/pdf-render/src/render.rs:434,471`, `canvas.rs:173`, `py-bindings/src/lib.rs:1802`, `codecs/jpx.rs` |
| C9 | §C: "synthetic-bold renders heavier than fitz" | **Unsupported** — no embolden code path exists. If anything pdfspine renders *lighter*. | `crates/pdf-render` |
| C10 | (silent) | **3 CI "guard" scripts are always-exit-0 M0 stubs** wired as named green checks (incl. the AGPL/license-provenance gate) — false confidence. **✅ FIXED in Phase 0 (P0-4)** — all 3 now enforce real invariants (proven by canary-and-revert); license provenance is enforced affirmatively via an allowlist instead of a raw GPL byte-scan (which false-positived on real public-domain fixtures). | `scripts/{test-order-guard,catalog-status-guard,manifest-lint}.py`, `.github/workflows/ci.yml:152` |
| C11 | §5: "folder rename = FINAL step" | Folder rename is **done** (cwd is `pdfspine`), but 7 committed files still hard-code `/workspace/pypdf` — harness reproducibility is broken until cleaned. **✅ FIXED in Phase 0 (P0-6)** — committed reports de-absolutized to repo-relative; only the gitignored/regenerable corpus manifests remain (→ P0-6r). | `conformance/gt/corpus-*/manifest.json`, `GT-REPORT-tables.md` |
| C12 | (silent) | **Silent wrong-answers:** `Page.get_text(clip=...)` drops the clip for `text/dict/json/html`; `Font(fontfile=/fontbuffer=)` silently returns Helvetica; `get_textpage_ocr(full=False)` silently does full-page OCR. | `crates/py-bindings/src/lib.rs:1481,4081,1527` |
| C13 | §3.B Pixmap(3) grouped flat | `samples_ptr` / `__array_interface__` are **small** (ride the existing `samples_mv` buffer protocol); only `warp` is medium. | `crates/py-bindings/src/lib.rs:3684,3697` |
| C14 | §E OCR: only "more languages" + "model distribution" | **OCR `recognize()` per-box loop is sequential** — rayon-parallelizing it is the single biggest *absolute-time* OCR speedup (seconds/page), far bigger than any render-perf item. | `crates/pdf-ocr/src/paddle/mod.rs:77` |

## 4. Phased plan

Effort: **S** ≈ hours · **M** ≈ 1–2 days · **L** ≈ multi-day. Each task lists **why · files · effort/impact
· Acceptance** (the green condition that means "done").

### Phase 0 — COMPLETE (2026-06-19) — committed on branch `phase0-blockers` (98a437a; P0-5r in ff6495c)

All six blockers landed and were verified (full §8 suite). Done summary:

- **P0-1 · Version-from-tag** — DONE. `pyproject.toml` `dynamic = ["version"]`; `__version__` resolves via `importlib.metadata` (falls back to `_core.__version__` in a raw tree); new `scripts/set_version_from_tag.py` bumps the workspace `Cargo.toml` + first-party path-dep reqs; `release.yml` gained a tag-guarded set-version step in both the wheels matrix and sdist jobs. Dev tree stays `0.0.0`; a tagged CI build ships the tag (proven with `v9.9.9`, reverted).
- **P0-2 · Deferred → `PdfUnsupportedError`** — DONE for the 40 Page/Document deferred symbols. Catalog now emits `python/pdfspine/_compat_deferred.py` (a self-maintaining `frozenset`, ships in the wheel); `Page/Document.__getattr__` route deferred members through it → `PdfUnsupportedError` with name+hint. Dead `_UNIMPLEMENTED_PAGE['get_pixmap']` removed; new `test_deferred_symbols.py`; `compat-symbol-guard.py` extended with lockstep + runtime checks. **Residual → P0-2r.**
- **P0-3 · Flip 4 + regen COMPAT** — DONE. The 4 flipped deferred→implemented in `_compat_catalog.py`; COMPAT regenerated to **651 / 52 / 66 = 769, coverage 84.7%**; README + PARITY counts refreshed (Document 119/17, Page 92/23). No count drift; the 4 verified live/non-stub via the in-process fitz shim (`.venv-oracle` unavailable — see §2).
- **P0-4 · Real CI guards** — DONE. All three (`test-order-guard`, `catalog-status-guard`, `manifest-lint`) now enforce real invariants, each proven by a canary-and-revert: catalog-guard re-renders COMPAT + baseline in-process and byte-compares; test-order-guard enforces 1:1 between catalog `red` rows and `RED: <ID>` source tags; manifest-lint enforces the affirmative-license allowlist + well-formed sha256 + no stale absolute paths. The raw GPL byte-scan was dropped (false-positived on real public-domain fixtures).
- **P0-5 · OCR wheel-bloat → feature-split** — DONE. `paddle-ocr` is default-OFF across pdf-ocr / pdf-api / py-bindings (`ocr` alias on py-bindings); models load at runtime from `PDFSPINE_OCR_MODELS`/`models/`. Lean default cdylib **6.75 MB** (was 37.1 MB); OCR build 21.4 MB; lean install raises `PdfUnsupportedError` pointing to `pdfspine[ocr]`. **Residual P0-5r — ✅ RESOLVED (see below).**
- **P0-6 · CHANGELOG + stale paths** — DONE. New `CHANGELOG.md` (Keep-a-Changelog + SemVer); `GT-REPORT-tables.md` de-absolutized to repo-relative; `run_gt.py` already resolves repo-root-relative. **Residual → P0-6r.**

**Residuals carried forward:**

- **P0-2r · 12 `_core` PyO3 deferred members still raise `AttributeError`** — *Rust-core* — the deferred members on non-subclassable `_core` types (Pixmap / DisplayList / Tools) can't route through the Python `__getattr__` path; making them raise `PdfUnsupportedError` needs a Rust-core change (outside the Python-scoped P0-2). Tracked as xfail + a COMPAT note.
- **P0-5r · OCR publishing — ✅ RESOLVED (2026-06-19, commit `ff6495c`)** — chose **Option A: a model-data companion + the OCR feature compiled into the published wheel**. The published `pdfspine` wheel compiles the `ocr` feature in (via `[tool.maturin] features`) but embeds no models; the ~16 MB models ship as a new `pdfspine-ocr-models` data distribution (`packages/pdfspine-ocr-models/`, hatchling force-include from `crates/pdf-ocr/models` — no git duplication) that the `[ocr]` extra depends on. `document.py` sets `PDFSPINE_OCR_MODELS` from the installed companion for `engine="paddle"`; resolution order PDFSPINE_OCR_MODELS → companion → in-repo dev fallback → clear `PdfUnsupportedError`. `release.yml` publishes both dists via OIDC trusted publishing; `docs/RELEASE-PYPI.md` §D.1 documents the flow.
- **P0-6r · `fetch_corpus.py` relative paths** — the gitignored/regenerable corpus manifests (`conformance/gt/corpus-*/manifest.json`) still embed absolute paths; `fetch_corpus.py` should emit manifest-relative pdf paths so future regenerations stay rename-proof.

### Phase 1 — Pre-launch quality (cheap + high-credibility)

- **P1-1 · Bundle a permissive fallback family for non-embedded standard-14 fonts** — *M · High*
  - **Why:** the single highest visual-correctness fix. Non-embedded Helvetica/Times/Courier body text renders **BLANK** (`resolve_font_program` → `None` → `draw_text` early-returns); the 3 worst RENDER-REPORT pages are all "missing body text". Metrics exist (`std_widths.rs`); only outlines are missing. **Independent of the Font refactor** (C3).
  - **Files:** `crates/pdf-render/src/render.rs:451,497`, `crates/pdf-fonts/src/std_widths.rs`, `crates/pdf-render/src/render.rs:30`.
  - **Decision:** Liberation (SIL OFL) vs Nimbus/URW — needs license vetting + NOTICE/provenance entry.
  - **Acceptance:** the 3 worst pages climb from ~0.55 toward ~0.95 SSIM (`render_diff.py`); aggregate mean rises; NOTICE updated.

- **P1-2 · Honor `/Decode [1 0]` on stencil ImageMask** — *S · Medium*
  - **Why:** `draw_image_mask` hardcodes default `/Decode [0 1]` and never reads the dict, so a `/Decode [1 0]` mask paints **fully inverted** (common in scanned/forms content). Cheap; do it while in the render path with P1-1.
  - **Files:** `crates/pdf-render/src/image.rs:118,149`, `crates/pdf-render/src/render.rs:1016`.
  - **Acceptance:** an `/ImageMask` with `/Decode [1 0]` renders correctly; regression test added.

- **P1-3 · Wire a real extraction/render CI regression gate** — *M · High*
  - **Why:** ci.yml has lint/test/pytest/wheels but **no accuracy/SSIM job**, and the only "guards" are the M0 stubs (C10). Without it, the std-14 fix and every later extraction fix can silently regress.
  - **Approach:** commit a tiny born-digital fixture set with inlined `gt_text` (`corpus-born/manifest.json` already inlines it); add a no-oracle pdfspine-vs-baseline mode to `run_gt.py` asserting per-doc `order >= threshold`; add SSIM via `render_diff.py` vs committed reference buffers.
  - **Files:** `.github/workflows/ci.yml:141`, `conformance/gt/run_gt.py:516`, `conformance/gt/render_diff.py:377`, `.gitignore:48`.
  - **Acceptance:** CI fails if born-digital order drops below threshold or a reference page's SSIM regresses.

### Phase 2 — Launch parity push (pure-Python / binding clusters → ~86–90%)

- **P2-1 · Page draw-convenience + loader/alias cluster (~12 symbols, pure-Python)** — *M · High (parity)*
  - **Why:** biggest parity-%-per-effort batch; all ride existing infra. `draw_curve/draw_quad/draw_sector/draw_squiggle/draw_zigzag` → already-implemented `Shape` methods via the proven `Page.draw_line/rect` pattern; `load_links` = alias of `get_links`; `update_link` = `delete_link`+`insert_link`; `load_annot`/`load_widget` index `annots()`/`widgets()`; `delete_widget` = `delete_annot`; `cluster_drawings` ports fitz's pure-Python algorithm over `get_drawings()`; `is_wrapped` exposes the existing wrap predicate.
  - **Files:** `python/pdfspine/document.py:748-874,1768,1782,2053,2098,1622`.
  - **Acceptance:** symbols flipped in the catalog + regen; each cross-checked vs `.venv-oracle`; new tests in `python/tests/test_longtail12.py`.

- **P2-2 · Wire `Document.convert_to_pdf` (image inputs)** — *M · Medium*
  - **Why:** Rust impl is finished but unexposed (C7); image-input case is a binding task. Lets `Document.open` transparently handle image files.
  - **Files:** `crates/pdf-image/src/imagedoc.rs:137`, `crates/pdf-api/src/image.rs:206`, `crates/py-bindings/src/lib.rs`, `python/pdfspine/document.py:42`.
  - **Acceptance:** works for image bytes; raises `PdfUnsupportedError` only for non-image input (fitz-correct); +1 symbol.

- **P2-3 · Small binding clusters** — *S–M · Low–Medium (parity)*
  - `Pixmap.samples_ptr` + `__array_interface__` (ride `samples_mv` buffer protocol — C13) · `crates/py-bindings/src/lib.rs:3684,3697`.
  - `Tools.image_profile` + `module.image_profile` (2 symbols; decoders + metadata-dict shape exist) · `crates/pdf-image/src/{imagedoc.rs:70,getpixmap.rs:172}`.
  - `Page.language` / `set_language` (mirror the implemented Annot `/Lang` accessors) · `crates/py-bindings/src/lib.rs:753,3266`.
  - `Page.set_contents` · `Document.get_outline_xrefs` · `embfile_upd` (on existing `update_stream` / `/Outlines` walker / embfile infra) · `crates/py-bindings/src/lib.rs:2825,3361`, `crates/pdf-edit/src/toc.rs:52`.
  - **Acceptance:** each flipped + regen + oracle-checked + tested.

- **P2-4 · Medium parity items** — *M · Low (parity)*
  - `set_toc_item` / `del_toc_item` / `version_count` (surgical `/Outlines` edits; `version_count` ≈ `/Prev`-chain count, `writer.rs:280`); `extract_font`; `subset`; `add_widget` / `add_caret_annot`.
  - **Acceptance:** semantics validated vs `.venv-oracle` (fitz's in-place rewrite, not full rebuild).

### Phase 3 — Post-launch correctness

- **P3-1 · Multi-column reading-order engine** — *L · High*
  - **Why:** the real-world text gap (C4). The recursive XY-cut + gutter engine exists but mis-cuts browser-CSS-multicolumn and real 2-col journal gutters.
  - **Files:** `crates/pdf-text/src/layout.rs:575,700,1050`.
  - **Acceptance:** PMC + born-digital GT order rises from ~0.4–0.65 toward fitz's ~0.98 (`run_gt.py`); no single-column regression; the P1-3 gate guards it.

- **P3-2 · Investigate PMC near-zero `lev` collapse** — *M · High*
  - **Why:** large 2-col PMC papers score `lev` 0.000–0.003 with a 4× jaccard gap — suggests dropped content beyond ordering. Diagnose: render one page, diff `get_text` vs fitz to localize.
  - **Files:** `crates/pdf-text/src/layout.rs:447`, `conformance/pdfspine_worker.py` (re-fetch via `pmc_fetch.py`).
  - **Acceptance:** root cause identified; if a content bug, fixed with a regression test.

- **P3-3 · Expand Indexed/Separation/DeviceN colorspaces + apply `/Decode` (render path)** — *L · Medium*
  - **Why:** Indexed images render as raw palette indices and Separation/DeviceN as raw tint values (palette/tint transform never run); `/Decode` honored only inside DCT. Separately, vector `scn` for a 1-component Separation maps tint 1.0 → **white** (inverted for dark spot inks); `cs`/`CS` are no-ops. The `PdfFunction` evaluator already exists. (ICC is explicitly **out** — large/low-value, keep as a documented deviation.)
  - **Files:** `crates/pdf-image/src/pixmap.rs:179`, `crates/pdf-image/src/codecs/mod.rs:221,248`, `crates/pdf-text/src/interp.rs:684,698,1402`, `crates/pdf-render/src/render.rs:1012`.
  - **Acceptance:** indexed + Separation/DeviceN images render with correct colors; dark spot-color fill no longer white; render_diff SSIM improves on affected govinfo/eurlex forms.

- **P3-4 · Cheap correctness insurance** — *S–M · Low–Medium*
  - **Kangxi-radical CJK fold** (NFKC at `predefined.rs:141 cid_to_unicode`, gated to CJK ranges) + a unit test · *S*.
  - **Edge-case tests:** vertical/Identity-V CJK (`wmode` plumbing exists, never exercised), ToUnicode-less Type0, overlapping/co-located text · *M*.
  - **Robustness at scale:** rerun `fetch_robustness.py --n 250+` GovDocs1/SafeDocs so "never panics" rests on thousands, not 30 clean PDFs · *S, benchmarking only*.
  - **Acceptance:** new tests/fold green; a refreshed robustness report.

- **P3-5 · FinTabNet gold-table GT** — *M · Medium* (optional)
  - **Why:** today's tables harness is fitz-**agreement** only (36–43% IoU match); no objective structure score. Wire FinTabNet.c (CDLA-Permissive) for the first absolute number.
  - **Acceptance:** an absolute cell-structure score vs human GT in a committed report.

### Phase 4 — Post-launch capability / strategic

- **P4-1 · Font handle carries `/FontFile*` program bytes** — *L · Medium (API)* · **NOT the rendering keystone (C3)**
  - **Why:** unblocks `Font.glyph_bbox`, `Font.buffer`, richer `valid_codepoints`, and user `Font(fontbuffer=)` for `insert_text` (+2 parity). Do it for **API completeness**, not rendering.
  - **Files:** `crates/pdf-fonts/src/font.rs:185,341,376`, `crates/pdf-render/src/text.rs:68`, `crates/py-bindings/src/lib.rs:4081,4229`.
  - **Acceptance:** `Font(fontfile=)` no longer silently falls back to Helvetica; `buffer`/`glyph_bbox` implemented + tested.

- **P4-2 · Type1 charstring (PFB/PFA) support** — *L · Medium* (after P1-1)
  - **Why:** removes the literal worst page (eurlex `32006L0112_ES`, SSIM 0.527). Type1 embedding is rare, and once P1-1 lands, descriptor-flag substitution covers most blank Type1 fonts cheaply. Route: Type1→CFF, or a permissive pure-Rust Type1 outliner (respect the Apache-2.0 / pure-Rust positioning).
  - **Files:** `crates/pdf-render/src/text.rs:93`, `crates/pdf-render/src/render.rs:955`.

- **P4-3 · OCR `recognize()` parallelism (rayon)** — *M · High (OCR latency)* · best perf-for-effort overall
  - **Why:** the per-box loop is sequential and CPU-bound; a scanned page has dozens–hundreds of boxes. rayon `par_iter` near-linearly cuts OCR wall time (seconds/page). The runnable cache is already `&self` + `Mutex`-guarded (C14).
  - **Files:** `crates/pdf-ocr/src/paddle/mod.rs:77`, `crates/pdf-ocr/src/paddle/model.rs:104,135`.
  - **Acceptance:** deterministic result order (collect indexed, sort by box order); measurable multi-core speedup; no correctness change.

- **P4-4 · API reference docs cover the full public surface** — *M · Medium*
  - **Why:** `docs/reference/` documents only 4 of ~20 public classes, hand-written (no mkdocstrings), so it drifts. Docstrings are already rich.
  - **Files:** `docs/reference/`, `mkdocs.yml`, `python/pdfspine/__init__.py:29`.
  - **Acceptance:** every exported class/function documented (ideally auto-generated from docstrings).

## 5. Deferred for v1 (do NOT spend effort here pre-release)

- **All render-perf work:** the outline-Path + ObjRef program caches already exist; only per-occurrence
  rasterization remains, and a coverage-mask cache risks sub-pixel-AA regressions. pdfspine already beats
  fitz on open + text. Ship without it. (Optional later: per-glyph coverage-mask cache, clip-bbox bounding,
  `Rc/Arc<Mask>` on `Canvas::save` to avoid q/Q clones; per-page rayon is unnecessary — `get_pixmap`
  already releases the GIL so cross-page threading works today.)
- **ICC-accurate colorspace transform:** large pure-Rust undertaking, marginal SSIM gain — documented deviation.
- **OCMD/layers (7), `Page.run`/`DisplayList.run`/`get_textpage` (device-callback replay),
  `Page.remove_rotation`, `Annot.get_textbox`, `convert_to_pdf` non-image:** genuinely blocked (need
  `/OCProperties`+OCMD plumbing, a device-replay engine, content-stream rewriting, annot-appearance textpage).
  Keep deferred; documenting prevents wasted effort.
- **Splitting the `lib.rs` (4711 lines) / `document.py` (3738 lines) monoliths:** real friction, zero
  correctness impact, churn risk — well after release.
- **Additional OCR languages:** the bundled `ch` model covers CJK+Latin; each extra lang is ~10 MB that
  compounds the wheel-bloat problem — only after P0-5.
- **DocLayNet / more-language corpus breadth:** lower priority than P3-1, which dominates any new corpus's numbers.

## 6. Task index

| ID | Title | Effort | Impact | Status | Phase |
|----|-------|:--:|:--:|:--:|:--:|
| P0-1 | Version-from-tag | S | High | ✅ done | 0 |
| P0-2 | Deferred → PdfUnsupportedError contract | M | High | ✅ done | 0 |
| P0-3 | Flip 4 mislabeled symbols + regen COMPAT (→84.7%) | S | Low | ✅ done | 0 |
| P0-4 | Implement/delete 3 fake CI guards | M | Med | ✅ done | 0 |
| P0-5 | OCR wheel-bloat decision (opt-in extra) | M | Med | ✅ done | 0 |
| P0-6 | CHANGELOG + fix stale `/workspace/pypdf` paths | S | Low | ✅ done | 0 |
| P0-2r | 12 `_core` PyO3 deferred members still AttributeError (Rust-core) | M | Med | open | 0r |
| P0-5r | OCR publishing — model-data companion + OCR-in-wheel (Option A) | M | High | ✅ done | 0r |
| P0-6r | `fetch_corpus.py` emits manifest-relative pdf paths | S | Low | open | 0r |
| P1-1 | Bundle std-14 fallback fonts (blank body text) | M | High | – | 1 |
| P1-2 | Honor `/Decode [1 0]` ImageMask | S | Med | – | 1 |
| P1-3 | Real extraction/render CI regression gate | M | High | – | 1 |
| P2-1 | Page draw-convenience + loader/alias (~12 syms) | M | High | – | 2 |
| P2-2 | `Document.convert_to_pdf` (image inputs) | M | Med | – | 2 |
| P2-3 | Small binding clusters (Pixmap/Tools/Page/Doc) | S–M | Low–Med | – | 2 |
| P2-4 | Medium parity (TOC edit, extract_font, subset…) | M | Low | – | 2 |
| P3-1 | Multi-column reading-order engine | L | High | – | 3 |
| P3-2 | PMC near-zero-lev investigation | M | High | – | 3 |
| P3-3 | Indexed/Separation/DeviceN + `/Decode` (render) | L | Med | – | 3 |
| P3-4 | Kangxi fold + edge-case tests + robustness rerun | S–M | Low–Med | – | 3 |
| P3-5 | FinTabNet gold-table GT | M | Med | – | 3 |
| P4-1 | Font handle carries `/FontFile*` (API only) | L | Med | – | 4 |
| P4-2 | Type1 charstring (PFB/PFA) support | L | Med | – | 4 |
| P4-3 | OCR `recognize()` rayon parallelism | M | High | – | 4 |
| P4-4 | Full public-surface API reference docs | M | Med | – | 4 |

**Recommended next 3 (in order):** *(Phase 0 + P0-5r COMPLETE — committed on `phase0-blockers`.)*
1. **Phase 1 — P1-1 std-14 fallback fonts** (+ the one-line P1-2 while in the render path, + P1-3 to lock quality) — the top visual-correctness fix now that the blockers are cleared.
2. **Phase 2 — P2-1 Page parity batch** (+ P2-2 convert_to_pdf) — biggest parity-%-per-effort momentum (~86%), no Rust-architecture risk.
3. Then the large **P3-1 multi-column** and **P4-1 Font** work, guarded by P1-3. (The remaining Phase 0 residuals P0-2r / P0-6r are low-priority cleanups.)

## 7. Pre-public chores + docs upkeep (do alongside / last)

- Reword any historical commit messages that contain backticks (shell substitutes them).
- **Keep `PARITY.md` + `docs/BENCHMARKS.md` current** — they drift as batches land; refresh after each.
- Docs-site completeness pass (`docs/guide`, `docs/reference`, `index.md`) — see P4-4.
- PyPI publish runbook: `docs/RELEASE-PYPI.md` (gated on P0-1 + P0-5); optional name trademark.
- The PyPI runbook **encodes the P0-5r OCR-distribution decision** (✅ done) — the published `pdfspine` wheel compiles the `ocr` feature in (no embedded models) and the `[ocr]` extra pulls the `pdfspine-ocr-models` data distribution; see `docs/RELEASE-PYPI.md` §D.1.
- Repo stays **PRIVATE** until everything is done (Phase 0 + Phase 1 + the parity push + docs), then flip to
  public + push (`gh` authed as VoldemortGin).

## 8. Verify suite (run from repo root before every commit)

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                      # expect 1349+ passed, 0 failed
source .venv/bin/activate && env -u CONDA_PREFIX maturin develop -q
env -u CONDA_PREFIX python -m pytest python/tests/ -q     # expect 593+ passed
python3 scripts/_compat_catalog.py && python3 scripts/compat-symbol-guard.py   # after any API batch
```

**Gotchas:** maturin needs `env -u CONDA_PREFIX`; commit messages must avoid backticks; only ONE agent
rebuilds the wheel at a time (shared `.venv`); always change dispositions in `scripts/_compat_catalog.py`
then regenerate — **never hand-edit `COMPAT.toml`** — and confirm coverage rises with zero regressions +
`compat-symbol-guard.py` exit 0; cross-check every API symbol against `.venv-oracle` (in `.venv`,
`import fitz` is the pdfspine shim). When a batch agent dies mid-run, check the working tree — it usually
left coherent, compiling work; verify + finish rather than restarting. **This machine's tesseract 5.5.2 /
leptonica-1.87.0 install is broken** (reproduces on a trivial external PNG: "Leptonica Error … image file
not found"), so the local OCR tests (3 Rust + 7 pytest) fail for env reasons, not code — ignore them here
and trust a clean machine / CI.

---

*Re-verified 2026-06-19 from a code-level 5-dimension survey (project health · API parity · rendering ·
extraction/conformance · perf/OCR). §3 is the correction log against this doc's previous A–F framing.
**Phase 0 + P0-5r landed on 2026-06-19** (committed on branch `phase0-blockers`: 98a437a, ff6495c) — §3
rows C1 / C2 / C6 / C10 / C11 are fixed; residuals P0-2r / P0-6r carried forward.*
