"""FinTabNet source extent proof, independent of model execution."""

import importlib.util
from pathlib import Path
from types import SimpleNamespace

import pdfspine
import pytest
from pdfspine import _tatr


def harness():
    path = Path(__file__).resolve().parents[2] / "conformance/gt/tables_diff.py"
    spec = importlib.util.spec_from_file_location("crop_proof", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def request():
    return {
        "table_id": "ADP_2008_page_35_0",
        "coordinate_space": "fintabnet-page",
        "bbox": [18, 63.61209487915039, 593.96142578125, 740.3187866210938],
        "source_page_bbox": [0, 0, 612, 918],
        "padding": 0,
    }


@pytest.mark.parametrize(
    "crop,extent",
    [
        ((0, 72, 612, 990), (612, 918)),
        ((9, 9, 603, 783), (594, 774)),
        ((36, 36, 630, 810), (594, 774)),
        (
            (4.536, 112.536, 607.46405, 895.46405),
            (602.9280395507812, 782.9280395507812),
        ),
        ((90, 216, 702, 1008), (612, 792)),
    ],
)
def test_known_cropped_source_extents_map_by_identity(crop, extent):
    h = harness()
    req = {**request(), "bbox": [10, 20, 300, 400], "source_page_bbox": [0, 0, *extent]}
    page = SimpleNamespace(rotation=0, cropbox=crop, mediabox=(0, 0, 1000, 1200))
    parsed = h._crop_request(req)
    h._validate_fintabnet_crop(page, parsed)
    assert parsed["bbox"] == req["bbox"]  # No CropBox coordinate subtraction.
    assert parsed["source_page_bbox"] == req["source_page_bbox"]


@pytest.mark.parametrize(
    "change,rotation",
    [
        ({"source_page_bbox": None}, 0),
        ({"source_page_bbox": [0, 0, 612, 1008]}, 0),
        ({"source_page_bbox": [1, 0, 612, 918]}, 0),
        ({"source_page_bbox": [0, 0, float("nan"), 918]}, 0),
        ({"source_page_bbox": [0, 0, 612, 0]}, 0),
        ({"bbox": [0, 0, 613, 900]}, 0),
        ({}, 90),
    ],
)
def test_invalid_proof_rejected_before_recognizer(
    tmp_path, monkeypatch, change, rotation
):
    doc = pdfspine.open()
    page = doc.new_page(width=612, height=1008)
    page.set_cropbox(pdfspine.Rect(0, 72, 612, 990))
    page.set_rotation(rotation)
    path = tmp_path / "cropped.pdf"
    doc.save(path)

    def trap(*args, **kwargs):
        raise AssertionError("model/recognizer must not run")

    monkeypatch.setattr(_tatr, "_recognize_gold_crop", trap)
    h = harness()
    with pytest.raises(ValueError):
        h._worker_pdfspine(
            str(path),
            0,
            "vision",
            "tatr",
            {"ocr_if_no_text": False},
            mode="gold-crop-tsr",
            crop_request={**request(), **change},
        )


def test_missing_proof_only_retains_verified_full_page_compatibility():
    h = harness()
    req = request()
    del req["source_page_bbox"]
    h._validate_fintabnet_crop(
        SimpleNamespace(
            rotation=0, cropbox=(0, 0, 612, 918), mediabox=(0, 0, 612, 918)
        ),
        h._crop_request(req),
    )
    with pytest.raises(ValueError):
        h._validate_fintabnet_crop(
            SimpleNamespace(
                rotation=0, cropbox=(0, 72, 612, 990), mediabox=(0, 0, 612, 1008)
            ),
            h._crop_request(req),
        )


def test_page_display_does_not_accept_mislabelled_source_proof():
    with pytest.raises(ValueError):
        harness()._crop_request({**request(), "coordinate_space": "page-display"})


def test_source_extent_is_part_of_response_identity():
    h = harness()
    crop = h._crop_request(request())
    config = h.eval_config("vision", "onnx", {}, mode="gold-crop-tsr")
    response = {
        "ok": True,
        "tables": [
            {
                "table_id": crop["table_id"],
                "crop_request": crop,
                "quality": "empty",
                "cells": [],
            }
        ],
        "backend_metadata": {"backend": "onnx", "requested": config},
    }
    h._validate_response(response, expected_config=config, expected_crop=crop)
    changed = {**crop, "source_page_bbox": [0, 0, 612, 919]}
    with pytest.raises(ValueError, match="identity"):
        h._validate_response(response, expected_config=config, expected_crop=changed)
