#!/usr/bin/env python3
"""catalog-status-guard — COMPAT.toml ⇄ catalog-generator sync gate (PRD §7 / §9.5).

``COMPAT.toml`` and ``compat/compat-baseline.txt`` are *generated* artifacts:
the single source of truth is ``scripts/_compat_catalog.py``. They are committed
so the ``compat-symbol-guard`` can diff against them at CI time (real PyMuPDF is
not installed), which means a hand-edit of either file — or a change to the
generator that was not re-committed — silently desynchronises the disposition
matrix from its source.

This guard enforces that invariant deterministically (no git history / PR
context needed):

  1. Re-run the generator's renderers in-process and compare their output, byte
     for byte, against the committed ``COMPAT.toml`` and
     ``compat/compat-baseline.txt``. Any drift fails (exit 1) with a diff.
  2. Re-parse the committed ``COMPAT.toml`` and assert every ``[[symbol]]``
     carries a *valid* disposition (implemented / deferred / out-of-scope) and
     that the ``[meta]`` counters match the actual row tallies — i.e. the file
     is internally consistent, not just well-formed TOML.

Break either side (edit COMPAT.toml by hand, change the generator without
regenerating, or corrupt a disposition / meta count) and this guard exits 1.

Run from the repo root::

    python3 scripts/catalog-status-guard.py        # exit 0 on success

Requires only the standard library (``tomllib``, Python 3.11+).
"""

from __future__ import annotations

import difflib
import importlib.util
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
GENERATOR = REPO_ROOT / "scripts" / "_compat_catalog.py"
COMPAT_TOML = REPO_ROOT / "COMPAT.toml"
BASELINE_TXT = REPO_ROOT / "compat" / "compat-baseline.txt"

VALID_DISPOSITIONS = {"implemented", "deferred", "out-of-scope"}


def _load_generator():
    """Import scripts/_compat_catalog.py as a module (it is *data only*)."""
    spec = importlib.util.spec_from_file_location("_compat_catalog", GENERATOR)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load generator at {GENERATOR}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _diff(committed: str, expected: str, name: str) -> list[str]:
    return list(
        difflib.unified_diff(
            committed.splitlines(),
            expected.splitlines(),
            fromfile=f"{name} (committed)",
            tofile=f"{name} (regenerated)",
            lineterm="",
        )
    )


def check_sync(mod) -> list[str]:
    """Byte-for-byte: committed artifacts == generator output."""
    errors: list[str] = []
    for path, expected in (
        (COMPAT_TOML, mod.render_toml()),
        (BASELINE_TXT, mod.render_baseline()),
    ):
        if not path.exists():
            errors.append(f"{path} not found (regenerate via scripts/_compat_catalog.py)")
            continue
        committed = path.read_text(encoding="utf-8")
        if committed != expected:
            d = _diff(committed, expected, path.name)
            head = "\n".join(d[:40])
            more = "" if len(d) <= 40 else f"\n    ... (+{len(d) - 40} more diff lines)"
            errors.append(
                f"{path.name} is out of sync with scripts/_compat_catalog.py — "
                f"regenerate with `python3 scripts/_compat_catalog.py`:\n{head}{more}"
            )
    return errors


def check_integrity() -> list[str]:
    """Dispositions valid + [meta] counters match the actual row tallies."""
    errors: list[str] = []
    data = tomllib.loads(COMPAT_TOML.read_text(encoding="utf-8"))

    seen: set[str] = set()
    counts = {d: 0 for d in VALID_DISPOSITIONS}
    for entry in data.get("symbol", []):
        name = entry.get("name")
        disp = entry.get("disposition")
        if not name:
            errors.append(f"[[symbol]] entry missing 'name': {entry!r}")
            continue
        if name in seen:
            errors.append(f"duplicate symbol in COMPAT.toml: {name!r}")
        seen.add(name)
        if disp not in VALID_DISPOSITIONS:
            errors.append(
                f"symbol {name!r} has invalid disposition {disp!r} "
                f"(want one of {sorted(VALID_DISPOSITIONS)})"
            )
            continue
        counts[disp] += 1

    meta = data.get("meta", {})
    total = len(seen)
    expected_meta = {
        "total": total,
        "implemented": counts["implemented"],
        "deferred": counts["deferred"],
        "out_of_scope": counts["out-of-scope"],
    }
    for key, want in expected_meta.items():
        got = meta.get(key)
        if got != want:
            errors.append(
                f"[meta].{key} = {got!r} but the actual row tally is {want} "
                f"(COMPAT.toml is internally inconsistent)"
            )
    return errors


def main(argv: list[str]) -> int:
    if not GENERATOR.exists():
        print(
            f"catalog-status-guard: FAIL — generator not found at {GENERATOR}",
            file=sys.stderr,
        )
        return 1

    mod = _load_generator()
    errors = check_sync(mod)
    errors += check_integrity()

    print("catalog-status-guard — COMPAT.toml ⇄ generator sync gate")
    print(f"  generator : {GENERATOR.relative_to(REPO_ROOT)}")
    print(f"  artifacts : {COMPAT_TOML.name}, {BASELINE_TXT.relative_to(REPO_ROOT)}")

    if errors:
        print(f"\n  FAIL ({len(errors)} problem(s)):", file=sys.stderr)
        for e in errors:
            print(f"    - {e}", file=sys.stderr)
        return 1

    print("  OK — committed COMPAT.toml/baseline match the generator; "
          "dispositions + meta counters are consistent.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
