"""Explicit raw-structure TableFormer experiment; never Page.find_tables backend."""

from __future__ import annotations

import copy
import hashlib
import importlib.metadata
import json
import math
from functools import lru_cache
from numbers import Integral, Real
from pathlib import Path

PINS = {
    "fast": (
        "3119563aab5a7c96fda4d621119b63fd8806272b86c30936d15507616422f718",
        "dca6762508dddfae6d57d6cb4ef822c6000119dff0f3b6489db7413118c2622a",
    ),
    "accurate": (
        "2a7d6c924b3cd12fb99a09280ca9c33a89c5d60b93253617d2e088c1a40374d9",
        "984e122ceb8ccf84d84c9d2882f6f2302a44b4f1e577babd6289892c36f3cffd",
    ),
}
MAX_SLOTS = 1_000_000


def options(value=None):
    result = {
        "variant": "fast",
        "model_dir": None,
        "dpi": 144,
        "device": "cpu",
        "num_threads": 2,
        "ocr_if_no_text": False,
        "local_files_only": True,
    }
    if value is not None:
        if not isinstance(value, dict) or set(value) - set(result):
            raise TypeError("unknown TableFormer evaluation options")
        result.update(value)
    if result["variant"] not in PINS or result["device"] != "cpu":
        raise ValueError("TableFormer requires fast/accurate and CPU")
    if result["ocr_if_no_text"] is not False or result["local_files_only"] is not True:
        raise ValueError("TableFormer requires native words and local-only models")
    for key, low, high in (("dpi", 36, 600), ("num_threads", 1, 16)):
        if type(result[key]) is not int or not low <= result[key] <= high:
            raise ValueError(f"invalid TableFormer {key}")
    if result["model_dir"] is not None and not isinstance(result["model_dir"], str):
        raise TypeError("model_dir must be a string")
    return result


def identity(path):
    p = Path(path).resolve()
    return {
        "path": str(p),
        "bytes": p.stat().st_size,
        "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
    }


def model_files(config):
    if not config["model_dir"]:
        raise ValueError("TableFormer requires an explicit model_dir")
    root = Path(config["model_dir"]).expanduser()
    weight = root / f"tableformer_{config['variant']}.safetensors"
    if list(root.glob("tableformer_*.safetensors")) != [weight]:
        raise ValueError("model_dir must contain exactly the selected single weight")
    files = [identity(weight), identity(root / "tm_config.json")]
    if tuple(f["sha256"] for f in files) != PINS[config["variant"]]:
        raise ValueError("TableFormer fixed model/config SHA mismatch")
    return files


@lru_cache(maxsize=2)
def _load(serialized):
    config = json.loads(serialized)
    files = model_files(config)
    if importlib.metadata.version("docling-ibm-models") != "4.0.2":
        raise ValueError("TableFormer requires docling-ibm-models==4.0.2")
    from docling_ibm_models.tableformer.data_management.tf_predictor import TFPredictor

    model_config = json.loads(Path(files[1]["path"]).read_text())
    model_config["model"]["save_dir"] = str(
        Path(config["model_dir"]).expanduser().absolute()
    )
    predictor = TFPredictor(
        copy.deepcopy(model_config), device="cpu", num_threads=config["num_threads"]
    )
    metadata = {
        "model_files": files,
        "effective_model_config": model_config,
        "runtime_source": identity(
            __import__(TFPredictor.__module__, fromlist=[""]).__file__
        ),
        "packages": {
            p: importlib.metadata.version(p)
            for p in (
                "docling-ibm-models",
                "torch",
                "torchvision",
                "opencv-python-headless",
                "numpy",
                "Pillow",
                "transformers",
                "safetensors",
            )
        },
    }
    return predictor, metadata


def convert_cells(raw):
    """Exclusive offsets to bounded ranges; bad topology stays a raw cell."""
    from pdfspine._tatr import _final_cells

    if not isinstance(raw, list) or any(not isinstance(c, dict) for c in raw):
        raise ValueError("invalid TableFormer response cells protocol")
    cells = []
    reasons = []
    occupied = 0
    for c in raw:
        spans = []
        for axis in ("row", "col"):
            start, end = (
                c.get(f"start_{axis}_offset_idx"),
                c.get(f"end_{axis}_offset_idx"),
            )
            span = c.get(f"{axis}_span")
            valid = all(
                isinstance(n, Integral) and not isinstance(n, bool)
                for n in (start, end, span)
            )
            if not valid or not 0 <= start < end <= MAX_SLOTS or end - start != span:
                spans.append(None)
            else:
                spans.append((int(start), int(end)))
        valid = all(s is not None for s in spans)
        if valid:
            slots = (spans[0][1] - spans[0][0]) * (spans[1][1] - spans[1][0])
            valid = (
                occupied + slots <= MAX_SLOTS and spans[0][1] * spans[1][1] <= MAX_SLOTS
            )
            if valid:
                occupied += slots
        if not valid:
            reasons.append("invalid-or-over-budget-span")
        box = c.get("bbox")
        bbox = (
            [box.get(k) for k in ("l", "t", "r", "b")]
            if isinstance(box, dict)
            else [None] * 4
        )
        cells.append(
            {
                "row_nums": list(range(*spans[0])) if valid else [],
                "column_nums": list(range(*spans[1])) if valid else [],
                "bbox": bbox,
                "cell_text": "",
            }
        )
    cells, validation = _final_cells(cells)
    return cells, reasons + validation


def recognize(page, bbox, *, options=None, padding=0, _predictor=None):
    from pdfspine import _onnx, _tatr
    import numpy as np

    config = globals()["options"](options)
    render_config = _tatr.TatrOptions(
        dpi=config["dpi"], ocr_if_no_text=False, device="cpu"
    )
    rendered, crop, metadata = _tatr._gold_crop(page, bbox, render_config, padding)
    if _predictor is None:
        predictor, runtime_metadata = _tatr._tsr_call(
            "load", _load, json.dumps(config, sort_keys=True)
        )
    else:
        predictor, runtime_metadata = _predictor, {"test_runtime": True}
    width, height = crop.image.size
    scale = 1024 / height
    input_box = [0, 0, width, height]
    tokens = [
        {
            "id": i,
            "text": str(t["text"]),
            "bbox": dict(zip(("l", "t", "r", "b"), t["bbox"])),
        }
        for i, t in enumerate(crop.tokens)
    ]
    image = np.asarray(crop.image.convert("RGB"))[:, :, ::-1].copy()
    payload = {"image": image, "width": width, "height": height, "tokens": tokens}
    raw = _tatr._tsr_call(
        "recognition",
        predictor.multi_table_predict,
        payload,
        [input_box.copy()],
        do_matching=False,
        sort_row_col_indexes=False,
        correct_overlapping_cells=False,
    )

    def postprocess():
        if not isinstance(raw, list) or len(raw) != 1 or not isinstance(raw[0], dict):
            raise ValueError("TableFormer must return one table response")
        cells, reasons = convert_cells(raw[0]["tf_responses"])
        # Invalid geometry is still retained for quality scoring, never passed to
        # geometry assignment. The table will receive TP0/rawFP/GTFN downstream.
        usable = [
            c
            for c in cells
            if len(c["bbox"]) == 4
            and all(isinstance(n, Real) and math.isfinite(n) for n in c["bbox"])
            and _tatr._area(c["bbox"]) > 0
        ]
        _onnx._assign_words(usable, crop.tokens)
        for c in usable:
            b = c["bbox"]
            c["bbox"] = list(
                _tatr._image_box_to_page(
                    (
                        b[0] + crop.bbox[0],
                        b[1] + crop.bbox[1],
                        b[2] + crop.bbox[0],
                        b[3] + crop.bbox[1],
                    ),
                    rendered,
                )
            )
        return cells, reasons

    cells, reasons = _tatr._tsr_call("postprocess", postprocess)

    def json_value(value):
        if isinstance(value, dict):
            return {str(k): json_value(v) for k, v in value.items()}
        if isinstance(value, (list, tuple)):
            return [json_value(v) for v in value]
        if value is None or isinstance(value, (str, bool)):
            return value
        if isinstance(value, Integral):
            return int(value)
        if isinstance(value, Real):
            return float(value) if math.isfinite(value) else None
        raise ValueError("unsupported TableFormer diagnostic value")

    # Only the actual final cells and model sequence are retained, not internal
    # tensors or matcher caches. Preserve invalid raw offsets without expanding.
    raw_diagnostic = _tatr._tsr_call(
        "serialization",
        json_value,
        {
            "tf_responses": raw[0]["tf_responses"],
            "prediction": raw[0].get("predict_details", {}).get("prediction", {}),
        },
    )
    metadata.update(runtime_metadata)
    metadata.update(
        effective_options=config,
        backend_source=identity(__file__),
        track_identity=f"tableformer-{config['variant']}-raw-structure",
        loaded_model_roles=["table"],
        executed_model_roles=["table"],
        word_assignment="onnx-native-overlap-center-nearest-v1",
        raw_tableformer=raw_diagnostic,
        recognition_options={
            "do_matching": False,
            "sort_row_col_indexes": False,
            "correct_overlapping_cells": False,
        },
        tableformer_geometry={
            "input_box": input_box,
            "color_order": "BGR",
            "resize_height": 1024,
            "resize_scale": scale,
            "resized_size": [int(width * scale), 1024],
            "internal_rounded_crop": [round(v * scale) for v in input_box],
            "actual_internal_crop": [
                0,
                0,
                min(round(width * scale), int(width * scale)),
                1024,
            ],
        },
    )
    return _tatr._CropRecognition(
        cells,
        metadata,
        "invalid" if reasons else "valid" if cells else "empty",
        tuple(reasons),
    )
