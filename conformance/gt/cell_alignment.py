"""Strict cell_span_f1_v1 diagnostic; independent of GriTS and TEDS.

Invalid gold is an input error. Invalid prediction topology receives zero true
positives and preserves every raw cell as a false positive, without repair.
"""

from __future__ import annotations

import unicodedata

MAX_GRID_SLOTS = 1_000_000


def topology_error(cells: list[dict]) -> str | None:
    rectangles = []
    max_row = max_column = 0
    if len(cells) > 10000:
        return "cell count exceeds evaluator limit"
    for cell in cells:
        if not isinstance(cell, dict):
            return "cell must be an object"
        axes = []
        for key in ("row_nums", "column_nums"):
            values = cell.get(key)
            if not isinstance(values, list) or not values:
                return f"{key} must be a nonempty list"
            if any(type(v) is not int or v < 0 for v in values):
                return f"{key} must contain nonnegative integers"
            values = sorted(values)
            if any(b != a + 1 for a, b in zip(values, values[1:])):
                return f"{key} must be unique and contiguous"
            axes.append((values[0], values[-1]))
        r, c = axes
        max_row, max_column = max(max_row, r[1] + 1), max(max_column, c[1] + 1)
        if max_row * max_column > MAX_GRID_SLOTS:
            return "grid exceeds evaluator slot limit"
        for previous_r, previous_c in rectangles:
            if max(r[0], previous_r[0]) <= min(r[1], previous_r[1]) and max(
                c[0], previous_c[0]
            ) <= min(c[1], previous_c[1]):
                return "overlapping or duplicate cell spans"
        rectangles.append((r, c))
    return None


def counts(tp: int, fp: int, fn: int) -> dict:
    return {
        "tp": tp,
        "fp": fp,
        "fn": fn,
        "precision": tp / (tp + fp) if tp + fp else (1.0 if not fn else 0.0),
        "recall": tp / (tp + fn) if tp + fn else (1.0 if not fp else 0.0),
        "f1": 2 * tp / (2 * tp + fp + fn) if 2 * tp + fp + fn else 1.0,
    }


def normalized_text(cell: dict) -> str:
    return " ".join(
        unicodedata.normalize("NFC", str(cell.get("cell_text", ""))).split()
    )


def score_cells(gold: list[dict], predicted: list[dict]) -> dict:
    error = topology_error(gold)
    if error:
        raise ValueError(f"invalid gold topology: {error}")
    error = topology_error(predicted)
    span_tp = content_tp = 0
    if not error:

        def key(cell):
            return tuple(sorted(cell["row_nums"])), tuple(sorted(cell["column_nums"]))

        actual = {key(cell): cell for cell in predicted}
        for cell in gold:
            match = actual.get(key(cell))
            if match is not None:
                span_tp += 1
                content_tp += normalized_text(cell) == normalized_text(match)
    return {
        "metric": "cell_span_f1_v1",
        "invalid_prediction": bool(error),
        "prediction_error": error,
        "gold_cells": len(gold),
        "predicted_cells": len(predicted),
        "span": counts(span_tp, len(predicted) - span_tp, len(gold) - span_tp),
        "content": counts(
            content_tp, len(predicted) - content_tp, len(gold) - content_tp
        ),
    }


def aggregate(records: list[dict]) -> dict:
    result = {
        "metric": "cell_span_f1_v1",
        "invalid_predictions": sum(r["invalid_prediction"] for r in records),
    }
    for name in ("span", "content"):
        sums = {key: sum(r[name][key] for r in records) for key in ("tp", "fp", "fn")}
        result[name] = counts(**sums)
    return result
