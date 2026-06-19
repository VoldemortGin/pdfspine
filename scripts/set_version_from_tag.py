#!/usr/bin/env python3
"""set_version_from_tag — set the workspace version from a release tag (CI only).

The crate version in ``Cargo.toml`` ([workspace.package].version) is the single
source of truth for the wheel: maturin's ``dynamic = ["version"]`` reads it into
the dist metadata (and thus ``pdfspine.__version__`` via importlib.metadata), and
``CARGO_PKG_VERSION`` reads it for ``_core.version()``. Local/dev builds leave it
at the ``0.0.0`` default; a tagged CI build calls this to stamp the real version.

Usage (from repo root)::

    python scripts/set_version_from_tag.py v1.2.3   # or a bare 1.2.3

The leading ``v`` is stripped. This rewrites the ``[workspace.package]`` version
*and* the matching ``version = "..."`` requirement on each first-party path crate
in ``[workspace.dependencies]`` (which must track the package version or the
workspace fails to resolve) — the same coherent bump ``cargo set-version
--workspace`` performs. pyproject.toml inherits the version via maturin's dynamic
metadata, so it needs no edit. Cross-platform (no shell/sed); runs identically on
Linux/macOS/Windows.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: set_version_from_tag.py <tag-or-version>", file=sys.stderr)
        return 2
    version = argv[1].lstrip("v").strip()
    if not version:
        print("error: empty version after stripping leading 'v'", file=sys.stderr)
        return 2

    text = CARGO_TOML.read_text(encoding="utf-8")

    # 1) The [workspace.package] version (the single source of truth). Anchor on
    # the table header so only the version line inside that table is touched.
    pkg_pattern = re.compile(
        r"(\[workspace\.package\][^\[]*?\nversion = \")[^\"]*(\")",
        re.DOTALL,
    )
    text, n = pkg_pattern.subn(rf"\g<1>{version}\g<2>", text, count=1)
    if n != 1:
        print(
            "error: could not find [workspace.package] version line in Cargo.toml",
            file=sys.stderr,
        )
        return 1

    # 2) The version requirement on each first-party path crate in
    # [workspace.dependencies] — e.g. `pdf-core = { path = "crates/pdf-core",
    # version = "0.0.0" }`. Cargo requires these to satisfy the crate's actual
    # version, so they must move with it (this is what `cargo set-version
    # --workspace` does). Only inline tables that carry a `path = "crates/..."`
    # are rewritten; third-party `req`-style versions are left alone.
    dep_pattern = re.compile(
        r"(\{[^{}]*path = \"crates/[^\"]+\"[^{}]*version = \")[^\"]*(\"[^{}]*\})"
    )
    text = dep_pattern.sub(rf"\g<1>{version}\g<2>", text)

    CARGO_TOML.write_text(text, encoding="utf-8")
    print(f"set workspace version to {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
