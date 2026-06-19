# pdfspine — GO-LIVE RUNBOOK (PyPI + public GitHub)

> **Audience:** the maintainer (or an automated agent) executing the first public
> release of `pdfspine`. This is the ordered, end-to-end runbook. Each step is
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
| PyPI distribution name | **`pdfspine`** (reserved / verified free) | USER-GATED (publish) |
| crates.io name | **`pdfspine`** (reserved for brand protection) | NOT PUBLISHED (Python-first, like ragspine; all crates `publish=false`) |
| Python import package | **`pdfspine`** (+ opt-in `pdfspine.fitz` / `pdfspine.pymupdf` compat shims via `install_fitz_shim()`) | DONE |
| License | **Apache-2.0** (`LICENSE` + `NOTICE` + per-data `PROVENANCE.md`) | DONE |
| GitHub repo | `github.com/VoldemortGin/pdfspine` — **currently PRIVATE** | USER-GATED (flip public) |
| Build backend | **maturin** (PyO3 compiled Rust extension `pdfspine._core`) | DONE |
| ABI | **abi3-py311** → ONE wheel per (OS, arch), CPython **≥ 3.11** | DONE |
| `requires-python` | `>=3.11` | DONE |
| Native build deps | pure-Rust codecs/crypto, **BUT** the OCR `tract` kernels compile per-arch **assembly** → a C/asm toolchain is needed to *build from source* (GH runners + maturin-action containers already have it) | DONE (documented in README) |
| OCR delivery | **Option A (P0-5r):** ONE `pdfspine` wheel with the PaddleOCR engine compiled IN, but the ~16 MB ONNX models NOT embedded; models ship as the separate `pdfspine-ocr-models` data distribution that the `pdfspine[ocr]` extra pulls in | DONE (§D.1) |
| Wheel size | `pdfspine` ~7 MB (compressed; OCR code in, models out); `pdfspine-ocr-models` ~15 MB universal `py3-none-any` data wheel | DONE |
| PyPI projects | **two**: `pdfspine` + `pdfspine-ocr-models` (each needs its own pending publisher — see §F.1) | USER-GATED (publish) |

---

## A. Final gate (run on the release commit) — READY

Run from the repo root; all must be green before tagging.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo deny check                       # license/advisory gate (no GPL/AGPL/LGPL/MPL/SSPL)

# Python side
maturin develop
pytest python/tests

# Drift / parity guards
python scripts/compat-symbol-guard.py
python scripts/manifest-lint.py
python scripts/test-order-guard.py
python scripts/catalog-status-guard.py
```

CI (`.github/workflows/ci.yml`) already runs all of the above on the 3-OS matrix
across Python 3.10–3.13; confirm the run on the release commit is green.

---

## B. Docs + version final pass — PARTLY DONE

Already applied in this audit pass (**DONE**):

- `pyproject.toml` classifiers: `Development Status :: 3 - Alpha`,
  `Intended Audience :: Developers`, OS classifiers (OS-Independent + Linux/MacOS/
  Windows), per-minor Python (3.11/3.12/3.13), extra Topic classifiers.
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

Still to do (**USER-GATED — version decision**):

1. **Bump the version** from `0.0.0` to a real release. `0.0.0` cannot be uploaded
   to PyPI. Recommended first artifact: **`0.1.0a1`** (PyPI alpha
   pre-release; `pip install pdfspine` won't pick it up without `--pre`), or
   `0.1.0` if you want it to be the default install target immediately. Keep the
   two in sync:
   - `Cargo.toml` `[workspace.package] version = "0.1.0a1"` → Cargo uses
     `0.1.0-a1` form; for the wheel set `pyproject.toml` `version = "0.1.0a1"`
     (PEP 440). (The two version strings differ in spelling by ecosystem; that is
     expected.)
2. **(Optional) CHANGELOG.md / first release notes** — recommended for the public
   narrative.
3. **(Optional, recommended) Community files** — `CONTRIBUTING.md`,
   `CODE_OF_CONDUCT.md`, `SECURITY.md` (GitHub surfaces their absence). Not a
   blocker.

---

## C. LOCAL folder rename — USER-GATED — **DO THIS LAST (build-env step)**

> This invalidates the current `.venv` (absolute paths) and any cached build. Do
> it only when steps A–B are settled and you are ready to build/publish. The git
> remote and repo name are unaffected (they are already `pdfspine`).

```bash
# 1. Deactivate any active venv first.
deactivate 2>/dev/null || true

# 2. Rename the working tree.
mv ~/workspace/pypdf ~/workspace/pdfspine

# 3. Recreate the venv at the new path (old .venv hardcodes the old path).
cd ~/workspace/pdfspine
rm -rf .venv
uv venv .venv && source .venv/bin/activate
pip install "maturin>=1.12,<2" pytest hypothesis

# 4. Re-verify the build at the new location.
maturin develop
python -c "import pdfspine; print(pdfspine.__version__)"
pytest python/tests -q
```

From here on, all commands run from `~/workspace/pdfspine`.

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

### D.1 OCR delivery — two distributions (Option A, P0-5r)

**Decision (P0-5r):** OCR ships as **one wheel with the PaddleOCR engine compiled
in + a separate models data package** — NOT as a second "fat" wheel, and NOT with
the ~16 MB ONNX models embedded in every wheel.

| Distribution | What | How it is built |
|---|---|---|
| **`pdfspine`** | the engine wheel — OCR *code* compiled in (`[tool.maturin] features = ["pyo3/abi3-py311", "ocr"]`), models **NOT** embedded | the `wheels` + `sdist` jobs (maturin) |
| **`pdfspine-ocr-models`** | pure-data, universal `py3-none-any` wheel — the 3 PP-OCRv4 ONNX weights only (import package `pdfspine_ocr_models`, exposes `models_dir()`) | the `ocr-models` job (`python -m build packages/pdfspine-ocr-models`) |

The companion does **not** keep a second copy of the models in git: its hatchling
build hook (`packages/pdfspine-ocr-models/hatch_build.py`) `force_include`s the 3
ONNX from the repo's single source of truth `crates/pdf-ocr/models/` into both its
sdist and its wheel at build time. `crates/pdf-ocr/models/` also remains the Rust
**dev fallback** for source checkouts.

**End-user UX:**

```bash
pip install pdfspine          # OCR code present, NO models -> engine="paddle" raises a clear error
pip install pdfspine[ocr]     # + pdfspine-ocr-models -> OCR works, fully offline
```

**Runtime model resolution order** (`python/pdfspine/document.py`
`_ensure_ocr_models_env` sets the env, then the Rust `models_dir()` in
`crates/pdf-ocr/src/paddle/model.rs` resolves it):

1. **`PDFSPINE_OCR_MODELS`** env var, if already set — explicit user override;
2. else the installed **`pdfspine_ocr_models`** companion (the `[ocr]` extra) —
   Python exports its `models_dir()` as `PDFSPINE_OCR_MODELS`;
3. else the in-repo **`crates/pdf-ocr/models`** dev fallback (source checkout);
4. else a clear **`PdfUnsupportedError`** pointing at `pip install pdfspine[ocr]`.

Local sanity build of the companion (optional, before tagging):

```bash
python -m build packages/pdfspine-ocr-models --outdir dist   # data sdist + universal wheel
python -c "import pdfspine_ocr_models, os; d=pdfspine_ocr_models.models_dir(); print(d, sorted(os.listdir(d)))"
```

The `ocr-models` job stamps the companion's version from the tag so it tracks the
`pdfspine` release it ships alongside, and the `publish` job uploads BOTH
distributions through the same OIDC trusted-publishing call (each matched to its
own pending publisher by project name).

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

The release workflow does NOT auto-smoke before publish, so verify the TestPyPI
dry run from a clean machine/venv first:

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

### F.1 PyPI (Trusted Publishing — preferred, no tokens)

One-time setup in the PyPI web UI (do BEFORE the first publish). There are **two**
PyPI projects (§D.1), so add the pending publisher **twice** — same Owner /
Repository / Workflow / Environment, once per project name:

1. **https://pypi.org** → account → **Publishing** → **Add a pending publisher**
   (do this for BOTH project names):
   - Project name: `pdfspine`  **and**  `pdfspine-ocr-models`
   - Owner: `VoldemortGin`
   - Repository: `pdfspine`
   - Workflow: `release.yml`
   - Environment: `pypi`
2. Repeat both on **https://test.pypi.org** with Environment `testpypi` (dry run).
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
| C | Local folder rename `pypdf` → `pdfspine` + recreate `.venv` (**LAST build step**) | USER-GATED |
| D | Build wheel matrix + sdist (`release.yml`) | READY |
| D.1 | OCR = `pdfspine` (engine-in wheel) + `pdfspine-ocr-models` (data) — Option A (P0-5r) | READY |
| E | Test-install wheels (TestPyPI dry run) | READY |
| F.1 | PyPI Trusted Publishing setup + publish (**two** pending publishers) | USER-GATED |
| F.2 | crates.io — name reserved, NOT published (Python-first) | DONE (decided) |
| G | Tag `v*` + flip repo public + push | USER-GATED |
| H | Post-publish verification | READY |

---

*Maintained alongside `PRD.md` (§11 packaging) and `docs/ROADMAP.md`.*
