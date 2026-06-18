#!/usr/bin/env python3
"""Differential TABLE-extraction test: pdfspine ``find_tables`` vs fitz.

pdfspine exposes ``Page.find_tables() -> TableFinder`` (M7). This harness measures,
objectively, how closely pdfspine's table detection and structure agree with fitz
(PyMuPDF), the reference implementation, on table-dense PDFs.

How it works
------------
fitz (AGPL) must NEVER be imported into our interpreter, and a Rust panic/abort in
pdfspine must not take down the run. So every ``find_tables`` call happens in an
ISOLATED SUBPROCESS under a wall-clock timeout (mirroring ``oracle_extract.py`` /
``pdfspine_worker.py`` / ``run_validation.py``):

- ``--worker pdfspine`` runs inside the project venv (with our built wheel).
- ``--worker fitz``  runs inside ``.venv-oracle`` (PyMuPDF).

Each worker reads ``<pdf> <page_index>`` and prints a JSON list of tables, each::

    {"bbox": [x0,y0,x1,y1], "row_count": R, "col_count": C, "cells_text": "flat text"}

The parent (the default mode) drives both workers per page and compares:

  (a) table-COUNT agreement   — |#pdfspine - #fitz| == 0 ?
  (b) GRID-SHAPE agreement    — for tables matched by bbox IoU > 0.5, does
                                 (rows, cols) match exactly?
  (c) CELL-TEXT agreement     — token F1 (via gt/score.py) of the two tables'
                                 flattened cell text, pdfspine-vs-fitz.

Reports per-doc and aggregate: table-count agreement rate, mean grid-shape match,
mean cell-text F1, plus the worst divergences with a one-line cause guess.

NOTE on the reference: this is pdfspine-vs-fitz *agreement*, not accuracy against a
human-labelled gold. fitz is the de-facto reference but is itself imperfect at
table detection; treat the numbers as parity-with-fitz, not ground truth.
(FinTabNet structural GT was considered as an objective anchor but skipped to keep
the run self-contained — see the report footer.)

Usage::

    env -u CONDA_PREFIX .venv/bin/python conformance/gt/tables_diff.py \\
        --corpus fixtures/corpus \\
        --report conformance/gt/TABLES-REPORT.md \\
        --json   conformance/gt/tables-results.json

    # or with explicit manifests (same format as run_gt.py):
    ... --manifest conformance/gt/born_manifest.json --sample 20
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
GT_DIR = Path(__file__).resolve().parent
THIS = Path(__file__).resolve()

# Default interpreters: pdfspine -> project venv with wheel; fitz -> .venv-oracle.
DEFAULT_OXIDE_PY = str(REPO_ROOT / ".venv" / "bin" / "python")
DEFAULT_FITZ_PY = str(REPO_ROOT / ".venv-oracle" / "bin" / "python")

# score.py lives next to this file (pure stdlib token-F1 scorer).
sys.path.insert(0, str(GT_DIR))


# ===========================================================================
# WORKER MODE — runs in an isolated subprocess (pdfspine venv OR .venv-oracle).
# Never mix the two engines in one interpreter. Output: JSON list of tables.
# ===========================================================================
def _flatten_cells(extract_rows: list) -> str:
    """Flatten a table's ``extract()`` grid (list[list[cell]]) to a text blob.

    Cells may be ``None`` (empty / merged-span placeholder) or strings (fitz) or
    arbitrary scalars (pdfspine). Order is row-major; we join with spaces. This is the
    text we token-F1 score between the two engines.
    """
    parts: list[str] = []
    for row in extract_rows or []:
        for cell in row or []:
            if cell is None:
                continue
            s = cell if isinstance(cell, str) else str(cell)
            s = s.strip()
            if s:
                parts.append(s)
    return " ".join(parts)


def _as_bbox(bbox_obj) -> list[float]:
    """Normalize a table bbox to ``[x0, y0, x1, y1]`` floats.

    pdfspine returns a ``Rect`` (tuple-iterable, also has .x0/.y0/.x1/.y1); fitz
    returns a 4-tuple. Both iterate to exactly four numbers.
    """
    try:
        vals = list(bbox_obj)
    except TypeError:
        vals = [getattr(bbox_obj, a) for a in ("x0", "y0", "x1", "y1")]
    x0, y0, x1, y1 = (float(v) for v in vals[:4])
    # Normalize so x0<=x1, y0<=y1 (defensive; engines may differ on origin).
    return [min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1)]


def _table_record(tbl) -> dict:
    """Extract the comparable fields from one engine's table object."""
    rec: dict = {"bbox": None, "row_count": None, "col_count": None, "cells_text": ""}
    try:
        rec["bbox"] = _as_bbox(tbl.bbox)
    except Exception as exc:  # noqa: BLE001
        rec["bbox_error"] = f"{type(exc).__name__}: {exc}"
    # row/col count: prefer explicit attrs, else derive from extract() shape.
    try:
        rec["row_count"] = int(tbl.row_count)
    except Exception:  # noqa: BLE001
        rec["row_count"] = None
    try:
        rec["col_count"] = int(tbl.col_count)
    except Exception:  # noqa: BLE001
        rec["col_count"] = None
    # cell text + shape fallback.
    try:
        ext = tbl.extract()
    except Exception as exc:  # noqa: BLE001
        ext = None
        rec["extract_error"] = f"{type(exc).__name__}: {exc}"
    if ext is not None:
        rec["cells_text"] = _flatten_cells(ext)
        if rec["row_count"] is None:
            rec["row_count"] = len(ext)
        if rec["col_count"] is None:
            rec["col_count"] = max((len(r or []) for r in ext), default=0)
    return rec


def _worker_pdfspine(pdf: str, page_index: int) -> list[dict]:
    import pdfspine

    doc = pdfspine.open(pdf)
    try:
        page = doc.load_page(page_index)
        finder = page.find_tables()
        tables = list(getattr(finder, "tables", finder))
        return [_table_record(t) for t in tables]
    finally:
        try:
            doc.close()
        except Exception:  # noqa: BLE001
            pass


def _worker_fitz(pdf: str, page_index: int) -> list[dict]:
    import fitz  # PyMuPDF (AGPL — subprocess only, .venv-oracle)

    doc = fitz.open(pdf)
    try:
        page = doc[page_index]
        finder = page.find_tables()
        tables = list(getattr(finder, "tables", finder))
        return [_table_record(t) for t in tables]
    finally:
        try:
            doc.close()
        except Exception:  # noqa: BLE001
            pass


def _run_worker(mode: str, pdf: str, page_index: int) -> int:
    """Worker entrypoint: emit ``{"ok":bool, "tables":[...], "error":...}`` JSON.

    fitz (and MuPDF's C layer) write warnings to **stdout** ("Consider using the
    pymupdf_layout package", "MuPDF error: ..."), which would corrupt the JSON the
    parent parses. We dup the real stdout fd aside, redirect fd 1 -> fd 2 (stderr)
    for the duration of extraction so ALL such chatter (Python- AND C-level) lands
    on stderr, then write the JSON result to the saved real stdout.
    """
    out: dict = {"ok": False, "tables": [], "error": None}
    real_stdout_fd = os.dup(1)
    try:
        os.dup2(2, 1)  # fd 1 -> stderr while the engine runs
        sys.stdout = sys.stderr
        try:
            if mode == "pdfspine":
                out["tables"] = _worker_pdfspine(pdf, page_index)
            elif mode == "fitz":
                out["tables"] = _worker_fitz(pdf, page_index)
            else:
                out["error"] = f"unknown worker mode {mode!r}"
                _emit_json(real_stdout_fd, out)
                return 2
            out["ok"] = True
        except Exception as exc:  # noqa: BLE001
            out["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        sys.stdout = sys.__stdout__
    _emit_json(real_stdout_fd, out)
    return 0


def _emit_json(fd: int, obj: dict) -> None:
    """Write JSON to the saved real stdout fd, bypassing any redirection."""
    data = (json.dumps(obj) + "\n").encode("utf-8")
    with contextlib.suppress(Exception):
        os.write(fd, data)
    with contextlib.suppress(Exception):
        os.close(fd)


# ===========================================================================
# PARENT MODE — drives both workers per page, compares, reports.
# ===========================================================================
def call_worker(py: str, mode: str, pdf: Path, page_index: int, timeout: float) -> dict:
    """Spawn an isolated worker; return its JSON (or a synthesized failure rec).

    A timeout / non-zero exit / SIGABRT (Rust panic) becomes ``ok=False`` with an
    error string so one bad page can never crash the whole differential run.
    """
    cmd = [py, str(THIS), "--worker", mode, "--pdf", str(pdf), "--page", str(page_index)]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"ok": False, "tables": [], "error": f"timeout after {timeout}s"}
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "tables": [], "error": f"spawn: {type(exc).__name__}: {exc}"}
    if proc.returncode != 0:
        # SIGABRT etc. surface as a negative returncode; capture stderr tail.
        tail = (proc.stderr or "").strip().splitlines()[-1:] or [""]
        return {"ok": False, "tables": [],
                "error": f"exit {proc.returncode}: {tail[0][:200]}"}
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError:
        tail = (proc.stdout or "").strip()[-200:]
        return {"ok": False, "tables": [], "error": f"bad json: {tail!r}"}


def iou(a: list[float], b: list[float]) -> float:
    """Intersection-over-union of two ``[x0,y0,x1,y1]`` boxes; 0 on no overlap."""
    if not a or not b:
        return 0.0
    ix0, iy0 = max(a[0], b[0]), max(a[1], b[1])
    ix1, iy1 = min(a[2], b[2]), min(a[3], b[3])
    iw, ih = ix1 - ix0, iy1 - iy0
    if iw <= 0 or ih <= 0:
        return 0.0
    inter = iw * ih
    area_a = max(0.0, a[2] - a[0]) * max(0.0, a[3] - a[1])
    area_b = max(0.0, b[2] - b[0]) * max(0.0, b[3] - b[1])
    union = area_a + area_b - inter
    return inter / union if union > 0 else 0.0


def match_tables(ox: list[dict], fz: list[dict], iou_thr: float = 0.5) -> list[tuple[int, int, float]]:
    """Greedy bbox-IoU matching pdfspine->fitz. Returns [(ox_i, fz_j, iou), ...].

    Highest-IoU pairs first; each table used at most once; only pairs above the
    threshold are kept. Unmatched tables on either side are reported separately.
    """
    cands: list[tuple[float, int, int]] = []
    for i, o in enumerate(ox):
        for j, f in enumerate(fz):
            v = iou(o.get("bbox") or [], f.get("bbox") or [])
            if v > iou_thr:
                cands.append((v, i, j))
    cands.sort(reverse=True)
    used_o: set[int] = set()
    used_f: set[int] = set()
    matches: list[tuple[int, int, float]] = []
    for v, i, j in cands:
        if i in used_o or j in used_f:
            continue
        used_o.add(i)
        used_f.add(j)
        matches.append((i, j, v))
    return matches


def compare_page(ox_res: dict, fz_res: dict, score_all) -> dict:
    """Compare one page's pdfspine vs fitz table sets. Returns a per-page record."""
    ox = ox_res.get("tables", []) if ox_res.get("ok") else []
    fz = fz_res.get("tables", []) if fz_res.get("ok") else []
    n_ox, n_fz = len(ox), len(fz)

    rec: dict = {
        "ox_ok": bool(ox_res.get("ok")),
        "fz_ok": bool(fz_res.get("ok")),
        "ox_error": ox_res.get("error"),
        "fz_error": fz_res.get("error"),
        "n_ox": n_ox,
        "n_fz": n_fz,
        "count_match": n_ox == n_fz,
        "matched": [],          # per matched pair: iou, shape match, cell f1
        "n_matched": 0,
        "shape_matches": 0,     # of matched pairs, how many had exact (rows,cols)
        "cell_f1_sum": 0.0,
    }
    # Only compare structure when both workers succeeded.
    if not (ox_res.get("ok") and fz_res.get("ok")):
        return rec

    for i, j, v in match_tables(ox, fz):
        o, f = ox[i], fz[j]
        shape_ok = (o.get("row_count") == f.get("row_count")
                    and o.get("col_count") == f.get("col_count"))
        sc = score_all(o.get("cells_text", ""), f.get("cells_text", ""))
        rec["matched"].append({
            "iou": round(v, 3),
            "ox_shape": [o.get("row_count"), o.get("col_count")],
            "fz_shape": [f.get("row_count"), f.get("col_count")],
            "shape_ok": shape_ok,
            "cell_f1": round(sc["f1"], 4),
            "cell_jaccard": round(sc["jaccard"], 4),
        })
        rec["n_matched"] += 1
        rec["shape_matches"] += int(shape_ok)
        rec["cell_f1_sum"] += sc["f1"]
    return rec


def guess_cause(doc_rec: dict) -> str:
    """One-line heuristic cause for a doc's divergence (for the worst-N table)."""
    n_ox = doc_rec["tot_ox"]
    n_fz = doc_rec["tot_fz"]
    nm = doc_rec["n_matched"]
    if doc_rec["ox_fail_pages"] and not doc_rec["fz_fail_pages"]:
        return "pdfspine worker failed/panicked on some page(s)"
    if doc_rec["fz_fail_pages"] and not doc_rec["ox_fail_pages"]:
        return "fitz worker failed on some page(s)"
    if n_ox == 0 and n_fz > 0:
        return "pdfspine finds NO tables where fitz does (detection miss)"
    if n_fz == 0 and n_ox > 0:
        return "pdfspine finds tables where fitz finds none (over-detection)"
    if n_ox < n_fz:
        return f"pdfspine under-segments: {n_ox} vs fitz {n_fz} tables (merges/misses)"
    if n_ox > n_fz:
        return f"pdfspine over-segments: {n_ox} vs fitz {n_fz} tables (splits/spurious)"
    if nm > 0 and doc_rec["shape_match_rate"] < 0.5:
        return "tables overlap but GRID shape disagrees (row/col boundary detection)"
    if nm > 0 and doc_rec["mean_cell_f1"] < 0.5:
        return "tables & grid align but CELL TEXT diverges (cell assignment/text)"
    return "minor / mixed divergence"


def process_doc(pdf: Path, doc_id: str, pdfspine_py: str, fitz_py: str,
                timeout: float, score_all) -> dict:
    """Run both engines over every page of one PDF; aggregate per-doc metrics."""
    # Page count from the pdfspine side (cheap, in-process is fine here — just count).
    try:
        import pdfspine  # noqa: PLC0415
        d = pdfspine.open(str(pdf))
        n_pages = d.page_count
        d.close()
    except Exception as exc:  # noqa: BLE001
        return {"id": doc_id, "pdf": str(pdf), "error": f"open: {type(exc).__name__}: {exc}",
                "pages": [], "n_pages": 0}

    page_recs: list[dict] = []
    for p in range(n_pages):
        ox_res = call_worker(pdfspine_py, "pdfspine", pdf, p, timeout)
        fz_res = call_worker(fitz_py, "fitz", pdf, p, timeout)
        page_recs.append(compare_page(ox_res, fz_res, score_all))

    # Aggregate over pages.
    tot_ox = sum(r["n_ox"] for r in page_recs)
    tot_fz = sum(r["n_fz"] for r in page_recs)
    n_matched = sum(r["n_matched"] for r in page_recs)
    shape_matches = sum(r["shape_matches"] for r in page_recs)
    cell_f1_sum = sum(r["cell_f1_sum"] for r in page_recs)
    count_match_pages = sum(1 for r in page_recs if r["count_match"])
    ox_fail = [i for i, r in enumerate(page_recs) if not r["ox_ok"]]
    fz_fail = [i for i, r in enumerate(page_recs) if not r["fz_ok"]]

    doc = {
        "id": doc_id,
        "pdf": str(pdf),
        "n_pages": n_pages,
        "tot_ox": tot_ox,
        "tot_fz": tot_fz,
        "n_matched": n_matched,
        "count_match_pages": count_match_pages,
        "count_agree_rate": (count_match_pages / n_pages) if n_pages else 1.0,
        "shape_match_rate": (shape_matches / n_matched) if n_matched else None,
        "mean_cell_f1": (cell_f1_sum / n_matched) if n_matched else None,
        "ox_fail_pages": ox_fail,
        "fz_fail_pages": fz_fail,
        "pages": page_recs,
    }
    doc["cause"] = guess_cause(doc)
    return doc


# --------------------------------------------------------------------------- #
# Manifest / corpus input gathering (mirrors run_gt.py loaders).
# --------------------------------------------------------------------------- #
def load_manifest_entries(path: Path) -> list[tuple[str, Path]]:
    data = json.loads(path.read_text(encoding="utf-8"))
    entries = data if isinstance(data, list) else (data.get("entries") or data.get("documents") or [])
    out: list[tuple[str, Path]] = []
    mdir = path.resolve().parent
    for e in entries:
        raw = e.get("pdf") or e.get("path") or e.get("file")
        if not raw:
            continue
        pdf = Path(raw)
        if not pdf.is_absolute():
            pdf = mdir / pdf
        doc_id = e.get("id") or pdf.name
        out.append((str(doc_id), pdf))
    return out


def gather_inputs(args) -> list[tuple[str, Path]]:
    inputs: list[tuple[str, Path]] = []
    for m in args.manifest or []:
        inputs.extend(load_manifest_entries(m))
    for c in args.corpus or []:
        for pdf in sorted(Path(c).glob("*.pdf")):
            inputs.append((pdf.name, pdf))
    # De-dup by resolved path, keep first id.
    seen: set[str] = set()
    uniq: list[tuple[str, Path]] = []
    for doc_id, pdf in inputs:
        key = str(pdf.resolve())
        if key in seen:
            continue
        seen.add(key)
        uniq.append((doc_id, pdf))
    if args.sample and args.sample < len(uniq):
        uniq = uniq[: args.sample]
    return uniq


# --------------------------------------------------------------------------- #
# Reporting
# --------------------------------------------------------------------------- #
def build_report(docs: list[dict], pdfspine_py: str, fitz_py: str,
                 fitz_version: str | None) -> str:
    valid = [d for d in docs if "error" not in d]
    # Aggregate (page- and table-weighted).
    tot_pages = sum(d["n_pages"] for d in valid)
    count_agree_pages = sum(d["count_match_pages"] for d in valid)
    tot_ox = sum(d["tot_ox"] for d in valid)
    tot_fz = sum(d["tot_fz"] for d in valid)
    tot_matched = sum(d["n_matched"] for d in valid)
    shape_matches = sum(sum(r["shape_matches"] for r in d["pages"]) for d in valid)
    cell_f1_sum = sum(sum(r["cell_f1_sum"] for r in d["pages"]) for d in valid)

    count_agree_rate = (count_agree_pages / tot_pages) if tot_pages else 0.0
    shape_match_rate = (shape_matches / tot_matched) if tot_matched else 0.0
    mean_cell_f1 = (cell_f1_sum / tot_matched) if tot_matched else 0.0
    match_recall_ox = (tot_matched / tot_ox) if tot_ox else 0.0
    match_recall_fz = (tot_matched / tot_fz) if tot_fz else 0.0

    L: list[str] = []
    L.append("# Table-extraction differential — pdfspine vs fitz\n")
    L.append(f"Harness: `{THIS}`  ")
    L.append(f"pdfspine python: `{pdfspine_py}`  ")
    L.append(f"fitz python:  `{fitz_py}` (PyMuPDF {fitz_version or '?'})  ")
    L.append("Match rule: bbox IoU > 0.5; grid-shape = exact (rows,cols); "
             "cell-text = token-F1 (`gt/score.py`) of flattened cells, pdfspine-vs-fitz.\n")

    L.append("## Aggregate (pdfspine vs fitz)\n")
    L.append(f"- Documents scored: **{len(valid)}** ({len(docs)-len(valid)} open-errors), "
             f"pages: **{tot_pages}**")
    L.append(f"- Tables detected: pdfspine **{tot_ox}**, fitz **{tot_fz}** "
             f"(ratio pdfspine/fitz = {tot_ox/tot_fz:.2f})" if tot_fz else
             f"- Tables detected: pdfspine **{tot_ox}**, fitz **{tot_fz}**")
    L.append(f"- **Table-count agreement** (per-page #pdfspine==#fitz): "
             f"**{count_agree_rate*100:.1f}%** ({count_agree_pages}/{tot_pages} pages)")
    L.append(f"- Tables matched by IoU>0.5: **{tot_matched}** "
             f"(= {match_recall_ox*100:.0f}% of pdfspine, {match_recall_fz*100:.0f}% of fitz tables)")
    L.append(f"- **Grid-shape match** on matched pairs (exact rows×cols): "
             f"**{shape_match_rate*100:.1f}%** ({shape_matches}/{tot_matched})")
    L.append(f"- **Mean cell-text F1** on matched pairs: **{mean_cell_f1:.3f}**\n")

    # Per-doc table (sorted worst-count-agreement first).
    L.append("## Per-document\n")
    L.append("| doc | pages | ox tbl | fz tbl | cnt-agree | matched | shape% | cell-F1 |")
    L.append("|-----|------:|-------:|-------:|----------:|--------:|-------:|--------:|")
    for d in sorted(valid, key=lambda x: (x["count_agree_rate"], x["mean_cell_f1"] or 0.0)):
        shape = f"{d['shape_match_rate']*100:.0f}%" if d["shape_match_rate"] is not None else "—"
        f1 = f"{d['mean_cell_f1']:.3f}" if d["mean_cell_f1"] is not None else "—"
        L.append(f"| {d['id']} | {d['n_pages']} | {d['tot_ox']} | {d['tot_fz']} | "
                 f"{d['count_agree_rate']*100:.0f}% | {d['n_matched']} | {shape} | {f1} |")
    L.append("")

    # Open errors.
    errd = [d for d in docs if "error" in d]
    if errd:
        L.append("## Documents that failed to open\n")
        for d in errd:
            L.append(f"- `{d['id']}`: {d['error']}")
        L.append("")

    # Worst divergences with cause guesses.
    def _divergence_score(d: dict) -> float:
        # higher == worse: count disagreement + shape miss + cell-f1 deficit.
        cnt = 1.0 - d["count_agree_rate"]
        shp = (1.0 - d["shape_match_rate"]) if d["shape_match_rate"] is not None else 0.5
        f1 = (1.0 - d["mean_cell_f1"]) if d["mean_cell_f1"] is not None else 0.5
        return cnt * 2 + shp + f1

    worst = sorted(valid, key=_divergence_score, reverse=True)[:8]
    L.append("## Worst divergences (one-line cause guess)\n")
    for d in worst:
        shape = f"{d['shape_match_rate']*100:.0f}%" if d["shape_match_rate"] is not None else "—"
        f1 = f"{d['mean_cell_f1']:.3f}" if d["mean_cell_f1"] is not None else "—"
        L.append(f"- **{d['id']}** — ox {d['tot_ox']} / fz {d['tot_fz']} tables, "
                 f"shape {shape}, cellF1 {f1} → _{d['cause']}_")
    L.append("")

    # Verdict.
    L.append("## Verdict\n")
    verdict = _verdict(count_agree_rate, shape_match_rate, mean_cell_f1,
                       match_recall_ox, match_recall_fz, tot_ox, tot_fz)
    L.extend(verdict)
    L.append("")

    L.append("## Notes / follow-ups\n")
    L.append("- This is parity-with-fitz *agreement*, not accuracy vs a human gold; "
             "fitz table detection is itself heuristic (lattice/stream) and imperfect.")
    L.append("- FinTabNet structural ground truth was considered as an objective anchor "
             "but **skipped** to keep the run self-contained and fast (HF fetch is heavy/"
             "flaky); add it later for an absolute structure score.")
    L.append("- All numbers are pdfspine-vs-fitz; no Rust changes were made.")
    return "\n".join(L)


def _verdict(cnt: float, shp: float, f1: float, rec_ox: float, rec_fz: float,
             tot_ox: int, tot_fz: int) -> list[str]:
    lines: list[str] = []
    # Overall headline.
    if cnt >= 0.8 and shp >= 0.7 and f1 >= 0.7:
        lines.append("**Strong parity.** pdfspine's `find_tables` largely agrees with fitz on "
                     "count, grid shape, and cell text.")
    elif cnt >= 0.5 and f1 >= 0.5:
        lines.append("**Partial parity.** pdfspine detects tables in the same regions as fitz, "
                     "but diverges on segmentation and/or grid structure on a meaningful share "
                     "of pages.")
    else:
        lines.append("**Weak parity.** pdfspine's table detection diverges substantially from "
                     "fitz on this corpus — treat `find_tables` as experimental.")
    # Direction of detection bias.
    if tot_fz and tot_ox / tot_fz < 0.75:
        lines.append(f"- pdfspine **under-detects**: {tot_ox} tables vs fitz {tot_fz} "
                     "(merging adjacent tables or missing them).")
    elif tot_fz and tot_ox / tot_fz > 1.33:
        lines.append(f"- pdfspine **over-detects**: {tot_ox} tables vs fitz {tot_fz} "
                     "(splitting one table into many / spurious detections).")
    else:
        lines.append(f"- Table counts are broadly comparable ({tot_ox} pdfspine / {tot_fz} fitz).")
    lines.append(f"- Of matched (overlapping) tables, exact grid shape agrees "
                 f"{shp*100:.0f}% of the time and cell text scores F1 {f1:.2f}.")
    return lines


# --------------------------------------------------------------------------- #
# Oracle version probe (for the report header).
# --------------------------------------------------------------------------- #
def probe_fitz_version(fitz_py: str) -> str | None:
    try:
        proc = subprocess.run(
            [fitz_py, "-c", "import fitz;print(getattr(fitz,'VersionBind',''))"],
            capture_output=True, text=True, timeout=30,
        )
        v = (proc.stdout or "").strip()
        return v or None
    except Exception:  # noqa: BLE001
        return None


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="pdfspine-vs-fitz table-extraction diff")
    # Hidden worker mode (re-invoked as a subprocess).
    ap.add_argument("--worker", choices=["pdfspine", "fitz"], default=None,
                    help=argparse.SUPPRESS)
    ap.add_argument("--pdf", default=None, help=argparse.SUPPRESS)
    ap.add_argument("--page", type=int, default=0, help=argparse.SUPPRESS)
    # Parent CLI.
    ap.add_argument("--manifest", type=Path, action="append", default=[],
                    help="corpus manifest JSON (repeatable; same format as run_gt.py)")
    ap.add_argument("--corpus", type=Path, action="append", default=[],
                    help="directory of *.pdf to scan (repeatable)")
    ap.add_argument("--sample", type=int, default=None, help="cap to first N documents")
    ap.add_argument("--pdfspine-python", default=DEFAULT_OXIDE_PY)
    ap.add_argument("--fitz-python", default=DEFAULT_FITZ_PY)
    ap.add_argument("--timeout", type=float, default=90.0,
                    help="per-(page,engine) wall-clock timeout (s)")
    # Defaults match existing gitignore globs so raw outputs are not tracked:
    #   gt-*.json is ignored; GT-REPORT*.md follows the committed-report convention.
    ap.add_argument("--report", type=Path, default=GT_DIR / "GT-REPORT-tables.md")
    ap.add_argument("--json", type=Path, default=GT_DIR / "gt-tables.json")
    args = ap.parse_args(argv)

    # Worker dispatch (isolated subprocess).
    if args.worker:
        if not args.pdf:
            sys.stdout.write(json.dumps({"ok": False, "tables": [], "error": "no --pdf"}))
            return 2
        return _run_worker(args.worker, args.pdf, args.page)

    # Parent.
    from score import score_all  # local, pure stdlib

    if not args.manifest and not args.corpus:
        # Default to the fixtures corpus (the IRS/GovInfo/CDC set).
        default_corpus = REPO_ROOT / "fixtures" / "corpus"
        if default_corpus.exists():
            args.corpus = [default_corpus]
        else:
            ap.error("provide --manifest and/or --corpus (no default corpus found)")

    if not Path(args.fitz_python).exists():
        print(f"WARNING: fitz python not found at {args.fitz_python}; fitz side will fail.",
              file=sys.stderr)

    inputs = gather_inputs(args)
    if not inputs:
        ap.error("no input PDFs found")
    fitz_version = probe_fitz_version(args.fitz_python)
    print(f"Scoring {len(inputs)} document(s). fitz={fitz_version}", flush=True)

    docs: list[dict] = []
    for k, (doc_id, pdf) in enumerate(inputs, 1):
        if not pdf.exists():
            docs.append({"id": doc_id, "pdf": str(pdf), "error": "pdf not found",
                         "pages": [], "n_pages": 0})
            print(f"[{k}/{len(inputs)}] {doc_id}: MISSING", flush=True)
            continue
        d = process_doc(pdf, doc_id, args.pdfspine_python, args.fitz_python,
                        args.timeout, score_all)
        docs.append(d)
        if "error" in d:
            print(f"[{k}/{len(inputs)}] {doc_id}: open-error {d['error']}", flush=True)
        else:
            shp = (f"{d['shape_match_rate']*100:.0f}%"
                   if d["shape_match_rate"] is not None else "—")
            f1 = f"{d['mean_cell_f1']:.2f}" if d["mean_cell_f1"] is not None else "—"
            print(f"[{k}/{len(inputs)}] {doc_id}: ox={d['tot_ox']} fz={d['tot_fz']} "
                  f"matched={d['n_matched']} shape={shp} cellF1={f1}", flush=True)

    # Write JSON + report.
    payload = {
        "pdfspine_python": args.pdfspine_python,
        "fitz_python": args.fitz_python,
        "fitz_version": fitz_version,
        "timeout": args.timeout,
        "n_docs": len(docs),
        "docs": docs,
    }
    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    report = build_report(docs, args.pdfspine_python, args.fitz_python, fitz_version)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(report, encoding="utf-8")
    print(f"\nWrote {args.json}\nWrote {args.report}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
