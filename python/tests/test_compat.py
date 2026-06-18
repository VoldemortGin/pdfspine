"""Tests for the PyMuPDF compatibility map (COMPAT.toml + compat-symbol-guard).

These operate purely on the checked-in artifacts and the ``python/`` source —
they do NOT import the compiled ``pdfspine`` module (the compat worktree has no
built wheel). Stdlib only (``tomllib``, Python 3.11+).

Runnable two ways::

    python3 -m pytest python/tests/test_compat.py -q
    python3 python/tests/test_compat.py        # falls back to a plain runner
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tomllib
from collections import Counter
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
COMPAT_TOML = REPO_ROOT / "COMPAT.toml"
BASELINE_TXT = REPO_ROOT / "compat" / "compat-baseline.txt"
GUARD = REPO_ROOT / "scripts" / "compat-symbol-guard.py"

VALID_DISPOSITIONS = {"implemented", "deferred", "out-of-scope"}


def _load_compat() -> dict:
    return tomllib.loads(COMPAT_TOML.read_text(encoding="utf-8"))


def _load_baseline() -> set[str]:
    out: set[str] = set()
    for raw in BASELINE_TXT.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


# ---------------------------------------------------------------------------
# (a) the guard logic exits 0 — every baseline symbol is dispositioned
# ---------------------------------------------------------------------------
def test_guard_exits_zero() -> None:
    """compat-symbol-guard returns exit 0 (full baseline dispositioned)."""
    proc = subprocess.run(
        [sys.executable, str(GUARD)],
        capture_output=True,
        text=True,
        cwd=str(REPO_ROOT),
    )
    assert proc.returncode == 0, (
        f"guard failed (exit {proc.returncode}):\n{proc.stdout}\n{proc.stderr}"
    )


def test_guard_module_main_returns_zero() -> None:
    """Invoke the guard's main() in-process and assert it returns 0."""
    spec = importlib.util.spec_from_file_location("_compat_guard", GUARD)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    assert mod.main([]) == 0


def test_every_baseline_symbol_is_dispositioned() -> None:
    """No baseline symbol may be missing from COMPAT.toml (PRD §7 / §17.2)."""
    baseline = _load_baseline()
    declared = {s["name"] for s in _load_compat()["symbol"]}
    missing = sorted(baseline - declared)
    assert not missing, f"baseline symbols missing a disposition: {missing}"


# ---------------------------------------------------------------------------
# (b) COMPAT.toml parses; dispositions are valid; counts are self-consistent
# ---------------------------------------------------------------------------
def test_compat_toml_parses_and_is_well_formed() -> None:
    data = _load_compat()
    symbols = data["symbol"]
    assert symbols, "COMPAT.toml declares no symbols"

    names = [s["name"] for s in symbols]
    dupes = sorted({n for n in names if names.count(n) > 1})
    assert not dupes, f"duplicate symbols in COMPAT.toml: {dupes}"

    for s in symbols:
        assert "name" in s and s["name"], f"entry missing name: {s!r}"
        assert "group" in s and s["group"], f"{s['name']} missing group"
        assert (
            s["disposition"] in VALID_DISPOSITIONS
        ), f"{s['name']} has invalid disposition {s.get('disposition')!r}"


def test_compat_meta_counts_match_entries() -> None:
    data = _load_compat()
    symbols = data["symbol"]
    meta = data["meta"]
    counts = Counter(s["disposition"] for s in symbols)

    assert meta["total"] == len(symbols)
    assert meta["implemented"] == counts["implemented"]
    assert meta["deferred"] == counts["deferred"]
    assert meta["out_of_scope"] == counts["out-of-scope"]
    assert meta["baseline"] == "1.24.x"


def test_baseline_and_compat_are_in_lockstep() -> None:
    """The baseline snapshot and COMPAT.toml cover exactly the same symbols."""
    baseline = _load_baseline()
    declared = {s["name"] for s in _load_compat()["symbol"]}
    assert baseline == declared, {
        "in_baseline_not_compat": sorted(baseline - declared),
        "in_compat_not_baseline": sorted(declared - baseline),
    }


# ---------------------------------------------------------------------------
# (c) compute + report compat coverage %
# ---------------------------------------------------------------------------
def compat_coverage() -> tuple[int, int, float, Counter]:
    symbols = _load_compat()["symbol"]
    counts = Counter(s["disposition"] for s in symbols)
    total = len(symbols)
    implemented = counts["implemented"]
    pct = 100.0 * implemented / total if total else 0.0
    return implemented, total, pct, counts


def test_coverage_is_reported_and_sane() -> None:
    implemented, total, pct, counts = compat_coverage()
    print(
        f"\nCOMPAT coverage: {implemented}/{total} = {pct:.1f}% implemented "
        f"(deferred={counts['deferred']}, out-of-scope={counts['out-of-scope']})"
    )
    assert total > 0
    assert 0.0 <= pct <= 100.0
    assert implemented + counts["deferred"] + counts["out-of-scope"] == total


# ---------------------------------------------------------------------------
# Plain runner fallback when pytest is unavailable
# ---------------------------------------------------------------------------
def _run_plain() -> int:
    tests = [
        test_guard_exits_zero,
        test_guard_module_main_returns_zero,
        test_every_baseline_symbol_is_dispositioned,
        test_compat_toml_parses_and_is_well_formed,
        test_compat_meta_counts_match_entries,
        test_baseline_and_compat_are_in_lockstep,
        test_coverage_is_reported_and_sane,
    ]
    failures = 0
    for t in tests:
        try:
            t()
            print(f"PASS {t.__name__}")
        except AssertionError as exc:  # noqa: PERF203
            failures += 1
            print(f"FAIL {t.__name__}: {exc}")
    implemented, total, pct, counts = compat_coverage()
    print(
        f"\nCOMPAT coverage: {implemented}/{total} = {pct:.1f}% implemented "
        f"(deferred={counts['deferred']}, out-of-scope={counts['out-of-scope']})"
    )
    print(f"\n{len(tests) - failures}/{len(tests)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(_run_plain())
