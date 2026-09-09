#!/usr/bin/env python3
"""Table-structure metrics beyond GriTS — TEDS-Struct and cell-alignment F1.

``grits.py`` gives the canonical FinTabNet.c number. Two more views are needed
for a financial-report evaluation set, and both live here so the future
``eval_tables.py`` CLI can report them side by side with GriTS:

* **TEDS-Struct** — the structure-only variant of Tree-Edit-Distance-based
  Similarity (Zhong, ShafieiBavani & Jimeno Yepes, *"Image-based table
  recognition: data, model, and evaluation"*, arXiv 1911.10683). It is the
  PubTabNet / ICDAR-2021 metric, so it is the second number people expect to
  see next to GriTS. GriTS is grid-based and forgiving of a shifted row; TEDS
  penalises every structural edit, so the two disagree in informative ways
  (e.g. a merged header row costs one edit in TEDS but a whole row of partial
  credit in GriTS).
* **Cell-alignment F1** — a plain detection-style score: does the predicted
  cell land on the gold cell's *geometry*? GriTS and TEDS are position-invariant
  by design, so a table whose cells are all correct but offset by one column
  can still score well on them. The bbox F1 catches that class of error and is
  what a downstream consumer (crop the cell, read the number) actually feels.

Why TEDS-Struct rather than full TEDS
-------------------------------------
Full TEDS also compares cell text with a normalised Levenshtein cost. Our text
score is already covered by ``GriTS_Con`` (which uses the same LCS family), so
adding text to TEDS would double-count it and make the structure signal harder
to read. The structure-only variant is what the PubTabNet leaderboard reports
as *TEDS-Struct* and what TATR / FinTabNet.c papers quote.

How TEDS-Struct is computed here
--------------------------------
1. GriTS cells -> an ordered labelled tree ``table -> tr* -> td*``. A cell
   contributes exactly one ``td`` node, on its *first* row (HTML semantics: a
   ``rowspan`` cell is written once and occupies later rows implicitly). Within
   a row the ``td`` nodes are ordered by their first column.
2. The ``td`` label carries the span as part of the label
   (``td``, ``td[colspan=2]``, ``td[rowspan=3]``, ``td[colspan=2 rowspan=3]``).
   This is the standard TEDS-Struct convention: node equality is tag +
   ``colspan`` + ``rowspan``; text is ignored.
3. Zhang-Shasha ordered tree edit distance (unit cost for insert / delete /
   relabel). Pure stdlib: post-order numbering, leftmost-leaf table, key roots,
   and the classic ``treedist`` / ``forestdist`` two-level DP. Complexity is
   O(|T1|·|T2|·min(depth,leaves)²); table trees have depth 2 so it is close to
   O(n²) in practice.
4. ``TEDS = 1 - d / max(|T_true|, |T_pred|)`` where ``|T|`` counts every node
   including the ``table`` root, so the denominator is never 0 and two empty
   tables score 1.0.

Guard rails: ``teds_struct`` returns ``None`` (never raises) when either tree
exceeds ``max_nodes`` or the DP runs past ``timeout_s``. The timeout is a plain
``time.monotonic()`` deadline checked in the key-root loops — no signals, no
threads — so the worst-case overshoot is one inner ``forestdist`` block.

Cell-alignment F1
-----------------
Greedy one-to-one matching on bbox IoU: enumerate every (true, pred) pair with
IoU >= ``iou_threshold``, sort by IoU descending, and accept a pair only when
both cells are still free. Greedy rather than Hungarian because cells rarely
overlap each other, so the assignment is essentially unambiguous and stdlib-only
is a hard requirement. ``require_text_match=True`` additionally demands equal
whitespace-collapsed, case-folded text. Cells without a bbox can never match;
when one side has *no* bbox at all the score is reported as skipped rather than
as a misleading 0.

Limitations
-----------
* TEDS-Struct ignores which rows are headers (``thead``/``tbody`` are not
  emitted) because pdfspine does not predict header rows; both sides are
  treated uniformly so nothing is lost in comparison.
* Zhang-Shasha is quadratic-plus; a 2000-node tree pair (~1000 cells) takes a
  few seconds in CPython, which is why ``max_nodes`` defaults to 2000.
* Cell IoU assumes both sides use the same coordinate system (PDF points,
  origin top-left, y down — the pdfspine ``Table.bbox`` convention that
  FinTabNet.c annotations already use).

Public API
----------
``teds_struct(true_cells, pred_cells, *, max_nodes=2000, timeout_s=20.0)``
``cells_to_structure_tree(cells) -> Node``
``tree_edit_distance(t1, t2) -> int``
``cell_alignment(true_cells, pred_cells, *, iou_threshold=0.5,
require_text_match=False) -> dict``

``*_cells`` follow the repo-wide GriTS cell dict::

    {"row_nums": [int, ...], "column_nums": [int, ...],
     "cell_text": str, "bbox": [x0, y0, x1, y1] | None}
"""

from __future__ import annotations

from dataclasses import dataclass, field
import time


# --------------------------------------------------------------------------- #
# Structure tree
# --------------------------------------------------------------------------- #
@dataclass
class Node:
    """Ordered labelled tree node (children keep document order)."""

    label: str
    children: list[Node] = field(default_factory=list)

    def size(self) -> int:
        """Number of nodes in the subtree rooted here (including itself)."""
        return 1 + sum(child.size() for child in self.children)


def _td_label(colspan: int, rowspan: int) -> str:
    attrs = []
    if colspan > 1:
        attrs.append(f"colspan={colspan}")
    if rowspan > 1:
        attrs.append(f"rowspan={rowspan}")
    return "td[" + " ".join(attrs) + "]" if attrs else "td"


def cells_to_structure_tree(cells: list[dict]) -> Node:
    """GriTS cells -> ``table -> tr* -> td*`` tree used by TEDS-Struct.

    * One ``tr`` per grid row ``0 .. max(row_nums)`` (rows with no cell
      *starting* in them still get an empty ``tr``, e.g. under a rowspan).
    * A cell yields exactly one ``td`` node, placed in the ``tr`` of its first
      row and ordered by its first column; its label encodes colspan/rowspan.
    * Cells with empty ``row_nums``/``column_nums`` are ignored.
    """
    starts: dict[int, list[tuple[int, str]]] = {}
    n_rows = 0
    for c in cells:
        rows = c.get("row_nums") or []
        cols = c.get("column_nums") or []
        if not rows or not cols:
            continue
        r0, r1 = min(rows), max(rows)
        c0, c1 = min(cols), max(cols)
        n_rows = max(n_rows, r1 + 1)
        starts.setdefault(r0, []).append((c0, _td_label(c1 - c0 + 1, r1 - r0 + 1)))
    table = Node("table")
    for r in range(n_rows):
        tr = Node("tr")
        for _, label in sorted(starts.get(r, []), key=lambda t: t[0]):
            tr.children.append(Node(label))
        table.children.append(tr)
    return table


# --------------------------------------------------------------------------- #
# Zhang-Shasha ordered tree edit distance
# --------------------------------------------------------------------------- #
def _post_order(root: Node) -> tuple[list[str], list[int], list[int]]:
    """Post-order labels (1-indexed via a dummy slot 0), leftmost leaves, key roots.

    ``lml[i]`` is the post-order index of the leftmost leaf under node ``i``.
    Key roots are the nodes that are the highest ancestor sharing their leftmost
    leaf — the sub-problems the Zhang-Shasha DP has to solve explicitly.
    Iterative traversal, so deep or wide trees cannot hit the recursion limit.
    """
    labels: list[str] = [""]
    lml: list[int] = [0]
    # (node, next_child_index) stack for an iterative post-order walk.
    stack: list[tuple[Node, int]] = [(root, 0)]
    first_leaf: list[int | None] = [None]  # parallels ``stack``
    while stack:
        node, idx = stack[-1]
        if idx < len(node.children):
            stack[-1] = (node, idx + 1)
            stack.append((node.children[idx], 0))
            first_leaf.append(None)
            continue
        stack.pop()
        leaf = first_leaf.pop()
        labels.append(node.label)
        my_index = len(labels) - 1
        my_lml = my_index if leaf is None else leaf
        lml.append(my_lml)
        if stack and first_leaf[-1] is None:
            first_leaf[-1] = my_lml
    highest: dict[int, int] = {}
    for i in range(1, len(labels)):
        highest[lml[i]] = i  # later (higher) nodes overwrite earlier ones
    keyroots = sorted(highest.values())
    return labels, lml, keyroots


def _zhang_shasha(t1: Node, t2: Node, deadline: float | None) -> int | None:
    labels1, lml1, keyroots1 = _post_order(t1)
    labels2, lml2, keyroots2 = _post_order(t2)
    n1, n2 = len(labels1) - 1, len(labels2) - 1
    tree_dist = [[0] * (n2 + 1) for _ in range(n1 + 1)]

    for i in keyroots1:
        li = lml1[i]
        for j in keyroots2:
            if deadline is not None and time.monotonic() >= deadline:
                return None
            lj = lml2[j]
            m = i - li + 2
            n = j - lj + 2
            fd = [[0] * n for _ in range(m)]
            for a in range(1, m):
                fd[a][0] = a
            for b in range(1, n):
                fd[0][b] = b
            for a in range(1, m):
                ia = li + a - 1
                row = fd[a]
                prev = fd[a - 1]
                ia_is_tree = lml1[ia] == li
                for b in range(1, n):
                    jb = lj + b - 1
                    if ia_is_tree and lml2[jb] == lj:
                        cost = 0 if labels1[ia] == labels2[jb] else 1
                        best = min(prev[b] + 1, row[b - 1] + 1, prev[b - 1] + cost)
                        row[b] = best
                        tree_dist[ia][jb] = best
                    else:
                        p = lml1[ia] - li
                        q = lml2[jb] - lj
                        row[b] = min(
                            prev[b] + 1, row[b - 1] + 1, fd[p][q] + tree_dist[ia][jb]
                        )
    return tree_dist[n1][n2]


def tree_edit_distance(t1: Node, t2: Node) -> int:
    """Zhang-Shasha ordered tree edit distance with unit insert/delete/relabel."""
    result = _zhang_shasha(t1, t2, None)
    assert result is not None  # no deadline -> always completes
    return result


# --------------------------------------------------------------------------- #
# TEDS-Struct
# --------------------------------------------------------------------------- #
def teds_struct(
    true_cells: list[dict],
    pred_cells: list[dict],
    *,
    max_nodes: int = 2000,
    timeout_s: float | None = 20.0,
) -> float | None:
    """TEDS-Struct similarity in ``[0, 1]`` of two GriTS cell lists.

    ``1 - edit_distance / max(|T_true|, |T_pred|)``; node counts include the
    ``table`` root, so two empty tables score 1.0 and an empty vs non-empty
    table scores strictly below 1.

    Returns ``None`` — never raises — when either tree has more than
    ``max_nodes`` nodes or when ``timeout_s`` (a monotonic deadline; ``None``
    disables it, ``0`` trips immediately) expires. Callers should report such
    pairs as *skipped* rather than scoring them 0.
    """
    t_true = cells_to_structure_tree(true_cells)
    t_pred = cells_to_structure_tree(pred_cells)
    n_true, n_pred = t_true.size(), t_pred.size()
    if n_true > max_nodes or n_pred > max_nodes:
        return None
    deadline = None if timeout_s is None else time.monotonic() + timeout_s
    if deadline is not None and time.monotonic() >= deadline:
        return None
    dist = _zhang_shasha(t_true, t_pred, deadline)
    if dist is None:
        return None
    return 1.0 - dist / max(n_true, n_pred)


# --------------------------------------------------------------------------- #
# Cell-alignment F1
# --------------------------------------------------------------------------- #
def _bbox_iou(b1: list[float], b2: list[float]) -> float:
    ix0, iy0 = max(b1[0], b2[0]), max(b1[1], b2[1])
    ix1, iy1 = min(b1[2], b2[2]), min(b1[3], b2[3])
    iw, ih = ix1 - ix0, iy1 - iy0
    inter = iw * ih if (iw > 0 and ih > 0) else 0.0
    a1 = max(0.0, b1[2] - b1[0]) * max(0.0, b1[3] - b1[1])
    a2 = max(0.0, b2[2] - b2[0]) * max(0.0, b2[3] - b2[1])
    union = a1 + a2 - inter
    return inter / union if union > 0 else 0.0


def _norm_text(s: object) -> str:
    return " ".join(str(s or "").split()).casefold()


def _valid_bbox(c: dict) -> list[float] | None:
    b = c.get("bbox")
    if not b or len(b) < 4:
        return None
    try:
        return [float(v) for v in b[:4]]
    except (TypeError, ValueError):
        return None


def cell_alignment(
    true_cells: list[dict],
    pred_cells: list[dict],
    *,
    iou_threshold: float = 0.5,
    require_text_match: bool = False,
) -> dict:
    """Greedy one-to-one bbox matching of predicted vs gold cells -> P/R/F1.

    A pair is a candidate when ``IoU >= iou_threshold`` (and, with
    ``require_text_match``, when the whitespace-collapsed case-folded texts are
    equal). Candidates are taken in IoU-descending order while both cells are
    unclaimed. ``precision = matched / n_pred``, ``recall = matched / n_true``.

    Edge conventions:

    * ``n_true == n_pred == 0`` -> precision = recall = f1 = 1.0.
    * exactly one side empty -> 0.0 across the board.
    * a side that is non-empty but has **no** cell with a usable bbox cannot
      be scored: ``precision``/``recall``/``f1`` are ``None`` and
      ``skipped_reason == "no bbox"``. Individual bbox-less cells on an
      otherwise boxed side simply never match (they still count in ``n_*``).

    Returned dict keys: ``precision, recall, f1, matched, n_true, n_pred,
    iou_threshold`` (+ ``skipped_reason`` when skipped). Floats are unrounded.
    """
    n_true, n_pred = len(true_cells), len(pred_cells)
    base = {
        "matched": 0,
        "n_true": n_true,
        "n_pred": n_pred,
        "iou_threshold": iou_threshold,
    }
    if n_true == 0 and n_pred == 0:
        return {"precision": 1.0, "recall": 1.0, "f1": 1.0, **base}
    if n_true == 0 or n_pred == 0:
        return {"precision": 0.0, "recall": 0.0, "f1": 0.0, **base}

    true_boxes = [(i, b) for i, c in enumerate(true_cells) if (b := _valid_bbox(c))]
    pred_boxes = [(j, b) for j, c in enumerate(pred_cells) if (b := _valid_bbox(c))]
    if not true_boxes or not pred_boxes:
        return {
            "precision": None,
            "recall": None,
            "f1": None,
            "skipped_reason": "no bbox",
            **base,
        }

    true_text = [_norm_text(c.get("cell_text")) for c in true_cells]
    pred_text = [_norm_text(c.get("cell_text")) for c in pred_cells]
    candidates: list[tuple[float, int, int]] = []
    for i, tb in true_boxes:
        for j, pb in pred_boxes:
            if require_text_match and true_text[i] != pred_text[j]:
                continue
            v = _bbox_iou(tb, pb)
            if v >= iou_threshold and v > 0.0:
                candidates.append((v, i, j))
    # Highest IoU first; index tie-break keeps the result deterministic.
    candidates.sort(key=lambda t: (-t[0], t[1], t[2]))
    used_true: set[int] = set()
    used_pred: set[int] = set()
    matched = 0
    for _, i, j in candidates:
        if i in used_true or j in used_pred:
            continue
        used_true.add(i)
        used_pred.add(j)
        matched += 1

    precision = matched / n_pred
    recall = matched / n_true
    f1 = 2 * precision * recall / (precision + recall) if matched else 0.0
    base["matched"] = matched
    return {"precision": precision, "recall": recall, "f1": f1, **base}
