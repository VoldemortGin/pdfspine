"""Evaluator-only TableFormer contracts; no neural runtime required."""

import importlib
import sys
from pathlib import Path

import pdfspine
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "conformance/gt"))


def adapter():
    return importlib.import_module("tableformer_adapter")


def cell(rows=(0, 1), cols=(0, 1), bbox=(0, 0, 100, 60)):
    return {
        "start_row_offset_idx": rows[0],
        "end_row_offset_idx": rows[1],
        "start_col_offset_idx": cols[0],
        "end_col_offset_idx": cols[1],
        "row_span": rows[1] - rows[0],
        "col_span": cols[1] - cols[0],
        "bbox": dict(zip(("l", "t", "r", "b"), bbox)),
    }


def test_explicit_backend_no_e2e():
    from tables_diff import eval_config

    with pytest.raises(ValueError, match="gold-crop"):
        eval_config("vision", "tableformer", {})
    assert (
        eval_config("vision", "tableformer", {}, mode="gold-crop-tsr")["backend"]
        == "tableformer"
    )


def test_real_native_crop_and_owned_boxes():
    a = adapter()
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=200)
    page.insert_text((25, 45), "NATIVE", fontsize=10)

    class Predictor:
        def multi_table_predict(self, page, boxes, **kwargs):
            assert kwargs == dict(
                do_matching=False,
                sort_row_col_indexes=False,
                correct_overlapping_cells=False,
            )
            assert page["image"].shape == (60, 100, 3)
            assert page["tokens"][0]["text"] == "NATIVE"
            assert isinstance(page["tokens"][0]["bbox"], dict)
            assert boxes == [[0, 0, 100, 60]]
            boxes[0][0] = 999
            return [
                {
                    "tf_responses": [cell()],
                    "predict_details": {"prediction": {"rs_seq": ["fcel", "nl"]}},
                }
            ]

    result = a.recognize(
        page, [20, 20, 120, 80], options={"dpi": 72}, _predictor=Predictor()
    )
    assert result.cells[0]["cell_text"] == "NATIVE"
    assert result.cells[0]["bbox"] == [20, 20, 120, 80]
    assert result.metadata["actual_pixel_bbox"] == [20, 20, 120, 80]
    assert result.metadata["tableformer_geometry"]["input_box"] == [0, 0, 100, 60]


@pytest.mark.parametrize(
    "rows,cols", [((0, 100000000), (0, 100000000)), ((2, 1), (0, 1)), ((-1, 1), (0, 1))]
)
def test_bad_span_preserves_raw_count(rows, cols):
    cells, reasons = adapter().convert_cells([cell(rows, cols)])
    assert len(cells) == 1 and reasons
    assert not cells[0]["row_nums"] or not cells[0]["column_nums"]


def test_empty_structural_cells_not_dropped():
    cells, reasons = adapter().convert_cells(
        [cell(), cell((1, 3), (0, 1), (0, 60, 100, 120))]
    )
    assert not reasons
    assert cells[1]["row_nums"] == [1, 2]
    assert cells[1]["cell_text"] == ""


def test_options_reject_hidden_alternatives():
    for options in (
        {"do_matching": True},
        {"device": "mps"},
        {"ocr_if_no_text": True},
        {"variant": "alias"},
    ):
        with pytest.raises((TypeError, ValueError)):
            adapter().options(options)


def test_model_directory_rejects_ambiguous_and_hash_mismatch(tmp_path):
    a = adapter()
    (tmp_path / "tableformer_fast.safetensors").write_bytes(b"bad")
    (tmp_path / "tm_config.json").write_text("{}")
    config = a.options({"model_dir": str(tmp_path)})
    with pytest.raises(ValueError, match="SHA"):
        a.model_files(config)
    (tmp_path / "tableformer_accurate.safetensors").write_bytes(b"bad")
    with pytest.raises(ValueError, match="single"):
        a.model_files(config)


def test_overlapping_and_inconsistent_spans_are_quality_failures():
    a = adapter()
    cells, reasons = a.convert_cells([cell(), cell()])
    assert len(cells) == 2 and "overlapping-cells" in reasons
    c = cell()
    c["row_span"] = 2
    cells, reasons = a.convert_cells([c])
    assert reasons and len(cells) == 1


def test_word_assignment_reuses_existing_ties_and_nearest():
    from pdfspine._onnx import _assign_words

    cells = [{"bbox": [0, 0, 10, 10]}, {"bbox": [20, 0, 30, 10]}]
    tokens = [
        {"bbox": [13, 3, 17, 7], "text": "nearest-tie"},
        {"bbox": [1, 1, 9, 9], "text": "inside"},
    ]
    assert _assign_words(cells, tokens) == 1
    assert "nearest-tie" in cells[0]["cell_text"] and cells[1]["cell_text"] == ""


@pytest.mark.parametrize("rotation", [0, 90, 180, 270])
def test_crop_geometry_roundtrip_and_raw_scalar_serialization(rotation):
    import json
    import numpy as np
    from pdfspine import _tatr

    doc = pdfspine.open()
    page = doc.new_page(width=200, height=240)
    page.set_cropbox(pdfspine.Rect(10, 20, 190, 220))
    page.set_rotation(rotation)

    class Predictor:
        def multi_table_predict(self, page, boxes, **kwargs):
            w, h = page["width"], page["height"]
            c = cell(bbox=(0, 0, w, h))
            c["row_span"] = np.int64(1)
            return [{"tf_responses": [c], "predict_details": {}}]

    result = adapter().recognize(
        page, [10.25, 20.25, 70.75, 90.75], options={"dpi": 144}, _predictor=Predictor()
    )
    assert result.cells[0]["bbox"] == result.metadata["recognition_crop_bbox"]
    json.dumps(result.metadata, allow_nan=False)

    class Broken:
        def multi_table_predict(self, *args, **kwargs):
            raise RuntimeError("actual failure")

    with pytest.raises(_tatr._TsrPipelineError, match="recognition.*actual failure"):
        adapter().recognize(
            page, [10, 20, 70, 90], options={"dpi": 72}, _predictor=Broken()
        )


def test_worker_identity_is_stable_across_crops(monkeypatch, tmp_path):
    import tables_diff as h
    from pdfspine import _tatr

    doc = pdfspine.open()
    doc.new_page(width=200, height=200)
    path = tmp_path / "source.pdf"
    doc.save(path)

    def recognize(page, bbox, **kwargs):
        return _tatr._CropRecognition(
            [],
            {
                "native_tokens": [{"text": str(bbox)}],
                "actual_pixel_bbox": bbox,
                "effective_options": {"variant": "fast"},
                "track_identity": "tableformer-fast-raw-structure",
                "loaded_model_roles": ["table"],
                "executed_model_roles": ["table"],
            },
            "empty",
        )

    monkeypatch.setattr(adapter(), "recognize", recognize)
    outputs = []
    for x in (10, 20):
        request = {
            "table_id": str(x),
            "bbox": [x, 10, 100, 100],
            "coordinate_space": "page-display",
            "padding": 0,
        }
        records, metadata = h._worker_pdfspine(
            str(path),
            0,
            "vision",
            "tableformer",
            {},
            mode="gold-crop-tsr",
            crop_request=request,
        )
        assert records[0]["metadata"]["actual_pixel_bbox"] == request["bbox"]
        outputs.append(metadata)
    assert outputs[0] == outputs[1]
    assert outputs[0]["requested"] == h.eval_config(
        "vision", "tableformer", {}, mode="gold-crop-tsr"
    )


def test_bgr_input_and_empty_prediction():
    doc = pdfspine.open()
    page = doc.new_page(width=100, height=100)
    page.draw_rect(pdfspine.Rect(0, 0, 100, 100), color=None, fill=(1, 0, 0))

    class Predictor:
        def multi_table_predict(self, page, boxes, **kwargs):
            assert list(page["image"][10, 10]) == [0, 0, 255]
            return [{"tf_responses": [], "predict_details": {}}]

    result = adapter().recognize(
        page, [0, 0, 100, 100], options={"dpi": 72}, _predictor=Predictor()
    )
    assert result.quality == "empty" and result.cells == []


def test_hf_snapshot_symlink_directory_is_not_replaced_by_blob_parent(
    monkeypatch, tmp_path
):
    import types

    a = adapter()
    snapshot = tmp_path / "snapshot"
    snapshot.mkdir()
    blobs = tmp_path / "blobs"
    blobs.mkdir()
    config_file = blobs / "config-hash"
    config_file.write_text('{"model":{}}')
    weights = blobs / "weight-hash"
    weights.write_bytes(b"weights")
    (snapshot / "tableformer_fast.safetensors").symlink_to(weights)
    (snapshot / "tm_config.json").symlink_to(config_file)
    monkeypatch.setattr(
        a, "model_files", lambda _: [a.identity(weights), a.identity(config_file)]
    )
    monkeypatch.setattr(a.importlib.metadata, "version", lambda _: "4.0.2")

    class Predictor:
        def __init__(self, config, **kwargs):
            assert Path(config["model"]["save_dir"]) == snapshot
            assert list(
                Path(config["model"]["save_dir"]).glob("tableformer_*.safetensors")
            )

    monkeypatch.setitem(
        sys.modules,
        "docling_ibm_models.tableformer.data_management.tf_predictor",
        types.SimpleNamespace(TFPredictor=Predictor),
    )
    import json

    a._load.cache_clear()
    a._load(json.dumps(a.options({"model_dir": str(snapshot)})))
    a._load.cache_clear()
