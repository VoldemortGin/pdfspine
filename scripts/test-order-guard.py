#!/usr/bin/env python3
"""test-order-guard — catalog ⇄ RED-tag consistency (PRD §10.1.1, D12).

The TDD protocol (PRD §10.1.1) gives every catalogued test a status in
``docs/test-case-catalog.md`` whose ``red`` value is *defined* by a matching RED
tag in the source tree::

    `red` — test landed and failing for the right reason (tagged
            #[ignore = "RED: <ID> …"]  /  @pytest.mark.xfail(strict=True,
            reason="RED: <ID> …")).

This guard enforces the deterministic, structural half of that protocol — the
part that needs no git history or PR context: the set of catalog IDs marked
``red`` must be **exactly** the set of RED ``<ID>`` tags present in the source
tree. Concretely it fails (exit 1) when either side drifts:

  1. A catalog row is marked ``red`` but no ``RED: <ID>`` tag exists in
     crates/**/tests, crates/**/src (``#[cfg(test)]``), or python/tests
     → a phantom RED status (the test was never actually landed RED).
  2. A ``RED: <ID>`` tag exists in the source tree but its catalog row is not
     marked ``red`` (it is missing, or ``catalogued``/``written``/``green``)
     → an un-catalogued / mis-statused RED test (impl may have been merged
     ahead of, or without, updating the catalog).

It also fails if a RED tag references an ``<ID>`` that is absent from the
catalog entirely (every test must trace to a catalogued case).

This is the same observable invariant the milestone-exit protocol relies on
(0 leftover RED at exit), made continuously enforceable.

Run from the repo root::

    python3 scripts/test-order-guard.py        # exit 0 on success

Requires only the standard library.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CATALOG = REPO_ROOT / "docs" / "test-case-catalog.md"

# Catalog table row:  | `GEOM-MAT-001` | feature | spec ref | status |
CATALOG_ROW_RE = re.compile(
    r"^\|\s*`(?P<id>[A-Za-z][A-Za-z0-9_]*(?:-[A-Za-z0-9_]+)+)`\s*\|"
    r".*\|\s*(?P<status>catalogued|written|red|green)\s*\|\s*$"
)

# RED tag in source:  RED: <ID>   (Rust #[ignore = "RED: …"] / pytest xfail reason).
RED_TAG_RE = re.compile(r"RED:\s*(?P<id>[A-Za-z][A-Za-z0-9_]*(?:-[A-Za-z0-9_]+)+)")

# Where RED tags may live.
SOURCE_GLOBS = (
    ("crates", "*.rs"),
    ("python", "*.py"),
)


def parse_catalog() -> tuple[set[str], dict[str, str]]:
    """Return (all_ids, {id: status}) parsed from the catalog tables."""
    all_ids: set[str] = set()
    status: dict[str, str] = {}
    if not CATALOG.exists():
        return all_ids, status
    for raw in CATALOG.read_text(encoding="utf-8").splitlines():
        m = CATALOG_ROW_RE.match(raw.strip())
        if m:
            cid = m["id"]
            all_ids.add(cid)
            status[cid] = m["status"]
    return all_ids, status


def scan_red_tags() -> dict[str, list[str]]:
    """Return {catalog-ID: [source locations]} for every RED: <ID> tag found."""
    found: dict[str, list[str]] = {}
    for sub, pattern in SOURCE_GLOBS:
        root = REPO_ROOT / sub
        if not root.exists():
            continue
        for path in root.rglob(pattern):
            try:
                text = path.read_text(encoding="utf-8")
            except OSError:
                continue
            for i, line in enumerate(text.splitlines(), start=1):
                for m in RED_TAG_RE.finditer(line):
                    rel = path.relative_to(REPO_ROOT)
                    found.setdefault(m["id"], []).append(f"{rel}:{i}")
    return found


def main(argv: list[str]) -> int:
    if not CATALOG.exists():
        print(
            f"test-order-guard: FAIL — catalog not found at {CATALOG}",
            file=sys.stderr,
        )
        return 1

    all_ids, status = parse_catalog()
    red_ids_in_catalog = {cid for cid, st in status.items() if st == "red"}
    red_tags = scan_red_tags()
    red_ids_in_source = set(red_tags)

    errors: list[str] = []

    # 1. catalog says red, but no RED tag in source.
    for cid in sorted(red_ids_in_catalog - red_ids_in_source):
        errors.append(
            f"{cid}: catalog status is `red` but no `RED: {cid}` tag exists in "
            f"the source tree (phantom RED — test never landed RED)"
        )

    # 2. RED tag in source, but catalog row is not `red` (or is absent).
    for cid in sorted(red_ids_in_source - red_ids_in_catalog):
        locs = ", ".join(red_tags[cid][:3])
        if cid not in all_ids:
            errors.append(
                f"{cid}: `RED: {cid}` tag in source ({locs}) but no catalog row "
                f"for this ID (every test must trace to a catalogued case)"
            )
        else:
            errors.append(
                f"{cid}: `RED: {cid}` tag in source ({locs}) but catalog status "
                f"is `{status[cid]}`, not `red`"
            )

    print("test-order-guard — catalog ⇄ RED-tag consistency")
    print(f"  catalog IDs        : {len(all_ids)}  ({CATALOG.relative_to(REPO_ROOT)})")
    print(f"  catalog `red` rows : {len(red_ids_in_catalog)}")
    print(f"  RED: tags in source: {len(red_ids_in_source)}")

    if errors:
        print(f"\n  FAIL ({len(errors)} mismatch(es)):", file=sys.stderr)
        for e in errors:
            print(f"    - {e}", file=sys.stderr)
        return 1

    print("  OK — catalog `red` rows and source RED tags are in 1:1 correspondence.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
