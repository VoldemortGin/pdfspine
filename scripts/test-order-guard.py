#!/usr/bin/env python3
"""test-order-guard — enforce test-precedes-implementation (PRD §10.1.1, D12).

The real guard inspects git history (NOT just the current diff) to verify the
two-state-per-test protocol:

  1. Each *new test function* added in a PR must have a matching entry in
     `docs/test-case-catalog.md` whose status was `catalogued` or `written` in a
     PRIOR merged commit (not introduced in the same PR).
  2. An implementation PR may only *un-ignore* a catalog ID whose RED test
     (`#[ignore = "RED: <ID> ..."]` / `@pytest.mark.xfail(... "RED: ...")`) was
     added in an EARLIER merged commit.
  3. A PR may not introduce a new public function whose only tests are added in
     the same PR unless those tests were catalogued in an earlier commit.

Algorithm sketch (to implement in M1 when the first real tests land):
  - Determine the merge base with the target branch (`git merge-base`).
  - Diff added test fns (`#[test]` / `fn <name>` in `tests/` and
    `#[cfg(test)]`, plus `def test_*` in `python/tests/`).
  - For each, resolve its CATALOG-ID (from a `// <ID>` trace comment / reason
    string) and `git log` the catalog entry to confirm a prior-commit status.
  - For un-ignored IDs, `git log -S` the RED tag to confirm it predates HEAD.
  - Exit non-zero with a precise message on any violation.

M0 status: lenient stub — always exits 0. There are no implementation-driving
tests yet (geometry tests landed green with their impl in this same M0 scaffold,
which is the allowed bootstrap). Wire the real git-history logic in M1.
"""

from __future__ import annotations

import sys

# TODO(M1): implement the git-history test-precedes-impl check described above.


def main(argv: list[str]) -> int:
    print("test-order-guard: M0 stub (no enforcement yet) — OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
