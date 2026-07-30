# pdfspine — PyPI release runbook

> **Audience:** the maintainer (or an automated agent) publishing a
> `pdfspine` release. The package and repository are already public. Each step is
> tagged:
>
> - **DONE** — already applied in the repo; nothing to do.
> - **READY** — code/config is in place; run the listed command(s) when you get here.
> - **USER-GATED** — needs your account, credentials, or an explicit decision
>   (PyPI ownership, making the repo public, the local folder rename).
>
> Do the steps **in order**. The local-folder rename (§C) is deliberately the
> **last build-environment step** before publishing, because it invalidates the
> current `.venv` and absolute paths.

---

## 0. Project facts (do not re-derive)

| Fact | Value | Status |
|---|---|---|
| PyPI distribution name | **`pdfspine`** | PUBLISHED |
| crates.io name | **`pdfspine`** (reserved for brand protection) | NOT PUBLISHED (Python-first, like ragspine; all crates `publish=false`) |
| Python import package | **`pdfspine`** (+ opt-in `pdfspine.fitz` / `pdfspine.pymupdf` compat shims via `install_fitz_shim()`) | DONE |
| License | **Apache-2.0** (`LICENSE` + `NOTICE` + per-data `PROVENANCE.md`) | DONE |
| GitHub repo | `github.com/VoldemortGin/pdfspine` — **public** | DONE |
| Build backend | **maturin** (PyO3 compiled Rust extension `pdfspine._core`) | DONE |
| ABI | **abi3-py311** → ONE wheel per (OS, arch), CPython **≥ 3.12** (floor set by `requires-python`) | DONE |
| `requires-python` | `>=3.12` | DONE |
| Native build deps | pure-Rust codecs/crypto, **BUT** the OCR `tract` kernels compile per-arch **assembly** → a C/asm toolchain is needed to *build from source* (GH runners + maturin-action containers already have it) | DONE (documented in README) |
| OCR delivery | `pdfspine` wheel contains the engine but not weights; the shared base dependency **`ocrspine-models`** supplies weights for the whole spine family | DONE (§D.1) |
| Optional extras | `[ocr]` and `[all]` are compatibility no-ops; a bare install is full-OCR-capable | DONE |
| PyPI project released here | **`pdfspine`** (`ocrspine-models` is released from the ocrspine repository) | PUBLISHED |

---

## A. Final gate (run on the release commit) — READY

Run from the repo root; all must be green before tagging.

```bash
./ci.sh
```

CI (`.github/workflows/ci.yml`) mirrors the quality gate and tests the supported
Python 3.12–3.14 range on Linux, macOS, and Windows. Confirm the release commit
is green before tagging.

---

## B. Docs + version final pass

Already applied in this audit pass (**DONE**):

- `pyproject.toml` classifiers: `Development Status :: 3 - Alpha`,
  `Intended Audience :: Developers`, OS classifiers (OS-Independent + Linux/MacOS/
  Windows), per-minor Python (3.12/3.13/3.14), extra Topic classifiers.
- `pyproject.toml` `project.urls` → `VoldemortGin/pdfspine`, and `Cargo.toml`
  `[workspace.package] repository` aligned to the same URL.
- `NOTICE` + `crates/pdf-ocr/models/PROVENANCE.md` now attribute the bundled
  PaddleOCR PP-OCR models (Apache-2.0, PaddlePaddle Authors).
- `README.md`: coverage **84.1% (647/769)**, test counts **1349/593**, OCR moved
  out of "planned", Accuracy section rewritten (fitz-parity text + beats-fitz
  Arabic + render near-parity/1.74× + PaddleOCR>fitz CJK), source-build C/asm
  toolchain note.
- `docs/index.md`, `docs/guide/migrating-from-pymupdf.md`, `PARITY.md`: coverage
  tables regenerated from `COMPAT.toml [meta]` → 647 / 56 / 66 / 769 / 84.1%.

The workspace Cargo version is the canonical source. Tagged builds run
`scripts/set_version_from_tag.py` before building, and the release must include
the corresponding changelog entry and synchronized current-state docs.

---

## D. Build wheels (CI matrix) — READY

The release workflow is in place: **`.github/workflows/release.yml`** builds the
full abi3 matrix + sdist and publishes via Trusted Publishing.

Matrix produced (one abi3 wheel each): linux `x86_64` (manylinux auto) + `aarch64`
(manylinux 2_28), macOS `x86_64` (macos-13) + `aarch64` (macos-14), Windows `x64`,
plus the sdist. All built `--strip`. maturin-action's containers carry the C/asm
toolchain the OCR `tract` kernels need.

- **Dry-run / TestPyPI:** GitHub → Actions → `release` → **Run workflow**, leave
  input `testpypi`.
- **Real release:** push a `v*` tag (§G) — builds + publishes to PyPI.

Local sanity build (optional, before tagging):

```bash
maturin build --release --out dist --strip
maturin sdist --out dist
python -m twine check dist/*           # README renders, metadata valid
python -m zipfile -l dist/pdfspine-*.whl | head    # inspect wheel contents
```

---

### D.1 OCR delivery — shared model distribution

The `pdfspine` wheel has the PaddleOCR engine compiled in but does not embed the
ONNX weights. The shared `ocrspine-models` base dependency supplies one copy of
the weights for pdfspine, docspine, and pptspine.

| Distribution | What | How it is built |
|---|---|---|
| **`pdfspine`** | the engine wheel — OCR *code* compiled in (`[tool.maturin] features = ["pyo3/abi3-py311", "ocr"]`), models **NOT** embedded | the `wheels` + `sdist` jobs (maturin) |
| **`ocrspine-models`** | shared pure-data `py3-none-any` wheel (import package `ocrspine_models`) | released from the sibling `ocrspine` repository |

`packages/pdfspine-ocr-models/` is an archived legacy companion retained for
compatibility archaeology; the release workflow must not build or publish it.

**End-user UX:**

```bash
pip install pdfspine          # engine + shared models dependency; OCR works offline
pip install pdfspine[ocr]     # equivalent compatibility spelling
```

**Runtime model resolution order** (`python/pdfspine/document.py`
`_ensure_ocr_models_env` sets the env, then the Rust `models_dir()` in
`crates/pdf-ocr/src/paddle/model.rs` resolves it):

1. **`PDFSPINE_OCR_MODELS`** env var, if already set — explicit user override;
2. else the installed **`ocrspine_models`** shared data package (default);
3. else the legacy **`pdfspine_ocr_models`** companion (backward compatibility);
4. else the in-repo **`ocrspine/models`** dev fallback (source checkout);
5. else a clear **`PdfUnsupportedError`** pointing at `pip install pdfspine`.

---

> **Note on the `fitz`/`pymupdf` compat shims — RESOLVED (opt-in, option C).**
> The shims now ship as **submodules of the package** (`pdfspine.fitz` /
> `pdfspine.pymupdf`), not as top-level packages. A default install therefore
> does **not** claim the global `fitz` / `pymupdf` import names, so it is
> collision-safe alongside a real PyMuPDF in the same environment — this is **no
> longer a go-live blocker**. The drop-in is one step away: `import pdfspine.fitz
> as fitz`, or `pdfspine.install_fitz_shim()` (idempotent, `setdefault`-based, so
> it never clobbers a real PyMuPDF) to make the literal `import fitz` resolve to
> the shim.

---

## E. Test-install the wheels — READY

The release workflow installs every natively runnable wheel and performs HTML
export plus real OCR smoke tests before publish. It also installs and imports
the sdist on Python 3.12. Verify the TestPyPI dry run as an additional check:

```bash
python -m venv /tmp/v && . /tmp/v/bin/activate
pip install --pre --index-url https://test.pypi.org/simple/ \
    --extra-index-url https://pypi.org/simple/ pdfspine
python -c "import pdfspine; print('ok', pdfspine.__version__); print(pdfspine.open)"
pip install pytest && pytest python/tests -q   # optional, against the sdist tree
```

Verify each platform wheel imports (the CI `wheels` smoke job in `ci.yml` already
does a `--no-index` install + `import pdfspine` per OS on every push).

---

## F. Publish — USER-GATED

### F.1 PyPI publishing

The `pdfspine` PyPI project already exists. Configure its publisher for this
repository and `release.yml`; `ocrspine-models` has an independent release
workflow in the ocrspine repository.

1. **https://pypi.org** → account → **Publishing** → **Add a pending publisher**
   - Project name: `pdfspine`
   - Owner: `VoldemortGin`
   - Repository: `pdfspine`
   - Workflow: `release.yml`
   - Environment: `pypi`
2. Repeat on **https://test.pypi.org** with Environment `testpypi` (dry run).
3. GitHub repo → Settings → Environments → create `pypi` and `testpypi`
   (optionally add a required reviewer on `pypi`).

Then publishing is automatic on tag push (§G) — `pypa/gh-action-pypi-publish`
uploads via OIDC and attaches PEP 740 build attestations. No tokens stored.

### F.2 crates.io — NOT published (Python-first, matches the spine family)

**Decision (2026-06-19): pdfspine ships via PyPI only, like its sibling `ragspine`
(pure Python, no crates.io).** The `pdfspine` name is **reserved on crates.io for
brand protection but nothing is published there.** ALL workspace crates are now
`publish = false` (pdf-core/pdf-api/pdf-crypto/pdf-edit/pdf-fonts/pdf-image/
pdf-ocr/pdf-render/pdf-text + py-bindings + pdf-testdata) — this prevents the
internal `pdf-*` crates from ever being accidentally published under fragmented
names, keeping the brand a single unified `pdfspine`.

This is NOT a release blocker — it is a deliberate non-action. If, in the future,
Rust developers want to depend on the engine directly, publishing to crates.io
would mean: name a public-facing crate `pdfspine` (a thin re-export of `pdf-api`,
NEVER `pdf-spine`), flip the whole dependency tree's `publish` back on, add
version-deps, and `cargo publish` each — a deliberate future effort, not part of
the v1 go-live. For v1: ignore crates.io beyond holding the reserved name.

---

## G. Git tag + flip repo public + push — USER-GATED

1. Land all changes on `main`; confirm CI green on that commit.
2. Tag and push (triggers `release.yml` → builds + publishes to PyPI):
   ```bash
   git tag v0.1.0a1
   git push origin v0.1.0a1
   ```
3. **Flip the GitHub repo public** (`VoldemortGin/pdfspine` → Settings → General →
   Change visibility → Public). Trusted Publishing works with private repos too,
   but an OSS package should have a public source link.
4. (Optional) `gh release create v0.1.0a1 --generate-notes`.

---

## H. Post-publish verification — READY

```bash
python -m venv /tmp/w && . /tmp/w/bin/activate
pip install --pre pdfspine          # drop --pre once you ship a non-alpha
python -c "import pdfspine; print(pdfspine.__version__); print(pdfspine.open)"
```

- https://pypi.org/project/pdfspine/ renders the README, shows Apache-2.0, links,
  and all platform wheels + sdist.
- crates.io: name `pdfspine` held (reserved) but NOT published for v1 — by design (Python-first).

---

## Status summary

| Step | What | Status |
|---|---|---|
| A | Final gate (fmt/clippy/test/deny/pytest/guards) | READY |
| B | Docs accuracy + classifiers + URLs + NOTICE/PROVENANCE | DONE |
| B | Version bump `0.0.0` → `0.1.0a1` (+ optional CHANGELOG/community files) | USER-GATED |
| D | Build wheel matrix + sdist (`release.yml`) | READY |
| D.1 | OCR = engine in `pdfspine` + shared base dependency `ocrspine-models` | READY |
| E | Test-install wheels (TestPyPI dry run) | READY |
| F.1 | Publish the `pdfspine` project | USER-GATED |
| F.2 | crates.io — name reserved, NOT published (Python-first) | DONE (decided) |
| G | Tag `v*` + flip repo public + push | USER-GATED |
| H | Post-publish verification | READY |

---

*Maintained alongside `PRD.md` (§11 packaging) and `docs/ROADMAP.md`.*
