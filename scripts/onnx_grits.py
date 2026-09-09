#!/usr/bin/env python3
"""Score the ONNX vision table backend against FinTabNet.c gold with GriTS.

In-process counterpart of ``conformance/gt/tables_diff.py --gold`` for
``find_tables(strategy="vision", backend="onnx")``: every page is processed
sequentially inside ONE interpreter, so the PP-DocLayout + SLANet-plus models
load exactly once. Gold parsing, bbox matching and the GriTS aggregation mirror
``tables_diff.py`` (``exclude_for_structure`` tables are skipped; unmatched
gold tables score 0 in the ``end_to_end`` view; ``matched_only`` restricts to
tables whose ``Table.bbox`` matched a gold bbox by IoU).

Requires the ``[onnx]`` extra and ``PDFSPINE_ONNX_MODELS``. The manifest is the
one written by ``conformance/gt/fetch_fintabnet.py``; ``pdfs/`` and
``annotations/`` are resolved relative to the manifest's directory when the
absolute paths recorded inside it do not exist on this machine.

    .venv/bin/python scripts/onnx_grits.py --out /tmp/grits.json
    .venv/bin/python scripts/onnx_grits.py --pages ADBE_2011_page_118 --out /tmp/g3.json
    .venv/bin/python scripts/onnx_grits.py --limit 20 \\
        --vision-options '{"layout_variant": "pp_doclayoutv3"}' --out /tmp/g20.json
"""

from __future__ import annotations

import argparse
from html.parser import HTMLParser
import json
from pathlib import Path
import sys
import time
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
GT_DIR = ROOT / "conformance" / "gt"
DEFAULT_MANIFEST = GT_DIR / "corpus-fintabnet" / "manifest.json"


# --------------------------------------------------------------------------- #
# Gold side (mirrors tables_diff.py)
# --------------------------------------------------------------------------- #
def _resolve_corpus_path(raw: str | None, mdir: Path, derived: Path) -> Path | None:
    """Prefer the manifest's recorded path when it exists, else the derived one."""
    if raw:
        p = Path(raw)
        if not p.is_absolute():
            p = mdir / raw
        if p.exists():
            return p
    if derived.exists():
        return derived
    return Path(raw) if raw else None


def load_gold_manifest(path: Path) -> list[dict[str, Any]]:
    """Load a FinTabNet.c manifest into per-page entries with gold tables.

    ``pdf``/``annotation`` fall back to ``<manifest dir>/pdfs/<id>.pdf`` and
    ``<manifest dir>/annotations/<id>_tables.json`` so a corpus copied to
    another machine keeps working. Tables flagged ``exclude_for_structure`` are
    dropped, as in ``tables_diff.py``.
    """
    data = json.loads(path.read_text(encoding="utf-8"))
    entries = data.get("entries") if isinstance(data, dict) else data
    mdir = path.resolve().parent
    out: list[dict[str, Any]] = []
    for e in entries or []:
        anno_raw = e.get("annotation")
        doc_id = e.get("document_id") or (Path(anno_raw).stem if anno_raw else None)
        if not doc_id:
            continue
        anno_p = _resolve_corpus_path(
            anno_raw, mdir, mdir / "annotations" / f"{doc_id}_tables.json"
        )
        if anno_p is None or not anno_p.exists():
            continue
        try:
            tables = json.loads(anno_p.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        gold_tables = [t for t in tables if not t.get("exclude_for_structure")]
        pdf_p = _resolve_corpus_path(
            e.get("pdf"), mdir, mdir / "pdfs" / f"{doc_id}.pdf"
        )
        out.append(
            {
                "document_id": doc_id,
                "pdf": pdf_p,
                "annotation": anno_p,
                "pdf_status": e.get("pdf_status"),
                "page_index": int(e.get("pdf_page_index", 0)),
                "gold_tables": gold_tables,
            }
        )
    return out


def _gold_cells_from_annotation(table_anno: dict) -> tuple[list[dict], list[float]]:
    """FinTabNet.c annotation table -> (GriTS cells, table bbox)."""
    cells: list[dict] = []
    for c in table_anno.get("cells", []):
        rn = list(c.get("row_nums") or [])
        cn = list(c.get("column_nums") or [])
        if not rn or not cn:
            continue
        text = c.get("json_text_content") or c.get("pdf_text_content") or ""
        cells.append({"row_nums": rn, "column_nums": cn, "cell_text": text.strip()})
    bbox = [float(v) for v in (table_anno.get("pdf_table_bbox") or [0, 0, 0, 0])[:4]]
    return cells, bbox


# --------------------------------------------------------------------------- #
# Predicted side
# --------------------------------------------------------------------------- #
def _as_bbox(bbox_obj: Any) -> list[float]:
    """Normalize a ``Rect``/4-sequence to ``[x0, y0, x1, y1]`` floats."""
    try:
        vals = list(bbox_obj)
    except TypeError:
        vals = [getattr(bbox_obj, a) for a in ("x0", "y0", "x1", "y1")]
    x0, y0, x1, y1 = (float(v) for v in vals[:4])
    return [min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1)]


class _TableHTMLParser(HTMLParser):
    """``Table.to_html()`` -> GriTS cells (colspan/rowspan via an occupancy grid)."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.cells: list[dict] = []
        self._row = -1
        self._occupied: set[tuple[int, int]] = set()
        self._col_cursor = 0
        self._cur: dict | None = None
        self._text: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        a = dict(attrs)
        if tag == "tr":
            self._row += 1
            self._col_cursor = 0
        elif tag in ("td", "th"):
            try:
                colspan = max(1, int(a.get("colspan") or "1"))
            except ValueError:
                colspan = 1
            try:
                rowspan = max(1, int(a.get("rowspan") or "1"))
            except ValueError:
                rowspan = 1
            col = self._col_cursor
            while (self._row, col) in self._occupied:
                col += 1
            row_nums = list(range(self._row, self._row + rowspan))
            column_nums = list(range(col, col + colspan))
            for r in row_nums:
                for cc in column_nums:
                    self._occupied.add((r, cc))
            self._col_cursor = col + colspan
            self._cur = {"row_nums": row_nums, "column_nums": column_nums}
            self._text = []
        elif tag == "br":
            self._text.append(" ")

    def handle_data(self, data: str) -> None:
        if self._cur is not None:
            self._text.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag in ("td", "th") and self._cur is not None:
            self._cur["cell_text"] = " ".join("".join(self._text).split())
            self.cells.append(self._cur)
            self._cur = None
            self._text = []


def _pred_cells_from_html(html: str | None) -> list[dict]:
    if not html:
        return []
    p = _TableHTMLParser()
    try:
        p.feed(html)
    except Exception:  # noqa: BLE001
        return []
    return [c for c in p.cells if "cell_text" in c]


def _pred_cells_from_table(tbl: Any) -> list[dict]:
    """Direct ``Table.spans`` + ``extract()`` cells; ``to_html()`` as fallback."""
    cells: list[dict] = []
    try:
        ext = tbl.extract()
        for row, column, row_span, column_span, _bbox in tbl.spans:
            text = ""
            if row < len(ext) and column < len(ext[row] or []):
                text = str(ext[row][column] or "")
            cells.append(
                {
                    "row_nums": list(range(int(row), int(row) + int(row_span))),
                    "column_nums": list(
                        range(int(column), int(column) + int(column_span))
                    ),
                    "cell_text": " ".join(text.split()),
                }
            )
    except Exception:  # noqa: BLE001
        cells = []
    if cells:
        return cells
    try:
        return _pred_cells_from_html(tbl.to_html())
    except Exception:  # noqa: BLE001
        return []


def _pred_record(tbl: Any) -> dict[str, Any]:
    bbox = _as_bbox(tbl.bbox)
    md = tbl.metadata if isinstance(getattr(tbl, "metadata", None), dict) else {}
    try:
        detection_bbox = _as_bbox(md.get("detection_bbox") or bbox)
    except (AttributeError, TypeError, ValueError):
        detection_bbox = bbox
    return {
        "cells": _pred_cells_from_table(tbl),
        "bbox": bbox,
        "detection_bbox": detection_bbox,
        "metadata": dict(md),
    }


def run_onnx_tables(
    pdf: Path, page_index: int, vision_options: dict[str, Any] | None
) -> list[dict[str, Any]]:
    import pdfspine

    doc = pdfspine.open(str(pdf))
    try:
        page = doc.load_page(page_index)
        finder = page.find_tables(
            strategy="vision", backend="onnx", vision_options=vision_options
        )
        return [_pred_record(t) for t in getattr(finder, "tables", finder)]
    finally:
        try:
            doc.close()
        except Exception:  # noqa: BLE001
            pass


# --------------------------------------------------------------------------- #
# Matching + scoring (mirrors tables_diff.py process_doc_gold)
# --------------------------------------------------------------------------- #
def iou(a: list[float], b: list[float]) -> float:
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


def match_tables(
    golds: list[dict], preds: list[dict], iou_thr: float = 0.5
) -> list[tuple[int, int, float]]:
    """Greedy highest-IoU-first one-to-one matching; returns ``(gold_i, pred_j, iou)``."""
    cands: list[tuple[float, int, int]] = []
    for i, g in enumerate(golds):
        for j, p in enumerate(preds):
            v = iou(g.get("bbox") or [], p.get("bbox") or [])
            if v >= iou_thr:
                cands.append((v, i, j))
    cands.sort(reverse=True)
    used_g: set[int] = set()
    used_p: set[int] = set()
    matches: list[tuple[int, int, float]] = []
    for v, i, j in cands:
        if i in used_g or j in used_p:
            continue
        used_g.add(i)
        used_p.add(j)
        matches.append((i, j, v))
    return matches


def _shape(cells: list[dict]) -> list[int]:
    nr = max((max(c["row_nums"]) for c in cells), default=-1) + 1
    nc = max((max(c["column_nums"]) for c in cells), default=-1) + 1
    return [nr, nc]


def _invalid_page(entry: dict, n_gold: int, error: str, elapsed: float) -> dict:
    return {
        "id": entry["document_id"],
        "pdf": str(entry["pdf"]),
        "status": "invalid",
        "error": error,
        "n_gold": n_gold,
        "n_pred": 0,
        "n_matched": 0,
        "n_detection_matched": 0,
        "detection_precision": None,
        "detection_recall": None,
        "detection_f1": None,
        "tables": [],
        "grits_top_sum": None,
        "grits_con_sum": None,
        "elapsed_s": round(elapsed, 2),
    }


def score_page(
    entry: dict, vision_options: dict[str, Any] | None, match_iou: float
) -> dict[str, Any]:
    """Run the ONNX backend on one page and GriTS-score it against the gold tables."""
    from grits import grits_con, grits_top

    t0 = time.time()
    golds: list[dict] = []
    for t in entry["gold_tables"]:
        cells, bbox = _gold_cells_from_annotation(t)
        if cells:
            golds.append({"cells": cells, "bbox": bbox})

    pdf = entry["pdf"]
    if pdf is None or not Path(pdf).exists():
        return _invalid_page(entry, len(golds), f"pdf missing: {pdf}", time.time() - t0)
    try:
        preds = run_onnx_tables(Path(pdf), entry["page_index"], vision_options)
    except Exception as exc:  # noqa: BLE001
        return _invalid_page(
            entry, len(golds), f"{type(exc).__name__}: {exc}", time.time() - t0
        )

    detection_preds = [{"bbox": p["detection_bbox"]} for p in preds]
    detection_pairs = match_tables(golds, detection_preds, iou_thr=match_iou)
    structure_pairs = match_tables(golds, preds, iou_thr=match_iou)
    detection_by_gold = {gi: (pi, v) for gi, pi, v in detection_pairs}
    structure_by_gold = {gi: (pi, v) for gi, pi, v in structure_pairs}

    table_recs: list[dict] = []
    for gi, g in enumerate(golds):
        det = detection_by_gold.get(gi)
        rec: dict[str, Any] = {
            "gold_index": gi,
            "gold_bbox": [round(v, 1) for v in g["bbox"]],
            "gold_shape": _shape(g["cells"]),
            "detection_matched": det is not None,
            "detection_iou": round(det[1], 3) if det else 0.0,
        }
        match = structure_by_gold.get(gi)
        if match is None:
            rec.update(
                {
                    "matched": False,
                    "iou": 0.0,
                    "pred_index": None,
                    "pred_shape": [0, 0],
                    "grits_top": 0.0,
                    "grits_con": 0.0,
                }
            )
        else:
            pi, v = match
            p = preds[pi]
            top, _, _ = grits_top(g["cells"], p["cells"])
            con, _, _ = grits_con(g["cells"], p["cells"])
            rec.update(
                {
                    "matched": True,
                    "iou": round(v, 3),
                    "pred_index": pi,
                    "pred_bbox": [round(x, 1) for x in p["bbox"]],
                    "pred_shape": _shape(p["cells"]),
                    "grits_top": top,
                    "grits_con": con,
                }
            )
        table_recs.append(rec)

    n_gold = len(golds)
    n_matched = sum(1 for r in table_recs if r["matched"])
    n_det = len(detection_pairs)
    precision = n_det / len(preds) if preds else 0.0
    recall = n_det / n_gold if n_gold else 0.0
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    backend_metadata = next((p["metadata"] for p in preds if p["metadata"]), {})
    return {
        "id": entry["document_id"],
        "pdf": str(pdf),
        "status": "valid",
        "error": None,
        "n_gold": n_gold,
        "n_pred": len(preds),
        "n_matched": n_matched,
        "n_detection_matched": n_det,
        "detection_precision": precision,
        "detection_recall": recall,
        "detection_f1": f1,
        "pred_shapes": [_shape(p["cells"]) for p in preds],
        "backend_metadata": {
            k: v
            for k, v in backend_metadata.items()
            if k not in {"detection_bbox", "recognition_crop_bbox"}
        },
        "tables": table_recs,
        "grits_top_sum": sum(r["grits_top"] for r in table_recs),
        "grits_con_sum": sum(r["grits_con"] for r in table_recs),
        "elapsed_s": round(time.time() - t0, 2),
    }


def _median(xs: list[float]) -> float:
    if not xs:
        return 0.0
    s = sorted(xs)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2


def summarize(pages: list[dict]) -> dict[str, Any]:
    """Detection P/R/F1 plus ``end_to_end`` and ``matched_only`` GriTS views."""
    valid = [p for p in pages if p.get("status") == "valid"]
    total_gold = sum(p["n_gold"] for p in valid)
    total_pred = sum(p["n_pred"] for p in valid)
    total_matched = sum(p["n_matched"] for p in valid)
    total_det = sum(p["n_detection_matched"] for p in valid)
    all_tables = [t for p in valid for t in p["tables"]]
    matched = [t for t in all_tables if t["matched"]]

    precision = total_det / total_pred if total_pred else 0.0
    recall = total_det / total_gold if total_gold else 0.0
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0

    end_top = [float(t["grits_top"]) for t in all_tables]
    end_con = [float(t["grits_con"]) for t in all_tables]
    m_top = [float(t["grits_top"]) for t in matched]
    m_con = [float(t["grits_con"]) for t in matched]
    return {
        "n_pages": len(pages),
        "n_valid_pages": len(valid),
        "n_invalid_pages": len(pages) - len(valid),
        "n_gold": total_gold,
        "n_pred": total_pred,
        "n_matched": total_matched,
        "n_detection_matched": total_det,
        "detection_precision": precision,
        "detection_recall": recall,
        "detection_f1": f1,
        "end_to_end": {
            "n_tables": total_gold,
            "grits_top_mean": sum(end_top) / total_gold if total_gold else 0.0,
            "grits_top_median": _median(end_top),
            "grits_con_mean": sum(end_con) / total_gold if total_gold else 0.0,
            "grits_con_median": _median(end_con),
        },
        "matched_only": {
            "n_tables": total_matched,
            "grits_top_mean": sum(m_top) / total_matched if total_matched else None,
            "grits_top_median": _median(m_top) if m_top else None,
            "grits_con_mean": sum(m_con) / total_matched if total_matched else None,
            "grits_con_median": _median(m_con) if m_con else None,
        },
    }


def _fmt(v: float | None) -> str:
    return "  n/a " if v is None else f"{v:.4f}"


def print_summary(pages: list[dict], summary: dict[str, Any]) -> None:
    print()
    print(f"{'doc_id':<28} {'gold':>7} {'pred':>7} {'iou':>6} {'top':>7} {'con':>7}")
    for p in pages:
        if p["status"] != "valid":
            print(f"{p['id']:<28} INVALID {p['error']}")
            continue
        if not p["tables"]:
            print(f"{p['id']:<28} {'-':>7} {p['n_pred']:>7}")
            continue
        for t in p["tables"]:
            gs = "x".join(map(str, t["gold_shape"]))
            ps = "x".join(map(str, t["pred_shape"])) if t["matched"] else "miss"
            print(
                f"{p['id']:<28} {gs:>7} {ps:>7} {t['iou']:>6.3f} "
                f"{t['grits_top']:>7.4f} {t['grits_con']:>7.4f}"
            )
    print()
    e2e, mo = summary["end_to_end"], summary["matched_only"]
    print(
        f"pages valid/total: {summary['n_valid_pages']}/{summary['n_pages']}   "
        f"n_gold={summary['n_gold']} n_pred={summary['n_pred']} "
        f"n_matched={summary['n_matched']} n_det_matched={summary['n_detection_matched']}"
    )
    print(
        f"detection  P={summary['detection_precision']:.4f} "
        f"R={summary['detection_recall']:.4f} F1={summary['detection_f1']:.4f}"
    )
    print(
        f"end_to_end   GriTS_Top={_fmt(e2e['grits_top_mean'])} "
        f"GriTS_Con={_fmt(e2e['grits_con_mean'])}  (n={e2e['n_tables']})"
    )
    print(
        f"matched_only GriTS_Top={_fmt(mo['grits_top_mean'])} "
        f"GriTS_Con={_fmt(mo['grits_con_mean'])}  (n={mo['n_tables']})"
    )


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--manifest",
        type=Path,
        default=DEFAULT_MANIFEST,
        help=f"FinTabNet.c manifest.json (default: {DEFAULT_MANIFEST})",
    )
    ap.add_argument("--pages", nargs="+", default=None, help="only these document_ids")
    ap.add_argument("--limit", type=int, default=None, help="stop after N pages")
    ap.add_argument("--out", type=Path, required=True, help="result JSON path")
    ap.add_argument(
        "--vision-options",
        default=None,
        help="JSON object passed as find_tables(vision_options=...)",
    )
    ap.add_argument(
        "--match-iou", type=float, default=0.5, help="bbox IoU threshold (default 0.5)"
    )
    return ap.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    sys.path.insert(0, str(GT_DIR))

    vision_options: dict[str, Any] | None = None
    if args.vision_options:
        vision_options = json.loads(args.vision_options)
        if not isinstance(vision_options, dict):
            raise SystemExit("--vision-options must be a JSON object")

    if not args.manifest.exists():
        raise SystemExit(f"manifest not found: {args.manifest}")
    entries = load_gold_manifest(args.manifest)
    if args.pages:
        wanted = list(dict.fromkeys(args.pages))
        by_id = {e["document_id"]: e for e in entries}
        missing = [d for d in wanted if d not in by_id]
        if missing:
            print(f"warning: not in manifest: {', '.join(missing)}", file=sys.stderr)
        entries = [by_id[d] for d in wanted if d in by_id]
    if args.limit is not None:
        entries = entries[: max(0, args.limit)]
    if not entries:
        raise SystemExit("no pages selected")

    import pdfspine

    print(f"pdfspine: {pdfspine.__file__}", flush=True)
    print(f"manifest: {args.manifest}  pages: {len(entries)}", flush=True)
    print(f"vision_options: {json.dumps(vision_options)}", flush=True)

    pages: list[dict] = []
    t_start = time.time()
    for k, entry in enumerate(entries, 1):
        rec = score_page(entry, vision_options, args.match_iou)
        pages.append(rec)
        if rec["status"] == "valid":
            n_gold = rec["n_gold"]
            top = rec["grits_top_sum"] / n_gold if n_gold else 0.0
            con = rec["grits_con_sum"] / n_gold if n_gold else 0.0
            print(
                f"[{k}/{len(entries)}] {rec['id']} gold={n_gold} pred={rec['n_pred']} "
                f"matched={rec['n_matched']} top={top:.4f} con={con:.4f} "
                f"({rec['elapsed_s']}s)",
                flush=True,
            )
        else:
            print(
                f"[{k}/{len(entries)}] {rec['id']} INVALID {rec['error']}", flush=True
            )

    summary = summarize(pages)
    result = {
        "manifest": str(args.manifest.resolve()),
        "backend": "onnx",
        "vision_options": vision_options,
        "match_iou": args.match_iou,
        "pdfspine_file": pdfspine.__file__,
        "pdfspine_version": getattr(pdfspine, "__version__", None),
        "elapsed_s": round(time.time() - t_start, 1),
        "summary": summary,
        "pages": pages,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        json.dumps(result, indent=1, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print_summary(pages, summary)
    print(f"\nwrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
