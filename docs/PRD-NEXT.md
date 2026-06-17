# PRD-NEXT — Remaining Work Roadmap

> Live to-do list for resuming the oxide-pdf build. Updated 2026-06-17. Completed work has been
> removed; this file tracks only what is LEFT. Symbol-coverage source of truth = `COMPAT.toml`;
> benchmark numbers = `docs/BENCHMARKS.md` + machine reports in `conformance/`.

## 0. Current state (what is DONE — do not redo)

**Text extraction is at parity with fitz** (proven objectively across born-digital, PMC scientific,
EUR-Lex 8 languages, CJK, GovInfo court/GAO/Federal-Register, and a robustness corpus — see
`docs/BENCHMARKS.md`). Five extraction fixes landed: column-major reading order, inter-word space
synthesis, device-space gap threshold, baseline-merged column split, and `find_tables` ruling-line
gating. Tables, multilingual, CJK, and domain breadth all measured at near-parity.

**Rendering (`get_pixmap`) is now near-parity for embedded-font text**: SSIM ~0.58 → **0.945 mean /
0.986 median** vs fitz after four root-cause fixes (full per-glyph `Trm` into the render path;
bare-CFF `FontFile3` parsing; CCITT/JBIG2 1-bpc polarity; CID-keyed CFF charset CID→GID). See §2.A
for what landed and the remaining long tail. **1338+ Rust + 391 pytest green.** API coverage **65.7%**
(505/769 in `COMPAT.toml`) after batch-1 Page geometry/boxes.

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

### A. RENDERING — major progress; long tail remains (MEDIUM value now)
`get_pixmap` jumped from SSIM ~0.58 → **0.945 mean / 0.986 median** vs fitz
(`conformance/gt/RENDER-REPORT.md`; corpus-born 0.995, eurlex 0.943, pmc 0.991, robustness 0.843,
fixtures 0.971). Four root-cause fixes landed:
1. ~~**Glyph horizontal positioning**~~ **DONE.** Root cause: the renderer scaled each glyph outline
   by `size/upem` (`Tfs` only), ignoring the CTM / text-matrix linear scale — so any PDF that bakes
   the font size into `Tm`/`cm` (Chrome, most PMC) drew glyphs 2× too big and overlapping. Fix:
   `pdf-text` now carries the full per-glyph `Trm = params·Tm·CTM` into the render path
   (`TextRun.trms`); the renderer places each outline with `scale(1/upem)·Trm·base` (also fixes
   rotated/sheared text). corpus-born 0.65 → 0.995.
2. ~~**Body glyphs not drawn for some embedded font types**~~ **DONE for bare CFF.** Root cause:
   `FontFile3` with `/Subtype /Type1C` (simple) or `/CIDFontType0C` (CID) is *bare* CFF (no sfnt
   wrapper), which `ttf-parser`'s `Face::parse` rejects (`UnknownMagic`) → whole pages blank. Fix:
   `GlyphFont` now falls back to `ttf-parser`'s public `cff::Table::parse` for bare CFF, and
   `resolve_gid` resolves simple-CFF glyphs by AGL name (charset) instead of code-as-gid. corpus-pmc
   0.40 → 0.86.
3. ~~**Image inversion (CCITT/JBIG2)**~~ **DONE.** Both 1-bpc fax/scan codecs emitted the fax-native
   "ink = 1" polarity, but the shared upsample (`bit 1 → 255`) + stencil path (`bit 0 → paint`) use
   the standard DeviceGray convention (`0 = black`), so every CCITT/JBIG2 image rendered inverted
   (over-dark scans). Both now emit `0 = black, 1 = white`. corpus-robustness 0.73 → 0.84; the worst
   case `govdocs1-00018` went SSIM −0.17 → ~0.99.
4. ~~**CID-keyed CFF (`CIDFontType0C`) rendered blank**~~ **DONE.** A Type0 font with `Identity-H`
   hands the renderer the **CID**, but for a CFF CIDFont the CID→GID mapping is the **CFF charset**
   (not `CIDToGIDMap`, which only applies to CIDFontType2/TrueType), and a subset renumbers its GIDs.
   `resolve_gid` was using the CID directly as a GID → wrong/notdef glyph → blank body text on every
   CID-CFF PDF (most PMC). `GlyphFont` now builds a `CID→GID` map from `ttf-parser`'s `cff::glyph_cid`
   and routes CID-keyed CFF through it. corpus-pmc 0.86 → **0.99**.

Remaining long tail (each smaller / independent; measure with `render_diff.py`):
- **Bare Type1 PFB/PFA** (`/FontFile`) — not parseable by `ttf-parser`; needs a Type1 charstring
  interpreter (or Type1→CFF). Hits eurlex `32006L0112_ES`, some govdocs. Text stays extractable.
- **Non-embedded standard-14 fonts** (Helvetica/Times/Courier with no embedded program) are not
  rasterized — no license-clean substitute bundled. Blanks most govdocs1 body text.
- **Image/colorspace fidelity** — remaining nuances (Indexed/Separation/ICC colorspaces, `/Decode`
  arrays not yet applied in the render path, halftone smoothing) may still tint some scanned/image
  pages; the gross 1-bpc inversion is fixed.
- **Synthetic-bold / heavy display fonts** render slightly heavier than fitz (minor).
Renderer code: `crates/pdf-render`; glyph data plumbing in `crates/pdf-text` (`interp.rs`,
`renderops.rs`). Measure every change with `render_diff.py`.

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

### C. API parity coverage (track A) — 65.7% → higher
> **NB (drift fixed 2026-06-17):** batches 3 & 4 hand-edited `COMPAT.toml` (Font/Colorspace/Link/
> Outline/TextWriter/Tools/xref-write/text-trace → 63.7%) but did NOT update the generator
> `scripts/_compat_catalog.py`, so regenerating regressed coverage to 53.7%. A reconciliation pass at
> the end of `_compat_catalog.py` (`_BATCH34_IMPLEMENTED`, 92 symbols) re-syncs the generator to the
> committed truth. **Always run `python3 scripts/_compat_catalog.py` after dispositioning and confirm
> coverage rises (it is the source of truth) — never hand-edit `COMPAT.toml`.**

Full per-symbol spec exists (workflow `wf_f5e56138-2f9`; 146 symbols: 68 pure-python, 66 needs-rust,
9 already-exist → just update COMPAT, 3 reclassify-oos). Two groups need re-spec (socket-failed):
Shape-members, TextPage-extract. The monoliths `python/oxide_pdf/document.py` + `crates/py-bindings/
src/lib.rs` mean batches that both touch them run SEQUENTIALLY; new pytest goes in
`python/tests/test_longtail6.py` (test_longtail5.py = batch-1). Batch order (cheap pure-python first):
1. ~~**Page geometry/boxes**~~ **DONE (batch-1, 2026-06-17, +15 → 65.7%):** `set_mediabox`/
   `set_cropbox`/`set_artbox`/`set_bleedbox`/`set_trimbox`, `artbox`/`bleedbox`/`trimbox`,
   `transformation_matrix`/`rotation_matrix`/`derotation_matrix` (fitz-matched at rot 0/90/180/270),
   `xref`, `parent` (python-level owning-Document ref), `mediabox_size`/`cropbox_position`. Only
   `remove_rotation` left deferred (needs content-stream rewriting). Tests in `test_longtail5.py`.
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
