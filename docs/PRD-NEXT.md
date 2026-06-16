# PRD-NEXT — Accuracy & Parity Roadmap (continuation)

> Working doc for resuming the oxide-pdf build. Created 2026-06-16. Priority order set by
> the user: **B (extraction accuracy) first, A (API parity coverage) interleaved.**
> Single source of truth for symbol coverage remains `COMPAT.toml`; this file tracks the
> in-flight accuracy work + the next coverage batches.

## 0. Status snapshot (as of 2026-06-16, this session)

Baseline before session: 1325 Rust + 374 pytest green; COMPAT coverage 63.7% (490/769).

**Done & committed this session:**
- `05e90a3` — Objective ground-truth accuracy harness `conformance/gt/` (see §2).
- `15d02a9` — **Multi-column reading-order fix** (occupancy-valley gutter detection in
  `crates/pdf-text/src/layout.rs`). Born-digital `order` 0.41–0.60 → **0.995–0.999**
  (now matching fitz 1.0); no PMC regression; 3 committed regression tests.
  Verified: fmt clean, clippy clean, cargo test **1325 passed**, pytest **374 passed**.

- `fa31931` — **Inter-word space synthesis** for TJ-kerned PDFs (#2 accuracy, §1.1).
  `build_line` synthesizes a space when along-axis glyph gap > shared `WORD_GAP_FRAC`
  (0.2×font). **PMC oxide f1 0.219→0.513, jaccard 0.181→0.403, order 0.650→0.965** — now
  matching fitz (0.530/0.976); born not regressed; 4 regression tests; cargo test + pytest
  374 green. (Done this session — was in-flight, now committed.)

## 1. Track B — Extraction accuracy (PRIORITY)

The headline goal "match or exceed fitz" is now **measured objectively** (§2), not via a
pseudo-oracle. Two problems were empirically separated:

- **Reading order** — FIXED this session (born-digital isolated it: order 0.55→0.998).
- **Content (word spacing + decoding)** — partially in-flight.

### 1.1 Missing inter-word spaces (IN-FLIGHT — highest remaining B value)
On TJ-kerned/LaTeX PDFs (e.g. PLoS papers) words are positioned without literal space
glyphs. `get_text("words")` recovers boundaries (gap-based, `words.rs WORD_GAP_FRAC=0.2`)
but `get_text("text"/"dict"/"blocks")` did NOT — `build_line()` concatenated glyphs with no
gap→space synthesis. Symptom: PMC212687 → `'AFunctionalAnalysisoftheSpacer'`.
Fix = synthesize spaces in `build_line` using the shared 0.2×font threshold (don't double
real spaces; compute gap along the reading axis; share the constant with words.rs; add
regression tests). **Objective:** PMC oxide f1 0.219→~0.5, order 0.650→~0.95; born must not
regress (lev ~0.92, order ~0.998).

### 1.2 Remaining content gap after spaces (diagnose next)
After 1.1, re-measure PMC. If oxide f1 still trails fitz (fitz ~0.53 vs GT), diagnose the
residual: candidate causes = glyph/CMap decoding on scientific Type1/CFF fonts, dropped
columns/pages, spurious tokens. Use the per-extractor token precision/recall in
`conformance/gt/run_gt.py` output (fitz-vs-GT vs oxide-vs-GT diff) to localize.

### 1.3 Refresh the fitz-oracle corpus report on current HEAD
`conformance/REPORT.md` (text vs fitz: Lev 0.823) is from 2026-06-15, BEFORE the
reading-order + spacing fixes. Re-fetch (`fetch_corpus.py`) + re-run `run_validation.py`
to get refreshed numbers; the multi-column IRS forms (p501 0.387, p502 0.537, p15 0.544)
should improve markedly now.

### 1.4 Strengthen the ground-truth layers
- **Expand born-digital** (`conformance/gt/born_digital.py`): add tables, figure+caption,
  footnotes, mixed 1/2-col, wider font/size variety. Cheap, perfect GT, zero license risk.
- **Expand PMC** (`pmc_fetch.py --n 25..50`): prefer final-published-version articles;
  improve XML↔PDF correspondence (body-only normalization both sides) so absolute content
  scores aren't depressed by the structural mismatch. NOTE: NCBI `oa_package` paths moved
  under `/pub/pmc/deprecated/` and are scheduled for **removal Aug 2026** → migrate the
  fetcher to AWS S3 `pmc-oa-opendata` (version-keyed, `license_code` in JSON) before then.
- **(Optional) GROTOAP2 TrueViz** layer (CC-BY): gold per-zone reading-order labels on PMC
  PDFs — the strongest objective reading-order scorer. Keep CERMINE *code* (AGPL) out.
- **Human spot-check**: for oxide-vs-fitz divergences, confirm who's right (catches the
  cases where we should *exceed* fitz, which the similarity metric alone can't credit).

### 1.5 Extend differential beyond text
- **Rendering**: pixel-diff oxide `get_pixmap` vs fitz render on the corpus (SSIM/MSE).
- **Tables**: `find_tables` vs fitz / vs JATS `<table-wrap>` ground truth.

### 1.6 Wire GT harness into CI as a regression gate
Add a fast born-digital subset (committed tiny synthetic PDFs, or generate in CI) with an
`order ≥ 0.95` gate so reading-order can never silently regress.

## 2. Reference — the objective GT harness (`conformance/gt/`)
Built this session. Scripts committed; corpora/cache/raw-json gitignored (regenerable).
- `score.py` — decomposed metrics: token P/R/F1, set Jaccard, **order** (alignment-based,
  isolates ordering from content), Levenshtein ratio; NFKC + ligature/soft-hyphen/de-hyphen
  normalization.
- `born_digital.py` — Chrome-rendered multi-column PDFs from public-domain prose (Gutenberg);
  source order = known reading order. Variants: 1col/2col/2col-justified/3col/2col-with-header/
  2col-narrow-gutter. `--out conformance/gt/corpus-born`.
- `pmc_fetch.py` — CC-BY/CC0 PMC OA sample (real PDF + JATS XML). `--out corpus-pmc --n N`.
- `jats_text.py` — JATS/NXML → logical-order body ground truth.
- `run_gt.py` — scores oxide vs fitz vs pdfminer vs SAME ground truth; emits GT-REPORT*.md +
  json with a "match/exceed fitz (order)" head-to-head. `--manifest <m> [--manifest <m2>]`.
- Oracle venv: `.venv-oracle` (fitz 1.27 + pdfminer). Project venv: `.venv` (oxide_pdf wheel).
- No oracle output is committed (clean-room / AGPL-safe).

## 3. Verify suite (run from repo root before every commit)
```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                      # expect ~1325+ passed, 0 failed
source .venv/bin/activate && env -u CONDA_PREFIX maturin develop -q
env -u CONDA_PREFIX python -m pytest python/tests/ -q     # expect 374+ passed
# accuracy objective functions:
env -u CONDA_PREFIX .venv/bin/python conformance/gt/run_gt.py --manifest conformance/gt/corpus-born/manifest.json --report /tmp/b.md --json /tmp/b.json
env -u CONDA_PREFIX .venv/bin/python conformance/gt/run_gt.py --manifest conformance/gt/corpus-pmc/manifest.json  --report /tmp/p.md --json /tmp/p.json
```
Gotcha: maturin needs `env -u CONDA_PREFIX`. Commit messages: **no backticks** (shell
command-substitutes them).

## 4. Track A — API parity coverage (interleave)
Current: 63.7% (490/769 in `COMPAT.toml`). A full per-symbol implementation spec for the
remaining deferred long-tail was produced this session (workflow `wf_f5e56138-2f9`,
result cached at `/private/tmp/.../tasks/w2avqqcpb.output`): **146 symbols specced — 68
pure-python, 66 needs-rust, 9 already-exists (just update COMPAT), 3 reclassify-oos.**
Two groups failed to spec (socket error) and need re-spec: **Shape-members, TextPage-extract.**

Suggested batch order (land cheap pure-python first; monoliths `document.py` + `lib.rs`
mean batches that both touch them run SEQUENTIALLY; new tests go in `python/tests/test_longtail5.py`):
1. **Page geometry/boxes** — `set_mediabox/set_cropbox` (PageEditor methods already exist in
   pdf-edit; just need pdf-api facade + PyPage binding + stub), `set_artbox/bleedbox/trimbox`
   (pure-python via `xref_set_key` once Page carries a Document parent ref — needed by several),
   `transformation_matrix/rotation_matrix/derotation_matrix`, `xref/parent`, `mediabox_size/
   cropbox_position`, `remove_rotation`.
2. **Document page-helpers** — `get_page_images/get_page_fonts/search_page_for/get_page_pixmap`
   (one-line delegations to `self[pno].*`), `get_page_labels/get_page_numbers/get_label`,
   page-ops `insert_page/copy_page/move_page/delete_pages`.
3. **Annot members** + **Widget appearance members** (colors/border/text-style) + **Shape**
   draw_quad/sector/squiggle/zigzag + insert_text/insert_textbox + props.
4. **TextPage** extractHTML/XHTML/XML/extractSelection/Textbox/search; **Font** glyph_bbox/
   valid_codepoints/buffer.
5. Document low-level COS (`update_object/update_stream/get_new_xref/...`), state/meta,
   then OCG/layers (post-v1).
Reclassify to out-of-scope / mark already-exists per the spec; regenerate `COMPAT.toml`
and refresh `PARITY.md` after each batch.

## 5. Pre-public chores (unchanged, from memory)
Folder rename `~/workspace/pypdf` → `oxide-pdf` + recreate `.venv` (FINAL step); commit-message
reword (backtick history); PyPI publish (`docs/RELEASE-PYPI.md`). Repo stays PRIVATE until
everything done (full parity + real-corpus accuracy + docs + CLI + OCR).
