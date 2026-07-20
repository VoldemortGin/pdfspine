# ADR 0001: Rust/PyO3 project-standard profile

- Status: Accepted
- Date: 2026-07-20
- Applies to: the `pdfspine` Python distribution and its Rust/PyO3 implementation

## Context

`pdfspine` is a Python package whose canonical PDF implementation is a Rust
workspace. PyO3 exposes the Rust façade as `pdfspine._core`, Maturin combines it
with the handwritten `python/pdfspine/` package, and the release is distributed
as abi3 wheels plus an sdist.

The general Python project standard assumes a pure-Python `src/` layout, Python
3.13 as the default floor, runtime instrumentation of Python-owned domain logic,
and Pydantic validation at Python-owned structured boundaries. Applying those
rules literally would either conflict with Maturin or duplicate validation and
domain logic already owned by Rust.

We therefore apply `rust-pyo3-project-standard` together with the parent Python
and Rust standards. Only the substitutions below are exceptions; Ruff, strict
typing, warnings-as-errors, artifact installation tests, documentation parity,
and the root quality gate remain mandatory.

## Decision

### Maturin source layout

- Substituted parent rule: Python packages must live under `src/<package>/`.
- Chosen behavior: retain `python/pdfspine/`.
- Evidence: `pyproject.toml` sets `build-backend = "maturin"`,
  `python-source = "python"`, and `module-name = "pdfspine._core"`; the package
  ships `py.typed`, public stubs, and installed-wheel import smoke tests.
- Affected files: `pyproject.toml`, `python/pdfspine/`, release and CI workflows.
- Revisit trigger: Maturin deprecates `python-source`, a built wheel exposes an
  unintended import package, or installed-artifact tests cannot validate the
  package without repository paths.

### Python floor and abi3 baseline

- Substituted parent rule: Python 3.13 is the minimum supported interpreter.
- Chosen behavior: support CPython 3.11 and newer with `abi3-py311`.
- Evidence: `Pixmap` exposes the zero-copy buffer protocol through stable-ABI
  slots available from CPython 3.11. Metadata, classifiers, CI, documentation,
  and wheel tags therefore use a 3.11 floor.
- Affected files: `pyproject.toml`, `crates/py-bindings/Cargo.toml`,
  `.github/workflows/ci.yml`, `.github/workflows/release.yml`, user docs.
- Revisit trigger: the buffer implementation no longer needs the 3.11 slots,
  PyO3 drops the baseline, or a deliberate breaking release raises the minimum
  Python version. Any change must update all compatibility surfaces together.

### Runtime type checking

- Substituted parent rule: apply Beartype to all Python callables.
- Chosen behavior: do not wrap native PyO3 callables merely for formal
  compliance. Handwritten Python and public stubs remain subject to strict
  static typing. Beartype is required if a Python module begins to own material
  domain policy, state transitions, or transformations rather than normalization
  and delegation.
- Evidence: parsing, rendering, editing, OCR, and error classification are
  canonical Rust operations; Python supplies protocols, path normalization,
  compatibility aliases, and ergonomic presentation helpers.
- Affected files: `crates/py-bindings/`, `python/pdfspine/`.
- Revisit trigger: a handwritten Python module becomes the canonical owner of a
  domain rule or maintains independent state beyond wrapper lifecycle state.

### Boundary validation and domain ownership

- Substituted parent rule: model every input boundary with Pydantic.
- Chosen behavior: Rust validates PDF/image bytes and native structures and maps
  failures into the documented `PdfError` hierarchy. The simple `argparse` CLI
  converts values directly into typed domain inputs. Pydantic remains required
  for any future Python-owned JSON, HTTP, configuration, subprocess-record, tool,
  or model-output boundary.
- Evidence: Rust is the canonical parser and transformation engine; duplicating
  its binary-format models in Python would create two conflicting contracts.
- Affected files: Rust core crates, `crates/py-bindings/`,
  `python/pdfspine/cli.py`, `python/pdfspine/document.py`.
- Revisit trigger: the Python layer starts accepting or emitting a structured
  process boundary independently of Rust, or an external service/configuration
  surface is added.

## Consequences

The project keeps its Maturin-compatible package shape, broad abi3 compatibility,
and one canonical Rust domain model. These exceptions do not waive any quality
gate. The root quality command, CI, pre-push hook, stubs, bundled LLM docs,
reference docs, and published site must continue to describe and test the same
installed public contract.
