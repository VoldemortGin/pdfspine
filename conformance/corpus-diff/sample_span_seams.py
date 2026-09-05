#!/usr/bin/env python3
"""Measure glyph geometry at seams that the current layout kept in one span."""

from __future__ import annotations

import argparse
import hashlib
import heapq
import json
import math
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, cast

import pdfspine

HERE = Path(__file__).resolve().parent
CUTS = (0.001, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2)
QUANTILES = (0.5, 0.9, 0.95, 0.99, 0.999)
TOP_N = 40
CHAR_FIELDS = (
    "c",
    "origin",
    "bbox",
    "matrix",
    "quad",
    "rendered_size",
    "seq",
    "synthetic",
)


def quantile(values: list[float], q: float) -> float | None:
    """Return the nearest-rank quantile of values."""
    if not values:
        return None
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, math.ceil(q * len(ordered)) - 1)]


def direction(char: dict[str, Any], line: dict[str, Any]) -> tuple[float, float] | None:
    """Reconstruct the direction used by DevGlyph for horizontal/vertical text."""
    x, y = line["dir"] if line.get("wmode", 0) == 1 else char["matrix"][:2]
    length = math.hypot(x, y)
    return (x / length, y / length) if length > 1e-12 else None


def seam_kind(left: str, right: str) -> str:
    """Classify the characters surrounding a seam for review strata."""
    if left.isspace() or right.isspace():
        return "space"
    if left.isalpha() and right.isalpha():
        return "letter_letter"
    if left.isdigit() and right.isdigit():
        return "digit_digit"
    if left.isalnum() and right.isalnum():
        return "letter_digit"
    return "punctuation"


def push_top(
    heap: list[tuple[float, int, dict[str, Any]]],
    value: float,
    serial: int,
    record: dict[str, Any],
) -> None:
    """Keep the TOP_N largest records in a bounded heap."""
    item = (value, serial, record)
    if len(heap) < TOP_N:
        heapq.heappush(heap, item)
    elif value > heap[0][0]:
        heapq.heapreplace(heap, item)


def main() -> None:
    """Extract rawdict from corpus.txt and write aggregate seam measurements."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", type=Path, default=HERE / "corpus.txt")
    parser.add_argument("--output", type=Path, default=HERE / "span-seams.json")
    parser.add_argument("--max-pages", type=int, default=20)
    args = parser.parse_args()

    paths = [
        Path(line) for line in args.corpus.read_text().splitlines() if line.strip()
    ]
    values: dict[str, list[float]] = {"linear": [], "dir_dot": [], "baseline": []}
    affected_spans: dict[str, dict[float, set[str]]] = {
        metric: {cut: set() for cut in CUTS} for metric in ("linear", "baseline")
    }
    affected_docs: dict[str, dict[float, set[int]]] = {
        metric: {cut: set() for cut in CUTS} for metric in ("linear", "baseline")
    }
    cut_seams: dict[str, Counter[float]] = {
        metric: Counter() for metric in ("linear", "baseline")
    }
    top: dict[str, list[tuple[float, int, dict[str, Any]]]] = {
        "linear": [],
        "baseline": [],
        "dir_change": [],
    }
    boundary: dict[str, dict[float, dict[str, tuple[float, dict[str, Any]] | None]]] = {
        metric: {cut: {"below": None, "above": None} for cut in CUTS}
        for metric in ("linear", "baseline")
    }
    font_stats: dict[str, Counter[str]] = defaultdict(Counter)
    document_stats: dict[str, Counter[str]] = defaultdict(Counter)
    kinds: Counter[str] = Counter()
    errors: list[dict[str, str]] = []
    degenerate = 0
    seam_count = 0
    span_count = 0
    serial = 0
    page_char_projection: dict[str, dict[str, int | str]] = {}
    total_chars = 0

    for doc_index, path in enumerate(paths):
        try:
            document = pdfspine.open(path)
            for page_index, page in enumerate(document):
                if page_index >= args.max_pages:
                    break
                raw = cast(dict[str, Any], page.get_text("rawdict"))
                blocks: list[dict[str, Any]] = raw.get("blocks", [])
                projected_chars = [
                    {field: char.get(field) for field in CHAR_FIELDS}
                    for block in blocks
                    if block.get("type", 0) == 0
                    for line in block.get("lines", [])
                    for span in line.get("spans", [])
                    for char in span.get("chars", [])
                ]
                projection_bytes = json.dumps(
                    projected_chars,
                    sort_keys=True,
                    separators=(",", ":"),
                    ensure_ascii=False,
                ).encode()
                page_char_projection[f"{doc_index}:{page_index}"] = {
                    "chars": len(projected_chars),
                    "sha256": hashlib.sha256(projection_bytes).hexdigest(),
                }
                total_chars += len(projected_chars)
                for block_index, block in enumerate(blocks):
                    if block.get("type", 0) != 0:
                        continue
                    for line_index, line in enumerate(block.get("lines", [])):
                        for span_index, span in enumerate(line.get("spans", [])):
                            glyphs: list[dict[str, Any]] = []
                            for char in span.get("chars", []):
                                if char.get("synthetic"):
                                    continue
                                if glyphs and char.get("seq") == glyphs[-1].get("seq"):
                                    continue
                                glyphs.append(char)
                            if not glyphs:
                                continue
                            span_count += 1
                            document_stats[str(path)]["spans"] += 1
                            if len(glyphs) < 2:
                                continue
                            span_id = f"{doc_index}:{page_index}:{block_index}:{line_index}:{span_index}"
                            text = "".join(str(char.get("c", "")) for char in glyphs)
                            for glyph_index, (left, right) in enumerate(
                                zip(glyphs, glyphs[1:])
                            ):
                                scale = max(
                                    float(left["rendered_size"]),
                                    float(right["rendered_size"]),
                                )
                                left_dir, right_dir = (
                                    direction(left, line),
                                    direction(right, line),
                                )
                                if (
                                    not math.isfinite(scale)
                                    or scale <= 1e-12
                                    or left_dir is None
                                    or right_dir is None
                                ):
                                    degenerate += 1
                                    continue
                                left_matrix, right_matrix = (
                                    left["matrix"],
                                    right["matrix"],
                                )
                                linear = (
                                    max(
                                        abs(
                                            float(left_matrix[i])
                                            - float(right_matrix[i])
                                        )
                                        for i in range(4)
                                    )
                                    / scale
                                )
                                dot = max(
                                    -1.0,
                                    min(
                                        1.0,
                                        left_dir[0] * right_dir[0]
                                        + left_dir[1] * right_dir[1],
                                    ),
                                )
                                dx = float(right["origin"][0]) - float(
                                    left["origin"][0]
                                )
                                dy = float(right["origin"][1]) - float(
                                    left["origin"][1]
                                )
                                baseline = (
                                    abs(dx * -left_dir[1] + dy * left_dir[0]) / scale
                                )
                                metrics = {
                                    "linear": linear,
                                    "dir_dot": dot,
                                    "baseline": baseline,
                                }
                                for metric, value in metrics.items():
                                    values[metric].append(value)
                                kind = seam_kind(
                                    str(left.get("c", "")), str(right.get("c", ""))
                                )
                                kinds[kind] += 1
                                font = str(span.get("font", ""))
                                font_stats[font]["seams"] += 1
                                font_stats[font]["linear_nonzero"] += linear > 0
                                font_stats[font]["baseline_nonzero"] += baseline > 0
                                for metric, value in (
                                    ("linear", linear),
                                    ("baseline", baseline),
                                ):
                                    for cut in CUTS:
                                        if value > cut:
                                            cut_seams[metric][cut] += 1
                                            affected_spans[metric][cut].add(span_id)
                                            affected_docs[metric][cut].add(doc_index)
                                record = {
                                    "document": str(path),
                                    "page": page_index,
                                    "block": block_index,
                                    "line": line_index,
                                    "span": span_index,
                                    "glyph": glyph_index,
                                    "font": font,
                                    "wmode": line.get("wmode", 0),
                                    "kind": kind,
                                    "context": text[
                                        max(0, glyph_index - 8) : glyph_index + 10
                                    ],
                                    "left": {
                                        key: left[key]
                                        for key in (
                                            "c",
                                            "matrix",
                                            "origin",
                                            "rendered_size",
                                            "seq",
                                        )
                                    },
                                    "right": {
                                        key: right[key]
                                        for key in (
                                            "c",
                                            "matrix",
                                            "origin",
                                            "rendered_size",
                                            "seq",
                                        )
                                    },
                                    **metrics,
                                }
                                serial += 1
                                push_top(top["linear"], linear, serial, record)
                                push_top(top["baseline"], baseline, serial, record)
                                push_top(top["dir_change"], 1.0 - dot, serial, record)
                                for metric, value in (
                                    ("linear", linear),
                                    ("baseline", baseline),
                                ):
                                    for cut in CUTS:
                                        side = "above" if value > cut else "below"
                                        previous = boundary[metric][cut][side]
                                        distance = abs(value - cut)
                                        if previous is None or distance < previous[0]:
                                            boundary[metric][cut][side] = (
                                                distance,
                                                record,
                                            )
                                seam_count += 1
                                document_stats[str(path)]["seams"] += 1
        except (OSError, RuntimeError, ValueError) as exc:
            errors.append({"document": str(path), "error": repr(exc)})

    distributions = {}
    for metric, metric_values in values.items():
        nonzero = [value for value in metric_values if value != 0]
        distributions[metric] = {
            "count": len(metric_values),
            "zero": len(metric_values) - len(nonzero),
            "nonzero": len(nonzero),
            "all_quantiles": {str(q): quantile(metric_values, q) for q in QUANTILES},
            "nonzero_quantiles": {str(q): quantile(nonzero, q) for q in QUANTILES},
            "min_nonzero": min(nonzero, default=None),
            "max": max(metric_values, default=None),
        }
    thresholds = {
        metric: {
            str(cut): {
                "seams": cut_seams[metric][cut],
                "spans": len(affected_spans[metric][cut]),
                "documents": len(affected_docs[metric][cut]),
            }
            for cut in CUTS
        }
        for metric in ("linear", "baseline")
    }
    result = {
        "schema": 1,
        "semantics": "adjacent non-synthetic glyphs already merged into one current span",
        "corpus": str(args.corpus.resolve()),
        "documents": len(paths),
        "max_pages": args.max_pages,
        "spans": span_count,
        "seams": seam_count,
        "degenerate": degenerate,
        "errors": errors,
        "seam_kinds": kinds,
        "distributions": distributions,
        "thresholds_strictly_greater_than": thresholds,
        "font_stats": dict(
            sorted(font_stats.items(), key=lambda item: -item[1]["seams"])
        ),
        "document_stats": document_stats,
        "char_projection": {
            "fields": CHAR_FIELDS,
            "total_chars": total_chars,
            "pages": page_char_projection,
        },
        "top": {
            metric: [record for _, _, record in sorted(heap, reverse=True)]
            for metric, heap in top.items()
        },
        "boundary_examples": {
            metric: {
                str(cut): {
                    side: item[1] if item is not None else None
                    for side, item in sides.items()
                }
                for cut, sides in cuts.items()
            }
            for metric, cuts in boundary.items()
        },
    }
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(
        f"documents={len(paths)} spans={span_count} seams={seam_count} degenerate={degenerate} errors={len(errors)}"
    )
    print(json.dumps(distributions, indent=2))
    print(json.dumps(thresholds, indent=2))


if __name__ == "__main__":
    main()
