#!/usr/bin/env python3
"""Run the repository's cross-platform Rust/PyO3 quality gate."""

import argparse
from collections.abc import Callable, Sequence
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
PHASE_ORDER = ("rust", "python", "drift", "artifacts")


def executable(name: str) -> str:
    """Return an executable path or fail with an actionable error."""
    resolved = shutil.which(name)
    if resolved is None:
        raise RuntimeError(f"required executable is not available on PATH: {name}")
    return resolved


def run(arguments: Sequence[str], *, cwd: Path = ROOT) -> None:
    """Run one gate command from the repository root."""
    command = [str(argument) for argument in arguments]
    print(f"\n+ {subprocess.list2cmdline(command)}", flush=True)
    subprocess.run(command, cwd=cwd, check=True)


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
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    selected = set(args.phase or PHASE_ORDER)
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
