#!/usr/bin/env python3
"""Compare two isolated pdfspine installations on one PDF, using fresh processes.

Example (all paths must be absolute)::

    python conformance/bench_glyph_geometry.py --pdf /path/document.pdf \
        --baseline-python /tmp/base/bin/python --current-python /tmp/new/bin/python \
        --output /tmp/glyph-performance.json --runs 7

Each mode/policy gets one discarded warmup per build, then alternating AB/BA
paired repetitions. RSS is the process high-water mark, including Python/native
imports. Timing starts after import and covers open, all pages, and close.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import resource
import statistics
import subprocess
import sys
import time
from pathlib import Path


def worker(pdf: Path, mode: str, policy: str) -> dict[str, object]:
    import pdfspine

    retained: list[object] = []
    started = time.perf_counter()
    with pdfspine.open(str(pdf)) as doc:
        pages = doc.page_count
        for page_index in range(pages):
            result = doc[page_index].get_text(mode)
            if policy == "retain":
                retained.append(result)
            del result
    elapsed = time.perf_counter() - started
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    # Darwin reports bytes; Linux reports KiB.
    rss_bytes = rss if sys.platform == "darwin" else rss * 1024
    return {
        "elapsed_s": elapsed,
        "peak_rss_bytes": rss_bytes,
        "pages": pages,
        "module": str(Path(pdfspine.__file__).resolve()),
        "python": platform.python_version(),
    }


def summarize(rows: list[dict[str, object]]) -> dict[str, object]:
    summary: dict[str, object] = {}
    for key in ("elapsed_s", "peak_rss_bytes"):
        values = [float(row[key]) for row in rows]  # type: ignore[arg-type]
        median = statistics.median(values)
        summary[key] = {
            "median": median,
            "min": min(values),
            "max": max(values),
            "mad": statistics.median(abs(value - median) for value in values),
        }
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, required=True)
    parser.add_argument("--baseline-python", type=Path)
    parser.add_argument("--current-python", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument(
        "--modes",
        nargs="+",
        choices=("text", "dict", "rawdict"),
        default=["text", "dict", "rawdict"],
    )
    parser.add_argument(
        "--policies",
        nargs="+",
        choices=("stream", "retain"),
        default=["stream", "retain"],
    )
    parser.add_argument("--worker", choices=("text", "dict", "rawdict"))
    parser.add_argument("--policy", choices=("stream", "retain"), default="stream")
    args = parser.parse_args()
    if args.worker:
        print(json.dumps(worker(args.pdf.resolve(), args.worker, args.policy)))
        return
    if (
        not args.baseline_python
        or not args.current_python
        or not args.output
        or args.runs < 1
    ):
        parser.error("both Python paths, --output, and positive --runs are required")

    env = dict(os.environ)
    for key in ("PYTHONPATH", "VIRTUAL_ENV", "CONDA_PREFIX"):
        env.pop(key, None)
    script = Path(__file__).resolve()
    interpreters = {"baseline": args.baseline_python, "current": args.current_python}
    data: dict[str, object] = {
        "machine": platform.platform(),
        "processor": platform.processor(),
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "input": str(args.pdf.resolve()),
        "input_bytes": args.pdf.stat().st_size,
        "sha256": hashlib.sha256(args.pdf.read_bytes()).hexdigest(),
        "runs": args.runs,
        "cells": [],
    }
    cells: list[dict[str, object]] = []
    data["cells"] = cells
    for policy in args.policies:
        for mode in args.modes:
            rows: dict[str, list[dict[str, object]]] = {
                name: [] for name in interpreters
            }
            warmups: dict[str, object] = {}
            order: list[str] = []
            for repetition in range(-1, args.runs):
                names = (
                    ("baseline", "current")
                    if repetition % 2 == 0
                    else ("current", "baseline")
                )
                for name in names:
                    completed = subprocess.run(
                        [
                            str(interpreters[name]),
                            str(script),
                            "--pdf",
                            str(args.pdf.resolve()),
                            "--worker",
                            mode,
                            "--policy",
                            policy,
                        ],
                        capture_output=True,
                        text=True,
                        check=True,
                        timeout=600,
                        env=env,
                        cwd="/tmp",
                    )
                    row = json.loads(completed.stdout)
                    if repetition == -1:
                        warmups[name] = row
                    else:
                        rows[name].append(row)
                        order.append(name)
                    print(
                        f"{policy}/{mode} {name} repetition={repetition}: "
                        f"{row['elapsed_s']:.4f}s {row['peak_rss_bytes'] / 1048576:.2f}MiB",
                        file=sys.stderr,
                        flush=True,
                    )
            cell = {
                "policy": policy,
                "mode": mode,
                "warmups": warmups,
                "order": order,
                "samples": rows,
                "summary": {name: summarize(samples) for name, samples in rows.items()},
            }
            cells.append(cell)
            args.output.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
