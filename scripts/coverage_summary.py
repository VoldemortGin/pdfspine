#!/usr/bin/env python3
"""Summarize combined LLVM/Python coverage; reject missing native counters.

This checks instrumentation, not a Rust percentage threshold. Python's real
fail_under gate remains the separate final ``coverage report`` command.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import os
import subprocess
from pathlib import Path


def digest(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def summarize(rust: dict, python: dict, lcov: str) -> dict:
    totals = {name: {"covered": 0, "count": 0} for name in ("lines", "branches")}
    crates: dict[str, dict] = {}
    for export in rust["data"]:
        for name, total in totals.items():
            for key in total:
                total[key] += export["totals"][name][key]
        for file in export["files"]:
            parts = Path(file["filename"]).parts
            if "crates" not in parts:
                continue
            crate = parts[parts.index("crates") + 1]
            counts = crates.setdefault(
                crate,
                {name: {"covered": 0, "count": 0} for name in totals},
            )
            for name in totals:
                for key in counts[name]:
                    counts[name][key] += file["summary"][name][key]

    bindings_lh = branch_count = branch_hits = 0
    in_bindings = False
    for line in lcov.splitlines():
        if line.startswith("SF:"):
            in_bindings = "py-bindings" in Path(line[3:]).parts
        elif line.startswith("LH:") and in_bindings:
            bindings_lh += int(line[3:])
        elif line.startswith("BRF:"):
            branch_count += int(line[4:])
        elif line.startswith("BRH:"):
            branch_hits += int(line[4:])
        elif line == "end_of_record":
            in_bindings = False

    errors = []
    if totals["branches"]["count"] <= 0 or branch_count <= 0:
        errors.append("Rust branch instrumentation missing from JSON or LCOV")
    if bindings_lh <= 0:
        errors.append("py-bindings LH is zero: combined native coverage is missing")
    return {
        "status": "invalid" if errors else "valid",
        "errors": errors,
        "rust": totals,
        "rust_crates": dict(sorted(crates.items())),
        "lcov": {"branches": branch_count, "covered_branches": branch_hits},
        "py_bindings_lh": bindings_lh,
        "python": python["totals"],
        "scope": "combined workspace Rust tests and instrumented Python tests",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust", type=Path, default=Path("coverage-rust.json"))
    parser.add_argument("--python", type=Path, default=Path("coverage-python.json"))
    parser.add_argument("--lcov", type=Path, default=Path("lcov.info"))
    parser.add_argument(
        "--rust-before-python",
        type=Path,
        default=Path("coverage-rust-before-python.json"),
    )
    parser.add_argument("--output", type=Path, default=Path("coverage-summary.json"))
    args = parser.parse_args()
    report = summarize(
        json.loads(args.rust.read_text()),
        json.loads(args.python.read_text()),
        args.lcov.read_text(),
    )
    report["source_commit"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], text=True
    ).strip()
    diff = subprocess.check_output(["git", "diff", "HEAD", "--binary"])
    report["tracked_source_dirty"] = bool(diff)
    report["tracked_diff_sha256"] = hashlib.sha256(diff).hexdigest()
    before = summarize(
        json.loads(args.rust_before_python.read_text()),
        {"totals": {}},
        args.lcov.read_text(),
    )["rust_crates"].get("py-bindings", {})
    after = report["rust_crates"].get("py-bindings", {})
    report["python_native_evidence"] = {
        "bindings_before_python": before,
        "bindings_after_python": after,
        "covered_delta": {
            name: after.get(name, {}).get("covered", 0)
            - before.get(name, {}).get("covered", 0)
            for name in ("lines", "branches")
        },
        "llvm_profile_file": os.environ.get("LLVM_PROFILE_FILE"),
        "toolchain": os.environ.get("RUSTUP_TOOLCHAIN"),
        "note": "LH is a degradation sentinel; zero delta alone does not disprove Python execution",
    }
    report["report_sha256"] = {
        str(path): digest(path)
        for path in (args.rust, args.rust_before_python, args.python, args.lcov)
    }
    # Record the actually importable module, not an arbitrary build-directory file.
    native = Path(importlib.import_module("pdfspine._core").__file__).resolve()
    report["native_extension"] = {"path": str(native), "sha256": digest(native)}
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 1 if report["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
