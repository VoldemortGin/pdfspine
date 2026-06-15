# oxide-pdf

**An Apache-2.0-licensed, pure-Rust reimplementation of [PyMuPDF](https://pymupdf.readthedocs.io/) (`fitz`), with PyO3 Python bindings.**

> Status: **early / work-in-progress (Milestone M0).** Today the wheel imports
> and the geometry layer is complete and tested; PDF parsing, text extraction,
> editing, and rendering are scheduled work (see [Roadmap](#roadmap)). It is not
> yet usable for real PDF work — watch the milestones.

---

## Why oxide-pdf?

PyMuPDF is excellent, but it is **AGPL-3.0** (or a commercial license from
Artifex). That licensing makes it a non-starter for many closed-source products,
SaaS backends, and permissively-licensed open-source projects.

oxide-pdf exists to provide a **drop-in-shaped, permissively-licensed (Apache-2.0)**
alternative:

- **Apache-2.0 throughout.** Every first-party crate is Apache-2.0 — a permissive
  license with an explicit patent grant — and the dependency graph is gated by
  `cargo-deny` to **exclude GPL / AGPL / LGPL / MPL / SSPL** from the shipped
  wheel. License cleanliness is a tested, CI-enforced property — not a promise.
- **Pure Rust, no C blob.** Self-contained wheels with no system `zlib`/C
  linkage and no bundled prebuilt engine (the differentiator vs pdfium-based
  wrappers).
- **`fitz` / `pymupdf` compatible surface.** A compatibility shim aims to let
  existing `import fitz` / `import pymupdf` code run unmodified for the supported
  subset, with a machine-readable `COMPAT.toml` documenting every deviation.
- **Memory-safe by construction.** `#![forbid(unsafe_code)]` in all first-party
  crates except the single audited PyO3 FFI chokepoint.

oxide-pdf is an independent **clean-room** project: no code, tests, or fixtures are
derived from MuPDF / PyMuPDF or any AGPL source.

## Architecture

oxide-pdf is a Cargo workspace with a strict dependency DAG: the Python bindings
touch exactly one façade crate, and core logic is split into independently
testable units.

```
                  py-bindings   (PyO3 cdylib -> oxide_pdf._core, abi3)
                       │   (depends on exactly one core crate)
                       ▼
                    pdf-api      facade / re-exports
        ┌──────────┬───┴────┬──────────┐
        ▼          ▼        ▼          ▼
    pdf-text   pdf-edit  pdf-image  pdf-render (future)
        │          │        │
        └────┬─────┘        │
             ▼              │
         pdf-fonts ◄────────┘
             ▼
         pdf-core   ◄────────  pdf-crypto   (core uses crypto behind the
                                             `encryption` feature)
```

| Crate | Responsibility |
|---|---|
| `pdf-core` | object model, lexer/parser, xref, filters, writer, **geometry** |
| `pdf-crypto` | Standard security handler (RC4 / AES-128 / AES-256) |
| `pdf-fonts` | font parsing for mapping (encodings / ToUnicode / CMap / widths) |
| `pdf-text` | content-stream text interpreter, `get_text`, search |
| `pdf-edit` | page ops, merge, annotations / forms, metadata / TOC, redaction |
| `pdf-image` | image-document support, image-XObject decode/encode, `Pixmap` |
| `pdf-render` | *(reserved)* vector rasterizer → `Pixmap` (post-v1) |
| `pdf-api` | unified ergonomic façade / re-exports |
| `py-bindings` | PyO3 wrappers → the `_core` extension module |

The Python side ships three packages: `oxide_pdf` (native, idiomatic), and the
`fitz` / `pymupdf` compatibility shims.

## Build & install (from source)

Requirements: Rust (pinned by `rust-toolchain.toml` to **1.96.0**), Python ≥
3.10, and [maturin](https://www.maturin.rs/) ≥ 1.12. [uv](https://docs.astral.sh/uv/)
is recommended for the virtualenv.

```bash
# Create an isolated environment and build + install the extension in-place.
uv venv .venv
source .venv/bin/activate
maturin develop

# Smoke-test the import.
python -c "import oxide_pdf; print(oxide_pdf.__version__)"
```

To build a redistributable wheel instead:

```bash
maturin build --release          # wheel lands in target/wheels/
```

## Develop / test

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pytest                           # Python smoke tests (after `maturin develop`)
```

oxide-pdf is built strictly **test-first** (red → green → refactor → harden). The
full per-function test plan lives in
[`docs/test-case-catalog.md`](docs/test-case-catalog.md); each test traces to a
spec clause or the PyMuPDF Tier-A documented contract.

## Roadmap

| Milestone | Scope |
|---|---|
| **M0** *(current)* | workspace + CI + TDD harness + **geometry** + stub wheel |
| M1 | PDF parse + object model + filters + crypto-read + repair |
| M2 | text extraction + search |
| M3 | save / incremental / GC + page ops + merge |
| M4 | annotations / forms / redaction |
| M5 | image documents + codecs + `Pixmap` + `fitz` shim hardening |
| M6 *(post-v1)* | vector page rendering (`get_pixmap` / SVG) |

The authoritative design — scope, priorities, milestone exit gates, and the
clean-room / licensing policy — is in [`PRD.md`](PRD.md).

## License

Apache-2.0. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). Third-party
dependency licenses (all permissive: MIT / Apache-2.0 / BSD / Zlib / …) are
bundled with each release; the shipped graph is verified free of copyleft
licenses by CI.
