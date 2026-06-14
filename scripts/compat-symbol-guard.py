#!/usr/bin/env python3
"""compat-symbol-guard — PyMuPDF baseline symbol disposition gate (PRD §7, D13).

The real guard fails CI if any PyMuPDF *public* symbol present in the pinned
baseline is **absent** from `COMPAT.toml` — forcing an explicit disposition
(implemented / deferred / out-of-scope -> raises `PdfUnsupportedError`) for every
surface, so no symbol silently degrades to `AttributeError`.

Algorithm sketch:
  - Load the pinned PyMuPDF baseline symbol list (a committed snapshot, e.g.
    `compat/pymupdf-baseline-<version>.txt`, generated clean-room by enumerating
    `dir(fitz)` + class members of the documented public API; never imported
    from PyMuPDF source at CI time).
  - Parse `COMPAT.toml` for every declared symbol + its status.
  - Diff: any baseline symbol missing a disposition -> failure (list them).
  - Optionally warn on COMPAT.toml symbols no longer in the baseline.

M0 status: lenient stub — `COMPAT.toml` and the baseline snapshot do not exist
yet (the shim is built in M5). Always exits 0. Wire enforcement when the shim
work begins.
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
COMPAT = REPO_ROOT / "COMPAT.toml"


def main(argv: list[str]) -> int:
    if not COMPAT.exists():
        print("compat-symbol-guard: M0 stub — COMPAT.toml not present yet (shim is M5) — OK")
        return 0
    # TODO(M5): diff the pinned PyMuPDF baseline against COMPAT.toml dispositions.
    print("compat-symbol-guard: M0 stub (no enforcement yet) — OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
