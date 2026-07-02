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
>
> **✅ Markdown → PDF original extension — COMPLETE (2026-07-02, in working tree).** See **§9**: a
> brand-new top-level `pdfspine.markdown_to_pdf()`, **NOT** part of the fitz-parity Phases 0–4 above.
> MD-0..MD-4 all landed 2026-07-02 (CJK **Option A**); P3-5 GriTS scored the same day. Remaining overall:
> the pre-public flip (§7) and the accepted/deferred items (§5, P3-1r).

## 1. Snapshot (verified 2026-06-19)

- **Gate:** 1349 Rust tests + 593 pytest fns floor (locally 658 pytest passed / 1 skipped / 8 xfailed) green ·
  clippy `-D warnings` clean on both lean and OCR variants · 0 real `TODO`/`unimplemented!`/`panic!` in
  `crates/` (4 guarded `unreachable!`) · full 7-job CI matrix over 3 OSes × 4 Pythons + an OIDC
  trusted-publishing `release.yml`. (The only local failures — 3 Rust + 7 pytest OCR tests — stem from a
  broken local tesseract/leptonica install, an env defect, not code; a clean machine still meets the
  1349/593 floors.)
- **API parity:** **682 / 769 implemented (88.7%)** — consistent across `COMPAT.toml`, README, and PARITY.md.
  21 deferred · 66 out-of-scope. Phase 2 landed +29; **P4-1** then added `Font.buffer`/`glyph_bbox` (Font class
  22/23). The remaining 21 deferred are the long tail (OCG layers, device-replay, a few Type0/Type3 edges).
- **Text extraction:** at fitz parity for **single-column AND multi-column**. The multi-column engine landed
  (06-16 PM) and **P3-2 verified it** (2026-06-20, fresh GT): PMC order **0.965 / 0.995** vs fitz 0.975/0.997,
  born-digital **0.996** vs 1.000 — within 0.000–0.009 per column doc (PMC212687 0.083→0.996, born 2col
  0.549→0.997). CJK / EUR-Lex / GovInfo at parity; Arabic/bidi beats fitz (logical order, UAX#9 reorder). One
  ordering-only residual: PMC212689 (order 0.645, tokens all present) → P3-1r.
- **Rendering (`get_pixmap`):** **SSIM 0.984 mean / 0.989 median** vs fitz (re-measured 2026-06-21 over 46
  docs) — **at/near parity**, up from 0.945 once the render-fidelity fixes landed: **P1-1** Liberation std-14
  (the worst pages were blank non-embedded standard-14 text), **P3-3** Indexed/Separation/DeviceN + `/Decode`,
  **P3-3r** CMYK black point (pure-K → 34,31,31 exact; saturated-CMY still differs, an inherent ICC limit),
  **P4-2** embedded Type1 (eurlex `32006L0112_ES` 0.527→0.993), **P1-1r** Symbol/ZapfDingbats (OFL Noto). No
  page is below 0.92 now (was 3 below 0.72); the new worst-10 are AA/hinting sub-pixel residuals, not missing
  content.
- **OCR:** Tesseract adapter + pure-Rust PaddleOCR (PP-OCRv5 via `tract`) both shipped, Python-selectable,
  scanned→searchable proven end-to-end, beats fitz on CJK. Wheel-bloat **resolved** and the publishing path
  is **decided + implemented** (P0-5r): the published `pdfspine` wheel compiles OCR in but embeds **no
  models** (~7 MB compressed; the `cargo` build default stays lean), and the ~16 MB models ship as a separate
  `pdfspine-ocr-models` data distribution the `[ocr]` extra pulls in (`pip install pdfspine[ocr]`); models
  resolve at runtime `PDFSPINE_OCR_MODELS` → companion → in-repo dev fallback (offline, no download). The
  per-box `recognize()` loop is now **rayon-parallel** (P4-3 done: 3.49× on a 42-box page, byte-identical).
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
- **Venvs:** `.venv` (pdfspine wheel) is the engine under test; `.venv-oracle` (real fitz 1.24.14 + pdfminer +
  pypdfium2 + rapidocr) is the GROUND-TRUTH oracle. **In `.venv`, `import fitz` is the pdfspine SHIM** — for
  true correctness always cross-check against `.venv-oracle`. No oracle output is ever committed. **Set up
  (2026-06-20):** `.venv-oracle` holds real PyMuPDF and is the live fitz reference (`.venv-oracle/bin/python`).
  **⚠ Version drift found 2026-07-02:** it now actually holds **1.27.2.3**, not the documented 1.24.14 COMPAT
  baseline (someone upgraded it). For baseline-pinned adjudications, spin a pinned venv:
  `python -m venv <dir> && <dir>/bin/pip install pymupdf==1.24.14` (the 2026-07-02 encryption adjudication
  ran both — identical answers on every probed point).
- **Reproducibility debt — RESOLVED (P0-6 + P0-6r):** committed manifests/reports no longer hard-code
  `/workspace/pypdf`, and `run_gt.py` now resolves each corpus pdf/nxml relative to its manifest dir (falling
  back to the basename beside the manifest), so even the gitignored corpus manifests' stale absolute paths no
  longer break scoring after a rename. (Closed while regenerating the GT reports during P3-2.)

## 3. Correction log — where the previous A–F framing was wrong

This doc used to be organized as priority areas A (OCR) / B (parity) / C (rendering) / D (extraction) /
E (perf) / F (polish) with HIGH/MEDIUM/LOW labels. A code-verified survey found that framing materially
mis-stated the work. The phased plan in §4 replaces it. Key corrections (all carry `file:line` evidence):

| # | Old framing said | Verified reality | Where |
|---|---|---|---|
| C1 | (silent) | **Version hard-pinned `0.0.0`**; no tag→version in `release.yml`. A tagged release ships a `0.0.0` wheel. **Hard blocker.** **✅ FIXED in Phase 0 (P0-1).** | `pyproject.toml:8`, `Cargo.toml:24`, `crates/py-bindings/src/lib.rs:33`, `.github/workflows/release.yml:48` |
| C2 | §7: "always `PdfUnsupportedError`, never `AttributeError`" | **~50 of 56 deferred symbols raise bare `AttributeError`.** `_UNIMPLEMENTED_*` maps cover only 2; the guard never checks runtime behavior. **✅ FIXED in Phase 0 (P0-2)** for the 40 Page/Document deferred symbols; the 5 deferred members on non-subclassable `_core` types then **✅ FIXED in P0-2r** (Rust `__getattr__`). | `python/pdfspine/document.py:37,2276,3700`, `scripts/compat-symbol-guard.py` |
| C3 | §F: Font-program refactor is the keystone that unblocks std-14 rendering | **False.** Renderer builds its own `GlyphFont` from `/FontFile*`; never consults `pdf_fonts::Font`. Std-14 fix = bundle a fallback family (independent). Refactor payoff = +2 API symbols only. | `crates/pdf-render/src/render.rs:497,528`, `crates/pdf-fonts/src/font.rs` |
| C4 | §D: "text already at parity, diminishing returns" | Read as overstated off a **stale 06-16-morning report** (PMC 0.08–0.44, born 0/6); the multi-column engine landed 06-16 PM and **✅ P3-2 verified parity** (2026-06-20): PMC order 0.965/0.995, born 0.996 vs fitz, reports regenerated. Residual: PMC212689 ordering (→ P3-1r). | `conformance/gt/GT-REPORT-pmc.md`, `GT-REPORT-born.md` |
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

- **P0-2r · `_core` PyO3 deferred members raised `AttributeError`** — *Rust-core* — **✅ FIXED (2026-06-21).** A Rust `__getattr__` on Pixmap / DisplayList / Tools makes their 5 deferred members (Pixmap.warp, DisplayList.get_textpage/run, Tools.set_annot_stem/set_subset_fontnames) raise `PdfUnsupportedError` on instance access (not `AttributeError`); xfails flipped to real asserts + a drift guard added. (The deferred set is **5 members**, not the 12 an earlier estimate carried.)
- **P0-5r · OCR publishing — ✅ RESOLVED (2026-06-19, commit `ff6495c`)** — chose **Option A: a model-data companion + the OCR feature compiled into the published wheel**. The published `pdfspine` wheel compiles the `ocr` feature in (via `[tool.maturin] features`) but embeds no models; the ~16 MB models ship as a new `pdfspine-ocr-models` data distribution (`packages/pdfspine-ocr-models/`, hatchling force-include from `crates/pdf-ocr/models` — no git duplication) that the `[ocr]` extra depends on. `document.py` sets `PDFSPINE_OCR_MODELS` from the installed companion for `engine="paddle"`; resolution order PDFSPINE_OCR_MODELS → companion → in-repo dev fallback → clear `PdfUnsupportedError`. `release.yml` publishes both dists via OIDC trusted publishing; `docs/RELEASE-PYPI.md` §D.1 documents the flow.
- **P0-6r · `fetch_corpus.py` relative paths** — the gitignored/regenerable corpus manifests (`conformance/gt/corpus-*/manifest.json`) still embed absolute paths; `fetch_corpus.py` should emit manifest-relative pdf paths so future regenerations stay rename-proof.

### Phase 1 — COMPLETE (2026-06-19) — committed on `main`

All three pre-launch quality items landed and were verified (full §8 suite; the new accuracy gate green). Done summary:

- **P1-1 · Liberation std-14 fallback fonts** — DONE. Bundled the 12 base-14-covering **Liberation 2.1.5** faces (**SIL OFL 1.1**, ~4.2 MB) under `crates/pdf-fonts/fonts/liberation/`; `render.rs::liberation_substitute` maps standard-14 names (+ Arial/Times New Roman/Courier New aliases, refined by `/FontDescriptor` serif/fixed-pitch/italic/force-bold) to them when a simple font has no embedded `/FontFile*`. Non-embedded Helvetica/Times/Courier body text now renders real glyphs instead of blank (real-page ink coverage +5..+10 pts; a bare `/Helvetica` with no `/FontDescriptor` also covered). `std_widths` stays authoritative for advances. NOTICE + per-dir PROVENANCE + `docs/guide/license.md` carry the OFL provenance. **Residual → P1-1r — ✅ FIXED 2026-06-21** (Symbol/ZapfDingbats now render via bundled OFL Noto fonts).
- **P1-2 · `/Decode [1 0]` ImageMask** — DONE. `draw_image_mask` reads `/Decode` (or inline `/D`) and inverts which sample paints; an inverted stencil no longer fills solid. Regression test added.
- **P1-3 · CI accuracy/SSIM regression gate** — DONE. Three tiny clean-room **CC0-1.0** born-digital fixtures (`fixtures/born/`, reproducible via `conformance/gt/make_ci_fixtures.py`, manifest-lint-cleared); `run_gt.py` gained a **no-oracle** reading-order gate vs inlined `gt_text` (`ci_manifest.json`) and `render_diff.py` a **committed-reference SSIM** gate (`conformance/gt/ssim-refs/`, captured post-fix). New `ci.yml` `accuracy-gate` job fails on regression. Thresholds carry margin (order 0.90, SSIM 0.97); both fail-paths verified. **Note:** with `.venv-oracle` absent, the SSIM gate is self-referential against committed buffers (still catches any renderer change — the requested no-oracle design).

**Residual carried forward:**

- **P1-1r · Symbol/ZapfDingbats fallback — ✅ DONE (2026-06-21)** — *was S · Low* — bundled 3 **OFL (SIL OFL 1.1)** Noto faces under `crates/pdf-fonts/fonts/symbols/` (~1.4 MB: NotoSansMath for Symbol's Greek/math, NotoSansSymbols2 for the ZapfDingbats block, NotoSansSymbols for the 5 crosses). `render.rs::std14_substitute` returns them for non-embedded Symbol/ZapfDingbats, reusing the existing Symbol/ZapfDingbats encoding→Unicode tables + `std_widths` for advances. Coverage **ZapfDingbats 94/94; Symbol 95/97**. Chose OFL over the AGPL URW drop-in to keep the crate Apache-clean — the tradeoff is the glyph SHAPES differ from Adobe's, so SSIM vs fitz is **<1.0 on these (rare) pages** (Symbol 0.61, ZapfDingbats 0.16) — correct semantic glyphs, not blank. **Residual:** `Euro` (U+20AC) + `radicalex` (U+F8E5 PUA) render `.notdef` (2 rare codes, documented in the font PROVENANCE).

### Phase 2 — COMPLETE (2026-06-20) — committed on `main`

All four parity-push clusters landed (+29 symbols, coverage 84.7%→88.4%, deferred 52→23) and were
oracle-cross-checked against real PyMuPDF 1.24.14 (`.venv-oracle`) with zero regressions. Done summary:

- **P2-1 · Page draw-convenience + loader/alias cluster (12 symbols, pure-Python)** — DONE. `draw_curve`/`draw_quad`/`draw_sector`/`draw_squiggle`/`draw_zigzag` (page-level draw convenience over `Shape`), `load_links`/`update_link`, `load_annot`/`load_widget`, `delete_widget`, `cluster_drawings`, `is_wrapped` — all flipped + regen + oracle-checked + tested.
- **P2-2 · `Document.convert_to_pdf` (image inputs)** — DONE (1 symbol). The finished Rust impl (C7) is now exposed; `Document.open` transparently handles image files, raising `PdfUnsupportedError` only for non-image input (fitz-correct). Oracle-checked.
- **P2-3 · Small binding clusters (9 symbols)** — DONE. `Pixmap.samples_ptr`/`Pixmap.__array_interface__` (numpy zero-copy), `Tools.image_profile` + module-level `image_profile`, `Page.language`/`set_language`, `Page.set_contents`, `Document.get_outline_xrefs`, `Document.embfile_upd` — all flipped + regen + oracle-checked + tested.
- **P2-4 · Medium parity items (7 symbols)** — DONE. TOC edits (`Document.set_toc_item`/`del_toc_item`), `Document.version_count`, `Document.extract_font`, `Document.subset`, `Page.add_widget`, `Page.add_caret_annot` — semantics validated vs `.venv-oracle` (fitz's in-place rewrite, not full rebuild).

**Residuals carried forward:**

- **P2r-1 · `set_toc`/`get_toc` page-mapping off-by-one** — *S · correctness* — **✅ FIXED (2026-06-21).**
  Fixed at the py-bindings boundary: PyMuPDF TOC pages are 1-based while the core `TocEntry.page` is 0-based;
  `get_toc` now `+1` / `set_toc` now `-1` with fitz clamping. Oracle-verified (`test_pytoc_004`).
- **P2r-2 · `image_profile` dict-key divergence — RESOLVED, NOT-A-BUG (2026-06-21)** — *S · Low* — **closed.**
  The earlier "divergence" premise was wrong: pdfspine's `image_profile` **already** matches the authoritative
  PyMuPDF `JM_image_profile` dict shape (`colorspace` as an int + a `cs-name`; no `type`/`size`/`colorspace.n`).
  No change needed.

**Note:** three P2 symbols (`image_profile`, `Pixmap.__array_interface__`, `Page.set_language`) could **not** be live-diffed because PyMuPDF 1.24.14's own runtime is broken for them (SWIG marshalling bugs); pdfspine implements the **documented PyMuPDF contract** for these.

### Phase 3 — Post-launch correctness

- **P3-1 · Multi-column reading-order engine — ✅ EFFECTIVELY DONE (verified 2026-06-20)** — *was L · High*
  - The recursive XY-cut + occupancy-valley gutter engine **already landed** (commits `9ff0e6a`/`e56bcb9`/`633f0f6`/`06d24c8`, 06-16 PM). Fresh GT (P3-2): PMC order **0.965/0.995** vs fitz 0.975/0.997, born-digital **0.996** vs 1.000 — within 0.000–0.009 per column doc (PMC212687 0.083→0.996, born 2col 0.549→0.997, 3col 0.409→0.996). No single-column regression; the P1-3 gate now guards it.
  - **Residual → P3-1r — ACCEPTED / WON'T-FIX (2026-06-21)** (*ordering-only, inherent tradeoff*): PMC212689
    scores order 0.645 vs fitz 0.749 — content at full parity (f1 0.940 / jaccard 0.868), only reading-order
    placement differs on this one real-world 2-col doc. A decisive experiment showed any ordering change that
    helps PMC212689 (+0.10, beats fitz) regresses PMC212688 (−0.19) and PMC176547 (−0.02); fitz itself is
    content-order-based (scores only 0.749 here too). Reverted — `layout.rs` == HEAD. Accepted as an inherent
    content-vs-geometric-order tradeoff, not an open todo.

- **P3-2 · PMC near-zero collapse — ✅ DONE (2026-06-20): diagnosed as a STALE REPORT, not a bug** — *M · High*
  - Root cause: `GT-REPORT-pmc.md`/`GT-REPORT-born.md` were generated 06-16 **morning**, before the column engine landed that afternoon. Independently verified the current build is at fitz parity (PMC212687 pdfspine 69409 vs fitz 69385 chars, direct word-jaccard 0.987; born multi-column jaccard 1.0). **No content-dropping bug exists.** Both reports regenerated against the current build + oracle; `run_gt.py` stale-path resolution + score-arg-swap fixed (closing **P0-6r**).

- **P3-3 · Indexed/Separation/DeviceN colorspaces + `/Decode` — ✅ DONE (2026-06-20, `9b01deb`)** — *was L · Medium*
  - New `crates/pdf-core/src/colorspace.rs` — one coherent `ColorSpace` resolver + the shared `PdfFunction` evaluator (types 0/2/3, moved from pdf-render, generalized multi-input for DeviceN). Indexed images now look up the palette; Separation/DeviceN run the tint transform; `/Decode` is applied generally (DCT/JPX excluded to avoid double-apply); and the vector `cs`/`scn` path (`interp.rs` + `state.rs`, q/Q-saved) runs the transform so a **dark 1-component Separation fill no longer renders white**. Pixel-exact vs the fitz oracle on synthetic Indexed/Separation/DeviceN//Decode cases; 4 pdf-core unit + 6 render-integration pixel tests; P1-3 SSIM gate green (no reference drift).
  - **Residual → P3-3r — ✅ FIXED (2026-06-21)** (*was S · Low*): added a **SWOP-like K-axis black point**
    across the 4 render paths, so pure-K (`0 0 0 1 k`) now renders **(34,31,31)** exact vs fitz (was 0,0,0).
    Saturated-CMY primaries still differ (an inherent ICC limitation, documented in code); ICC-accurate spaces
    stay out-of-scope (ICCBased falls back by `/N`); DeviceN type-0 multi-axis tables use nearest-sample per
    non-primary axis. Minor documented inconsistency: the `pdf-edit/src/annot.rs` annotation-appearance
    authoring path was left on the naive transform (out of render scope).

- **P3-4 · Cheap correctness insurance — ✅ DONE (2026-06-20)** — *was S–M · Low–Medium*
  - **Kangxi fold:** `crates/pdf-fonts/src/cmap.rs::invert_to_cid_unicode` now NFKC-folds Kangxi Radicals (U+2F00–U+2FDF) to the canonical ideograph on the predefined-CMap / no-`/ToUnicode` path. Oracle-checked: fitz folds Kangxi (214/214) but **NOT** the CJK Radicals Supplement (U+2E80–U+2EFF) — so pdfspine folds **only Kangxi** to match fitz, and keeps U+2F00 verbatim on the explicit-`/ToUnicode` path. +3 Rust tests.
  - **Edge-case tests** (`python/tests/test_p3_4_edge_cases.py`, 6, oracle-checked): ToUnicode-less Type0 ✓, overlapping/co-located text ✓ (same char multiset as fitz), single-column vertical CJK ✓. **Residual → P3-4r — ✅ FIXED (2026-06-21):** vertical writing-mode (wmode 1) now reads `/W2`+`/DW2` metrics and applies a −y advance, so multi-column vertical CJK reads columns right-to-left like fitz (oracle-verified); the P3-4 tripwire test now asserts the correct order.
  - **Robustness:** new `conformance/gt/run_robustness.py` + `ROBUSTNESS-REPORT.md`; **0 panics** over N=43 GovDocs1 (target 250 — network-bound shortfall behind the local proxy; re-run on an unthrottled link to grow N).

- **P3-5 · FinTabNet gold-table GT — ✅ DONE (scored 2026-07-02)** — *M · Medium*
  - Harness (built 2026-06-20): `conformance/gt/grits.py` (pure-stdlib **GriTS** Top+Con, AGPL-free port, 7-case self-test passes), `fetch_fintabnet.py` (FinTabNet.c, CDLA-Permissive), and a `tables_diff.py --gold` mode (parse gold → run pdfspine `find_tables` in the isolated worker → match by IoU → GriTS). Default fitz-agreement mode unchanged.
  - **Unblocked + scored (2026-07-02):** the original `dax-cdn.cdn.appdomain.cloud` host is **permanently decommissioned** (whole DNS zone SERVFAIL); `fetch_fintabnet.py` now extracts source PDFs from the verbatim HF mirror (`Leon1207/FinTabNet` `archive.zip`, license unchanged CDLA-Permissive-1.0) via zip64 HTTP-Range member extraction (central-directory index cached; `--self-test` green 3/3; provenance recorded in manifest `pdf_source`/`pdf_source_original`). Corpus: **150 pages / 186 structure-eligible gold tables**.
  - **Scores** (recall-weighted over all 186 gold tables; `tables_diff.py --gold` gained a `--strategy` flag, default `lines` unchanged): default `lines` GriTS_Top **0.073** / Con **0.070** (39/150 pages any detection) — **parity with fitz**, whose default also detects ~0 on these borderless financial tables; `strategy="text"` GriTS_Top **0.185** / Con **0.107** (148/150 pages, 148/148 predictions match gold IoU>0.5) — the engine's real detection capability, with a documented structure-quality tradeoff (page-grid over-merge/over-split; matched-only means drop vs lines' few-but-clean matches). Both far below TATR ~0.98 — this is a *baseline tracking metric* for `find_tables`, not a claim. Reports: `GT-REPORT-tables-gold.md` (+ strategy-comparison section) and `GT-REPORT-tables-gold-text.md`.

### Phase 4 — Post-launch capability / strategic

- **P4-1 · Font handle carries `/FontFile*` program bytes — ✅ DONE (2026-06-21)** — *was L · Medium (API)* · NOT the rendering keystone (C3)
  - New `Font::from_program` (via `ttf-parser`, sharing the renderer's infra): a program-backed `Font` (from `/FontFile*` or user `fontfile=`/`fontbuffer=`) now serves `buffer()` (program bytes), `glyph_bbox(chr)` (real per-glyph outline box), and a real-cmap `valid_codepoints()`. `Font(fontfile=)`/`Font(fontbuffer=)` load the real program (ValueError/OSError on bad input — **no silent Helvetica fallback**). Oracle-cross-checked on Liberation Sans (name / buffer-len / glyph_count exact). **+2 parity** (`Font.buffer`, `Font.glyph_bbox`) → 682/769, **88.7%**; Font class 22/23.
  - Note: pdfspine's `glyph_bbox` returns the **real per-glyph** box; PyMuPDF returns the font-level FontBBox for every glyph (pdfspine strictly more correct). A metrics-only Core-14 handle still raises `PdfUnsupportedError` for `buffer`/`glyph_bbox` (no license-clean bundled substitute, per repo policy).

- **P4-2 · Type1 charstring (PFB/PFA) support — ✅ DONE (2026-06-21)** — *was L · Medium*
  - New first-party, dependency-free `crates/pdf-render/src/type1.rs` (~912 lines): eexec-decrypt (R=55665) + per-charstring decrypt + a Type1 charstring interpreter (hsbw/sbw, the moveto/lineto/curveto family, closepath, callsubr, `seac` accent composition, `flex` via OtherSubrs) feeding the same `PathSink` as the CFF/TrueType paths. `resolve_font_program` now includes `/FontFile`, so an **embedded Type1 font renders real glyphs** instead of blank. Verified **pixel-exact vs the fitz oracle** on a synthetic Type1 `/FontFile` PDF (byte diff 0/120000, SSIM 1.0; was blank pre-P4-2); 2 unit + 1 render-integration test. cargo/clippy/pytest(711)/P1-3 gate all green. Apache-2.0 / pure-Rust (no FreeType).
  - **Residual → P4-2r — ✅ FIXED (2026-06-21)** (*was S · Low*): the outliner now parses the cleartext builtin `/Encoding` (`dup <code> /<name> put`), and `resolve_gid` gains a code→name→GID fallback for non-AGL builtin encodings used without a PDF `/Encoding`. Multiple-Master / hint-replacement (OtherSubr 3 / OtherSubrs ≥14) remain documented safe no-ops (MM renders the default master); all degrade safely, never panic.

- **P4-3 · OCR `recognize()` rayon parallelism — ✅ DONE (2026-06-21)** — *was M · High (OCR latency)*
  - `PaddleOcr::recognize`'s per-box loop is now a rayon `par_iter` with an **indexed collect** → output byte-identical to the sequential version (deterministic, proven vs a captured baseline + a 1-thread-vs-N fingerprint). **3.49× speedup** on a 42-box page (16 cores: 2858ms → 819ms). The `&self`+`Mutex`/`OnceLock` model cache was already thread-safe (no `model.rs` change). `rayon` is a feature-gated (`paddle-ocr`) optional dep — **not** in the lean base wheel. No correctness change, no new symbols.

- **P4-4 · API reference docs cover the full public surface — ✅ DONE (2026-06-21)** — *was M · Medium*
  - Wired **mkdocstrings** (python/griffe, sphinx docstring style) into `mkdocs.yml` + a `docs` extra in `pyproject.toml`; rewrote the drift-prone hand-written `docs/reference/` into 14 mkdocstrings-rendered pages auto-generated from the (already rich) docstrings, plus `scripts/gen_docs_constants.py` (constants value tables) + `scripts/check_docs_coverage.py` (acceptance gate). **307/307 public symbols** documented (= `pdfspine.__all__`); **`mkdocs build --strict` exit 0, no warnings**. `/site/` gitignored.

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
- **Encryption-semantics COMPAT gaps (found during the 2026-07-02 `test_encrypted` adjudication;
  oracle-verified on both 1.24.14 and 1.27.2.3):** `62997e2` auto-auth itself is fitz-correct
  (`needs_pass` falsy on empty-user-pw docs — tests were fixed to match), but 5 systematic deviations
  remain: `is_encrypted` means "has /Encrypt" in pdfspine vs "encrypted AND not yet authenticated" in fitz;
  pre-auth `permissions` (-44 vs 0); pre-auth `metadata` (decrypted-garbage dict vs `None`); post-auth
  `needs_pass` (flips False vs stays truthy in fitz); `metadata["encryption"]` missing the ` RC4` suffix.
  Fixing `is_encrypted` naively breaks `test_pyenc_001_aes256_roundtrip` / `test_edit.py:284` /
  `test_longtail11.py:308` — needs its own adjudication round, not a drive-by.

## 6. Task index

| ID | Title | Effort | Impact | Status | Phase |
|----|-------|:--:|:--:|:--:|:--:|
| P0-1 | Version-from-tag | S | High | ✅ done | 0 |
| P0-2 | Deferred → PdfUnsupportedError contract | M | High | ✅ done | 0 |
| P0-3 | Flip 4 mislabeled symbols + regen COMPAT (→84.7%) | S | Low | ✅ done | 0 |
| P0-4 | Implement/delete 3 fake CI guards | M | Med | ✅ done | 0 |
| P0-5 | OCR wheel-bloat decision (opt-in extra) | M | Med | ✅ done | 0 |
| P0-6 | CHANGELOG + fix stale `/workspace/pypdf` paths | S | Low | ✅ done | 0 |
| P0-2r | `_core` PyO3 deferred members → PdfUnsupportedError (Rust `__getattr__`, 5 members) | M | Med | ✅ done | 0r |
| P0-5r | OCR publishing — model-data companion + OCR-in-wheel (Option A) | M | High | ✅ done | 0r |
| P0-6r | run_gt.py resolves stale-absolute corpus paths (rename-proof) | S | Low | ✅ done | 0r |
| P1-1 | Liberation std-14 fallback fonts (blank body text) | M | High | ✅ done | 1 |
| P1-2 | Honor `/Decode [1 0]` ImageMask | S | Med | ✅ done | 1 |
| P1-3 | Real extraction/render CI accuracy gate | M | High | ✅ done | 1 |
| P1-1r | Symbol/ZapfDingbats fallback (OFL Noto, approx shapes) | S | Low | ✅ done | 1r |
| P2-1 | Page draw-convenience + loader/alias (12 syms) | M | High | ✅ done | 2 |
| P2-2 | `Document.convert_to_pdf` (image inputs) | M | Med | ✅ done | 2 |
| P2-3 | Small binding clusters (Pixmap/Tools/Page/Doc) | S–M | Low–Med | ✅ done | 2 |
| P2-4 | Medium parity (TOC edit, extract_font, subset…) | M | Low | ✅ done | 2 |
| P2r-1 | `set_toc`/`get_toc` page-mapping off-by-one (1-based vs 0-based) | S | Low | ✅ done | 2r |
| P2r-2 | `image_profile` dict-key — NOT-A-BUG (already matches JM_image_profile) | S | Low | ✅ resolved | 2r |
| P3-1 | Multi-column reading-order engine (landed 06-16; verified) | L | High | ✅ done | 3 |
| P3-2 | PMC collapse — diagnosed (stale report, no bug) + reports regen | M | High | ✅ done | 3 |
| P3-1r | PMC212689 ordering-only residual (order 0.645 vs 0.749) | S | Low | accepted · won't-fix | 3r |
| P3-3 | Indexed/Separation/DeviceN + `/Decode` (render) | L | Med | ✅ done | 3 |
| P3-3r | CMYK→RGB K-axis black point (pure-K 34,31,31 vs fitz) | S | Low | ✅ done | 3r |
| P3-4 | Kangxi fold + edge-case tests + robustness rerun | S–M | Low–Med | ✅ done | 3 |
| P3-4r | vertical writing-mode (wmode 1, `/W2`+`/DW2`, −y advance) | M | Low | ✅ done | 3r |
| P3-5 | FinTabNet GriTS absolute score (HF-mirror fetch; lines 0.073 / text 0.185 Top) | M | Med | ✅ done | 3 |
| P4-1 | Font carries `/FontFile*` (buffer/glyph_bbox, +2) | L | Med | ✅ done | 4 |
| P4-2 | Type1 charstring (PFB/PFA) support | L | Med | ✅ done | 4 |
| P4-2r | Type1 builtin `/Encoding` parse (hint-replace / MM stay safe no-ops) | S | Low | ✅ done | 4r |
| P4-3 | OCR `recognize()` rayon parallelism (3.49×) | M | High | ✅ done | 4 |
| P4-4 | Full public-surface API docs (mkdocstrings, 307/307) | M | Med | ✅ done | 4 |

**Recommended next (in order):** *(**Phases 0–4 + the residual sweep + P1-1r ALL LANDED 2026-06-21** — on `main`;
parity **88.7%**; every actionable residual is now fixed or flagged. P3-5 score blocked on sandbox data egress.)*
1. **Pre-public chores** (§7) — render-SSIM re-measured (now **0.984 / 0.989**, at parity); P3-5 GriTS
   **scored 2026-07-02** (lines 0.073 = fitz-parity-zero; text 0.185, the real capability number); then
   **flip the repo public + push** (feature-complete at 88.7% — multi-column, colorspaces, OCR parallelism,
   embedded fonts incl. Type1, vertical CJK, Symbol/Dingbats all landed; render at fitz parity).
2. **Accepted / further work** — P3-1r is an accepted won't-fix (inherent content-vs-geometric-order tradeoff,
   `layout.rs` == HEAD); the 21 remaining deferred are the genuinely-blocked long tail (OCG/layers,
   device-replay, a few Type0/Type3 edges).

## 7. Pre-public chores + docs upkeep (do alongside / last)

- Reword any historical commit messages that contain backticks (shell substitutes them).
- **Keep `PARITY.md` + `docs/BENCHMARKS.md` current** — they drift as batches land; refresh after each.
- **Aggregate render-SSIM re-measure** — ✅ DONE (2026-06-21): re-ran `render_diff.py` over the 46-doc render
  corpus vs `.venv-oracle`; **0.945 → 0.984 mean / 0.989 median** (at/near parity); `RENDER-REPORT.md` +
  `docs/BENCHMARKS.md` §5 refreshed. The committed P1-3 `ssim-refs/` were untouched.
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

## 9. Markdown → PDF — original extension — ✅ COMPLETE (2026-07-01 → 2026-07-02)

**What & positioning.** A brand-new top-level API `pdfspine.markdown_to_pdf(md_or_path, ...) -> Document`
that renders Markdown to PDF. This is a **pdfspine original extension** (a self-authored, deterministic
block-level layout engine that reuses the existing drawing primitives). It is **NOT** PyMuPDF's
`Story` / `Xml` / `insert_htmlbox` HTML-CSS engine — that stays out of scope (root `PRD.md` §3.2 #2). It
implements no CSS/HTML DOM, only deterministic Markdown layout. Direction is md→pdf (authoring), orthogonal
to the existing pdf→md extraction (`Table.to_markdown`) and image→pdf (`convert_to_pdf`).

**Locked decisions (2026-07-01, via user Q&A):**
- **Route:** pure-Rust in-house renderer — `pulldown-cmark` parses, an in-house layout engine reuses the
  drawing primitives. No external / Python typesetting library, no HTML-CSS engine.
- **Scope:** CommonMark + GFM tables + strikethrough + task lists + image embedding.
- **API:** one top-level function `markdown_to_pdf()`. Original extension → **not** in the fitz-compat
  surface / `COMPAT.toml` (no `PdfUnsupportedError` contract, no parity counting).
- **Defaults (all options-configurable):** A4 595×842pt + 72pt margins; body Helvetica / heading
  Helvetica-Bold / code Courier (Base-14, zero embedding); images from local path + `data:` URI only
  (**no remote-URL fetch** — keeps the no-network property); links rendered as blue text, no link
  annotation in v1.

**✅ CJK DECISION — RESOLVED (2026-07-02, user chose Option A).** Don't bundle a font; expose `font=` /
`cjk_font=` to pass a TTF path (PingFang / Noto Sans CJK) for Chinese; unset → CJK renders as missing
glyphs; wheel stays small. No system-font auto-detection, no bundled CJK font. When a TTF is given, embed
via the existing `pdf_edit::fontfile::EmbeddedFont` (Type0/Identity-H, usage-subset `write_type0`), measure
via `EmbeddedFont::char_advance`; `cjk_font` acts as a per-char fallback for codepoints the active Base-14
(or `font=`) face can't encode.

**Reuse map (落点地图 — don't re-investigate):**
- New crate `crates/pdf-markdown` (deps: pdf-core + pdf-edit + pulldown-cmark; add pdf-fonts / pdf-image in
  MD-1). Facade in `pdf-api`, binding in `py-bindings`.
- **Measure / line-break:** `pdf_fonts::std_widths::string_advance(std_name, text, fontsize)`
  (`crates/pdf-fonts/src/std_widths.rs:151`).
- **User-TTF embed + measure (Option A):** `pdf_edit::fontfile::EmbeddedFont` (`crates/pdf-edit/src/fontfile.rs`)
  — `parse(program)` / `glyph_id(ch)` / `char_advance(ch)` / `write_type0(doc, used)` (Type0/Identity-H,
  Flate FontFile2, usage-subset ToUnicode). **Parse ONCE per document and accumulate `used` across all runs,
  write_type0 ONCE at the end** — calling `insert_text(fontfile=)` per run would re-embed the program per call.
- **Base-14 resolve / resource:** `pdf_edit` `resolve_base14` (`crates/pdf-edit/src/text.rs:81`),
  `base14_font_object` (`text.rs:108`).
- **Draw:** `pdf_edit::insert_text` (baseline-point, `text.rs:152`); `Shape` / `draw_rect`
  (`crates/pdf-edit/src/drawing.rs:118` / one-shot `:368`) for code-block backgrounds, table borders,
  blockquote bars; `insert_image_jpeg` / `insert_image_rgb` (`crates/pdf-edit/src/image.rs:33` / `:81`;
  PNG → decode to RGB via `pdf-image` first).
- **Paginate / new page:** `PageEditor::new_page(index, w, h)` (`crates/pdf-edit/src/page_ops.rs:77`);
  build-from-scratch template `pdf-image/src/imagedoc.rs` `empty_seed_pdf()` / `build_pdf()`;
  emit bytes via `DocumentStore::save_to_vec(&SaveOptions::default().with_xref_style(XrefStyle::Table))`.
- **⚠ TRAP:** `insert_textbox` (`text.rs:238`) **drops lines past the bottom edge and does not return the
  overflow text** → unusable for cross-page flow. The layout engine MUST do its own per-line measure +
  wrap + `new_page` before overflow.
- **Python binding path (copy `image_to_pdf`):** Rust `#[pyfunction]` in `crates/py-bindings/src/lib.rs`
  (template `image_to_pdf` ~:3992, register in the `_core` `#[pymodule]` ~:5248, return `PyBytes`);
  Python wrapper in `python/pdfspine/document.py` (mirror `_open_image_bytes` ~:4582 —
  `Document(_core.open_bytes(bytes))`); export in `python/pdfspine/__init__.py` `__all__`; stubs in
  `_core.pyi` + `document.pyi`.

**Phased plan:**
- **MD-0 · Scaffold — ✅ DONE (2026-07-01).** New `crates/pdf-markdown/{Cargo.toml, src/lib.rs,
  tests/smoke.rs}`; root `Cargo.toml` members + `[workspace.dependencies]` (pulldown-cmark 0.12,
  `default-features = false`, with a license note); `supply-chain/config.toml` `[policy.pdf-markdown]`.
  Placeholder `markdown_to_pdf(&str) -> pdf_core::error::Result<Vec<u8>>` parses with
  `ENABLE_TABLES|ENABLE_STRIKETHROUGH|ENABLE_TASKLISTS`, emits a blank A4 page. build / test / fmt /
  `clippy -D warnings` all green.
- **MD-1 · Layout engine — ✅ DONE (2026-07-02).** Build-from-scratch pipeline (NOT PageEditor/`insert_text`
  — those re-embed the font per call): pulldown-cmark events → block model (`model.rs`) → image resolve
  (`images.rs`) → measure/wrap/paginate (`layout.rs`, top-left coords) → content-stream + doc assembly
  (`render.rs`, y-flip; one content stream per page, one shared object per font, deterministic object
  order). Covers H1–H6, paragraphs, bold/italic/bold-italic/inline-code/strikethrough(drawn)/links(blue),
  ordered(+start)/unordered/nested/task lists (vector bullets/checkboxes), nested blockquotes (bar splits
  across pages), code blocks (0.9× Courier, grey bg, char-level hard-break), HR, GFM tables (measured
  col-widths + fair-share shrink, header bold+grey, per-row pagination), images (local + data: URI, JPEG
  passthrough incl. CMYK `/Decode`, PNG/BMP/GIF/WEBP/TIFF via pdf-image, alpha→white composite).
  `Options{page_w/h, margins×4, body_font_size, font, cjk_font, base_dir}` (#[non_exhaustive]). Option A:
  per-char fallback base14→cjk_font→'?', font=→cjk_font→.notdef; embed parse ONCE/doc, `write_type0` ONCE
  (test asserts exactly 1 `/FontFile2`). **48 integration tests** (9 files) round-trip via pdf-api; gates
  green: fmt ✓ clippy -D warnings ✓ `cargo test --workspace` **1442 passed / 0 failed** (new floor).
  Known v1 limits: single-face `font=` (no bold/italic variants), images inside headings/table cells
  dropped, table rows don't split across pages (no header repeat), HTML blocks ignored, WinAnsi
  0x80–0x9F degrade to '?' without cjk_font.
- **MD-2 · Facade — ✅ DONE (2026-07-02).** `crates/pdf-api/src/markdown.rs` re-exports
  `markdown_to_pdf(&str, &Options)` + `MarkdownOptions`; root re-export in `pdf-api/src/lib.rs`; bad-input
  `InvalidArgument` explicitly mapped → `Unsupported` → `PdfUnsupportedError` (the image-path error policy).
  3 facade tests in `crates/pdf-api/tests/markdown_facade.rs`.
- **MD-3 · Python binding — ✅ DONE (2026-07-02).** `#[pyfunction] markdown_to_pdf` (GIL released) →
  `PyBytes`; `document.py` wrapper `markdown_to_pdf(md_or_path, *, font=, cjk_font=, base_dir=,
  page_width=, page_height=, margins=, body_font_size=) -> Document`. `md_or_path` heuristic: treated as a
  FILE iff suffix ∈ {"", .md, .markdown, .txt} AND `Path.is_file()` (then base_dir defaults to its parent);
  anything else = literal Markdown text. `font`/`cjk_font` accept path or bytes; `margins` float or 4-tuple.
  Exported in `__all__` + stubs (`__init__.pyi`/`_core.pyi`/`document.pyi`); **NOT** in the fitz shim /
  COMPAT surface (original extension; compat-symbol-guard is one-directional — no exception needed, verified
  zero COMPAT diff). 10 pytest cases (`MARKDOWN-TO-PDF-001..010`) registered in `docs/test-case-catalog.md`.
  Gates: workspace **1445** green, pytest **749 passed** (≫721 floor; the only 2 fails are a pre-existing
  `test_encrypted.py` conflict with commit `62997e2` auto-auth, unrelated — adjudicated separately).
- **MD-4 · Docs & wrap-up — ✅ DONE (2026-07-02).** README capability row + Quick-start example;
  `CHANGELOG.md` Unreleased/Added; root `PRD.md` §3.2 #2 original-extension note (Story/HTML-CSS stays
  out-of-scope); `_llms` (api/overview/recipes incl. a Chinese `cjk_font` recipe/gotchas — CJK silent-`?`
  degradation listed FIRST; examples live-verified, macOS Hiragino/Songti **TTC collections work**,
  PingFang.ttc absent on this machine); `docs/reference/functions.md` mkdocstrings entry;
  `THIRD-PARTY-NOTICES.md` hand-extended (NO generator exists in-repo despite its header claim — ad-hoc
  since `59f66a0`): +pulldown-cmark 0.12.2 (MIT) + transitive unicase 2.9.0, 184→186 components. Gates:
  `check_docs_coverage.py` 310/310, `mkdocs build --strict` exit 0 / 0 warnings, manifest-lint 0. Also
  fixed two pre-existing strict-gate breakages surfaced by the door: `ImageTable`/`ImageTableCell` were in
  `__all__` but missing from `docs/reference/tables.md`, and `docs/BENCHMARKS.md:15` linked out-of-site
  `../PARITY.md` (→ GitHub blob URL per license.md precedent).

**Resume pointer.** MD-0..MD-4 are **all complete (2026-07-02)** and verified via the full §8 suite; the
feature lives in the working tree pending commit. Nothing left in §9 — future markdown work (link
annotations, bold/italic user-font variants, row-splitting tables, HTML passthrough) would be a new PRD
section, not a resumption of this one.

## 10. Shared typesetting engine `pdf-typeset` — Phase A of docspine/pptspine faithful PDF export

**What & positioning.** The **shared-engine project (Phase A)** enabling faithful PDF **export** of
`.docx` (docspine) and `.pptx` (pptspine). pdfspine grows one new workspace crate,
**`crates/pdf-typeset`** (next to `pdf-markdown` in the members list, `Cargo.toml:3-15`); the consumers
add `doc-render` / `ppt-render` crates **in their own repos** and pull pdfspine crates via **git
dependency + pinned rev**, copying the ocrspine precedent (docspine `Cargo.toml:22-29`). `pdf-typeset`
re-exports the pdf-edit surface consumers need, so doc-render/ppt-render declare exactly **ONE** pdfspine
git dep. Scope: **(a)** input model — styled runs (per-run family/size/bold/italic/underline/strike/
color/highlight), paragraph props (alignment incl. justify, line spacing multiple+exact, space
before/after, first-line & hanging indent, left/right indent, list bullet/numbering labels), blocks, and
an absolutely-positioned TextBox spec (fixed rect, vertical anchor top/middle/bottom, word-wrap on/off,
normAutofit fontScale, rotation); **(b)** font resolution & management; **(c)** flow layout with
pagination callbacks (generalized from pdf-markdown) + box layout; **(d)** a preset-geometry subset for
pptx autoshapes; **(e)** table layout primitives (grid measure, cell block layout, per-edge border
painting); **(f)** emission via pdf-edit ops. **Non-goals:** the content-level
`to_markdown → markdown_to_pdf` path explicitly does **NOT** count as export fidelity; Phase A does
**NOT** rewire pdf-markdown onto pdf-typeset (`markdown_to_pdf` stays green, untouched — generalization
is **copy-adapt**; consolidation is a noted future option); no shaping/kerning/ligatures; gradients/
shadows/3D out (below); not part of the fitz-compat/COMPAT surface (crate-level Rust API — the Python
surface lives in the consumer repos, which surface warnings via `warnings.warn`).

**Locked decisions (2026-07-02, family-wide design brief — do not reopen):**
- **Fonts — `fontdb` 0.23 + first-party thin resolver.** fontdb is MIT (passes the `deny.toml:1-27`
  allowlist), 100% pure Rust (directory scan + pure-Rust fontconfig-XML parsing — no CoreText/
  DirectWrite/libfontconfig), pins the **same ttf-parser 0.25** already locked (`Cargo.lock:2101-2104` —
  no duplicate version), enumerates **every TTC face** (`FaceInfo.index`; `with_face_data` hands back the
  exact `(bytes, face_index)` pair the new `parse_indexed` needs), and matches **localized CJK family
  names** (verified in source: all name-table language variants participate in `query`). It is
  dormant-but-stable (0.23.0, 2024-10; still resvg/usvg's pinned backend) — pin the exact version,
  vendoring is the exit strategy; hand-extend `THIRD-PARTY-NOTICES.md` (no generator exists,
  `docs/PRD-NEXT.md:412-413`). fontdb's own query is exact/case-sensitive, so the first-party resolver
  adds: a normalized (case/width-folded) name index, bold/italic → `Query{weight, style}` mapping, a
  **configurable substitution table** with built-in three-platform defaults (宋体/SimSun → Songti SC
  (macOS) / SimSun (Windows) / Noto Serif CJK SC (Linux); 微软雅黑/Microsoft YaHei → PingFang SC /
  Microsoft YaHei / Noto Sans CJK SC; Calibri → Carlito-if-present; Times New Roman → bundled Liberation
  Serif), a per-character fallback chain, and a final fallback to the bundled Liberation/Noto faces
  (`crates/pdf-fonts/src/liberation.rs:36-52`).
- **EmbeddedFont upgrades:** `Face::parse` with a **real TTC face index** (today hardcoded 0 at
  `crates/pdf-edit/src/fontfile.rs:62` and `:108`); a **multi-face family registry**
  (regular/bold/italic/bold-italic = 4 distinct embedded fonts, one embed per doc per face); and a
  **usage-based glyph SUBSETTER — strategic requirement** (system CJK fonts are 10–90 MB; whole-font
  embed kept only as a debug flag).
- **No synthetic bold in v1** — consistent with the render side having no embolden path (correction C9,
  §3): a missing face → substitution/similarity fallback + `ExportWarning`, never fake emboldening.
- **Preset geometry:** v1 subset ≈ **35 presets** (rect, roundRect, ellipse, line, straightConnector1,
  bentConnector2/3, triangle, rtTriangle, diamond, parallelogram, trapezoid, pentagon, hexagon, octagon,
  plus, arc, pie, chord, donut, right/left/up/down/leftRight arrows, star4/5/6, chevron, homePlate,
  wedgeRectCallout, flowChartProcess/Decision/Terminator/Data); every other `prstGeom` value degrades to
  its **bounding-box rect + `ExportWarning`**, with the shape's text still laid out on top (text layout
  is geometry-independent).
- **Fills:** solid fill IN; **constant alpha IN** (cheap `ca`/`CA` ExtGState via the existing
  `add_resource` plumbing, `crates/pdf-edit/src/content.rs:141-172`); **gradients/shadows/3D OUT** for
  v1 (gradient → representative solid color + warning; shadows dropped + warning). Required drawing
  upgrades: alpha ExtGState, `W n` clipping, line join/cap, arbitrary arc→Bézier, roundRect, shape-level
  transforms (`q cm … Q`), text rotation.
- **Engine output:** `ExportResult` carrying pdf bytes/ops + `Vec<ExportWarning>` — every
  unsupported-feature degradation enumerated; consumers surface them in Python via `warnings.warn`.
- **Determinism weakens to per-font-environment:** same machine + same installed fonts ⇒ identical
  bytes; cross-machine output may differ (system-font resolution) — unlike pdf-markdown's absolute
  contract (`crates/pdf-markdown/src/lib.rs:106-110`).
- **Fixture policy:** docspine/pptspine keep their no-binary-fixture charter (synthesize OOXML zips in
  conftest); pdf-typeset unit fixtures follow the `fixtures/born` deterministic-generator pattern (P1-3).

**Acceptance-gate stack (family-wide — every phase gate below cites these anchors):**
1. **CI-blocking content read-back:** source-side `to_text()` vs pdfspine `Page.get_text()`
   (`python/pdfspine/document.py:1813-1843`, words-with-coords), scored with `conformance/gt/score.py`
   (`content_scores:152`, `order_score:198`) — **token-F1 ≥ 0.99 AND order ≥ 0.99** on synthetic fixtures.
2. **CI-blocking structural/geometry asserts:** page count/size, `get_text_words` coordinate tolerance,
   `extractIMGINFO` image survival, non-blank raster via `conformance/gt/render_diff.py`
   (`_near_blank:463-469`).
3. **Local-only advisory LibreOffice oracle:** `/Applications/LibreOffice.app/Contents/MacOS/soffice`
   (binary verified present, 25.2.1.2), `--headless --convert-to pdf`; both PDFs rasterized with
   pdfspine's own `get_pixmap`; SSIM via `render_diff.py` (`ssim:242-281`); **advisory band 0.80–0.90 —
   never in CI**.
4. **Once stable:** committed `.ssimref` references at `--min-ssim 0.97`, following the existing
   accuracy-gate pattern (`.github/workflows/ci.yml:189-194`).

**Reuse map (落点地图 — don't re-investigate):**
- **Pipeline skeleton (copy-adapt from `crates/pdf-markdown`):** the 5-stage pure pipeline
  (`src/lib.rs:117-124`) is the right skeleton. `model.rs` is replaced entirely by the new input IR;
  ~60–70% of `layout.rs` *logic* survives (greedy first-fit wrap + char-granularity force-split
  `layout.rs:310-394`; paginating `Ctx` + `ensure(h)` `:410-461`; pending-gap collapse-at-page-top
  `:442-452` — Word-ish space-before/after semantics for free; per-line pagination `emit_lines`
  `:468-488`; list-marker drawing `:709-775`; table measure/shrink `:834-945`); near-100% of `render.rs`
  assembly survives modulo N fonts (two-pass whole-doc glyph-usage accumulation `render.rs:39-91`, one
  `write_type0` per face per doc `:75-84`, deterministic `SaveOptions` + content-hash `/ID` `:13-15`,
  build-from-scratch seed `:159-177`). Keep top-left authoring coords + y-flip at emission
  (`layout.rs:4-6`).
- **Measurement invariant to preserve:** measurement and drawing share one resolution path
  (`crates/pdf-markdown/src/fonts.rs:16-17`; `FontSet::advance` `:209-218`; per-char fallback `resolve`
  `:169-205`; gid memoization `:99-121`); Base-14 advances via `pdf_fonts::std_widths::string_advance`
  (`crates/pdf-fonts/src/std_widths.rs:147-157`). Advances stay strictly additive per char — no shaper.
- **EmbeddedFont (`crates/pdf-edit/src/fontfile.rs`):** `parse` (`:61-100`), `glyph_id` (`:105-112`),
  `write_type0` Type0/Identity-H with usage-scoped `/W` + `/ToUnicode` (`:155-254`, `:256-268`,
  `:289-314`); the always-written ToUnicode is what guarantees gate-1 read-back extractability.
- **⚠ TRAP — face index 0 is hardcoded TWICE** (`fontfile.rs:62` and the `glyph_id` re-parse at `:108`);
  `ttf_parser::fonts_in_collection` is used nowhere in the repo. TS-3 must thread one `face_index`
  through both call sites.
- **⚠ TRAP — no subsetting exists today:** the whole program is embedded verbatim
  (`fontfile.rs:36-37,81,156-171`); the module doc's "feature-gated `subset` path" (`fontfile.rs:4-5`)
  does not exist anywhere. Raw `.ttc` bytes would embed the **entire multi-face collection** (macOS
  Songti.ttc ≈ 90 MB) — TS-3's subsetter is what makes system-CJK embedding viable at all.
- **⚠ TRAP — `Frag` has no per-run size** (`crates/pdf-markdown/src/layout.rs:148-156`) and `emit_lines`
  assumes one size per paragraph (`:468-476`); `Face` is a fixed 7-variant enum with fixed `F0..F6`
  resource names (`fonts.rs:51-77`) that ripples into assembly pass-1 (`render.rs:41-43,73`).
  pdf-typeset replaces both with `FaceId(usize)` + size-carrying frags; line height = max over frags,
  baseline from real ascent/descent already carried by `EmbeddedFont` (`fontfile.rs:84-88`) — retire the
  `BASELINE_FACTOR = 0.8` heuristic (`layout.rs:23-24`).
- **⚠ TRAP — justify exists nowhere:** `align_offset` is Left/Center/Right only (`layout.rs:516-522`),
  paragraphs hardcode Left (`:573-580`), and pdf-edit's `Align::Justify` silently renders Left
  (`crates/pdf-edit/src/text.rs:44-48`). `Tw` cannot implement it either (Identity-H 2-byte codes — `Tw`
  only affects single-byte code 32) → justify = redistributing the line's space-frag widths, last line
  left.
- **⚠ TRAP — RGB only, no alpha, no clip, no join/cap:** `Color` emits `rg`/`RG` only
  (`crates/pdf-edit/src/color.rs:5-50`); no `ca`/`CA` ExtGState in any authoring API (the only ExtGState
  is the hardcoded highlight `/BM /Multiply`, `annot.rs:2044-2058`); no `W`/`W n` is ever emitted by
  authoring code; `finish()` parameterizes width + dashes only (`drawing.rs:263-271`).
- **⚠ TRAP — `insert_textbox` drops overflow silently** (`text.rs:238`; §9's trap stands): flow layout
  must keep doing its own measure/wrap/paginate; box layout does its own v-align/autofit/clip.
- **Drawing primitives (`crates/pdf-edit/src/drawing.rs`):** `Shape` builder (`:28-42`) — `draw_line`
  `:86` / `draw_polyline` `:96` / `draw_rect` `:118` / `draw_bezier` `:131` / Catmull-Rom `draw_curve`
  `:151` / 4-Bézier `draw_oval`/`draw_circle` `:187-249`; `finish(color, fill, width, dashes, even_odd,
  close_path)` wraps each group `q…Q` (`:263-317`); even-odd `f*` enables donut/frame multi-subpath
  fills. Straight-edge presets (triangles, diamonds, arrows, stars, chevron…) are drawable **today**;
  only arcs/roundRect need the new arc→Bézier segmenting.
- **Transform precedent:** full affine `Matrix` incl. arbitrary-angle `rotate(deg)`
  (`crates/pdf-core/src/geom/matrix.rs:95`, full ops `:9-193`); arbitrary `q a b c d e f cm … Q` emission
  proven in `show_pdf_page` placement (`crates/pdf-edit/src/merge.rs:202-211`). pptx `rot`/`flipH`/
  `flipV` and text rotation = lay out unrotated, wrap the op batch in `q <cm> … Q`.
- **Gate machinery (reuse as-is):** `Page.get_text` words-with-coords
  (`python/pdfspine/document.py:1813-1843`); `conformance/gt/score.py` (`content_scores:152`,
  `order_score:198`); `render_diff.py` (`ssim:242-281`, `_near_blank:463-469`); the committed-`.ssimref`
  gate pattern (`.github/workflows/ci.yml:189-194`).
- **Consumer wiring precedent:** ocrspine-style git dependency + pinned rev (docspine
  `Cargo.toml:22-29`); pdf-typeset re-exports the needed `pdf_edit`/`pdf_core` types so consumers stay
  single-dep.

**Design sketch (`crates/pdf-typeset` — deps: pdf-core + pdf-edit + pdf-fonts + fontdb):**
- **Modules:** `model` (input IR) · `fontres` (fontdb + resolver) · `faces` (`FaceId` registry +
  embed/subset bookkeeping) · `flow` (tokens/wrap/paginate — generalized `layout.rs`) · `boxes` (TextBox
  layout) · `preset` (autoshape outlines → `Shape` ops) · `table` (grid measure / cell layout / per-edge
  borders) · `emit` (op IR → content streams → bytes — copy-adapt `render.rs`) · `warn`
  (`ExportWarning`). `lib.rs` re-exports the consumer-facing pdf-edit/pdf-core surface.
- **Input model (signature level; `#[non_exhaustive]` where growth is expected):**

  ```rust
  pub struct Run { pub text: String, pub style: RunStyle }
  pub struct RunStyle { pub family: String, pub size: f64, pub bold: bool, pub italic: bool,
      pub underline: bool, pub strike: bool, pub color: Rgb, pub highlight: Option<Rgb> }
  pub enum Align { Left, Center, Right, Justify }
  pub enum LineSpacing { Multiple(f64), Exact(f64) }
  pub struct ParaProps { pub align: Align, pub spacing: LineSpacing, pub space_before: f64,
      pub space_after: f64, pub indent_left: f64, pub indent_right: f64,
      pub first_line_indent: f64 /* negative = hanging */, pub list: Option<ListLabel> }
  pub struct ListLabel { pub text: String, pub gutter: f64 } // consumer computes counters/format
  pub enum Block { Paragraph(ParaProps, Vec<Run>), Table(TableSpec), Image(ImageSpec), PageBreak }
  pub enum VAnchor { Top, Middle, Bottom }
  pub struct TextBoxSpec { pub rect: Rect, pub v_anchor: VAnchor, pub wrap: bool,
      pub font_scale: Option<f64> /* normAutofit */, pub rotation_deg: f64, pub clip: bool,
      pub blocks: Vec<Block> }
  pub struct ExportResult { pub pdf: Vec<u8>, pub warnings: Vec<ExportWarning> }
  #[non_exhaustive] pub enum ExportWarning { FontSubstituted { requested: String, used: String },
      GlyphFallback { ch: char, family: String }, PresetDegraded { preset: String },
      GradientDegraded { .. }, BoxOverflowClipped { .. } /* … */ }
  ```

  List counters/numbering formats (`%1.%2`, roman/alpha, restarts) are computed by the **consumer** (it
  owns `numbering.xml` / `buChar` semantics); pdf-typeset receives the final label string + indents.
- **Resolver architecture:** `fontdb::Database` (system scan, or injected `load_font_data` fixtures for
  tests) → first-party folded-name index + substitution tables + weight/style `Query` → `(bytes,
  face_index)` via `with_face_data` → `FaceId` in the embed registry (programs cached per export run);
  per-char fallback chain resolved at tokenization time (same place as today, `layout.rs:248`); every
  substitution/degrade appends an `ExportWarning` — degrade-never-panic (`fonts.rs:15-17` house rule).
- **Flow core:** pagination becomes a callback/trait (`PageProvider: fn next_page(&mut self) ->
  PageGeom`) so docspine sections (per-section page size/margins), plain pages, and fixed-box layout
  (no pagination; clip/overflow policy instead) all share one measure/wrap/emit core.

**Phased plan** (effort per §4 scale — **S** ≈ hours · **M** ≈ 1–2 days · **L** ≈ multi-day; each task
lists why · files · effort · **Acceptance**, the green condition that means "done"):
- **TS-1 · Crate scaffold + input model** — *M*. New `crates/pdf-typeset` (copy MD-0's scaffold recipe:
  workspace members + `[workspace.dependencies]` + `supply-chain/config.toml` policy); the `model`/`warn`
  types above; op IR (extend pdf-markdown's `Op` vocabulary, `layout.rs:88-139`, with size-carrying text
  + shape/alpha/clip ops). **Acceptance:** workspace fmt / clippy `-D warnings` / test green with the
  crate in; existing floors untouched (`cargo test --workspace` ≥ **1445 passed / 0 failed**;
  pdf-markdown outputs byte-identical); model-construction unit tests pass.
- **TS-2 · System font resolution** — *L*.
  - fontdb 0.23 wiring, pinned exact; `memmap` feature decision recorded (*S*).
  - Folded-name index + three-platform substitution tables (built-in defaults + user override) (*M*).
  - Weight/style `Query` mapping + per-char fallback chain + bundled Liberation/Noto final fallback +
    `FontSubstituted`/`GlyphFallback` warning channel (*M*).
  - **Acceptance:** `cargo deny check` green with fontdb added (`deny.toml:1-27` allowlist);
    `THIRD-PARTY-NOTICES.md` hand-extended; resolver unit tests run against an **injected**
    deterministic Database (committed fixture fonts, no system dependence) and pass on all 3 CI OSes; a
    local (non-CI) macOS test resolves 宋体 → Songti SC and 微软雅黑 → PingFang SC; an unknown family
    returns Liberation + exactly one `FontSubstituted` warning (never an error).
- **TS-3 · Multi-face EmbeddedFont + TTC face index + glyph subsetter** — *L*.
  - `EmbeddedFont::parse_indexed(program, face_index)` threading the index through `fontfile.rs:62` and
    `:108`, + `fonts_in_collection` enumeration (*S*).
  - 4-slot family registry (regular/bold/italic/bold-italic, each embedded once per doc) replacing the
    fixed 7-face bookkeeping (`fonts.rs:51-77`, `render.rs:41-43,73`) (*M*).
  - Usage-based glyph subsetter: rebuild glyf/loca/cmap/hmtx/head/hhea/maxp (+ minimal name/post/OS-2)
    from used gids with composite-glyph closure; whole-font embed behind a debug flag (*L* core).
  - **Acceptance:** a doc using all four styles embeds **exactly 4 `/FontFile2`** (extend the
    one-FontFile2 lock, `crates/pdf-markdown/tests/fonts_embed.rs:23`); embedding ≤ 100 glyphs from a
    ≥ 10 MB system TTC face yields a FontFile2 **< 5% of source size**; subset text read-back exact
    (gate 1 ≥ 0.99) and raster SSIM subset-vs-whole-font **≥ 0.99** (`render_diff.py ssim:242-281`).
- **TS-4 · Flow-layout generalization** — *L*.
  - `FaceId` + per-frag size + real-ascent line boxes (mixed sizes in one line share one baseline) (*M*).
  - Justify via space-frag redistribution, last line left (*M*).
  - Underline/strike/highlight decorations (clone the strike mechanism `layout.rs:502-512`; highlight =
    `FillRect` behind frags using line-box metrics) (*S*).
  - First-line/hanging/left/right indents (per-line-index widths in `wrap()`) + configurable line
    spacing (multiple/exact) + space before/after (parameterize the `layout.rs:19-30` consts) + list
    labels from `ListLabel` (drawing side reusable, `layout.rs:709-775`) (*M*).
  - Table primitives generalized from `layout_table` (`layout.rs:834-945`): fixed + auto grid measure
    (keep fair-share shrink `:865-889`), cell block layout, per-edge border painting (4 `Line` ops per
    cell instead of `StrokeRect`) (*M*).
  - **Acceptance:** synthetic-fixture read-back green — **token-F1 ≥ 0.99 AND order ≥ 0.99** (`score.py`
    `content_scores:152` / `order_score:198`); justified interior lines' right edges within **0.5 pt** of
    the column edge and mixed-size lines share a single baseline y (asserted on `get_text_words` coords,
    `document.py:1813-1843`); repeated runs byte-identical (same font environment).
- **TS-5 · Text boxes** — *M*. Fixed rect + `VAnchor` (two-pass: wrap → total height → offset), wrap-off
  mode (hard-break lines only), `normAutofit` fontScale (binary-search re-wrap over the pure measure
  path), rotation (`q cm Q` wrap via `Matrix::rotate`), optional `re W n` clip. **Acceptance:** all
  words of a boxed fixture land inside the box rect **± 1 pt** (`get_text_words`); middle/bottom-anchored
  fixtures hit the expected first-baseline y ± 1 pt; an overflowing autofit fixture scales down until
  **zero words lost** in read-back; a 90°-rotated box still passes read-back + non-blank raster
  (`_near_blank:463-469`).
- **TS-6 · Preset geometry subset + drawing upgrades** — *L*.
  - pdf-edit `Shape` upgrades: arbitrary elliptical-arc→Bézier segmenting + roundRect; line join/cap
    params; constant-alpha ExtGState (`ca`/`CA` via `add_resource`, `content.rs:141-172`); `W n`
    clipping; shape-level `q cm Q` transforms (*M*).
  - The ~35 preset outlines, table-driven, + `rot`/`flipH`/`flipV` handling (*M*).
  - Degradation policy: unknown preset → bounding-box rect + `PresetDegraded`; gradient →
    representative solid + `GradientDegraded`; shadows dropped + warning (*S*).
  - **Acceptance:** every v1 preset has a deterministic raster fixture that is non-blank
    (`_near_blank:463-469`) and matches its committed reference at **SSIM ≥ 0.97**; an unsupported
    preset produces the rect fallback + exactly one `PresetDegraded` warning (asserted); alpha fills
    register one ExtGState object reused across ops (object-count assert).
- **TS-7 · Conformance harness extension** — *M*. Wire typeset fixtures into the gate stack: read-back
  scoring via `score.py`; committed `.ssimref` at `--min-ssim 0.97` (`ci.yml:189-194` pattern); and the
  **local-only** LO-oracle script (`soffice --headless --convert-to pdf`, rasterize both sides with
  pdfspine `get_pixmap`, SSIM via `render_diff.py ssim:242-281`, advisory band 0.80–0.90).
  **Acceptance:** the CI accuracy-gate job fails on a seeded layout regression and passes at HEAD
  (canary-and-revert proven — the P0-4 norm); the LO script runs end-to-end locally on one `.docx` and
  one `.pptx` sample and emits an advisory report; **no LibreOffice dependency appears in CI**.
- **Downstream unblocking:** **Phase B (pptspine `ppt-render`)** starts when the TS-2/3/5/6 gates are
  green; **Phase C (docspine `doc-render`)** when the TS-2/3/4 gates (incl. TS-4's table primitives) are
  green. Phase B/C PRDs live in the consumer repos; each pins a pdfspine rev with its needed TS tasks
  landed.

**Resume pointer.** Nothing in §10 has started — begin at **TS-1**. The family-wide decisions above are
locked (2026-07-02 design brief): do not reopen fontdb-vs-alternatives, the no-synthetic-bold rule, the
~35-preset subset, or the gradients-out call. `markdown_to_pdf` (§9) must stay green and untouched
throughout Phase A — rewiring pdf-markdown onto pdf-typeset is a future option, not this phase. Verify
every TS task with the §8 suite plus this section's gate stack.

---

*Re-verified 2026-06-19 from a code-level 5-dimension survey (project health · API parity · rendering ·
extraction/conformance · perf/OCR). §3 is the correction log against this doc's previous A–F framing.
**Phase 0 + P0-5r + Phase 1 on 2026-06-19; Phase 2 + P3-1/P3-2/P3-3/P3-4 on 2026-06-20** (on `main`; coverage
84.7%→**88.7%**; multi-column + colorspaces at parity; OCR `recognize()` parallel + Font program bytes) — §3
rows C1 / C2 / C4 / C6 / C7 / C10 / C11 / C13 fixed + P0-6r closed; P3-5 GriTS harness landed (score blocked
on sandbox CDN egress). **P4-1 / P4-3 / P4-4 / P4-2 landed 2026-06-21**, and a **residuals-clearing sweep the
same day (2026-06-21)** fixed P0-2r / P2r-1 / P3-3r / P3-4r / P4-2r / **P1-1r** (Symbol/ZapfDingbats via OFL
Noto) and resolved P2r-2 as not-a-bug; **render SSIM re-measured 0.945→0.984** (at fitz parity). All
oracle-cross-checked; cargo/clippy clean, pytest 721, P1-3 gate green, parity 88.7%. **Phases 0–4 + every
actionable item complete**; **P3-5 scored 2026-07-02** (DAX dead → HF-mirror fetch; lines 0.073 fitz-parity /
text 0.185 capability) — remaining = **P3-1r** (accepted won't-fix), **§9 MD-1..MD-4**, and the pre-public
flip-to-public.*
