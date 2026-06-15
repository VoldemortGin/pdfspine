# oxide-pdf — PyPI Release Sub-PRD / Runbook

> **Audience:** an automated agent (Codex) or a human executing the first PyPI
> publish of `oxide-pdf`. This is a self-contained, step-by-step runbook with
> exact config. Anything marked **[MANUAL — pypi.org]** must be done in the PyPI
> web UI by a human/account owner; everything else is code + CI.

---

## 0. Project facts (do not re-derive)

| Fact | Value |
|---|---|
| PyPI distribution name | **`oxide-pdf`** (verified FREE on PyPI 2026-06-15) |
| Python import package | **`oxide_pdf`** (+ `import fitz` compat shim — see §4) |
| License | **Apache-2.0** (`LICENSE` + `NOTICE` present) |
| GitHub repo | `git@github.com:VoldemortGin/oxide-pdf.git` (owner `VoldemortGin`) — **currently PRIVATE** |
| Build backend | **maturin** (PyO3) — this is a **compiled Rust extension**, not pure Python |
| ABI | **abi3-py311** → ONE wheel per (OS, arch) covers CPython **≥ 3.11** |
| `requires-python` | **`>=3.11`** (buffer-protocol stable-ABI slots need 3.11) |
| Native deps | **pure Rust** (tiny-skia, hayro-\*, image, RustCrypto, …) — **no external C libraries** → wheels are self-contained; no `auditwheel`/`delocate` C-lib bundling needed |
| Free-threaded (PEP 703) wheels | **out of scope for first release** (separate artifact later, per PRD §9.4) |

---

## 1. Recommended first version: `0.1.0a1` (alpha pre-release)

**Rationale (honest):** the library is feature-rich (M0–M6 + parts of M7) and
1100+ tests green, **but it has NOT yet been validated on a real-world PDF
corpus** (no differential-accuracy numbers vs PyMuPDF yet — see the project's
validation task). Shipping the first artifact as a **pre-release** (`0.1.0a1`):
- PyPI marks it "pre-release"; `pip install oxide-pdf` will **not** pick it up
  unless the user passes `--pre` — so nobody is surprised by alpha-quality.
- Lets us exercise the whole publish pipeline for real, safely.

Bump to `0.1.0` (normal release) once the real-corpus validation has produced
acceptable accuracy/open-rate numbers. Use **SemVer**; stay `0.x` (API may change)
until we commit to stability.

> If the owner prefers, `0.1.0` is acceptable — just know it becomes the default
> `pip install` target immediately.

---

## 2. Pre-flight readiness checklist (do BEFORE tagging)

- [ ] `LICENSE` is Apache-2.0 and `NOTICE` exists (✅ already).
- [ ] `pyproject.toml` metadata complete (see §3).
- [ ] `README.md` renders on PyPI: run `python -m twine check dist/*` after build (§9).
- [ ] Version set consistently (see §3 — pick `dynamic` or hard-code `0.1.0a1`).
- [ ] **Decide the `fitz`/`pymupdf` top-level-name question (§4) — IMPORTANT.**
- [ ] Repo made **public** (recommended for an OSS release; Trusted Publishing
      works with private repos too, but an open-source package should have a
      public source link). *(This is the owner's call — flip when ready.)*
- [ ] `cargo test --workspace`, `pytest`, `cargo clippy -D warnings`, `cargo deny check` all green on the release commit.
- [ ] CHANGELOG entry / release notes drafted.

---

## 3. `pyproject.toml` — required metadata

Ensure the `[project]` and `[tool.maturin]` tables contain at least:

```toml
[build-system]
requires = ["maturin>=1.7,<2"]
build-backend = "maturin"

[project]
name = "oxide-pdf"
# Option A (recommended): let maturin read the version from Cargo.toml -> use dynamic
dynamic = ["version"]
# Option B: hard-code -> version = "0.1.0a1"
description = "Apache-2.0, pure-Rust reimplementation of PyMuPDF (fitz): parse/repair/decrypt PDFs, extract text, edit/merge/save, annotations/forms/redaction, render pages to images. Python via PyO3."
readme = "README.md"
requires-python = ">=3.11"
license = "Apache-2.0"                 # SPDX expression (PEP 639)
license-files = ["LICENSE", "NOTICE"]
authors = [{ name = "oxide-pdf authors" }]
keywords = ["pdf", "fitz", "pymupdf", "text-extraction", "render", "rust", "mupdf-alternative"]
classifiers = [
  "Development Status :: 3 - Alpha",
  "License :: OSI Approved :: Apache Software License",
  "Programming Language :: Python :: 3",
  "Programming Language :: Python :: 3.11",
  "Programming Language :: Python :: 3.12",
  "Programming Language :: Python :: 3.13",
  "Programming Language :: Rust",
  "Operating System :: OS Independent",
  "Topic :: Software Development :: Libraries :: Python Modules",
  "Topic :: Text Processing :: General",
  "Topic :: Multimedia :: Graphics :: Graphics Conversion",
]

[project.urls]
Homepage = "https://github.com/VoldemortGin/oxide-pdf"
Repository = "https://github.com/VoldemortGin/oxide-pdf"
Issues = "https://github.com/VoldemortGin/oxide-pdf/issues"

[tool.maturin]
module-name = "oxide_pdf._core"
python-source = "python"
features = ["pyo3/abi3-py311"]
# strip release binaries for smaller wheels
strip = true
# IMPORTANT: control which top-level python packages ship — see §4
# include = ["python/oxide_pdf/**/*"]   # if you must exclude fitz/pymupdf
```

> If `name = "oxide-pdf"` with hyphen errors in maturin, keep the hyphen for the
> dist name; the import package is `oxide_pdf` (dir under `python/`). The current
> repo is already configured this way.

---

## 4. **DECISION — do NOT ship top-level `fitz` / `pymupdf` in the published wheel** (recommended)

The repo has `python/fitz/` and `python/pymupdf/` top-level compat packages so
`import fitz` works in dev. **Publishing those top-level names to PyPI is risky:**
- They would **collide with real PyMuPDF** if a user has both installed
  (`import fitz` could resolve to ours, silently breaking their PyMuPDF code).
- Squatting another project's import name in a public wheel is hostile/confusing.

**Recommended for v0.1.x:** ship **only** the `oxide_pdf` package. Provide
compatibility as a sub-import (e.g. `from oxide_pdf import fitz as fitz` /
`import oxide_pdf.fitz`) rather than a top-level `fitz`. Offer a separate opt-in
distribution (`oxide-pdf-fitz`) later if a true drop-in `import fitz` is wanted.

**Action for Codex:**
1. Confirm the built wheel's contents: `python -m zipfile -l dist/oxide_pdf-*.whl`
   (or `unzip -l`). It should contain `oxide_pdf/…` and **must NOT** contain
   top-level `fitz/` or `pymupdf/`.
2. If they ARE present, exclude them: move the compat shims under
   `python/oxide_pdf/fitz/` + `python/oxide_pdf/pymupdf/` (adjust imports), or set
   `[tool.maturin] include`/`exclude` so only `oxide_pdf` is packaged. Re-verify.
3. Update the README install/usage to show `import oxide_pdf` (and the compat
   import path) — not a bare `import fitz`.

*(If the owner explicitly wants the aggressive drop-in `import fitz` behavior,
ship them but document the PyMuPDF collision loudly. Default = do not ship.)*

---

## 5. Build matrix (what artifacts to produce)

Because of **abi3-py311**, you build **one wheel per (OS, arch)** — NOT one per
Python version. Plus one `sdist`.

| Platform | Target(s) | Notes |
|---|---|---|
| Linux glibc | `x86_64`, `aarch64` | manylinux2014 (`manylinux: auto`); aarch64 via `--zig` or QEMU |
| Linux musl | `x86_64`, `aarch64` | `musllinux_1_2` |
| macOS | `x86_64`, `aarch64` (or `universal2`) | runner `macos-14` |
| Windows | `x64` | (arm64 optional) |
| sdist | — | source dist; buildable by anyone with a Rust toolchain |

All wheels tagged `cp311-abi3-<platform>`. Pure-Rust → no C-lib repair needed.

---

## 6. CI release workflow — `.github/workflows/release.yml`

Build all artifacts with **PyO3/maturin-action**, publish via **PyPI Trusted
Publishing (OIDC, no tokens)**. Create this file:

```yaml
name: release
on:
  push:
    tags: ["v*"]
  workflow_dispatch:
    inputs:
      publish_target:
        description: "pypi or testpypi"
        default: "testpypi"

permissions:
  contents: read

jobs:
  linux:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target: [x86_64, aarch64]
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with:
          target: ${{ matrix.target }}
          args: --release --out dist --features pyo3/abi3-py311
          manylinux: auto
          sccache: "true"
      - uses: actions/upload-artifact@v4
        with: { name: wheels-linux-${{ matrix.target }}, path: dist }

  musllinux:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target: [x86_64, aarch64]
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with:
          target: ${{ matrix.target }}
          args: --release --out dist --features pyo3/abi3-py311
          manylinux: musllinux_1_2
      - uses: actions/upload-artifact@v4
        with: { name: wheels-musllinux-${{ matrix.target }}, path: dist }

  macos:
    runs-on: macos-14
    strategy:
      matrix:
        target: [x86_64, aarch64]
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with:
          target: ${{ matrix.target }}
          args: --release --out dist --features pyo3/abi3-py311
      - uses: actions/upload-artifact@v4
        with: { name: wheels-macos-${{ matrix.target }}, path: dist }

  windows:
    runs-on: windows-latest
    strategy:
      matrix:
        target: [x64]
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with:
          target: ${{ matrix.target }}
          args: --release --out dist --features pyo3/abi3-py311
      - uses: actions/upload-artifact@v4
        with: { name: wheels-windows-${{ matrix.target }}, path: dist }

  sdist:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with: { command: sdist, args: --out dist }
      - uses: actions/upload-artifact@v4
        with: { name: wheels-sdist, path: dist }

  # ---- smoke-test each wheel before publishing ----
  smoke:
    needs: [linux, macos, windows, sdist]
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-14, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with: { python-version: "3.11" }
      - uses: actions/download-artifact@v4
        with: { path: dist, merge-multiple: true }
      - name: install + smoke
        shell: bash
        run: |
          python -m pip install --upgrade pip
          pip install --only-binary :all: --find-links dist oxide-pdf
          python -c "import oxide_pdf; print('import OK', oxide_pdf.__version__)"
          pip install pytest
          pytest python/tests -q

  publish:
    needs: [linux, musllinux, macos, windows, sdist, smoke]
    runs-on: ubuntu-latest
    # Trusted Publishing (OIDC) — NO API token
    environment: ${{ github.event.inputs.publish_target == 'pypi' && 'pypi' || 'testpypi' }}
    permissions:
      id-token: write          # REQUIRED for OIDC Trusted Publishing
    steps:
      - uses: actions/download-artifact@v4
        with: { path: dist, merge-multiple: true }
      - name: Publish to TestPyPI
        if: github.event_name == 'workflow_dispatch' && github.event.inputs.publish_target == 'testpypi'
        uses: pypa/gh-action-pypi-publish@release/v1
        with:
          repository-url: https://test.pypi.org/legacy/
      - name: Publish to PyPI
        if: startsWith(github.ref, 'refs/tags/v')
        uses: pypa/gh-action-pypi-publish@release/v1
```

Notes:
- `pypa/gh-action-pypi-publish@release/v1` with `id-token: write` + a configured
  GitHub `environment` = token-less Trusted Publishing. It also **auto-generates
  PEP 740 build attestations** (satisfies our supply-chain/provenance goal, PRD §11.4).
- Tag push (`v0.1.0a1`) → publishes to **PyPI**. `workflow_dispatch` with
  `testpypi` → publishes to **TestPyPI** (dry run).

---

## 7. **[MANUAL — pypi.org]** Configure Trusted Publishing (one-time, no tokens)

Do this in the PyPI web UI as the account owner BEFORE the first publish:

1. Log in to **https://pypi.org** → account → **Publishing** → **Add a pending publisher**
   (the project `oxide-pdf` doesn't exist yet — a "pending" publisher creates it on first upload).
2. Fill:
   - **PyPI Project Name:** `oxide-pdf`
   - **Owner:** `VoldemortGin`
   - **Repository name:** `oxide-pdf`
   - **Workflow name:** `release.yml`
   - **Environment name:** `pypi`
3. Repeat the same on **https://test.pypi.org** with **Environment name:** `testpypi`
   (for the dry run).
4. In GitHub repo → Settings → Environments, create environments **`pypi`** and
   **`testpypi`** (optionally add required reviewers/protection on `pypi`).

No API tokens are stored anywhere. ✅

---

## 8. Dry run on TestPyPI (do this FIRST)

1. Ensure §7 TestPyPI pending publisher + `testpypi` environment exist.
2. Trigger: GitHub → Actions → `release` → **Run workflow** → input `testpypi`.
3. Confirm all build jobs + `smoke` pass and `publish` uploads to TestPyPI.
4. Verify from a clean machine/venv:
   ```bash
   python -m venv /tmp/v && . /tmp/v/bin/activate
   pip install --pre --index-url https://test.pypi.org/simple/ \
       --extra-index-url https://pypi.org/simple/ oxide-pdf
   python -c "import oxide_pdf; d=oxide_pdf.open; print('ok', oxide_pdf.__version__)"
   ```
   (extra-index-url lets TestPyPI resolve real deps if any.)

---

## 9. Local verification before tagging (sanity, optional but recommended)

```bash
# from repo root, in a venv with maturin + twine
maturin build --release --features pyo3/abi3-py311 --out dist
maturin sdist --out dist
python -m twine check dist/*            # README renders, metadata valid
python -m zipfile -l dist/oxide_pdf-*.whl | grep -E "fitz/|pymupdf/" \
  && echo "WARN: top-level shim shipped — see §4" || echo "wheel clean (oxide_pdf only)"
# clean-venv install + smoke
python -m venv /tmp/w && . /tmp/w/bin/activate && pip install dist/oxide_pdf-*.whl
python -c "import oxide_pdf; print(oxide_pdf.__version__)"
```

---

## 10. Release procedure (the actual publish)

1. Land all changes on `main`; ensure CI green.
2. Set the version: edit `Cargo.toml` `[workspace.package] version = "0.1.0a1"`
   (maturin reads it if `dynamic = ["version"]`), commit.
3. **Make the repo public** (if releasing as OSS) — owner decision.
4. Tag and push:
   ```bash
   git tag v0.1.0a1
   git push origin v0.1.0a1
   ```
5. The `release` workflow runs on the tag → builds all wheels + sdist → smoke →
   **publishes to PyPI** via OIDC.
6. Create a GitHub Release for the tag with notes (optional: `gh release create v0.1.0a1 --generate-notes`).

---

## 11. Post-publish verification

```bash
# fresh venv, install from real PyPI (note --pre for the alpha)
pip install --pre oxide-pdf
python -c "import oxide_pdf; print(oxide_pdf.__version__); print(oxide_pdf.open)"
```
- Check the project page https://pypi.org/project/oxide-pdf/ renders the README,
  shows Apache-2.0, links, and the platform wheels are all present.
- Confirm wheels exist for: linux x86_64+aarch64 (manylinux+musllinux),
  macOS x86_64+arm64, windows x64, plus the sdist.

---

## 12. Risks & gotchas (read before running)

1. **`fitz`/`pymupdf` top-level collision** — §4. Resolve before publishing.
2. **abi3 floor is 3.11** — `requires-python = ">=3.11"` MUST be set or 3.10
   users get a confusing install. (Buffer-protocol slots need 3.11.)
3. **aarch64 Linux cross-build** — `manylinux: auto` + `--zig` (or QEMU). The
   maturin-action handles it; if it flakes, drop aarch64 from v0.1 and add later.
4. **Version must be a valid PEP 440 string** — `0.1.0a1` (not `0.1.0-alpha1`).
5. **First upload uses the PENDING publisher** — §7 must be done first, else the
   `publish` job fails with "project does not exist / not authorized."
6. **`id-token: write`** permission on the `publish` job is mandatory for OIDC.
7. **Don't commit any PyPI token** — Trusted Publishing means there are none.
8. **License metadata** — PEP 639 `license = "Apache-2.0"` needs a recent
   maturin/packaging; if the build complains, fall back to the classifier-only
   form (`License :: OSI Approved :: Apache Software License`) and a `license = {text=...}` or `license-files`.
9. **Free-threaded (3.13t) wheels** — not built here; that's a separate future
   artifact (abi3 doesn't cover the free-threaded ABI).

---

## 13. Acceptance criteria (definition of done for the release)

- [ ] TestPyPI dry run succeeded; clean-venv install + import worked.
- [ ] §7 Trusted Publishing configured for both PyPI and TestPyPI.
- [ ] Wheel verified to contain **only `oxide_pdf`** (no top-level `fitz`/`pymupdf`), §4.
- [ ] `twine check` passes (README renders).
- [ ] Tag `v0.1.0a1` pushed; `release` workflow green end-to-end.
- [ ] `pip install --pre oxide-pdf` works from real PyPI on Linux/macOS/Windows;
      `import oxide_pdf` succeeds and `__version__` matches.
- [ ] PyPI project page shows Apache-2.0, README, all platform wheels + sdist.
- [ ] (Provenance) PEP 740 attestations attached by gh-action-pypi-publish.

---

*Maintained alongside `PRD.md` (§11 packaging) and `docs/ROADMAP.md`. Update the
recommended version/state as validation completes.*
