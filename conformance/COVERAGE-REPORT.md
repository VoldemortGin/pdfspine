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

The checked-in CI job generates the same Rust LCOV scope, but its 2026-09-05
Codecov upload for this commit was rejected because no upload token was
provided. Before this measurement, the repository contained no retained test
coverage artifact or trustworthy current percentage. The 89.3% figure in
`PARITY.md` and `COMPAT.toml` is PyMuPDF API implementation coverage, not test
coverage.
