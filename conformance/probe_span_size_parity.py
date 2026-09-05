#!/usr/bin/env python3
"""Compare structured span sizes with a separately installed PyMuPDF oracle.

Run after freezing the span-layout baseline. Engine workers run in separate
processes and write temporary JSONL; only aggregate measurements belong in git.
Matching uses exact Unicode and a bidirectionally unique origin match within
0.01 pt, not span indices or bbox metrics. Whitespace is excluded because the
engines synthesize spaces differently. Oracle /Rotate origins are mapped into
the displayed page frame before matching.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import itertools
import json
import math
import os
import statistics
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


def digest(value: Any) -> str:
    def binary(item: Any) -> dict[str, Any]:
        if isinstance(item, bytes):
            return {
                "__bytes_sha256__": hashlib.sha256(item).hexdigest(),
                "length": len(item),
            }
        raise TypeError(f"unsupported hash value: {type(item).__name__}")

    payload = json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False, default=binary
    )
    return hashlib.sha256(payload.encode()).hexdigest()


def spans(data: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        span
        for block in data.get("blocks", [])
        if block.get("type") == 0
        for line in block["lines"]
        for span in line["spans"]
    ]


def extract_worker(args: argparse.Namespace) -> None:
    engine = importlib.import_module(args.worker)
    paths = [
        Path(line) for line in args.corpus.read_text().splitlines() if line.strip()
    ]
    with args.output.open("w", encoding="utf-8") as output:
        for index, path in enumerate(paths):
            record: dict[str, Any] = {
                "index": index,
                "path": str(path),
                "pages": [],
                "error": None,
            }
            try:
                with engine.open(str(path)) as document:
                    record["page_count"] = len(document)
                    for page_index in range(min(len(document), args.max_pages)):
                        page = document[page_index]
                        raw = page.get_text("rawdict")
                        page_spans = spans(raw)
                        rows = []
                        invalid = 0
                        alias_mismatches = 0
                        for span in page_spans:
                            public = float(span["size"])
                            declared = float(span.get("declared_size", public))
                            rendered = float(span.get("rendered_size", public))
                            alias_mismatches += int(public != rendered)
                            for char in span["chars"]:
                                if char["c"].isspace() or char.get("synthetic", False):
                                    continue
                                x, y = char["origin"]
                                if args.worker == "fitz" and page.rotation:
                                    point = engine.Point(x, y) * page.rotation_matrix
                                    x, y = point.x, point.y
                                if not all(
                                    math.isfinite(v)
                                    for v in (x, y, public, declared, rendered)
                                ):
                                    invalid += 1
                                    continue
                                rows.append(
                                    [
                                        char["c"],
                                        x,
                                        y,
                                        public,
                                        declared,
                                        rendered,
                                        span["font"],
                                    ]
                                )
                        result: dict[str, Any] = {
                            "index": page_index,
                            "chars": rows,
                            "invalid": invalid,
                            "size_rendered_mismatches": alias_mismatches,
                        }
                        if args.worker == "pdfspine":
                            result["text_sha256"] = digest(page.get_text("text"))
                            result["words_sha256"] = digest(page.get_text("words"))
                            # Removing only a text span's public size must make
                            # baseline/candidate geometry and structure identical.
                            for span in page_spans:
                                del span["size"]
                            result["other_rawdict_sha256"] = digest(raw)
                            # Check all four public structured formats on the
                            # first sampled page of every document.
                            if page_index == 0:
                                sizes = [
                                    row["size"]
                                    for row in spans(page.get_text("rawdict"))
                                ]
                                consistent = True
                                for mode in ("dict", "json", "rawjson"):
                                    formatted = page.get_text(mode)
                                    if isinstance(formatted, str):
                                        formatted = json.loads(formatted)
                                    actual = [row["size"] for row in spans(formatted)]
                                    consistent &= len(actual) == len(sizes) and all(
                                        math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-6)
                                        for a, b in zip(actual, sizes)
                                    )
                                result["four_formats_consistent"] = consistent
                        record["pages"].append(result)
            except Exception as error:
                record["error"] = f"{type(error).__name__}: {error}"
            output.write(json.dumps(record, allow_nan=False) + "\n")
            output.flush()


def unique_matches(ours: list[list[Any]], oracle: list[list[Any]], tolerance: float):
    grid: dict[tuple[str, int, int], list[int]] = defaultdict(list)
    for index, row in enumerate(oracle):
        grid[
            (row[0], math.floor(row[1] / tolerance), math.floor(row[2] / tolerance))
        ].append(index)
    candidates: list[list[int]] = []
    uses: Counter[int] = Counter()
    for row in ours:
        gx, gy = math.floor(row[1] / tolerance), math.floor(row[2] / tolerance)
        found = [
            index
            for dx in (-1, 0, 1)
            for dy in (-1, 0, 1)
            for index in grid.get((row[0], gx + dx, gy + dy), [])
            if abs(row[1] - oracle[index][1]) <= tolerance
            and abs(row[2] - oracle[index][2]) <= tolerance
        ]
        candidates.append(found)
        uses.update(found)
    pairs = [
        (index, found[0])
        for index, found in enumerate(candidates)
        if len(found) == 1 and uses[found[0]] == 1
    ]
    ambiguous = sum(
        bool(found) and (len(found) != 1 or uses[found[0]] != 1) for found in candidates
    )
    return pairs, ambiguous


def errors_summary(values: list[float]) -> dict[str, float | int | None]:
    if not values:
        return {"count": 0, "mean": None, "median": None, "p95": None, "max": None}
    ordered = sorted(values)
    return {
        "count": len(values),
        "mean": statistics.fmean(values),
        "median": statistics.median(ordered),
        "p95": ordered[math.ceil(0.95 * len(ordered)) - 1],
        "max": ordered[-1],
    }


def compare(args: argparse.Namespace) -> dict[str, Any]:
    counts: Counter[str] = Counter()
    absolute: dict[str, list[float]] = {"baseline": [], "candidate": []}
    relative: dict[str, list[float]] = {"baseline": [], "candidate": []}
    documents = []
    exceptions = []
    worst: list[dict[str, Any]] = []
    files = [
        args.work_dir / f"{name}.jsonl" for name in ("baseline", "candidate", "oracle")
    ]
    with files[0].open() as bf, files[1].open() as cf, files[2].open() as of:
        for lines in itertools.zip_longest(bf, cf, of):
            if any(line is None for line in lines):
                raise RuntimeError(
                    "worker files contain different numbers of documents"
                )
            base, candidate, oracle = [json.loads(line) for line in lines]
            if len({row["path"] for row in (base, candidate, oracle)}) != 1:
                raise RuntimeError("worker corpus order differs")
            counts["documents"] += 1
            if any(row["error"] for row in (base, candidate, oracle)):
                exceptions.append(
                    {
                        "path": base["path"],
                        "errors": [row["error"] for row in (base, candidate, oracle)],
                    }
                )
                continue
            if len({len(row["pages"]) for row in (base, candidate, oracle)}) != 1:
                exceptions.append(
                    {"path": base["path"], "error": "sampled page counts differ"}
                )
                continue
            doc_counts: Counter[str] = Counter()
            doc_errors = {"baseline": [], "candidate": []}
            for bp, cp, op in zip(base["pages"], candidate["pages"], oracle["pages"]):
                counts["pages"] += 1
                for key in ("text_sha256", "words_sha256", "other_rawdict_sha256"):
                    if bp[key] != cp[key]:
                        counts[f"regression_{key}"] += 1
                if cp.get("four_formats_consistent") is False:
                    counts["four_format_failures"] += 1
                counts["candidate_size_rendered_mismatches"] += cp[
                    "size_rendered_mismatches"
                ]
                ours, reference = cp["chars"], op["chars"]
                counts["candidate_chars"] += len(ours)
                counts["oracle_chars"] += len(reference)
                counts["invalid_chars"] += sum(page["invalid"] for page in (bp, cp, op))
                if len(bp["chars"]) != len(ours) or any(
                    left[:3] != right[:3] for left, right in zip(bp["chars"], ours)
                ):
                    counts["character_alignment_regressions"] += 1
                    continue
                pairs, ambiguous = unique_matches(
                    ours, reference, args.origin_tolerance
                )
                counts["ambiguous_candidate_chars"] += ambiguous
                counts["matched_chars"] += len(pairs)
                doc_counts["matched"] += len(pairs)
                doc_counts["candidate_chars"] += len(ours)
                doc_counts["oracle_chars"] += len(reference)
                for oi, ri in pairs:
                    before, after, expected = (
                        bp["chars"][oi][3],
                        ours[oi][3],
                        reference[ri][3],
                    )
                    eb, ec = abs(before - expected), abs(after - expected)
                    tolerance = max(1e-4, abs(expected) * 1e-5)
                    for name, error in (("baseline", eb), ("candidate", ec)):
                        absolute[name].append(error)
                        doc_errors[name].append(error)
                        if abs(expected) > 1e-12:
                            relative[name].append(error / abs(expected))
                        counts[f"{name}_within_tolerance"] += error <= tolerance
                    counts["strict_tolerance_lost"] += eb <= tolerance < ec
                    counts["point01_tolerance_lost"] += eb <= 0.01 < ec
                    counts["baseline_within_point01"] += eb <= 0.01
                    counts["candidate_within_point01"] += ec <= 0.01
                    outcome = (
                        "improved"
                        if eb - ec > tolerance
                        else "worsened"
                        if ec - eb > tolerance
                        else "unchanged"
                    )
                    counts[outcome] += 1
                    doc_counts[outcome] += 1
                    if outcome == "worsened":
                        worst.append(
                            {
                                "path": candidate["path"],
                                "page": cp["index"],
                                "char": ours[oi][0],
                                "origin": ours[oi][1:3],
                                "font": ours[oi][6],
                                "oracle_font": reference[ri][6],
                                "before": before,
                                "after": after,
                                "oracle": expected,
                                "extra_error": ec - eb,
                            }
                        )
                        worst.sort(key=lambda row: row["extra_error"], reverse=True)
                        del worst[20:]
            documents.append(
                {
                    "path": base["path"],
                    "counts": dict(doc_counts),
                    "absolute_error": {
                        name: errors_summary(values)
                        for name, values in doc_errors.items()
                    },
                }
            )
    return {
        "counts": dict(counts),
        "origin_tolerance_pt": args.origin_tolerance,
        "candidate_coverage": counts["matched_chars"]
        / max(1, counts["candidate_chars"]),
        "oracle_coverage": counts["matched_chars"] / max(1, counts["oracle_chars"]),
        "absolute_error_pt": {
            name: errors_summary(values) for name, values in absolute.items()
        },
        "relative_error": {
            name: errors_summary(values) for name, values in relative.items()
        },
        "exceptions": exceptions,
        "worst_worsened": worst,
        "documents": documents,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--max-pages", type=int, default=20)
    parser.add_argument("--origin-tolerance", type=float, default=0.01)
    parser.add_argument("--worker", choices=("pdfspine", "fitz"))
    parser.add_argument("--baseline-python", type=Path)
    parser.add_argument("--candidate-python", type=Path)
    parser.add_argument("--oracle-python", type=Path)
    parser.add_argument("--work-dir", type=Path)
    parser.add_argument("--reuse-workers", action="store_true")
    args = parser.parse_args()
    if args.worker:
        extract_worker(args)
        return
    if not args.work_dir or not all(
        (args.baseline_python, args.candidate_python, args.oracle_python)
    ):
        parser.error("all three interpreters and --work-dir are required")
    if args.origin_tolerance <= 0 or args.max_pages <= 0:
        parser.error("tolerance and page limit must be positive")
    args.work_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    for key in ("PYTHONPATH", "VIRTUAL_ENV", "CONDA_PREFIX"):
        env.pop(key, None)
    if not args.reuse_workers:
        for name, interpreter, engine in (
            ("baseline", args.baseline_python, "pdfspine"),
            ("candidate", args.candidate_python, "pdfspine"),
            ("oracle", args.oracle_python, "fitz"),
        ):
            print(f"Extracting {name}", file=sys.stderr, flush=True)
            # Preserve the venv launcher path: resolving its python symlink
            # would silently run the base interpreter without site-packages.
            subprocess.run(
                [
                    str(interpreter.absolute()),
                    str(Path(__file__).resolve()),
                    "--worker",
                    engine,
                    "--corpus",
                    str(args.corpus.resolve()),
                    "--output",
                    str((args.work_dir / f"{name}.jsonl").resolve()),
                    "--max-pages",
                    str(args.max_pages),
                ],
                check=True,
                env=env,
                cwd="/tmp",
            )
    result = compare(args)
    result["corpus_sha256"] = hashlib.sha256(args.corpus.read_bytes()).hexdigest()
    result["max_pages"] = args.max_pages
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(
        json.dumps(
            {
                key: value
                for key, value in result.items()
                if key not in ("documents", "worst_worsened")
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
