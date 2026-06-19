# Installation

!!! warning "Not yet on PyPI"
    pdfspine is pre-1.0 and is **not yet published to PyPI**. The
    `pip install pdfspine` command below is the *planned* path once the first
    release ships. For now, build from source with
    [maturin](https://www.maturin.rs/).

## Requirements

- **Python ≥ 3.11.** The 3.11 floor is required by the `Pixmap` zero-copy buffer
  protocol (the stable-ABI buffer slots landed in CPython 3.11). The wheel is an
  `abi3` wheel, so a single build covers 3.11 and newer.
- **Rust** (pinned by `rust-toolchain.toml`) and **maturin ≥ 1.12** — only needed
  to build from source.
- [uv](https://docs.astral.sh/uv/) is recommended for managing the virtualenv,
  but any virtualenv tool works.

## Install from PyPI (planned)

Once published, installation will be the usual:

```bash
pip install pdfspine
```

This provides the `pdfspine` (native) package from one wheel, plus the opt-in
`pdfspine.fitz` / `pdfspine.pymupdf` compatibility submodules. By default it does
**not** claim the global `fitz` / `pymupdf` import names, so it is collision-safe
alongside a real PyMuPDF; call `pdfspine.install_fitz_shim()` to register them.

## Build from source (today)

Clone the repository and build the extension in place:

```bash
# Create an isolated environment.
uv venv .venv
source .venv/bin/activate          # Windows: .venv\Scripts\activate

# Build the Rust extension and install it into the environment.
maturin develop --release
```

Then smoke-test the import:

```bash
python -c "import pdfspine; print(pdfspine.__version__)"
```

To build a redistributable wheel instead of installing in place:

```bash
maturin build --release            # wheel lands in target/wheels/
pip install target/wheels/pdfspine-*.whl
```

## Verify

```python
import pdfspine

print(pdfspine.__version__)
print(pdfspine.version)           # version tuple from the Rust core

# The opt-in fitz compat shim works too:
import pdfspine.fitz as fitz
print(fitz.pymupdf_version)        # the PyMuPDF baseline this shim targets
```
