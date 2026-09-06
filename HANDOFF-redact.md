# HANDOFF — redact.rs `'` / `"` operator fix (2026-09-05)

Branch `worktree-agent-ac5d2a0ee9c5053bd`, based on main `21d636a`. Fixes
`docs/PRD-NEXT.md` §0 queue item 1, third bullet (`redact.rs` rewrote `'` and `"`
as a bare `TJ`).

## Done

- **Fix** `crates/pdf-edit/src/redact.rs` (`apply_op`, `'` and `"` arms, ~L363–395):
  `'` now emits an explicit `T*` before the rewritten show; `"` emits
  `aw Tw`, `ac Tc`, `T*` (via `emit_op` on the operand slices) before the show.
  The walk state was already updated correctly — only the emission was missing.
  Works on the mapped path (`rewrite_show` → `TJ` / nothing) and the
  unmappable-font path (`emit_tj_literal` → `Tj`). Tokenizer needs no change:
  `'` / `"` are regular chars, so they lex as `Keyword::Other` and the operands
  (1 for `'`, 3 for `"`) accumulate in `ops` as for every operator.
- **Rust e2e** `crates/pdf-edit/tests/redact_edge_e2e.rs`: `REDACT-TEXT-010..013`
  (`'` line advance; `"` spacing inherited by a later `'`; mixed `Tj`/`'`/`"` with
  an entirely dropped `'` line; `'`/`"` under a missing font keep the expansion).
  Helpers `word_origin` / `assert_origin` / `assert_unshifted` / `count` compare
  pre- vs post-redaction glyph origins through `pdf_text::interpret_page`.
  Canary: all four fail on the unfixed source, pass on the fixed one.
- **Python oracle test** `python/tests/test_m4.py::test_pym4_redact_003_…`
  (`PYM4-REDACT-003`): pdfspine redacts the `'`/`"` page; real PyMuPDF redacts the
  explicit `T*`/`Tw`/`Tc`/`Tj` spelling of the same page; both outputs are read and
  rendered by real PyMuPDF in a subprocess. Survivor word boxes agree within
  0.5 pt (measured Δ = 0.0 on every edge), windowed SSIM = 0.9993 (≥ 0.99 gate).
  Also checks pdfspine's own reader: survivors keep their pre-redaction boxes.
  Oracle discovery: `.venv-oracle/{bin/python,Scripts/python.exe}` next to the repo,
  else `sys.executable` if a fresh process imports a `pymupdf` that does not pull in
  `pdfspine` (skip otherwise). Canary: fails on the unfixed extension
  (`'ONEKEEP' != 'ONE'`, lines collapsed onto line one).
- Catalog rows added (`docs/test-case-catalog.md`), CHANGELOG Unreleased
  `### Fixed` entry added.

## PyMuPDF 对拍 conclusion (important)

Real PyMuPDF 1.28.2 (MuPDF 1.27) **mishandles `'` / `"` itself** in
`apply_redactions`: its filter rewrites `'` → `T* … Tj` and `"` → `Tc Tw T* … Tj`
(the same expansion this fix uses) **but drops the leading** (`14 TL` vanishes;
`0 -14 TD` is rewritten to `72 700 TD`), so lines 2–4 collapse onto line 1's
baseline and the secret under the rect is not even removed — this happens even
when the redaction rect touches nothing. With explicit `T*` MuPDF is correct and
its output matches pdfspine's to 3 decimals. Hence the oracle test compares
against MuPDF's redaction of the explicit spelling, not of the `'`/`"` page.
Probe scripts (not committed): scratchpad `oracle_probe.py`, `oracle_variants.py`.

## Gates run (all green unless noted)

- `cargo fmt --check` ✓; `cargo clippy --workspace --all-features --all-targets -- -D warnings` ✓
- `cargo test -p pdf-edit --test redact_e2e --test redact_edge_e2e` ✓ (16 + 17)
- `python -m pytest -W error python/tests/test_m4.py` ✓ (17 passed, oracle test runs
  in the task venv with real pymupdf)
- `ruff format --check python/pdfspine python/tests scripts` ✓, `ruff check` on
  test_m4.py ✓
- four guards (`test-order-guard`, `catalog-status-guard`, `compat-symbol-guard`,
  `manifest-lint`) ✓
- **Pending / not confirmed before the quota cut-off**: the *full*
  `cargo test --workspace --all-features` and the *full*
  `python -m pytest -W error --doctest-modules python/pdfspine python/tests` were
  started in the background; only the redact suites and test_m4 were seen green.
  Nothing outside `redact.rs` changed, so a regression elsewhere is unlikely.

## Next steps

```bash
cd /Users/linhan/startup/spine/pdfspine/.claude/worktrees/agent-ac5d2a0ee9c5053bd
export CARGO_TARGET_DIR=/Volumes/Cargo/target/pdfspine-redact CARGO_BUILD_JOBS=4 TMPDIR=/Volumes/ExternalSSD/tmp
cargo test --workspace --all-features
python3 -m venv .venv-task && .venv-task/bin/pip install "maturin>=1.12,<2" pytest hypothesis "ruff==0.14.14" pymupdf
unset CONDA_PREFIX; VIRTUAL_ENV=$PWD/.venv-task PATH=$PWD/.venv-task/bin:$PATH maturin develop
.venv-task/bin/python -m pytest -W error --doctest-modules python/pdfspine python/tests
# then merge the branch into main, `maturin develop --release` in .venv, push.
```

## Known risks

- `"` with malformed operands (fewer than 2 numbers) emits only the operators whose
  operand exists, then `T*` and the show — a best-effort expansion, same as before
  for the state update.
- The content rewrite still converts every kept `Tj` / `'` / `"` run into `TJ`
  (pre-existing behaviour); byte-level preservation of untouched runs was not a goal.
- The oracle test spawns one subprocess for the probe even where it will skip.
