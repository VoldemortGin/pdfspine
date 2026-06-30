#!/usr/bin/env bash
# Local release CI/CD for pdfspine (private repo — GitHub Actions is billable and
# may be blocked, so we build + publish from a developer machine instead).
#
# Builds the multi-platform wheels that cross-compile cleanly from macOS — macOS
# (arm64 + x86_64) and Linux manylinux (x86_64 + aarch64, via zig) — plus the
# sdist, then publishes to PyPI with the token in ~/.pypirc. Windows wheels can't
# be cross-compiled from macOS here; Windows users fall back to the sdist (needs a
# Rust toolchain) until a wheel is built on a Windows host or via GitHub CI.
#
# Usage (from repo root):
#   scripts/release-local.sh 0.1.2            # build + publish to PyPI
#   scripts/release-local.sh 0.1.2 --dry-run  # build + twine check only, no upload
#
# Prereqs (one-time): rustup targets x86_64-apple-darwin, x86_64-unknown-linux-gnu,
# aarch64-unknown-linux-gnu; `uv pip install --python .venv/bin/python ziglang`;
# a PyPI token under [pypi] in ~/.pypirc.
set -euo pipefail

VERSION="${1:?usage: release-local.sh <version> [--dry-run]}"
DRY_RUN="${2:-}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VENV="$ROOT/.venv"
MATURIN="$VENV/bin/maturin"
# zig (from the ziglang wheel) — absolute dir so build.rs subprocesses find it.
ZIGDIR="$(cd "$(dirname "$(find "$VENV" -name zig -type f | head -1)")" && pwd)"

echo "==> set version $VERSION"
python3 scripts/set_version_from_tag.py "v$VERSION"

rm -rf dist
echo "==> sdist"
"$MATURIN" sdist --out dist

echo "==> macOS arm64 (native)"
"$MATURIN" build --release --out dist
echo "==> macOS x86_64"
"$MATURIN" build --release --target x86_64-apple-darwin --out dist

for tgt in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
  echo "==> Linux $tgt (manylinux, zig)"
  PATH="$ZIGDIR:$PATH" "$MATURIN" build --release --target "$tgt" --zig --out dist
done

echo "==> twine check"
uvx twine check dist/*

if [ "$DRY_RUN" = "--dry-run" ]; then
  echo "==> dry-run: skipping upload. Built:"
  ls -1 dist
  exit 0
fi

echo "==> publish to PyPI (~/.pypirc token)"
uvx twine upload --skip-existing dist/*
echo "==> done: pdfspine $VERSION published (macOS + Linux wheels + sdist)."
echo "    Windows wheel not built locally; add it on a Windows host if needed."
