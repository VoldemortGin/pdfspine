# Test coverage report

Measured locally on **2026-09-05 (Pacific time)** at commit `2b7df16` — the
targeted-test batch (`4149071` Rust tests, `2b7df16` Python tests) that
followed the `1f82655` combined profile. Product code is unchanged since the
`0.7.1` release (`9da7ca6`) apart from `13c25f2` (a `py-bindings` performance
change), so these numbers describe the released code under the enlarged suite.

There is no single reliable cross-language percentage. Python's `coverage.py`
cannot see native Rust execution, and a cargo-only `cargo llvm-cov` run cannot
see the Python suite. The Rust rows below therefore come from the combined
profile described in the next section (an instrumented PyO3 extension driven by
`pytest`, merged with the Rust workspace tests); the two source sets are still
reported separately.

## Headline results

| Scope | Metric | Covered / total | Coverage |
|---|---:|---:|---:|
| Python package (`python/pdfspine/*.py`) | lines/statements | 5,275 / 5,344 | **98.71%** |
| Python package | branches | 1,427 / 1,498 | **95.26%** |
| Python package | coverage.py combined line + branch score | 6,702 / 6,842 | **97.95%** |
| Rust workspace, combined profile (all crates, all features) | lines | 38,053 / 41,886 | **90.85%** |
| Rust workspace, combined profile | functions | 4,149 / 4,519 | **91.81%** |
| Rust workspace, combined profile | regions | 66,719 / 74,504 | **89.55%** |
| Rust libraries excluding `py-bindings` | lines | 34,852 / 38,418 | **90.72%** |
| Rust libraries excluding `py-bindings` and `pdf-ocr` | lines | 34,367 / 37,888 | **90.71%** |
| `pdf-core` only | lines | 5,431 / 6,040 | **89.92%** |

The Rust workspace number includes `py-bindings` in the denominator. That crate
has 0 Rust unit tests; it is counted (3,201 / 3,468 lines, 92.30%) only because
the profile is the combined one — a cargo-only run still reports it as 0 (see
the historical table below). `pdf-testdata` is a comment-only placeholder and
emitted no reportable source lines.

The Python combined score is coverage.py's weighted total over 5,344 statements
and 1,498 branches. It is the figure the `fail_under = 96` ratchet in
`pyproject.toml` compares against; the line percentage is the usual answer when
someone asks for line coverage.

## Combined Rust+Python profile (Python-driven instrumented extension)

Measured on **2026-09-05 (Pacific time)** at commit `2b7df16` (1,774 Rust
tests passed / 1 ignored; 1,059 Python tests passed / 66 skipped; 158 profraw
files merged). The "before" columns are the same profile re-measured at
`a45b66d`, the parent of the two test commits (`4149071`, `2b7df16`); between
the two only tests were added, so every delta is attributable to the new tests.
The earlier measurement at `1f82655` (workspace **87.02%**, `py-bindings`
83.92%, `pdf-api` 74.90%) agrees with the `a45b66d` column within
instrumentation noise and is kept in this file's history.

The caveat above — that Python's exercise of the native extension is invisible to
a cargo-only `cargo llvm-cov` run — is removed by instrumenting the PyO3 extension
and driving it from `pytest`. `cargo llvm-cov show-env` exports the LLVM
instrumentation environment; the Rust workspace tests and a `maturin`-built
`_core` both write into one profraw set, which `cargo llvm-cov report` combines.
That attributes the binding and Python-facing API lines to the tests that
actually reach them. A cargo-only run reports `py-bindings` at 0 / 3,468 and
`pdf-api` at about 22% (see the historical table below); the combined profile:

| Crate | combined lines @ `a45b66d` | combined lines @ `2b7df16` | line % @ `2b7df16` | Δ (pp) |
|---|---:|---:|---:|---:|
| `pdf-api` | 2,408 / 3,215 (74.90%) | 2,929 / 3,215 | **91.10%** | +16.20 |
| `pdf-core` | 5,401 / 6,040 (89.42%) | 5,431 / 6,040 | 89.92% | +0.50 |
| `pdf-crypto` | 801 / 836 (95.81%) | 801 / 836 | 95.81% | +0.00 |
| `pdf-edit` | 7,652 / 8,729 (87.66%) | 7,957 / 8,729 | **91.16%** | +3.50 |
| `pdf-fonts` | 2,206 / 2,434 (90.63%) | 2,206 / 2,434 | 90.63% | +0.00 |
| `pdf-image` | 2,217 / 2,732 (81.15%) | 2,212 / 2,732 | 80.97% | −0.18 |
| `pdf-markdown` | 1,480 / 1,579 (93.73%) | 1,480 / 1,579 | 93.73% | +0.00 |
| `pdf-ocr` | 485 / 530 (91.51%) | 485 / 530 | 91.51% | +0.00 |
| `pdf-render` | 2,975 / 3,739 (79.57%) | 3,394 / 3,739 | **90.77%** | +11.20 |
| `pdf-text` | 5,373 / 5,773 (93.07%) | 5,444 / 5,773 | 94.30% | +1.23 |
| `pdf-typeset` | 2,513 / 2,811 (89.40%) | 2,513 / 2,811 | 89.40% | +0.00 |
| `py-bindings` | 2,910 / 3,468 (83.91%) | 3,201 / 3,468 | **92.30%** | +8.39 |
| **Workspace total** | 36,421 / 41,886 (86.95%) | **38,053 / 41,886** | **90.85%** | +3.90 |

The test batch was aimed at the ten files with the most uncovered lines in the
`1f82655` profile (five Rust, five Python). Per-file result, same two runs
(Python from `coverage.py --branch`, Rust from the merged `cargo llvm-cov`
report):

| File | before | after | Δ (pp) | uncovered lines |
|---|---:|---:|---:|---:|
| `crates/pdf-api/src/lib.rs` | 1,301 / 1,862 (69.87%) | 1,810 / 1,862 (**97.21%**) | +27.34 | 561 → 52 |
| `crates/py-bindings/src/lib.rs` | 2,910 / 3,468 (83.91%) | 3,201 / 3,468 (**92.30%**) | +8.39 | 558 → 267 |
| `crates/pdf-render/src/type1.rs` | 582 / 892 (65.25%) | 847 / 892 (**94.96%**) | +29.71 | 310 → 45 |
| `crates/pdf-edit/src/redact.rs` | 633 / 898 (70.49%) | 869 / 898 (**96.77%**) | +26.28 | 265 → 29 |
| `crates/pdf-render/src/render.rs` | 533 / 775 (68.77%) | 681 / 775 (**87.87%**) | +19.10 | 242 → 94 |
| `python/pdfspine/document.py` | 2,038 / 2,433 (83.76%) | 2,414 / 2,433 (**99.22%**) | +15.45 | 395 → 19 |
| `python/pdfspine/_tatr.py` | 516 / 712 (72.47%) | 710 / 712 (**99.72%**) | +27.25 | 196 → 2 |
| `python/pdfspine/_tatr_postprocess.py` | 402 / 532 (75.56%) | 531 / 532 (**99.81%**) | +24.25 | 130 → 1 |
| `python/pdfspine/geometry.py` | 673 / 789 (85.30%) | 789 / 789 (**100.00%**) | +14.70 | 116 → 0 |
| `python/pdfspine/helpers.py` | 212 / 288 (73.61%) | 288 / 288 (**100.00%**) | +26.39 | 76 → 0 |
| Python package, lines | 4,384 / 5,344 (82.04%) | 5,275 / 5,344 (**98.71%**) | +16.67 | 960 → 69 |
| Python package, branches | 955 / 1,498 (63.75%) | 1,427 / 1,498 (**95.26%**) | +31.51 | 543 → 71 |
| Python package, coverage.py combined | 5,339 / 6,842 (78.03%) | 6,702 / 6,842 (**97.95%**) | +19.92 | 1,503 → 140 |

The remaining `py-bindings` lines are mostly error-mapping closures and
`PyErr` conversions that need malformed native state to reach; the remaining
`render.rs` lines are the Type0/CID `resolve_gid` and Type3 charproc paths,
which need composite-font fixtures and are left for the font roadmap items.
`pdf-image` moves by 5 lines (−0.18 pp) between the two runs with no change to
its tests; that is run-to-run instrumentation noise. `pdf-testdata` remains a
comment-only placeholder and emits no reportable lines.

Two product behaviours were documented by the new tests rather than changed
(tests only): `Page.remove_rotation()` raises `PdfUnsupportedError` on a rotated
page that carries form widgets, because it assigns `widget.rect` on existing
(read-only) widgets (`python/pdfspine/document.py:3428-3430`); and
`pdf-edit/src/redact.rs` re-emits the `'` and `"` show operators as bare `TJ`,
dropping the implicit line advance and spacing operands from the rewritten
stream (a fidelity issue; secret removal is unaffected).

### Reproduction (combined profile)

From the repository root, with the pinned 1.96.0 toolchain, `cargo-llvm-cov`
0.8.5, and the project `.venv` (Python 3.12 + `maturin`). A scratch
`CARGO_TARGET_DIR` keeps every build artifact out of the repo `target/`; the lean
`CARGO_*_DEBUG=0` / `CARGO_INCREMENTAL=0` flags do not affect line coverage (the
coverage map lives in `covmap`/`covfun`, not DWARF):

```bash
env -u CONDA_PREFIX \
  VIRTUAL_ENV="$PWD/.venv" PATH="$PWD/.venv/bin:$PATH" \
  RUSTC="$HOME/.rustup/toolchains/1.96.0-aarch64-apple-darwin/bin/rustc" \
  CARGO_TARGET_DIR=/tmp/pdfspine-combined-cov/target \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  bash -eo pipefail -c '
    # capture-then-source: `source <(...)` breaks under `set -u`.
    cargo llvm-cov show-env --export-prefix > /tmp/pdfspine-cov-env.sh
    source /tmp/pdfspine-cov-env.sh                      # RUSTC_WRAPPER + LLVM_PROFILE_FILE
    cargo llvm-cov clean --workspace
    cargo test --workspace --all-features                # Rust test profraw
    maturin develop                                      # instrumented _core.abi3.so (DEBUG)
    COVERAGE_FILE=/tmp/pdfspine-combined.coverage \
      python -m coverage run --branch --source=python/pdfspine \
      -m pytest -W error --doctest-modules python/pdfspine python/tests
    cargo llvm-cov report --lcov --output-path /tmp/pdfspine-combined-lcov.info
    cargo llvm-cov report --json --output-path /tmp/pdfspine-combined-cov.json
  '
```

Verify success by the signal that matters — `py-bindings` line coverage is
non-zero in the combined report (it is 0 in the cargo-only profile). Rebuild the
normal extension afterwards (`maturin develop --release`, or `pip install -e .`)
so the working tree is not left with an instrumented `.so`.

## Rust coverage by crate (historical cargo-only profile at `9da7ca6`)

This is the cargo-only scope measured at the `0.7.1` release commit, kept for
comparison with the combined profile above; it does not see the Python suite.

| Crate | Covered / total lines | Line coverage | Covered / total functions | Function coverage |
|---|---:|---:|---:|---:|
| `pdf-api` | 701 / 3,215 | 21.80% | 114 / 500 | 22.80% |
| `pdf-core` | 5,253 / 6,040 | 86.97% | 610 / 727 | 83.91% |
| `pdf-crypto` | 801 / 836 | 95.81% | 65 / 65 | 100.00% |
| `pdf-edit` | 6,519 / 8,729 | 74.68% | 650 / 879 | 73.95% |
| `pdf-fonts` | 2,194 / 2,434 | 90.14% | 247 / 274 | 90.15% |
| `pdf-image` | 2,193 / 2,732 | 80.27% | 215 / 261 | 82.38% |
| `pdf-markdown` | 1,480 / 1,579 | 93.73% | 92 / 94 | 97.87% |
| `pdf-ocr` | 477 / 530 | 90.00% | 37 / 40 | 92.50% |
| `pdf-render` | 2,988 / 3,739 | 79.91% | 270 / 310 | 87.10% |
| `pdf-text` | 5,281 / 5,773 | 91.48% | 486 / 511 | 95.11% |
| `pdf-typeset` | 2,513 / 2,811 | 89.40% | 205 / 219 | 93.61% |
| `py-bindings` | 0 / 3,465 | 0.00% | 0 / 637 | 0.00% |

## Reproduction

Python 3.12.11, coverage.py 7.16.0:

```bash
env -u CONDA_PREFIX \
  VIRTUAL_ENV="$PWD/.venv" PATH="$PWD/.venv/bin:$PATH" \
  COVERAGE_FILE=/tmp/pdfspine-python-9da7ca6-full.coverage \
  .venv/bin/python -m coverage run --branch --source=python/pdfspine \
  -m pytest -W error --doctest-modules python/pdfspine python/tests
env -u CONDA_PREFIX \
  COVERAGE_FILE=/tmp/pdfspine-python-9da7ca6-full.coverage \
  .venv/bin/python -m coverage json \
  -o /tmp/pdfspine-python-9da7ca6-full-coverage.json
```

Result: 814 passed, 63 skipped. The source scope includes every `.py` module in
`python/pdfspine`; it excludes native `_core.abi3.so`, stubs, tests, scripts,
and conformance tools. Two lines were excluded by coverage pragmas.

Rust 1.96.0, cargo-llvm-cov 0.8.5:

```bash
env -u CONDA_PREFIX \
  PATH="/Users/linhan/.rustup/toolchains/1.96.0-aarch64-apple-darwin/bin:$PATH" \
  RUSTC=/Users/linhan/.rustup/toolchains/1.96.0-aarch64-apple-darwin/bin/rustc \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  CARGO_TARGET_DIR=/tmp/pdfspine-rust-cov-9da7ca6-target \
  cargo llvm-cov --workspace --all-features --json \
  --output-path /tmp/pdfspine-rust-9da7ca6-coverage.json

env -u CONDA_PREFIX \
  PATH="/Users/linhan/.rustup/toolchains/1.96.0-aarch64-apple-darwin/bin:$PATH" \
  RUSTC=/Users/linhan/.rustup/toolchains/1.96.0-aarch64-apple-darwin/bin/rustc \
  CARGO_TARGET_DIR=/tmp/pdfspine-rust-cov-9da7ca6-target \
  cargo llvm-cov report --lcov \
  --output-path /tmp/pdfspine-rust-9da7ca6-lcov.info
```

Result: 1,702 passed, 1 ignored. This is the repository CI coverage scope:
workspace tests, all features, and first-party Rust source under `crates/*/src`.
Dependencies, fuzz targets, examples, Python tests, and pure-Python modules are
outside this Rust report. Reducing debug information and disabling incremental
compilation only reduced temporary artifact size; coverage instrumentation
remained enabled. LLVM branch coverage was not enabled, so the Rust report has
no branch percentage (branch count is zero).

Raw reports are intentionally kept outside the repository:

- `/tmp/pdfspine-python-9da7ca6-full-coverage.json`
- `/tmp/pdfspine-rust-9da7ca6-coverage.json`
- `/tmp/pdfspine-rust-9da7ca6-lcov.info`

The checked-in CI `coverage` job builds this combined Rust+Python profile rather
than the cargo-only scope: inside a job-local venv it sources the `cargo llvm-cov`
instrumentation, builds the instrumented `_core` with `maturin develop`, and lets
`pytest` drive it. The extension must be a **debug** build: `maturin develop`
places `_core` in the target's `debug/` dir, which `cargo llvm-cov report` scans
for objects, so the pytest-run profraw is merged; a release `pip install -e .`
build lands in `release/`, is absent from report's `-object` list, and its
counts are silently dropped — degrading the profile to cargo-only. The first CI
run of an earlier `pip install -e .` variant (run `33972346887`) hit exactly
that: it acquired an OIDC token and retained the artifact, but the LCOV showed
`py-bindings` 0/3,465. The job now uses `maturin develop` and an `awk` guard that
fails the step loudly if `py-bindings` has zero covered lines, so the regression
cannot pass silently. The job retains `lcov.info`, `coverage-python.xml`, and
`coverage-python.json` as a 90-day `coverage-reports` artifact, and uploads to
Codecov tokenlessly via OIDC (`use_oidc: true` with job-level `id-token: write`),
split into `rust` and `python` flags — replacing the earlier upload rejected for
lack of a token (the public repo has no `CODECOV_TOKEN` secret). A Python
`fail_under` ratchet (96 — floor of the 97.95% combined statement+branch total
that `coverage report` enforces under `branch = true`, minus one; it was 77 at
`1f82655`) is checked by `coverage report`. OIDC token acquisition and artifact retention are confirmed by
run `33972346887`; the combined (non-degraded) LCOV and Codecov ingestion await
the first post-fix run (Codecov ingestion also needs the repository activated on
codecov.io, a one-time account action). Before this work the repository contained
no retained coverage artifact or trustworthy current percentage. The 89.3% figure
in `PARITY.md` and `COMPAT.toml` is PyMuPDF API implementation coverage, not test
coverage.
