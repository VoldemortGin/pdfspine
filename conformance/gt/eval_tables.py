#!/usr/bin/env python3
"""Financial-report table-structure evaluation set — scoring CLI.

This is the harness behind the table-structure evaluation set: it runs pdfspine
``Page.find_tables`` with one or more backends over a gold corpus of
financial-report pages, scores every gold table, and writes a machine-readable
JSON plus a markdown report in the style of ``GT-REPORT-tables-gold.md``.

Metrics (all pure stdlib, see the sibling modules)
--------------------------------------------------
* **GriTS_Top / GriTS_Con** (``grits.py``) — the canonical FinTabNet.c metric.
  Grid-based, position- and transpose-invariant, with per-cell partial
  credit. ``Top`` scores cell topology (row/col spans); ``Con`` scores cell
  text content. Directly comparable to the Table-Transformer papers.
* **TEDS-Struct** (``table_metrics.teds_struct``) — the PubTabNet /
  ICDAR-2021 tree-edit-distance metric, structure only. Penalises every
  structural edit, so it disagrees with GriTS in informative ways (a merged
  header row is one edit in TEDS, a whole row of partial credit in GriTS).
  ``None`` (tree too large / timed out) is reported as ``teds_skipped`` and is
  never counted as 0.
* **Cell-alignment F1** (``table_metrics.cell_alignment``) — bbox IoU
  precision/recall of predicted vs gold cells. GriTS and TEDS are position
  invariant by design; this is the number a downstream consumer (crop the
  cell, read the figure) actually feels.

Two modes, and why ``gold-crop`` exists
----------------------------------------
* ``e2e`` — one ``find_tables`` call per page; predicted tables are matched to
  gold tables greedily by bbox IoU (``--iou``). Missed gold tables score 0 on
  every metric (recall-weighted, the FinTabNet.c convention). Detection
  precision/recall/F1 are reported alongside.
* ``gold-crop`` — one call per gold table with ``clip=<gold bbox>``; for the
  ONNX backend ``vision_options={"skip_layout": True}`` bypasses the layout
  detector so only the structure model (SLANet-plus) is exercised. This is
  the *TSR-only* measurement that ``docs/adr/0002-table-structure-backends.md``
  names as its decision gate: the default structure model changes only when a
  candidate's structure-stage GriTS_Con beats the incumbent on the 186-table
  slice, and that comparison is only apples-to-apples against Microsoft's
  published numbers when the detector is taken out of the loop. Note that the
  native ``lines``/``text`` strategies do not honour ``clip`` (pdfspine only
  forwards it to the vision backends); for them gold-crop degenerates to
  "best-overlap table on the full page" and the run is flagged
  ``clip_honored: false``.

Adding your own financial-report pages
--------------------------------------
Drop a PDF under ``conformance/gt/corpus-finance/pdfs/`` and a
``<stem>_p<N>.gold.json`` (or ``.gold.html``) under
``conformance/gt/corpus-finance/annotations/`` — ``draft-gold`` writes a
machine draft for you to correct. See ``conformance/gt/corpus-finance/README.md``
and ``table_gold.py`` for the accepted schemas.

Usage::

    python conformance/gt/eval_tables.py run --backend lines --backend onnx \\
        --mode gold-crop --report conformance/gt/GT-REPORT-tables-eval.md
    python conformance/gt/eval_tables.py draft-gold --pdf x.pdf --page 3
    python conformance/gt/eval_tables.py seed-subset --size 40
"""

from __future__ import annotations

import argparse
from concurrent.futures import ProcessPoolExecutor
from datetime import datetime, timezone
import json
import logging
import os
from pathlib import Path
import random
import statistics
import subprocess
import sys
import time
import warnings

GT_DIR = Path(__file__).resolve().parent
REPO_ROOT = GT_DIR.parents[1]
sys.path.insert(0, str(GT_DIR))

from grits import grits_con, grits_top  # noqa: E402
from table_gold import (  # noqa: E402
    TAG_VOCABULARY,
    GoldPage,
    GoldTable,
    cells_to_html,
    grid_shape,
    load_gold_page_file,
    load_manifest,
    normalize_cell,
    page_tags,
    table_tags,
)
from table_metrics import cell_alignment, teds_struct  # noqa: E402
from tables_diff import (  # noqa: E402
    DEFAULT_FITZ_PY,
    _pred_cells_from_html,
    _table_record,
    call_worker,
    iou,
    match_tables,
)

SCHEMA = "pdfspine.table-eval.v1"
SEED_SCHEMA = "pdfspine.table-eval.seed-subset.v1"
BACKENDS = ("lines", "text", "onnx", "tatr")
MODES = ("e2e", "gold-crop")
FITZ_BACKEND = "fitz-oracle"
DEFAULT_CORPUS = GT_DIR / "corpus-fintabnet"
DEFAULT_OUT = GT_DIR / "ci-tables-eval-results.json"
FINANCE_DIR = GT_DIR / "corpus-finance"
STRUCTURAL_TAGS = {"multi-header", "spanning", "wide", "tall"}
DELTA_METRICS = (
    "n_gold_tables",
    "n_pred_tables",
    "n_matched",
    "detection_precision",
    "detection_recall",
    "detection_f1",
    "recall_weighted.grits_top_mean",
    "recall_weighted.grits_con_mean",
    "recall_weighted.teds_struct_mean",
    "recall_weighted.cell_f1_mean",
    "matched_only.grits_top_mean",
    "matched_only.grits_con_mean",
    "matched_only.teds_struct_mean",
    "matched_only.cell_f1_mean",
    "per_table_mean_s",
)

_log = logging.getLogger("eval_tables")


# --------------------------------------------------------------------------- #
# Small helpers
# --------------------------------------------------------------------------- #
def _now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _r3(v: object) -> str:
    if v is None:
        return "n/a"
    if isinstance(v, bool):
        return str(v)
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        return f"{v:.3f}"
    return str(v)


def _mean(xs: list[float]) -> float | None:
    return statistics.fmean(xs) if xs else None


def _median(xs: list[float]) -> float | None:
    return statistics.median(xs) if xs else None


def _git(*args: str) -> str | None:
    try:
        proc = subprocess.run(
            ["git", *args], cwd=GT_DIR, capture_output=True, text=True, timeout=10
        )
    except Exception:  # noqa: BLE001
        return None
    if proc.returncode != 0:
        return None
    return proc.stdout.strip() or None


def _code_version() -> dict:
    version = None
    try:
        import pdfspine

        version = getattr(pdfspine, "__version__", None)
    except Exception:  # noqa: BLE001
        pass
    return {
        "commit": _git("rev-parse", "--short", "HEAD"),
        "branch": _git("rev-parse", "--abbrev-ref", "HEAD"),
        "pdfspine": version,
    }


def _display_path(path: Path) -> str:
    """Repo-relative when inside the checkout, else absolute (kept portable)."""
    try:
        return str(Path(os.path.abspath(path)).relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def _resolve_fitz_py(explicit: str | None) -> str:
    """``--fitz-py`` > ``tables_diff.DEFAULT_FITZ_PY`` > sibling of the running venv."""
    if explicit:
        return explicit
    if Path(DEFAULT_FITZ_PY).exists():
        return DEFAULT_FITZ_PY
    # Do not resolve(): sys.executable is a venv symlink to the base interpreter.
    sibling = Path(sys.executable).parents[2] / ".venv-oracle" / "bin"
    for name in ("python", "python.exe"):
        cand = sibling / name
        if cand.exists():
            return str(cand)
    return DEFAULT_FITZ_PY


# --------------------------------------------------------------------------- #
# Corpus loading
# --------------------------------------------------------------------------- #
def _gold_file_pdf(gold_path: Path, page: GoldPage, corpus_dir: Path) -> Path | None:
    """Resolve the PDF for a hand-written gold file.

    Order: a ``"pdf"`` key inside the JSON (relative to the file, then to the
    corpus dir); ``pdfs/<document_id>.pdf``; ``pdfs/<stem>.pdf`` with a
    trailing ``_p<N>`` page suffix stripped.
    """
    candidates: list[Path] = []
    if gold_path.name.lower().endswith(".gold.json"):
        try:
            data = json.loads(gold_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            data = None
        raw = data.get("pdf") if isinstance(data, dict) else None
        if raw:
            p = Path(str(raw))
            candidates += (
                [p] if p.is_absolute() else [gold_path.parent / p, corpus_dir / p]
            )
    stem = page.document_id
    pdf_dir = corpus_dir / "pdfs"
    candidates.append(pdf_dir / f"{stem}.pdf")
    head, sep, tail = stem.rpartition("_p")
    if sep and tail.isdigit():
        candidates.append(pdf_dir / f"{head}.pdf")
    for cand in candidates:
        if cand.exists():
            return cand
    return None


def load_corpus(path: Path, *, skipped: list[str] | None = None) -> list[GoldPage]:
    """Load either a FinTabNet.c manifest corpus or a directory of gold files.

    * ``<dir>/manifest.json`` (or a manifest file path) -> ``load_manifest``.
    * otherwise every ``*.gold.json`` / ``*.gold.html`` in ``<dir>`` and
      ``<dir>/annotations`` -> ``load_gold_page_file``, PDF resolved from
      ``<dir>/pdfs``.
    """
    path = Path(path)
    if path.is_file():
        return load_manifest(path, skipped=skipped)
    manifest = path / "manifest.json"
    if manifest.exists():
        return load_manifest(manifest, skipped=skipped)
    files: list[Path] = []
    for sub in (path, path / "annotations"):
        if not sub.is_dir():
            continue
        found = [
            p
            for p in sorted(sub.iterdir())
            if p.name.lower().endswith((".gold.json", ".gold.html"))
        ]
        json_stems = {
            p.name[: -len(".gold.json")]
            for p in found
            if p.name.lower().endswith(".gold.json")
        }
        # A ``.gold.html`` next to its ``.gold.json`` twin is the preview
        # ``draft-gold`` writes, not a second page.
        files += [
            p
            for p in found
            if not (
                p.name.lower().endswith(".gold.html")
                and p.name[: -len(".gold.html")] in json_stems
            )
        ]
    pages: list[GoldPage] = []
    for f in files:
        try:
            page = load_gold_page_file(f)
        except (OSError, ValueError) as exc:
            _log.warning("unreadable gold file %s: %s", f, exc)
            if skipped is not None:
                skipped.append(f.name)
            continue
        page.pdf = _gold_file_pdf(f, page, path)
        pages.append(page)
    return pages


def _select_pages(pages: list[GoldPage], args) -> list[GoldPage]:
    wanted: set[str] = set()
    for chunk in args.pages or []:
        wanted.update(s.strip() for s in chunk.split(",") if s.strip())
    if wanted:
        pages = [p for p in pages if p.document_id in wanted]
        missing = wanted - {p.document_id for p in pages}
        if missing:
            print(
                f"warning: --pages ids not in corpus: {', '.join(sorted(missing))}",
                file=sys.stderr,
            )
    if args.limit is not None:
        pages = pages[: args.limit]
    return pages


# --------------------------------------------------------------------------- #
# Prediction (runs in the worker process; everything picklable)
# --------------------------------------------------------------------------- #
_DOC_CACHE: dict[str, object] = {}


def _open_page(pdf: str, page_index: int):
    """``pdfspine.open`` with a one-document cache (same PDF reused per worker)."""
    import pdfspine

    doc = _DOC_CACHE.get(pdf)
    if doc is None:
        for old in _DOC_CACHE.values():
            try:
                old.close()
            except Exception:  # noqa: BLE001
                pass
        _DOC_CACHE.clear()
        doc = pdfspine.open(pdf)
        _DOC_CACHE[pdf] = doc
    return doc[page_index]


def _find_tables_kwargs(
    backend: str, mode: str, clip: list[float] | None, dpi: int | None
) -> dict:
    if backend == "lines":
        kw: dict = {"strategy": "lines"}
    elif backend == "text":
        kw = {"strategy": "text"}
    elif backend == "onnx":
        vo: dict = {}
        if dpi:
            vo["dpi"] = int(dpi)
        if mode == "gold-crop":
            vo["skip_layout"] = True
        kw = {"strategy": "vision", "backend": "onnx", "vision_options": vo}
    elif backend == "tatr":
        kw = {"strategy": "vision"}
        if dpi:
            kw["vision_options"] = {"dpi": int(dpi)}
    else:
        raise ValueError(f"unknown backend {backend!r}")
    if clip is not None:
        kw["clip"] = tuple(float(v) for v in clip)
    return kw


def _classify_error(exc: BaseException) -> str:
    """``"unavailable"`` for a missing runtime / model, ``"error"`` otherwise."""
    if isinstance(exc, (ImportError, ModuleNotFoundError)):
        return "unavailable"
    try:
        from pdfspine import PdfUnsupportedError

        if isinstance(exc, PdfUnsupportedError):
            return "unavailable"
    except Exception:  # noqa: BLE001
        pass
    return "error"


def _slim_record(rec: dict) -> dict:
    return {
        "bbox": rec.get("bbox"),
        "row_count": rec.get("row_count"),
        "col_count": rec.get("col_count"),
        "cells": rec.get("cells") or [],
        "html": rec.get("html"),
    }


def predict(
    pdf: str,
    page_index: int,
    backend: str,
    mode: str,
    clip: list[float] | None,
    opts: dict,
) -> dict:
    """Run one backend on one page (optionally clipped). Never raises."""
    t0 = time.perf_counter()
    try:
        if backend == FITZ_BACKEND:
            res = call_worker(
                opts["fitz_py"], "fitz", Path(pdf), page_index, float(opts["timeout"])
            )
            if not res.get("ok"):
                err = str(res.get("error") or "fitz worker failed")
                kind = "unavailable" if "No module named" in err else "error"
                if "spawn:" in err and "No such file" in err:
                    kind = "unavailable"
                return {
                    "ok": False,
                    "error": err,
                    "error_kind": kind,
                    "elapsed_s": time.perf_counter() - t0,
                }
            tables = [_slim_record(r) for r in res.get("tables") or []]
        else:
            with warnings.catch_warnings():
                warnings.simplefilter("ignore")
                page = _open_page(pdf, page_index)
                kw = _find_tables_kwargs(backend, mode, clip, opts.get("dpi"))
                finder = page.find_tables(**kw)
                tables = [_slim_record(_table_record(t)) for t in finder.tables]
    except Exception as exc:  # noqa: BLE001
        return {
            "ok": False,
            "error": f"{type(exc).__name__}: {exc}",
            "error_kind": _classify_error(exc),
            "elapsed_s": time.perf_counter() - t0,
        }
    return {"ok": True, "tables": tables, "elapsed_s": time.perf_counter() - t0}


# --------------------------------------------------------------------------- #
# Scoring (shared by the single-process and the pool path)
# --------------------------------------------------------------------------- #
def _pred_cells(record: dict) -> list[dict]:
    """Predicted GriTS cells *with* bbox (direct spans first, HTML fallback)."""
    out: list[dict] = []
    for cell in record.get("cells") or []:
        rows = [int(v) for v in cell.get("row_nums") or []]
        cols = [int(v) for v in cell.get("column_nums") or []]
        if not rows or not cols:
            continue
        out.append(
            {
                "row_nums": rows,
                "column_nums": cols,
                "cell_text": " ".join(str(cell.get("cell_text") or "").split()),
                "bbox": cell.get("bbox"),
            }
        )
    return out if out else _pred_cells_from_html(record.get("html"))


def score_table(
    gold: dict, pred: dict | None, iou_value: float | None, opts: dict
) -> dict:
    """Score one gold table against its matched prediction (``None`` = miss)."""
    out: dict = {
        "table_id": gold["table_id"],
        "matched": pred is not None,
        "iou": iou_value,
        "gold_shape": list(grid_shape(gold["cells"])),
        "pred_shape": None,
        "grits_top": 0.0,
        "grits_con": 0.0,
        "teds_struct": 0.0,
        "cell_f1": 0.0,
        "cell_precision": 0.0,
        "cell_recall": 0.0,
        "cell_matched": 0,
    }
    if pred is None:
        return out
    pred_cells = _pred_cells(pred)
    out["pred_shape"] = (
        list(grid_shape(pred_cells))
        if pred_cells
        else [
            int(pred.get("row_count") or 0),
            int(pred.get("col_count") or 0),
        ]
    )
    if not pred_cells:
        return out
    out["grits_top"] = grits_top(gold["cells"], pred_cells)[0]
    out["grits_con"] = grits_con(gold["cells"], pred_cells)[0]
    out["teds_struct"] = teds_struct(
        gold["cells"],
        pred_cells,
        max_nodes=int(opts.get("teds_max_nodes") or 2000),
        timeout_s=float(opts.get("teds_timeout") or 20.0),
    )
    align = cell_alignment(
        gold["cells"], pred_cells, iou_threshold=float(opts.get("iou") or 0.5)
    )
    out["cell_f1"] = align["f1"]
    out["cell_precision"] = align["precision"]
    out["cell_recall"] = align["recall"]
    out["cell_matched"] = align["matched"]
    if align.get("skipped_reason"):
        out["cell_skipped_reason"] = align["skipped_reason"]
    return out


def run_task(task: dict) -> dict:
    """One unit of work: a page (``e2e``) or a single gold table (``gold-crop``).

    Returns ``{"status": "ok"|"error"|"unavailable", "records": [...],
    "n_pred": int, "elapsed_s": float, "error": str|None}``. Records carry
    ``document_id``/``table_id``/``backend``/``mode`` plus the metric fields;
    a gold table without bbox is emitted as ``{"skipped": "no gold bbox"}``.
    """
    backend, mode, opts = task["backend"], task["mode"], task["opts"]
    base = {"document_id": task["document_id"], "backend": backend, "mode": mode}
    golds = task["gold_tables"]
    clip = golds[0]["bbox"] if mode == "gold-crop" else None
    if mode == "gold-crop" and clip is None:
        return {
            "status": "ok",
            "error": None,
            "records": [
                {**base, "table_id": golds[0]["table_id"], "skipped": "no gold bbox"}
            ],
            "n_pred": 0,
            "elapsed_s": 0.0,
        }
    pred = predict(task["pdf"], task["page_index"], backend, mode, clip, opts)
    if not pred["ok"]:
        return {
            "status": pred["error_kind"],
            "error": pred["error"],
            "records": [],
            "n_pred": 0,
            "elapsed_s": pred["elapsed_s"],
        }
    tables = pred["tables"]
    records: list[dict] = []
    if mode == "e2e":
        boxed = [g for g in golds if g["bbox"] is not None]
        matches = match_tables(boxed, tables, float(opts.get("iou") or 0.5))
        by_gold = {gi: (pj, v) for gi, pj, v in matches}
        gi = 0
        for g in golds:
            if g["bbox"] is None:
                records.append(
                    {**base, "table_id": g["table_id"], "skipped": "no gold bbox"}
                )
                continue
            pj, v = by_gold.get(gi, (None, None))
            gi += 1
            rec = score_table(g, tables[pj] if pj is not None else None, v, opts)
            records.append({**base, **rec, "elapsed_s": pred["elapsed_s"]})
    else:
        g = golds[0]
        best, best_iou = None, 0.0
        for t in tables:
            v = iou(g["bbox"], t.get("bbox") or [])
            if v > best_iou:
                best, best_iou = t, v
        rec = score_table(g, best, best_iou if best is not None else None, opts)
        records.append({**base, **rec, "elapsed_s": pred["elapsed_s"]})
    return {
        "status": "ok",
        "error": None,
        "records": records,
        "n_pred": len(tables),
        "elapsed_s": pred["elapsed_s"],
    }


# --------------------------------------------------------------------------- #
# Task building / execution
# --------------------------------------------------------------------------- #
def _gold_payload(t: GoldTable) -> dict:
    return {"table_id": t.table_id, "bbox": t.bbox, "cells": t.cells}


def build_tasks(
    pages: list[GoldPage], backend: str, mode: str, opts: dict
) -> list[dict]:
    tasks: list[dict] = []
    for page in pages:
        common = {
            "document_id": page.document_id,
            "pdf": str(page.pdf),
            "page_index": page.page_index,
            "backend": backend,
            "mode": mode,
            "opts": opts,
        }
        if mode == "e2e":
            tasks.append(
                {**common, "gold_tables": [_gold_payload(t) for t in page.tables]}
            )
        else:
            for t in page.tables:
                tasks.append({**common, "gold_tables": [_gold_payload(t)]})
    return tasks


def _task_label(task: dict) -> str:
    label = task["document_id"]
    if task["mode"] == "gold-crop":
        label += f"/{task['gold_tables'][0]['table_id']}"
    return f"{label} {task['backend']}/{task['mode']}"


def execute_tasks(tasks: list[dict], jobs: int) -> list[dict] | str:
    """Run ``tasks`` (in order); return results, or a string when the run is aborted.

    The first task is always executed alone: ``"unavailable"`` there skips the
    whole backend, ``"error"`` there aborts (a harness that cannot run page one
    is broken, not a model miss). Later errors are recorded and the run goes on.
    """
    if not tasks:
        return []
    results: list[dict] = []
    total = len(tasks)

    def progress(i: int, task: dict, res: dict) -> None:
        status = "" if res["status"] == "ok" else f" {res['status'].upper()}"
        print(
            f"[{i}/{total}] {_task_label(task)} {res['elapsed_s']:.1f}s{status}",
            file=sys.stderr,
            flush=True,
        )

    first = run_task(tasks[0]) if jobs <= 1 else None
    if first is None:
        pool = ProcessPoolExecutor(max_workers=jobs)
        first = pool.submit(run_task, tasks[0]).result()
    else:
        pool = None
    progress(1, tasks[0], first)
    if first["status"] != "ok":
        if pool is not None:
            pool.shutdown(wait=False)
        return f"{first['status']}: {first['error']}"
    results.append(first)
    rest = tasks[1:]
    if pool is None:
        for i, task in enumerate(rest, start=2):
            res = run_task(task)
            progress(i, task, res)
            results.append(res)
    else:
        with pool:
            for i, (task, res) in enumerate(
                zip(rest, pool.map(run_task, rest)), start=2
            ):
                progress(i, task, res)
                results.append(res)
    return results


# --------------------------------------------------------------------------- #
# Tags, summary, deltas
# --------------------------------------------------------------------------- #
def _record_tags(
    page: GoldPage, table: GoldTable, lines_detected: bool | None
) -> list[str]:
    found = set(table.tags) | set(table_tags(table))
    if len(page.tables) >= 2:
        found.add("multi-table")
    if lines_detected is False:
        found.add("borderless")
    if not found & STRUCTURAL_TAGS:
        found.add("plain")
    return [t for t in TAG_VOCABULARY if t in found]


def _metric_block(records: list[dict]) -> dict:
    out: dict = {}
    for key in ("grits_top", "grits_con", "teds_struct", "cell_f1"):
        vals = [r[key] for r in records if r.get(key) is not None]
        out[f"{key}_mean"] = _mean(vals)
        out[f"{key}_median"] = _median(vals)
    return out


def summarize(
    tasks: list[dict], results: list[dict], records: list[dict], n_pages: int
) -> dict:
    scored = [r for r in records if "skipped" not in r and "error" not in r]
    matched = [r for r in scored if r["matched"]]
    n_pred = sum(res["n_pred"] for res in results)
    pages_with_detection = len(
        {t["document_id"] for t, res in zip(tasks, results) if res["n_pred"] > 0}
    )
    mode = tasks[0]["mode"] if tasks else None
    n_gold = len([r for r in records if "skipped" not in r])
    n_matched = len(matched)
    if mode == "e2e":
        prec = n_matched / n_pred if n_pred else 0.0
        rec = n_matched / n_gold if n_gold else 0.0
        f1 = 2 * prec * rec / (prec + rec) if (prec + rec) else 0.0
    else:
        prec = rec = f1 = None
    by_tag: dict[str, dict] = {}
    for tag in TAG_VOCABULARY:
        sub = [r for r in scored if tag in (r.get("tags") or [])]
        if sub:
            blk = _metric_block(sub)
            by_tag[tag] = {
                "n": len(sub),
                "grits_top_mean": blk["grits_top_mean"],
                "grits_con_mean": blk["grits_con_mean"],
                "teds_struct_mean": blk["teds_struct_mean"],
                "cell_f1_mean": blk["cell_f1_mean"],
            }
    total_elapsed = sum(res["elapsed_s"] for res in results)
    return {
        "total_pages": n_pages,
        "n_gold_tables": n_gold,
        "n_scored_tables": len(scored),
        "n_skipped_tables": len([r for r in records if "skipped" in r]),
        "n_pred_tables": n_pred,
        "n_matched": n_matched,
        "pages_with_detection": pages_with_detection,
        "detection_precision": prec,
        "detection_recall": rec,
        "detection_f1": f1,
        "recall_weighted": _metric_block(scored),
        "matched_only": _metric_block(matched),
        "teds_skipped": len([r for r in matched if r.get("teds_struct") is None]),
        "cell_skipped": len([r for r in matched if r.get("cell_f1") is None]),
        "n_errors": len([res for res in results if res["status"] != "ok"]),
        "total_elapsed_s": total_elapsed,
        "per_table_mean_s": total_elapsed / n_gold if n_gold else None,
        "by_tag": by_tag,
    }


def _flatten(summary: dict) -> dict:
    flat: dict = {}
    for k, v in summary.items():
        if isinstance(v, dict) and k in ("recall_weighted", "matched_only"):
            for kk, vv in v.items():
                flat[f"{k}.{kk}"] = vv
        elif not isinstance(v, dict):
            flat[k] = v
    return flat


def compute_deltas(baseline: dict, current: list[dict]) -> dict:
    """Align runs by (backend, mode) and diff the headline metrics."""
    base_runs = {
        (r.get("backend"), r.get("mode")): r for r in baseline.get("runs") or []
    }
    cur_runs = {(r["backend"], r["mode"]): r for r in current}
    out: dict = {"added": [], "removed": [], "runs": []}
    for key in sorted(
        set(base_runs) | set(cur_runs), key=lambda k: (str(k[0]), str(k[1]))
    ):
        name = f"{key[0]}/{key[1]}"
        if key not in base_runs:
            out["added"].append(name)
            continue
        if key not in cur_runs:
            out["removed"].append(name)
            continue
        b = _flatten(base_runs[key].get("summary") or {})
        c = _flatten(cur_runs[key].get("summary") or {})
        rows = []
        for m in DELTA_METRICS:
            bv, cv = b.get(m), c.get(m)
            delta = (
                (cv - bv)
                if isinstance(bv, (int, float)) and isinstance(cv, (int, float))
                else None
            )
            rows.append({"metric": m, "baseline": bv, "now": cv, "delta": delta})
        out["runs"].append({"backend": key[0], "mode": key[1], "metrics": rows})
    return out


def _delta_str(v: float | None) -> str:
    if v is None:
        return "n/a"
    if isinstance(v, int) and not isinstance(v, bool):
        return f"{v:+d}"
    return f"{v:+.3f}"


def _delta_lines(deltas: dict) -> list[str]:
    lines: list[str] = []
    for run in deltas["runs"]:
        lines.append(f"### {run['backend']} / {run['mode']}")
        lines.append("")
        lines.append("| metric | baseline | now | delta |")
        lines.append("|--------|---------:|----:|------:|")
        for row in run["metrics"]:
            lines.append(
                f"| {row['metric']} | {_r3(row['baseline'])} | {_r3(row['now'])} "
                f"| {_delta_str(row['delta'])} |"
            )
        lines.append("")
    if deltas["added"]:
        lines.append(f"- New in this run (no baseline): {', '.join(deltas['added'])}")
    if deltas["removed"]:
        lines.append(f"- In baseline but not run now: {', '.join(deltas['removed'])}")
    return lines


# --------------------------------------------------------------------------- #
# Output: stdout table, markdown report
# --------------------------------------------------------------------------- #
_AGG_COLS = (
    ("backend", "backend"),
    ("mode", "mode"),
    ("gold", "n_gold_tables"),
    ("pred", "n_pred_tables"),
    ("matched", "n_matched"),
    ("det-F1", "detection_f1"),
    ("GriTS_Top", "recall_weighted.grits_top_mean"),
    ("GriTS_Con", "recall_weighted.grits_con_mean"),
    ("TEDS-S", "recall_weighted.teds_struct_mean"),
    ("cell-F1", "recall_weighted.cell_f1_mean"),
    ("m/GriTS_Con", "matched_only.grits_con_mean"),
    ("m/TEDS-S", "matched_only.teds_struct_mean"),
    ("teds-skip", "teds_skipped"),
    ("errors", "n_errors"),
    ("s/table", "per_table_mean_s"),
)


def _aggregate_rows(runs: list[dict]) -> list[list[str]]:
    rows: list[list[str]] = []
    for run in runs:
        if run["status"] != "ok":
            rows.append(
                [run["backend"], run["mode"], run["status"]]
                + ["-"] * (len(_AGG_COLS) - 3)
            )
            continue
        flat = _flatten(run["summary"])
        flat["backend"], flat["mode"] = run["backend"], run["mode"]
        rows.append([_r3(flat.get(key)) for _, key in _AGG_COLS])
    return rows


def _print_table(headers: list[str], rows: list[list[str]]) -> None:
    widths = [
        max(len(h), *(len(r[i]) for r in rows)) if rows else len(h)
        for i, h in enumerate(headers)
    ]
    print("  ".join(h.ljust(w) for h, w in zip(headers, widths)))
    for r in rows:
        print("  ".join(c.ljust(w) for c, w in zip(r, widths)))


def _md_table(
    headers: list[str], rows: list[list[str]], align_from: int = 2
) -> list[str]:
    sep = ["---" if i < align_from else "---:" for i in range(len(headers))]
    return [
        "| " + " | ".join(headers) + " |",
        "|" + "|".join(sep) + "|",
        *("| " + " | ".join(r) + " |" for r in rows),
    ]


def build_report(result: dict, pages: list[GoldPage], deltas: dict | None) -> str:
    runs = result["runs"]
    ok_runs = [r for r in runs if r["status"] == "ok"]
    harness = f"`conformance/gt/eval_tables.py run` (backends: {', '.join(r['backend'] for r in runs)}; modes: {', '.join(sorted({r['mode'] for r in runs}))})"
    lic_anno = {p.anno_license for p in pages if p.anno_license} or {"see corpus"}
    lic_pdf = {p.pdf_license for p in pages if p.pdf_license} or {"see corpus"}
    lines = [
        "# Table structure evaluation — pdfspine `find_tables` vs gold (GriTS / TEDS-S / cell-F1)",
        "",
        f"Harness: {harness}  ",
        "Metric: **GriTS_Top / GriTS_Con** (`grits.py`), **TEDS-Struct** and **cell-alignment F1** (`table_metrics.py`).  ",
        f"Dataset: `{result['corpus']}` — annotations `{'/'.join(sorted(lic_anno))}`, source PDFs `{'/'.join(sorted(lic_pdf))}`.  ",
        f"Provenance: generated {result['generated']} at commit `{result['code_version'].get('commit')}` "
        f"(branch `{result['code_version'].get('branch')}`, pdfspine `{result['code_version'].get('pdfspine')}`).",
        "",
        "## Sample / provenance / license",
        "",
        f"- Pages scored: **{result['n_pages']}**; gold tables: **{sum(len(p.tables) for p in pages)}**.",
        f"- Annotations license: **{'/'.join(sorted(lic_anno))}**; source-PDF license: **{'/'.join(sorted(lic_pdf))}**.",
        "- Missed gold tables score 0 on every metric (recall-weighted); `matched_only` columns exclude them. "
        "TEDS-S `None` (tree too large / timeout) is counted in `teds_skipped`, never as 0.",
        "",
        "## Aggregate",
        "",
    ]
    lines += _md_table([h for h, _ in _AGG_COLS], _aggregate_rows(runs))
    for run in runs:
        if run["status"] != "ok":
            lines.append(
                f"- `{run['backend']}/{run['mode']}`: {run['status']} — {run.get('reason')}"
            )
    lines.append("")
    for run in ok_runs:
        s = run["summary"]
        if s.get("by_tag"):
            lines.append(f"### By tag — {run['backend']} / {run['mode']}")
            lines.append("")
            lines += _md_table(
                ["tag", "n", "GriTS_Top", "GriTS_Con", "TEDS-S", "cell-F1"],
                [
                    [
                        tag,
                        str(v["n"]),
                        _r3(v["grits_top_mean"]),
                        _r3(v["grits_con_mean"]),
                        _r3(v["teds_struct_mean"]),
                        _r3(v["cell_f1_mean"]),
                    ]
                    for tag, v in s["by_tag"].items()
                ],
                align_from=1,
            )
            lines.append("")
    lines += ["## Per-document", ""]
    for run in ok_runs:
        lines.append(f"### {run['backend']} / {run['mode']}")
        lines.append("")
        rows = []
        for page in pages:
            recs = [r for r in run["tables"] if r["document_id"] == page.document_id]
            scored = [r for r in recs if "skipped" not in r]
            if not scored:
                rows.append(
                    [
                        page.document_id,
                        str(len(page.tables)),
                        "-",
                        "-",
                        "-",
                        "-",
                        "-",
                        "-",
                        "skipped",
                    ]
                )
                continue
            tags = sorted(
                {t for r in scored for t in r.get("tags") or []},
                key=TAG_VOCABULARY.index,
            )
            rows.append(
                [
                    page.document_id,
                    str(len(page.tables)),
                    str(len([r for r in scored if r["matched"]])),
                    _r3(_mean([r["grits_top"] for r in scored])),
                    _r3(_mean([r["grits_con"] for r in scored])),
                    _r3(
                        _mean(
                            [
                                r["teds_struct"]
                                for r in scored
                                if r["teds_struct"] is not None
                            ]
                        )
                    ),
                    _r3(
                        _mean(
                            [r["cell_f1"] for r in scored if r["cell_f1"] is not None]
                        )
                    ),
                    _r3(_mean([r["elapsed_s"] for r in scored])),
                    ", ".join(tags),
                ]
            )
        lines += _md_table(
            [
                "doc",
                "gold",
                "matched",
                "GriTS_Top",
                "GriTS_Con",
                "TEDS-S",
                "cell-F1",
                "s",
                "tags",
            ],
            rows,
            align_from=1,
        )
        lines.append("")
    if deltas is not None:
        lines += ["## Delta vs baseline", ""]
        lines += _delta_lines(deltas)
        lines.append("")
    lines += ["## Interpretation", ""]
    lines += _interpretation(result)
    return "\n".join(lines) + "\n"


def _interpretation(result: dict) -> list[str]:
    ok_runs = [r for r in result["runs"] if r["status"] == "ok"]
    out: list[str] = []
    if not ok_runs:
        return ["- No backend produced a scored run."]
    best = max(
        ok_runs, key=lambda r: r["summary"]["recall_weighted"]["grits_con_mean"] or 0.0
    )
    bs = best["summary"]
    out.append(
        f"- Best recall-weighted GriTS_Con: **{best['backend']}/{best['mode']}** at "
        f"{_r3(bs['recall_weighted']['grits_con_mean'])} (GriTS_Top {_r3(bs['recall_weighted']['grits_top_mean'])}, "
        f"TEDS-S {_r3(bs['recall_weighted']['teds_struct_mean'])}, cell-F1 {_r3(bs['recall_weighted']['cell_f1_mean'])})."
    )
    for run in ok_runs:
        s = run["summary"]
        miss = s["n_gold_tables"] - s["n_matched"]
        parts = [f"{miss}/{s['n_gold_tables']} gold tables missed"]
        if run["mode"] == "e2e":
            parts.append(
                f"detection P/R/F1 {_r3(s['detection_precision'])}/{_r3(s['detection_recall'])}/{_r3(s['detection_f1'])}"
            )
        if s["teds_skipped"]:
            parts.append(f"TEDS-S skipped on {s['teds_skipped']} tables")
        if s["cell_skipped"]:
            parts.append(
                f"cell-F1 unavailable on {s['cell_skipped']} tables (no cell bbox)"
            )
        if s["n_errors"]:
            parts.append(f"{s['n_errors']} task errors")
        if run.get("clip_honored") is False:
            parts.append(
                "`clip` is not honoured by this backend, so gold-crop is best-overlap on the full page"
            )
        out.append(f"- `{run['backend']}/{run['mode']}`: " + "; ".join(parts) + ".")
    for run in result["runs"]:
        if run["status"] != "ok":
            out.append(
                f"- `{run['backend']}/{run['mode']}` {run['status']}: {run.get('reason')}."
            )
    out.append(
        "- GriTS and TEDS-S are position-invariant; cell-F1 is the geometric check. "
        "A high GriTS with a low cell-F1 means the grid is right but cells are shifted."
    )
    return out


# --------------------------------------------------------------------------- #
# Subcommand: run
# --------------------------------------------------------------------------- #
def cmd_run(args) -> int:
    corpus = Path(args.corpus)
    if not corpus.exists():
        print(f"error: corpus not found: {corpus}", file=sys.stderr)
        return 2
    skipped: list[str] = []
    pages = _select_pages(load_corpus(corpus, skipped=skipped), args)
    no_pdf = [p.document_id for p in pages if p.pdf is None]
    pages = [p for p in pages if p.pdf is not None and p.tables]
    if no_pdf:
        print(
            f"warning: {len(no_pdf)} page(s) without PDF skipped: {', '.join(no_pdf[:5])}",
            file=sys.stderr,
        )
    if skipped:
        print(
            f"warning: {len(skipped)} gold entries unreadable (see log)",
            file=sys.stderr,
        )
    if not pages:
        print("error: no scorable pages (no gold tables with PDFs)", file=sys.stderr)
        return 2

    backends = list(dict.fromkeys(args.backend or ["lines"]))
    if "lines" in backends:  # run first so `borderless` tags exist for later runs
        backends.remove("lines")
        backends.insert(0, "lines")
    opts = {
        "iou": args.iou,
        "teds_max_nodes": args.teds_max_nodes,
        "teds_timeout": args.teds_timeout,
        "dpi": args.dpi,
        "timeout": args.timeout,
        "fitz_py": _resolve_fitz_py(args.fitz_py),
    }
    plan = [(b, args.mode) for b in backends]
    if args.oracle_fitz:
        if args.mode == "gold-crop":
            print(
                "note: --oracle-fitz supports e2e only (tables_diff fitz worker takes no clip) — fitz skipped",
                file=sys.stderr,
            )
        else:
            plan.append((FITZ_BACKEND, "e2e"))

    page_by_id = {p.document_id: p for p in pages}
    lines_detected: dict[str, bool] = {}
    runs: list[dict] = []
    for backend, mode in plan:
        tasks = build_tasks(pages, backend, mode, opts)
        print(f"== {backend}/{mode}: {len(tasks)} task(s)", file=sys.stderr)
        results = execute_tasks(tasks, args.jobs)
        if isinstance(results, str):
            kind, _, reason = results.partition(": ")
            if kind == "unavailable":
                print(
                    f"backend '{backend}' unavailable: {reason} — skipped",
                    file=sys.stderr,
                )
                runs.append(
                    {
                        "backend": backend,
                        "mode": mode,
                        "status": "unavailable",
                        "reason": reason,
                    }
                )
                continue
            print(
                f"error: first task failed for {backend}/{mode}: {reason}",
                file=sys.stderr,
            )
            return 3
        if backend == "lines":
            for task, res in zip(tasks, results):
                did = task["document_id"]
                lines_detected[did] = (
                    lines_detected.get(did, False) or res["n_pred"] > 0
                )
        records: list[dict] = []
        for task, res in zip(tasks, results):
            page = page_by_id[task["document_id"]]
            by_id = {t.table_id: t for t in page.tables}
            if res["status"] != "ok":
                for g in task["gold_tables"]:
                    records.append(
                        {
                            "document_id": task["document_id"],
                            "table_id": g["table_id"],
                            "backend": backend,
                            "mode": mode,
                            "status": "error",
                            "error": res["error"],
                        }
                    )
                continue
            for rec in res["records"]:
                if "skipped" not in rec:
                    rec["tags"] = _record_tags(
                        page,
                        by_id[rec["table_id"]],
                        lines_detected.get(page.document_id),
                    )
                records.append(rec)
        run = {
            "backend": backend,
            "mode": mode,
            "status": "ok",
            "clip_honored": (backend in ("onnx", "tatr"))
            if mode == "gold-crop"
            else None,
            "summary": summarize(tasks, results, records, len(pages)),
            "tables": records,
        }
        runs.append(run)

    if not any(r["status"] == "ok" for r in runs):
        print("error: no backend could run", file=sys.stderr)
        return 3

    result = {
        "schema": SCHEMA,
        "generated": _now(),
        "corpus": _display_path(corpus),
        "n_pages": len(pages),
        "code_version": _code_version(),
        "options": {k: v for k, v in opts.items() if k != "fitz_py"},
        "runs": runs,
    }
    deltas = None
    if args.baseline:
        try:
            baseline = json.loads(Path(args.baseline).read_text(encoding="utf-8"))
        except (OSError, ValueError) as exc:
            print(
                f"warning: cannot read baseline {args.baseline}: {exc}", file=sys.stderr
            )
        else:
            deltas = compute_deltas(baseline, runs)
            result["baseline"] = {
                "path": str(args.baseline),
                "generated": baseline.get("generated"),
                "deltas": deltas,
            }

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
    if args.report:
        rp = Path(args.report)
        rp.parent.mkdir(parents=True, exist_ok=True)
        rp.write_text(build_report(result, pages, deltas), encoding="utf-8")

    _print_table([h for h, _ in _AGG_COLS], _aggregate_rows(runs))
    if deltas is not None:
        print()
        print("Delta vs baseline:")
        for run in deltas["runs"]:
            print(f"  {run['backend']}/{run['mode']}")
            _print_table(
                ["metric", "baseline", "now", "delta"],
                [
                    [
                        r["metric"],
                        _r3(r["baseline"]),
                        _r3(r["now"]),
                        _delta_str(r["delta"]),
                    ]
                    for r in run["metrics"]
                ],
            )
        if deltas["added"]:
            print(f"  new (no baseline): {', '.join(deltas['added'])}")
        if deltas["removed"]:
            print(f"  in baseline only: {', '.join(deltas['removed'])}")
    print(f"\nresults: {out}" + (f"\nreport: {args.report}" if args.report else ""))
    return 0


# --------------------------------------------------------------------------- #
# Subcommand: draft-gold
# --------------------------------------------------------------------------- #
def _human_cell(cell: dict) -> dict:
    rows, cols = cell["row_nums"], cell["column_nums"]
    out = {
        "row": min(rows),
        "col": min(cols),
        "rowspan": max(rows) - min(rows) + 1,
        "colspan": max(cols) - min(cols) + 1,
        "text": cell.get("cell_text", ""),
        "bbox": cell.get("bbox"),
    }
    if cell.get("header"):
        out["header"] = True
    return out


def cmd_draft_gold(args) -> int:
    pdf = Path(args.pdf)
    if not pdf.exists():
        print(f"error: PDF not found: {pdf}", file=sys.stderr)
        return 2
    stem = pdf.stem
    out = (
        Path(args.out)
        if args.out
        else FINANCE_DIR / "annotations" / f"{stem}_p{args.page}.gold.json"
    )
    opts = {"dpi": args.dpi, "timeout": args.timeout, "fitz_py": ""}
    pred = predict(str(pdf), args.page, args.backend, "e2e", None, opts)
    if not pred["ok"]:
        print(
            f"error: {args.backend} failed on {pdf} page {args.page}: {pred['error']}",
            file=sys.stderr,
        )
        return 3
    tables = []
    html_parts = []
    for i, rec in enumerate(pred["tables"]):
        cells = [c for c in (normalize_cell(c) for c in _pred_cells(rec)) if c]
        tables.append(
            {
                "table_id": f"t{i}",
                "bbox": rec.get("bbox"),
                "tags": [],
                "cells": [_human_cell(c) for c in cells],
            }
        )
        html_parts.append(
            f"<!-- t{i} bbox={rec.get('bbox')} -->\n" + cells_to_html(cells)
        )
    try:
        pdf_ref = os.path.relpath(os.path.abspath(pdf), os.path.abspath(out.parent))
    except ValueError:  # different drive on Windows
        pdf_ref = str(pdf.resolve())
    draft = {
        "_draft": True,
        "_generated_by": {
            "tool": "conformance/gt/eval_tables.py draft-gold",
            "backend": args.backend,
            "generated": _now(),
            "code_version": _code_version(),
        },
        "document_id": f"{stem}_p{args.page}",
        "pdf": pdf_ref.replace(os.sep, "/"),
        "page_index": args.page,
        "tables": tables,
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(draft, indent=2, ensure_ascii=False), encoding="utf-8")
    html_out = (
        out.with_name(out.name[: -len(".gold.json")] + ".gold.html")
        if out.name.endswith(".gold.json")
        else out.with_suffix(".gold.html")
    )
    html_out.write_text(
        f"<!-- DRAFT generated by eval_tables.py draft-gold ({args.backend}); verify against {pdf.name} page {args.page} -->\n"
        + "\n".join(html_parts)
        + "\n",
        encoding="utf-8",
    )
    print(
        f"draft gold: {out}  ({len(tables)} table(s), {sum(len(t['cells']) for t in tables)} cells)"
    )
    print(f"preview:    {html_out}")
    print(
        'This is a machine draft — check every cell against the PDF, fix errors, then set "_draft" to false.'
    )
    return 0


# --------------------------------------------------------------------------- #
# Subcommand: seed-subset
# --------------------------------------------------------------------------- #
def select_subset(
    infos: list[dict], size: int, min_per_tag: int, seed: int = 0
) -> list[dict]:
    """Deterministic stratified pick: ``min_per_tag`` per tag, rest by rarity.

    ``infos`` are ``{"document_id", "tags", ...}`` dicts. Pages are ordered by
    ``document_id`` and shuffled with a fixed seed so the choice within a tag
    is not alphabetical but still reproducible.
    """
    infos = sorted(infos, key=lambda p: p["document_id"])
    rng = random.Random(seed)
    order = list(infos)
    rng.shuffle(order)
    tag_total = {t: sum(1 for p in infos if t in p["tags"]) for t in TAG_VOCABULARY}
    chosen: dict[str, dict] = {}
    for tag in sorted(TAG_VOCABULARY, key=lambda t: tag_total[t]):  # rarest first
        have = sum(1 for p in chosen.values() if tag in p["tags"])
        for p in order:
            if have >= min_per_tag or len(chosen) >= size:
                break
            if tag in p["tags"] and p["document_id"] not in chosen:
                chosen[p["document_id"]] = p
                have += 1

    def rarity(p: dict) -> float:
        return sum(1.0 / tag_total[t] for t in p["tags"] if tag_total.get(t))

    rest = [p for p in infos if p["document_id"] not in chosen]
    rest.sort(key=lambda p: (-rarity(p), p["document_id"]))
    for p in rest:
        if len(chosen) >= size:
            break
        chosen[p["document_id"]] = p
    return sorted(chosen.values(), key=lambda p: p["document_id"])


def cmd_seed_subset(args) -> int:
    corpus = Path(args.corpus)
    if not corpus.exists():
        print(f"error: corpus not found: {corpus}", file=sys.stderr)
        return 2
    pages = [p for p in load_corpus(corpus) if p.pdf is not None and p.tables]
    if not pages:
        print("error: corpus has no scorable pages", file=sys.stderr)
        return 2
    opts = {"dpi": None, "timeout": args.timeout, "fitz_py": ""}
    infos: list[dict] = []
    for i, page in enumerate(pages, start=1):
        pred = predict(str(page.pdf), page.page_index, "lines", "e2e", None, opts)
        if i == 1 and not pred["ok"]:
            print(
                f"error: lines strategy failed on first page: {pred['error']}",
                file=sys.stderr,
            )
            return 3
        detected = bool(pred["ok"] and pred["tables"])
        print(
            f"[{i}/{len(pages)}] {page.document_id} lines {pred['elapsed_s']:.2f}s",
            file=sys.stderr,
        )
        infos.append(
            {
                "document_id": page.document_id,
                "n_tables": len(page.tables),
                "tags": page_tags(page, lines_detected=detected),
                "lines_detected": detected,
            }
        )
    chosen = select_subset(infos, args.size, args.min_per_tag, args.seed)
    tag_counts = {t: sum(1 for p in chosen if t in p["tags"]) for t in TAG_VOCABULARY}
    corpus_counts = {t: sum(1 for p in infos if t in p["tags"]) for t in TAG_VOCABULARY}
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "schema": SEED_SCHEMA,
                "generated": _now(),
                "corpus": _display_path(corpus),
                "size": len(chosen),
                "selection_rule": (
                    f"stratified: >= {args.min_per_tag} pages per tag in TAG_VOCABULARY (rarest tag first, "
                    f"seed={args.seed} shuffle), remaining slots by tag rarity (sum of 1/corpus_count over "
                    "the page's tags), ties by document_id; `borderless` = strategy='lines' found no table"
                ),
                "tag_counts": tag_counts,
                "corpus_tag_counts": corpus_counts,
                "pages": chosen,
            },
            indent=2,
        ),
        encoding="utf-8",
    )
    _print_table(
        ["tag", "corpus", "subset"],
        [[t, str(corpus_counts[t]), str(tag_counts[t])] for t in TAG_VOCABULARY],
    )
    print(f"\nselected {len(chosen)}/{len(infos)} pages -> {out}")
    return 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def _build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(
        prog="eval_tables.py",
        description="Score pdfspine find_tables against the financial-report gold set.",
    )
    sub = ap.add_subparsers(dest="cmd", required=True)

    run = sub.add_parser("run", help="run backends over the corpus and score")
    run.add_argument("--corpus", default=str(DEFAULT_CORPUS))
    run.add_argument(
        "--backend", action="append", choices=BACKENDS, help="repeatable; default lines"
    )
    run.add_argument("--mode", choices=MODES, default="e2e")
    run.add_argument("--limit", type=int, default=None, help="only the first N pages")
    run.add_argument(
        "--pages",
        action="append",
        help="document_id list (comma-separated, repeatable)",
    )
    run.add_argument("--jobs", type=int, default=1)
    run.add_argument("--out", default=str(DEFAULT_OUT))
    run.add_argument("--report", default=None, help="markdown report path")
    run.add_argument(
        "--baseline", default=None, help="previous results JSON to diff against"
    )
    run.add_argument(
        "--oracle-fitz",
        action="store_true",
        help="also score PyMuPDF via .venv-oracle (e2e only)",
    )
    run.add_argument(
        "--fitz-py",
        default=None,
        help="interpreter with PyMuPDF (default: .venv-oracle)",
    )
    run.add_argument("--iou", type=float, default=0.5)
    run.add_argument("--teds-max-nodes", type=int, default=2000)
    run.add_argument("--teds-timeout", type=float, default=20.0)
    run.add_argument(
        "--dpi", type=int, default=None, help="vision_options dpi for onnx/tatr"
    )
    run.add_argument(
        "--timeout", type=float, default=120.0, help="fitz subprocess timeout (s)"
    )
    run.set_defaults(func=cmd_run)

    draft = sub.add_parser(
        "draft-gold", help="export a machine draft *.gold.json for manual correction"
    )
    draft.add_argument("--pdf", required=True)
    draft.add_argument("--page", type=int, required=True, help="0-based page index")
    draft.add_argument("--backend", choices=BACKENDS, default="onnx")
    draft.add_argument("--out", default=None)
    draft.add_argument("--dpi", type=int, default=None)
    draft.add_argument("--timeout", type=float, default=120.0)
    draft.set_defaults(func=cmd_draft_gold)

    seed = sub.add_parser(
        "seed-subset", help="pick a stratified recommended subset of the corpus"
    )
    seed.add_argument("--corpus", default=str(DEFAULT_CORPUS))
    seed.add_argument("--out", default=str(FINANCE_DIR / "seed-subset.json"))
    seed.add_argument("--size", type=int, default=40)
    seed.add_argument("--min-per-tag", type=int, default=4)
    seed.add_argument("--seed", type=int, default=0)
    seed.add_argument("--timeout", type=float, default=120.0)
    seed.set_defaults(func=cmd_seed_subset)
    return ap


def main(argv: list[str] | None = None) -> int:
    logging.basicConfig(level=logging.WARNING, format="%(levelname)s: %(message)s")
    args = _build_parser().parse_args(argv)
    if getattr(args, "jobs", 1) is not None and getattr(args, "jobs", 1) < 1:
        print("error: --jobs must be >= 1", file=sys.stderr)
        return 2
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
