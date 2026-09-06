# HANDOFF — `get_text(clip=)` fix (WIP, 2026-09-05)

Branch `worktree-agent-adf6ef49d2e5f6b57` (worktree of main `21d636a`). Task:
PRD-NEXT §0 queue item 1 — the Rust side of `Page.get_text(..., clip=)` ignored
`clip` in every mode (text/dict/rawdict/words/blocks/json/…); only
`_markdown_source` / `get_text_layout` / `get_text_words` / `get_text_blocks`
filtered by bbox in Python. The agent was stopped for quota before tests and
gates ran; the code change is complete, `cargo fmt` applied, and `cargo check`
passes for `pdf-text`, `pdf-api` and `py-bindings`.

## 1. PyMuPDF semantics (real PyMuPDF 1.28.2, probe below)

Probe PDF: 300×300 page, `insert_text` lines "Header text" (y=30),
"alpha beta gamma" (baseline y=100), "delta epsilon" (y=140), "Footer" (y=280),
plus a 50×50 image at (200,200)-(250,250). `band = (0, 90, 300, 115)`;
`b` = the glyph "b" of "beta", fitz bbox (82.688, 87.1, 89.36, 103.588).

| case | PyMuPDF 1.28.2 | pdfspine before | pdfspine after (design) |
|---|---|---|---|
| `text`, clip=band | `"alpha beta gamma\n"` | full page | same as fitz |
| clip cuts 30 % / 90 % into `b` | `"alpha b\n"` (glyph kept whole) | full page | same |
| clip ends exactly at `b.x0` | `"alpha \n"` (touch = out; space glyph kept) | full page | same |
| clip ends 0.01 pt into `b` cell | `"alpha \n"` (fitz tests the **ink** box) | full page | `"alpha b\n"` — known divergence: pdfspine only has the glyph cell |
| clip = bottom 20 % of line bbox | `"p\ng\n"` (descender ink only) | full page | whole line (cell overlap) — same divergence class |
| `dict` width/height with clip | clip's (84.69 × 25) | page's | clip's |
| `dict` block/line/span bbox | union of kept chars, **not** clamped to clip | — | same |
| image block inside / partial / outside clip | kept / bbox = intersection / kept with an inverted bbox (MuPDF artefact) | kept unchanged | kept / intersection / **dropped** (deliberate) |
| clip `(0,0,0,0)`, `Rect()`, outside page, inverted rect | `""` | full page | `""` (inverted rect is normalized → same result on this probe) |
| `html` / `xhtml` / `xml` with clip | clip ignored | ignored | ignored |
| `textpage=` given + clip | clip ignored (documented) | ignored | ignored |
| `get_textpage(clip=)`, any flags (0 too) | clipped | not clipped | clipped |
| `flags=0` / `flags=3` (no MEDIABOX_CLIP) + clip | still clipped → clip is **not** tied to `TEXT_MEDIABOX_CLIP` | — | always clipped |
| `words` partial | `("alpha")`, `("b")`, block/line numbers restart at 0 | full page | same |
| `search_for("beta", clip straddling b)` | `[]` (needle no longer in the clipped textpage) | 1 hit | `[]` |
| `sort=True` + clip | sorted after clipping | — | same |
| `Annot.get_text` | fitz extracts the annot's own appearance text (`""`) | page text | page text under the annot rect, now clipped (pdfspine's documented semantic; out of scope) |

Conclusion: PyMuPDF clips **per character at TextPage build time** (strict
overlap of the glyph ink box with the clip; touching is out), for every
`get_text` option and for `search_for`. Nothing is block-level.

Probe script + outputs: `scratchpad/probe/{probe.py,probe.pdf,fitz.json,pdfspine-before.json}`
under `/private/tmp/claude-501/-Users-linhan-startup-spine/239f61a9-0348-4965-8645-ae66f43b8c09/scratchpad`
(session scratch; may be gone — `probe.py` is ~120 lines and easy to recreate from the table).

## 2. What is done (uncommitted → committed as WIP)

- `crates/pdf-text/src/serialize.rs`: new `pub fn clip_textpage(tp, clip) -> TextPage`
  next to `get_textbox`, reusing `char_overlaps_clip` (strict overlap). Drops
  empty spans/lines/blocks, rebuilds `text`, span/line/block bbox (painted chars
  only, synthesized-space seam only when nothing else is left), span
  `origin`/`matrix`/`rendered_size`/`quad` (via `layout::DirEnvelope`, now
  `pub(crate)`), `seq` (min), block/line `number` restart at 0, image blocks
  intersected with the clip (dropped when empty), `width`/`height` = clip's.
- `crates/pdf-text/src/lib.rs`: exports `clip_textpage`.
- `crates/pdf-api/src/text.rs`: `textpage(page, flags, clip)` now applies the
  clip after the build (was `_clip`). This also makes `search` (which already
  passed `opts.clip`) and `PyTextPage`/`get_textpage(clip=)` honour it.
- `crates/py-bindings/src/lib.rs`: `Page.get_text` builds the TextPage with the
  clip when `textpage` is `None` and option is not html/xhtml/xml, then hands it
  to `text_output_to_py` unchanged — one dispatch-layer change, all modes covered.
  Doc comments of `get_text` / `get_textpage` updated.
- `CHANGELOG.md` Unreleased → `### Fixed` entry.
- No Python source changed. The Python bbox filters in `document.py`
  (`_markdown_source`, `get_text_layout`, `get_text_words`, `get_text_blocks`)
  are now redundant for `_markdown_source` but must stay (idempotent) — add a
  one-line comment there, nothing else.

## 3. Not done

1. Tests (none written yet):
   - Rust `crates/pdf-text/tests/serialize_unit.rs` → `SERIAL-CLIP-001..007`:
     strict overlap vs touch; empty result; bbox/number rebuild; width/height;
     image block kept/clamped/dropped; span origin/matrix from first kept char;
     full-page clip reproduces `to_text`/`to_words` of the unclipped page.
   - Rust `crates/pdf-api/tests/textpage_reuse.rs` → `TEXTPAGE-CLIP-001/002`:
     fixture "Hello" 12 pt, widths 500 → glyphs 6 pt wide from x=20, baseline
     device y=100; clip `(0,80,35,120)` → `"Hel"`; `search` "Hello" → 0 hits.
   - Python `python/tests/test_text.py` → `PYTEXT-012..018` (text/dict/rawdict/
     words/blocks clip, partial char, empty clip, clip outside page, html/xhtml/
     xml ignore, `textpage=` wins, `get_textpage(clip=)`, dict width/height,
     numbering restart, `search_for` straddle, real-PyMuPDF parity with the
     `_real_pymupdf()` skip pattern from `test_markdown_to_pdf.py`).
   - Catalog rows in `docs/test-case-catalog.md` for all of the above (green).
2. The 300-document no-clip byte-identity proof. `corpus300.py` (scratchpad)
   hashes text/dict/rawdict/words/blocks per document over the frozen manifest;
   corpus PDFs live only in the **main checkout** (`conformance/gt/corpus-*`,
   untracked), so run it with root `/Users/linhan/startup/spine/pdfspine`.
   Baseline run was killed mid-way (no complete `baseline.json`). Expected: all
   300 identical except `fixtures/typeset/typeset-lo-slide.pdf` (stale manifest
   hash → skip; handled by another agent).
3. Gates not run: clippy, `cargo test`, pytest, ruff, mypy, guard scripts.
4. `docs/guide/text-extraction.md` line ~38 could say the clip is character-level
   and ignored for html/xhtml/xml / with `textpage=` (optional).

## 4. Next commands

```sh
export CARGO_TARGET_DIR=/Volumes/Cargo/target/pdfspine-clip CARGO_BUILD_JOBS=4 TMPDIR=/Volumes/ExternalSSD/tmp
unset CONDA_PREFIX      # maturin refuses VIRTUAL_ENV + CONDA_PREFIX together
python3 -m venv .venv-task && . .venv-task/bin/activate
pip install "maturin>=1.12,<2" pytest hypothesis "ruff==0.14.14" mypy pymupdf
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test -p pdf-text -p pdf-api
maturin develop
python -m pytest -W error --doctest-modules python/pdfspine python/tests -x -q
ruff format --check python/pdfspine python/tests scripts && ruff check python/pdfspine python/tests scripts && mypy
for g in test-order-guard catalog-status-guard compat-symbol-guard manifest-lint; do python scripts/$g.py; done
```

## 5. Known risks

- `union_rects` starts from `Rect::default()` (empty) and relies on
  `Rect::union`'s empty-operand rule (same idiom as `words.rs::flush`).
- `search` behaviour changes only with `clip`: a needle straddling the clip
  edge is no longer a hit (PyMuPDF-faithful; no existing test covers it).
- `dict["width"/"height"]` under clip now equal the clip size (PyMuPDF rule);
  check `to_markdown(clip=)` still reads fine — `_markdown.py` uses only `bbox`.
- The ink-vs-cell divergence (table above) is inherent; document it in the
  catalog row / findings rather than trying to emulate ink boxes.
