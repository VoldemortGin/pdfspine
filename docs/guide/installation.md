# Installation

!!! warning "Alpha status"
    pdfspine is on PyPI but remains pre-1.0. Verify output on your own documents
    before relying on it in production.

## Requirements

- **Python ≥ 3.12** (`requires-python = ">=3.12"`). The wheel is an `abi3`
  (`abi3-py311`) wheel — the 3.11 ABI floor comes from the `Pixmap` zero-copy
  buffer protocol (the stable-ABI buffer slots landed in CPython 3.11) — so a
  single build covers every supported interpreter; the 3.12 install floor is
  set by the package metadata.
- **Rust** (pinned by `rust-toolchain.toml`) and **maturin ≥ 1.12** — only needed
  to build from source.
- [uv](https://docs.astral.sh/uv/) is recommended for managing the virtualenv,
  but any virtualenv tool works.

## Install from PyPI

pdfspine is published on PyPI:

```bash
pip install pdfspine
```

This provides the `pdfspine` (native) package from one wheel, plus the opt-in
`pdfspine.fitz` / `pdfspine.pymupdf` compatibility submodules. By default it does
**not** claim the global `fitz` / `pymupdf` import names, so it is collision-safe
alongside a real PyMuPDF; call `pdfspine.install_fitz_shim()` to register them.
OCR works out of the box: the PP-OCRv5 weights come from the shared
`ocrspine-models` data package, installed automatically as a runtime dependency.

### Optional Table Transformer backend

The TATR vision backend is intentionally not part of the base install:

```bash
pip install "pdfspine[tatr]"
```

It adds Torch, Transformers, and Pillow. Model weights remain separate and are
loaded from the local Hugging Face cache (or `PDFSPINE_TATR_MODELS`) at runtime;
see [Tables](../reference/tables.md#vision-table-transformer) for pinned download
commands. This keeps a normal pdfspine wheel lightweight and offline-safe.

The optional TATR runtime is supported on **CPython 3.12–3.14** for Linux
(glibc/manylinux 2.28+, x86_64 or aarch64), macOS on Apple silicon, and Windows
x86_64. It is not offered on Intel macOS, musl/Alpine Linux, Windows ARM64, or
Python 3.15+. Those limits come from the available PyTorch wheels and do not
restrict the base `pdfspine` package. Dependency markers skip the ML packages on
unsupported Python, OS, and CPU combinations, so those environments can still
install base pdfspine. PEP 508 has no standard libc marker, however: on
musl/Alpine, resolving `pdfspine[tatr]` may fail because PyTorch publishes glibc
wheels. Use the base install there, or preinstall a separately validated
source-built Torch stack that satisfies the extra; requesting vision without a
usable runtime raises `PdfUnsupportedError`.

!!! note "Linux install size"
    PyPI's standard Linux Torch distribution may also install CUDA runtime
    packages even when pdfspine will run TATR on CPU. Follow PyTorch's official
    CPU-only installation instructions first if download or environment size is
    important; then install `pdfspine[tatr]`.

## Build from source

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
