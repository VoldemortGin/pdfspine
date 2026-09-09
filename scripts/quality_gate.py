#!/usr/bin/env python3
"""Run the repository's cross-platform Rust/PyO3 quality gate."""

import argparse
from collections.abc import Callable, Sequence
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
PHASE_ORDER = ("rust", "extension", "python", "drift", "artifacts")

# Everything that feeds the compiled `pdfspine._core` extension. The python phase
# only proves anything about the current Rust sources when pytest imports an
# extension built from exactly these inputs, so their content is fingerprinted.
EXTENSION_INPUTS = (
    "crates",
    "Cargo.toml",
    "Cargo.lock",
    "pyproject.toml",
    "rust-toolchain*",
)
# `maturin develop` (editable) writes the module next to the pure-Python sources.
EXTENSION_PACKAGE = ROOT / "python" / "pdfspine"
EXTENSION_PATTERNS = ("_core*.so", "_core*.pyd")
EXTENSION_STAMP = ROOT / ".gate" / "extension.stamp"
SKIP_EXTENSION_ENV = "PDFSPINE_GATE_SKIP_EXTENSION"


def executable(name: str) -> str:
    """Return an executable path or fail with an actionable error."""
    resolved = shutil.which(name)
    if resolved is None:
        raise RuntimeError(f"required executable is not available on PATH: {name}")
    return resolved


def run(
    arguments: Sequence[str],
    *,
    cwd: Path = ROOT,
    env: dict[str, str] | None = None,
) -> None:
    """Run one gate command from the repository root."""
    command = [str(argument) for argument in arguments]
    print(f"\n+ {subprocess.list2cmdline(command)}", flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def run_rust() -> None:
    """Run the Rust formatting, lint, test, and dependency-policy gates."""
    cargo = executable("cargo")
    run([cargo, "fmt", "--all", "--check"])
    run(
        [
            cargo,
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ]
    )
    run([cargo, "test", "--workspace", "--all-features"])
    run([cargo, "deny", "check"])


def extension_inputs() -> list[Path]:
    """Return every tracked or untracked (non-ignored) extension input file."""
    listing = subprocess.run(
        [
            executable("git"),
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            *EXTENSION_INPUTS,
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout
    return sorted({ROOT / entry for entry in listing.decode().split("\0") if entry})


def extension_fingerprint(paths: Sequence[Path]) -> str:
    """Hash the paths and contents of the extension inputs (mtime-independent)."""
    digest = hashlib.sha256()
    for path in paths:
        digest.update(path.relative_to(ROOT).as_posix().encode())
        digest.update(b"\0")
        try:
            content = path.read_bytes()
        except FileNotFoundError:
            # Deleted on disk but still in the index: a change like any other.
            digest.update(b"missing")
        else:
            digest.update(f"{len(content)}\0".encode())
            digest.update(content)
        digest.update(b"\0")
    return digest.hexdigest()


def extension_artifacts() -> list[Path]:
    """Return the compiled `_core` modules installed in the source package."""
    return sorted(
        path
        for pattern in EXTENSION_PATTERNS
        for path in EXTENSION_PACKAGE.glob(pattern)
    )


def extension_rebuild_reason(fingerprint: str) -> str | None:
    """Return why the extension must be rebuilt, or None when it is current."""
    if not extension_artifacts():
        return "extension missing"
    if not EXTENSION_STAMP.exists():
        return "no stamp"
    try:
        stamp = json.loads(EXTENSION_STAMP.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return "unreadable stamp"
    if stamp.get("fingerprint") != fingerprint:
        return "Rust inputs changed since last build"
    if stamp.get("python") != sys.prefix:
        return "extension was built for a different interpreter"
    return None


def write_extension_stamp(fingerprint: str, artifact: Path) -> None:
    EXTENSION_STAMP.parent.mkdir(exist_ok=True)
    stamp = {
        "fingerprint": fingerprint,
        "built_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "extension": artifact.relative_to(ROOT).as_posix(),
        "python": sys.prefix,
    }
    EXTENSION_STAMP.write_text(json.dumps(stamp, indent=2) + "\n", encoding="utf-8")


def run_extension() -> None:
    """Rebuild the `_core` extension unless it matches the current Rust inputs."""
    fingerprint = extension_fingerprint(extension_inputs())
    reason = extension_rebuild_reason(fingerprint)
    if reason is None:
        print(f"extension up to date (fingerprint {fingerprint[:12]})", flush=True)
        return
    print(f"extension: rebuilding ({reason})", flush=True)
    if sys.prefix == sys.base_prefix:
        raise RuntimeError(
            "the extension can only be rebuilt from the project virtual environment; "
            f"activate .venv (running interpreter: {sys.executable})"
        )
    if importlib.util.find_spec("maturin") is None:
        raise RuntimeError(
            f"maturin is not installed in {sys.prefix}; install it with: "
            f'"{sys.executable}" -m pip install "maturin>=1.12,<2"'
        )
    # Point maturin at this interpreter's environment explicitly: it refuses to
    # run when both VIRTUAL_ENV and CONDA_PREFIX are set (a conda base shell).
    env = {key: value for key, value in os.environ.items() if key != "CONDA_PREFIX"}
    env["VIRTUAL_ENV"] = sys.prefix
    run([sys.executable, "-m", "maturin", "develop", "--release"], env=env)
    artifacts = extension_artifacts()
    if not artifacts:
        raise RuntimeError(
            f"maturin develop finished but no _core module exists in {EXTENSION_PACKAGE}"
        )
    write_extension_stamp(fingerprint, artifacts[0])
    print(f"extension rebuilt (fingerprint {fingerprint[:12]})", flush=True)


def run_python() -> None:
    """Run formatting, linting, typing, and Python API tests."""
    python = sys.executable
    sources = ["python/pdfspine", "python/tests", "scripts"]
    run([python, "-m", "ruff", "format", "--check", *sources])
    run([python, "-m", "ruff", "check", *sources])
    # pyproject.toml owns the strict migration scope. Keep this command
    # argument-free so expanding that scope cannot drift from the gate.
    run([python, "-m", "mypy"])
    run(
        [
            python,
            "-m",
            "pytest",
            "-W",
            "error",
            "--doctest-modules",
            "python/pdfspine",
            "python/tests",
        ]
    )


def run_drift() -> None:
    """Run the repository's deterministic compatibility and provenance guards."""
    python = sys.executable
    for relative_path in (
        "scripts/test-order-guard.py",
        "scripts/catalog-status-guard.py",
        "scripts/compat-symbol-guard.py",
        "scripts/manifest-lint.py",
    ):
        run([python, relative_path])


def venv_python(venv: Path) -> Path:
    """Return the interpreter path for a virtual environment on any platform."""
    if os.name == "nt":
        return venv / "Scripts" / "python.exe"
    return venv / "bin" / "python"


def smoke_install(artifact: Path, environment: Path) -> None:
    """Install one built artifact into a clean environment and smoke its API."""
    run([sys.executable, "-m", "venv", str(environment)])
    python = venv_python(environment)
    run([str(python), "-m", "pip", "install", "--upgrade", "pip"])
    run([str(python), "-m", "pip", "install", str(artifact)])
    run([str(python), "scripts/html_export_smoke.py"])


def one_artifact(directory: Path, pattern: str) -> Path:
    """Return the sole artifact matching a build pattern."""
    matches = sorted(directory.glob(pattern))
    if len(matches) != 1:
        rendered = ", ".join(path.name for path in matches) or "none"
        raise RuntimeError(
            f"expected exactly one {pattern} artifact in {directory}, found: {rendered}"
        )
    return matches[0]


def run_artifacts() -> None:
    """Build and clean-install both the wheel and source distribution."""
    maturin = executable("maturin")
    with tempfile.TemporaryDirectory(prefix="pdfspine-quality-") as temporary:
        temporary_root = Path(temporary)
        distributions = temporary_root / "dist"
        distributions.mkdir()

        run([maturin, "build", "--release", "--out", str(distributions)])
        run([maturin, "sdist", "--out", str(distributions)])

        wheel = one_artifact(distributions, "*.whl")
        source_distribution = one_artifact(distributions, "*.tar.gz")
        smoke_install(wheel, temporary_root / "wheel-venv")
        smoke_install(source_distribution, temporary_root / "sdist-venv")


PHASE_RUNNERS: dict[str, Callable[[], None]] = {
    "rust": run_rust,
    "extension": run_extension,
    "python": run_python,
    "drift": run_drift,
    "artifacts": run_artifacts,
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--phase",
        action="append",
        choices=PHASE_ORDER,
        help="run only this phase; repeat to select multiple phases (default: all)",
    )
    parser.add_argument(
        "--skip-extension-check",
        action="store_true",
        help="do not verify/rebuild the _core extension before the python phase "
        f"(also: {SKIP_EXTENSION_ENV}=1)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    selected = set(args.phase or PHASE_ORDER)
    if "python" in selected:
        # pytest is only meaningful against an extension built from the current
        # Rust sources, so selecting python always brings the extension check.
        selected.add("extension")
    if args.skip_extension_check or os.environ.get(SKIP_EXTENSION_ENV) == "1":
        selected.discard("extension")
    try:
        for phase in PHASE_ORDER:
            if phase in selected:
                print(f"\n== {phase} ==", flush=True)
                PHASE_RUNNERS[phase]()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        print(f"\nQUALITY GATE FAILED: {error}", file=sys.stderr)
        return 1

    print("\nQUALITY GATE PASSED", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
