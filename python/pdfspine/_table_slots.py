"""Validate backend origin records and build the shared typed table snapshot."""

from collections.abc import Sequence
import math
from typing import Literal, TypeAlias

from .geometry import Rect
from .models import TableCell, TableSlot

CellRecord: TypeAlias = tuple[
    int, int, int, int, tuple[float, float, float, float], str | None
]
TableSnapshot: TypeAlias = tuple[
    tuple[tuple[TableSlot, ...], ...], tuple[TableCell, ...]
]


def table_snapshot(
    row_count: int, col_count: int, records: Sequence[CellRecord]
) -> TableSnapshot:
    """Use structural spans exclusively to establish ownership of each slot.

    The private backend contract defines record text as ``None`` (unavailable),
    ``""`` (blank), or extracted text. It is independent of legacy ``extract``
    and ``cells`` grids, whose ``None`` values have several meanings.
    """
    if (
        type(row_count) is not int
        or type(col_count) is not int
        or row_count < 0
        or col_count < 0
    ):
        raise ValueError("table grid dimensions must be nonnegative integers")

    grid = [
        [TableSlot(row, col, "unavailable", None) for col in range(col_count)]
        for row in range(row_count)
    ]
    for row, col, row_span, col_span, bbox, text in records:
        if (
            any(type(value) is not int for value in (row, col, row_span, col_span))
            or row < 0
            or col < 0
            or row_span < 1
            or col_span < 1
            or row + row_span > row_count
            or col + col_span > col_count
        ):
            raise ValueError("table cell span is outside the grid or invalid")
        if (
            len(bbox) != 4
            or not all(math.isfinite(value) for value in bbox)
            or bbox[0] >= bbox[2]
            or bbox[1] >= bbox[3]
        ):
            raise ValueError("table cell bbox must be finite and nonempty")
        if text is not None and not isinstance(text, str):
            raise ValueError("table cell text must be a string or None")
        if any(
            grid[r][c].cell is not None
            for r in range(row, row + row_span)
            for c in range(col, col + col_span)
        ):
            raise ValueError("overlapping table cell spans have ambiguous origins")

        state: Literal["present", "blank", "unavailable"]
        if text is None:
            state = "unavailable"
        elif not text.strip():
            state, text = "blank", ""
        else:
            state = "present"
        cell = TableCell(row, col, row_span, col_span, Rect(bbox), state, text)
        for r in range(row, row + row_span):
            for c in range(col, col + col_span):
                slot_state: Literal[
                    "present", "blank", "unavailable", "continuation"
                ] = state if (r, c) == (row, col) else "continuation"
                grid[r][c] = TableSlot(r, c, slot_state, cell)

    cells = tuple(
        slot.cell
        for grid_row in grid
        for slot in grid_row
        if slot.cell is not None and slot.origin == (slot.row, slot.col)
    )
    return tuple(tuple(grid_row) for grid_row in grid), cells
