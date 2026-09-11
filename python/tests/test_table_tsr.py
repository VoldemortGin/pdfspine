"""Detector-free structure evaluation: real crop path, fake neural models."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pdfspine
import pytest
from pdfspine import _onnx, _tatr


def harness():
    path = Path(__file__).resolve().parents[2] / "conformance/gt/tables_diff.py"
    spec = importlib.util.spec_from_file_location("tsr_harness", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fixture_module():
    spec = importlib.util.spec_from_file_location(
        "tsr_fixtures", Path(__file__).with_name("test_tatr_tables.py")
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_gold_mode_is_explicit():
    config = harness().eval_config("vision", "onnx", {}, mode="gold-crop-tsr")
    assert config["mode"] == "gold-crop-tsr"
    assert config["options"]["ocr_if_no_text"] is False


@pytest.mark.parametrize("backend", [_tatr, _onnx])
def test_detector_trap_real_crop(backend):
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=200)
    page.insert_text((25, 45), "NATIVE", fontsize=10)

    class Runtime:
        metadata = {"backend": "fake"}

        def detect(self, *args):
            raise AssertionError("detector executed")

        detect_layout = detect

        def recognize(self, image, threshold):
            assert image.size == (100, 60)
            return [
                {"label": "table", "score": 0.9, "bbox": [0, 0, 100, 60]},
                {"label": "table row", "score": 0.9, "bbox": [0, 0, 100, 60]},
                {"label": "table column", "score": 0.9, "bbox": [0, 0, 100, 60]},
            ]

        def recognize_table(self, image, options):
            assert image.size == (100, 60)
            return ["<tr>", "<td></td>", "</tr>"], [[0, 0, 100, 60]], [0.9] * 3

    result = backend._recognize_gold_crop(
        page, (20, 20, 120, 80), options={"dpi": 72}, _runtime=Runtime()
    )
    assert len(result.cells) == 1
    assert result.cells[0]["cell_text"] == "NATIVE"
    assert result.cells[0]["bbox"] == pytest.approx((20, 20, 120, 80))
    assert result.metadata["word_policy"] == "native-only"
    assert "detection_bbox" not in result.metadata


@pytest.mark.parametrize("rotation", [0, 90, 180, 270])
def test_nonzero_crop_rotation_native_coordinates(rotation, monkeypatch):
    # Existing generated PDF contains CropBox [100 80 712 872] and known A1.
    _cropped_rotated_table_pdf = fixture_module()._cropped_rotated_table_pdf
    doc = pdfspine.open(stream=_cropped_rotated_table_pdf(rotation))
    page = doc[0]
    normalized = (0.0, 150.0, 310.0, 235.0)
    display = _tatr._normalized_box_to_display(normalized, 612, 792, rotation)

    class Empty:
        def detect(self, *args):
            raise AssertionError("must bypass detector")

        def recognize(self, image, threshold):
            assert image.size == (310, 85)
            return []

    result = _tatr._recognize_gold_crop(
        page, display, options={"dpi": 72}, _runtime=Empty()
    )
    assert result.quality == "empty"
    assert result.metadata["actual_pixel_bbox"] == [0, 150, 310, 235]
    assert sorted(t["text"] for t in result.metadata["native_tokens"]) == [
        "A1",
        "A2",
        "B1",
        "B2",
        "C1",
        "C2",
    ]
    a = next(t["bbox"] for t in result.metadata["native_tokens"] if t["text"] == "A1")
    assert 9 < a[0] < 11 and 25 < a[1] < 40


def test_fractional_crop_and_overlap_boundary(monkeypatch):
    from PIL import Image

    tokens = [
        {"bbox": [0, 0, 20, 10], "text": "half"},
        {"bbox": [0, 0, 19, 10], "text": "less"},
        {"bbox": [0, 0, 21, 10], "text": "more"},
    ]
    rendered = _tatr._RenderedPage(
        Image.new("RGB", (100, 100)), tokens, (0, 0, 100, 100), 1, 1, "pdfspine-native"
    )
    monkeypatch.setattr(_tatr, "_render_page", lambda *a: rendered)
    _, crop, meta = _tatr._gold_crop(None, (10.3, 0, 30.1, 20.1), None, 0)
    assert crop.bbox == (10, 0, 31, 21)
    assert crop.image.size == (21, 21)
    assert [t["text"] for t in crop.tokens] == ["half", "more"]
    assert crop.tokens[0]["bbox"] == [-10, 0, 10, 10]
    _, padded, _ = _tatr._gold_crop(None, (1.3, 1.2, 30.1, 20.1), None, 3)
    assert padded.bbox == (0, 0, 34, 24)


def test_final_scalar_semantics_and_invalid_spans():
    import numpy as np

    def cell(rows):
        return {
            "row_nums": rows,
            "column_nums": [np.int64(0)],
            "bbox": [np.float64(0), 0, 10, 10],
            "cell_text": "X",
        }

    good, reasons = _tatr._final_cells([cell([np.int64(1), np.float64(0)])])
    assert not reasons and good[0]["row_nums"] == [1, 0]
    for rows in ([0, 0], [0, 2], [np.float64(1.2)]):
        raw, reasons = _tatr._final_cells([cell(rows)])
        assert len(raw) == 1 and reasons
    raw, reasons = _tatr._final_cells([cell([0]), cell([0])])
    assert len(raw) == 2 and "overlapping-cells" in reasons
    scored = harness()._score_crop(
        [{"row_nums": [0], "column_nums": [0], "cell_text": "X"}],
        {"cells": raw, "quality": "invalid", "quality_reasons": reasons},
    )
    assert scored["cell_alignment"]["span"]["fp"] == 2
    assert scored["cell_alignment"]["span"]["tp"] == 0
    assert scored["grits_top"] == 0


def test_postprocess_exception_is_not_empty_prediction(monkeypatch):
    _structure_objects, _tokens = (
        fixture_module()._structure_objects,
        fixture_module()._tokens,
    )
    from PIL import Image

    rendered = _tatr._RenderedPage(
        Image.new("RGB", (200, 100)), [], (0, 0, 200, 100), 1, 1, "pdfspine-native"
    )
    crop = _tatr._TableCrop(
        rendered.image, _tokens(), (0, 0, 200, 100), False, (200, 100)
    )

    def broken(*a):
        raise KeyError("assembler-bug")

    monkeypatch.setattr(_tatr._postprocess, "objects_to_table_structures", broken)
    assert (
        _tatr._table_from_structure(_structure_objects(), crop, rendered, 0.9, {})
        is None
    )
    with pytest.raises(KeyError, match="assembler-bug"):
        _tatr._table_from_structure(
            _structure_objects(), crop, rendered, 0.9, {}, _strict_results=[]
        )


def test_no_ocr_and_explicit_conflicting_options():
    class Page:
        def get_text(self, *a, **k):
            return []

        def get_textpage_ocr(self, *a, **k):
            raise AssertionError("OCR called")

    assert _tatr._native_words(Page(), _tatr.TatrOptions(ocr_if_no_text=False)) == (
        [],
        "pdfspine-native",
    )
    for backend in (_tatr, _onnx):
        with pytest.raises(ValueError, match="OCR"):
            backend._recognize_gold_crop(
                None, (0, 0, 10, 10), options={"ocr_if_no_text": True}
            )


def test_onnx_table_only_session_and_worker_provenance(monkeypatch, tmp_path):
    import importlib.metadata
    import sys
    from types import SimpleNamespace
    import numpy as np

    table_path = tmp_path / "table.onnx"
    table_path.write_bytes(b"fake model file, never used by real ORT")
    layout_path = tmp_path / "ABSENT-layout.onnx"
    created = []

    class Session:
        def __init__(self, path, **kwargs):
            created.append(path)
            assert path == str(table_path)

        def get_modelmeta(self):
            return SimpleNamespace(custom_metadata_map={})

        def get_inputs(self):
            return [SimpleNamespace(name="x")]

        def get_providers(self):
            return ["CPUExecutionProvider"]

        def run(self, *args):
            dictionary = [_onnx._SOS, *_onnx.SLANET_STRUCTURE_DICT, _onnx._EOS]
            probs = np.zeros((1, 4, len(dictionary)))
            for i, token in enumerate(["<tr>", "<td></td>", "</tr>", _onnx._EOS]):
                probs[0, i, dictionary.index(token)] = 0.9
            boxes = np.tile([0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0], (1, 4, 1))
            return [boxes, probs]

    monkeypatch.setitem(
        sys.modules,
        "onnxruntime",
        SimpleNamespace(
            get_available_providers=lambda: ["CPUExecutionProvider"],
            SessionOptions=lambda: SimpleNamespace(),
            InferenceSession=Session,
        ),
    )
    real_version = importlib.metadata.version
    monkeypatch.setattr(
        importlib.metadata,
        "version",
        lambda n: "fake-ORT" if n == "onnxruntime" else real_version(n),
    )
    doc = pdfspine.open()
    doc.new_page(width=100, height=100).insert_text((10, 40), "WORD", fontsize=10)
    pdf = tmp_path / "one.pdf"
    doc.save(pdf)
    h = harness()
    request = {
        "table_id": "one",
        "bbox": [0, 0, 100, 100],
        "coordinate_space": "page-display",
        "padding": 0,
    }
    opts = {"dpi": 72, "layout_model": str(layout_path), "table_model": str(table_path)}
    try:
        records, metadata = h._worker_pdfspine(
            str(pdf),
            0,
            "vision",
            "onnx",
            opts,
            mode="gold-crop-tsr",
            crop_request=request,
        )
        assert records[0]["cells"][0]["cell_text"] == "WORD"
        assert created == [str(table_path)]
        assert [v["role"] for v in metadata["model_files"]] == ["table"]
        assert metadata["session_providers"] == {"table": ["CPUExecutionProvider"]}
        assert metadata["executed_model_roles"] == ["table"]
        runtime = _onnx._get_runtime(_onnx.OnnxOptions.from_mapping(opts))
        runtime._sessions["layout"] = runtime._sessions[
            "table"
        ]  # previously used session
        reused = h._vision_runtime_metadata(
            "vision", "onnx", opts, mode="gold-crop-tsr", used_runtime=runtime
        )
        assert reused["loaded_model_roles"] == ["layout", "table"]
        assert reused["executed_model_roles"] == ["table"]
        assert [item["role"] for item in reused["model_files"]] == ["table"]
        h._validate_response(
            {"ok": True, "tables": records, "backend_metadata": metadata},
            expected_config=h.eval_config("vision", "onnx", opts, mode="gold-crop-tsr"),
            expected_crop=request,
        )
    finally:
        _onnx.clear_model_cache()


def test_strict_final_cells_survive_default_record_filter(monkeypatch):
    from PIL import Image

    f = fixture_module()
    rendered = _tatr._RenderedPage(
        Image.new("RGB", (200, 100)), [], (0, 0, 200, 100), 1, 1, "pdfspine-native"
    )
    crop = _tatr._TableCrop(rendered.image, [], (0, 0, 200, 100), False, (200, 100))
    cell = {
        "row_nums": [0],
        "column_nums": [0],
        "bbox": [0, 0, 100, 50],
        "cell_text": "A",
    }
    monkeypatch.setattr(
        _tatr._postprocess,
        "table_structure_to_cells",
        lambda *a: ([dict(cell), dict(cell)], 0.9),
    )
    default = _tatr._table_from_structure(
        f._structure_objects(), crop, rendered, 0.8, {}
    )
    strict = []
    _tatr._table_from_structure(
        f._structure_objects(), crop, rendered, 0.8, {}, _strict_results=strict
    )
    assert len(default.spans) == 1
    assert len(strict[0].cells) == 2 and strict[0].quality == "invalid"


def test_onnx_low_score_and_missing_final_geometry(monkeypatch):
    class Runtime:
        def recognize_table(self, image, opts):
            return ["<tr>", "<td></td>", "</tr>"], [], [0.1] * 3

    doc = pdfspine.open()
    page = doc.new_page(width=100, height=100)
    low = _onnx._recognize_gold_crop(
        page, (0, 0, 50, 50), options={"table_min_score": 0.5}, _runtime=Runtime()
    )
    assert low.quality == "empty" and not low.cells
    bad = _onnx._recognize_gold_crop(page, (0, 0, 50, 50), _runtime=Runtime())
    assert bad.quality == "invalid" and len(bad.cells) == 1


def test_special_tokens_are_not_bad_cells_but_invalid_final_span_is():
    probabilities = [[0, 0.9, 0, 0], [0, 0, 0.9, 0], [0, 0, 0, 0.9], [0, 0.9, 0, 0]]
    tokens, boxes, scores = _onnx._decode_structure(
        probabilities, [[0, 0, 1, 0, 1, 1, 0, 1]] * 4, ["<tr>", "<td></td>"], 10
    )
    assert tokens == ["<tr>", "<td></td>"]  # EOS stops before padded next token.
    good = _onnx._structure_to_cells(tokens, boxes, scores, _strict=True)
    assert len(good) == 1 and not _tatr._final_cells(good)[1]
    invalid_tokens = ["<tr>", "<td", ' colspan="0"', ">", "</td>", "</tr>"]
    default = _onnx._structure_to_cells(invalid_tokens, [[0, 0, 10, 10]])
    strict = _onnx._structure_to_cells(invalid_tokens, [[0, 0, 10, 10]], _strict=True)
    assert default[0]["column_nums"] == [0]
    assert len(strict) == 1 and _tatr._final_cells(strict)[1]


def test_tsr_aggregate_keeps_unreviewed_and_missing_denominators(monkeypatch, tmp_path):
    import json

    h = harness()
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"fake; worker injected")
    gold = {
        "structure_id": "first",
        "pdf_table_bbox": [0, 0, 10, 10],
        "cells": [{"row_nums": [0], "column_nums": [0], "json_text_content": "G"}],
    }
    page = {
        "document_id": "one",
        "pdf": pdf,
        "page_index": 0,
        "gold_tables": [gold],
        "review_status": "unreviewed",
    }
    monkeypatch.setattr(
        h,
        "load_gold_manifest",
        lambda p: [
            page,
            {**page, "document_id": "missing", "pdf": tmp_path / "absent"},
        ],
    )

    class Worker:
        def __init__(self, *a, **kw):
            pass

        def close(self):
            pass

        def call(self, pdf, page, request):
            return {
                "ok": True,
                "tables": [
                    {
                        "table_id": request["table_id"],
                        "crop_request": request,
                        "cells": [],
                        "quality": "empty",
                    }
                ],
                "backend_metadata": {
                    "backend": "onnx",
                    "requested": h.eval_config(
                        "vision", "onnx", {}, mode="gold-crop-tsr"
                    ),
                },
            }

    monkeypatch.setattr(h, "PersistentPdfspineWorker", Worker)
    report, output = tmp_path / "report.md", tmp_path / "out.json"
    assert (
        h.run_gold(
            tmp_path / "manifest",
            "python",
            1,
            report,
            output,
            "vision",
            backend="onnx",
            mode="gold-crop-tsr",
        )
        == 3
    )
    result = json.loads(output.read_text())
    assert result["status"] == "incomplete" and not result["comparable"]
    assert result["n_tables_requested"] == 2 and result["n_tables_evaluated"] == 1
    assert result["official_aggregate"] is None
    assert result["diagnostic_partial_aggregate"]["cell_alignment"]["span"]["fn"] == 1
    assert "detection_f1" not in result


def test_tsr_worker_failure_is_invalid_not_zero_score(monkeypatch, tmp_path):
    import json

    h = harness()
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"x")
    gold = {
        "structure_id": "one",
        "pdf_table_bbox": [0, 0, 10, 10],
        "cells": [{"row_nums": [0], "column_nums": [0], "json_text_content": "X"}],
    }
    monkeypatch.setattr(
        h,
        "load_gold_manifest",
        lambda p: [
            {
                "document_id": "x",
                "pdf": pdf,
                "page_index": 0,
                "gold_tables": [gold],
                "review_status": "source-annotations",
            }
        ],
    )

    class Worker:
        def __init__(self, *a, **kw):
            pass

        def close(self):
            pass

        def call(self, *a):
            return {"ok": False, "tables": [], "error": "KeyError: assembler bug"}

    monkeypatch.setattr(h, "PersistentPdfspineWorker", Worker)
    assert (
        h.run_gold(
            tmp_path / "m",
            "python",
            1,
            tmp_path / "r.md",
            tmp_path / "r.json",
            "vision",
            backend="tatr",
            mode="gold-crop-tsr",
        )
        == 2
    )
    data = json.loads((tmp_path / "r.json").read_text())
    assert data["status"] == "invalid" and data["official_aggregate"] is None
    assert data["n_tables_evaluated"] == 0 and data["n_tables_requested"] == 1


def test_one_shot_and_persistent_forward_crop_identity(monkeypatch, tmp_path):
    import io
    import json
    from types import SimpleNamespace

    h = harness()
    crop = {
        "table_id": "tbl",
        "bbox": [1, 2, 10, 20],
        "coordinate_space": "page-display",
        "padding": 0,
    }
    config = h.eval_config("vision", "onnx", {}, mode="gold-crop-tsr")

    def response(rid):
        return {
            "schema": h.WORKER_SCHEMA,
            "type": "result",
            "request_id": rid,
            "ok": True,
            "tables": [
                {
                    "table_id": "tbl",
                    "crop_request": crop,
                    "cells": [],
                    "quality": "empty",
                }
            ],
            "backend_metadata": {"backend": "onnx", "requested": config},
        }

    commands = []

    def run(cmd, **kwargs):
        commands.append(cmd)
        return SimpleNamespace(
            returncode=0, stdout=json.dumps(response("x.pdf:0")), stderr=""
        )

    monkeypatch.setattr(h.subprocess, "run", run)
    assert h.call_worker(
        "python",
        "pdfspine",
        tmp_path / "x.pdf",
        0,
        1,
        "vision",
        "onnx",
        {},
        eval_mode="gold-crop-tsr",
        crop_request=crop,
    )["ok"]
    assert json.loads(commands[0][commands[0].index("--crop-request-json") + 1]) == crop
    writes = io.StringIO()
    process = SimpleNamespace(
        stdout=io.StringIO(
            json.dumps({"schema": h.WORKER_SCHEMA, "type": "ready"})
            + "\n"
            + json.dumps(response("x.pdf:0:1"))
            + "\n"
        ),
        stdin=writes,
        poll=lambda: 0,
        wait=lambda **kw: 0,
    )
    monkeypatch.setattr(h.subprocess, "Popen", lambda *a, **kw: process)
    worker = h.PersistentPdfspineWorker(
        "python", "vision", 1, 1, "onnx", {}, mode="gold-crop-tsr"
    )
    assert worker.call(tmp_path / "x.pdf", 0, crop)["ok"]
    request = json.loads(writes.getvalue())
    assert request["mode"] == "gold-crop-tsr" and request["crop_request"] == crop
    worker.close()
    with pytest.raises(ValueError, match="identity"):
        h._validate_response(
            response("x"),
            expected_config=config,
            expected_crop={**crop, "table_id": "WRONG"},
        )


def test_strict_inference_exception_keeps_original_cause():
    class Bad:
        def recognize(self, *a):
            raise ArithmeticError("broken model")

    doc = pdfspine.open()
    page = doc.new_page(width=100, height=100)
    with pytest.raises(_tatr._TsrPipelineError, match="recognition") as caught:
        _tatr._recognize_gold_crop(page, (0, 0, 20, 20), _runtime=Bad())
    assert isinstance(caught.value.__cause__, ArithmeticError)


def test_huge_final_span_has_bounded_strict_allocation():
    cells = _onnx._structure_to_cells(
        ["<tr>", "<td", ' rowspan="10000"', ' colspan="10000"', ">", "</td>", "</tr>"],
        [[0, 0, 10, 10]],
        _strict=True,
    )
    assert len(cells) == 1 and cells[0]["row_nums"] == []
    assert _tatr._final_cells(cells)[1]


def test_invalid_gold_is_incomplete_before_worker(monkeypatch, tmp_path):
    import json

    h = harness()
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"x")
    cell = {"row_nums": [0], "column_nums": [0], "json_text_content": "X"}
    gold = {
        "structure_id": "one",
        "pdf_table_bbox": [0, 0, 10, 10],
        "cells": [cell, cell],
    }
    monkeypatch.setattr(
        h,
        "load_gold_manifest",
        lambda p: [
            {
                "document_id": "x",
                "pdf": pdf,
                "page_index": 0,
                "gold_tables": [gold],
                "review_status": "source-annotations",
            }
        ],
    )

    def forbidden(*a, **kw):
        raise AssertionError("worker must not start")

    monkeypatch.setattr(h, "PersistentPdfspineWorker", forbidden)
    assert (
        h.run_gold(
            tmp_path / "m",
            "python",
            1,
            tmp_path / "r.md",
            tmp_path / "r.json",
            "vision",
            backend="tatr",
            mode="gold-crop-tsr",
        )
        == 3
    )
    data = json.loads((tmp_path / "r.json").read_text())
    assert data["status"] == "incomplete" and data["n_tables_requested"] == 1
    assert data["n_tables_attempted"] == 0 and data["error"] is None


def test_strict_budget_matches_harness_and_preserves_raw_count():
    import cell_alignment

    assert _tatr._STRICT_MAX_GRID_SLOTS == cell_alignment.MAX_GRID_SLOTS
    cells = [
        {"row_nums": [9999], "column_nums": [0], "bbox": [0, 0, 1, 1]},
        {"row_nums": [0], "column_nums": [9999], "bbox": [0, 0, 1, 1]},
    ]
    raw, reasons = _tatr._final_cells(cells)
    assert len(raw) == 2 and "grid-slot-limit" in reasons
