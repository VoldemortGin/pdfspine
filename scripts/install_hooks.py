#!/usr/bin/env python3
"""Configure Git to use the repository's version-controlled hooks."""

from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    subprocess.run(
        ["git", "config", "core.hooksPath", ".githooks"],
        cwd=ROOT,
        check=True,
    )
    print("Configured core.hooksPath=.githooks")


if __name__ == "__main__":
    main()
