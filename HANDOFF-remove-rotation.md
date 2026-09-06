# HANDOFF — `Page.remove_rotation()` widget fix (2026-09-05, WIP)

Branch `worktree-agent-ab9e257cbb9ab89c3` on top of main `21d636a`. Python-only
change; no Rust touched. Wrapped up early on coordinator request (quota), so the
full gate has NOT been run — see "Next steps".

## What is done

- `python/pdfspine/document.py` `Page.remove_rotation()`:
  - Widget rects are no longer assigned through the read-only `Widget.rect`
    (that was the `PdfUnsupportedError`). pdfspine's `Page.annots()` already
    yields the widget annotations, so their `/Rect` is rewritten through the
    `Annot` handle; the separate widget loop is gone.
  - Root cause found while fixing: pdfspine's `Annot.rect` / `Widget.rect` /
    `link["from"]` are PDF **user space (y-up, unrotated)** — the same space as
    the content stream — while PyMuPDF's are y-down page space. The old code
    applied PyMuPDF's y-down inverse (`inv`) to y-up rects, so 90°/270°
    annotations and links landed off-page (e.g. `[-190 10 -160 60]`), and links
    were transformed twice (annot loop + link loop). Now every non-link
    annotation `/Rect` and every link rect is transformed by `mat` (the `cm`
    matrix prefixed to the content) exactly once; links are skipped in the
    annot loop and handled by the PyMuPDF-style delete/insert loop.
  - When an annotation has a single `/AP /N` stream (`apn_bbox()` finite), its
    `/Matrix` is composed with `mat` so the appearance follows the content
    without regeneration (exact for any BBox/Matrix; state-dict `/N` is left
    alone — viewer maps BBox to the new `/Rect`).
  - `/MK /R` untouched: PyMuPDF 1.28.2 does not touch it either.
  - Return value unchanged (`inv = ~mat`, as PyMuPDF).
  - Import `PDF_ANNOT_LINK` from `pdfspine.constants`.
- `python/tests/test_longtail13.py::test_remove_rotation_rewrites_annot_rect`:
  expectation changed from `before * inv` (which encoded the off-page bug:
  `(-90, 10, -50, 50)`) to `before * ~inv`.
- `python/tests/test_page_edit_branches.py`: new
  `test_docpy_037_remove_rotation_rewrites_widget_rects[0/90/180/270]` — two
  widgets + a Square annot; asserts every annot/widget rect == `before * ~inv`,
  stays inside `page.rect`, and (0/90/180) matches the PyMuPDF 1.28.2 oracle
  within 0.5 pt. Targeted runs: 56 passed (both files), ruff format/check and
  mypy clean.
- `CHANGELOG.md` Unreleased `### Fixed` entry added.

## PyMuPDF 1.28.2 comparison (400×600 page, /Rotate r, text widget user-space
/Rect [20 540 120 570], checkbox [200 280 220 300])

| r | PyMuPDF widget /Rect after | pdfspine after (this branch) | visual |
|---|---|---|---|
| 0 | [20 540 120 570] | same | identical |
| 90 | [540 280 570 380] / [280 180 300 200] | same | identical |
| 180 | [280 30 380 60] / [180 300 200 320] | same | identical |
| 270 | [-170 -180 -140 -80] (off page, PyMuPDF bug; render shows widgets vanish) | [30 20 60 120] / [300 200 320 220] | pdfspine correct |

PyMuPDF mechanics: `widget.rect = r; widget.update()` where `widget.rect` is read
AFTER `set_rotation(0)`/`set_mediabox` in the new y-down page space, `r =
rect * inv`, then `pdf_set_annot_rect` + full AP regeneration (text ends up
upright in the tall box). Its y-down `inv` equals the user-space `mat` under
the y-flip only for 90°/180°, hence its 270° breakage. pdfspine keeps the
appearance instead of regenerating (AP `/Matrix ∘ mat`).

Render check (scratch script `visual_check.py`): pdfspine output saved before
and after `remove_rotation()`, rendered with real PyMuPDF at 72 dpi — 0 differing
pixels for all four angles (widgets with regenerated text AP, Square annot with
fill/stroke, link). pdfspine's own `get_pixmap` does not draw annotation
appearances, so it can only confirm the page content, which it does.

## Not done

1. Full gate not run: `cargo fmt --check`, `cargo clippy --workspace
   --all-features --all-targets -- -D warnings`, `cargo test --workspace
   --all-features` (Rust untouched, so these should be unaffected), full
   `python -m pytest -W error --doctest-modules python/pdfspine python/tests`,
   `scripts/test-order-guard.py`, `scripts/catalog-status-guard.py`,
   `scripts/compat-symbol-guard.py`, `scripts/manifest-lint.py`.
2. `docs/test-case-catalog.md` row `DOCPY-037` (line ~880) still says
   "rewrites links (90/180/270 + identity)"; extend the feature text to mention
   widget/annot rects (keep status `green`). `catalog-status-guard.py` may or
   may not require this — run it.
3. `docs/PRD-NEXT.md` §0 queue item 1 still lists this bug; strike the
   `remove_rotation` clause after merge.
4. Commit message is `wip(...)`; squash/reword to
   `fix(python): let remove_rotation transform widget rectangles` when merging.

## Next steps (commands, from the worktree root)

```
export CARGO_TARGET_DIR=/Volumes/Cargo/target/pdfspine-rot CARGO_BUILD_JOBS=4 TMPDIR=/Volumes/ExternalSSD/tmp
python3 -m venv .venv-task && .venv-task/bin/pip install "maturin>=1.12,<2" pytest hypothesis "ruff==0.14.14" mypy pymupdf
env -u CONDA_PREFIX PATH="$PWD/.venv-task/bin:$PATH" VIRTUAL_ENV="$PWD/.venv-task" maturin develop
.venv-task/bin/python -m pytest -W error --doctest-modules python/pdfspine python/tests -q
.venv-task/bin/python -m ruff format --check python/pdfspine python/tests scripts && .venv-task/bin/python -m ruff check python/pdfspine python/tests scripts && .venv-task/bin/python -m mypy
python3 scripts/test-order-guard.py && python3 scripts/catalog-status-guard.py && python3 scripts/compat-symbol-guard.py && python3 scripts/manifest-lint.py
cargo fmt --all --check && cargo clippy --workspace --all-features --all-targets -- -D warnings && cargo test --workspace --all-features
```

## Known risks

- Behavior change beyond the widget crash: non-widget annotation and link rects
  on 90°/270° pages now land where the content is (previously off-page), and
  `/AP /N /Matrix` is now composed for any annotation with a single AP stream.
  Any test that pinned the old `rect * inv` value would need the same update
  as `test_longtail13.py`.
- Widgets whose `/AP /N` is a state dictionary (real-world checkboxes/radios)
  only get `/Rect` moved; the viewer scales BBox into the new rect (a no-op for
  square boxes).
- pdfspine relies on `Page.annots()` including widget annotations; if that is
  ever changed for PyMuPDF parity, add an explicit widget loop via
  `page.load_annot(widget.xref)`.
