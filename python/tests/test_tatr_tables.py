"""TATR-* — offline tests for the optional vision table backend."""

from __future__ import annotations

import builtins
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import os
from pathlib import Path
import sys
import time
from types import ModuleType, SimpleNamespace

import pdfspine
import pytest

from pdfspine import _tatr


_BLANK_PDF = (
    b"%PDF-1.7\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
    b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n"
    b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n"
    b"trailer<</Root 1 0 R>>\n%%EOF"
)


def _structure_objects() -> list[dict]:
    return [
        {"label": "table", "score": 0.99, "bbox": [0, 0, 200, 100]},
        {"label": "table column", "score": 0.98, "bbox": [0, 0, 100, 100]},
        {"label": "table column", "score": 0.97, "bbox": [100, 0, 200, 100]},
        {"label": "table row", "score": 0.98, "bbox": [0, 0, 200, 50]},
        {"label": "table row", "score": 0.97, "bbox": [0, 50, 200, 100]},
        {
            "label": "table column header",
            "score": 0.96,
            "bbox": [0, 0, 200, 50],
        },
        {
            "label": "table spanning cell",
            "score": 0.95,
            "bbox": [0, 0, 200, 50],
        },
    ]


def _tokens() -> list[dict]:
    values = [
        ([10, 12, 75, 30], "Revenue", 0, 0, 0),
        ([115, 12, 175, 30], "2025", 0, 0, 1),
        ([10, 65, 75, 84], "1,234.50", 1, 0, 0),
        ([115, 65, 175, 84], "(9.00)", 1, 0, 1),
    ]
    return [
        {
            "bbox": bbox,
            "text": text,
            "block_num": block,
            "line_num": line,
            "span_num": word,
        }
        for bbox, text, block, line, word in values
    ]


def _pure_table() -> _tatr._TatrTableRecord:
    rendered = _tatr._RenderedPage(
        image=None,
        tokens=[],
        page_bbox=(0.0, 0.0, 100.0, 50.0),
        scale_x=2.0,
        scale_y=2.0,
        text_source="pdfspine-native",
    )
    crop = _tatr._TableCrop(
        image=None,
        tokens=_tokens(),
        bbox=(0.0, 0.0, 200.0, 100.0),
        rotated=False,
        unrotated_size=(200, 100),
    )
    table = _tatr._table_from_structure(
        _structure_objects(),
        crop,
        rendered,
        detection_score=0.99,
        runtime_metadata={"backend": "tatr", "revision": "test"},
    )
    assert table is not None
    return table


def test_tatr_001_official_postprocess_merged_header_and_exact_text():
    table = _pure_table()
    assert (table.row_count, table.col_count) == (2, 2)
    assert table.extract() == [
        ["Revenue 2025", None],
        ["1,234.50", "(9.00)"],
    ]
    assert any(span[:4] == (0, 0, 1, 2) for span in table.spans)
    assert table.header == ["Revenue 2025", None]
    assert 'colspan="2"' in table.to_html()
    assert "1,234.50" in table.to_markdown()
    assert table.grits_cells[0]["text_source"] == "pdfspine-native"
    assert table.bbox == pytest.approx((0.0, 0.0, 100.0, 50.0))
    assert table.spans[0][4] == pytest.approx((0.0, 0.0, 100.0, 25.0))


def test_tatr_002_model_revisions_and_offline_default_are_pinned():
    options = _tatr.TatrOptions()
    assert options.local_files_only is True
    assert len(options.detection_revision or "") == 40
    assert len(options.structure_revision or "") == 40
    assert options.crop_padding == 10
    assert options.native_line_guidance is True


def test_tatr_options_normalize_device_names():
    assert _tatr.TatrOptions(device=" CUDA:0 ").device == "cuda:0"


@pytest.mark.parametrize(
    ("options", "error"),
    [
        ({"dpi": 0}, "dpi"),
        ({"detection_threshold": 2}, "detection_threshold"),
        ({"crop_padding": 21}, "crop_padding"),
        ({"dpi": "144"}, "dpi must be an int"),
        ({"detection_threshold": True}, "real number"),
        ({"detection_threshold": float("nan")}, "finite"),
        ({"local_files_only": "false"}, "must be a bool"),
        ({"native_line_guidance": "false"}, "must be a bool"),
        ({"device": 1}, "device must be a string"),
        ({"device": "banana"}, "device must be"),
        ({"typo": True}, "unknown TATR option"),
    ],
)
def test_tatr_003_options_are_validated(options, error):
    with pytest.raises((TypeError, ValueError), match=error):
        _tatr.TatrOptions.from_mapping(options)


def test_tatr_004_missing_optional_runtime_has_install_hint(monkeypatch):
    real_import = builtins.__import__

    def blocked_import(name, *args, **kwargs):
        if name == "torch":
            raise ModuleNotFoundError("test blocks torch")
        return real_import(name, *args, **kwargs)

    monkeypatch.setattr(builtins, "__import__", blocked_import)
    with pytest.raises(pdfspine.PdfUnsupportedError, match=r"pdfspine\[tatr\]"):
        _tatr._TransformersRuntime(
            _tatr.TatrOptions(),
            _tatr.DETECTION_MODEL,
            _tatr.STRUCTURE_MODEL,
        )


def test_tatr_broken_optional_runtime_has_install_hint(monkeypatch):
    real_import = builtins.__import__

    def broken_import(name, *args, **kwargs):
        if name == "torch":
            raise OSError("missing dynamic library")
        return real_import(name, *args, **kwargs)

    monkeypatch.setattr(builtins, "__import__", broken_import)
    with pytest.raises(pdfspine.PdfUnsupportedError, match=r"pdfspine\[tatr\]"):
        _tatr._TransformersRuntime(
            _tatr.TatrOptions(),
            _tatr.DETECTION_MODEL,
            _tatr.STRUCTURE_MODEL,
        )


def test_tatr_005_page_dispatch_returns_public_table(monkeypatch):
    record = _pure_table()
    seen = {}

    def fake_find(page, *, clip, options):
        seen.update(page=page, clip=clip, options=options)
        return _tatr._TatrTableFinderRecord([record])

    monkeypatch.setattr(_tatr, "find_tables", fake_find)
    page = pdfspine.open(stream=_BLANK_PDF)[0]
    finder = page.find_tables(
        strategy="vision",
        backend="tatr",
        clip=(0, 0, 100, 100),
        vision_options={"dpi": 96},
    )
    assert isinstance(finder, pdfspine.TableFinder)
    assert isinstance(finder[0], pdfspine.Table)
    assert finder[0].source == "tatr"
    assert finder[0].metadata["revision"] == "test"
    assert seen["page"] is page
    assert seen["options"] == {"dpi": 96}


def test_tatr_006_native_strategy_never_imports_runtime(monkeypatch):
    def fail(*args, **kwargs):
        raise AssertionError("vision backend must not run")

    monkeypatch.setattr(_tatr, "find_tables", fail)
    page = pdfspine.open(stream=_BLANK_PDF)[0]
    finder = page.find_tables(strategy="lines")
    assert isinstance(finder, pdfspine.TableFinder)
    assert len(finder) == 0


def test_tatr_007_unknown_backend_and_misplaced_options_are_rejected():
    page = pdfspine.open(stream=_BLANK_PDF)[0]
    with pytest.raises(pdfspine.PdfUnsupportedError, match="unsupported"):
        page.find_tables(backend="unknown")
    with pytest.raises(TypeError, match="vision_options"):
        page.find_tables(strategy="lines", vision_options={"dpi": 96})


def test_tatr_008_rotated_crop_bbox_maps_back_to_page():
    rendered = _tatr._RenderedPage(
        image=None,
        tokens=[],
        page_bbox=(0.0, 0.0, 100.0, 100.0),
        scale_x=2.0,
        scale_y=2.0,
        text_source="none",
    )
    crop = _tatr._TableCrop(
        image=None,
        tokens=[],
        bbox=(20.0, 40.0, 120.0, 240.0),
        rotated=True,
        unrotated_size=(100, 200),
    )
    # Continuous image-box coordinates have no pixel-index ``-1`` offset.
    # Forward rotation maps source [10,20,30,60] to [140,10,180,30].
    result = _tatr._crop_box_to_page([140, 10, 180, 30], crop, rendered)
    assert result == pytest.approx((15.0, 30.0, 25.0, 50.0))


def test_tatr_008b_render_uses_zero_based_rotated_cropbox_displaylist(monkeypatch):
    calls = []

    class FakeDisplayList:
        def get_pixmap(self, **kwargs):
            calls.append(kwargs)
            return SimpleNamespace(
                width=560,
                height=400,
                n=3,
                samples=b"not-decoded-by-the-fake-image",
            )

    class FakePage:
        rect = (100.0, 80.0, 300.0, 360.0)
        cropbox = (100.0, 80.0, 300.0, 360.0)
        rotation = 90

        def get_displaylist(self):
            return FakeDisplayList()

        def get_pixmap(self, **_kwargs):
            raise AssertionError("image-only Page.get_pixmap fast path must not run")

        def get_text(self, _kind, **_kwargs):
            # Display-space form of unrotated (10,20,30,40) under /Rotate=90.
            return [(240.0, 10.0, 260.0, 30.0, "A", 0, 0, 0)]

    class FakeImage:
        def __init__(self, size):
            self.size = size

        def rotate(self, degrees, *, expand):
            assert (degrees, expand) == (90, True)
            return FakeImage((self.size[1], self.size[0]))

    fake_pil = ModuleType("PIL")
    fake_pil.Image = SimpleNamespace(
        frombytes=lambda _mode, size, _samples: FakeImage(size)
    )
    monkeypatch.setitem(sys.modules, "PIL", fake_pil)

    rendered = _tatr._render_page(
        FakePage(), _tatr.TatrOptions(dpi=144, ocr_if_no_text=False)
    )
    assert calls == [{"dpi": 144, "colorspace": 3, "alpha": False}]
    assert rendered.page_bbox == (0.0, 0.0, 280.0, 200.0)
    assert (rendered.scale_x, rendered.scale_y) == (2.0, 2.0)
    assert rendered.tokens[0]["bbox"] == [20.0, 40.0, 60.0, 80.0]


def test_tatr_009_direct_cells_preserve_empty_topology():
    table = _pure_table()
    occupied = {
        (row, column)
        for cell in table.grits_cells
        for row in cell["row_nums"]
        for column in cell["column_nums"]
    }
    assert occupied == {(0, 0), (0, 1), (1, 0), (1, 1)}


def _tables_diff_module():
    path = Path(__file__).resolve().parents[2] / "conformance" / "gt" / "tables_diff.py"
    spec = importlib.util.spec_from_file_location("_pdfspine_tables_diff_test", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_tatr_010_benchmark_prefers_direct_cells_and_penalizes_false_positive(tmp_path):
    harness = _tables_diff_module()
    gold = [
        {
            "pdf_table_bbox": [0, 0, 100, 100],
            "cells": [
                {
                    "row_nums": [0],
                    "column_nums": [0],
                    "json_text_content": "A",
                }
            ],
        }
    ]
    predictions = {
        "ok": True,
        "error": None,
        "tables": [
            {
                "bbox": [0, 0, 100, 100],
                "cells": [
                    {
                        "row_nums": [0],
                        "column_nums": [0],
                        "cell_text": "A",
                    }
                ],
                "html": "<table><tr><td>WRONG</td></tr></table>",
            },
            {
                "bbox": [120, 120, 180, 180],
                "cells": [
                    {
                        "row_nums": [0],
                        "column_nums": [0],
                        "cell_text": "extra",
                    }
                ],
            },
        ],
    }
    result = harness.process_doc_gold(
        tmp_path / "unused.pdf",
        "fixture",
        gold,
        0,
        sys.executable,
        10,
        predictor=lambda _pdf, _page: predictions,
        match_iou=0.5,
    )
    assert result["grits_top_sum"] == 1.0
    assert result["grits_con_sum"] == 1.0
    assert result["detection_precision"] == 0.5
    assert result["detection_recall"] == 1.0
    assert result["detection_f1"] == pytest.approx(2 / 3)


def test_tatr_011_persistent_worker_serves_two_requests_in_one_process(tmp_path):
    harness = _tables_diff_module()
    pdf = tmp_path / "blank.pdf"
    pdf.write_bytes(_BLANK_PDF)
    worker = harness.PersistentPdfspineWorker(sys.executable, "lines", 10, 10)
    try:
        pid = worker._process.pid
        first = worker.call(pdf, 0)
        second = worker.call(pdf, 0)
        assert first["ok"] and second["ok"]
        assert worker._process.pid == pid
        assert worker._process.poll() is None
    finally:
        worker.close()


@pytest.mark.skipif(
    os.environ.get("PDFSPINE_RUN_TATR_MODEL") != "1",
    reason="requires the two pinned TATR checkpoints in the local HF cache",
)
def test_tatr_012_real_pinned_models_offline_smoke():
    fixture = _m7_fixture_module()
    page = pdfspine.open(stream=fixture._ruled_table_pdf())[0]
    finder = page.find_tables(
        strategy="vision",
        backend="tatr",
        vision_options={
            "local_files_only": True,
            "device": "cpu",
            "ocr_if_no_text": False,
        },
    )
    assert len(finder) == 1
    assert (finder[0].row_count, finder[0].col_count) == (2, 3)
    assert finder[0].extract() == [
        ["A1", "B1", "C1"],
        ["A2", "B2", "C2"],
    ]
    assert finder[0].bbox.x1 > 350
    assert finder[0].metadata["geometry_source"].endswith("vector-lines-guidance")
    assert finder[0].metadata["detection_bbox"][2] < 350
    assert finder[0].metadata["truncated"] is False
    pure = page.find_tables(
        strategy="vision",
        backend="tatr",
        vision_options={
            "local_files_only": True,
            "device": "cpu",
            "ocr_if_no_text": False,
            "native_line_guidance": False,
        },
    )
    assert len(pure) == 1
    assert pure[0].bbox.x1 > 340
    assert pure[0].metadata["geometry_source"].endswith("adaptive-context")
    assert pure[0].metadata["crop_expansions"] >= 1


def _m7_fixture_module():
    fixture_path = Path(__file__).with_name("test_m7.py")
    spec = importlib.util.spec_from_file_location(
        "_pdfspine_m7_tatr_fixture", fixture_path
    )
    assert spec is not None and spec.loader is not None
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    return fixture


def _cropped_rotated_table_pdf(rotation: int) -> bytes:
    fixture = _m7_fixture_module()
    content = "1 w\n"
    for y in (700, 670, 640):
        content += f"100 {y} m 400 {y} l S\n"
    for x in (100, 200, 300, 400):
        content += f"{x} 640 m {x} 700 l S\n"
    content += "BT /F1 10 Tf\n"
    for x, y, value in [
        (110, 685, "A1"),
        (210, 685, "B1"),
        (310, 685, "C1"),
        (110, 655, "A2"),
        (210, 655, "B2"),
        (310, 655, "C2"),
    ]:
        content += f"1 0 0 1 {x} {y} Tm ({value}) Tj\n"
    content += "ET\n"
    raw = content.encode()
    stream = f"<< /Length {len(raw)} >>\nstream\n".encode() + raw + b"\nendstream"
    page = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 812 952] "
        b"/CropBox [100 80 712 872] "
        + f"/Rotate {rotation} ".encode()
        + b"/Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>"
    )
    return fixture._build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>"),
            (3, fixture._font()),
            (4, page),
            (5, stream),
        ],
        1,
    )


@pytest.mark.parametrize("rotation", [0, 90, 180, 270])
@pytest.mark.skipif(
    os.environ.get("PDFSPINE_RUN_TATR_MODEL") != "1",
    reason="requires the two pinned TATR checkpoints in the local HF cache",
)
def test_tatr_012b_nonzero_cropbox_and_page_rotation(rotation):
    page = pdfspine.open(stream=_cropped_rotated_table_pdf(rotation))[0]
    finder = page.find_tables(
        strategy="vision",
        backend="tatr",
        vision_options={
            "local_files_only": True,
            "device": "cpu",
            "ocr_if_no_text": False,
        },
    )
    assert len(finder) == 1
    assert (finder[0].row_count, finder[0].col_count) == (2, 3)
    assert finder[0].extract() == [
        ["A1", "B1", "C1"],
        ["A2", "B2", "C2"],
    ]
    assert finder[0].metadata["page_rotation"] == rotation
    assert finder[0].bbox.x0 >= 0 and finder[0].bbox.y0 >= 0
    if rotation == 0:
        # Device coordinates are CropBox-relative, never offset by (100, 80).
        assert finder[0].bbox.x0 < 50


def test_tatr_013_fake_runtime_extracts_multiple_tables(monkeypatch):
    class FakeImage:
        def __init__(self, size):
            self.size = size

        def crop(self, bbox):
            return FakeImage((round(bbox[2] - bbox[0]), round(bbox[3] - bbox[1])))

        def rotate(self, _degrees, *, expand):
            assert expand
            return FakeImage((self.size[1], self.size[0]))

    def token(bbox, text, block, word):
        return {
            "bbox": bbox,
            "text": text,
            "block_num": block,
            "line_num": 0,
            "span_num": word,
        }

    rendered = _tatr._RenderedPage(
        image=FakeImage((400, 200)),
        tokens=[
            token([10, 30, 60, 50], "L1", 0, 0),
            token([110, 30, 160, 50], "R1", 0, 1),
            token([230, 30, 280, 50], "L2", 1, 0),
            token([330, 30, 380, 50], "R2", 1, 1),
        ],
        page_bbox=(0.0, 0.0, 200.0, 100.0),
        scale_x=2.0,
        scale_y=2.0,
        text_source="pdfspine-native",
    )
    monkeypatch.setattr(_tatr, "_render_page", lambda _page, _options: rendered)

    class FakeRuntime:
        metadata = {"backend": "tatr", "fixture": True}

        def detect(self, _image, _threshold):
            return [
                {"label": "table", "score": 0.99, "bbox": [0, 20, 180, 80]},
                {"label": "table", "score": 0.98, "bbox": [220, 20, 400, 80]},
            ]

        def recognize(self, image, _threshold):
            width, height = image.size
            return [
                {"label": "table", "score": 0.99, "bbox": [0, 0, width, height]},
                {
                    "label": "table column",
                    "score": 0.98,
                    "bbox": [0, 0, width / 2, height],
                },
                {
                    "label": "table column",
                    "score": 0.98,
                    "bbox": [width / 2, 0, width, height],
                },
                {"label": "table row", "score": 0.98, "bbox": [0, 0, width, height]},
            ]

    finder = _tatr.find_tables(
        None,
        options={"ocr_if_no_text": False, "adaptive_crop": False},
        _runtime=FakeRuntime(),
    )
    assert len(finder) == 2
    assert [table.extract() for table in finder.tables] == [
        [["L1", "R1"]],
        [["L2", "R2"]],
    ]
    clipped = _tatr.find_tables(
        None,
        clip=(0, 0, 100, 100),
        options={"ocr_if_no_text": False, "adaptive_crop": False},
        _runtime=FakeRuntime(),
    )
    assert len(clipped) == 1
    assert clipped[0].extract() == [["L1", "R1"]]


def test_tatr_014_runtime_cache_ignores_page_and_threshold_options(monkeypatch):
    created = []

    class FakeRuntime:
        def __init__(self, options, detection_source, structure_source):
            created.append((options, detection_source, structure_source))

    monkeypatch.setattr(_tatr, "_TransformersRuntime", FakeRuntime)
    _tatr.clear_model_cache()
    try:
        first = _tatr._get_runtime(_tatr.TatrOptions(dpi=96, detection_threshold=0.4))
        second = _tatr._get_runtime(
            _tatr.TatrOptions(
                dpi=200,
                detection_threshold=0.9,
                local_files_only=False,
            )
        )
        assert first is second
        assert len(created) == 1
    finally:
        _tatr.clear_model_cache()


def test_tatr_015_concurrent_first_calls_load_models_once(monkeypatch):
    created = []

    class SlowFakeRuntime:
        def __init__(self, options, detection_source, structure_source):
            created.append((options, detection_source, structure_source))
            time.sleep(0.02)

    monkeypatch.setattr(_tatr, "_TransformersRuntime", SlowFakeRuntime)
    _tatr.clear_model_cache()
    try:
        options = _tatr.TatrOptions()
        with ThreadPoolExecutor(max_workers=4) as executor:
            runtimes = list(
                executor.map(lambda _index: _tatr._get_runtime(options), range(4))
            )
        assert len({id(runtime) for runtime in runtimes}) == 1
        assert len(created) == 1
    finally:
        _tatr.clear_model_cache()


def test_tatr_016_prebuilt_runtime_platform_boundaries():
    assert (
        _tatr._runtime_platform_error(
            python_version=(3, 15),
            platform_name="linux",
            machine="x86_64",
            libc_name="glibc",
        )
        is not None
    )
    assert (
        _tatr._runtime_platform_error(
            python_version=(3, 12), platform_name="darwin", machine="x86_64"
        )
        is not None
    )
    assert (
        _tatr._runtime_platform_error(
            python_version=(3, 12),
            platform_name="linux",
            machine="x86_64",
            libc_name="musl",
        )
        is not None
    )
    assert (
        _tatr._runtime_platform_error(
            python_version=(3, 12), platform_name="win32", machine="arm64"
        )
        is not None
    )
    assert (
        _tatr._runtime_platform_error(
            python_version=(3, 14), platform_name="darwin", machine="arm64"
        )
        is None
    )
    assert (
        _tatr._runtime_platform_error(
            python_version=(3, 14), platform_name="freebsd14", machine="x86_64"
        )
        is not None
    )


def test_tatr_adaptive_limits_never_shrink_an_overlapping_detection():
    current = {"label": "table", "bbox": [100, 100, 200, 200]}
    overlapping = {"label": "table", "bbox": [150, 100, 250, 200]}
    limits = _tatr._detection_expansion_limits(
        current, [current, overlapping], (400, 400)
    )
    expanded = _tatr._expand_detection(current, {"right"}, limits)
    assert expanded["bbox"][2] >= current["bbox"][2]


def test_tatr_017_local_model_sources_do_not_claim_hub_revisions(monkeypatch, tmp_path):
    detection = tmp_path / "detection"
    structure = tmp_path / "structure"
    detection.mkdir()
    structure.mkdir()
    monkeypatch.setenv("PDFSPINE_TATR_DETECTION_MODEL", str(detection))
    monkeypatch.setenv("PDFSPINE_TATR_STRUCTURE_MODEL", str(structure))
    created = []

    class FakeRuntime:
        def __init__(self, options, detection_source, structure_source):
            created.append((options, detection_source, structure_source))

    monkeypatch.setattr(_tatr, "_TransformersRuntime", FakeRuntime)
    _tatr.clear_model_cache()
    try:
        _tatr._get_runtime(_tatr.TatrOptions())
        options, detection_source, structure_source = created[0]
        assert options.detection_revision is None
        assert options.structure_revision is None
        assert detection_source == str(detection)
        assert structure_source == str(structure)
    finally:
        _tatr.clear_model_cache()
