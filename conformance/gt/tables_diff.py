#!/usr/bin/env python3
"""Differential TABLE-extraction test: pdfspine ``find_tables`` vs fitz.

pdfspine exposes ``Page.find_tables() -> TableFinder`` (M7). This harness measures,
objectively, how closely pdfspine's table detection and structure agree with fitz
(PyMuPDF), the reference implementation, on table-dense PDFs.

How it works
------------
fitz (AGPL) must NEVER be imported into our interpreter, and a Rust panic/abort in
pdfspine must not take down the run. So every ``find_tables`` call happens in a
SUBPROCESS under a wall-clock timeout (mirroring ``oracle_extract.py`` /
``pdfspine_worker.py`` / ``run_validation.py``). Native strategies use one
isolated process per page; the vision benchmark uses one persistent JSONL process
so the two TATR checkpoints load only once:

- ``--worker pdfspine`` runs inside the project venv (with our built wheel).
- ``--worker fitz``  runs inside ``.venv-oracle`` (PyMuPDF).

Each worker reads ``<pdf> <page_index>`` and prints a JSON list of tables, each::

    {"bbox": [x0,y0,x1,y1], "row_count": R, "col_count": C, "cells_text": "flat text"}

The parent (the default mode) drives both workers per page and compares:

  (a) table-COUNT agreement   — |#pdfspine - #fitz| == 0 ?
  (b) GRID-SHAPE agreement    — for tables matched by bbox IoU >= 0.5, does
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
import dataclasses
import hashlib
import importlib.metadata
import math
import platform
from functools import lru_cache
import json
import os
import queue
import subprocess
import sys
import threading
from html.parser import HTMLParser
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
    # Prefer a direct, lossless cell representation. pdfspine exposes spans for
    # both native and TATR tables; HTML remains only a compatibility fallback for
    # fitz and historical result files.
    rec["cells"] = []
    if ext is not None:
        try:
            for row, column, row_span, column_span, bbox in tbl.spans:
                raw_span = [row, column, row_span, column_span]
                if any(type(value) is not int for value in raw_span) or row < 0 or column < 0 or row_span <= 0 or column_span <= 0 or max(row_span, column_span) > 1_000_000:
                    rec["cells"].append({"row_nums": [], "column_nums": [], "cell_text": "", "raw_span": raw_span})
                    continue
                text = ""
                if row < len(ext) and column < len(ext[row] or []):
                    text = str(ext[row][column] or "")
                rec["cells"].append({
                    "row_nums": list(range(int(row), int(row) + int(row_span))),
                    "column_nums": list(
                        range(int(column), int(column) + int(column_span))
                    ),
                    "bbox": _as_bbox(bbox),
                    "cell_text": text,
                })
        except AttributeError:
            rec.pop("cells", None)  # A legacy table without spans may use HTML.
        except Exception as exc:  # noqa: BLE001
            rec["serialization_error"] = f"{type(exc).__name__}: {exc}"
    # fitz has no ``spans`` and no ``to_html``; its Table exposes a plain
    # row-major grid of cell bboxes (``rows[i].cells``, ``None`` where it found
    # no cell) and ``extract()`` text. Rebuild unspanned cells from those so the
    # oracle is scoreable at all -- without this every fitz table reaches the
    # scorer as an empty prediction and silently scores 0 on every metric.
    if "cells" not in rec and "serialization_error" not in rec and ext is not None:
        try:
            rec["cells"] = []
            for row_index, row in enumerate(tbl.rows):
                row_text = ext[row_index] if row_index < len(ext) else []
                for col_index, cell_bbox in enumerate(row.cells or []):
                    if cell_bbox is None:
                        continue
                    text = ""
                    if col_index < len(row_text or []):
                        text = str((row_text or [])[col_index] or "")
                    rec["cells"].append({
                        "row_nums": [row_index],
                        "column_nums": [col_index],
                        "bbox": _as_bbox(cell_bbox),
                        "cell_text": text,
                    })
        except AttributeError:
            rec.pop("cells", None)
        except Exception as exc:  # noqa: BLE001
            rec["serialization_error"] = f"{type(exc).__name__}: {exc}"
    for attr in ("confidence", "source", "text_source", "metadata"):
        try:
            rec[attr] = getattr(tbl, attr)
        except Exception:  # noqa: BLE001
            pass
    # Cell-structure HTML (with colspan/rowspan) for the gold-GT / GriTS mode. Both
    # pdfspine and fitz Tables expose ``to_html()``; it is the cleanest source of
    # per-cell row/col spans + text. Captured opportunistically; absent on error.
    try:
        rec["html"] = tbl.to_html()
    except Exception:  # noqa: BLE001
        rec["html"] = None
    return rec


WORKER_SCHEMA = "pdfspine.table-worker.v2"


def eval_config(strategy="lines", backend=None, vision_options=None, *, mode="page-e2e"):
    strategy = strategy.casefold()
    backend = (backend or ("tatr" if strategy in {"vision", "tatr"} else "native")).casefold()
    if backend not in {"native", "tatr", "onnx", "tableformer"}:
        raise ValueError(f"unsupported evaluation backend: {backend}")
    if backend == "tableformer" and mode != "gold-crop-tsr":
        raise ValueError("TableFormer evaluation only supports gold-crop-tsr")
    if strategy not in {"lines", "lines_strict", "text", "vision", "tatr"}:
        raise ValueError(f"unsupported strategy: {strategy}")
    if (backend == "native") != (strategy not in {"vision", "tatr"}):
        raise ValueError("native strategies and vision backends cannot be mixed")
    if vision_options is not None and not isinstance(vision_options, dict):
        raise ValueError("vision options must be a JSON object")
    options = dict(vision_options or {})
    json.dumps(options, allow_nan=False)
    if backend == "native" and options:
        raise ValueError("native evaluation does not accept vision options")
    if options.get("local_files_only") is False:
        raise ValueError("evaluation does not download model weights")
    result = {"strategy": strategy, "backend": backend, "options": options}
    if mode not in {"page-e2e", "gold-crop-tsr"}:
        raise ValueError("unsupported evaluation mode")
    if mode == "gold-crop-tsr":
        if backend == "native":
            raise ValueError("native extraction is not detector-free TSR")
        if options.get("ocr_if_no_text", False) is not False:
            raise ValueError("gold-crop TSR requires native words with OCR disabled")
        options["ocr_if_no_text"] = False
        result.update(mode=mode, word_source="pdfspine-native")
    return result


@lru_cache(maxsize=64)
def _file_identity(filename, size, modified):
    path = Path(filename)
    with path.open("rb") as stream:
        sha = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"path": str(path.resolve()), "bytes": size, "sha256": sha}


def _identity(path):
    path = Path(path).resolve()
    stat = path.stat()
    return _file_identity(str(path), stat.st_size, stat.st_mtime_ns)


def _vision_runtime_metadata(strategy, backend=None, vision_options=None, *, mode="page-e2e", used_runtime=None):
    config = eval_config(strategy, backend, vision_options, mode=mode)
    import pdfspine
    from pdfspine import _core
    metadata = {"backend": config["backend"], "requested": config,
                "evaluator_source": _identity(THIS),
                "python": sys.version, "executable": sys.executable,
                "platform": platform.platform(), "module": pdfspine.__file__,
                "extension": _identity(_core.__file__)}
    if config["backend"] == "native":
        metadata["effective_options"] = {}
        return metadata
    if config["backend"] == "onnx":
        from pdfspine import _onnx as implementation
        options = implementation.OnnxOptions.from_mapping(config["options"])
        packages = ("onnxruntime", "numpy", "Pillow")
    else:
        from pdfspine import _tatr as implementation
        options = implementation.TatrOptions.from_mapping(config["options"])
        packages = ("torch", "transformers", "numpy", "Pillow")
    runtime = used_runtime if used_runtime is not None else implementation._get_runtime(options)
    metadata.update(dict(runtime.metadata))
    metadata["effective_options"] = dataclasses.asdict(options)
    metadata["backend_source"] = _identity(implementation.__file__)
    metadata["word_policy"] = "native-with-ocr-fallback" if options.ocr_if_no_text else "native-only"
    metadata["packages"] = {name: importlib.metadata.version(name) for name in packages}
    if config["backend"] == "onnx":
        metadata["session_providers"] = {name: session.get_providers() for name, session in getattr(runtime, "_sessions", {}).items()}
    files = []
    roles = (("table",) if mode == "gold-crop-tsr" else ("layout", "table")) if config["backend"] == "onnx" else ("detection", "structure")
    if mode == "gold-crop-tsr":
        metadata["executed_model_roles"] = ["table" if config["backend"] == "onnx" else "structure"]
        metadata["loaded_model_roles"] = sorted(getattr(runtime, "_sessions", {})) if config["backend"] == "onnx" else list(roles)
    for role in roles:
        source = metadata.get(role + "_model")
        if not source:
            raise ValueError(f"runtime omitted {role} model provenance")
        path = Path(source).expanduser()
        if not path.exists() and config["backend"] == "tatr":
            from huggingface_hub import try_to_load_from_cache
            for filename in ("config.json", "model.safetensors", "pytorch_model.bin"):
                cached = try_to_load_from_cache(source, filename, revision=metadata.get(role + "_revision"))
                if isinstance(cached, str):
                    files.append({"role": role, "filename": filename, **_identity(cached)})
            if not any(f["role"] == role for f in files):
                raise ValueError(f"cached model provenance unavailable: {source}")
        elif path.is_dir():
            for child in sorted(path.rglob("*")):
                if child.is_file() and child.suffix in {".json", ".safetensors", ".bin", ".onnx"}:
                    files.append({"role": role, "filename": child.name, **_identity(child)})
        else:
            files.append({"role": role, "filename": path.name, **_identity(path)})
    for role in roles:
        if not any(item["role"] == role and Path(item["filename"]).suffix in {".onnx", ".safetensors", ".bin"} for item in files):
            raise ValueError(f"missing model weight fingerprint: {role}")
    metadata["model_files"] = files
    return metadata


def _crop_request(value):
    if not isinstance(value, dict) or set(value) - {"table_id", "bbox", "coordinate_space", "padding", "source_page_bbox"}:
        raise ValueError("gold crop request must be a known-field object")
    if not isinstance(value.get("table_id"), str) or not value["table_id"]:
        raise ValueError("gold crop requires a nonempty table ID")
    if value.get("coordinate_space") not in {"page-display", "fintabnet-page"}:
        raise ValueError("gold crop requires an explicit supported coordinate space")
    bbox = value.get("bbox")
    if not isinstance(bbox, (list, tuple)) or len(bbox) != 4 or any(type(v) not in (float, int) or not math.isfinite(v) for v in bbox) or bbox[2] <= bbox[0] or bbox[3] <= bbox[1]:
        raise ValueError("gold crop requires a finite positive box")
    padding = value.get("padding", 0)
    if type(padding) is not int or not 0 <= padding <= 20:
        raise ValueError("gold crop padding must be integer pixels in [0,20]")
    result = {**value, "bbox": list(bbox), "padding": padding}
    if "source_page_bbox" in value:
        extent = value["source_page_bbox"]
        if value["coordinate_space"] != "fintabnet-page":
            raise ValueError("source page extent requires explicit FinTabNet coordinates")
        if (not isinstance(extent, (list, tuple)) or len(extent) != 4
                or any(type(v) not in (float, int) or not math.isfinite(v) for v in extent)
                or extent[:2] not in ([0, 0], (0, 0)) or extent[2] <= 0 or extent[3] <= 0):
            raise ValueError("FinTabNet source page extent must be finite, positive and zero-origin")
        if bbox[0] < 0 or bbox[1] < 0 or bbox[2] > extent[2] or bbox[3] > extent[3]:
            raise ValueError("gold crop lies outside its source page extent")
        result["source_page_bbox"] = list(extent)
    return result


def _validate_fintabnet_crop(page, request):
    """Prove the source page.rect convention; never translate by CropBox origin."""
    if page.rotation != 0:
        raise ValueError("FinTabNet source crop requires unrotated page coordinates")
    crop = tuple(page.cropbox)
    extent = request.get("source_page_bbox")
    if extent is None:
        if crop != tuple(page.mediabox):
            raise ValueError("cropped FinTabNet page requires source_page_bbox proof")
        return  # Existing unrotated full-page requests retain their contract.
    visible = (crop[2] - crop[0], crop[3] - crop[1])
    if any(not math.isfinite(v) or v <= 0 for v in visible) or any(
        abs(actual - declared) > 1e-3 for actual, declared in zip(visible, extent[2:])
    ):
        raise ValueError("FinTabNet source page extent does not match visible page dimensions")


def _validate_response(value, request_id=None, expected_config=None, expected_crop=None):
    if not isinstance(value, dict) or type(value.get("ok")) is not bool:
        raise ValueError("worker response must contain boolean ok")
    if request_id is not None and value.get("request_id") != request_id:
        raise ValueError("worker response request_id mismatch")
    if not isinstance(value.get("tables"), list):
        raise ValueError("worker tables must be a list")
    if not isinstance(value.get("backend_metadata", {}), dict):
        raise ValueError("worker metadata must be an object")
    if value["ok"] and expected_config is not None:
        metadata = value.get("backend_metadata", {})
        if metadata.get("backend") != expected_config["backend"] or metadata.get("requested") != expected_config:
            raise ValueError("worker backend/options identity does not match request")
    if value["ok"] and expected_config and expected_config.get("mode") == "gold-crop-tsr":
        if len(value["tables"]) != 1:
            raise ValueError("gold crop must return exactly one identity-bearing result")
        record = value["tables"][0]
        if not isinstance(record, dict) or record.get("crop_request") != _crop_request(expected_crop):
            raise ValueError("gold crop result identity mismatch")
        if record.get("table_id") != expected_crop["table_id"] or record.get("quality") not in {"valid", "empty", "invalid"}:
            raise ValueError("invalid gold crop result status/identity")
        if not isinstance(record.get("cells"), list) or any(not isinstance(c, dict) for c in record["cells"]):
            raise ValueError("gold crop cells must be raw objects")
        json.dumps(record, allow_nan=False)
        return value
    for table in value["tables"]:
        if not isinstance(table, dict):
            raise ValueError("worker table must be an object")
        bbox = table.get("bbox")
        if not isinstance(bbox, (tuple, list)) or len(bbox) != 4 or any(type(x) not in (int, float) or not math.isfinite(x) for x in bbox):
            raise ValueError("worker table bbox must be finite")
        if "cells" in table and (not isinstance(table["cells"], list) or any(not isinstance(c, dict) for c in table["cells"])):
            raise ValueError("worker cells must be a list of objects")
    return value


def _worker_pdfspine(
    pdf: str,
    page_index: int,
    strategy: str = "lines", backend=None, vision_options=None,
    *, mode="page-e2e", crop_request=None,
) -> tuple[list[dict], dict]:
    import pdfspine

    doc = pdfspine.open(pdf)
    try:
        page = doc.load_page(page_index)
        config = eval_config(strategy, backend, vision_options, mode=mode)
        if mode == "gold-crop-tsr":
            request = _crop_request(crop_request)
            if request["coordinate_space"] == "fintabnet-page":
                _validate_fintabnet_crop(page, request)
            if config["backend"] == "tableformer":
                import tableformer_adapter
                result = tableformer_adapter.recognize(page, request["bbox"], options=config["options"], padding=request["padding"])
                from pdfspine import _core
                metadata = {**{key: result.metadata[key] for key in (
                                "model_files", "effective_model_config", "runtime_source", "packages",
                                "effective_options", "backend_source", "track_identity",
                                "loaded_model_roles", "executed_model_roles", "word_assignment", "recognition_options") if key in result.metadata},
                            "backend": "tableformer", "requested": config,
                            "evaluator_source": _identity(THIS), "python": sys.version,
                            "executable": sys.executable, "platform": platform.platform(),
                            "module": pdfspine.__file__, "extension": _identity(_core.__file__)}
                record = {"table_id": request["table_id"], "crop_request": request,
                          "cells": result.cells, "quality": result.quality,
                          "quality_reasons": list(result.reasons), "metadata": result.metadata}
                return [record], metadata
            from pdfspine import _onnx, _tatr
            implementation = _onnx if config["backend"] == "onnx" else _tatr
            options_type = implementation.OnnxOptions if config["backend"] == "onnx" else implementation.TatrOptions
            options = options_type.from_mapping(config["options"])
            result = implementation._recognize_gold_crop(page, request["bbox"], options=config["options"], padding=request["padding"])
            runtime = implementation._get_runtime(options)
            record = {"table_id": request["table_id"], "crop_request": request,
                      "cells": result.cells, "quality": result.quality, "quality_reasons": list(result.reasons),
                      "metadata": result.metadata}
            metadata = _vision_runtime_metadata(strategy, backend, config["options"], mode=mode, used_runtime=runtime)
            return [record], metadata
        kwargs = {} if config["backend"] == "native" else {"backend": config["backend"], "vision_options": config["options"]}
        finder = page.find_tables(strategy=strategy, **kwargs)
        tables = list(getattr(finder, "tables", finder))
        records = [_table_record(t) for t in tables]
        if any(record.get("extract_error") or record.get("serialization_error") for record in records):
            raise ValueError("table extraction failed during worker serialization")
        metadata = _vision_runtime_metadata(strategy, backend, vision_options)
        if not metadata:
            metadata = next(
                (
                    dict(record["metadata"])
                    for record in records
                    if isinstance(record.get("metadata"), dict)
                    and record["metadata"]
                ),
                {},
            )
        return records, metadata
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


def _run_worker(mode: str, pdf: str, page_index: int, strategy: str = "lines", backend=None, vision_options=None, request_id=None, eval_mode="page-e2e", crop_request=None) -> int:
    """Worker entrypoint: emit ``{"ok":bool, "tables":[...], "error":...}`` JSON.

    fitz (and MuPDF's C layer) write warnings to **stdout** ("Consider using the
    pymupdf_layout package", "MuPDF error: ..."), which would corrupt the JSON the
    parent parses. We dup the real stdout fd aside, redirect fd 1 -> fd 2 (stderr)
    for the duration of extraction so ALL such chatter (Python- AND C-level) lands
    on stderr, then write the JSON result to the saved real stdout.
    """
    out: dict = {
        "schema": WORKER_SCHEMA, "type": "result", "request_id": request_id,
        "ok": False,
        "tables": [],
        "backend_metadata": {},
        "error": None,
    }
    real_stdout_fd = os.dup(1)
    try:
        os.dup2(2, 1)  # fd 1 -> stderr while the engine runs
        sys.stdout = sys.stderr
        try:
            if mode == "pdfspine":
                out["tables"], out["backend_metadata"] = _worker_pdfspine(
                    pdf, page_index, strategy, backend, vision_options, mode=eval_mode, crop_request=crop_request
                )
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


def _write_all(fd: int, data: bytes) -> None:
    """Write all bytes to ``fd``; ``os.write`` may legally short-write."""
    remaining = memoryview(data)
    while remaining:
        written = os.write(fd, remaining)
        if written <= 0:
            raise BrokenPipeError("table worker protocol write returned zero bytes")
        remaining = remaining[written:]


def _emit_json(fd: int, obj: dict) -> None:
    """Write JSON to the saved real stdout fd, bypassing any redirection."""
    data = (json.dumps(obj) + "\n").encode("utf-8")
    with contextlib.suppress(Exception):
        _write_all(fd, data)
    with contextlib.suppress(Exception):
        os.close(fd)


def _write_json_line(fd: int, obj: dict) -> None:
    """Write one JSONL message without closing a persistent worker's fd."""
    _write_all(fd, (json.dumps(obj) + "\n").encode("utf-8"))


def _run_jsonl_worker(strategy: str, backend=None, vision_options=None) -> int:
    """Persistent pdfspine worker; TATR models are loaded at most once."""
    real_stdout_fd = os.dup(1)
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    _write_json_line(real_stdout_fd, {
        "schema": WORKER_SCHEMA,
        "type": "ready",
        "backend": {"name": "pdfspine", "strategy": strategy},
    })
    try:
        for raw in sys.stdin:
            request: dict = {}
            try:
                request = json.loads(raw)
                if request.get("type") == "close":
                    break
                if request.get("schema") != WORKER_SCHEMA or request.get("type") != "extract":
                    raise ValueError("invalid worker request schema/type")
                request_id = request.get("request_id")
                tables, backend_metadata = _worker_pdfspine(
                    str(request["pdf"]),
                    int(request.get("page_index", 0)),
                    str(request.get("strategy") or strategy),
                    request.get("backend", backend), request.get("options", vision_options),
                    mode=request.get("mode", "page-e2e"), crop_request=request.get("crop_request"),
                )
                response = {
                    "schema": WORKER_SCHEMA,
                    "type": "result",
                    "request_id": request_id,
                    "ok": True,
                    "tables": tables,
                    "backend_metadata": backend_metadata,
                    "error": None,
                }
            except Exception as exc:  # noqa: BLE001
                response = {
                    "schema": WORKER_SCHEMA,
                    "type": "result",
                    "request_id": request.get("request_id"),
                    "ok": False,
                    "tables": [],
                    "backend_metadata": {},
                    "error": f"{type(exc).__name__}: {exc}",
                }
            _write_json_line(real_stdout_fd, response)
    finally:
        with contextlib.suppress(Exception):
            os.close(real_stdout_fd)
    return 0


class PersistentPdfspineWorker:
    """JSONL client used by vision benchmarks to amortize model startup."""

    def __init__(
        self,
        python: str,
        strategy: str,
        timeout: float,
        startup_timeout: float, backend=None, vision_options=None, *, mode="page-e2e",
    ) -> None:
        self.config = eval_config(strategy, backend, vision_options, mode=mode)
        self.strategy = strategy
        self.timeout = timeout
        self.startup_timeout = startup_timeout
        self._first_request = True
        self._sequence = 0
        self._lines: queue.Queue[str | None] = queue.Queue()
        self._process = subprocess.Popen(
            [python, "-u", str(THIS), "--worker-jsonl", "--strategy", strategy],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        assert self._process.stdout is not None

        def read_stdout() -> None:
            for line in self._process.stdout:
                self._lines.put(line)
            self._lines.put(None)

        self._reader = threading.Thread(target=read_stdout, daemon=True)
        self._reader.start()
        try:
            ready = self._read_line(30.0)
            if ready.get("type") != "ready" or ready.get("schema") != WORKER_SCHEMA:
                raise RuntimeError(
                    f"persistent table worker did not become ready: {ready}"
                )
        except Exception:
            self.close()
            raise

    def _read_line(self, timeout: float) -> dict:
        try:
            line = self._lines.get(timeout=timeout)
        except queue.Empty as exc:
            raise TimeoutError(f"table worker timed out after {timeout}s") from exc
        if line is None:
            raise RuntimeError(
                f"table worker exited with code {self._process.poll()}"
            )
        return json.loads(line)

    def call(self, pdf: Path, page_index: int, crop_request=None) -> dict:
        self._sequence += 1
        request_id = f"{pdf.name}:{page_index}:{self._sequence}"
        request = {
            "schema": WORKER_SCHEMA,
            "type": "extract",
            "request_id": request_id,
            "pdf": str(pdf),
            "page_index": int(page_index),
            "strategy": self.strategy,
            "backend": self.config["backend"], "options": self.config["options"],
        }
        if self.config.get("mode") == "gold-crop-tsr":
            request.update(mode="gold-crop-tsr", crop_request=_crop_request(crop_request))
        try:
            if self._process.stdin is None:
                raise RuntimeError("table worker stdin is closed")
            self._process.stdin.write(json.dumps(request) + "\n")
            self._process.stdin.flush()
            wait = self.startup_timeout if self._first_request else self.timeout
            response = self._read_line(wait)
            self._first_request = False
            if response.get("request_id") != request_id:
                raise RuntimeError("table worker response request_id mismatch")
            if response.get("schema") != WORKER_SCHEMA or response.get("type") != "result":
                raise ValueError("worker result schema/type mismatch")
            return _validate_response(response, request_id, self.config, crop_request)
        except Exception as exc:  # noqa: BLE001
            self.close()
            return {
                "ok": False,
                "tables": [],
                "backend_metadata": {},
                "error": f"{type(exc).__name__}: {exc}",
            }

    def close(self) -> None:
        process = getattr(self, "_process", None)
        if process is None:
            return
        if process.poll() is None:
            try:
                if process.stdin is not None:
                    process.stdin.write(json.dumps({"type": "close"}) + "\n")
                    process.stdin.flush()
                process.wait(timeout=5)
            except Exception:  # noqa: BLE001
                with contextlib.suppress(Exception):
                    process.terminate()
                with contextlib.suppress(Exception):
                    process.wait(timeout=2)
                if process.poll() is None:
                    with contextlib.suppress(Exception):
                        process.kill()
                    with contextlib.suppress(Exception):
                        process.wait(timeout=2)
        for stream in (process.stdin, process.stdout):
            if stream is not None:
                with contextlib.suppress(Exception):
                    stream.close()
        reader = getattr(self, "_reader", None)
        if reader is not None:
            reader.join(timeout=2)


# ===========================================================================
# PARENT MODE — drives both workers per page, compares, reports.
# ===========================================================================
def call_worker(py: str, mode: str, pdf: Path, page_index: int, timeout: float,
                strategy: str = "lines", backend=None, vision_options=None, *, eval_mode="page-e2e", crop_request=None) -> dict:
    """Spawn an isolated worker; return its JSON (or a synthesized failure rec).

    A timeout / non-zero exit / SIGABRT (Rust panic) becomes ``ok=False`` with an
    error string so one bad page can never crash the whole differential run.
    ``strategy`` is forwarded to the pdfspine worker's ``find_tables`` (the fitz
    worker ignores it; default ``"lines"`` preserves historical behavior).
    """
    cmd = [py, str(THIS), "--worker", mode, "--pdf", str(pdf), "--page", str(page_index),
           "--strategy", strategy]
    if backend is not None:
        cmd += ["--backend", backend]
    if vision_options is not None:
        cmd += ["--options-json", json.dumps(vision_options, allow_nan=False)]
    if eval_mode != "page-e2e":
        cmd += ["--eval-mode", eval_mode, "--crop-request-json", json.dumps(_crop_request(crop_request))]
    try:
        request_id = f"{pdf.name}:{page_index}"
        cmd += ["--request-id", request_id]
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {
            "ok": False,
            "tables": [],
            "backend_metadata": {},
            "error": f"timeout after {timeout}s",
        }
    except Exception as exc:  # noqa: BLE001
        return {
            "ok": False,
            "tables": [],
            "backend_metadata": {},
            "error": f"spawn: {type(exc).__name__}: {exc}",
        }
    if proc.returncode != 0:
        # SIGABRT etc. surface as a negative returncode; capture stderr tail.
        tail = (proc.stderr or "").strip().splitlines()[-1:] or [""]
        return {
            "ok": False,
            "tables": [],
            "backend_metadata": {},
            "error": f"exit {proc.returncode}: {tail[0][:200]}",
        }
    try:
        response = json.loads(proc.stdout)
        if not isinstance(response, dict) or response.get("schema") != WORKER_SCHEMA or response.get("type") != "result":
            raise ValueError("worker result schema/type mismatch")
        return _validate_response(response, request_id, eval_config(strategy, backend, vision_options, mode=eval_mode) if mode == "pdfspine" else None, crop_request)
    except (json.JSONDecodeError, ValueError, TypeError):
        tail = (proc.stdout or "").strip()[-200:]
        return {
            "ok": False,
            "tables": [],
            "backend_metadata": {},
            "error": f"bad json: {tail!r}",
        }


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
            if v >= iou_thr:
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


# ===========================================================================
# GOLD-GT MODE — score pdfspine ``find_tables`` against FinTabNet.c human gold
# cell structure with GriTS (the recognized TSR metric). This is the FIRST
# ABSOLUTE cell-structure number (vs. the fitz-AGREEMENT default mode above).
#
# Pipeline per page:
#   1. parse gold tables from the FinTabNet.c annotation -> GriTS cells,
#   2. run pdfspine in an isolated worker -> predicted tables (+ to_html()),
#   3. consume direct predicted cells (HTML only for legacy fallback),
#   4. match predicted<->gold tables by bbox IoU, score each pair with GriTS_Top
#      (topology) and GriTS_Con (content); unmatched gold tables score 0.
# ===========================================================================
def _gold_cells_from_annotation(table_anno: dict) -> tuple[list[dict], list[float]]:
    """FinTabNet.c annotation table -> (GriTS cells, table bbox).

    Each gold cell carries ``row_nums``/``column_nums`` (the span) and the cell's
    gold text (``json_text_content`` preferred, else ``pdf_text_content``).
    """
    cells: list[dict] = []
    for c in table_anno.get("cells", []):
        rn = list(c.get("row_nums") or [])
        cn = list(c.get("column_nums") or [])
        text = (c.get("json_text_content") or c.get("pdf_text_content") or "")
        cells.append({"row_nums": rn, "column_nums": cn, "cell_text": text.strip()})
    bbox = [float(v) for v in (table_anno.get("pdf_table_bbox") or [0, 0, 0, 0])[:4]]
    return cells, bbox


class _TableHTMLParser(HTMLParser):
    """Parse a ``<table>`` (as emitted by ``Table.to_html()``) into GriTS cells.

    Honors ``colspan``/``rowspan`` with a standard occupancy grid (like an HTML
    renderer): each ``<td>``/``<th>`` claims the next free column in its row and
    fills the rows/cols it spans, so the resulting ``row_nums``/``column_nums``
    are the true grid indices the cell occupies. ``<br>`` becomes a space.
    """

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.cells: list[dict] = []
        self._row = -1
        self._occupied: set[tuple[int, int]] = set()
        self._col_cursor = 0
        self._cur: dict | None = None
        self._text: list[str] = []

    def handle_starttag(self, tag: str, attrs) -> None:  # noqa: ANN001
        a = dict(attrs)
        if tag == "tr":
            self._row += 1
            self._col_cursor = 0
        elif tag in ("td", "th"):
            try:
                colspan = max(1, int(a.get("colspan", "1")))
            except ValueError:
                colspan = 1
            try:
                rowspan = max(1, int(a.get("rowspan", "1")))
            except ValueError:
                rowspan = 1
            # advance to the next free column in this row
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
    """Predicted table HTML -> GriTS cells (empty list on missing/garbled HTML)."""
    if not html:
        return []
    p = _TableHTMLParser()
    try:
        p.feed(html)
    except Exception:  # noqa: BLE001
        return []
    return [c for c in p.cells if "cell_text" in c]


def _pred_cells_from_record(record: dict) -> list[dict]:
    """Prefer a predictor's direct cells; fall back to historical HTML."""
    if "cells" in record:
        return [{**cell, "cell_text": " ".join(str(cell.get("cell_text") or "").split())} for cell in record["cells"]]
    return _pred_cells_from_html(record.get("html"))


def process_doc_gold(pdf: Path, doc_id: str, gold_tables: list[dict],
                     page_index: int, pdfspine_py: str, timeout: float,
                     strategy: str = "lines", predictor=None,
                     match_iou: float = 0.5, backend=None, vision_options=None) -> dict:
    """Score one page's pdfspine tables vs the page's gold tables with GriTS.

    ``gold_tables`` is the list of FinTabNet.c annotation tables for this page
    (already filtered to structure-eligible). Returns a per-doc record with, per
    matched table, GriTS_Top and GriTS_Con; unmatched gold tables count as 0.
    """
    from grits import grits_con, grits_top  # local import (pure stdlib helper)
    from cell_alignment import score_cells, topology_error, aggregate

    # Gold side.
    golds: list[dict] = []
    for t in gold_tables:
        cells, bbox = _gold_cells_from_annotation(t)
        if topology_error(cells):
            raise ValueError(f"invalid gold topology: {topology_error(cells)}")
        golds.append({"cells": cells, "bbox": bbox})

    # Native strategies retain per-page isolation. Vision uses a persistent
    # worker supplied by run_gold so two large models are loaded only once.
    ox_res = (
        predictor(pdf, page_index)
        if predictor is not None
        else call_worker(pdfspine_py, "pdfspine", pdf, page_index, timeout, strategy, **({"backend": backend, "vision_options": vision_options} if backend is not None or vision_options is not None else {}))
    )
    try:
        _validate_response(ox_res)
    except (ValueError, TypeError) as exc:
        ox_res = {"ok": False, "tables": [], "error": f"protocol: {exc}"}
    raw_metadata = ox_res.get("backend_metadata")
    backend_metadata = dict(raw_metadata) if isinstance(raw_metadata, dict) else {}
    if not ox_res.get("ok"):
        # An execution failure is not a model miss.  Keep every metric undefined
        # so callers cannot accidentally aggregate infrastructure failure as a
        # zero-quality prediction.
        return {
            "id": doc_id,
            "pdf": str(pdf),
            "status": "invalid",
            "ox_ok": False,
            "ox_error": ox_res.get("error") or "pdfspine worker failed",
            "n_gold": len(golds),
            "n_pred": 0,
            "n_matched": 0,
            "n_detection_matched": 0,
            "detection_precision": None,
            "detection_recall": None,
            "detection_f1": None,
            "match_iou": match_iou,
            "backend_metadata": backend_metadata,
            "tables": [],
            "grits_top_sum": None,
            "grits_con_sum": None,
        }

    preds: list[dict] = []
    for rec in ox_res.get("tables", []):
        record_metadata = rec.get("metadata")
        if not backend_metadata and isinstance(record_metadata, dict):
            backend_metadata = dict(record_metadata)
        cells = _pred_cells_from_record(rec)
        table_bbox = rec.get("bbox") or [0, 0, 0, 0]
        detection_bbox = (
            record_metadata.get("detection_bbox")
            if isinstance(record_metadata, dict)
            else None
        )
        try:
            detection_bbox = _as_bbox(detection_bbox or table_bbox)
        except (AttributeError, TypeError, ValueError):
            detection_bbox = table_bbox
        preds.append({
            "cells": cells,
            "bbox": table_bbox,
            "detection_bbox": detection_bbox,
        })

    # Keep detector localization and final extraction geometry as two explicit
    # contracts. TATR's optional native-line guidance may enlarge only the
    # recognition crop; it must not receive credit in detector P/R/F1. Native
    # strategies have no raw detector box and therefore fall back to Table.bbox.
    detection_preds = [{"bbox": p["detection_bbox"]} for p in preds]
    detection_pairs = match_tables(golds, detection_preds, iou_thr=match_iou)
    structure_pairs = match_tables(golds, preds, iou_thr=match_iou)
    detection_by_gold = {gold_i: (pred_i, score) for gold_i, pred_i, score in detection_pairs}

    table_recs: list[dict] = []
    cell_records = []
    used_predictions = set()
    for gi, g in enumerate(golds):
        detection_match = detection_by_gold.get(gi)
        detection_fields = {
            "detection_matched": detection_match is not None,
            "detection_iou": round(detection_match[1], 3) if detection_match else 0.0,
        }
        match = next((pr for pr in structure_pairs if pr[0] == gi), None)
        if match is None:
            # Gold table the predictor missed entirely -> GriTS 0 (full penalty).
            cell_records.append(score_cells(g["cells"], []))
            table_recs.append({
                "matched": False, "iou": 0.0,
                "grits_top": 0.0, "grits_con": 0.0,
                "gold_shape": _shape(g["cells"]), "pred_shape": [0, 0],
                **detection_fields,
            })
            continue
        _, pi, v = match
        p = preds[pi]
        used_predictions.add(pi)
        alignment = score_cells(g["cells"], p["cells"])
        cell_records.append(alignment)
        if alignment["invalid_prediction"]:
            gt_top = gt_con = 0.0
        else:
            gt_top, _, _ = grits_top(g["cells"], p["cells"])
            gt_con, _, _ = grits_con(g["cells"], p["cells"])
        table_recs.append({
            "matched": True, "iou": round(v, 3),
            # Keep full precision for aggregate statistics; round only in reports.
            "grits_top": gt_top, "grits_con": gt_con,
            "gold_shape": _shape(g["cells"]), "pred_shape": None if alignment["invalid_prediction"] else _shape(p["cells"]),
            "invalid_prediction": alignment["invalid_prediction"],
            "prediction_error": alignment["prediction_error"], "cell_alignment": alignment,
            **detection_fields,
        })

    for pi, prediction in enumerate(preds):
        if pi not in used_predictions:
            cell_records.append(score_cells([], prediction["cells"]))
    n_gold = len(golds)
    n_matched = sum(1 for r in table_recs if r["matched"])
    n_detection_matched = len(detection_pairs)
    precision = n_detection_matched / len(preds) if preds else 0.0
    recall = n_detection_matched / n_gold if n_gold else 0.0
    detection_f1 = (
        2 * precision * recall / (precision + recall)
        if precision + recall
        else 0.0
    )
    return {
        "id": doc_id,
        "pdf": str(pdf),
        "status": "valid",
        "ox_ok": bool(ox_res.get("ok")),
        "ox_error": ox_res.get("error"),
        "n_gold": n_gold,
        "n_pred": len(preds),
        "n_matched": n_matched,
        "n_detection_matched": n_detection_matched,
        "detection_precision": precision,
        "detection_recall": recall,
        "detection_f1": detection_f1,
        "match_iou": match_iou,
        "backend_metadata": backend_metadata,
        "tables": table_recs,
        "input_pdf": _identity(pdf) if pdf.is_file() else None,
        "text_sources": sorted({str(rec.get("text_source", "not-reported")) for rec in ox_res.get("tables", [])}),
        "raw_predictions": ox_res.get("tables", []),
        "cell_alignment_records": cell_records,
        "cell_alignment": aggregate(cell_records),
        "grits_top_sum": sum(r["grits_top"] for r in table_recs),
        "grits_con_sum": sum(r["grits_con"] for r in table_recs),
    }


def _gold_metric_summary(docs: list[dict]) -> dict:
    """Aggregate valid gold pages into detection and two GriTS views.

    ``end_to_end`` assigns zero to missed gold tables. ``matched_only`` measures
    structure only on tables whose final ``Table.bbox`` matched the gold bbox.
    Detector P/R/F1 uses the raw model ``metadata.detection_bbox`` when present,
    falling back to ``Table.bbox`` for backends without separate detector output.
    Neither view is silently computed from failed worker pages.
    """
    from cell_alignment import aggregate
    valid = [d for d in docs if d.get("status", "valid") == "valid" and d.get("ox_ok")]
    total_gold = sum(d["n_gold"] for d in valid)
    total_pred = sum(d["n_pred"] for d in valid)
    total_matched = sum(d["n_matched"] for d in valid)
    total_detection_matched = sum(
        d.get("n_detection_matched", d["n_matched"]) for d in valid
    )
    all_tables = [table for doc in valid for table in doc["tables"]]
    matched_tables = [table for table in all_tables if table["matched"]]

    precision = total_detection_matched / total_pred if total_pred else 0.0
    recall = total_detection_matched / total_gold if total_gold else 0.0
    detection_f1 = (
        2 * precision * recall / (precision + recall)
        if precision + recall
        else 0.0
    )

    end_top = [float(table["grits_top"]) for table in all_tables]
    end_con = [float(table["grits_con"]) for table in all_tables]
    matched_top = [float(table["grits_top"]) for table in matched_tables]
    matched_con = [float(table["grits_con"]) for table in matched_tables]
    return {
        "n_gold": total_gold,
        "n_pred": total_pred,
        "n_matched": total_matched,
        "n_detection_matched": total_detection_matched,
        "detection_precision": precision,
        "detection_recall": recall,
        "detection_f1": detection_f1,
        "cell_alignment": aggregate([r for d in valid for r in d.get("cell_alignment_records", [])]),
        "end_to_end": {
            "n_tables": total_gold,
            "grits_top_mean": sum(end_top) / total_gold if total_gold else 0.0,
            "grits_top_median": _median(end_top),
            "grits_con_mean": sum(end_con) / total_gold if total_gold else 0.0,
            "grits_con_median": _median(end_con),
        },
        "matched_only": {
            "n_tables": total_matched,
            "grits_top_mean": (
                sum(matched_top) / total_matched if total_matched else None
            ),
            "grits_top_median": _median(matched_top) if matched_top else None,
            "grits_con_mean": (
                sum(matched_con) / total_matched if total_matched else None
            ),
            "grits_con_median": _median(matched_con) if matched_con else None,
        },
    }


def _shape(cells: list[dict]) -> list[int]:
    nr = max((max(c["row_nums"]) for c in cells), default=-1) + 1
    nc = max((max(c["column_nums"]) for c in cells), default=-1) + 1
    return [nr, nc]


def load_gold_manifest(path: Path) -> list[dict]:
    """Load a FinTabNet.c manifest (from ``fetch_fintabnet.py``).

    Returns per-page dicts with absolute ``pdf``/``annotation`` paths,
    ``pdf_status``, and the parsed structure-eligible gold tables.
    """
    data = json.loads(path.read_text(encoding="utf-8"))
    entries = data.get("entries") if isinstance(data, dict) else data
    if not isinstance(entries, list):
        raise ValueError("manifest entries must be a list")
    mdir = path.resolve().parent
    out: list[dict] = []
    manifest_issues = []
    ledger_tables = {}
    legacy = isinstance(data, dict) and data.get("schema") is None and data.get("dataset") == "FinTabNet.c"
    financial = isinstance(data, dict) and data.get("schema") == "pdfspine.financial-eval-drafts.v1"
    if not legacy and not financial:
        manifest_issues.append("unknown evaluation manifest schema/track")
    if financial and data.get("review_status") not in ("unreviewed", "reviewed"):
        manifest_issues.append("financial manifest requires explicit review_status")
    for name in ("selection", "review_ledger"):
        resource = data.get(name) if isinstance(data, dict) else None
        if financial and resource is None:
            manifest_issues.append(f"missing required {name} reference")
        if resource is not None:
            if not isinstance(resource, dict) or not isinstance(resource.get("path"), str):
                manifest_issues.append(f"invalid {name} reference")
            else:
                resource_path = (mdir / resource["path"]).resolve()
                if not resource_path.is_file():
                    manifest_issues.append(f"missing {name}")
                elif not resource.get("sha256") or _identity(resource_path)["sha256"] != resource["sha256"]:
                    manifest_issues.append(f"{name} SHA-256 mismatch")
    if financial and not any("review_ledger" in issue for issue in manifest_issues):
        try:
            ledger = json.loads((mdir / data["review_ledger"]["path"]).read_text())
            if ledger.get("dataset_id") != data.get("dataset_id") or ledger.get("selection_sha256") != data.get("selection", {}).get("sha256"):
                raise ValueError("ledger dataset/selection identity mismatch")
            for table in ledger["tables"]:
                identity = table["structure_id"]
                if not isinstance(identity, str) or identity in ledger_tables:
                    raise ValueError("ledger table IDs must be unique strings")
                ledger_tables[identity] = table
        except (ValueError, OSError, KeyError, TypeError, AttributeError) as exc:
            manifest_issues.append(f"invalid review_ledger: {exc}")
            ledger_tables = {}
    for index, e in enumerate(entries or []):
        issues = list(manifest_issues)
        if not isinstance(e, dict):
            e = {}
            issues.append("entry must be an object")
        table_refs = []
        ledger_reviews_complete = not financial
        if financial:
            ledger_reviews_complete = True
            if e.get("review_status") not in ("unreviewed", "reviewed"):
                issues.append("entry requires explicit review_status")
            for name in ("pdf_sha256", "annotation_sha256"):
                value = e.get(name)
                if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdefABCDEF" for c in value):
                    issues.append(f"missing/invalid required {name}")
            table_refs = e.get("tables")
            if not isinstance(table_refs, list) or not table_refs:
                issues.append("financial entry requires table artifact/review bindings")
                table_refs = []
            else:
                raw_table_refs = table_refs
                table_refs = []
                for table_ref in raw_table_refs:
                    if not isinstance(table_ref, dict):
                        issues.append("invalid table artifact reference")
                        continue
                    identity = table_ref.get("structure_id")
                    if not isinstance(identity, str) or not identity:
                        issues.append("table structure_id must be a nonempty string")
                        continue
                    if type(table_ref.get("source_table_index")) is not int or table_ref["source_table_index"] < 0:
                        issues.append("table source_table_index must be a nonnegative integer")
                        continue
                    if table_ref.get("review_status") not in ("unreviewed", "reviewed"):
                        issues.append("table requires explicit review_status")
                        continue
                    if any(not isinstance(table_ref.get(name), str) or not table_ref[name] for name in ("html", "html_sha256", "cells", "cells_sha256")):
                        issues.append("table artifact path/SHA fields must be nonempty strings")
                        continue
                    table_refs.append(table_ref)
                    ledger_table = ledger_tables.get(identity, {})
                    if not ledger_table:
                        issues.append("table missing from review_ledger")
                    else:
                        for name in ("document_id", "pdf_sha256", "annotation_sha256"):
                            if ledger_table.get(name) != e.get(name):
                                issues.append(f"ledger {name} binding mismatch")
                        for name in ("html", "html_sha256", "cells", "cells_sha256", "source_table_index", "review_status"):
                            if ledger_table.get(name) != table_ref.get(name):
                                issues.append(f"ledger table {name} binding mismatch")
                    human = ledger_table.get("human_review", {})
                    ledger_reviews_complete = ledger_reviews_complete and isinstance(human, dict) and human.get("status") == "reviewed" and bool(human.get("reviewer")) and bool(human.get("reviewed_at"))
                    if table_ref.get("review_status") not in ("unreviewed", "reviewed"):
                        issues.append("table requires explicit review_status")
                    for name in ("html", "cells"):
                        filename, expected = table_ref.get(name), table_ref.get(name + "_sha256")
                        if not isinstance(filename, str) or not isinstance(expected, str):
                            issues.append(f"missing table {name}/SHA binding")
                            continue
                        artifact = (mdir / filename).resolve()
                        if not artifact.is_file():
                            issues.append(f"missing table {name}")
                        elif _identity(artifact)["sha256"] != expected:
                            issues.append(f"table {name} SHA-256 mismatch")
        anno = e.get("annotation")
        if not isinstance(anno, str) or not anno:
            issues.append("annotation path must be a nonempty string")
            anno = None
        anno_p = (mdir / anno).resolve() if anno else None
        pdf = e.get("pdf")
        if not isinstance(pdf, str) or not pdf:
            issues.append("PDF path must be a nonempty string")
            pdf = None
        pdf_p = (mdir / pdf).resolve() if pdf else None
        page_index = e.get("pdf_page_index", 0)
        if type(page_index) is not int or page_index < 0:
            issues.append("pdf_page_index must be a nonnegative integer")
            page_index = 0
        review_state = e.get("review_status", data.get("review_status", "source-annotations" if legacy else "unknown") if isinstance(data, dict) else "unknown")
        if not isinstance(review_state, str):
            issues.append("review_status must be a string")
            review_state = "invalid"
        tables = []
        if anno_p is None or not anno_p.is_file():
            issues.append("missing annotation")
        else:
            try:
                tables = json.loads(anno_p.read_text(encoding="utf-8"))
                if not isinstance(tables, list) or any(not isinstance(t, dict) for t in tables):
                    raise ValueError("annotation must be a list of tables")
            except (ValueError, OSError) as exc:
                tables = []
                issues.append(f"invalid annotation: {exc}")
        if pdf_p is None or not pdf_p.is_file():
            issues.append("missing PDF")
        for role, target in (("pdf", pdf_p), ("annotation", anno_p)):
            expected = e.get(role + "_sha256")
            if expected and target and target.is_file() and _identity(target)["sha256"] != expected:
                issues.append(f"{role} SHA-256 mismatch")
        gold_tables = [t for t in tables if not t.get("exclude_for_structure")]
        if financial:
            eligible = {index: table for index, table in enumerate(tables) if not table.get("exclude_for_structure")}
            references = table_refs
            indices = [ref.get("source_table_index") for ref in references if isinstance(ref, dict)] if isinstance(references, list) else []
            if any(type(index) is not int for index in indices) or len(indices) != len(set(indices)) or set(indices) != set(eligible):
                issues.append("table review bindings do not cover eligible annotation tables exactly")
            else:
                for ref in references:
                    if eligible[ref["source_table_index"]].get("structure_id") != ref.get("structure_id"):
                        issues.append("annotation structure_id does not match review binding")
        out.append({
            "document_id": e.get("document_id") or (anno_p.stem if anno_p else f"entry-{index}"),
            "pdf": pdf_p, "annotation": anno_p,
            "partition": e.get("partition"),
            "historical_benchmark_exposure": e.get("historical_benchmark_exposure"),
            "input_issues": issues,
            "manifest_track": "historical-source-annotations" if legacy else "financial-drafts" if financial else "unknown",
            "manifest_review_status": data.get("review_status") if isinstance(data, dict) else None,
            "ledger_reviews_complete": ledger_reviews_complete,
            "all_table_reviews_complete": not financial or all(isinstance(t, dict) and t.get("review_status") == "reviewed" for t in table_refs),
            "input_identities": {role: _identity(path) for role, path in (("pdf", pdf_p), ("annotation", anno_p)) if path and path.is_file()},
            "review_status": review_state,
            "pdf_status": e.get("pdf_status"),
            "page_index": page_index,
            "gold_tables": gold_tables,
            "anno_license": e.get("anno_license"),
            "pdf_license": e.get("pdf_license"),
        })
    return out


def _score_crop(gold, record):
    from cell_alignment import score_cells, counts, topology_error
    from grits import grits_top, grits_con
    cells = record["cells"]
    scored = score_cells(gold, cells)
    invalid = scored["invalid_prediction"] or record["quality"] == "invalid"
    if invalid:
        scored.update(invalid_prediction=True, prediction_error=record.get("quality_reasons") or scored["prediction_error"])
        scored["span"] = counts(0, len(cells), len(gold))
        scored["content"] = counts(0, len(cells), len(gold))
    if topology_error(gold):
        raise ValueError("invalid gold topology")
    return {"cell_alignment": scored,
            "grits_top": 0.0 if invalid else grits_top(gold, cells)[0],
            "grits_con": 0.0 if invalid else grits_con(gold, cells)[0]}


def _run_gold_crops(manifest_path, python, timeout, report_path, json_path, strategy, startup_timeout, backend, vision_options):
    """Identity-paired native-word TSR; no detector matching or detector metrics."""
    from cell_alignment import aggregate
    config = eval_config(strategy, backend, vision_options, mode="gold-crop-tsr")
    pages, results, issues = [], [], []
    worker = None
    error = None
    attempted = 0
    metadata = None
    providers = {}
    try:
        pages = load_gold_manifest(manifest_path)
        if not pages:
            raise ValueError("manifest has no requested pages")
    except (ValueError, OSError, TypeError, KeyError, AttributeError) as exc:
        issues.append(f"input-validation: {type(exc).__name__}: {exc}")
    prepared = []
    from cell_alignment import topology_error
    for page in pages:
        if page.get("input_issues") or not page.get("pdf") or not page["pdf"].is_file():
            issues.append({"document_id": page["document_id"], "issues": page.get("input_issues") or ["missing PDF"]})
            continue
        ids = [t.get("structure_id") or f"{page['document_id']}:{i}" for i, t in enumerate(page["gold_tables"])]
        for index, table in enumerate(page["gold_tables"]):
            try:
                table_id = ids[index]
                if ids.count(table_id) != 1:
                    raise ValueError("duplicate requested table identity")
                gold, bbox = _gold_cells_from_annotation(table)
                if topology_error(gold):
                    raise ValueError(f"invalid gold topology: {topology_error(gold)}")
                crop_input = {"table_id": table_id, "bbox": bbox, "coordinate_space": "fintabnet-page", "padding": 0}
                if "pdf_full_page_bbox" in table:
                    if page.get("manifest_track") not in {"historical-source-annotations", "financial-drafts"}:
                        raise ValueError("source page proof requires a recognized FinTabNet annotation track")
                    crop_input["source_page_bbox"] = table["pdf_full_page_bbox"]
                request = _crop_request(crop_input)
                prepared.append((page, gold, request))
            except (ValueError, TypeError, KeyError, OverflowError) as exc:
                issues.append({"document_id": page["document_id"], "table_index": index, "issues": [f"input-validation: {type(exc).__name__}: {exc}"]})
    try:
        for page, gold, request in prepared:
                if worker is None:
                    worker = PersistentPdfspineWorker(python, strategy, timeout, startup_timeout, backend, config["options"], mode="gold-crop-tsr")
                table_id = request["table_id"]
                attempted += 1
                response = worker.call(page["pdf"], page["page_index"], request)
                if not response.get("ok"):
                    raise RuntimeError(response.get("error", "crop execution failed"))
                _validate_response(response, expected_config=config, expected_crop=request)
                current = response["backend_metadata"]
                stable = {k: v for k, v in current.items() if k != "session_providers"}
                if metadata is not None and stable != metadata:
                    raise ValueError("backend metadata changed within TSR run")
                metadata = stable
                for role, value in current.get("session_providers", {}).items():
                    if role in providers and providers[role] != value:
                        raise ValueError("backend session providers changed within TSR run")
                    providers[role] = value
                record = response["tables"][0]
                results.append({"document_id": page["document_id"], "table_id": table_id,
                                "prediction": record, **_score_crop(gold, record)})
    except Exception as exc:
        error = f"{type(exc).__name__}: {exc}"
    finally:
        if worker is not None:
            try:
                worker.close()
            except Exception as exc:
                error = error or f"shutdown: {type(exc).__name__}: {exc}"
    review_complete = bool(pages) and all(p.get("review_status") in {"source-annotations", "reviewed"} and p.get("all_table_reviews_complete", True) and p.get("ledger_reviews_complete", True) and (p.get("manifest_track") != "financial-drafts" or p.get("manifest_review_status") == "reviewed") for p in pages)
    requested = sum(len(p["gold_tables"]) for p in pages)
    complete = bool(pages) and not issues and len(results) == requested
    status = "invalid" if error else "valid" if complete and review_complete else "incomplete"
    diagnostic = {"cell_alignment": aggregate([r["cell_alignment"] for r in results]),
                  "grits_top": sum(r["grits_top"] for r in results)/len(results) if results else None,
                  "grits_con": sum(r["grits_con"] for r in results)/len(results) if results else None}
    payload = {"mode": "gold-crop-tsr", "requested": config, "status": status,
               "execution_status": "failed" if error else "success" if complete else "partial",
               "comparable": status == "valid", "official_aggregate": diagnostic if status == "valid" else None,
               "diagnostic_partial_aggregate": diagnostic, "n_pages_requested": len(pages) if pages else None,
               "n_tables_requested": requested if pages else None, "n_tables_attempted": attempted,
               "n_tables_evaluated": len(results), "missing_inputs": issues, "error": error,
               "backend_metadata": metadata, "docs": results,
               "review_statuses": sorted({p.get("review_status", "unknown") for p in pages}),
               "unknown_table_count": bool(issues), "word_source": "pdfspine-native", "ocr_enabled": False}
    json_path.parent.mkdir(parents=True, exist_ok=True)
    json_path.write_text(json.dumps(payload, indent=2, allow_nan=False))
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(f"# Gold-crop TSR: {status.upper()}\n\nNative words, OCR disabled; table-ID pairing, no detection metrics.\n\nRequested {payload['n_tables_requested']} tables; evaluated {len(results)}.\n\nOfficial aggregate: `{payload['official_aggregate']}`.\n\nUnreviewed drafts remain diagnostic-only. Details and provenance are in `{json_path.name}`.\n")
    return 0 if status == "valid" else 2 if error else 3


def run_gold(manifest_path: Path, pdfspine_py: str, timeout: float,
             report_path: Path, json_path: Path, strategy: str = "lines",
             startup_timeout: float = 600.0, match_iou: float = 0.5,
             backend=None, vision_options=None, *, mode="page-e2e") -> int:
    """Drive the gold-GT GriTS run over a FinTabNet.c manifest; write report+json.

    If no source PDFs are present (the common BLOCKED case in restricted
    environments), still writes a clean report stating exactly what is missing and
    how to obtain it — never a fabricated score.
    """
    if mode == "gold-crop-tsr":
        return _run_gold_crops(manifest_path, pdfspine_py, timeout, report_path, json_path, strategy, startup_timeout, backend, vision_options)
    config = eval_config(strategy, backend, vision_options)
    try:
        pages = load_gold_manifest(manifest_path)
        if not pages:
            raise ValueError("manifest has no requested pages")
    except (ValueError, OSError, TypeError, KeyError, AttributeError) as exc:
        failure = {"status": "incomplete", "execution_status": "not-started", "comparable": False,
                   "error_phase": "input-validation", "error": f"{type(exc).__name__}: {exc}",
                   "n_pages_requested": None, "n_pages_attempted": 0,
                   "grits_end_to_end": None, "cell_alignment": None, "docs": []}
        json_path.parent.mkdir(parents=True, exist_ok=True)
        json_path.write_text(json.dumps(failure, indent=2))
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text("# INCOMPLETE evaluation input\n\n" + failure["error"] + "\nNo score was computed.\n")
        return 3
    scored = [p for p in pages if p["pdf"] and p["pdf"].exists() and not p.get("input_issues")]
    print(f"Gold pages: {len(pages)} (PDF present: {len(scored)})", flush=True)

    wants_persistent = config["backend"] != "native" and bool(scored)
    persistent = None
    docs: list[dict] = []
    run_error: str | None = None
    observed_session_providers = {}
    try:
        if wants_persistent:
            persistent = PersistentPdfspineWorker(
                pdfspine_py, strategy, timeout, startup_timeout, backend, vision_options
            )
        for k, pg in enumerate(scored, 1):
            d = process_doc_gold(
                pg["pdf"], pg["document_id"], pg["gold_tables"],
                pg["page_index"], pdfspine_py, timeout, strategy,
                predictor=persistent.call if persistent is not None else None,
                match_iou=match_iou, backend=backend, vision_options=vision_options,
            )
            docs.append(d)
            prior_metadata = next((item.get("backend_metadata") for item in docs[:-1] if item.get("backend_metadata")), None)
            current_metadata = d.get("backend_metadata", {})
            if prior_metadata:
                def stable(metadata):
                    return {k: v for k, v in metadata.items() if k != "session_providers"}
                if stable(current_metadata) != stable(prior_metadata):
                    raise ValueError("backend metadata changed within one evaluation run")
            for role, providers in current_metadata.get("session_providers", {}).items():
                if role in observed_session_providers and observed_session_providers[role] != providers:
                    raise ValueError("backend session providers changed within one evaluation run")
                observed_session_providers[role] = providers
            if not d["ox_ok"]:
                run_error = (
                    f"{pg['document_id']} page {pg['page_index']}: "
                    f"{d['ox_error']}"
                )
                print(
                    f"[{k}/{len(scored)}] INVALID — {run_error}",
                    file=sys.stderr,
                    flush=True,
                )
                break
            mt = d["n_matched"]
            det_mt = d["n_detection_matched"]
            gt = d["n_gold"]
            top = (d["grits_top_sum"] / gt) if gt else 0.0
            con = (d["grits_con_sum"] / gt) if gt else 0.0
            print(
                f"[{k}/{len(scored)}] {pg['document_id']}: gold={gt} "
                f"pred={d['n_pred']} det-matched={det_mt} "
                f"structure-matched={mt} Det-F1={d['detection_f1']:.3f} "
                f"GriTS_Top={top:.3f} GriTS_Con={con:.3f}",
                flush=True,
            )
    except Exception as exc:  # noqa: BLE001
        run_error = f"{type(exc).__name__}: {exc}"
        print(f"INVALID gold run: {run_error}", file=sys.stderr, flush=True)
    finally:
        if persistent is not None:
            try:
                persistent.close()
            except Exception as exc:  # noqa: BLE001
                if run_error is None:
                    run_error = f"worker shutdown failed: {type(exc).__name__}: {exc}"

    execution_status = "failed" if run_error else ("partial" if len(scored) != len(pages) else "success")
    review_statuses = sorted({p.get("review_status", "unknown") for p in pages})
    review_complete = all(value in {"source-annotations", "reviewed"} for value in review_statuses) and all(p.get("all_table_reviews_complete", True) and p.get("ledger_reviews_complete", True) and (p.get("manifest_track") != "financial-drafts" or p.get("manifest_review_status") == "reviewed") for p in pages)
    status = "invalid" if run_error else ("valid" if execution_status == "success" and review_complete else "incomplete")
    comparable = status == "valid"
    summary = _gold_metric_summary(docs) if comparable else None
    backend_metadata = next(
        (d["backend_metadata"] for d in docs if d.get("backend_metadata")),
        {},
    )

    payload = {
        "status": status,
        "execution_status": execution_status,
        "comparable": comparable, "review_statuses": review_statuses,
        "purpose": "acceptance" if comparable else "diagnostic",
        "configuration": config,
        "gold_track": "original-source-annotations",
        "gold_text_selection": "historical-json-truthy-fallback-pdf",
        "manifest_sha256": _identity(manifest_path)["sha256"],
        "input_provenance": [{"id": p["document_id"], "files": p.get("input_identities", {}), "partition": p.get("partition"), "historical_benchmark_exposure": p.get("historical_benchmark_exposure")} for p in pages],
        "metric_source": _identity(GT_DIR / "cell_alignment.py"),
        "grits_source": _identity(GT_DIR / "grits.py"),
        "missing_inputs": [{"id": p["document_id"], "issues": p.get("input_issues") or ["missing PDF"]} for p in pages if p not in scored],
        "n_pages_requested": len(pages), "n_pages_missing": len(pages) - len(scored),
        "n_tables_requested_known": sum(len(p["gold_tables"]) for p in pages),
        "n_pages_unknown_table_count": sum(any("annotation" in issue for issue in p.get("input_issues", [])) for p in pages),
        "n_tables_missing_known": sum(len(p["gold_tables"]) for p in pages if p not in scored),
        "n_tables_evaluated": sum(d["n_gold"] for d in docs if d.get("ox_ok")),
        "diagnostic_summary": _gold_metric_summary(docs) if not comparable else None,
        "cell_alignment": summary["cell_alignment"] if summary is not None and comparable else None,
        "error": run_error,
        "mode": "gold-fintabnet",
        "manifest": str(manifest_path),
        "metric": "GriTS (grits.py)",
        "pdfspine_python": pdfspine_py,
        "find_tables_strategy": strategy,
        "worker": "persistent-jsonl" if wants_persistent else "per-page",
        "match_iou": match_iou,
        "bbox_contract": {
            "detection_metrics": (
                "Table.metadata.detection_bbox when present; otherwise Table.bbox"
            ),
            "grits_pairing": "final Table.bbox",
        },
        "detection_precision": (
            summary["detection_precision"] if summary is not None else None
        ),
        "detection_recall": (
            summary["detection_recall"] if summary is not None else None
        ),
        "detection_f1": summary["detection_f1"] if summary is not None else None,
        "grits_end_to_end": summary["end_to_end"] if summary is not None else None,
        "grits_matched_only": (
            summary["matched_only"] if summary is not None else None
        ),
        "backend_metadata": backend_metadata,
        "n_pages_total": len(pages),
        "n_pages_with_pdf": len(scored),
        "n_pages_attempted": len(docs),
        "n_pages_succeeded": sum(1 for d in docs if d.get("ox_ok")),
        "docs": docs,
    }
    json_path.parent.mkdir(parents=True, exist_ok=True)
    json_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding="utf-8")
    report = build_gold_report(
        manifest_path,
        pages,
        scored,
        docs,
        strategy,
        match_iou,
        status=status,
        run_error=run_error,
    )
    if not comparable:
        report = "**DIAGNOSTIC ONLY — not a complete reviewed/comparable acceptance run.**\n\n" + report
    report += "\n## Evaluation v2 contract\n\n" + f"Configuration: `{json.dumps(config, sort_keys=True)}`\n\nComparable: **{comparable}**; review states: {review_statuses}.\n"
    report += f"Requested/evaluated/missing pages: {len(pages)}/{len(docs)}/{len(pages)-len(scored)}. Draft or incomplete metrics are diagnostic only.\n"
    if summary is not None:
        report += "\nStrict cell_span_f1_v1 (micro): `" + json.dumps(summary["cell_alignment"], sort_keys=True) + "`\n"
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(report, encoding="utf-8")
    print(f"\nWrote {json_path}\nWrote {report_path}", flush=True)
    return 0 if comparable else 3


def build_gold_report(manifest_path: Path, pages: list[dict], scored: list[dict],
                      docs: list[dict], strategy: str = "lines",
                      match_iou: float = 0.5, *, status: str | None = None,
                      run_error: str | None = None) -> str:
    """Render the committed gold-GT report (absolute GriTS or a clean blocked one)."""
    man = json.loads(manifest_path.read_text(encoding="utf-8"))
    n_total = len(pages)
    n_pdf = len(scored)
    n_gold_tables = sum(len(p["gold_tables"]) for p in pages)
    anno_lic = man.get("annotations_license", "CDLA-Permissive-2.0")
    pdf_lic = man.get("pdf_license", "CDLA-Permissive-1.0")
    pdf_status = man.get("pdf_status_counts", {})
    if status is None:
        if any(not doc.get("ox_ok") for doc in docs):
            status = "invalid"
        elif n_pdf == 0:
            status = "blocked"
        else:
            status = "valid"

    L: list[str] = []
    L.append("# Table cell-structure GOLD GT — pdfspine `find_tables` vs FinTabNet.c (GriTS)\n")
    L.append(
        f"Harness: `{THIS}` (`--gold --strategy {strategy}`; find_tables "
        f"`strategy=\"{strategy}\"`; bbox match IoU ≥ {match_iou})  "
    )
    L.append("Metric: **GriTS** (Grid Table Similarity, Smock et al. arXiv:2303.00716 / 2203.12555) "
             "— `conformance/gt/grits.py`.  ")
    L.append(f"Dataset: **FinTabNet.c** — annotations `{anno_lic}`, source PDFs `{pdf_lic}`.  ")
    L.append(f"Provenance: annotations from `{man.get('annotations_source')}`; "
             f"source PDFs from `{man.get('pdf_source')}`.\n")
    L.append(f"Run status: **{status.upper()}**.\n")
    backend_metadata = next(
        (d["backend_metadata"] for d in docs if d.get("backend_metadata")),
        {},
    )
    if backend_metadata:
        L.append("Backend metadata: `" + json.dumps(
            backend_metadata, sort_keys=True, ensure_ascii=False
        ) + "`.\n")

    L.append("## Why GriTS\n")
    L.append("GriTS scores cell **topology** (row/col spans) and cell **content** in one "
             "F-score framework with per-cell partial credit, is transpose- and "
             "position-invariant (the two properties an ideal TSR metric should have), and "
             "is the canonical metric for FinTabNet.c. We compute **GriTS_Top** (topology) and "
             "**GriTS_Con** (content) via the factored 2D-MSS heuristic, a faithful stdlib port "
             "of Microsoft's reference `grits.py` (numpy→lists, `fitz.Rect` IoU→plain "
             "arithmetic; no AGPL `fitz` is imported). The metric implementation is shared, "
             "but this harness runs **full-page table detection first**. Microsoft's published "
             "TATR ~0.98 structure score evaluates TSR on gold cropped-table images with table "
             "words and is therefore **not directly comparable** to the end-to-end score below.\n")

    L.append("## Sample / provenance / license\n")
    L.append(f"- Sample requested: **{man.get('sample_requested')}** pages; fetched "
             f"**{n_total}** gold annotation pages (**{n_gold_tables}** structure-eligible "
             "gold tables).")
    L.append(f"- Source PDFs fetched: **{n_pdf}** / {n_total} (pdf status: `{pdf_status}`).")
    L.append(f"- Annotations license: **{anno_lic}** (permissive; commercial reuse OK).")
    L.append(f"- Source-PDF license: **{pdf_lic}** (permissive).")
    L.append("- Only permissively-licensed data is used. The data itself is gitignored "
             "(`conformance/gt/corpus-*/`); the committed deliverables are the fetcher, the "
             "metric, the harness mode, and this report.\n")

    if status in {"invalid", "incomplete"}:
        L.append(f"## Status: {status.upper()} — execution failed or requested inputs unavailable\n")
        L.append(f"- Error: `{run_error or 'requested PDF/annotation/provenance inputs unavailable'}`")
        L.append(f"- Pages completed successfully: **{sum(1 for d in docs if d.get('ox_ok'))}**; "
                 f"attempted: **{len(docs)}** / {n_pdf} PDFs present.")
        L.append("- **No aggregate detection or GriTS score is reported.** An execution "
                 "failure is not a model miss and must never be converted to zero. Re-run after "
                 "fixing the model cache, optional dependencies, device, or timeout.\n")
        return "\n".join(L)

    if n_pdf == 0:
        L.extend(_gold_blocked_section(man, pages))
        return "\n".join(L)

    summary = _gold_metric_summary(docs)
    end_to_end = summary["end_to_end"]
    matched_only = summary["matched_only"]
    valid = [d for d in docs if d.get("ox_ok")]

    L.append(f"## End-to-end extraction score (`strategy=\"{strategy}\"` vs gold)\n")
    L.append(
        f"- Gold tables scored: **{summary['n_gold']}** (across {len(valid)} pages); "
        f"pdfspine predicted **{summary['n_pred']}**, raw detector boxes matched "
        f"**{summary['n_detection_matched']}**, and final table boxes matched "
        f"**{summary['n_matched']}** by IoU ≥ {match_iou}."
    )
    L.append(
        f"- **Detection precision {summary['detection_precision']:.3f}, recall "
        f"{summary['detection_recall']:.3f}, F1 {summary['detection_f1']:.3f}**. "
        "These metrics use `Table.metadata.detection_bbox` when the backend "
        "provides it, before native-line crop guidance; other backends fall back "
        "to `Table.bbox`. Extra predictions reduce precision."
    )
    L.append(f"- **Recall-weighted GriTS_Top: mean "
             f"{end_to_end['grits_top_mean']:.3f}, median "
             f"{end_to_end['grits_top_median']:.3f}**")
    L.append(f"- **Recall-weighted GriTS_Con: mean "
             f"{end_to_end['grits_con_mean']:.3f}, median "
             f"{end_to_end['grits_con_median']:.3f}**")
    L.append("  Missed gold tables count as 0 by this harness's deliberate end-to-end "
             "policy. This is useful for tracking extraction quality, but it is not the "
             "published gold-crop TSR protocol.\n")

    L.append("## Matched-only structure score\n")
    if matched_only["n_tables"]:
        L.append(
            f"- Tables: **{matched_only['n_tables']}** (final `Table.bbox` "
            "IoU-matched only)."
        )
        L.append(f"- **GriTS_Top: mean {matched_only['grits_top_mean']:.3f}, median "
                 f"{matched_only['grits_top_median']:.3f}**")
        L.append(f"- **GriTS_Con: mean {matched_only['grits_con_mean']:.3f}, median "
                 f"{matched_only['grits_con_median']:.3f}**")
    else:
        L.append("- No table met the bbox match threshold; matched-only GriTS is undefined.")
    L.append("- This removes the missed-detection penalty, but detector-produced crops and "
             "pdfspine word extraction can still differ from Microsoft's evaluation inputs. "
             "A true apples-to-apples comparison with the published ~0.98 requires a pending "
             "**gold-crop TSR-only mode** that bypasses table detection and uses the benchmark's "
             "table crops/words.\n")

    L.append("## Per-document\n")
    L.append("| doc | gold | pred | det-match | final-match | GriTS_Top | GriTS_Con |")
    L.append("|-----|-----:|-----:|----------:|------------:|----------:|----------:|")
    for d in sorted(valid, key=lambda x: (x["grits_con_sum"] / x["n_gold"]) if x["n_gold"] else 0):
        g = d["n_gold"]
        top = (d["grits_top_sum"] / g) if g else 0.0
        con = (d["grits_con_sum"] / g) if g else 0.0
        L.append(
            f"| {d['id']} | {g} | {d['n_pred']} | "
            f"{d['n_detection_matched']} | {d['n_matched']} | "
            f"{top:.3f} | {con:.3f} |"
        )
    L.append("")
    return "\n".join(L)


def _gold_blocked_section(man: dict, pages: list[dict]) -> list[str]:
    """The clean BLOCKED report body when source PDFs were unreachable."""
    L: list[str] = []
    L.append("## Status: BLOCKED on source PDFs (no number fabricated)\n")
    L.append("The FinTabNet.c **gold annotations were fetched successfully** and parse "
             "cleanly into GriTS cells (verified: self-GriTS = 1.0 on every parsed gold "
             "table). The **GriTS metric and the full scoring harness are implemented and "
             "self-tested**. What is missing is the **source PDFs**: the `find_tables` "
             "prediction step needs the original FinTabNet single-page PDFs, whose page "
             "coordinate system the gold `pdf_bbox`/`pdf_table_bbox` annotations live in.\n")
    L.append("### Exactly what is missing\n")
    L.append("- The `bsmock/FinTabNet.c` HF dataset ships **annotations only** "
             "(`FinTabNet.c-PDF_Annotations.tar.gz` = 77,437 JSONs, **zero PDFs**).")
    L.append(f"- The matching PDFs come from the FinTabNet 1.0.0 mirror zip at "
             f"`{man.get('pdf_source')}` (the original DAX CDN is decommissioned — "
             "see `fetch_fintabnet.py`).")
    L.append("- That mirror was unreachable from this environment (or missing the "
             "members) on every retry in this run.")
    L.append(f"- Per-page fetch status in this run: `{man.get('pdf_status_counts')}`.\n")
    L.append("### How to unblock (one of)\n")
    L.append("1. Run `conformance/gt/fetch_fintabnet.py` from a network that can reach "
             "HuggingFace; it will Range-extract the per-page PDFs from the mirror zip and "
             "the manifest will flip `pdf_status` to `ok`. Then re-run `tables_diff.py "
             "--gold` — it is ready and will emit the absolute GriTS numbers with no "
             "further code change.")
    L.append("2. Or obtain the FinTabNet 1.0.0 `pdf/` tree by any other means and drop the "
             "single-page PDFs into the corpus `pdfs/` dir as `<document_id>.pdf` (the "
             "fetcher treats them as cached; `pdf_rel_path` is recorded per entry in the "
             "manifest).\n")
    L.append("### What IS proven now (no PDFs needed)\n")
    L.append("- `grits.py` self-test: 7 known-answer cases pass (identity=1.0, content "
             "sensitivity, topology text-blindness, spanning-cell penalty, empty/shape "
             "mismatch, LCS).")
    L.append("- Gold parser validated on real FinTabNet.c annotations: structure-eligible "
             "tables parse to GriTS cells with spans; self-GriTS = 1.0 on all of them.")
    L.append("- The pdfspine prediction path (direct `Table.spans` + `extract()` → "
             "GriTS cells, with HTML fallback) is implemented and unit-checked.")
    L.append("- The **full scoring pipeline is verified end-to-end** on a real pdfspine "
             "detection (CDC fixture, a 3×4 table): scoring its own output as gold gives "
             "GriTS_Top = 1.000 / GriTS_Con = 1.000 (matched IoU 1.0); a gold with one "
             "column removed drops to GriTS_Top ≈ 0.52 / GriTS_Con ≈ 0.67 — i.e. worker → "
             "direct cells → match → GriTS works and is sensitive to structural error.\n")
    L.append("This is the optional P3-5 task; per the PRD a clean blocked report (data "
             "unobtainable in-environment) is an acceptable deliverable. The harness will "
             "produce the absolute number unchanged the moment the PDFs are reachable.\n")

    # A short per-doc table of the GOLD structure that is fetched and ready to score.
    L.append("### Gold sample ready to score (per-doc)\n")
    L.append("| document_id | gold tables | gold rows×cols (largest) | pdf_status |")
    L.append("|-------------|------------:|--------------------------|------------|")
    rows: list[tuple] = []
    for p in pages:
        gts = p["gold_tables"]
        if not gts:
            continue
        # largest gold table shape on the page
        best = (0, 0)
        for t in gts:
            cells, _ = _gold_cells_from_annotation(t)
            sh = _shape(cells)
            if sh[0] * sh[1] > best[0] * best[1]:
                best = (sh[0], sh[1])
        rows.append((p["document_id"], len(gts), f"{best[0]}×{best[1]}",
                     p.get("pdf_status") or "—"))
    for did, ng, shp, st in rows[:30]:
        L.append(f"| {did} | {ng} | {shp} | {st} |")
    L.append("")
    return L


def _median(xs: list[float]) -> float:
    if not xs:
        return 0.0
    s = sorted(xs)
    n = len(s)
    mid = n // 2
    return s[mid] if n % 2 else (s[mid - 1] + s[mid]) / 2


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
    L.append("Match rule: bbox IoU >= 0.5; grid-shape = exact (rows,cols); "
             "cell-text = token-F1 (`gt/score.py`) of flattened cells, pdfspine-vs-fitz.\n")

    L.append("## Aggregate (pdfspine vs fitz)\n")
    L.append(f"- Documents scored: **{len(valid)}** ({len(docs)-len(valid)} open-errors), "
             f"pages: **{tot_pages}**")
    L.append(f"- Tables detected: pdfspine **{tot_ox}**, fitz **{tot_fz}** "
             f"(ratio pdfspine/fitz = {tot_ox/tot_fz:.2f})" if tot_fz else
             f"- Tables detected: pdfspine **{tot_ox}**, fitz **{tot_fz}**")
    L.append(f"- **Table-count agreement** (per-page #pdfspine==#fitz): "
             f"**{count_agree_rate*100:.1f}%** ({count_agree_pages}/{tot_pages} pages)")
    L.append(f"- Tables matched by IoU>=0.5: **{tot_matched}** "
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
    ap.add_argument("--worker-jsonl", action="store_true", help=argparse.SUPPRESS)
    ap.add_argument("--request-id", default=None, help=argparse.SUPPRESS)
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
    ap.add_argument("--startup-timeout", type=float, default=600.0,
                    help="first persistent vision request timeout, including model load (s)")
    ap.add_argument("--match-iou", type=float, default=0.5,
                    help="minimum bbox IoU for gold table matching (default: 0.5)")
    ap.add_argument("--backend", choices=["native", "tatr", "onnx", "tableformer"], default=None)
    ap.add_argument("--eval-mode", choices=["page-e2e", "gold-crop-tsr"], default="page-e2e")
    ap.add_argument("--crop-request-json", default=None, help=argparse.SUPPRESS)
    ap.add_argument("--options-json", default=None, help="explicit vision options JSON object; no downloads")
    ap.add_argument("--strategy", default="lines",
                    help="find_tables strategy for the pdfspine worker in --gold mode "
                         "('lines' = default & historical behavior, 'text' = text-based "
                         "detection for borderless tables, 'vision' = TATR); non-'lines' runs write to "
                         "strategy-suffixed default report/json paths")
    # GOLD-GT mode: score pdfspine vs FinTabNet.c human gold with GriTS (absolute).
    ap.add_argument("--gold", type=Path, default=None,
                    help="FinTabNet.c manifest (from fetch_fintabnet.py): score "
                         "pdfspine find_tables vs gold cell structure with GriTS")
    # Defaults match existing gitignore globs so raw outputs are not tracked:
    #   gt-*.json is ignored; GT-REPORT*.md follows the committed-report convention.
    ap.add_argument("--report", type=Path, default=GT_DIR / "GT-REPORT-tables.md")
    ap.add_argument("--json", type=Path, default=GT_DIR / "gt-tables.json")
    args = ap.parse_args(argv)
    try:
        options = json.loads(args.options_json) if args.options_json is not None else None
        config = eval_config(args.strategy, args.backend, options, mode=args.eval_mode)
    except (ValueError, TypeError) as exc:
        ap.error(str(exc))
    if not 0.0 <= args.match_iou <= 1.0:
        ap.error("--match-iou must be between 0 and 1")
    if any(not math.isfinite(v) or v <= 0 for v in (args.timeout, args.startup_timeout)):
        ap.error("--timeout and --startup-timeout must be positive")

    # Worker dispatch (isolated subprocess).
    if args.worker_jsonl:
        return _run_jsonl_worker(args.strategy, args.backend, options)
    if args.worker:
        if not args.pdf:
            sys.stdout.write(json.dumps({"ok": False, "tables": [], "error": "no --pdf"}))
            return 2
        return _run_worker(args.worker, args.pdf, args.page, args.strategy, args.backend, options, args.request_id, args.eval_mode, json.loads(args.crop_request_json) if args.crop_request_json else None)

    # GOLD-GT mode (absolute cell-structure score vs FinTabNet.c via GriTS).
    if args.gold is not None:
        if not args.gold.exists():
            ap.error(f"gold manifest not found: {args.gold}")
        # Non-default strategies get suffixed default paths so a 'text' run can
        # never silently clobber the canonical (lines-default) gold report.
        sfx = "" if args.strategy == "lines" else f"-{args.strategy}"
        if args.eval_mode != "page-e2e":
            sfx += "-gold-crop-tsr-native-words"
        if args.backend is not None or options is not None:
            digest = hashlib.sha256(json.dumps(config, sort_keys=True).encode()).hexdigest()[:10]
            sfx += f"-{config['backend']}-{digest}"
        report = (args.report if args.report != GT_DIR / "GT-REPORT-tables.md"
                  else GT_DIR / f"GT-REPORT-tables-gold{sfx}.md")
        jsonp = (args.json if args.json != GT_DIR / "gt-tables.json"
                 else GT_DIR / f"gt-tables-gold{sfx}.json")
        return run_gold(
            args.gold, args.pdfspine_python, args.timeout, report, jsonp,
            args.strategy, args.startup_timeout, args.match_iou, args.backend, options, mode=args.eval_mode,
        )

    if args.eval_mode != "page-e2e" or args.backend is not None or options is not None:
        ap.error("--backend/--options-json require --gold; parity mode retains its historical native path")

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
