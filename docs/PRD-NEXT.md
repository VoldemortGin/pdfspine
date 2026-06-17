# PRD-NEXT — Remaining Work Roadmap

> Live to-do list for resuming the oxide-pdf build. Updated 2026-06-17. Completed work has been
> removed; this file tracks only what is LEFT. Symbol-coverage source of truth = `COMPAT.toml`;
> benchmark numbers = `docs/BENCHMARKS.md` + machine reports in `conformance/`.

## 0. Current state (what is DONE — do not redo)

**Text extraction is at parity with fitz** (proven objectively across born-digital, PMC scientific,
EUR-Lex 8 languages, CJK, GovInfo court/GAO/Federal-Register, and a robustness corpus — see
`docs/BENCHMARKS.md`). Five extraction fixes landed: column-major reading order, inter-word space
synthesis, device-space gap threshold, baseline-merged column split, and `find_tables` ruling-line
gating. Tables, multilingual, CJK, and domain breadth all measured at near-parity. **1335 Rust +
374 pytest green.** API coverage 63.7% (490/769 in `COMPAT.toml`).

## 1. Tools available (reuse, don't rebuild)

Objective ground-truth + differential harness lives in `conformance/gt/` (scripts committed;
corpora/cache/`*-results.json` gitignored, regenerable):
- `run_gt.py` — scores oxide vs fitz vs pdfminer vs SAME ground truth → `GT-REPORT.md`.
- `score.py` — decomposed metrics (lev/f1/jaccard/order), CJK-aware, NFKC normalization.
- Fetchers/generators: `born_digital.py`, `born_cjk.py`, `pmc_fetch.py`, `fetch_eurlex.py`
  (8 langs), `fetch_govinfo.py` (court/GAO/FR), `fetch_robustness.py` (GovDocs1/SafeDocs).
- `tables_diff.py` — find_tables vs fitz. `render_diff.py` — get_pixmap vs fitz (SSIM).
- Real-corpus differential vs fitz: `conformance/run_validation.py` + `fetch_corpus.py`.
- Oracle venv `.venv-oracle` (fitz 1.27 + pdfminer); project venv `.venv` (oxide_pdf wheel). No
  oracle output is ever committed (clean-room / AGPL-safe).

## 2. Remaining work

### A. RENDERING — the biggest remaining gap to "match fitz" (HIGH value, substantial)
`get_pixmap` is NOT at parity: SSIM ~0.58 vs fitz (`conformance/gt/RENDER-REPORT.md`). Page geometry
is correct (page-box sizes match ≤1px); the glyph rasterizer is the problem. Findings, by impact:
1. **Glyph horizontal positioning** — glyphs overlap/compress on clean text (wrong advance-width
   handling or text-matrix scaling). Hits ALL text; fix first. Objective fn = born-digital render
   SSIM (`render_diff.py` on `corpus-born`), target ≥0.9. NB extraction positions are already
   correct, so the renderer uses a different/buggy advance path than `pdf-text` layout.
2. **Body glyphs not drawn for some embedded font types** (subset CIDFonts / Type1 / certain
   encodings) — vector rules render but text is blank on many PMC/IRS/govdocs pages.
3. **Image/colorspace fidelity** — scanned-image pages render with wrong brightness/contrast.
4. **Synthetic-bold / heavy display fonts** render lighter than fitz (minor).
Renderer code: `crates/pdf-render`. Measure every change with `render_diff.py`.

### B. Extraction breadth (LOWER priority — diminishing returns; text already at parity)
- **RTL / Arabic (bidi)** — the one untested script class; most likely to surface a real bug. Needs
  bidi-aware GT (visual vs logical order). Born-digital Arabic (Chrome) or UN ODS Arabic PDFs.
- **FinTabNet gold table GT** — now that `find_tables` is fixed, validate table *structure* against
  human ground truth (not just fitz). FinTabNet (IBM, CDLA-Permissive) ships real PDF pages + cell
  structure; HF `bsmock/FinTabNet.c`. (Earlier fetch was flaky — retry.)
- **Scale robustness** — `fetch_robustness.py` got only 23 (throttled link); rerun for thousands of
  GovDocs1/SafeDocs PDFs for stronger never-panic + differential evidence.
- **More domains/langs** — DocLayNet (finance/law/patent/manual, per-cell text GT; official 7.5GB
  zip ships real PDFs — HF mirrors strip them; needs zip64 range-extraction), more EUR-Lex, Japanese.
- **Kangxi-radical fold (CJK polish)** — oxide raw CJK output uses radical codepoints (U+2F09 ⼉)
  where fitz folds to canonical ideographs (U+513F 儿). NFKC-equivalent/cosmetic. Small
  `pdf-fonts` CID→Unicode fix.
- **CI gate** — wire a born-digital `order ≥ 0.95` (and tables count-agreement) regression gate into CI.

### C. API parity coverage (track A) — 63.7% → higher
Full per-symbol spec exists (workflow `wf_f5e56138-2f9`; 146 symbols: 68 pure-python, 66 needs-rust,
9 already-exist → just update COMPAT, 3 reclassify-oos). Two groups need re-spec (socket-failed):
Shape-members, TextPage-extract. The monoliths `python/oxide_pdf/document.py` + `crates/py-bindings/
src/lib.rs` mean batches that both touch them run SEQUENTIALLY; new pytest goes in
`python/tests/test_longtail5.py`. Suggested batch order (cheap pure-python first):
1. **Page geometry/boxes** — `set_mediabox`/`set_cropbox` (PageEditor methods already exist in
   pdf-edit; need pdf-api facade + PyPage binding + stub), `set_artbox`/`bleedbox`/`trimbox`
   (pure-python via `xref_set_key` once Page carries a Document parent ref — shared prerequisite),
   `transformation_matrix`/`rotation_matrix`/`derotation_matrix`, `xref`/`parent`, `mediabox_size`/
   `cropbox_position`, `remove_rotation`.
2. **Document page-helpers** — `get_page_images`/`get_page_fonts`/`search_page_for`/`get_page_pixmap`
   (one-line delegations), `get_page_labels`/`get_page_numbers`/`get_label`, page-ops
   `insert_page`/`copy_page`/`move_page`/`delete_pages`.
3. **Annot members** + **Widget appearance** (colors/border/text-style) + **Shape** draw_quad/sector/
   squiggle/zigzag + insert_text/insert_textbox + props.
4. **TextPage** extractHTML/XHTML/XML/extractSelection/Textbox/search; **Font** glyph_bbox/
   valid_codepoints/buffer.
5. Document low-level COS (`update_object`/`update_stream`/`get_new_xref`/…), state/meta, OCG/layers.
Regenerate `COMPAT.toml` + refresh `PARITY.md` after each batch.

## 3. Verify suite (run from repo root before every commit)
```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                      # expect 1335+ passed, 0 failed
source .venv/bin/activate && env -u CONDA_PREFIX maturin develop -q
env -u CONDA_PREFIX python -m pytest python/tests/ -q     # expect 374+ passed
```
Gotchas: maturin needs `env -u CONDA_PREFIX`. Commit messages: **no backticks** (shell substitutes
them). Only ONE agent rebuilds the wheel at a time; don't run scoring while a wheel rebuild is in
flight (shared `.venv`). Subagents must not commit (main loop verifies + commits).

## 4. Pre-public chores (do last, before going public)
Folder rename `~/workspace/pypdf` → `oxide-pdf` + recreate `.venv` (FINAL step); commit-message
backtick reword; PyPI publish (`docs/RELEASE-PYPI.md`). Repo stays PRIVATE until everything is done
(full parity + accuracy + docs + CLI + OCR), then flip to public + push.
