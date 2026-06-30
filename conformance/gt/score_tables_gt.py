#!/usr/bin/env python3
"""Objective TABLE-extraction accuracy: pdfspine vs fitz vs *known* ground truth.

Unlike ``tables_diff.py`` (which uses fitz as a pseudo-oracle), this scores both
engines against the TRUE cell grid of each table, manufactured by construction in
``corpus-tables/`` (a manifest carrying every cell + the table's border style).
This is the objective table GT that FinTabNet would have provided — built locally
because the FinTabNet CDN is unreachable.

fitz (AGPL) is never imported into our interpreter, and a Rust panic must not kill
the run, so each ``find_tables`` happens in an ISOLATED SUBPROCESS:
  - ``--worker pdfspine`` runs in the project venv (built wheel).
  - ``--worker fitz``     runs in ``.venv-oracle`` (PyMuPDF).

Each table is scored with the bordered/borderless-appropriate strategy (``lines``
for ruled tables, ``text`` for whitespace-aligned ones — what a caller would pick).
Metric = cell-level F1: a cell counts as matched iff its normalized text equals the
GT cell at the same (row, col). Reports per-table pdfspine vs fitz vs GT and the
aggregate (mean F1, count of pdfspine >= fitz, strict wins).

Run from ROOT:  ``.venv/bin/python conformance/gt/score_tables_gt.py``
"""

from __future__ import annotations

import json
import os
import statistics as st
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus-tables"
ROOT = HERE.parents[1]
FITZ_PY = ROOT / ".venv-oracle" / "bin" / "python"


def _extract(strategy: str, pdf: str, engine: str) -> dict:
    """Worker: return {"grid": [[...]]} for the first table on page 0."""
    if engine == "pdfspine":
        import pdfspine

        pg = pdfspine.open(pdf)[0]
        tf = pg.find_tables(strategy=strategy)
        tbls = getattr(tf, "tables", tf)
        return {"grid": tbls[0].extract() if tbls else []}
    else:  # fitz
        import fitz

        kw = {} if strategy == "lines" else {
            "vertical_strategy": "text",
            "horizontal_strategy": "text",
        }
        pg = fitz.open(pdf)[0]
        tbls = pg.find_tables(**kw).tables
        return {"grid": tbls[0].extract() if tbls else []}


def _run_worker(engine: str, strategy: str, pdf: str) -> dict:
    """Drive a worker in its own venv/subprocess; return its parsed grid."""
    py = sys.executable if engine == "pdfspine" else str(FITZ_PY)
    out = subprocess.run(
        [py, __file__, "--worker", engine, "--strategy", strategy, "--pdf", pdf],
        capture_output=True,
        text=True,
        timeout=90,
    )
    for line in out.stdout.splitlines():
        if line.startswith("{"):
            return json.loads(line)
    return {"grid": []}


def _norm(c) -> str:
    return str(c).strip() if c is not None else ""


def _cells(grid) -> dict:
    s = {}
    for r, row in enumerate(grid or []):
        for c, v in enumerate(row):
            if _norm(v):
                s[(r, c)] = _norm(v)
    return s


def _f1(gt_grid, ext_grid) -> float:
    g, e = _cells(gt_grid), _cells(ext_grid)
    if not g:
        return 0.0
    m = sum(1 for k in g if e.get(k) == g[k])
    p = m / len(e) if e else 0.0
    r = m / len(g)
    return round(2 * p * r / (p + r), 3) if (p + r) else 0.0


def main() -> int:
    manifest = json.loads((CORPUS / "manifest.json").read_text())
    print(f"{'table (type)':26s} {'strat':6s} {'pdfspine':9s} {'fitz':9s} winner")
    pf, ff, win, surp = [], [], 0, 0
    for t in manifest:
        pdf = str(CORPUS / t["pdf"])
        strat = t["strategy"]
        sp = _f1(t["grid"], _run_worker("pdfspine", strat, pdf)["grid"])
        sf = _f1(t["grid"], _run_worker("fitz", strat, pdf)["grid"])
        pf.append(sp)
        ff.append(sf)
        if sp >= sf - 0.01:
            win += 1
        if sp > sf + 0.01:
            surp += 1
        typ = "bordered" if t["bordered"] else "borderless"
        w = "pdfspine" if sp > sf + 0.01 else ("fitz" if sf > sp + 0.01 else "tie")
        print(f"{t['name'] + ' (' + typ + ')':26s} {strat:6s} {sp:<9} {sf:<9} {w}")
    print(
        f"\nmean: pdfspine {st.mean(pf):.3f}  fitz {st.mean(ff):.3f}"
        f" | pdfspine>=fitz {win}/{len(pf)} | strict wins {surp}"
    )
    return 0


if __name__ == "__main__":
    if "--worker" in sys.argv:
        a = sys.argv
        eng = a[a.index("--worker") + 1]
        strat = a[a.index("--strategy") + 1]
        pdf = a[a.index("--pdf") + 1]
        try:
            print(json.dumps(_extract(strat, pdf, eng), ensure_ascii=False))
        except Exception as e:  # noqa: BLE001 — worker isolation: never crash parent
            print(json.dumps({"grid": [], "err": str(e)}, ensure_ascii=False))
        sys.exit(0)
    sys.exit(main())
