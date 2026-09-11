"""Evaluation protocol and exact-span metric acceptance cases; no models."""

import importlib.util
from pathlib import Path
import sys

import pytest

GT = Path(__file__).resolve().parents[2] / "conformance" / "gt"
sys.path.insert(0, str(GT))


def load(name):
    spec = importlib.util.spec_from_file_location(name, GT / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def cell(rows=(0,), cols=(0,), text="A"):
    return {"row_nums": list(rows), "column_nums": list(cols), "cell_text": text}


def test_cell_span_identity_content_and_missing():
    metric = load("cell_alignment")
    assert metric.score_cells([cell()], [cell()])["span"]["f1"] == 1
    wrong = metric.score_cells([cell()], [cell(text="B")])
    assert wrong["span"]["f1"] == 1
    assert wrong["content"]["f1"] == 0
    assert metric.score_cells([cell()], [])["span"]["fn"] == 1


def test_invalid_prediction_is_quality_failure_without_cell_loss():
    metric = load("cell_alignment")
    result = metric.score_cells([cell()], [cell(), cell()])
    assert result["invalid_prediction"]
    assert result["span"] == {
        "tp": 0,
        "fp": 2,
        "fn": 1,
        "precision": 0,
        "recall": 0,
        "f1": 0,
    }
    with pytest.raises(ValueError, match="gold"):
        metric.score_cells([cell(), cell()], [cell()])


def test_span_merge_empty_and_unicode_normalization():
    metric = load("cell_alignment")
    assert (
        metric.score_cells([cell(cols=(0, 1))], [cell(), cell(cols=(1,))])["span"]["f1"]
        == 0
    )
    assert (
        metric.score_cells([cell(text="e\u0301  €")], [cell(text="é\n€")])["content"][
            "f1"
        ]
        == 1
    )
    assert (
        metric.score_cells([cell(text="(1)")], [cell(text="1")])["content"]["f1"] == 0
    )
    assert metric.score_cells([], [])["span"]["f1"] == 1


def test_worker_config_rejects_backend_disguise():
    harness = load("tables_diff")
    assert harness.eval_config("lines", None, None)["backend"] == "native"
    with pytest.raises(ValueError):
        harness.eval_config("lines", "onnx", {})
    with pytest.raises(ValueError):
        harness.eval_config("vision", "tableformer", {})


def test_direct_invalid_cells_are_not_repaired_or_filtered():
    harness = load("tables_diff")
    cells = [cell(), cell(rows=())]
    assert (
        len(
            harness._pred_cells_from_record({"cells": cells, "html": "<table></table>"})
        )
        == 2
    )


def gold_table():
    return {
        "pdf_table_bbox": [0, 0, 100, 100],
        "cells": [{**cell(), "json_text_content": "A"}],
    }


def prediction(cells):
    return {
        "ok": True,
        "tables": [{"bbox": [0, 0, 100, 100], "cells": cells}],
        "backend_metadata": {},
    }


def test_invalid_prediction_preserves_grits_denominator_and_extra_false_positives(
    tmp_path,
):
    harness = load("tables_diff")
    result = harness.process_doc_gold(
        tmp_path / "x.pdf",
        "x",
        [gold_table()],
        0,
        sys.executable,
        1,
        predictor=lambda *_: prediction([cell(), cell()]),
    )
    assert result["status"] == "valid"
    assert result["n_gold"] == result["n_matched"] == 1
    assert result["grits_top_sum"] == result["grits_con_sum"] == 0
    assert result["cell_alignment"]["invalid_predictions"] == 1
    assert result["cell_alignment"]["span"]["fp"] == 2
    malformed = harness.process_doc_gold(
        tmp_path / "x.pdf",
        "x",
        [gold_table()],
        0,
        sys.executable,
        1,
        predictor=lambda *_: {"ok": True, "tables": "bad"},
    )
    assert malformed["status"] == "invalid" and malformed["grits_top_sum"] is None


def write_manifest(tmp_path, review=None, missing=False):
    import json

    annotation = tmp_path / "annotation.json"
    annotation.write_text(json.dumps([gold_table()]))
    (tmp_path / "present.pdf").write_bytes(b"fake PDF: tests mock worker")
    entries = [
        {
            "document_id": "present",
            "pdf": "present.pdf",
            "annotation": "annotation.json",
        }
    ]
    if missing:
        entries.append(
            {
                "document_id": "missing",
                "pdf": "missing.pdf",
                "annotation": "annotation.json",
            }
        )
    data = {"dataset": "FinTabNet.c", "entries": entries}
    if review:
        data["review_status"] = review
    manifest = tmp_path / "manifest.json"
    manifest.write_text(json.dumps(data))
    return manifest


@pytest.mark.parametrize(
    "review,missing,status,comparable",
    [
        (None, False, "valid", True),
        ("unreviewed", False, "incomplete", False),
        (None, True, "incomplete", False),
    ],
)
def test_run_status_preserves_historical_source_track_and_draft_boundary(
    tmp_path, monkeypatch, review, missing, status, comparable
):
    import json

    harness = load("tables_diff")
    manifest = write_manifest(tmp_path, review, missing)
    monkeypatch.setattr(
        harness, "call_worker", lambda *_args, **_kwargs: prediction([cell()])
    )
    result = tmp_path / "out.json"
    code = harness.run_gold(manifest, sys.executable, 1, tmp_path / "report.md", result)
    data = json.loads(result.read_text())
    assert data["status"] == status and data["comparable"] is comparable
    assert (code == 0) is comparable
    assert data["n_pages_requested"] == (2 if missing else 1)
    assert data["n_pages_missing"] == int(missing)
    if not comparable:
        assert data["grits_end_to_end"] is None
        assert data["diagnostic_summary"]["n_gold"] == 1


def test_missing_annotation_entry_not_silently_dropped(tmp_path):
    import json

    harness = load("tables_diff")
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps({"entries": [{"pdf": "none.pdf", "annotation": "none.json"}]})
    )
    pages = harness.load_gold_manifest(manifest)
    assert len(pages) == 1 and "missing annotation" in pages[0]["input_issues"]


def test_oneshot_options_are_forwarded_and_bad_response_rejected(tmp_path, monkeypatch):
    import json
    from types import SimpleNamespace

    harness = load("tables_diff")
    commands = []

    def run(cmd, **_kwargs):
        commands.append(cmd)
        return SimpleNamespace(
            returncode=0,
            stdout=json.dumps(
                {
                    "schema": harness.WORKER_SCHEMA,
                    "type": "result",
                    "request_id": cmd[cmd.index("--request-id") + 1],
                    "ok": True,
                    "tables": [],
                    "backend_metadata": {
                        "backend": "onnx",
                        "requested": harness.eval_config(
                            "vision", "onnx", {"dpi": 123}
                        ),
                    },
                }
            ),
        )

    monkeypatch.setattr(harness.subprocess, "run", run)
    assert harness.call_worker(
        "python", "pdfspine", tmp_path / "x.pdf", 0, 1, "vision", "onnx", {"dpi": 123}
    )["ok"]
    command = commands[0]
    assert command[command.index("--backend") + 1] == "onnx"
    assert json.loads(command[command.index("--options-json") + 1]) == {"dpi": 123}
    with pytest.raises(ValueError, match="request_id"):
        harness._validate_response(
            {"ok": True, "tables": [], "request_id": "other"}, "expected"
        )


def test_onnx_metadata_uses_requested_runtime_and_hashes_without_tatr(
    tmp_path, monkeypatch
):
    from types import SimpleNamespace
    from pdfspine import _onnx, _tatr

    harness = load("tables_diff")
    layout, table = tmp_path / "layout.onnx", tmp_path / "table.onnx"
    layout.write_bytes(b"test layout identity")
    table.write_bytes(b"test structure identity")
    seen = []

    def runtime(options):
        seen.append(options)
        return SimpleNamespace(
            metadata={
                "backend": "onnx",
                "layout_model": str(layout),
                "table_model": str(table),
                "providers": ["CPUExecutionProvider"],
            }
        )

    monkeypatch.setattr(_onnx, "_get_runtime", runtime)
    monkeypatch.setattr(
        _tatr, "_get_runtime", lambda *_: pytest.fail("ONNX metadata loaded TATR")
    )
    monkeypatch.setattr(
        harness.importlib.metadata, "version", lambda _name: "test-runtime"
    )
    metadata = harness._vision_runtime_metadata(
        "vision",
        "onnx",
        {"dpi": 123, "layout_model": str(layout), "table_model": str(table)},
    )
    assert seen[0].dpi == metadata["effective_options"]["dpi"] == 123
    assert metadata["backend"] == "onnx"
    assert {entry["filename"] for entry in metadata["model_files"]} == {
        "layout.onnx",
        "table.onnx",
    }
    assert all(len(entry["sha256"]) == 64 for entry in metadata["model_files"])


def test_persistent_v2_roundtrip_options(tmp_path, monkeypatch):
    import io
    import json
    from types import SimpleNamespace

    harness = load("tables_diff")
    ready = {"schema": harness.WORKER_SCHEMA, "type": "ready"}
    response = {
        "schema": harness.WORKER_SCHEMA,
        "type": "result",
        "request_id": "x.pdf:0:1",
        "ok": True,
        "tables": [],
        "backend_metadata": {
            "backend": "onnx",
            "requested": harness.eval_config("vision", "onnx", {"dpi": 123}),
        },
    }
    writes = io.StringIO()
    process = SimpleNamespace(
        stdout=io.StringIO(json.dumps(ready) + "\n" + json.dumps(response) + "\n"),
        stdin=writes,
        poll=lambda: 0,
        wait=lambda **_: 0,
    )
    monkeypatch.setattr(harness.subprocess, "Popen", lambda *_args, **_kwargs: process)
    worker = harness.PersistentPdfspineWorker(
        "python", "vision", 1, 1, "onnx", {"dpi": 123}
    )
    assert worker.call(tmp_path / "x.pdf", 0)["ok"]
    request = json.loads(writes.getvalue())
    assert request["backend"] == "onnx" and request["options"] == {"dpi": 123}
    worker.close()


def test_global_grid_extent_is_bounded_before_grits_allocation():
    metric = load("cell_alignment")
    result = metric.score_cells([cell()], [cell(rows=(9999,)), cell(cols=(9999,))])
    assert result["invalid_prediction"]
    assert result["span"]["fp"] == 2


def test_span_sets_allow_amp_permutation_without_deduplication():
    metric = load("cell_alignment")
    source = cell(cols=(8, 7))  # Actual AMP FinTabNet.c span ordering.
    assert metric.score_cells([source], [cell(cols=(7, 8))])["span"]["f1"] == 1
    assert source["column_nums"] == [8, 7]
    assert metric.score_cells([source], [cell(cols=(7, 7, 8))])["invalid_prediction"]
    assert metric.score_cells([source], [cell(cols=(7, 9))])["invalid_prediction"]


def test_declared_review_ledger_hash_is_required_for_complete_inputs(tmp_path):
    import json

    harness = load("tables_diff")
    manifest = write_manifest(tmp_path, "unreviewed")
    data = json.loads(manifest.read_text())
    data["review_ledger"] = {"path": "ledger.json", "sha256": "0" * 64}
    manifest.write_text(json.dumps(data))
    assert (
        "missing review_ledger"
        in harness.load_gold_manifest(manifest)[0]["input_issues"]
    )
    (tmp_path / "ledger.json").write_text("{}")
    assert (
        "review_ledger SHA-256 mismatch"
        in harness.load_gold_manifest(manifest)[0]["input_issues"]
    )


def test_runtime_metadata_drift_invalidates_comparison(tmp_path, monkeypatch):
    import json

    harness = load("tables_diff")
    manifest = write_manifest(tmp_path)
    data = json.loads(manifest.read_text())
    data["entries"].append({**data["entries"][0], "document_id": "second"})
    manifest.write_text(json.dumps(data))
    sequence = iter(["one", "changed"])

    def predictor(*_args, **_kwargs):
        result = prediction([cell()])
        result["backend_metadata"] = {"model_sha": next(sequence)}
        return result

    monkeypatch.setattr(harness, "call_worker", predictor)
    result = tmp_path / "out.json"
    assert (
        harness.run_gold(manifest, sys.executable, 1, tmp_path / "out.md", result) != 0
    )
    report = json.loads(result.read_text())
    assert report["status"] == "invalid"
    assert report["grits_end_to_end"] is None
    assert "metadata changed" in report["error"]


def test_table_serializer_keeps_illegal_raw_span_as_quality_failure():
    from types import SimpleNamespace

    harness = load("tables_diff")
    table = SimpleNamespace(
        bbox=(0, 0, 10, 10),
        row_count=1,
        col_count=1,
        extract=lambda: [["A"]],
        spans=[(0.5, 0, 1, 1, (0, 0, 10, 10))],
        to_html=lambda: "<table><tr><td>A</td></tr></table>",
    )
    record = harness._table_record(table)
    assert len(record["cells"]) == 1
    assert record["cells"][0]["raw_span"][0] == 0.5
    assert load("cell_alignment").score_cells([cell()], record["cells"])[
        "invalid_prediction"
    ]


@pytest.mark.parametrize("annotation", [[1], {"wrong": "object"}])
def test_malformed_annotation_schema_produces_incomplete_report(tmp_path, annotation):
    import json

    harness = load("tables_diff")
    manifest = write_manifest(tmp_path)
    (tmp_path / "annotation.json").write_text(json.dumps(annotation))
    output = tmp_path / "output.json"
    assert (
        harness.run_gold(manifest, sys.executable, 1, tmp_path / "report.md", output)
        != 0
    )
    report = json.loads(output.read_text())
    assert report["status"] == "incomplete"
    assert report["n_pages_requested"] == 1
    assert report["n_pages_unknown_table_count"] == 1
    assert "invalid annotation" in report["missing_inputs"][0]["issues"][0]


def write_financial_manifest(tmp_path, reviewed=False):
    import hashlib
    import json

    manifest = write_manifest(tmp_path)
    data = json.loads(manifest.read_text())
    state = "reviewed" if reviewed else "unreviewed"
    annotation = [dict(gold_table(), structure_id="T0")]
    (tmp_path / "annotation.json").write_text(json.dumps(annotation))
    (tmp_path / "selection.json").write_text("{}")
    (tmp_path / "T0.html").write_text("<table><tr><td>A</td></tr></table>")
    (tmp_path / "T0.json").write_text("{}")

    def sha(name):
        return hashlib.sha256((tmp_path / name).read_bytes()).hexdigest()

    entry = data["entries"][0]
    entry.update(
        review_status=state,
        pdf_sha256=sha("present.pdf"),
        annotation_sha256=sha("annotation.json"),
    )
    table = {
        "structure_id": "T0",
        "source_table_index": 0,
        "review_status": state,
        "html": "T0.html",
        "html_sha256": sha("T0.html"),
        "cells": "T0.json",
        "cells_sha256": sha("T0.json"),
    }
    entry["tables"] = [table]
    ledger = {
        "dataset_id": "test-v1",
        "selection_sha256": sha("selection.json"),
        "tables": [
            {
                **table,
                "document_id": entry["document_id"],
                "pdf_sha256": entry["pdf_sha256"],
                "annotation_sha256": entry["annotation_sha256"],
                "human_review": {
                    "status": state,
                    "reviewer": "explicit test reviewer" if reviewed else None,
                    "reviewed_at": "2026-09-11" if reviewed else None,
                },
            }
        ],
    }
    (tmp_path / "ledger.json").write_text(json.dumps(ledger))
    data.update(
        schema="pdfspine.financial-eval-drafts.v1",
        dataset_id="test-v1",
        review_status=state,
        selection={"path": "selection.json", "sha256": sha("selection.json")},
        review_ledger={"path": "ledger.json", "sha256": sha("ledger.json")},
    )
    manifest.write_text(json.dumps(data))
    return manifest


@pytest.mark.parametrize("reviewed", [False, True])
def test_financial_schema_keeps_execution_and_human_review_separate(
    tmp_path, monkeypatch, reviewed
):
    import json

    harness = load("tables_diff")
    manifest = write_financial_manifest(tmp_path, reviewed)
    monkeypatch.setattr(
        harness, "call_worker", lambda *_args, **_kwargs: prediction([cell()])
    )
    output = tmp_path / "output.json"
    code = harness.run_gold(manifest, sys.executable, 1, tmp_path / "report.md", output)
    report = json.loads(output.read_text())
    assert report["execution_status"] == "success"
    assert report["status"] == ("valid" if reviewed else "incomplete")
    assert report["comparable"] is reviewed
    assert (code == 0) is reviewed


def test_unknown_schema_and_financial_missing_hash_never_auto_legacy(tmp_path):
    import json

    harness = load("tables_diff")
    manifest = write_financial_manifest(tmp_path)
    data = json.loads(manifest.read_text())
    del data["entries"][0]["pdf_sha256"]
    manifest.write_text(json.dumps(data))
    assert any(
        "pdf_sha256" in issue
        for issue in harness.load_gold_manifest(manifest)[0]["input_issues"]
    )
    data["schema"] = "unknown-v9"
    manifest.write_text(json.dumps(data))
    assert (
        "unknown evaluation manifest schema/track"
        in harness.load_gold_manifest(manifest)[0]["input_issues"]
    )


def test_worker_response_backend_and_options_must_match_request():
    harness = load("tables_diff")
    config = harness.eval_config("vision", "onnx", {"dpi": 123})
    for metadata in [
        {"backend": "tatr", "requested": config},
        {"backend": "onnx", "requested": harness.eval_config("vision", "onnx", {})},
    ]:
        with pytest.raises(ValueError, match="identity"):
            harness._validate_response(
                {"ok": True, "tables": [], "backend_metadata": metadata},
                expected_config=config,
            )


def test_lazy_table_provider_added_then_changed_is_detected(tmp_path, monkeypatch):
    import json

    harness = load("tables_diff")
    manifest = write_manifest(tmp_path)
    data = json.loads(manifest.read_text())
    data["entries"] *= 3
    manifest.write_text(json.dumps(data))
    providers = iter(
        [
            {"layout": ["CPU"]},
            {"layout": ["CPU"], "table": ["CPU"]},
            {"layout": ["CPU"], "table": ["CUDA"]},
        ]
    )

    def worker(*_args, **_kwargs):
        result = prediction([cell()])
        result["backend_metadata"] = {
            "backend": "onnx",
            "session_providers": next(providers),
        }
        return result

    monkeypatch.setattr(harness, "call_worker", worker)
    output = tmp_path / "output.json"
    harness.run_gold(manifest, sys.executable, 1, tmp_path / "report.md", output)
    report = json.loads(output.read_text())
    assert report["status"] == "invalid" and "providers changed" in report["error"]
    assert report["n_pages_attempted"] == 3


@pytest.mark.parametrize("kind", ["tables-null", "id-list"])
def test_bad_financial_reference_types_keep_structured_incomplete(tmp_path, kind):
    import json

    harness = load("tables_diff")
    manifest = write_financial_manifest(tmp_path)
    data = json.loads(manifest.read_text())
    if kind == "tables-null":
        data["entries"][0]["tables"] = None
    else:
        data["entries"][0]["tables"][0]["structure_id"] = []
    manifest.write_text(json.dumps(data))
    assert harness.load_gold_manifest(manifest)[0]["input_issues"]
    output = tmp_path / "out.json"
    harness.run_gold(manifest, sys.executable, 1, tmp_path / "out.md", output)
    assert json.loads(output.read_text())["status"] == "incomplete"


@pytest.mark.parametrize(
    "field,value",
    [
        ("pdf", []),
        ("annotation", 42),
        ("pdf_page_index", "oops"),
        ("review_status", None),
    ],
)
def test_financial_entry_scalars_are_validated_before_use(tmp_path, field, value):
    import json

    harness = load("tables_diff")
    manifest = write_financial_manifest(tmp_path)
    data = json.loads(manifest.read_text())
    data["entries"][0][field] = value
    manifest.write_text(json.dumps(data))
    page = harness.load_gold_manifest(manifest)[0]
    assert page["input_issues"]
    assert isinstance(page["review_status"], str)
    output = tmp_path / "out.json"
    harness.run_gold(manifest, sys.executable, 1, tmp_path / "out.md", output)
    assert json.loads(output.read_text())["status"] == "incomplete"


def test_financial_manifest_loader_contract(tmp_path):
    """The same contract can check the frozen data candidate without inference."""
    import json
    import os

    harness = load("tables_diff")
    supplied = os.environ.get("PDFSPINE_EVAL_TEST_MANIFEST")
    manifest = Path(supplied) if supplied else write_financial_manifest(tmp_path)
    declared = json.loads(manifest.read_text())
    pages = harness.load_gold_manifest(manifest)
    assert len(pages) == len(declared["entries"])
    assert all(not page["input_issues"] for page in pages)
    assert all(page["review_status"] == "unreviewed" for page in pages)
    assert not any(page["ledger_reviews_complete"] for page in pages)
    assert sum(len(page["gold_tables"]) for page in pages) == sum(
        len(entry["tables"]) for entry in declared["entries"]
    )
