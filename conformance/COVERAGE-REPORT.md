# Test coverage report

Measured locally on **2026-09-05 (Pacific time)** at commit
`9da7ca6b8a61b6f24d2c6318bab5c412604a16ee` (`0.7.1`). The release is a
documentation-only change from `0.7.0`, so the product-code coverage is also
representative of that release.

There is no single reliable cross-language percentage. Python's `coverage.py`
cannot see native Rust execution, and the Rust CI coverage command does not run
the Python suite against an instrumented PyO3 extension. The two source sets are
therefore reported separately.

## Headline results

| Scope | Metric | Covered / total | Coverage |
|---|---:|---:|---:|
| Python package (`python/pdfspine/*.py`) | lines/statements | 4,384 / 5,344 | **82.04%** |
| Python package | branches | 955 / 1,498 | **63.75%** |
| Python package | coverage.py combined line + branch score | 5,339 / 6,842 | **78.03%** |
| Rust workspace, CI scope (all crates, all features) | lines | 30,400 / 41,883 | **72.58%** |
| Rust workspace, CI scope | functions | 2,991 / 4,517 | **66.22%** |
| Rust workspace, CI scope | regions | 54,096 / 74,408 | **72.70%** |
| Rust libraries excluding `py-bindings` | lines | 30,400 / 38,418 | **79.13%** |
| Rust libraries excluding `py-bindings` and `pdf-ocr` | lines | 29,923 / 37,888 | **78.98%** |
| `pdf-core` only | lines | 5,253 / 6,040 | **86.97%** |

The Rust workspace number includes `py-bindings` in the denominator. That crate
has 0 Rust unit tests and contributes 0 / 3,465 covered lines in this command.
Python tests do exercise the installed native extension, but that execution is
not visible to `cargo llvm-cov`; treating it as covered would be an unsupported
inference. `pdf-testdata` is a comment-only placeholder and emitted no
reportable source lines.

The Python combined score is coverage.py's weighted total over 5,344 statements
and 1,498 branches. It is included for reproducibility; the line percentage is
the usual answer when someone asks for line coverage.

## Combined Rust+Python profile (Python-driven instrumented extension)

Measured on **2026-09-05 (Pacific time)** at commit
`1f82655` — the commit after the `9da7ca6` headline measurement. The Python suite
is byte-for-byte identical here (814 passed / 63 skipped, line **82.04%**, branch
**63.75%**), so the two commits share one Python profile; only the Rust side is
re-measured below.

The caveat above — that Python's exercise of the native extension is invisible to
a cargo-only `cargo llvm-cov` run — is removed by instrumenting the PyO3 extension
and driving it from `pytest`. `cargo llvm-cov show-env` exports the LLVM
instrumentation environment; the Rust workspace tests and a `maturin`-built
`_core` both write into one profraw set (155 profraw files merged), which
`cargo llvm-cov report` combines. That attributes the binding and Python-facing
API lines to the tests that actually reach them:

| Crate | cargo-only lines | combined lines | combined line % | Δ (pp) |
|---|---:|---:|---:|---:|
| `pdf-api` | 701 / 3,215 (21.80%) | 2,408 / 3,215 | **74.90%** | +53.09 |
| `pdf-core` | 5,253 / 6,040 (86.97%) | 5,394 / 6,040 | 89.30% | +2.33 |
| `pdf-crypto` | 801 / 836 (95.81%) | 801 / 836 | 95.81% | +0.00 |
| `pdf-edit` | 6,519 / 8,729 (74.68%) | 7,652 / 8,729 | 87.66% | +12.98 |
| `pdf-fonts` | 2,194 / 2,434 (90.14%) | 2,206 / 2,434 | 90.63% | +0.49 |
| `pdf-image` | 2,193 / 2,732 (80.27%) | 2,218 / 2,732 | 81.19% | +0.92 |
| `pdf-markdown` | 1,480 / 1,579 (93.73%) | 1,480 / 1,579 | 93.73% | +0.00 |
| `pdf-ocr` | 477 / 530 (90.00%) | 485 / 530 | 91.51% | +1.51 |
| `pdf-render` | 2,988 / 3,739 (79.91%) | 2,974 / 3,739 | 79.54% | −0.37 |
| `pdf-text` | 5,281 / 5,773 (91.48%) | 5,407 / 5,773 | 93.66% | +2.18 |
| `pdf-typeset` | 2,513 / 2,811 (89.40%) | 2,513 / 2,811 | 89.40% | +0.00 |
| `py-bindings` | 0 / 3,465 (0.00%) | 2,908 / 3,465 | **83.92%** | +83.92 |
| **Workspace total** | 30,400 / 41,883 (72.58%) | **36,446 / 41,883** | **87.02%** | +14.44 |

The two crates the cargo-only profile could not see are the whole point:
`py-bindings` rises from 0 to **83.92%** and `pdf-api` from 21.80% to **74.90%**,
lifting workspace line coverage from 72.58% to **87.02%** over the same 41,883
instrumented lines. `pdf-render` dips 0.37 pp (14 lines) because the `maturin`
build does not link every example/harness path a pure `cargo test` build does;
that difference is within run-to-run instrumentation noise. `pdf-testdata`
remains a comment-only placeholder and emits no reportable lines.

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

## Rust coverage by crate

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
`fail_under` ratchet (77 — floor of the combined statement+branch total that
`coverage report` enforces under `branch = true`, minus one) is checked by
`coverage report`. OIDC token acquisition and artifact retention are confirmed by
run `33972346887`; the combined (non-degraded) LCOV and Codecov ingestion await
the first post-fix run (Codecov ingestion also needs the repository activated on
codecov.io, a one-time account action). Before this work the repository contained
no retained coverage artifact or trustworthy current percentage. The 89.3% figure
in `PARITY.md` and `COMPAT.toml` is PyMuPDF API implementation coverage, not test
coverage.
