#!/usr/bin/env python3
"""manifest-lint — affirmative-license fixture gate + manifest hygiene (PRD §10.3, D11).

The corpus is a license-exposure surface, so the policy is
**affirmative-permissive-license-REQUIRED**, not merely "AGPL-absent". This lint
enforces two invariants:

A) fixtures/MANIFEST.toml — every committed/declared fixture is affirmatively
   cleared:
     (a) required fields present: path, source, license, sha256, cleared_by,
         cleared_date;
     (b) `license` is in the §6.3 permissive allowlist (or an explicit PD/CC0
         declaration) — empty / "unknown" FAILS (absence of a positive license
         is a hard fail);
     (c) `cleared_by` is non-empty;
     (d) sha256 is a well-formed 64-hex digest, and — when the fixture file is
         present on disk (the corpus itself is regenerable / git-ignored) —
         matches the file bytes;
     (e) directory ⇄ manifest reconciliation: when the corpus dir is present,
         every file under it must be declared (no undeclared fixtures).

   The clean-room "no AGPL-derived corpus" invariant is enforced *affirmatively*
   by rule (b): an AGPL/MuPDF-derived file could never carry a permissive/PD
   clearance. A raw byte-scan for "GPL" is deliberately NOT used — arbitrary
   compressed PDF streams contain that 3-byte sequence by chance, so it only
   yields false positives (false confidence), not a real signal.

B) conformance manifests (git-tracked *.json under conformance/) are well-formed
   JSON and carry NO stale absolute paths (e.g. `/Users/...`, `/home/...`,
   `C:\\...`) that would only resolve on one developer's machine.

Run from the repo root::

    python3 scripts/manifest-lint.py        # exit 0 on success

Requires only the standard library (``tomllib``, Python 3.11+).
"""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES_DIR = REPO_ROOT / "fixtures"
MANIFEST = FIXTURES_DIR / "MANIFEST.toml"
CONFORMANCE_DIR = REPO_ROOT / "conformance"

ALLOWED_LICENSES = {
    "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "Zlib",
    "Unicode-DFS-2016", "ISC", "CC0-1.0", "Unlicense", "PD", "Public-Domain",
}
REQUIRED_FIELDS = ("path", "source", "license", "sha256", "cleared_by", "cleared_date")

SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
# Absolute paths that only resolve on one machine: POSIX /Users//home//root,
# or a Windows drive letter (C:\ ... ). Repo-relative paths are fine.
ABS_PATH_RE = re.compile(r"(?:/Users/|/home/|/root/|[A-Za-z]:\\\\|[A-Za-z]:/)")


def _git_tracked(pattern: str) -> list[Path]:
    """git-tracked files matching a pathspec (empty list if git is unavailable)."""
    try:
        out = subprocess.check_output(
            ["git", "-C", str(REPO_ROOT), "ls-files", pattern],
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError):
        return []
    return [REPO_ROOT / line for line in out.decode("utf-8").splitlines() if line]


def lint_fixtures() -> list[str]:
    errors: list[str] = []
    if not MANIFEST.exists():
        return [f"fixtures manifest not found at {MANIFEST}"]

    try:
        data = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        return [f"{MANIFEST.name} is not valid TOML: {exc}"]

    declared: set[Path] = set()
    for entry in data.get("fixture", []):
        path = entry.get("path") or "<no-path>"

        missing = [f for f in REQUIRED_FIELDS if not entry.get(f)]
        if missing:
            errors.append(f"{path}: missing required field(s) {missing}")

        lic = entry.get("license")
        if lic not in ALLOWED_LICENSES:
            errors.append(
                f"{path}: license {lic!r} is not in the affirmative allowlist "
                f"(empty/unknown is a hard fail); permitted: {sorted(ALLOWED_LICENSES)}"
            )

        if not entry.get("cleared_by"):
            errors.append(f"{path}: empty cleared_by (every fixture needs a named clearer)")

        sha = entry.get("sha256")
        if not (isinstance(sha, str) and SHA256_RE.match(sha)):
            errors.append(f"{path}: sha256 {sha!r} is not a 64-char lowercase hex digest")

        if isinstance(entry.get("path"), str):
            fpath = FIXTURES_DIR / entry["path"]
            declared.add(fpath.resolve())
            # Integrity check only when the (regenerable) file is present.
            if fpath.is_file() and isinstance(sha, str) and SHA256_RE.match(sha):
                actual = hashlib.sha256(fpath.read_bytes()).hexdigest()
                if actual != sha:
                    errors.append(
                        f"{path}: sha256 mismatch — manifest {sha}, file {actual}"
                    )

    # Directory ⇄ manifest reconciliation (only when the corpus dir is present).
    if FIXTURES_DIR.exists():
        for p in FIXTURES_DIR.rglob("*"):
            if not p.is_file() or p.name == "MANIFEST.toml":
                continue
            if p.resolve() not in declared:
                rel = p.relative_to(FIXTURES_DIR)
                errors.append(f"{rel}: present under fixtures/ but not declared in MANIFEST.toml")

    return errors


def lint_conformance_manifests() -> list[str]:
    """Tracked conformance *.json: well-formed JSON, no stale absolute paths."""
    errors: list[str] = []
    tracked = dict.fromkeys(
        _git_tracked("conformance/**/*.json") + _git_tracked("conformance/*.json")
    )
    for path in tracked:
        if not path.exists():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            errors.append(f"{path.relative_to(REPO_ROOT)}: unreadable ({exc})")
            continue
        try:
            json.loads(text)
        except json.JSONDecodeError as exc:
            errors.append(f"{path.relative_to(REPO_ROOT)}: malformed JSON ({exc})")
            continue
        if ABS_PATH_RE.search(text):
            errors.append(
                f"{path.relative_to(REPO_ROOT)}: contains stale absolute path(s) "
                f"(machine-specific; use repo-relative paths)"
            )
    return errors


def main(argv: list[str]) -> int:
    fixture_errors = lint_fixtures()
    conformance_errors = lint_conformance_manifests()
    errors = fixture_errors + conformance_errors

    print("manifest-lint — affirmative-license fixtures + manifest hygiene")
    print(f"  fixtures manifest    : {MANIFEST.relative_to(REPO_ROOT)}")
    print(f"  conformance manifests: git-tracked *.json under conformance/")

    if errors:
        print(f"\n  FAIL ({len(errors)} problem(s)):", file=sys.stderr)
        for e in errors:
            print(f"    - {e}", file=sys.stderr)
        return 1

    print("  OK — every fixture is affirmatively cleared; "
          "conformance manifests are well-formed with no stale absolute paths.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
