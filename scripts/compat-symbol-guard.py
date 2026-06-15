#!/usr/bin/env python3
"""compat-symbol-guard — PyMuPDF baseline symbol disposition gate (PRD §7 / §17.2).

Enforces that **every** public symbol in the pinned PyMuPDF baseline carries an
explicit disposition in ``COMPAT.toml``. Per PRD §7, any PyMuPDF symbol not
dispositioned is ``out-of-scope`` by default and must raise
``PdfUnsupportedError`` (never ``AttributeError``) — so a baseline symbol that is
*absent* from ``COMPAT.toml`` means a piece of surface was silently un-tracked.
That is a hard failure (exit 1), forcing a deliberate disposition for any newly
surfaced API when the baseline evolves (PRD §17.2).

What it does
  1. Load ``COMPAT.toml`` — every ``[[symbol]]`` entry + its disposition.
  2. Load the checked-in baseline list ``compat/compat-baseline.txt`` (a snapshot;
     real PyMuPDF is not installed at CI time).
  3. FAIL (exit 1) if any baseline symbol has no entry in ``COMPAT.toml``.
  4. FAIL (exit 1) on COMPAT.toml integrity problems (bad/missing disposition,
     duplicate symbol).
  5. WARN (does not fail) if a COMPAT.toml symbol is not in the baseline list
     (stale entry), or if a symbol marked ``implemented`` cannot be found by name
     anywhere under ``python/`` (best-effort static check; the worktree has no
     compiled wheel so this is name-presence only).

Run from the repo root::

    python3 scripts/compat-symbol-guard.py        # exit 0 on success
    python3 scripts/compat-symbol-guard.py -v     # also print the warnings

Requires only the standard library (``tomllib``, Python 3.11+).
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
COMPAT = REPO_ROOT / "COMPAT.toml"
BASELINE = REPO_ROOT / "compat" / "compat-baseline.txt"
PYTHON_SRC = REPO_ROOT / "python"

VALID_DISPOSITIONS = {"implemented", "deferred", "out-of-scope"}


def load_baseline(path: Path) -> set[str]:
    """The flat set of pinned baseline symbol names (comments/blanks ignored)."""
    out: set[str] = set()
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        out.add(line)
    return out


def load_compat(path: Path) -> tuple[dict[str, str], list[str]]:
    """Returns ``(name -> disposition, errors)`` parsed from COMPAT.toml.

    ``errors`` collects integrity problems (missing/invalid disposition,
    duplicate ``name``) that must fail the guard.
    """
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    dispositions: dict[str, str] = {}
    errors: list[str] = []
    for entry in data.get("symbol", []):
        name = entry.get("name")
        disp = entry.get("disposition")
        if not name:
            errors.append(f"[[symbol]] entry missing 'name': {entry!r}")
            continue
        if name in dispositions:
            errors.append(f"duplicate symbol declared in COMPAT.toml: {name!r}")
            continue
        if disp not in VALID_DISPOSITIONS:
            errors.append(
                f"symbol {name!r} has invalid disposition {disp!r} "
                f"(want one of {sorted(VALID_DISPOSITIONS)})"
            )
        dispositions[name] = disp  # type: ignore[assignment]
    return dispositions, errors


def _source_symbol_index() -> set[str]:
    """The set of identifier-like tokens that appear in the python/ source.

    Best-effort: every ``def``/``class`` name, attribute/property name, and
    ``__all__`` entry, gathered by a light textual scan. Used only to *warn*
    about ``implemented`` symbols whose name is nowhere in source. The worktree
    has no compiled ``_core`` extension, so this is intentionally name-level.
    """
    import re

    names: set[str] = set()
    ident = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
    def_re = re.compile(r"^\s*(?:def|class)\s+([A-Za-z_][A-Za-z0-9_]*)")
    assign_re = re.compile(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=")
    if not PYTHON_SRC.exists():
        return names
    for py in PYTHON_SRC.rglob("*.py"):
        try:
            text = py.read_text(encoding="utf-8")
        except OSError:
            continue
        for line in text.splitlines():
            m = def_re.match(line)
            if m:
                names.add(m.group(1))
            m = assign_re.match(line)
            if m:
                names.add(m.group(1))
        # also index every bare identifier token (cheap superset, catches
        # method names referenced via aliases, string output options, etc.)
        names.update(ident.findall(text))
    return names


def _member(symbol: str) -> str:
    """The trailing member of ``Class.member`` (or the whole name)."""
    return symbol.rsplit(".", 1)[-1]


def check_implemented_in_source(
    dispositions: dict[str, str], baseline: set[str]
) -> list[str]:
    """Warnings for ``implemented`` symbols whose name is absent from source."""
    src = _source_symbol_index()
    if not src:
        return []
    warnings: list[str] = []
    for name, disp in dispositions.items():
        if disp != "implemented":
            continue
        member = _member(name)
        # dunder protocol methods + grouped constant-family rows are exempt;
        # they are not always literal tokens in the thin wrappers.
        if member.startswith("__") and member.endswith("__"):
            continue
        if member in src or name in src:
            continue
        warnings.append(name)
    return warnings


def main(argv: list[str]) -> int:
    verbose = any(a in ("-v", "--verbose") for a in argv)

    if not COMPAT.exists():
        print(f"compat-symbol-guard: FAIL — {COMPAT} not found", file=sys.stderr)
        return 1
    if not BASELINE.exists():
        print(f"compat-symbol-guard: FAIL — {BASELINE} not found", file=sys.stderr)
        return 1

    baseline = load_baseline(BASELINE)
    dispositions, errors = load_compat(COMPAT)

    # Hard failure: baseline symbols with no disposition.
    missing = sorted(baseline - set(dispositions))

    # Soft: COMPAT entries no longer in the baseline (stale surface).
    stale = sorted(set(dispositions) - baseline)

    # Soft: implemented-but-not-found-in-source.
    impl_warnings = check_implemented_in_source(dispositions, baseline)

    ok = not missing and not errors

    print("compat-symbol-guard — PyMuPDF baseline disposition gate")
    print(f"  baseline symbols : {len(baseline)}  ({BASELINE.name})")
    print(f"  dispositioned    : {len(dispositions)}  ({COMPAT.name})")
    counts = {d: sum(1 for v in dispositions.values() if v == d) for d in VALID_DISPOSITIONS}
    total = len(dispositions) or 1
    print(
        f"  implemented={counts['implemented']} "
        f"deferred={counts['deferred']} "
        f"out-of-scope={counts['out-of-scope']}  "
        f"coverage={100.0 * counts['implemented'] / total:.1f}%"
    )

    if errors:
        print(f"\n  COMPAT.toml integrity errors ({len(errors)}):", file=sys.stderr)
        for e in errors:
            print(f"    - {e}", file=sys.stderr)

    if missing:
        print(
            f"\n  FAIL: {len(missing)} baseline symbol(s) have no disposition "
            f"in {COMPAT.name}:",
            file=sys.stderr,
        )
        for m in missing:
            print(f"    - {m}", file=sys.stderr)
        print(
            "\n  Every baseline PyMuPDF symbol must be explicitly dispositioned "
            "(implemented / deferred / out-of-scope). Add the missing entries to "
            "COMPAT.toml (regenerate via scripts/_compat_catalog.py).",
            file=sys.stderr,
        )

    if stale:
        print(f"\n  WARN: {len(stale)} COMPAT.toml symbol(s) not in the baseline list:")
        if verbose:
            for s in stale:
                print(f"    - {s}")
        else:
            print("    (run with -v to list; regenerate the baseline if intended)")

    if impl_warnings:
        print(
            f"\n  WARN: {len(impl_warnings)} 'implemented' symbol(s) not found by "
            f"name under python/ (informational; no compiled wheel in worktree):"
        )
        if verbose:
            for w in impl_warnings:
                print(f"    - {w}")
        else:
            print("    (run with -v to list)")

    if ok:
        print("\n  OK — every baseline symbol is dispositioned.")
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
