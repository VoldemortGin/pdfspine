#!/usr/bin/env python3
"""GriTS (Grid Table Similarity) — pure-stdlib reimplementation.

GriTS is the recognized cell-structure metric proposed in Smock, Pesala & Abraham,
*"GriTS: Grid table similarity metric for table structure recognition"* (arXiv
2203.12555). It is the metric used to evaluate Microsoft's Table Transformer on
PubTables-1M and FinTabNet.c, so it is the right yardstick for an ABSOLUTE
cell-structure score of pdfspine ``find_tables`` against the FinTabNet.c gold GT.

Why GriTS (over TEDS-Struct / cell-adjacency F1)
------------------------------------------------
* It scores cell **topology** (row/col spans) and cell **content** in a single,
  comparable F-score framework, with partial credit per grid cell.
* It is **transpose-invariant** and **position-invariant** (rows and columns are
  equally weighted; credit does not depend on a cell's absolute location), the two
  properties an ideal TSR metric should have (paper §3).
* It is the canonical FinTabNet.c metric, so the resulting number is directly
  comparable to published Table-Transformer results — exactly what an "absolute"
  score needs.

What this file implements
-------------------------
A faithful port of the reference ``factored_2dmss`` algorithm from
``microsoft/table-transformer`` (``src/grits.py``), with TWO deliberate
substitutions so it runs in the project venv with NO third-party deps and never
imports the AGPL ``fitz``:

* ``numpy`` DP tables  -> nested Python lists,
* ``fitz.Rect`` IoU    -> plain-arithmetic IoU,
* text LCS similarity  -> ``difflib.SequenceMatcher`` (stdlib; the reference uses
  the same ``SequenceMatcher``-based ratio).

The 2D-MSS (maximum-similarity substructure) is NP-hard; like the reference we use
the polynomial-time FACTORED heuristic: align the rows (outer DP whose inner reward
is a 1D column alignment), align the columns (the transpose), then sum the per-cell
rewards over the resulting row/column index alignment. ``GriTS`` is the F-score of
that matched similarity (Eq. 6 of the paper).

Public API
----------
``grits_top(true_cells, pred_cells) -> (fscore, precision, recall)``  — topology
``grits_con(true_cells, pred_cells) -> (fscore, precision, recall)``  — content

where ``*_cells`` is a list of cell dicts with at least::

    {"row_nums": [int, ...], "column_nums": [int, ...],
     "cell_text": str}            # cell_text only needed for grits_con

Spanning cells simply list every row/column index they occupy.
"""

from __future__ import annotations

from difflib import SequenceMatcher


# --------------------------------------------------------------------------- #
# F-score (mirrors reference conventions exactly)
# --------------------------------------------------------------------------- #
def compute_fscore(num_true_positives: float, num_true: int,
                   num_positives: int) -> tuple[float, float, float]:
    """F-score with the reference's edge conventions.

    precision = 1 when there are no predicted cells; recall = 1 when there are no
    true cells; fscore = 0 when precision or recall is 0.
    """
    precision = num_true_positives / num_positives if num_positives > 0 else 1.0
    recall = num_true_positives / num_true if num_true > 0 else 1.0
    if precision + recall > 0:
        fscore = 2 * precision * recall / (precision + recall)
    else:
        fscore = 0.0
    return fscore, precision, recall


# --------------------------------------------------------------------------- #
# Reward functions (per grid-cell similarity f(.,.) in [0,1])
# --------------------------------------------------------------------------- #
def iou(b1, b2) -> float:
    """IoU of two ``[x0,y0,x1,y1]`` boxes (used for GriTS_Top relative spans)."""
    if not b1 or not b2:
        return 0.0
    ix0, iy0 = max(b1[0], b2[0]), max(b1[1], b2[1])
    ix1, iy1 = min(b1[2], b2[2]), min(b1[3], b2[3])
    iw, ih = ix1 - ix0, iy1 - iy0
    inter = iw * ih if (iw > 0 and ih > 0) else 0.0
    a1 = max(0.0, b1[2] - b1[0]) * max(0.0, b1[3] - b1[1])
    a2 = max(0.0, b2[2] - b2[0]) * max(0.0, b2[3] - b2[1])
    union = a1 + a2 - inter
    return inter / union if union > 0 else 0.0


def lcs_similarity(s1: str, s2: str) -> float:
    """Normalized LCS similarity of two strings (used for GriTS_Con).

    Matches the reference: 2*|LCS| / (|s1|+|s2|) via ``SequenceMatcher`` matching
    blocks. Two empty strings score 1.0.
    """
    if len(s1) == 0 and len(s2) == 0:
        return 1.0
    if len(s1) == 0 or len(s2) == 0:
        return 0.0
    sm = SequenceMatcher(None, s1, s2, autojunk=False)
    lcs = sum(block.size for block in sm.get_matching_blocks())
    return 2 * lcs / (len(s1) + len(s2))


# --------------------------------------------------------------------------- #
# cells -> grid-cell matrices
# --------------------------------------------------------------------------- #
def _grid_shape(cells: list[dict]) -> tuple[int, int]:
    num_rows = max((max(c["row_nums"]) for c in cells), default=-1) + 1
    num_cols = max((max(c["column_nums"]) for c in cells), default=-1) + 1
    return num_rows, num_cols


def cells_to_text_grid(cells: list[dict]) -> list[list[str]]:
    """Grid of cell text; a spanning cell's text repeats at every grid location."""
    if not cells:
        return [[]]
    nr, nc = _grid_shape(cells)
    grid: list[list[str]] = [["" for _ in range(nc)] for _ in range(nr)]
    for c in cells:
        text = c.get("cell_text", "") or ""
        for r in c["row_nums"]:
            for col in c["column_nums"]:
                grid[r][col] = text
    return grid


def cells_to_relspan_grid(cells: list[dict]) -> list[list[list[float]]]:
    """Grid of relative-span boxes for GriTS_Top.

    For grid cell (i,j) belonging to a cell spanning rows [min_r..max_r] and
    columns [min_c..max_c], the relative-span box is
    ``[min_c - j, min_r - i, max_c + 1 - j, max_r + 1 - i]`` — i.e. the size and
    offset of the owning cell in grid units. A non-spanning cell is ``[0,0,1,1]``.
    """
    if not cells:
        return [[]]
    nr, nc = _grid_shape(cells)
    grid: list[list[list[float]]] = [
        [[0.0, 0.0, 0.0, 0.0] for _ in range(nc)] for _ in range(nr)
    ]
    for c in cells:
        min_r, max_r = min(c["row_nums"]), max(c["row_nums"]) + 1
        min_c, max_c = min(c["column_nums"]), max(c["column_nums"]) + 1
        for r in c["row_nums"]:
            for col in c["column_nums"]:
                grid[r][col] = [
                    float(min_c - col),
                    float(min_r - r),
                    float(max_c - col),
                    float(max_r - r),
                ]
    return grid


# --------------------------------------------------------------------------- #
# Factored 2D-MSS (polynomial-time heuristic) — port of the reference
# --------------------------------------------------------------------------- #
def _init_dp(n1: int, n2: int) -> tuple[list[list[float]], list[list[int]]]:
    scores = [[0.0] * (n2 + 1) for _ in range(n1 + 1)]
    pointers = [[0] * (n2 + 1) for _ in range(n1 + 1)]
    for i in range(1, n1 + 1):
        pointers[i][0] = -1  # up
    for j in range(1, n2 + 1):
        pointers[0][j] = 1   # left
    return scores, pointers


def _traceback(pointers: list[list[int]]) -> tuple[list[int], list[int]]:
    """Diagonal moves yield the aligned (seq1_idx, seq2_idx) pairs.

    Pointer convention: -1 = up, 1 = left, 0 = diag (a match).
    """
    i = len(pointers) - 1
    j = len(pointers[0]) - 1
    a1: list[int] = []
    a2: list[int] = []
    while not (i == 0 and j == 0):
        p = pointers[i][j]
        if p == -1:
            i -= 1
        elif p == 1:
            j -= 1
        else:
            i -= 1
            j -= 1
            a1.append(i)
            a2.append(j)
    return a1[::-1], a2[::-1]


def _align_1d(seq1: list[tuple], seq2: list[tuple], rewards: dict) -> float:
    """1D DP alignment score; entries are keys into ``rewards`` (concatenated)."""
    n1, n2 = len(seq1), len(seq2)
    scores, _ = _init_dp(n1, n2)
    for i in range(1, n1 + 1):
        row_i = scores[i]
        row_im1 = scores[i - 1]
        s1e = seq1[i - 1]
        for j in range(1, n2 + 1):
            reward = rewards[s1e + seq2[j - 1]]
            diag = row_im1[j - 1] + reward
            skip2 = row_i[j - 1]
            skip1 = row_im1[j]
            row_i[j] = diag if diag >= skip1 and diag >= skip2 else max(skip1, skip2)
    return scores[n1][n2]


def _align_2d_outer(true_shape: tuple[int, int], pred_shape: tuple[int, int],
                    rewards: dict) -> tuple[list[int], list[int], float]:
    """Outer DP over rows; inner reward = 1D alignment of the two rows' columns."""
    n1, n2 = true_shape[0], pred_shape[0]
    scores, pointers = _init_dp(n1, n2)
    true_cols = range(true_shape[1])
    pred_cols = range(pred_shape[1])
    for i in range(1, n1 + 1):
        row_i = scores[i]
        row_im1 = scores[i - 1]
        prow_i = pointers[i]
        seq1 = [(i - 1, tc) for tc in true_cols]
        for j in range(1, n2 + 1):
            reward = _align_1d(seq1, [(j - 1, pc) for pc in pred_cols], rewards)
            diag = row_im1[j - 1] + reward
            same_row = row_i[j - 1]   # skip a predicted row (left)
            same_col = row_im1[j]     # skip a true row (up)
            best = max(diag, same_col, same_row)
            row_i[j] = best
            if diag == best:
                prow_i[j] = 0
            elif same_col == best:
                prow_i[j] = -1
            else:
                prow_i[j] = 1
    a_true, a_pred = _traceback(pointers)
    return a_true, a_pred, scores[n1][n2]


def factored_2dmss(true_grid: list[list], pred_grid: list[list],
                   reward_function) -> tuple[float, float, float]:
    """Factored 2D-MSS GriTS F-score (and precision/recall) of two grids.

    ``true_grid``/``pred_grid`` are equal-width rectangular matrices of grid-cell
    features (relative-span boxes for topology, text for content). Empty grids are
    handled via the F-score edge conventions.
    """
    tr = len(true_grid)
    tc = len(true_grid[0]) if tr else 0
    pr = len(pred_grid)
    pc = len(pred_grid[0]) if pr else 0
    num_true = tr * tc
    num_pos = pr * pc
    if num_true == 0 or num_pos == 0:
        return compute_fscore(0.0, num_true, num_pos)

    # Pre-compute the full reward tensor once (keyed by (trow,tcol,prow,pcol) and
    # its transpose) — exactly as the reference does.
    rewards: dict = {}
    transpose: dict = {}
    for ti in range(tr):
        trow = true_grid[ti]
        for tj in range(tc):
            te = trow[tj]
            for pi in range(pr):
                prow = pred_grid[pi]
                for pj in range(pc):
                    r = reward_function(te, prow[pj])
                    rewards[(ti, tj, pi, pj)] = r
                    transpose[(tj, ti, pj, pi)] = r

    true_rows, pred_rows, _ = _align_2d_outer((tr, tc), (pr, pc), rewards)
    true_cols, pred_cols, _ = _align_2d_outer((tc, tr), (pc, pr), transpose)

    match = 0.0
    for ti, pi in zip(true_rows, pred_rows):
        for tj, pj in zip(true_cols, pred_cols):
            match += rewards[(ti, tj, pi, pj)]

    return compute_fscore(match, num_true, num_pos)


# --------------------------------------------------------------------------- #
# Public entry points
# --------------------------------------------------------------------------- #
def grits_top(true_cells: list[dict], pred_cells: list[dict]) -> tuple[float, float, float]:
    """GriTS_Top (cell topology): (fscore, precision, recall)."""
    return factored_2dmss(
        cells_to_relspan_grid(true_cells),
        cells_to_relspan_grid(pred_cells),
        iou,
    )


def grits_con(true_cells: list[dict], pred_cells: list[dict]) -> tuple[float, float, float]:
    """GriTS_Con (cell content): (fscore, precision, recall)."""
    return factored_2dmss(
        cells_to_text_grid(true_cells),
        cells_to_text_grid(pred_cells),
        lcs_similarity,
    )


# --------------------------------------------------------------------------- #
# Self-test: known-answer checks against hand-computed cases
# --------------------------------------------------------------------------- #
def _self_test() -> int:
    def cell(rows, cols, text=""):
        return {"row_nums": list(rows), "column_nums": list(cols), "cell_text": text}

    # 1) Identical 2x2 simple grid -> perfect score on both metrics.
    g = [cell([0], [0], "a"), cell([0], [1], "b"),
         cell([1], [0], "c"), cell([1], [1], "d")]
    ft, pt, rt = grits_top(g, g)
    fc, pc, rc = grits_con(g, g)
    assert abs(ft - 1.0) < 1e-9, ft
    assert abs(fc - 1.0) < 1e-9, fc

    # 2) Content metric: one cell text differs by one char -> < 1 but > 0.
    g2 = [cell([0], [0], "a"), cell([0], [1], "b"),
          cell([1], [0], "c"), cell([1], [1], "X")]
    fc2, _, _ = grits_con(g, g2)
    assert 0.0 < fc2 < 1.0, fc2

    # 3) Topology metric is text-blind: same spans, different text -> 1.0.
    ft2, _, _ = grits_top(g, g2)
    assert abs(ft2 - 1.0) < 1e-9, ft2

    # 4) A spanning cell vs the same grid split into two cells -> topology < 1.
    span = [cell([0, 1], [0], "h"), cell([0], [1], "b"), cell([1], [1], "d")]
    split = [cell([0], [0], "h"), cell([1], [0], "h"),
             cell([0], [1], "b"), cell([1], [1], "d")]
    ft3, _, _ = grits_top(span, split)
    assert 0.0 < ft3 < 1.0, ft3
    # Identical span structure -> 1.0.
    ft4, _, _ = grits_top(span, span)
    assert abs(ft4 - 1.0) < 1e-9, ft4

    # 5) Empty prediction -> 0 (recall side), but no crash.
    fe, _, _ = grits_con(g, [])
    assert fe == 0.0, fe

    # 6) Different shapes: 2x2 GT vs 2x3 pred -> partial credit in (0,1).
    g3 = [cell([0], [0], "a"), cell([0], [1], "b"), cell([0], [2], "z"),
          cell([1], [0], "c"), cell([1], [1], "d"), cell([1], [2], "z")]
    fc3, p3, r3 = grits_con(g, g3)
    assert 0.0 < fc3 < 1.0, fc3
    # recall (over GT) should exceed precision (over the larger prediction).
    assert r3 > p3, (r3, p3)

    # 7) lcs_similarity sanity.
    assert lcs_similarity("", "") == 1.0
    assert lcs_similarity("abc", "abc") == 1.0
    assert lcs_similarity("abc", "") == 0.0
    assert 0.0 < lcs_similarity("abcd", "abxd") < 1.0

    print("grits.py self-test OK (7 known-answer cases)")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(_self_test())
