"""Verify the API reference covers the FULL public surface (PRD-NEXT P4-4).

Acceptance: every name in ``pdfspine.__all__`` is documented somewhere under
``docs/reference/``. A name counts as documented if it is either

  * rendered by an mkdocstrings directive ``::: pdfspine.<name>``  (classes /
    functions — methods & properties then auto-render from docstrings), or
  * listed by name on a reference page (the ~250 module-level constants and the
    re-exported value singletons live in ``constants.md``, generated from the
    installed package by ``scripts/gen_docs_constants.py``).

Exit non-zero (and print the gaps) if anything is missing.

    .venv/bin/python scripts/check_docs_coverage.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import pdfspine

ROOT = Path(__file__).resolve().parents[1]
REF = ROOT / "docs" / "reference"

# Directives like `::: pdfspine.Document` (optionally `pdfspine.constants`).
_DIRECTIVE = re.compile(r"^:::\s+pdfspine(?:\.([A-Za-z_][A-Za-z0-9_]*))?\s*$")


def main() -> int:
    text_by_file: dict[Path, str] = {
        p: p.read_text(encoding="utf-8") for p in sorted(REF.glob("*.md"))
    }
    all_text = "\n".join(text_by_file.values())

    # Names rendered via an `::: pdfspine.<name>` directive.
    rendered: set[str] = set()
    for content in text_by_file.values():
        for line in content.splitlines():
            m = _DIRECTIVE.match(line.strip())
            if m and m.group(1):
                rendered.add(m.group(1))

    expected = list(pdfspine.__all__)
    documented: dict[str, str] = {}
    missing: list[str] = []

    for name in expected:
        if name in rendered:
            documented[name] = "directive"
        elif re.search(rf"`{re.escape(name)}`", all_text):
            # Listed verbatim in a table/prose (constants, value singletons).
            documented[name] = "listed"
        else:
            missing.append(name)

    total = len(expected)
    ok = len(documented)
    by_directive = sum(1 for v in documented.values() if v == "directive")
    by_listing = ok - by_directive

    print(f"public symbols (pdfspine.__all__): {total}")
    print(f"  documented: {ok}/{total}")
    print(f"    via ::: directive: {by_directive}")
    print(f"    via constants/listing: {by_listing}")

    if missing:
        print(f"\nMISSING ({len(missing)}):")
        for name in missing:
            print(f"  - {name}")
        return 1

    print("\nALL public symbols documented.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
