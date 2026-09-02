"""Failure semantics and metric-contract tests for the TATR gold harness."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tomllib

import pytest


def _harness():
    path = Path(__file__).resolve().parents[2] / "conformance" / "gt" / "tables_diff.py"
    spec = importlib.util.spec_from_file_location("_pdfspine_tatr_harness_test", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _gold_table(text: str = "A") -> dict:
    return {
        "pdf_table_bbox": [0, 0, 100, 100],
        "cells": [
            {
                "row_nums": [0],
                "column_nums": [0],
                "json_text_content": text,
            }
        ],
    }


def test_harness_worker_failure_is_invalid_not_zero_score(tmp_path):
    harness = _harness()
    result = harness.process_doc_gold(
        tmp_path / "unused.pdf",
        "fixture",
        [_gold_table()],
        0,
        sys.executable,
        1,
        predictor=lambda _pdf, _page: {
            "ok": False,
            "tables": [],
            "backend_metadata": {},
            "error": "model cache missing",
        },
    )

    assert result["status"] == "invalid"
    assert result["ox_ok"] is False
    assert result["detection_f1"] is None
    assert result["grits_top_sum"] is None
    assert result["tables"] == []


def test_run_gold_stops_at_first_failure_and_writes_invalid_report(
    tmp_path, monkeypatch
):
    harness = _harness()
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "annotations_license": "CDLA-Permissive-2.0",
                "pdf_license": "CDLA-Permissive-1.0",
                "sample_requested": 2,
            }
        ),
        encoding="utf-8",
    )
    pdfs = [tmp_path / "one.pdf", tmp_path / "two.pdf"]
    for pdf in pdfs:
        pdf.write_bytes(b"present")
    pages = [
        {
            "document_id": f"doc-{index}",
            "pdf": pdf,
            "page_index": 0,
            "gold_tables": [_gold_table()],
        }
        for index, pdf in enumerate(pdfs)
    ]
    monkeypatch.setattr(harness, "load_gold_manifest", lambda _path: pages)
    calls = []

    def fail_worker(*args, **kwargs):
        calls.append((args, kwargs))
        return {
            "ok": False,
            "tables": [],
            "backend_metadata": {},
            "error": "synthetic worker failure",
        }

    monkeypatch.setattr(harness, "call_worker", fail_worker)
    report = tmp_path / "report.md"
    output = tmp_path / "result.json"
    code = harness.run_gold(
        manifest,
        sys.executable,
        1,
        report,
        output,
        strategy="lines",
    )

    payload = json.loads(output.read_text(encoding="utf-8"))
    assert code != 0
    assert len(calls) == 1
    assert payload["status"] == "invalid"
    assert payload["detection_f1"] is None
    assert payload["grits_end_to_end"] is None
    assert payload["n_pages_attempted"] == 1
    rendered = report.read_text(encoding="utf-8")
    assert "Status: INVALID" in rendered
    assert "No aggregate detection or GriTS score" in rendered


def test_zero_detection_keeps_worker_level_runtime_metadata(tmp_path):
    harness = _harness()
    metadata = {
        "backend": "tatr",
        "detection_revision": "det-revision",
        "structure_revision": "structure-revision",
        "device": "cpu",
    }
    result = harness.process_doc_gold(
        tmp_path / "unused.pdf",
        "fixture",
        [_gold_table()],
        0,
        sys.executable,
        1,
        predictor=lambda _pdf, _page: {
            "ok": True,
            "tables": [],
            "backend_metadata": metadata,
            "error": None,
        },
    )

    assert result["status"] == "valid"
    assert result["n_pred"] == 0
    assert result["backend_metadata"] == metadata


def test_metric_summary_separates_end_to_end_and_matched_only():
    harness = _harness()
    docs = [
        {
            "status": "valid",
            "ox_ok": True,
            "n_gold": 2,
            "n_pred": 1,
            "n_matched": 1,
            "tables": [
                {"matched": True, "grits_top": 0.8, "grits_con": 0.6},
                {"matched": False, "grits_top": 0.0, "grits_con": 0.0},
            ],
        }
    ]

    summary = harness._gold_metric_summary(docs)
    assert summary["end_to_end"]["grits_top_mean"] == pytest.approx(0.4)
    assert summary["end_to_end"]["grits_con_mean"] == pytest.approx(0.3)
    assert summary["matched_only"]["grits_top_mean"] == pytest.approx(0.8)
    assert summary["matched_only"]["grits_con_mean"] == pytest.approx(0.6)


def test_detection_metrics_use_raw_bbox_but_grits_uses_final_bbox(tmp_path):
    harness = _harness()
    result = harness.process_doc_gold(
        tmp_path / "unused.pdf",
        "fixture",
        [_gold_table()],
        0,
        sys.executable,
        1,
        predictor=lambda _pdf, _page: {
            "ok": True,
            "backend_metadata": {"backend": "tatr"},
            "error": None,
            "tables": [
                {
                    "bbox": [0, 0, 100, 100],
                    "metadata": {"detection_bbox": [0, 0, 40, 100]},
                    "cells": [
                        {
                            "row_nums": [0],
                            "column_nums": [0],
                            "cell_text": "A",
                        }
                    ],
                }
            ],
        },
    )

    assert result["n_detection_matched"] == 0
    assert result["detection_f1"] == 0.0
    assert result["n_matched"] == 1
    assert result["tables"][0]["matched"] is True
    assert result["tables"][0]["detection_matched"] is False
    assert result["grits_top_sum"] == pytest.approx(1.0)


def test_jsonl_protocol_retries_short_writes(monkeypatch):
    harness = _harness()
    chunks: list[bytes] = []

    def short_write(_fd, data):
        chunk = bytes(data[:3])
        chunks.append(chunk)
        return len(chunk)

    monkeypatch.setattr(harness.os, "write", short_write)
    harness._write_json_line(123, {"type": "result", "ok": True})
    payload = b"".join(chunks)
    assert payload.endswith(b"\n")
    assert json.loads(payload) == {"type": "result", "ok": True}


def test_tatr_extra_encodes_supported_python_os_and_arch_matrix():
    root = Path(__file__).resolve().parents[2]
    project = tomllib.loads((root / "pyproject.toml").read_text(encoding="utf-8"))
    optional = project["project"]["optional-dependencies"]
    for extra in ("tatr", "all"):
        for requirement in optional[extra]:
            assert "python_version < '3.15'" in requirement
            assert "sys_platform == 'linux'" in requirement
            assert "platform_machine == 'aarch64'" in requirement
            assert "sys_platform == 'darwin'" in requirement
            assert "platform_machine == 'arm64'" in requirement
            assert "sys_platform == 'win32'" in requirement
            assert "platform_machine == 'AMD64'" in requirement
