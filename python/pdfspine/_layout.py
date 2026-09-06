"""Layout-preserving plain text — the engine behind ``Page.get_text("layout")``
(pdfspine-original extension, not part of the PyMuPDF surface).

The input is the ``get_text("words")`` tuple list. Words are regrouped into
visual lines with a *y tolerance* (a word joins the line whose anchor is
within ``y_tolerance`` points of the word's vertical center), ordered by
``x0`` inside a line, and painted onto a character grid so that horizontal
positions survive as space padding, ``pdftotext -layout`` style.
"""

from __future__ import annotations

from statistics import median
from typing import Sequence

_MAX_BLANK_LINES = 2
_BLANK_LINE_RATIO = 1.9


def _word_geometry(
    words: Sequence[Sequence[object]],
) -> list[tuple[float, float, float, float, str]]:
    out: list[tuple[float, float, float, float, str]] = []
    for word in words:
        text = str(word[4]).strip()
        if not text:
            continue
        x0, y0, x1, y1 = (float(word[i]) for i in range(4))
        if x1 < x0:
            x0, x1 = x1, x0
        if y1 < y0:
            y0, y1 = y1, y0
        out.append((x0, y0, x1, y1, text))
    return out


def group_lines(
    words: Sequence[tuple[float, float, float, float, str]], *, y_tolerance: float
) -> list[list[tuple[float, float, float, float, str]]]:
    """Groups words into visual lines by vertical-center tolerance.

    Words are visited top-to-bottom; a word joins the current line when its
    vertical center lies within ``y_tolerance`` of the line's anchor (the
    center of its first word). Comparing against the anchor rather than the
    previous word keeps sub-point baseline jitter from chaining lines together.
    Each line is returned sorted by ``x0``.
    """
    ordered = sorted(words, key=lambda w: ((w[1] + w[3]) / 2.0, w[0]))
    lines: list[list[tuple[float, float, float, float, str]]] = []
    anchor = 0.0
    for word in ordered:
        cy = (word[1] + word[3]) / 2.0
        if lines and abs(cy - anchor) <= y_tolerance:
            lines[-1].append(word)
        else:
            lines.append([word])
            anchor = cy
    for line in lines:
        line.sort(key=lambda w: (w[0], w[1]))
    return lines


def layout_text(
    words: Sequence[Sequence[object]],
    *,
    y_tolerance: float = 3.0,
    char_width: float | None = None,
) -> str:
    """Renders ``get_text("words")`` tuples as layout-preserving text.

    ``y_tolerance`` (points) controls line grouping; ``char_width`` is the
    grid cell width in points (``None`` derives the median glyph width from
    the words). Vertical gaps wider than a typical line pitch become blank
    lines (at most two). Returns ``""`` for a page without words; otherwise
    every line — including the last — ends with ``"\\n"``.
    """
    if y_tolerance < 0:
        raise ValueError("y_tolerance must be >= 0")
    if char_width is not None and char_width <= 0:
        raise ValueError("char_width must be > 0")
    geometry = _word_geometry(words)
    if not geometry:
        return ""

    if char_width is None:
        char_width = median((w[2] - w[0]) / len(w[4]) for w in geometry)
        if not char_width > 0:
            char_width = 1.0
    left = min(w[0] for w in geometry)
    lines = group_lines(geometry, y_tolerance=y_tolerance)

    pitch = median(max(w[3] - w[1] for w in line) for line in lines) * 1.2
    rows: list[str] = []
    previous_center: float | None = None
    for line in lines:
        center = median((w[1] + w[3]) / 2.0 for w in line)
        if previous_center is not None and pitch > 0:
            ratio = (center - previous_center) / pitch
            if ratio >= _BLANK_LINE_RATIO:
                rows.extend([""] * min(_MAX_BLANK_LINES, int(round(ratio)) - 1))
        previous_center = center

        parts: list[str] = []
        cursor = 0
        for x0, _y0, _x1, _y1, text in line:
            column = int(round((x0 - left) / char_width))
            if parts:
                column = max(column, cursor + 1)
            parts.append(" " * (column - cursor))
            parts.append(text)
            cursor = column + len(text)
        rows.append("".join(parts).rstrip())
    return "\n".join(rows) + "\n"
