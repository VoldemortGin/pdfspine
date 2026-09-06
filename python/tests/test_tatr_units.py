"""TATR-* — model-free unit tests for :mod:`pdfspine._tatr`.

Torch/Transformers/Pillow are not installed in the test venv (same as CI).  All
tests here run without them: the pure geometry/option/record helpers are called
directly, and the few runtime-bound paths use tiny fakes injected via
``monkeypatch`` (fake ``sys.modules`` entries, fake runtime/page/image objects),
in the same spirit as ``test_tatr_tables.py``.
"""

from __future__ import annotations

from types import ModuleType, SimpleNamespace
import sys

import pytest

import pdfspine
from pdfspine import _tatr


# --------------------------------------------------------------------------- #
# Small fakes reused across tests
# --------------------------------------------------------------------------- #
class FakeImage:
    """Minimal Pillow stand-in: only the ops TATR calls."""

    def __init__(self, size):
        self.size = size

    def crop(self, bbox):
        return FakeImage((round(bbox[2] - bbox[0]), round(bbox[3] - bbox[1])))

    def rotate(self, _degrees, *, expand):
        assert expand
        return FakeImage((self.size[1], self.size[0]))

    def resize(self, target):
        return FakeImage(target)

    def tobytes(self):
        return b"\x00\x00\x00" * (self.size[0] * self.size[1])


def _rendered(image=None, tokens=None, page_bbox=(0.0, 0.0, 200.0, 100.0), **kw):
    params = {
        "image": image,
        "tokens": tokens or [],
        "page_bbox": page_bbox,
        "scale_x": 2.0,
        "scale_y": 2.0,
        "text_source": "pdfspine-native",
    }
    params.update(kw)
    return _tatr._RenderedPage(**params)


def _token(bbox, text, block=0, line=0, word=0):
    return {
        "bbox": bbox,
        "text": text,
        "block_num": block,
        "line_num": line,
        "span_num": word,
    }


# --- TATR-042: counter-clockwise box rotation for every quadrant ---
def test_tatr_042_rotate_box_ccw_quadrants():
    box = [10, 20, 30, 40]
    assert _tatr._rotate_box_ccw(box, 100, 200, 0) == [10, 20, 30, 40]
    assert _tatr._rotate_box_ccw(box, 100, 200, 90) == [20, 70, 40, 90]
    assert _tatr._rotate_box_ccw(box, 100, 200, 180) == [70, 160, 90, 180]
    assert _tatr._rotate_box_ccw(box, 100, 200, 270) == [160, 10, 180, 30]
    # 360 degrees wraps back to the identity.
    assert _tatr._rotate_box_ccw(box, 100, 200, 360) == [10, 20, 30, 40]


# --- TATR-043: display<->normalized box rotation is invertible ---
@pytest.mark.parametrize("rotation", [0, 90, 180, 270])
def test_tatr_043_normalized_display_roundtrip(rotation):
    box = (10.0, 20.0, 30.0, 40.0)
    display = _tatr._normalized_box_to_display(box, 100.0, 200.0, rotation)
    restored = _tatr._display_box_to_normalized(display, 100.0, 200.0, rotation)
    assert tuple(restored) == pytest.approx(box)
    if rotation:
        assert tuple(display) != box


# --- TATR-044: page->image derives unrotated size when absent ---
def test_tatr_044_page_box_to_image_derives_unrotated_size():
    rotated = _rendered(
        page_bbox=(0.0, 0.0, 100.0, 200.0), rotation=90, unrotated_size=None
    )
    assert _tatr._page_box_to_image((10, 20, 30, 40), rotated) == pytest.approx(
        (40.0, 140.0, 80.0, 180.0)
    )
    upright = _rendered(
        page_bbox=(0.0, 0.0, 100.0, 200.0), rotation=0, unrotated_size=None
    )
    assert _tatr._page_box_to_image((10, 20, 30, 40), upright) == pytest.approx(
        (20.0, 40.0, 60.0, 80.0)
    )


# --- TATR-045: image->page inverts the rotated-size derivation ---
def test_tatr_045_image_box_to_page_rotated_size():
    rotated = _rendered(
        page_bbox=(0.0, 0.0, 100.0, 200.0), rotation=90, unrotated_size=None
    )
    assert _tatr._image_box_to_page((40, 140, 80, 180), rotated) == pytest.approx(
        (10.0, 20.0, 30.0, 40.0)
    )


# --- TATR-046: grid boundaries and clip intersection ---
def test_tatr_046_grid_boundaries_and_intersects_clip():
    assert _tatr._grid_boundaries([], 0) == []
    boundaries = _tatr._grid_boundaries([(0, 0, 10, 10), (20, 0, 30, 10)], 0)
    assert boundaries == [0.0, 15.0, 30.0]

    table = SimpleNamespace(bbox=(0.0, 0.0, 10.0, 10.0))
    assert _tatr._intersects_clip(table, None) is True
    assert _tatr._intersects_clip(table, (5, 5, 15, 15)) is True
    assert _tatr._intersects_clip(table, (20, 20, 30, 30)) is False
    with pytest.raises(ValueError, match="4-sequence"):
        _tatr._intersects_clip(table, (1, 2, 3))


# --- TATR-047: option validation error messages ---
@pytest.mark.parametrize(
    ("options", "error"),
    [
        ({"structure_threshold": float("nan")}, "structure_threshold must be finite"),
        ({"structure_threshold": 2}, r"structure_threshold must be in \[0, 1\]"),
        ({"detection_model": "   "}, "model identifiers must not be empty"),
        ({"structure_model": " "}, "model identifiers must not be empty"),
        ({"ocr_engine": "  "}, "OCR engine and language must not be empty"),
        ({"ocr_language": ""}, "OCR engine and language must not be empty"),
        ({"crop_padding": 10.0}, "crop_padding must be an int"),
        ({"detection_revision": 123}, "detection_revision must be a string or None"),
    ],
)
def test_tatr_047_option_validation_messages(options, error):
    with pytest.raises((TypeError, ValueError), match=error):
        _tatr.TatrOptions.from_mapping(options)


# --- TATR-048: from_mapping handles None and non-mappings ---
def test_tatr_048_from_mapping_none_and_non_mapping():
    assert _tatr.TatrOptions.from_mapping(None) == _tatr.TatrOptions()
    with pytest.raises(TypeError, match="vision_options must be a mapping or None"):
        _tatr.TatrOptions.from_mapping([("dpi", 96)])


# --- TATR-049: model sources resolve a PDFSPINE_TATR_MODELS root ---
def test_tatr_049_model_sources_root_directory(monkeypatch, tmp_path):
    monkeypatch.delenv("PDFSPINE_TATR_DETECTION_MODEL", raising=False)
    monkeypatch.delenv("PDFSPINE_TATR_STRUCTURE_MODEL", raising=False)
    (tmp_path / "detection").mkdir()
    (tmp_path / "structure-recognition-v1.1-all").mkdir()
    monkeypatch.setenv("PDFSPINE_TATR_MODELS", str(tmp_path))
    detection, structure = _tatr._model_sources(_tatr.TatrOptions())
    assert detection == str(tmp_path / "detection")
    assert structure == str(tmp_path / "structure-recognition-v1.1-all")

    # A root without the expected subdirectories leaves the hub ids untouched.
    monkeypatch.setenv("PDFSPINE_TATR_MODELS", str(tmp_path / "empty"))
    detection2, structure2 = _tatr._model_sources(_tatr.TatrOptions())
    assert detection2 == _tatr.DETECTION_MODEL
    assert structure2 == _tatr.STRUCTURE_MODEL


# --- TATR-050: model kwargs only pin a revision for non-local refs ---
def test_tatr_050_model_kwargs_revision_logic(tmp_path):
    hub = _tatr._model_kwargs(_tatr.DETECTION_MODEL, "abc123", True)
    assert hub == {
        "local_files_only": True,
        "trust_remote_code": False,
        "revision": "abc123",
    }
    local = _tatr._model_kwargs(str(tmp_path), "abc123", True)
    assert "revision" not in local
    no_rev = _tatr._model_kwargs(_tatr.DETECTION_MODEL, None, False)
    assert no_rev == {"local_files_only": False, "trust_remote_code": False}


# --- TATR-051: unsupported-platform diagnostics ---
@pytest.mark.parametrize(
    ("kwargs", "fragment"),
    [
        ({"platform_name": "darwin", "machine": "ppc"}, "Apple Silicon"),
        (
            {"platform_name": "linux", "machine": "ppc64le", "libc_name": "glibc"},
            "x86-64 or ARM64",
        ),
        ({"platform_name": "win32", "machine": "arm64"}, "Windows require x86-64"),
        (
            {"platform_name": "linux", "machine": "x86_64", "libc_name": "musl"},
            "glibc-based",
        ),
    ],
)
def test_tatr_051_runtime_platform_error_edges(kwargs, fragment):
    message = _tatr._runtime_platform_error(python_version=(3, 12), **kwargs)
    assert message is not None and fragment in message


# --- TATR-052: missing-runtime prefers the platform message ---
def test_tatr_052_missing_runtime_platform_branch(monkeypatch):
    monkeypatch.setattr(_tatr, "_runtime_platform_error", lambda: "no wheels here")
    error = _tatr._missing_runtime(ValueError("boom"))
    assert isinstance(error, pdfspine.PdfUnsupportedError)
    assert "unavailable on this platform" in str(error)
    assert "no wheels here" in str(error)


# --- TATR-053: crop-edge detection and rotation remapping ---
def test_tatr_053_structure_crop_edges_and_unrotate():
    assert _tatr._structure_crop_edges([], (100, 100)) == {
        "left",
        "top",
        "right",
        "bottom",
    }
    edges = _tatr._structure_crop_edges(
        [{"label": "table", "score": 0.9, "bbox": [0, 0, 50, 50]}], (100, 100)
    )
    assert edges == {"left", "top"}
    assert _tatr._unrotate_crop_edges({"left", "right"}, False) == {"left", "right"}
    assert _tatr._unrotate_crop_edges({"left", "top"}, True) == {"bottom", "left"}


# --- TATR-054: expansion limits from vertically stacked detections ---
def test_tatr_054_detection_expansion_limits_vertical():
    current = {"label": "table", "bbox": [100, 100, 200, 200]}
    above = {"label": "table", "bbox": [100, 0, 200, 90]}
    below = {"label": "table", "bbox": [100, 210, 200, 300]}
    limits = _tatr._detection_expansion_limits(
        current, [current, above, below], (400, 400)
    )
    assert limits[1] == pytest.approx(95.0)
    assert limits[3] == pytest.approx(205.0)


# --- TATR-055: detection expansion grows every requested edge ---
def test_tatr_055_expand_detection_all_edges():
    detection = {"label": "table", "bbox": [100, 100, 200, 200]}
    expanded = _tatr._expand_detection(
        detection, {"left", "top", "right", "bottom"}, (0.0, 0.0, 400.0, 400.0)
    )
    assert expanded["bbox"] == (75.0, 75.0, 225.0, 225.0)


# --- TATR-056: native line anchors, success and failure ---
def test_tatr_056_native_line_anchors():
    assert _tatr._native_line_anchors(None, None) == []
    assert _tatr._native_line_anchors(SimpleNamespace(), None) == []

    class GoodPage:
        def find_tables(self, strategy, clip):
            assert strategy == "lines"
            return SimpleNamespace(tables=[SimpleNamespace(bbox=(0, 0, 100, 100))])

    assert _tatr._native_line_anchors(GoodPage(), None) == [(0.0, 0.0, 100.0, 100.0)]

    class BadPage:
        def find_tables(self, strategy, clip):
            raise RuntimeError("no vector lines")

    assert _tatr._native_line_anchors(BadPage(), None) == []


# --- TATR-057: line-guided detection snaps to the best anchor ---
def test_tatr_057_guided_detection_matches_anchor():
    rendered = _rendered(
        page_bbox=(0.0, 0.0, 1000.0, 1000.0),
        scale_x=1.0,
        scale_y=1.0,
        rotation=0,
        unrotated_size=(1000.0, 1000.0),
    )
    detection = {"label": "table", "score": 0.9, "bbox": [100, 100, 200, 200]}
    anchors = [
        [10, 10, 10, 10],  # zero area -> skipped
        [0, 0, 1000, 1000],  # area ratio out of range -> skipped
        [150, 150, 260, 260],  # containment too low -> skipped
        [95, 95, 205, 205],  # best match
    ]
    used: set[int] = set()
    result, page_bbox, guided = _tatr._guided_detection(
        detection, anchors, rendered, used
    )
    assert guided is True
    assert used == {3}
    assert result["bbox"] == pytest.approx((95.0, 95.0, 205.0, 205.0))
    assert page_bbox == pytest.approx((100.0, 100.0, 200.0, 200.0))

    # An already-used anchor is skipped, leaving no match.
    _, _, guided_again = _tatr._guided_detection(
        detection, [[95, 95, 205, 205]], rendered, {0}
    )
    assert guided_again is False


# --- TATR-058: native words fall back to OCR when no text layer ---
def test_tatr_058_native_words_ocr_fallback():
    class OcrPage:
        def get_text(self, kind, sort=False, textpage=None):
            return [(0, 0, 10, 10, "hi", 0, 0, 0)] if textpage is not None else []

        def get_textpage_ocr(self, dpi, language, engine):
            return "TP"

    words, source = _tatr._native_words(
        OcrPage(), _tatr.TatrOptions(ocr_if_no_text=True)
    )
    assert source == "pdfspine-ocr" and len(words) == 1

    class FailingOcrPage:
        def get_text(self, kind, sort=False, textpage=None):
            return []

        def get_textpage_ocr(self, dpi, language, engine):
            raise RuntimeError("ocr unavailable")

    words2, source2 = _tatr._native_words(FailingOcrPage(), _tatr.TatrOptions())
    assert words2 == [] and source2 == "none"

    class TextPage:
        def get_text(self, kind, sort=False, textpage=None):
            return []

    words3, source3 = _tatr._native_words(
        TextPage(), _tatr.TatrOptions(ocr_if_no_text=False)
    )
    assert words3 == [] and source3 == "pdfspine-native"


# --- TATR-059: crop rejection and rotated-crop token remapping ---
def test_tatr_059_make_crop_tiny_and_rotated():
    rendered = _rendered(image=FakeImage((400, 200)))
    assert _tatr._make_crop(rendered, {"bbox": [0, 0, 0.5, 0.5]}, 0) is None

    rendered_rot = _rendered(
        image=FakeImage((400, 200)),
        tokens=[_token([10, 20, 30, 40], "x")],
    )
    crop = _tatr._make_crop(
        rendered_rot, {"label": "table rotated", "bbox": [0, 0, 200, 100]}, 0
    )
    assert crop is not None
    assert crop.rotated is True
    assert crop.image.size == (100, 200)
    assert crop.unrotated_size == (200, 100)
    assert crop.tokens[0]["bbox"] == [60, 10, 80, 30]


# --- fake Pillow for _render_page ---
def _fake_pil():
    module = ModuleType("PIL")
    module.Image = SimpleNamespace(
        frombytes=lambda _mode, size, _samples: FakeImage(size)
    )
    return module


class _FakePixmap:
    def __init__(self, n=3):
        self.width = 100
        self.height = 50
        self.n = n
        self.samples = b"x"


class _FakeDisplayList:
    def __init__(self, pixmap):
        self._pixmap = pixmap

    def get_pixmap(self, **_kwargs):
        return self._pixmap


# --- TATR-060: render page with callable metadata and skipped words ---
def test_tatr_060_render_page_callable_metadata(monkeypatch):
    monkeypatch.setitem(sys.modules, "PIL", _fake_pil())

    class CallablePage:
        rect = (0.0, 0.0, 100.0, 50.0)

        def get_displaylist(self):
            return _FakeDisplayList(_FakePixmap(3))

        def cropbox(self):
            return (0.0, 0.0, 100.0, 50.0)

        def rotation(self):
            return 0

        def get_text(self, _kind, sort=False):
            return [
                (10, 10, 20, 20, "ok", 0, 0, 0),
                (0, 0, 0, 0, "", 0, 0, 0),  # empty text -> skipped
                (1,),  # too short -> skipped
            ]

    rendered = _tatr._render_page(
        CallablePage(), _tatr.TatrOptions(ocr_if_no_text=False)
    )
    assert rendered.page_bbox == (0.0, 0.0, 100.0, 50.0)
    assert rendered.rotation == 0
    assert len(rendered.tokens) == 1
    assert rendered.tokens[0]["bbox"] == [10.0, 10.0, 20.0, 20.0]


# --- TATR-061: render page degenerate crop, wrong channels, and missing PIL ---
def test_tatr_061_render_page_error_and_degenerate_paths(monkeypatch):
    monkeypatch.setitem(sys.modules, "PIL", _fake_pil())

    class DegeneratePage:
        rect = (0.0, 0.0, 0.0, 50.0)
        cropbox = (0.0, 0.0, 0.0, 50.0)
        rotation = 0

        def get_displaylist(self):
            return _FakeDisplayList(_FakePixmap(3))

        def get_text(self, _kind, sort=False):
            return []

    degenerate = _tatr._render_page(DegeneratePage(), _tatr.TatrOptions())
    assert degenerate.text_source == "none"
    assert degenerate.tokens == []

    class GrayPage:
        rect = (0.0, 0.0, 100.0, 50.0)
        cropbox = (0.0, 0.0, 100.0, 50.0)
        rotation = 0

        def get_displaylist(self):
            return _FakeDisplayList(_FakePixmap(1))

    with pytest.raises(pdfspine.PdfUnsupportedError, match="channels"):
        _tatr._render_page(GrayPage(), _tatr.TatrOptions())

    monkeypatch.setitem(sys.modules, "PIL", None)
    with pytest.raises(pdfspine.PdfUnsupportedError, match=r"pdfspine\[tatr\]"):
        _tatr._render_page(GrayPage(), _tatr.TatrOptions())


# --- TATR-062: table record skips malformed cells and spans-only bbox ---
def test_tatr_062_table_record_skips_and_span_bbox():
    cells = [
        {
            "row_nums": [0, 2],
            "column_nums": [0],
            "bbox": [0, 0, 10, 30],
        },  # non-contiguous rows
        {
            "row_nums": [0],
            "column_nums": [0, 2],
            "bbox": [0, 0, 30, 10],
        },  # non-contiguous cols
        {
            "row_nums": [0],
            "column_nums": [0],
            "bbox": [0, 0, 10, 10],
            "cell_text": "A",
            "header": True,
        },
        {
            "row_nums": [0],
            "column_nums": [0],
            "bbox": [0, 0, 10, 10],
            "cell_text": "B",
        },  # overlap
        {"row_nums": [1], "column_nums": [1], "bbox": [10, 10, 10, 10]},  # zero area
    ]
    row_boxes = [(0, 0, 10, 10), (0, 10, 10, 20), (0, 20, 10, 30)]
    column_boxes = [(0, 0, 10, 10), (10, 0, 20, 10), (20, 0, 30, 10)]
    record = _tatr._TatrTableRecord(cells, row_boxes, column_boxes, 0.9, "src", {})
    assert len(record.spans) == 1
    assert record.spans[0][:4] == (0, 0, 1, 1)
    assert record.extract()[0][0] == "A"
    assert record.header == ["A", None, None]

    # No structural boxes -> bbox comes from the accepted span rectangles.
    span_only = _tatr._TatrTableRecord(
        [
            {
                "row_nums": [0],
                "column_nums": [0],
                "bbox": [0, 0, 10, 10],
                "cell_text": "X",
            }
        ],
        [],
        [],
        0.5,
        "src",
        {},
    )
    assert span_only.bbox == (0.0, 0.0, 10.0, 10.0)


# --- TATR-063: HTML rowspan/empty cells and empty markdown ---
def test_tatr_063_table_record_html_and_markdown():
    cells = [
        {
            "row_nums": [0, 1],
            "column_nums": [0],
            "bbox": [0, 0, 10, 20],
            "cell_text": "span",
        },
        {
            "row_nums": [0],
            "column_nums": [1],
            "bbox": [10, 0, 20, 10],
            "cell_text": "a",
        },
    ]
    row_boxes = [(0, 0, 20, 10), (0, 10, 20, 20)]
    column_boxes = [(0, 0, 10, 20), (10, 0, 20, 20)]
    record = _tatr._TatrTableRecord(cells, row_boxes, column_boxes, 0.9, "src", {})
    html = record.to_html()
    assert 'rowspan="2"' in html
    assert "<td></td>" in html  # the uncovered (1, 1) slot

    empty = _tatr._TatrTableRecord([], [], [], 0.0, "src", {})
    assert empty.to_markdown() == ""


def _structure_objects():
    return [
        {"label": "table", "score": 0.99, "bbox": [0, 0, 200, 100]},
        {"label": "table column", "score": 0.98, "bbox": [0, 0, 100, 100]},
        {"label": "table column", "score": 0.97, "bbox": [100, 0, 200, 100]},
        {"label": "table row", "score": 0.98, "bbox": [0, 0, 200, 50]},
        {"label": "table row", "score": 0.97, "bbox": [0, 50, 200, 100]},
    ]


def _crop_and_render():
    rendered = _rendered(page_bbox=(0.0, 0.0, 100.0, 50.0))
    crop = _tatr._TableCrop(
        image=None,
        tokens=[_token([10, 10, 50, 40], "A"), _token([110, 10, 150, 40], "B", word=1)],
        bbox=(0.0, 0.0, 200.0, 100.0),
        rotated=False,
        unrotated_size=(200, 100),
    )
    return crop, rendered


# --- TATR-064: _table_from_structure returns None on every failure path ---
def test_tatr_064_table_from_structure_none_paths(monkeypatch):
    crop, rendered = _crop_and_render()

    # No 'table' object at all.
    assert (
        _tatr._table_from_structure(
            [{"label": "table column", "score": 0.9, "bbox": [0, 0, 100, 100]}],
            crop,
            rendered,
            0.9,
            {},
        )
        is None
    )

    # Only a table object -> no rows/columns survive post-processing.
    assert (
        _tatr._table_from_structure(
            [{"label": "table", "score": 0.9, "bbox": [0, 0, 200, 100]}],
            _tatr._TableCrop(None, [], (0, 0, 200, 100), False, (200, 100)),
            rendered,
            0.9,
            {},
        )
        is None
    )

    # Unknown structure labels are dropped, but a valid table still builds.
    objects = _structure_objects() + [
        {"label": "garbage", "score": 0.5, "bbox": [10, 10, 20, 20]}
    ]
    assert _tatr._table_from_structure(objects, crop, rendered, 0.9, {}) is not None

    # A post-processing exception is swallowed into None.
    def boom(*_args, **_kwargs):
        raise ValueError("post-processing failed")

    monkeypatch.setattr(_tatr._postprocess, "table_structure_to_cells", boom)
    assert (
        _tatr._table_from_structure(_structure_objects(), crop, rendered, 0.9, {})
        is None
    )

    # No cells produced -> None.
    monkeypatch.setattr(
        _tatr._postprocess, "table_structure_to_cells", lambda *a, **k: ([], 0.0)
    )
    assert (
        _tatr._table_from_structure(_structure_objects(), crop, rendered, 0.9, {})
        is None
    )


# --------------------------------------------------------------------------- #
# Fake torch / transformers stacks (built only where the assertion is real)
# --------------------------------------------------------------------------- #
def _bare_runtime(torch):
    runtime = object.__new__(_tatr._TransformersRuntime)
    runtime._torch = torch
    return runtime


def _cuda_torch(*, available=False, count=0, mps=False):
    torch = ModuleType("torch")
    torch.cuda = SimpleNamespace(
        is_available=lambda: available, device_count=lambda: count
    )
    torch.backends = SimpleNamespace(mps=SimpleNamespace(is_available=lambda: mps))
    return torch


# --- TATR-065: device resolution across all backends ---
def test_tatr_065_resolve_device_branches():
    assert _bare_runtime(_cuda_torch(available=True))._resolve_device("auto") == "cuda"
    assert _bare_runtime(_cuda_torch(available=False))._resolve_device("auto") == "cpu"
    assert _bare_runtime(_cuda_torch())._resolve_device("cpu") == "cpu"
    assert (
        _bare_runtime(_cuda_torch(available=True, count=1))._resolve_device("cuda:0")
        == "cuda:0"
    )
    assert _bare_runtime(_cuda_torch(mps=True))._resolve_device("mps") == "mps"

    with pytest.raises(pdfspine.PdfUnsupportedError, match="CUDA is unavailable"):
        _bare_runtime(_cuda_torch(available=False))._resolve_device("cuda")
    with pytest.raises(pdfspine.PdfUnsupportedError, match="only 1 CUDA"):
        _bare_runtime(_cuda_torch(available=True, count=1))._resolve_device("cuda:5")
    with pytest.raises(pdfspine.PdfUnsupportedError, match="MPS is unavailable"):
        _bare_runtime(_cuda_torch(mps=False))._resolve_device("mps")


class _FakeTensor:
    def view(self, *_a):
        return self

    def reshape(self, *_a):
        return self

    def permute(self, *_a):
        return self

    def unsqueeze(self, *_a):
        return self

    def to(self, *_a, **_k):
        return self

    def __sub__(self, _other):
        return self

    def __truediv__(self, _other):
        return self


class _Scalar:
    def __init__(self, value):
        self._value = value

    def detach(self):
        return self

    def cpu(self):
        return self

    def item(self):
        return self._value


class _Box:
    def __init__(self, values):
        self._values = values

    def detach(self):
        return self

    def cpu(self):
        return self

    def tolist(self):
        return self._values


class _Probabilities:
    def __init__(self, scores, labels):
        self._scores = scores
        self._labels = labels

    def max(self, _dim):
        return self._scores, self._labels


class _Indexable:
    def __init__(self, value):
        self._value = value

    def __getitem__(self, _index):
        return self._value


class _Logits:
    def __init__(self, probabilities):
        self._probabilities = probabilities

    def softmax(self, _dim):
        return _Indexable(self._probabilities)


class _Outputs:
    def __init__(self, logits, pred_boxes):
        self.logits = logits
        self.pred_boxes = pred_boxes


class _FakeModel:
    def __init__(self):
        self.eval_called = False

    def eval(self):
        self.eval_called = True

    def to(self, _device):
        return self

    def __call__(self, pixel_values=None):
        scores = [_Scalar(0.9), _Scalar(0.1), _Scalar(0.95)]
        labels = [_Scalar(0), _Scalar(0), _Scalar(6)]
        boxes = [
            _Box([0.5, 0.5, 1.0, 1.0]),
            _Box([0.2, 0.2, 0.1, 0.1]),
            _Box([0.5, 0.5, 0.5, 0.5]),
        ]
        probabilities = _Probabilities(scores, labels)
        return _Outputs(_Logits(probabilities), _Indexable(boxes))


def _predict_torch():
    torch = _cuda_torch()
    torch.float32 = "float32"
    torch.uint8 = "uint8"
    torch.tensor = lambda *_a, **_k: _FakeTensor()
    torch.frombuffer = lambda *_a, **_k: _FakeTensor()

    class _Inference:
        def __enter__(self):
            return self

        def __exit__(self, *_a):
            return False

    torch.inference_mode = lambda: _Inference()
    return torch


def _predict_transformers():
    module = ModuleType("transformers")

    class _Config:
        @staticmethod
        def get_config_dict(_source, **_kwargs):
            return ({"dilation": None}, {})

        @staticmethod
        def from_dict(config_dict):
            return config_dict

    class _AutoModel:
        @staticmethod
        def from_pretrained(_source, config=None, **_kwargs):
            return _FakeModel()

    module.AutoModelForObjectDetection = _AutoModel
    module.TableTransformerConfig = _Config
    return module


# --- TATR-066: full runtime construction, detection and recognition ---
def test_tatr_066_transformers_runtime_detect_recognize(monkeypatch):
    monkeypatch.setitem(sys.modules, "torch", _predict_torch())
    monkeypatch.setitem(sys.modules, "transformers", _predict_transformers())
    runtime = _tatr._TransformersRuntime(
        _tatr.TatrOptions(device="cpu"), _tatr.DETECTION_MODEL, _tatr.STRUCTURE_MODEL
    )
    assert runtime.device == "cpu"
    assert runtime.metadata["backend"] == "tatr"
    assert runtime.metadata["detection_revision"] == _tatr.DETECTION_REVISION

    image = FakeImage((100, 50))
    # Only the label-0 detection above threshold survives (label 6 -> "no object",
    # the 0.1 score is below the 0.5 threshold).
    assert runtime.detect(image, 0.5) == [
        {"label": "table", "score": 0.9, "bbox": [0.0, 0.0, 100.0, 50.0]}
    ]
    assert runtime.recognize(image, 0.5) == [
        {"label": "table", "score": 0.9, "bbox": [0.0, 0.0, 100.0, 50.0]}
    ]


# --- TATR-067: a failed checkpoint load is a clear PdfUnsupportedError ---
def test_tatr_067_runtime_reports_checkpoint_load_failure(monkeypatch):
    monkeypatch.setitem(sys.modules, "torch", _predict_torch())
    transformers = _predict_transformers()

    def failing_from_pretrained(_source, config=None, **_kwargs):
        raise OSError("checkpoint not in local cache")

    transformers.AutoModelForObjectDetection.from_pretrained = staticmethod(
        failing_from_pretrained
    )
    monkeypatch.setitem(sys.modules, "transformers", transformers)
    with pytest.raises(
        pdfspine.PdfUnsupportedError, match="Could not load the pinned TATR checkpoints"
    ) as excinfo:
        _tatr._TransformersRuntime(
            _tatr.TatrOptions(device="cpu"),
            _tatr.DETECTION_MODEL,
            _tatr.STRUCTURE_MODEL,
        )
    assert "local cache only" in str(excinfo.value)


# --- TATR-068: the runtime cache evicts once it is full ---
def test_tatr_068_cached_runtime_evicts(monkeypatch):
    class FakeRuntime:
        def __init__(self, options, detection_source, structure_source):
            pass

    monkeypatch.setattr(_tatr, "_TransformersRuntime", FakeRuntime)
    _tatr.clear_model_cache()
    try:
        for device in ["cpu", "cuda", "cuda:0", "cuda:1", "mps"]:
            _tatr._get_runtime(_tatr.TatrOptions(device=device))
        assert len(_tatr._MODEL_CACHE) == 4
    finally:
        _tatr.clear_model_cache()


class _FakeRuntime:
    metadata = {"backend": "tatr"}

    def __init__(self, detections, recognitions):
        self._detections = detections
        self._recognitions = recognitions
        self.recognize_calls = 0

    def detect(self, _image, _threshold):
        return list(self._detections)

    def recognize(self, image, _threshold):
        self.recognize_calls += 1
        if callable(self._recognitions):
            return self._recognitions(image)
        return list(self._recognitions)


# --- TATR-069: find_tables short-circuits on empty pages and bad crops ---
def test_tatr_069_find_tables_short_circuits(monkeypatch):
    empty_render = _rendered(image=FakeImage((10, 10)), page_bbox=(0.0, 0.0, 0.0, 0.0))
    monkeypatch.setattr(_tatr, "_render_page", lambda _p, _o: empty_render)
    finder = _tatr.find_tables(
        None, options={"native_line_guidance": False}, _runtime=_FakeRuntime([], [])
    )
    assert len(finder) == 0

    render = _rendered(image=FakeImage((400, 200)))
    monkeypatch.setattr(_tatr, "_render_page", lambda _p, _o: render)

    # A non-table detection is ignored.
    non_table = _FakeRuntime(
        [{"label": "no object", "score": 0.9, "bbox": [0, 0, 100, 100]}], []
    )
    finder2 = _tatr.find_tables(
        None,
        options={"native_line_guidance": False, "adaptive_crop": False},
        _runtime=non_table,
    )
    assert len(finder2) == 0

    # A sub-pixel detection cannot produce a crop.
    tiny = _FakeRuntime(
        [{"label": "table", "score": 0.9, "bbox": [0, 0, 0.5, 0.5]}], []
    )
    finder3 = _tatr.find_tables(
        None,
        options={
            "native_line_guidance": False,
            "adaptive_crop": False,
            "crop_padding": 0,
        },
        _runtime=tiny,
    )
    assert len(finder3) == 0


# --- TATR-070: adaptive cropping expands a truncated detection ---
def test_tatr_070_find_tables_adaptive_expansion(monkeypatch):
    render = _rendered(image=FakeImage((400, 200)), text_source="pdfspine-native")
    monkeypatch.setattr(_tatr, "_render_page", lambda _p, _o: render)

    captured: dict = {}
    original = _tatr._table_from_structure

    def spy(
        objects,
        crop,
        rendered,
        detection_score,
        runtime_metadata,
        structure_threshold=0.5,
    ):
        captured.update(runtime_metadata)
        return original(
            objects,
            crop,
            rendered,
            detection_score,
            runtime_metadata,
            structure_threshold,
        )

    monkeypatch.setattr(_tatr, "_table_from_structure", spy)

    def full_crop_table(image):
        width, height = image.size
        return [{"label": "table", "score": 0.9, "bbox": [0, 0, width, height]}]

    runtime = _FakeRuntime(
        [{"label": "table", "score": 0.9, "bbox": [20, 20, 180, 80]}], full_crop_table
    )
    _tatr.find_tables(
        None,
        options={"native_line_guidance": False, "adaptive_crop": True},
        _runtime=runtime,
    )
    # Initial crop plus two adaptive expansions -> three recognize calls.
    assert runtime.recognize_calls == 3
    assert captured["geometry_source"] == "tatr-structure+adaptive-context"
    assert captured["crop_expansions"] == 2
    assert captured["truncated"] is True


# --- TATR-071: vector line guidance overrides the detector crop ---
def test_tatr_071_find_tables_line_guided(monkeypatch):
    render = _rendered(image=FakeImage((400, 200)))
    monkeypatch.setattr(_tatr, "_render_page", lambda _p, _o: render)

    captured: dict = {}
    original = _tatr._table_from_structure

    def spy(
        objects,
        crop,
        rendered,
        detection_score,
        runtime_metadata,
        structure_threshold=0.5,
    ):
        captured.update(runtime_metadata)
        return original(
            objects,
            crop,
            rendered,
            detection_score,
            runtime_metadata,
            structure_threshold,
        )

    monkeypatch.setattr(_tatr, "_table_from_structure", spy)

    class LinePage:
        def find_tables(self, strategy, clip):
            return SimpleNamespace(tables=[SimpleNamespace(bbox=(4, 4, 96, 46))])

    def full_crop_table(image):
        width, height = image.size
        return [{"label": "table", "score": 0.9, "bbox": [0, 0, width, height]}]

    runtime = _FakeRuntime(
        [{"label": "table", "score": 0.9, "bbox": [20, 20, 180, 80]}], full_crop_table
    )
    _tatr.find_tables(LinePage(), options={"adaptive_crop": False}, _runtime=runtime)
    assert captured["geometry_source"] == "tatr-structure+vector-lines-guidance"
    assert captured["crop_expansions"] == 1
