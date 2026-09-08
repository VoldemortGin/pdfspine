"""ONNX-* — offline tests for the optional ONNX layout/table backend.

onnxruntime, numpy and Pillow are not required (same as CI): the pure
decoders are exercised directly and the two models (PP-DocLayout layout,
SLANet-plus tables) are replaced by fakes injected through ``_runtime=``.
``ONNX-014`` runs the real PP-DocLayout preprocessing when numpy/Pillow are
importable and skips otherwise. The end-to-end case replays the native
``strategy="lines"`` grid of a ruled fixture through the full ONNX pipeline
(render -> crop -> structure tokens -> text-layer fill -> HTML) and compares
cell for cell. Every character always comes from the PDF text layer.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
from types import ModuleType, SimpleNamespace

import pdfspine
import pytest

from pdfspine import _onnx, _tatr


# --------------------------------------------------------------------------- #
# Fakes and fixtures
# --------------------------------------------------------------------------- #
class FakeImage:
    """Minimal Pillow stand-in: only the ops the backend calls."""

    def __init__(self, size):
        self.size = size

    def crop(self, bbox):
        return FakeImage((round(bbox[2] - bbox[0]), round(bbox[3] - bbox[1])))

    def rotate(self, _degrees, *, expand):
        assert expand
        return FakeImage((self.size[1], self.size[0]))


def _fake_pil():
    module = ModuleType("PIL")
    module.Image = SimpleNamespace(
        frombytes=lambda _mode, size, _samples: FakeImage(size)
    )
    return module


def _token(bbox, text, block=0, line=0, word=0):
    return {
        "bbox": list(bbox),
        "text": text,
        "block_num": block,
        "line_num": line,
        "span_num": word,
    }


def _rendered(image=None, tokens=None, page_bbox=(0.0, 0.0, 200.0, 100.0)):
    return _tatr._RenderedPage(
        image=image or FakeImage((400, 200)),
        tokens=tokens or [],
        page_bbox=page_bbox,
        scale_x=2.0,
        scale_y=2.0,
        text_source="pdfspine-native",
    )


def _m7_fixture_module():
    fixture_path = Path(__file__).with_name("test_m7.py")
    spec = importlib.util.spec_from_file_location(
        "_pdfspine_m7_onnx_fixture", fixture_path
    )
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    return fixture


_BLANK_PDF = (
    b"%PDF-1.7\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
    b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n"
    b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n"
    b"trailer<</Root 1 0 R>>\n%%EOF"
)

_SIMPLE_2X2 = [
    "<tbody>",
    "<tr>",
    "<td></td>",
    "<td></td>",
    "</tr>",
    "<tr>",
    "<td></td>",
    "<td></td>",
    "</tr>",
    "</tbody>",
]


def _fake_onnx_modules(monkeypatch, providers=("CPUExecutionProvider",), sessions=None):
    """Install fake ``numpy`` / ``onnxruntime`` / ``PIL`` modules."""

    created = []

    class FakeSession:
        def __init__(self, path, sess_options=None, providers=None):
            created.append((path, tuple(providers or ())))

        def get_modelmeta(self):
            return SimpleNamespace(custom_metadata_map={})

        def get_inputs(self):
            # No static ``image`` shape: the variant's default edge is kept.
            return []

    ort = ModuleType("onnxruntime")
    ort.get_available_providers = lambda: list(providers)
    ort.SessionOptions = lambda: SimpleNamespace(log_severity_level=0)
    ort.InferenceSession = sessions or FakeSession
    monkeypatch.setitem(sys.modules, "onnxruntime", ort)
    monkeypatch.setitem(sys.modules, "numpy", ModuleType("numpy"))
    monkeypatch.setitem(sys.modules, "PIL", _fake_pil())
    return created


def _input_spec(name, shape):
    return SimpleNamespace(name=name, shape=shape)


def _session_with_inputs(*specs):
    return SimpleNamespace(get_inputs=lambda: list(specs))


# --------------------------------------------------------------------------- #
# ONNX-001: option validation
# --------------------------------------------------------------------------- #
def test_onnx_001_options_validation_and_mapping():
    default = _onnx.OnnxOptions()
    assert default.providers == "auto"
    assert default.channel_order == "bgr"
    assert default.layout_nms_iou == 0.6
    assert default.layout_threshold == 0.5
    assert default.layout_size is None
    assert default.layout_variant == "auto"
    listed = _onnx.OnnxOptions(
        providers=["CPUExecutionProvider"],
        channel_order=" RGB ",
        layout_variant=" PP_DocLayoutV3 ",
    )
    assert listed.providers == ("CPUExecutionProvider",)
    assert listed.channel_order == "rgb"
    assert listed.layout_variant == "pp_doclayoutv3"
    assert _onnx.OnnxOptions(layout_variant="AUTO").layout_variant == "auto"
    assert _onnx.OnnxOptions(layout_size=640).layout_size == 640
    assert _onnx.OnnxOptions(layout_threshold=1).layout_threshold == 1.0
    assert _onnx.OnnxOptions(layout_nms_iou=None).layout_nms_iou is None
    assert _onnx.OnnxOptions(table_model=Path("x.onnx")).table_model == "x.onnx"
    assert _onnx.OnnxOptions.from_mapping(None) == default
    assert _onnx.OnnxOptions.from_mapping({"dpi": 96}).dpi == 96
    with pytest.raises(TypeError, match="unknown ONNX option"):
        _onnx.OnnxOptions.from_mapping({"bogus": 1})
    with pytest.raises(TypeError, match="mapping"):
        _onnx.OnnxOptions.from_mapping(["dpi"])
    for options, error, message in [
        ({"dpi": 96.0}, TypeError, "dpi must be an int"),
        ({"dpi": 10}, ValueError, "dpi"),
        ({"layout_threshold": True}, TypeError, "real number"),
        ({"layout_threshold": 1.5}, ValueError, r"\[0, 1\]"),
        ({"layout_nms_iou": "x"}, TypeError, "layout_nms_iou"),
        ({"layout_nms_iou": 2.0}, ValueError, "layout_nms_iou"),
        ({"layout_size": 1000}, ValueError, "multiple of 32"),
        ({"layout_size": "640"}, TypeError, "layout_size"),
        ({"layout_variant": "nope"}, ValueError, "layout_variant"),
        ({"layout_variant": 3}, TypeError, "layout_variant"),
        ({"table_size": 8}, ValueError, "table_size"),
        ({"channel_order": "gray"}, ValueError, "channel_order"),
        ({"channel_order": 3}, TypeError, "channel_order"),
        ({"providers": "cuda"}, ValueError, "providers"),
        ({"providers": ()}, ValueError, "must not be empty"),
        ({"providers": [1]}, TypeError, "providers"),
        ({"crop_padding": 50}, ValueError, "crop_padding"),
        ({"layout_model": 5}, TypeError, "layout_model"),
        ({"ocr_if_no_text": 1}, TypeError, "ocr_if_no_text"),
        ({"ocr_engine": " "}, ValueError, "OCR engine"),
    ]:
        with pytest.raises(error, match=message):
            _onnx.OnnxOptions(**options)


# --------------------------------------------------------------------------- #
# ONNX-002: model path resolution and missing runtime / model errors
# --------------------------------------------------------------------------- #
def test_onnx_002_model_paths_and_missing_errors(monkeypatch, tmp_path):
    monkeypatch.delenv(_onnx.MODELS_ENV, raising=False)
    layout, table = _onnx._model_paths(_onnx.OnnxOptions())
    assert layout == Path(_onnx.LAYOUT_MODEL_FILE)
    assert table == Path(_onnx.TABLE_MODEL_FILE)

    monkeypatch.setenv(_onnx.MODELS_ENV, str(tmp_path))
    layout, table = _onnx._model_paths(_onnx.OnnxOptions())
    assert layout == tmp_path / _onnx.LAYOUT_MODEL_FILE
    assert table == tmp_path / _onnx.TABLE_MODEL_FILE

    # The V3 variant resolves to its own default file name.
    layout_v3, table_v3 = _onnx._model_paths(
        _onnx.OnnxOptions(layout_variant="pp_doclayoutv3")
    )
    assert layout_v3 == tmp_path / _onnx.LAYOUT_MODEL_FILES["pp_doclayoutv3"]
    assert layout_v3 != layout
    assert table_v3 == table

    explicit = tmp_path / "custom-layout.onnx"
    layout, table = _onnx._model_paths(_onnx.OnnxOptions(layout_model=str(explicit)))
    assert layout == explicit
    assert table == tmp_path / _onnx.TABLE_MODEL_FILE

    # Missing runtime: a clear install hint, before any model access.
    monkeypatch.setitem(sys.modules, "onnxruntime", None)
    with pytest.raises(pdfspine.PdfUnsupportedError, match=r"pdfspine\[onnx\]"):
        _onnx._OnnxRuntime(layout, table, "auto")

    # Runtime present, model file absent: the download URL is in the message.
    _fake_onnx_modules(monkeypatch)
    runtime = _onnx._OnnxRuntime(layout, table, "auto")
    with pytest.raises(pdfspine.PdfUnsupportedError) as excinfo:
        runtime._session("layout")
    assert _onnx.LAYOUT_MODEL_URL in str(excinfo.value)
    assert _onnx.MODELS_ENV in str(excinfo.value)
    with pytest.raises(pdfspine.PdfUnsupportedError) as excinfo:
        runtime._session("table")
    assert _onnx.TABLE_MODEL_URL in str(excinfo.value)

    # The V3 runtime quotes the V3 file name and download URL, not the L ones.
    runtime_v3 = _onnx._OnnxRuntime(layout_v3, table, "auto", "pp_doclayoutv3")
    with pytest.raises(pdfspine.PdfUnsupportedError) as excinfo:
        runtime_v3._session("layout")
    message = str(excinfo.value)
    assert _onnx.LAYOUT_MODEL_FILES["pp_doclayoutv3"] in message
    assert _onnx.LAYOUT_MODEL_URLS["pp_doclayoutv3"] in message
    assert _onnx.LAYOUT_MODEL_URL not in message
    assert repr(str(layout_v3)) in message


# --------------------------------------------------------------------------- #
# ONNX-003: structure token decoding with 8-point quads
# --------------------------------------------------------------------------- #
def _one_hot(index, size):
    row = [0.0] * size
    row[index] = 0.9
    return row


def test_onnx_003_decode_structure_tokens_and_quads():
    dictionary = list(_onnx.SLANET_STRUCTURE_DICT)
    size = len(dictionary) + 2
    index = {token: i + 1 for i, token in enumerate(dictionary)}
    eos = size - 1
    sequence = ["<tr>", "<td", ' colspan="2"', ">", "</td>", "<td></td>", "</tr>"]
    probs = [_one_hot(0, size)]  # leading <sos> is skipped, not a stop
    probs += [_one_hot(index[token], size) for token in sequence]
    probs += [_one_hot(eos, size), _one_hot(index["<tr>"], size)]
    quads = [[0.0] * 8 for _ in probs]
    # Quad corners are given clockwise from top-left; second <td> is rotated.
    quads[2] = [0.1, 0.1, 0.5, 0.1, 0.5, 0.3, 0.1, 0.3]
    quads[6] = [0.9, 0.3, 0.9, 0.5, 0.5, 0.5, 0.5, 0.3]
    tokens, boxes, scores = _onnx._decode_structure(probs, quads, dictionary, 100.0)
    assert tokens == sequence
    assert boxes == [[10.0, 10.0, 50.0, 30.0], [50.0, 30.0, 90.0, 50.0]]
    assert scores == [pytest.approx(0.9)] * len(sequence)
    # Four-value boxes and malformed quads degrade gracefully.
    assert _onnx._quad_to_rect([0.2, 0.1, 0.1, 0.4], 10.0) == [1.0, 1.0, 2.0, 4.0]
    assert _onnx._quad_to_rect([0.5], 10.0) == [0.0, 0.0, 0.0, 0.0]


# --------------------------------------------------------------------------- #
# ONNX-004: tokens -> occupancy grid with colspan / rowspan / header
# --------------------------------------------------------------------------- #
def test_onnx_004_structure_to_cells_spans_and_header():
    tokens = [
        "<thead>",
        "<tr>",
        "<td",
        ' colspan="2"',
        ">",
        "</td>",
        "<td",
        ' rowspan="2"',
        ">",
        "</td>",
        "</tr>",
        "</thead>",
        "<tbody>",
        "<tr>",
        "<td></td>",
        "<td></td>",
        "</tr>",
        "<tr>",
        "<td",
        ' colspan="3"',
        ">",
        "</td>",
        "</tr>",
        "</tbody>",
    ]
    boxes = [
        [0, 0, 20, 10],
        [20, 0, 30, 20],
        [0, 10, 10, 20],
        [10, 10, 20, 20],
        [0, 20, 30, 30],
    ]
    cells = _onnx._structure_to_cells(tokens, boxes)
    assert [(c["row_nums"], c["column_nums"], c["header"]) for c in cells] == [
        ([0], [0, 1], True),
        ([0, 1], [2], True),
        ([1], [0], False),
        ([1], [1], False),
        ([2], [0, 1, 2], False),
    ]
    assert cells[1]["bbox"] == [20, 0, 30, 20]
    # Cells without a box or with a degenerate box are dropped; a missing
    # leading <tr> still starts row 0.
    assert _onnx._structure_to_cells(["<td></td>", "<td></td>"], [[0, 0, 5, 5]]) == [
        {
            "row_nums": [0],
            "column_nums": [0],
            "bbox": [0, 0, 5, 5],
            "header": False,
            "score": 1.0,
            "cell_text": "",
        }
    ]
    assert _onnx._structure_to_cells(["<tr>", "<td></td>"], [[0, 0, 0, 5]]) == []


# --------------------------------------------------------------------------- #
# ONNX-005: word -> cell assignment never loses a text-layer word
# --------------------------------------------------------------------------- #
def test_onnx_005_assign_words_overlap_centre_nearest():
    cells = [
        {"bbox": [0, 0, 50, 20], "cell_text": ""},
        {"bbox": [50, 0, 100, 20], "cell_text": ""},
        {"bbox": [0, 20, 100, 60], "cell_text": ""},
    ]
    tokens = [
        _token([5, 5, 25, 15], "left"),
        _token([45, 5, 65, 15], "straddle"),  # < 50% in either -> centre in right
        _token([10, 40, 30, 50], "second"),
        _token([10, 25, 30, 35], "first"),
        _token([120, 70, 140, 80], "outside"),  # nearest cell fallback
        _token([40, 40, 60, 50], "second-b"),
    ]
    coverage = _onnx._assign_words(cells, tokens)
    assert coverage == 1.0
    assert cells[0]["cell_text"] == "left"
    assert cells[1]["cell_text"] == "straddle"
    assert cells[2]["cell_text"] == "first\nsecond second-b\noutside"
    assert _onnx._assign_words(cells, []) == 0.0
    assert _onnx._assign_words([], tokens) == 0.0


# --------------------------------------------------------------------------- #
# ONNX-006: PP-DocLayout RT-DETR row decoding (threshold, labels, NMS, order)
# --------------------------------------------------------------------------- #
def test_onnx_006_decode_layout_rtdetr_rows_labels_and_nms():
    labels = _onnx.PP_DOCLAYOUT_L_LABELS
    labels_v3 = _onnx.PP_DOCLAYOUTV3_LABELS
    # Pin the class-id order: the model output only carries the integer id.
    assert len(labels) == 23 and labels[8] == "table"
    assert len(labels_v3) == 25
    assert _onnx.LAYOUT_LABELS is labels
    cls = labels.index
    image_size = (400, 200)
    rows = [
        [cls("table"), 0.9, 10.0, 20.0, 210.0, 120.0],  # pixels taken verbatim
        [cls("text"), 0.8, -5.0, -5.0, 450.0, 250.0],  # clamped to the image
        [cls("paragraph_title"), 0.79, 10.0, 10.0, 50.0, 30.0],
        [cls("table_title"), 0.78, 10.0, 40.0, 50.0, 60.0],
        [cls("header"), 0.77, 10.0, 70.0, 50.0, 90.0],
        [cls("number"), 0.76, 60.0, 70.0, 80.0, 90.0],
        [cls("formula"), 0.75, 10.0, 100.0, 50.0, 120.0],
        [cls("image"), 0.74, 10.0, 130.0, 50.0, 150.0],
        [-1, 0.99, 0.0, 0.0, 100.0, 100.0],  # RT-DETR padding row
        [cls("text"), 0.4, 0.0, 0.0, 100.0, 100.0],  # below threshold
        [cls("text"), 0.9, 30.0, 30.0, 30.0, 80.0],  # zero area
        [42, 0.6, 0.0, 0.0, 100.0, 100.0],  # unknown class id
        [cls("text"), 0.9, 1.0, 2.0, 3.0],  # truncated row
    ]
    decoded = _onnx._decode_layout(rows, image_size, 0.5, labels)
    assert [(d["raw_label"], d["label"], d["score"]) for d in decoded] == [
        ("table", "table", 0.9),
        ("text", "plain text", 0.8),
        ("paragraph_title", "title", 0.79),
        ("table_title", "table_caption", 0.78),
        ("header", "abandon", 0.77),
        ("number", "abandon", 0.76),
        ("formula", "isolate_formula", 0.75),
        ("image", "figure", 0.74),
        ("42", "42", 0.6),
    ]
    assert decoded[0]["bbox"] == (10.0, 20.0, 210.0, 120.0)
    assert decoded[1]["bbox"] == (0.0, 0.0, 400.0, 200.0)
    assert all("read_order" not in d for d in decoded)
    assert _onnx._decode_layout(rows, image_size, 0.95, labels) == []

    # ``count`` (the head's second output) truncates the padded row list.
    assert [
        d["raw_label"]
        for d in _onnx._decode_layout(rows, image_size, 0.5, labels, count=1)
    ] == ["table"]
    assert _onnx._decode_layout(rows, image_size, 0.5, labels, count=0) == []
    for count in (len(rows), 99, -1):
        assert (
            len(_onnx._decode_layout(rows, image_size, 0.5, labels, count=count)) == 9
        )

    # Label map per variant: PP-DocLayout-L has no vision footnote, so its
    # ``footnote`` is the closest thing to a table footnote; V3 keeps page
    # footnotes as body text and maps ``vision_footnote`` instead.
    l_footnote = _onnx._decode_layout(
        [[cls("footnote"), 0.9, 0.0, 0.0, 10.0, 10.0]],
        image_size,
        0.5,
        labels,
        _onnx.LAYOUT_LABEL_MAP,
    )
    assert (l_footnote[0]["raw_label"], l_footnote[0]["label"]) == (
        "footnote",
        "table_footnote",
    )
    cls_v3 = labels_v3.index
    v3_rows = [
        [cls_v3("footnote"), 0.9, 0.0, 150.0, 200.0, 190.0, 3.0],
        [cls_v3("vision_footnote"), 0.8, 0.0, 100.0, 200.0, 140.0, 2.0],
        [cls_v3("text"), 0.7, 0.0, 0.0, 200.0, 90.0, 1.0],
        [cls_v3("text"), 0.6, 250.0, 0.0, 300.0, 90.0, float("nan")],
    ]
    v3 = _onnx._decode_layout(
        v3_rows, image_size, 0.5, labels_v3, _onnx.LAYOUT_LABEL_MAP_V3
    )
    assert [(d["raw_label"], d["label"], d.get("read_order")) for d in v3] == [
        ("footnote", "plain text", 3.0),
        ("vision_footnote", "table_footnote", 2.0),
        ("text", "plain text", 1.0),
        ("text", "plain text", None),  # non-finite key is dropped
    ]

    # NMS only suppresses within the same model class: RT-DETR emits duplicate
    # (query, class) pairs for one region, but a text box over an image box is
    # real. ``per_class=False`` gives the classic class-agnostic pass.
    overlapping = [
        [cls("table"), 0.9, 10.0, 10.0, 110.0, 110.0],
        [cls("table"), 0.8, 12.0, 12.0, 112.0, 112.0],  # IoU ~0.92, same class
        [cls("image"), 0.7, 12.0, 12.0, 112.0, 112.0],  # same box, other class
        [cls("table"), 0.6, 100.0, 100.0, 200.0, 200.0],  # barely touches
    ]
    with_nms = _onnx._decode_layout(overlapping, image_size, 0.5, labels, nms_iou=0.6)
    assert [(d["raw_label"], d["score"]) for d in with_nms] == [
        ("table", 0.9),
        ("image", 0.7),
        ("table", 0.6),
    ]
    without_nms = _onnx._decode_layout(overlapping, image_size, 0.5, labels)
    assert len(without_nms) == 4
    class_agnostic = _onnx._nms(without_nms, 0.6, per_class=False)
    assert [(d["raw_label"], d["score"]) for d in class_agnostic] == [
        ("table", 0.9),
        ("table", 0.6),
    ]
    assert _onnx._nms(without_nms, 0.6) == with_nms

    assert _onnx._labels_from_metadata({"names": "{1: 'b', 0: 'a'}"}) == ("a", "b")
    assert _onnx._labels_from_metadata({"names": "['x', 'y']"}) == ("x", "y")
    assert _onnx._labels_from_metadata(
        {"names": "not python", "character": "p\nq\n"}
    ) == (
        "p",
        "q",
    )
    assert _onnx._labels_from_metadata({}) is None


# --------------------------------------------------------------------------- #
# ONNX-007: backend dispatch from the public Page API
# --------------------------------------------------------------------------- #
def test_onnx_007_page_dispatch(monkeypatch):
    seen = {}
    finder = _tatr._TatrTableFinderRecord([])

    def fake_onnx(page, *, clip=None, options=None, _runtime=None):
        seen["onnx"] = (page, clip, options)
        return finder

    def fake_tatr(page, *, clip=None, options=None, _runtime=None):
        seen["tatr"] = (page, clip, options)
        return finder

    monkeypatch.setattr(_onnx, "find_tables", fake_onnx)
    monkeypatch.setattr(_tatr, "find_tables", fake_tatr)
    page = pdfspine.open(stream=_BLANK_PDF)[0]
    result = page.find_tables(
        strategy="vision", backend="ONNX", vision_options={"dpi": 96}, clip=(0, 0, 1, 1)
    )
    assert isinstance(result, pdfspine.TableFinder) and len(result) == 0
    assert seen["onnx"] == (page, (0, 0, 1, 1), {"dpi": 96})
    assert "tatr" not in seen
    page.find_tables(strategy="vision")
    assert seen["tatr"][2] is None
    page.find_tables(backend="onnx")
    assert seen["onnx"][2] is None
    with pytest.raises(pdfspine.PdfUnsupportedError, match="'tatr' or 'onnx'"):
        page.find_tables(backend="paddle")
    with pytest.raises(TypeError, match="vision_options"):
        page.find_tables(vision_options={"dpi": 96})

    monkeypatch.setattr(
        _onnx,
        "find_layout",
        lambda page, *, options=None, _runtime=None: [("layout", options)],
    )
    monkeypatch.setattr(
        _onnx,
        "get_layout_html",
        lambda page, *, options=None, _runtime=None: repr(options),
    )
    assert page.find_layout(dpi=96) == [("layout", {"dpi": 96})]
    assert page.find_layout() == [("layout", None)]
    assert page.get_layout_html(providers=["CPUExecutionProvider"]) == (
        "{'providers': ['CPUExecutionProvider']}"
    )
    assert page.get_layout_html() == "None"
    assert "LayoutBlock" in pdfspine.__all__ and "OnnxOptions" in pdfspine.__all__


# --------------------------------------------------------------------------- #
# ONNX-008: fake runtime, two tables, clip filtering
# --------------------------------------------------------------------------- #
class _TwoTableRuntime:
    metadata = {"backend": "onnx", "fixture": True}

    def __init__(self, tokens=None):
        self.tokens = tokens if tokens is not None else _SIMPLE_2X2[:5] + ["</tbody>"]

    def detect_layout(self, _image, _options):
        return [
            {
                "label": "table",
                "raw_label": "table",
                "score": 0.99,
                "bbox": (0, 20, 180, 80),
            },
            {
                "label": "plain text",
                "raw_label": "text",
                "score": 0.99,
                "bbox": (0, 0, 180, 20),
            },
            {
                "label": "table",
                "raw_label": "table",
                "score": 0.98,
                "bbox": (220, 20, 400, 80),
            },
        ]

    def recognize_table(self, image, _options):
        width, height = image.size
        tokens = ["<tbody>", "<tr>", "<td></td>", "<td></td>", "</tr>", "</tbody>"]
        boxes = [[0, 0, width / 2, height], [width / 2, 0, width, height]]
        return tokens, boxes, [0.9] * len(tokens)


def test_onnx_008_fake_runtime_two_tables_and_clip(monkeypatch):
    rendered = _rendered(
        tokens=[
            _token([10, 30, 60, 50], "L1", 0, 0, 0),
            _token([110, 30, 160, 50], "R1", 0, 0, 1),
            _token([230, 30, 280, 50], "L2", 1, 0, 0),
            _token([330, 30, 380, 50], "R2", 1, 0, 1),
        ]
    )
    monkeypatch.setattr(_onnx, "_render_page", lambda _page, _options: rendered)
    finder = _onnx.find_tables(
        None, options={"ocr_if_no_text": False}, _runtime=_TwoTableRuntime()
    )
    assert len(finder) == 2
    assert [table.extract() for table in finder.tables] == [
        [["L1", "R1"]],
        [["L2", "R2"]],
    ]
    assert all(table.source == "onnx" for table in finder.tables)
    assert finder[0].metadata["backend"] == "onnx"
    assert finder[0].metadata["geometry_source"] == "slanet-plus-cells"
    assert finder[0].confidence == pytest.approx(0.9)
    clipped = _onnx.find_tables(
        None, clip=(0, 0, 100, 100), _runtime=_TwoTableRuntime()
    )
    assert len(clipped) == 1
    assert clipped[0].extract() == [["L1", "R1"]]
    with pytest.raises(ValueError, match="clip"):
        _onnx.find_tables(None, clip=(0, 0, 1), _runtime=_TwoTableRuntime())
    # An empty page short-circuits without touching the models.
    empty = _rendered(page_bbox=(0.0, 0.0, 0.0, 0.0))
    monkeypatch.setattr(_onnx, "_render_page", lambda _page, _options: empty)
    assert len(_onnx.find_tables(None, _runtime=object())) == 0
    assert _onnx.find_layout(None, _runtime=object()) == []
    assert _onnx.get_layout_html(None, _runtime=object()) == ""


# --------------------------------------------------------------------------- #
# ONNX-009: stubbed models replay the native lines grid, cell for cell
# --------------------------------------------------------------------------- #
class _ReplayRuntime:
    """Fake models that answer with the native ``lines`` geometry."""

    metadata = {"backend": "onnx", "fixture": True}

    def __init__(self, page, lines_table, tokens, padding):
        self.page = page
        self.table = lines_table
        self.tokens = tokens
        self.padding = padding
        self.crop_origin = None

    def _scale(self, image):
        return image.size[0] / self.page.rect.width, image.size[
            1
        ] / self.page.rect.height

    def detect_layout(self, image, _options):
        sx, sy = self._scale(image)
        x0, y0, x1, y1 = self.table.bbox
        bbox = (x0 * sx, y0 * sy, x1 * sx, y1 * sy)
        self.crop_origin = (bbox[0] - self.padding, bbox[1] - self.padding)
        self.scale = (sx, sy)
        return [{"label": "table", "raw_label": "table", "score": 0.95, "bbox": bbox}]

    def recognize_table(self, _image, _options):
        sx, sy = self.scale
        ox, oy = self.crop_origin
        boxes = []
        for row in self.table.cells:
            for cell in row:
                if cell is None:
                    continue
                boxes.append(
                    [
                        cell.x0 * sx - ox,
                        cell.y0 * sy - oy,
                        cell.x1 * sx - ox,
                        cell.y1 * sy - oy,
                    ]
                )
        return self.tokens, boxes, [0.9] * len(self.tokens)


@pytest.mark.parametrize(
    ("fixture_name", "tokens", "expected_spans"),
    [
        (
            "_ruled_table_pdf",
            [
                "<tbody>",
                "<tr>",
                "<td></td>",
                "<td></td>",
                "<td></td>",
                "</tr>",
                "<tr>",
                "<td></td>",
                "<td></td>",
                "<td></td>",
                "</tr>",
                "</tbody>",
            ],
            [
                (0, 0, 1, 1),
                (0, 1, 1, 1),
                (0, 2, 1, 1),
                (1, 0, 1, 1),
                (1, 1, 1, 1),
                (1, 2, 1, 1),
            ],
        ),
        (
            "_merged_header_pdf",
            [
                "<thead>",
                "<tr>",
                "<td",
                ' colspan="2"',
                ">",
                "</td>",
                "</tr>",
                "</thead>",
                "<tbody>",
                "<tr>",
                "<td></td>",
                "<td></td>",
                "</tr>",
                "</tbody>",
            ],
            [(0, 0, 1, 2), (1, 0, 1, 1), (1, 1, 1, 1)],
        ),
    ],
)
def test_onnx_009_stubbed_models_match_native_lines_grid(
    monkeypatch, fixture_name, tokens, expected_spans
):
    monkeypatch.setitem(sys.modules, "PIL", _fake_pil())
    fixture = _m7_fixture_module()
    page = pdfspine.open(stream=getattr(fixture, fixture_name)())[0]
    lines = page.find_tables(strategy="lines")
    assert len(lines) == 1
    truth = lines[0]
    padding = 4
    runtime = _ReplayRuntime(page, truth, tokens, padding)
    finder = _onnx.find_tables(
        page,
        options={"crop_padding": padding, "ocr_if_no_text": False},
        _runtime=runtime,
    )
    assert len(finder) == 1
    table = finder[0]
    assert table.source == "onnx"
    assert table.text_source == "pdfspine-native"
    assert table.extract() == truth.extract()
    assert (table.row_count, table.col_count) == (truth.row_count, truth.col_count)
    assert [span[:4] for span in table.spans] == expected_spans
    for span, truth_span in zip(table.spans, truth.spans):
        assert span[4] == pytest.approx(tuple(truth_span[4]), abs=1.0)
    assert table.bbox == pytest.approx(tuple(truth.bbox), abs=1.0)
    html = table.to_html()
    assert html.startswith("<table>") and html.endswith("</table>")
    assert html.count("<tr>") == truth.row_count
    if fixture_name == "_merged_header_pdf":
        assert '<th colspan="2">HEAD</th>' in html
        assert table.header == ["HEAD", None]
    else:
        assert "<td>A1</td><td>B1</td><td>C1</td>" in html
    # The public wrapper exposes the same record through the Page API.
    monkeypatch.setattr(_onnx, "_get_runtime", lambda _config: runtime)
    public = page.find_tables(
        strategy="vision", backend="onnx", vision_options={"crop_padding": padding}
    )
    assert public[0].extract() == truth.extract()
    assert public[0].to_markdown() == table.to_markdown()


# --------------------------------------------------------------------------- #
# ONNX-010: reading order and semantic HTML tag mapping
# --------------------------------------------------------------------------- #
def _block(x0, y0, x1, y1, label, score=0.9):
    return (
        _onnx.LayoutBlock(pdfspine.Rect(x0, y0, x1, y1), label, score),
        {"label": label},
    )


def test_onnx_010_reading_order_and_layout_html(monkeypatch):
    page_bbox = (0.0, 0.0, 200.0, 300.0)
    blocks = [
        _block(110, 120, 190, 160, "right-1"),
        _block(10, 120, 90, 160, "left-1"),
        _block(10, 10, 190, 30, "full"),
        _block(110, 40, 190, 100, "right-0"),
        _block(10, 40, 90, 60, "left-0"),
        _block(10, 70, 90, 100, "left-0b"),
        _block(20, 200, 180, 220, "full-2"),
        _block(80, 230, 120, 240, "centred"),
    ]
    ordered = [block.label for block, _ in _onnx._reading_order(blocks, page_bbox)]
    assert ordered == [
        "full",
        "left-0",
        "left-0b",
        "left-1",
        "right-0",
        "right-1",
        "full-2",
        "centred",
    ]
    degenerate = _onnx._reading_order(blocks[:2], (0.0, 0.0, 0.0, 0.0))
    assert [b.label for b, _ in degenerate] == ["left-1", "right-1"]
    # ``raw_label`` defaults to the normalised label.
    assert blocks[0][0].raw_label == "right-1"
    titled = _onnx.LayoutBlock(pdfspine.Rect(0, 0, 1, 1), "title", 0.5, "doc_title")
    assert (titled.label, titled.raw_label) == ("title", "doc_title")

    tokens = [
        _token([20, 10, 60, 20], "Header", 0, 0, 0),
        _token([20, 40, 80, 50], "Title", 1, 0, 0),
        _token([20, 60, 60, 70], "Body", 2, 0, 0),
        _token([64, 60, 100, 70], "text", 2, 0, 1),
        _token([20, 80, 80, 90], "Caption", 3, 0, 0),
        _token([20, 100, 60, 110], "r1", 4, 0, 0),
        _token([20, 120, 60, 130], "r2", 4, 1, 0),
        _token([20, 160, 60, 170], "E=mc2", 5, 0, 0),
        _token([20, 180, 60, 190], "Note", 6, 0, 0),
        _token([20, 250, 60, 260], "Stray", 7, 0, 0),
    ]
    rendered = _rendered(tokens=tokens)

    class LayoutRuntime:
        metadata = {"backend": "onnx"}

        def detect_layout(self, _image, _options):
            # Normalised ``label`` plus the PP-DocLayout class it came from.
            return [
                {
                    "label": "abandon",
                    "raw_label": "header",
                    "score": 0.9,
                    "bbox": (0, 0, 400, 25),
                },
                {
                    "label": "title",
                    "raw_label": "paragraph_title",
                    "score": 0.9,
                    "bbox": (0, 35, 400, 55),
                },
                {
                    "label": "plain text",
                    "raw_label": "text",
                    "score": 0.9,
                    "bbox": (0, 55, 400, 75),
                },
                {
                    "label": "table_caption",
                    "raw_label": "table_title",
                    "score": 0.9,
                    "bbox": (0, 75, 400, 95),
                },
                {
                    "label": "table",
                    "raw_label": "table",
                    "score": 0.9,
                    "bbox": (0, 95, 400, 135),
                },
                {
                    "label": "figure",
                    "raw_label": "image",
                    "score": 0.9,
                    "bbox": (0, 135, 400, 155),
                },
                {
                    "label": "isolate_formula",
                    "raw_label": "formula",
                    "score": 0.9,
                    "bbox": (0, 155, 400, 175),
                },
                {"label": "unknown label", "score": 0.9, "bbox": (0, 175, 400, 195)},
                {
                    "label": "plain text",
                    "raw_label": "aside_text",
                    "score": 0.9,
                    "bbox": (0, 195, 400, 199),
                },
            ]

        def recognize_table(self, _image, _options):
            return [], [], []

    monkeypatch.setattr(_onnx, "_render_page", lambda _page, _options: rendered)
    html = _onnx.get_layout_html(None, _runtime=LayoutRuntime())
    assert html == (
        "<h2>Title</h2>\n"
        "<p>Body text</p>\n"
        '<p class="table_caption">Caption</p>\n'
        '<pre class="table">r1\nr2</pre>\n'
        '<figure data-bbox="0.00 67.50 200.00 77.50"></figure>\n'
        '<p class="formula">E=mc2</p>\n'
        '<p class="unknown_label">Note</p>\n'
        '<pre class="unclaimed">Stray</pre>\n'
    )
    assert "Header" not in html
    layout = _onnx.find_layout(None, _runtime=LayoutRuntime())
    assert [block.label for block in layout][:3] == ["abandon", "title", "plain text"]
    assert [block.raw_label for block in layout][:3] == [
        "header",
        "paragraph_title",
        "text",
    ]
    # A detection without ``raw_label`` falls back to its label.
    assert (layout[7].label, layout[7].raw_label) == ("unknown label", "unknown label")
    assert (layout[8].label, layout[8].raw_label) == ("plain text", "aside_text")
    assert layout[0].bbox == pdfspine.Rect(0, 0, 200, 12.5)
    assert layout[0].score == 0.9

    # A recognised table is emitted as <table>; the words it consumes do not
    # reappear in later blocks.
    html = _onnx.get_layout_html(None, _runtime=_TwoTableRuntime())
    assert html.count("<table>") == 2
    assert html.startswith(
        "<table><tr><td>Header<br>Title<br>Body text<br>Caption</td>"
    )
    assert html.count("Header") == 1 and "<p>" not in html


# --------------------------------------------------------------------------- #
# ONNX-011: SLANet output identification by last dimension
# --------------------------------------------------------------------------- #
def test_onnx_011_split_table_outputs_by_shape():
    boxes = SimpleNamespace(shape=(1, 7, 8))
    probs = SimpleNamespace(shape=(1, 7, 50))
    assert _onnx._split_table_outputs([boxes, probs]) == (boxes, probs)
    assert _onnx._split_table_outputs([probs, boxes]) == (boxes, probs)
    with pytest.raises(pdfspine.PdfUnsupportedError, match=r"\(1, 7, 8\)"):
        _onnx._split_table_outputs([boxes, SimpleNamespace(shape=(1, 7, 8))])
    with pytest.raises(pdfspine.PdfUnsupportedError, match="Unexpected"):
        _onnx._split_table_outputs([probs])


# --------------------------------------------------------------------------- #
# ONNX-012: grid approximation and runtime cache
# --------------------------------------------------------------------------- #
def test_onnx_012_grid_boxes_and_runtime_cache(monkeypatch, tmp_path):
    cells = [
        {"row_nums": [0], "column_nums": [0, 1], "bbox": [0, 0, 20, 10]},
        {"row_nums": [1], "column_nums": [0], "bbox": [0, 10, 10, 20]},
        {"row_nums": [1, 2], "column_nums": [1], "bbox": [10, 10, 20, 30]},
    ]
    rows, columns = _onnx._grid_boxes(cells)
    assert rows == [(0, 0, 20, 10), (0, 10, 20, 20), (0, 10, 20, 30)]
    assert columns == [(0, 0, 10, 30), (10, 0, 20, 30)]
    assert _onnx._grid_boxes([]) == ([], [])

    created = _fake_onnx_modules(monkeypatch)
    monkeypatch.setenv(_onnx.MODELS_ENV, str(tmp_path))
    _onnx.clear_model_cache()
    try:
        first = _onnx._get_runtime(_onnx.OnnxOptions(dpi=96))
        second = _onnx._get_runtime(_onnx.OnnxOptions(layout_threshold=0.5))
        assert first is second
        other = _onnx._get_runtime(
            _onnx.OnnxOptions(providers=["CPUExecutionProvider"])
        )
        assert other is not first
        assert len(_onnx._MODEL_CACHE) == 2
        # The variant is part of the cache key: V3 gets its own runtime, with
        # its own class list, label map, model file and input edge.
        v3 = _onnx._get_runtime(_onnx.OnnxOptions(layout_variant="pp_doclayoutv3"))
        assert v3 is not first
        assert (
            _onnx._get_runtime(
                _onnx.OnnxOptions(layout_variant="pp_doclayoutv3", dpi=72)
            )
            is v3
        )
        assert len(_onnx._MODEL_CACHE) == 3
        assert v3.layout_variant == "pp_doclayoutv3"
        assert v3.layout_labels == _onnx.PP_DOCLAYOUTV3_LABELS
        assert v3.layout_label_map is _onnx.LAYOUT_LABEL_MAP_V3
        assert v3.metadata["layout_variant"] == "pp_doclayoutv3"
        assert v3.metadata["layout_model"] == str(
            tmp_path / _onnx.LAYOUT_MODEL_FILES["pp_doclayoutv3"]
        )
        assert v3.metadata["preprocessing"] == (
            "pp-doclayout-800-rgb/slanet-plus-488-imagenet"
        )
        # ``auto`` infers the variant from an explicit model file name; the
        # default V3 file resolves to the same spec and hits the cache.
        named = _onnx._get_runtime(
            _onnx.OnnxOptions(layout_model=str(tmp_path / "pp_doc_layoutv3.onnx"))
        )
        assert named is v3
        custom = _onnx._get_runtime(
            _onnx.OnnxOptions(layout_model=str(tmp_path / "custom_v3.onnx"))
        )
        assert custom.layout_variant == "pp_doclayoutv3" and custom is not v3
        assert len(_onnx._MODEL_CACHE) == 4
        assert {spec.layout_variant for spec in _onnx._MODEL_CACHE} == {
            "pp_doclayout_l",
            "pp_doclayoutv3",
        }
        for index in range(4):
            _onnx._get_runtime(
                _onnx.OnnxOptions(table_model=str(tmp_path / f"t{index}.onnx"))
            )
        assert len(_onnx._MODEL_CACHE) == 4  # oldest entries were evicted
        assert created == []  # sessions are created lazily, on first use
        (tmp_path / _onnx.LAYOUT_MODEL_FILE).write_bytes(b"onnx")
        assert first._session("layout") is first._session("layout")
        assert created == [
            (str(tmp_path / _onnx.LAYOUT_MODEL_FILE), ("CPUExecutionProvider",))
        ]
        assert first.layout_variant == _onnx.DEFAULT_LAYOUT_VARIANT
        assert first.layout_labels == _onnx.PP_DOCLAYOUT_L_LABELS
        assert first.layout_label_map is _onnx.LAYOUT_LABEL_MAP
        assert first.structure_dict == _onnx.SLANET_STRUCTURE_DICT
        assert first.metadata["providers"] == ["CPUExecutionProvider"]
        assert first.metadata["layout_variant"] == "pp_doclayout_l"
        assert first.metadata["preprocessing"] == (
            "pp-doclayout-640-rgb/slanet-plus-488-imagenet"
        )
    finally:
        _onnx.clear_model_cache()
    assert _onnx._MODEL_CACHE == {}


# --------------------------------------------------------------------------- #
# ONNX-013: execution-provider resolution
# --------------------------------------------------------------------------- #
def test_onnx_013_resolve_providers():
    cuda = ("CUDAExecutionProvider", "CPUExecutionProvider")
    assert _onnx._resolve_providers("auto", cuda) == cuda
    assert _onnx._resolve_providers(
        "auto", ("CoreMLExecutionProvider", "CPUExecutionProvider")
    ) == ("CPUExecutionProvider",)
    assert _onnx._resolve_providers(("CPUExecutionProvider",), cuda) == (
        "CPUExecutionProvider",
    )
    with pytest.raises(pdfspine.PdfUnsupportedError, match="TensorrtExecutionProvider"):
        _onnx._resolve_providers(("TensorrtExecutionProvider",), cuda)


# --------------------------------------------------------------------------- #
# ONNX-014: PP-DocLayout input tensor, input edge and variant resolution
# --------------------------------------------------------------------------- #
def test_onnx_014_layout_input_and_variant_resolution(monkeypatch, tmp_path):
    np = pytest.importorskip("numpy")
    Image = pytest.importorskip("PIL.Image")

    # Plain resize to a square, RGB in [0, 1], NCHW: no letterbox padding and
    # no ImageNet mean/std (a red page stays exactly (1, 0, 0) everywhere).
    red = Image.new("RGBA", (400, 200), (255, 0, 0, 255))
    array, scale_factor = _onnx._layout_input(red, 64, np)
    assert array.shape == (1, 3, 64, 64)
    assert array.dtype == np.float32
    assert array.flags["C_CONTIGUOUS"]
    assert scale_factor == (64 / 200, 64 / 400)  # (scale_y, scale_x)
    assert float(array[0, 0].min()) == 1.0 and float(array[0, 0].max()) == 1.0
    assert float(array[0, 1:].max()) == 0.0
    blue = Image.new("RGB", (10, 30), (0, 0, 255))
    array, scale_factor = _onnx._layout_input(blue, 32, np)
    assert array.shape == (1, 3, 32, 32)
    assert float(array[0, 2].min()) == 1.0 and float(array[0, :2].max()) == 0.0
    assert scale_factor == (32 / 30, 32 / 10)
    with pytest.raises(pdfspine.PdfUnsupportedError, match="empty page image"):
        _onnx._layout_input(FakeImage((0, 10)), 32, np)

    # The static square edge is read from the ``image`` input only.
    assert (
        _onnx._input_edge(_session_with_inputs(_input_spec("image", [1, 3, 640, 640])))
        == 640
    )
    assert (
        _onnx._input_edge(
            _session_with_inputs(
                _input_spec("im_shape", [1, 2]), _input_spec("image", [-1, 3, 800, 800])
            )
        )
        == 800
    )
    for shape in (
        [1, 3, "h", "w"],
        [1, 3, None, None],
        [1, 3, 640, 800],
        [3, 640, 640],
        [1, 3, 0, 0],
        None,
    ):
        assert (
            _onnx._input_edge(_session_with_inputs(_input_spec("image", shape))) is None
        )
    assert (
        _onnx._input_edge(_session_with_inputs(_input_spec("images", [1, 3, 640, 640])))
        is None
    )
    assert _onnx._input_edge(_session_with_inputs()) is None

    # Variant from the file name (basename only), then option precedence.
    assert _onnx._variant_from_name("pp_doc_layoutv3.onnx") == "pp_doclayoutv3"
    assert _onnx._variant_from_name("/models/PP-DocLayoutV3.onnx") == "pp_doclayoutv3"
    assert _onnx._variant_from_name("pp_doclayout_l.onnx") == "pp_doclayout_l"
    assert _onnx._variant_from_name("layout.onnx") == "pp_doclayout_l"
    assert (
        _onnx._variant_from_name(str(Path("v3-models") / "pp_doclayout_l.onnx"))
        == "pp_doclayout_l"
    )
    assert _onnx._layout_variant(_onnx.OnnxOptions()) == _onnx.DEFAULT_LAYOUT_VARIANT
    assert (
        _onnx._layout_variant(_onnx.OnnxOptions(layout_model="x_v3.onnx"))
        == "pp_doclayoutv3"
    )
    assert (
        _onnx._layout_variant(
            _onnx.OnnxOptions(layout_model="x_v3.onnx", layout_variant="pp_doclayout_l")
        )
        == "pp_doclayout_l"
    )
    assert (
        _onnx._layout_variant(_onnx.OnnxOptions(layout_variant="pp_doclayoutv3"))
        == "pp_doclayoutv3"
    )
    assert set(_onnx.LAYOUT_MODEL_FILES) == set(_onnx.LAYOUT_VARIANTS)
    assert set(_onnx.LAYOUT_MODEL_URLS) == set(_onnx.LAYOUT_VARIANTS)
    assert set(_onnx.LAYOUT_INPUT_SIZES) == set(_onnx.LAYOUT_VARIANTS)
    assert set(_onnx.LAYOUT_LABELS_BY_VARIANT) == set(_onnx.LAYOUT_VARIANTS)
    assert set(_onnx.LAYOUT_LABEL_MAPS) == set(_onnx.LAYOUT_VARIANTS)

    # ``detect_layout`` feeds the PaddleDetection triple by input name, takes
    # the box count from the second output and returns decoded detections.
    feeds_seen = []

    class FakeSession:
        inputs = [
            _input_spec("image", [1, 3, 64, 64]),
            _input_spec("im_shape", [1, 2]),
            _input_spec("scale_factor", [1, 2]),
        ]

        def __init__(self, path, sess_options=None, providers=None):
            pass

        def get_modelmeta(self):
            return SimpleNamespace(custom_metadata_map={})

        def get_inputs(self):
            return list(self.inputs)

        def run(self, _names, feeds):
            feeds_seen.append(feeds)
            table = _onnx.PP_DOCLAYOUT_L_LABELS.index("table")
            text = _onnx.PP_DOCLAYOUT_L_LABELS.index("text")
            rows = [
                [table, 0.9, 10.0, 20.0, 210.0, 120.0],
                [text, 0.8, 0.0, 0.0, 50.0, 50.0],
                [text, 0.7, 0.0, 0.0, 50.0, 50.0],  # beyond ``count``
                [-1, 0.0, 0.0, 0.0, 0.0, 0.0],
            ]
            return [
                np.asarray([rows], dtype=np.float32),
                np.asarray([2], dtype=np.int32),
            ]

    ort = ModuleType("onnxruntime")
    ort.get_available_providers = lambda: ["CPUExecutionProvider"]
    ort.SessionOptions = lambda: SimpleNamespace(log_severity_level=0)
    ort.InferenceSession = FakeSession
    monkeypatch.setitem(sys.modules, "onnxruntime", ort)
    layout_path = tmp_path / _onnx.LAYOUT_MODEL_FILE
    layout_path.write_bytes(b"onnx")
    runtime = _onnx._OnnxRuntime(layout_path, tmp_path / "t.onnx", "auto")
    assert runtime.metadata["preprocessing"].startswith("pp-doclayout-640-")
    detections = runtime.detect_layout(red, _onnx.OnnxOptions())
    # The edge comes from the session (64), not the variant default (640).
    assert runtime.metadata["preprocessing"] == (
        "pp-doclayout-64-rgb/slanet-plus-488-imagenet"
    )
    assert [
        (d["raw_label"], d["label"], d["score"], d["bbox"]) for d in detections
    ] == [
        ("table", "table", pytest.approx(0.9), (10.0, 20.0, 210.0, 120.0)),
        ("text", "plain text", pytest.approx(0.8), (0.0, 0.0, 50.0, 50.0)),
    ]
    feeds = feeds_seen[-1]
    assert set(feeds) == {"image", "im_shape", "scale_factor"}
    assert feeds["image"].shape == (1, 3, 64, 64)
    assert feeds["im_shape"].tolist() == [[64.0, 64.0]]
    assert feeds["scale_factor"].tolist() == [
        [pytest.approx(64 / 200), pytest.approx(64 / 400)]
    ]
    # An explicit ``layout_size`` overrides the session edge.
    runtime.detect_layout(red, _onnx.OnnxOptions(layout_size=32))
    assert feeds_seen[-1]["image"].shape == (1, 3, 32, 32)
    assert feeds_seen[-1]["im_shape"].tolist() == [[32.0, 32.0]]
    # A model without an ``image`` input is rejected with its input names.
    FakeSession.inputs = [_input_spec("x", [1, 3, 64, 64])]
    other = _onnx._OnnxRuntime(layout_path, tmp_path / "t.onnx", "auto")
    with pytest.raises(
        pdfspine.PdfUnsupportedError, match="expected an 'image' input, got x"
    ):
        other.detect_layout(red, _onnx.OnnxOptions())


# --------------------------------------------------------------------------- #
# ONNX-015: PP-DocLayoutV3 reading order and footnote HTML
# --------------------------------------------------------------------------- #
class _V3Runtime:
    metadata = {"backend": "onnx", "layout_variant": "pp_doclayoutv3"}

    def __init__(self, detections):
        self.detections = detections

    def detect_layout(self, _image, _options):
        return [dict(d) for d in self.detections]

    def recognize_table(self, _image, _options):
        return [], [], []


def test_onnx_015_v3_read_order_and_footnote_html(monkeypatch):
    tokens = [
        _token([20, 10, 60, 20], "Second", 0, 0, 0),
        _token([20, 60, 60, 70], "First", 1, 0, 0),
        _token([20, 120, 60, 130], "Note", 2, 0, 0),
        _token([20, 160, 60, 170], "Vis", 3, 0, 0),
    ]
    rendered = _rendered(tokens=tokens)
    monkeypatch.setattr(_onnx, "_render_page", lambda _page, _options: rendered)
    # Geometrically top-to-bottom, but the model's reading-order keys say the
    # second block is read first.
    detections = [
        {
            "label": "plain text",
            "raw_label": "text",
            "score": 0.9,
            "bbox": (0, 0, 400, 30),
            "read_order": 2.0,
        },
        {
            "label": "plain text",
            "raw_label": "text",
            "score": 0.8,
            "bbox": (0, 50, 400, 80),
            "read_order": 1.0,
        },
        {
            "label": "plain text",
            "raw_label": "footnote",
            "score": 0.7,
            "bbox": (0, 110, 400, 140),
            "read_order": 3.0,
        },
        {
            "label": "table_footnote",
            "raw_label": "vision_footnote",
            "score": 0.6,
            "bbox": (0, 150, 400, 180),
            "read_order": 4.0,
        },
    ]
    layout = _onnx.find_layout(None, _runtime=_V3Runtime(detections))
    assert [(block.bbox.y0, block.raw_label) for block in layout] == [
        (25.0, "text"),
        (0.0, "text"),
        (55.0, "footnote"),
        (75.0, "vision_footnote"),
    ]
    assert (layout[2].label, layout[2].raw_label) == ("plain text", "footnote")
    assert layout[3].label == "table_footnote"
    html = _onnx.get_layout_html(None, _runtime=_V3Runtime(detections))
    assert html == (
        "<p>First</p>\n"
        "<p>Second</p>\n"
        '<p class="footnote">Note</p>\n'
        '<p class="table_footnote">Vis</p>\n'
    )
    # Equal keys fall back to geometry (y, then x).
    tied = [dict(d, read_order=1.0) for d in detections]
    layout = _onnx.find_layout(None, _runtime=_V3Runtime(tied))
    assert [block.bbox.y0 for block in layout] == [0.0, 25.0, 55.0, 75.0]
    # As soon as one detection lacks a key the geometric band order is used.
    mixed = [dict(d) for d in detections]
    del mixed[1]["read_order"]
    layout = _onnx.find_layout(None, _runtime=_V3Runtime(mixed))
    assert [block.bbox.y0 for block in layout] == [0.0, 25.0, 55.0, 75.0]
    expected = _onnx._reading_order(
        _onnx._layout_blocks(rendered, _V3Runtime(detections), _onnx.OnnxOptions())[
            ::-1
        ],
        rendered.page_bbox,
    )
    assert layout == [block for block, _ in expected]
    html = _onnx.get_layout_html(None, _runtime=_V3Runtime(mixed))
    assert html.startswith("<p>Second</p>\n<p>First</p>\n")
