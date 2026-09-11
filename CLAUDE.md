# pdfspine development guide

## Start here

- [docs/PRD-NEXT.md](docs/PRD-NEXT.md) §0 is the live restart entry, ordered
  backlog, and working rules. [PRD.md](PRD.md) preserves the original
  v1 scope/history. Read the current queue before choosing work.
- [CHANGELOG.md](CHANGELOG.md) separates released behavior from `[Unreleased]`.
  Use git status/log to establish the actual checkout state; this guide does
  not pin a development branch or test count.
- [docs/reading-order-root-cause.md](docs/reading-order-root-cause.md) records
  reading-order experiments and corpus acceptance bars; read it before changing
  layout ordering. [docs/pymupdf-compat-findings.md](docs/pymupdf-compat-findings.md)
  records intentional oracle differences.
- [docs/RELEASE-PYPI.md](docs/RELEASE-PYPI.md) is the release runbook.
  [docs/adr/0001-rust-pyo3-project-profile.md](docs/adr/0001-rust-pyo3-project-profile.md)
  describes the Rust/PyO3 project profile.

## Repository map

The Cargo workspace has 13 crates; [Cargo.toml](Cargo.toml) is authoritative.

| Crate | Responsibility |
|---|---|
| `crates/pdf-core` | PDF objects, parsing, document store, geometry, optional content |
| `crates/pdf-crypto` | Encryption and security handlers |
| `crates/pdf-fonts` | Font programs, encodings, metrics, bundled faces |
| `crates/pdf-text` | Content interpretation, text extraction, layout and tables |
| `crates/pdf-edit` | Document/page editing and PDF writing |
| `crates/pdf-image` | Image codecs, pixmaps and image-document support |
| `crates/pdf-markdown` | Markdown-to-PDF conversion |
| `crates/pdf-typeset` | Shared typesetting engine for DOCX/PPTX PDF export |
| `crates/pdf-render` | Page rasterization and SVG rendering |
| `crates/pdf-ocr` | OCR adapters and searchable-PDF support |
| `crates/pdf-api` | Public Rust facade joining the engine crates |
| `crates/py-bindings` | PyO3 extension, `pdfspine._core` |
| `crates/pdf-testdata` | Shared test data and fixture support |

- `python/pdfspine/`: Python package and compatibility shims;
  `python/tests/`: Python tests; each crate also has Rust tests.
- `conformance/`: oracle comparisons, benchmarks and ground-truth evaluation;
  `fixtures/`: test inputs. Large corpora and some reports are gitignored and
  may be absent in a new checkout.
- `scripts/`, `.github/workflows/`, `.githooks/`: gate, generation, CI and hooks.
- `fuzz/` is excluded from the stable workspace and uses the nightly fuzzing
  toolchain. It is separate from normal workspace tests.

## Gate and local environment

[rust-toolchain.toml](rust-toolchain.toml) pins Rust **1.96.0**, rustfmt and
clippy. [pyproject.toml](pyproject.toml) requires Python **3.12+** and configures
Maturin/PyO3. The maintained local `.venv` provides maturin, mypy, pytest and
Ruff **0.14.14**; use its tools to avoid formatter drift. `.venv-oracle` holds
real PyMuPDF for subprocess comparisons, separate from pdfspine's fitz shim.
These environments are local setup, not tracked repository files.

From the repository root on the maintained macOS checkout:

```sh
PATH="$PWD/.venv/bin:$HOME/.cargo/bin:$PATH" TMPDIR=/Volumes/ExternalSSD/tmp ./ci.sh
```

[ci.sh](ci.sh) delegates to [scripts/quality_gate.py](scripts/quality_gate.py).
The default phase order is `rust → extension → python → drift → artifacts`:
Rust formatting/clippy/tests/dependency policy; current-extension verification;
Python checks/tests; compatibility/provenance guards; release wheel/sdist
builds and clean-install smoke checks. `./ci.sh --help` lists phase selection.
The [pre-push hook](.githooks/pre-push) invokes the same complete gate when
`core.hooksPath` points to `.githooks`.

The extension phase fingerprints Rust/build inputs against
`.gate/extension.stamp` and rebuilds via `maturin develop --release` when needed.
Keep that check enabled locally after Rust changes: selecting the Python phase
also selects extension verification. Hosted CI uses
`PDFSPINE_GATE_SKIP_EXTENSION=1` after its own editable package installation;
that is a CI setup detail, not the default local command.

For the existing Python-format requirement before pushing Python changes:

```sh
.venv/bin/python -m ruff format --check python/pdfspine python/tests scripts
```

On the maintained machine, `target/` points to
`/Volumes/Cargo/target/pdfspine`; temporary builds and large evaluation evidence
belong on the external SSD. The `/Volumes/...` paths are machine-specific:
check that the volumes exist before using them. An isolated worktree does not
automatically inherit `.venv`, corpus fixtures, or the target symlink. Keep
concurrent builds and extension outputs isolated as needed to protect another
task's running gate. See PRD-NEXT §0 for the existing long-run gate guidance.

## Generated status and family routing

Per-symbol compatibility status comes from
[scripts/_compat_catalog.py](scripts/_compat_catalog.py); `COMPAT.toml` is
generated and should not be hand-edited. The compatibility guard is
[scripts/compat-symbol-guard.py](scripts/compat-symbol-guard.py). Follow the
live PRD working rules for per-item branches, completion records and evidence.

Family relationships, dependency directions and cross-repository work are in
[docs/spine-family.md](docs/spine-family.md), especially §6.15 and §7. Its source
of truth is `~/startup/spine/docs/spine-family.md`; the copy in this repository
is synchronized from there. Family-document edits follow the existing §7
source-and-sync procedure, rather than editing a single repository's copy.

When working in this family checkout, also read the family-root `CLAUDE.md`
**if it exists and is accessible**. It is currently absent; this guide does not
assume instructions in a missing file. Do not modify sibling repositories
outside the current task's scope; route needed changes through the family
backlog or an explicitly assigned task in that repository. This guide adds no
approval requirement or replacement for the user's session/AGENTS instructions.
