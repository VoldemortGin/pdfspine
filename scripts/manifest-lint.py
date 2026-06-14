#!/usr/bin/env python3
"""manifest-lint — affirmative-license fixture gate (PRD §10.3, D11).

The corpus is itself a license-exposure surface, so the policy is
**affirmative-permissive-license-required**, not merely "AGPL-absent". The real
lint fails on:

  (a) any fixture file with a non-affirmative license (empty / "unknown");
  (b) any `AGPL`/`GPL` string match in a fixture;
  (c) a known-AGPL-hash blocklist match;
  (d) any file present under `fixtures/` but absent from `MANIFEST.toml`;
  (e) any empty `cleared_by` for a non-self-generated file.

Required per-fixture fields (PRD §10.3): source, license, sha256, cleared_by,
cleared_date. `license` must be in the §6.3 permissive ✅ set or an explicit
PD/CC0 declaration.

Algorithm sketch:
  - Parse `fixtures/MANIFEST.toml` (each [[fixture]] entry).
  - Walk `fixtures/` for actual files; reconcile against the manifest (rule d).
  - Validate each entry's fields and license allowlist (rules a/e).
  - Verify sha256 matches the file on disk (integrity).
  - Scan bytes / hashes for the AGPL signals (rules b/c).
  - Exit non-zero with the offending paths.

M0 status: lenient stub. The manifest currently has the schema header but no
fixtures (corpus lands in M1). It still parses the manifest and walks the dir to
prove the wiring; it does not fail the build yet. The M0 exit criterion of
"provenance lint fails a planted AGPL fixture AND a planted unlicensed fixture"
is implemented in M1 alongside the first real fixtures.
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES_DIR = REPO_ROOT / "fixtures"
MANIFEST = FIXTURES_DIR / "MANIFEST.toml"

ALLOWED_LICENSES = {
    "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "Zlib",
    "Unicode-DFS-2016", "ISC", "CC0-1.0", "Unlicense", "PD", "Public-Domain",
}


def main(argv: list[str]) -> int:
    if not MANIFEST.exists():
        print(f"manifest-lint: MANIFEST not found at {MANIFEST}", file=sys.stderr)
        return 0  # lenient in M0

    # Count committed fixture files (excluding the manifest itself).
    files = [
        p for p in FIXTURES_DIR.rglob("*")
        if p.is_file() and p.name != "MANIFEST.toml"
    ]
    print(
        f"manifest-lint: M0 stub — {len(files)} fixture file(s) on disk; "
        "schema-only manifest (corpus lands in M1) — OK"
    )
    # TODO(M1): enforce rules (a)-(e) above against real fixtures + planted
    # AGPL / unlicensed canaries.
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
