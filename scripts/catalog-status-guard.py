#!/usr/bin/env python3
"""catalog-status-guard — milestone-exit catalog gate (PRD §10.1.1 step 3).

The real guard verifies that, at a milestone boundary, there are **0 remaining
RED tags** for that milestone's catalog IDs, i.e. every catalogued test has been
driven green.

Algorithm sketch:
  - Parse `docs/test-case-catalog.md` into rows (ID, feature, spec ref, status).
  - For the milestone under test, assert every row's status == `green`.
  - Cross-check the source tree for leftover RED markers:
      * Rust:   `#[ignore = "RED: <ID> ...`
      * Python: `@pytest.mark.xfail(strict=True, reason="RED: <ID> ...`
    Any match whose <ID> belongs to the milestone is a failure.
  - Exit non-zero listing the offending IDs.

M0 status: lenient stub — validates the catalog file *parses* and reports the
status histogram, but does not fail the build. Wire strict milestone-exit
enforcement in M1.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CATALOG = REPO_ROOT / "docs" / "test-case-catalog.md"

ROW_RE = re.compile(r"^\|\s*`?(?P<id>[A-Z0-9][A-Z0-9-]+)`?\s*\|.*\|\s*(?P<status>catalogued|written|red|green)\s*\|")


def main(argv: list[str]) -> int:
    if not CATALOG.exists():
        print(f"catalog-status-guard: catalog not found at {CATALOG}", file=sys.stderr)
        return 0  # lenient in M0

    histogram: dict[str, int] = {}
    for line in CATALOG.read_text(encoding="utf-8").splitlines():
        m = ROW_RE.match(line.strip())
        if m:
            histogram[m["status"]] = histogram.get(m["status"], 0) + 1

    print(f"catalog-status-guard: M0 stub — status histogram: {histogram or '{}'} — OK")
    # TODO(M1): fail when a milestone has any non-green catalogued ID or any
    # leftover RED tag in the source tree.
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
