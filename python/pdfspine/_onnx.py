"""Optional ONNX vision backend: DocLayout-YOLO layout + SLANet-plus tables.

The two networks only predict *where* things are (layout regions, table cells).
Every character of text comes from the PDF text layer through pdfspine's native
word coordinates; the models never regenerate text and no OCR is applied.

Runtime dependencies (``onnxruntime``, ``numpy``, ``Pillow``) are imported
lazily, so importing :mod:`pdfspine` has no ML dependency and never loads a
model. Model weights are not shipped in the wheel: they are resolved from
``PDFSPINE_ONNX_MODELS`` or explicit ``OnnxOptions`` paths at call time.

Pre/post-processing follows RapidAI's ``rapid_layout`` (DocLayout-YOLO) and
``rapid_table`` (SLANet-plus) reference implementations.
"""

from __future__ import annotations

import ast
from dataclasses import dataclass, fields
from html import escape
import math
import os
from pathlib import Path
import threading
from typing import Any, Mapping, Sequence

from ._core import PdfUnsupportedError
from ._tatr import (
    _RenderedPage,
    _TableCrop,
    _TatrTableFinderRecord,
    _TatrTableRecord,
    _area,
    _box4,
    _crop_box_to_page,
    _image_box_to_page,
    _intersection_area,
    _intersects_clip,
    _iob,
    _make_crop,
    _render_page,
)
from .geometry import Rect


MODELS_ENV = "PDFSPINE_ONNX_MODELS"
LAYOUT_MODEL_FILE = "doclayout_yolo_docstructbench_imgsz1024.onnx"
TABLE_MODEL_FILE = "slanet-plus.onnx"
LAYOUT_MODEL_URL = (
    "https://www.modelscope.cn/models/RapidAI/RapidLayout/resolve/v1.2.0/"
    "onnx/doclayout/doclayout_yolo_docstructbench_imgsz1024.onnx"
)
TABLE_MODEL_URL = "https://www.modelscope.cn/models/RapidAI/RapidTable/resolve/v2.0.0/slanet-plus.onnx"

# DocLayout-YOLO DocStructBench classes, in class-id order. The RapidAI export
# also embeds this list in the ONNX metadata (``names`` as a Python dict
# literal and ``character`` as one name per line); when present, the embedded
# list wins over this fallback.
LAYOUT_LABELS: tuple[str, ...] = (
    "title",
    "plain text",
    "abandon",
    "figure",
    "figure_caption",
    "table",
    "table_caption",
    "table_footnote",
    "isolate_formula",
    "formula_caption",
)

# SLANet-plus structure vocabulary. Source: RapidAI RapidTable v2.0.0
# ``slanet-plus.onnx`` metadata key ``character`` (48 entries, identical to
# PaddleOCR's ``table_structure_dict_ch.txt`` with ``merge_no_span_structure``).
# The decoder prepends ``<sos>`` and appends ``<eos>``, giving the 50 classes of
# the structure output. When the loaded model carries its own ``character``
# metadata that list is used instead.
SLANET_STRUCTURE_DICT: tuple[str, ...] = (
    "<thead>",
    "</thead>",
    "<tbody>",
    "</tbody>",
    "<tr>",
    "</tr>",
    "<td",
    ">",
    "</td>",
    *(f' colspan="{n}"' for n in range(2, 21)),
    *(f' rowspan="{n}"' for n in range(2, 21)),
    "<td></td>",
)
_SOS = "<sos>"
_EOS = "<eos>"
_TD_TOKENS = frozenset({"<td", "<td></td>", "<td>"})

_IMAGENET_MEAN = (0.485, 0.456, 0.406)
_IMAGENET_STD = (0.229, 0.224, 0.225)
_LETTERBOX_FILL = 114

_TEXT_LABELS = {
    "title": "h2",
    "plain text": "p",
}
_CLASSED_LABELS = {
    "table_caption",
    "figure_caption",
    "table_footnote",
}
_FORMULA_LABELS = {"isolate_formula", "formula_caption"}


@dataclass(frozen=True)
class OnnxOptions:
    """Validated options for the opt-in ONNX layout/table backend."""

    dpi: int = 144
    layout_model: str | None = None
    table_model: str | None = None
    providers: str | tuple[str, ...] = "auto"
    layout_size: int = 1024
    layout_threshold: float = 0.25
    layout_nms_iou: float | None = 0.7
    table_size: int = 488
    table_min_score: float = 0.0
    channel_order: str = "bgr"
    crop_padding: int = 10
    ocr_if_no_text: bool = True
    ocr_engine: str = "paddle"
    ocr_language: str = "eng"

    def __post_init__(self) -> None:
        for name in ("dpi", "layout_size", "table_size", "crop_padding"):
            if type(getattr(self, name)) is not int:
                raise TypeError(f"ONNX {name} must be an int")
        for name in ("layout_threshold", "table_min_score"):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                raise TypeError(f"ONNX {name} must be a real number")
            object.__setattr__(self, name, float(value))
        if self.layout_nms_iou is not None:
            value = self.layout_nms_iou
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                raise TypeError("ONNX layout_nms_iou must be a real number or None")
            object.__setattr__(self, "layout_nms_iou", float(value))
        for name in ("layout_model", "table_model"):
            value = getattr(self, name)
            if value is not None and not isinstance(value, (str, os.PathLike)):
                raise TypeError(f"ONNX {name} must be a path or None")
            if value is not None:
                object.__setattr__(self, name, os.fspath(value))
        for name in ("channel_order", "ocr_engine", "ocr_language"):
            if not isinstance(getattr(self, name), str):
                raise TypeError(f"ONNX {name} must be a string")
        object.__setattr__(self, "channel_order", self.channel_order.casefold().strip())
        if isinstance(self.providers, str):
            object.__setattr__(self, "providers", self.providers.strip())
        elif isinstance(self.providers, Sequence) and all(
            isinstance(item, str) for item in self.providers
        ):
            object.__setattr__(self, "providers", tuple(self.providers))
        else:
            raise TypeError("ONNX providers must be 'auto' or a sequence of strings")
        if type(self.ocr_if_no_text) is not bool:
            raise TypeError("ONNX ocr_if_no_text must be a bool")
        self._validate()

    @classmethod
    def from_mapping(cls, value: Mapping[str, object] | None) -> "OnnxOptions":
        if value is None:
            return cls()
        if not isinstance(value, Mapping):
            raise TypeError("vision_options must be a mapping or None")
        known = {field.name for field in fields(cls)}
        unknown = sorted((key for key in value if key not in known), key=str)
        if unknown:
            raise TypeError(
                "unknown ONNX option(s): " + ", ".join(str(k) for k in unknown)
            )
        return cls(**dict(value))  # type: ignore[arg-type]

    def _validate(self) -> None:
        if not 36 <= self.dpi <= 600:
            raise ValueError("ONNX dpi must be between 36 and 600")
        if self.layout_size < 32 or self.layout_size % 32:
            raise ValueError("ONNX layout_size must be a positive multiple of 32")
        if self.table_size < 32:
            raise ValueError("ONNX table_size must be at least 32 pixels")
        for name in ("layout_threshold", "table_min_score"):
            value = getattr(self, name)
            if not math.isfinite(value) or not 0.0 <= value <= 1.0:
                raise ValueError(f"ONNX {name} must be in [0, 1]")
        if self.layout_nms_iou is not None and not 0.0 <= self.layout_nms_iou <= 1.0:
            raise ValueError("ONNX layout_nms_iou must be in [0, 1] or None")
        if self.channel_order not in {"bgr", "rgb"}:
            raise ValueError("ONNX channel_order must be 'bgr' or 'rgb'")
        if not 0 <= self.crop_padding <= 20:
            raise ValueError("ONNX crop_padding must be between 0 and 20 pixels")
        if isinstance(self.providers, str) and self.providers != "auto":
            raise ValueError(
                "ONNX providers must be 'auto' or a sequence of provider names"
            )
        if not isinstance(self.providers, str) and not self.providers:
            raise ValueError("ONNX providers sequence must not be empty")
        if not self.ocr_engine.strip() or not self.ocr_language.strip():
            raise ValueError("ONNX OCR engine and language must not be empty")


@dataclass(frozen=True)
class LayoutBlock:
    """One layout region in page coordinates (pdfspine extra; not in PyMuPDF).

    ``label`` is a DocLayout-YOLO class name (``title``, ``plain text``,
    ``abandon``, ``figure``, ``figure_caption``, ``table``, ``table_caption``,
    ``table_footnote``, ``isolate_formula``, ``formula_caption``).
    """

    bbox: Rect
    label: str
    score: float


# --------------------------------------------------------------------------- #
# Model file resolution and runtime cache
# --------------------------------------------------------------------------- #
def _model_paths(options: OnnxOptions) -> tuple[Path, Path]:
    """Resolve the two model files (existence is checked when they are loaded)."""

    root = os.environ.get(MODELS_ENV)
    base = Path(root).expanduser() if root else None

    def resolve(explicit: str | None, filename: str) -> Path:
        if explicit:
            return Path(explicit).expanduser()
        if base is not None:
            return base / filename
        return Path(filename)

    return (
        resolve(options.layout_model, LAYOUT_MODEL_FILE),
        resolve(options.table_model, TABLE_MODEL_FILE),
    )


def _missing_model(kind: str, path: Path) -> PdfUnsupportedError:
    filename, url = (
        (LAYOUT_MODEL_FILE, LAYOUT_MODEL_URL)
        if kind == "layout"
        else (TABLE_MODEL_FILE, TABLE_MODEL_URL)
    )
    return PdfUnsupportedError(
        f"The ONNX {kind} model was not found at {os.fspath(path)!r}. Download "
        f"{filename} from {url} into a directory and point {MODELS_ENV} at it, "
        f"or pass vision_options={{'{kind}_model': '/path/to/{filename}'}}. "
        "Model weights are never included in the pdfspine wheel."
    )


def _missing_runtime(exc: BaseException) -> PdfUnsupportedError:
    return PdfUnsupportedError(
        "The ONNX vision backend needs the optional runtime. Install it with "
        "`pip install 'pdfspine[onnx]'` (onnxruntime, numpy, Pillow). The base "
        f"pdfspine wheel intentionally does not include it ({type(exc).__name__}: {exc})."
    )


def _resolve_providers(
    requested: str | tuple[str, ...], available: Sequence[str]
) -> tuple[str, ...]:
    """Pick execution providers: CUDA when installed, CPU otherwise."""

    if requested == "auto":
        if "CUDAExecutionProvider" in available:
            return ("CUDAExecutionProvider", "CPUExecutionProvider")
        return ("CPUExecutionProvider",)
    missing = [name for name in requested if name not in available]
    if missing:
        raise PdfUnsupportedError(
            "ONNX execution provider(s) not available in this onnxruntime build: "
            + ", ".join(missing)
            + f" (available: {', '.join(available) or 'none'})"
        )
    return tuple(requested)


def _labels_from_metadata(metadata: Mapping[str, str]) -> tuple[str, ...] | None:
    names = metadata.get("names")
    if names:
        try:
            parsed = ast.literal_eval(names)
        except (ValueError, SyntaxError):
            parsed = None
        if isinstance(parsed, dict) and parsed:
            return tuple(str(parsed[key]) for key in sorted(parsed))
        if isinstance(parsed, (list, tuple)) and parsed:
            return tuple(str(item) for item in parsed)
    character = metadata.get("character")
    if character:
        lines = [line for line in character.split("\n") if line]
        if lines:
            return tuple(lines)
    return None


class _OnnxRuntime:
    """Lazily created onnxruntime sessions for both models, shared per process."""

    def __init__(
        self,
        layout_path: Path,
        table_path: Path,
        providers: str | tuple[str, ...],
    ) -> None:
        try:
            import numpy
            import onnxruntime
            from PIL import Image  # noqa: F401  (validated here, used lazily)
        except (ImportError, ModuleNotFoundError, OSError) as exc:
            raise _missing_runtime(exc) from exc
        self._np = numpy
        self._ort = onnxruntime
        self._paths = {"layout": layout_path, "table": table_path}
        self._sessions: dict[str, Any] = {}
        self._lock = threading.Lock()
        self.providers = _resolve_providers(
            providers, tuple(onnxruntime.get_available_providers())
        )
        self._layout_labels: tuple[str, ...] = LAYOUT_LABELS
        self._structure_dict: tuple[str, ...] = SLANET_STRUCTURE_DICT
        self.metadata: dict[str, Any] = {
            "backend": "onnx",
            "layout_model": os.fspath(layout_path),
            "table_model": os.fspath(table_path),
            "providers": list(self.providers),
            "preprocessing": "doclayout-letterbox-rgb/slanet-plus-488-imagenet",
        }

    def _session(self, kind: str) -> Any:
        with self._lock:
            session = self._sessions.get(kind)
            if session is not None:
                return session
            path = self._paths[kind]
            if not path.is_file():
                raise _missing_model(kind, path)
            options = self._ort.SessionOptions()
            options.log_severity_level = 3
            session = self._ort.InferenceSession(
                os.fspath(path), sess_options=options, providers=list(self.providers)
            )
            custom = dict(session.get_modelmeta().custom_metadata_map)
            embedded = _labels_from_metadata(custom)
            if embedded is not None:
                if kind == "layout":
                    self._layout_labels = embedded
                else:
                    self._structure_dict = embedded
            self._sessions[kind] = session
            return session

    @property
    def layout_labels(self) -> tuple[str, ...]:
        return self._layout_labels

    @property
    def structure_dict(self) -> tuple[str, ...]:
        return self._structure_dict

    def detect_layout(self, image: Any, options: OnnxOptions) -> list[dict[str, Any]]:
        session = self._session("layout")
        np = self._np
        array, ratio, pad = _letterbox(image, options.layout_size, np)
        outputs = session.run(None, {session.get_inputs()[0].name: array})
        output = outputs[0]
        if output.ndim == 3:
            output = output[0]
        rows = output.tolist()
        return _decode_layout(
            rows,
            ratio,
            pad,
            image.size,
            options.layout_threshold,
            self._layout_labels,
            options.layout_nms_iou,
        )

    def recognize_table(
        self, image: Any, options: OnnxOptions
    ) -> tuple[list[str], list[list[float]], list[float]]:
        session = self._session("table")
        np = self._np
        array = _table_input(image, options.table_size, options.channel_order, np)
        outputs = session.run(None, {session.get_inputs()[0].name: array})
        bbox_preds, structure_probs = _split_table_outputs(outputs)
        scale = float(max(image.size))
        return _decode_structure(
            structure_probs[0].tolist(),
            bbox_preds[0].tolist(),
            self._structure_dict,
            scale,
        )


@dataclass(frozen=True)
class _RuntimeSpec:
    layout_path: str
    table_path: str
    providers: str | tuple[str, ...]


_MODEL_CACHE: dict[_RuntimeSpec, _OnnxRuntime] = {}
_MODEL_CACHE_LOCK = threading.Lock()


def _cached_runtime(spec: _RuntimeSpec) -> _OnnxRuntime:
    with _MODEL_CACHE_LOCK:
        cached = _MODEL_CACHE.get(spec)
        if cached is not None:
            return cached
        runtime = _OnnxRuntime(
            Path(spec.layout_path), Path(spec.table_path), spec.providers
        )
        if len(_MODEL_CACHE) >= 4:
            _MODEL_CACHE.pop(next(iter(_MODEL_CACHE)))
        _MODEL_CACHE[spec] = runtime
        return runtime


def clear_model_cache() -> None:
    """Drop cached onnxruntime sessions (mainly useful in tests)."""

    with _MODEL_CACHE_LOCK:
        _MODEL_CACHE.clear()


def _get_runtime(options: OnnxOptions) -> _OnnxRuntime:
    layout_path, table_path = _model_paths(options)
    return _cached_runtime(
        _RuntimeSpec(os.fspath(layout_path), os.fspath(table_path), options.providers)
    )


# --------------------------------------------------------------------------- #
# DocLayout-YOLO pre/post-processing
# --------------------------------------------------------------------------- #
def _letterbox(
    image: Any, size: int, np: Any
) -> tuple[Any, float, tuple[float, float]]:
    """Resize with a centred 114-grey letterbox and return NCHW float32 RGB."""

    from PIL import Image

    width, height = image.size
    ratio = min(size / width, size / height)
    new_width = max(1, round(width * ratio))
    new_height = max(1, round(height * ratio))
    left = int(round((size - new_width) / 2.0 - 0.1))
    top = int(round((size - new_height) / 2.0 - 0.1))
    resized = image.convert("RGB").resize((new_width, new_height), Image.BILINEAR)
    canvas = Image.new("RGB", (size, size), (_LETTERBOX_FILL,) * 3)
    canvas.paste(resized, (left, top))
    array = np.asarray(canvas, dtype=np.float32) / 255.0
    array = np.ascontiguousarray(array.transpose(2, 0, 1))[None]
    return array, ratio, (float(left), float(top))


def _nms(boxes: list[dict[str, Any]], iou_threshold: float) -> list[dict[str, Any]]:
    kept: list[dict[str, Any]] = []
    for candidate in sorted(boxes, key=lambda b: -float(b["score"])):
        suppressed = False
        for existing in kept:
            inter = _intersection_area(candidate["bbox"], existing["bbox"])
            union = _area(candidate["bbox"]) + _area(existing["bbox"]) - inter
            if union > 0 and inter / union >= iou_threshold:
                suppressed = True
                break
        if not suppressed:
            kept.append(candidate)
    return kept


def _decode_layout(
    rows: Sequence[Sequence[float]],
    ratio: float,
    pad: tuple[float, float],
    image_size: tuple[int, int],
    threshold: float,
    labels: Sequence[str],
    nms_iou: float | None = None,
) -> list[dict[str, Any]]:
    """Decode YOLOv10 end-to-end rows ``[x0, y0, x1, y1, conf, cls]``."""

    width, height = image_size
    results: list[dict[str, Any]] = []
    for row in rows:
        if len(row) < 6:
            continue
        score = float(row[4])
        if not math.isfinite(score) or score < threshold:
            continue
        class_id = int(row[5])
        label = labels[class_id] if 0 <= class_id < len(labels) else str(class_id)
        x0 = (float(row[0]) - pad[0]) / ratio
        y0 = (float(row[1]) - pad[1]) / ratio
        x1 = (float(row[2]) - pad[0]) / ratio
        y1 = (float(row[3]) - pad[1]) / ratio
        bbox = (
            max(0.0, min(float(width), x0)),
            max(0.0, min(float(height), y0)),
            max(0.0, min(float(width), x1)),
            max(0.0, min(float(height), y1)),
        )
        if _area(bbox) <= 0:
            continue
        results.append({"label": label, "score": score, "bbox": bbox})
    if nms_iou is not None:
        results = _nms(results, nms_iou)
    results.sort(key=lambda item: -float(item["score"]))
    return results


# --------------------------------------------------------------------------- #
# SLANet-plus pre/post-processing
# --------------------------------------------------------------------------- #
def _table_input(image: Any, size: int, channel_order: str, np: Any) -> Any:
    """Resize the long side to ``size``, ImageNet-normalise, zero-pad, NCHW."""

    from PIL import Image

    width, height = image.size
    ratio = size / max(width, height)
    new_width = max(1, int(width * ratio))
    new_height = max(1, int(height * ratio))
    resized = image.convert("RGB").resize((new_width, new_height), Image.BILINEAR)
    array = np.asarray(resized, dtype=np.float32) / 255.0
    if channel_order == "bgr":
        array = array[:, :, ::-1]
    mean = np.asarray(_IMAGENET_MEAN, dtype=np.float32)
    std = np.asarray(_IMAGENET_STD, dtype=np.float32)
    array = (array - mean) / std
    canvas = np.zeros((size, size, 3), dtype=np.float32)
    canvas[:new_height, :new_width, :] = array
    return np.ascontiguousarray(canvas.transpose(2, 0, 1))[None]


def _split_table_outputs(outputs: Sequence[Any]) -> tuple[Any, Any]:
    """Identify (bbox_preds, structure_probs) by their last dimension, not index."""

    bbox_preds = None
    structure_probs = None
    for output in outputs:
        last = int(output.shape[-1])
        if last == 8 and bbox_preds is None:
            bbox_preds = output
        elif last > 8 and structure_probs is None:
            structure_probs = output
    if bbox_preds is None or structure_probs is None:
        shapes = [tuple(int(v) for v in output.shape) for output in outputs]
        raise PdfUnsupportedError(
            "Unexpected SLANet-plus outputs; expected one [*, *, 8] bbox tensor and "
            f"one [*, *, N] structure tensor, got shapes {shapes}"
        )
    return bbox_preds, structure_probs


def _decode_structure(
    structure_probs: Sequence[Sequence[float]],
    bbox_preds: Sequence[Sequence[float]],
    dictionary: Sequence[str],
    scale: float,
) -> tuple[list[str], list[list[float]], list[float]]:
    """Greedy-decode structure tokens; one axis-aligned box per ``<td`` token.

    ``structure_probs`` is ``[L, len(dictionary) + 2]`` (``<sos>`` first,
    ``<eos>`` last), ``bbox_preds`` is ``[L, 8]`` normalised quad corners.
    Boxes are returned in crop pixels (``normalised * scale``).
    """

    characters = [_SOS, *dictionary, _EOS]
    eos_index = len(characters) - 1
    tokens: list[str] = []
    boxes: list[list[float]] = []
    scores: list[float] = []
    for step, probs in enumerate(structure_probs):
        best = 0
        best_prob = float(probs[0]) if probs else 0.0
        for index, prob in enumerate(probs):
            value = float(prob)
            if value > best_prob:
                best, best_prob = index, value
        if best == eos_index and step > 0:
            break
        if best == 0 or best >= len(characters):
            continue
        token = characters[best]
        if token in _TD_TOKENS:
            quad = (
                [float(v) for v in bbox_preds[step]] if step < len(bbox_preds) else []
            )
            boxes.append(_quad_to_rect(quad, scale))
        tokens.append(token)
        scores.append(best_prob)
    return tokens, boxes, scores


def _quad_to_rect(quad: Sequence[float], scale: float) -> list[float]:
    if len(quad) >= 8:
        xs = [float(quad[i]) * scale for i in range(0, 8, 2)]
        ys = [float(quad[i]) * scale for i in range(1, 8, 2)]
    elif len(quad) == 4:
        xs = [float(quad[0]) * scale, float(quad[2]) * scale]
        ys = [float(quad[1]) * scale, float(quad[3]) * scale]
    else:
        return [0.0, 0.0, 0.0, 0.0]
    return [min(xs), min(ys), max(xs), max(ys)]


def _span_value(token: str, name: str) -> int | None:
    prefix = f' {name}="'
    if token.startswith(prefix) and token.endswith('"'):
        digits = token[len(prefix) : -1]
        if digits.isdecimal():
            return max(1, int(digits))
    return None


def _structure_to_cells(
    tokens: Sequence[str],
    boxes: Sequence[Sequence[float]],
    scores: Sequence[float] | None = None,
) -> list[dict[str, Any]]:
    """Turn the HTML structure tokens into grid cells with row/column spans."""

    cells: list[dict[str, Any]] = []
    occupied: set[tuple[int, int]] = set()
    row = -1
    column = 0
    in_header = False
    box_index = 0
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token == "<thead>":
            in_header = True
        elif token == "</thead>":
            in_header = False
        elif token == "<tr>":
            row += 1
            column = 0
        elif token in _TD_TOKENS:
            colspan = rowspan = 1
            score = (
                float(scores[index])
                if scores is not None and index < len(scores)
                else 1.0
            )
            if token == "<td":
                cursor = index + 1
                while cursor < len(tokens) and tokens[cursor] != ">":
                    attribute = tokens[cursor]
                    value = _span_value(attribute, "colspan")
                    if value is not None:
                        colspan = value
                    value = _span_value(attribute, "rowspan")
                    if value is not None:
                        rowspan = value
                    cursor += 1
                index = cursor
            if row < 0:
                row = 0
            while (row, column) in occupied:
                column += 1
            rows = list(range(row, row + rowspan))
            columns = list(range(column, column + colspan))
            occupied.update((r, c) for r in rows for c in columns)
            bbox = list(boxes[box_index]) if box_index < len(boxes) else None
            box_index += 1
            cells.append(
                {
                    "row_nums": rows,
                    "column_nums": columns,
                    "bbox": bbox,
                    "header": in_header,
                    "score": score,
                    "cell_text": "",
                }
            )
            column += colspan
        index += 1
    return [
        cell for cell in cells if cell["bbox"] is not None and _area(cell["bbox"]) > 0
    ]


# --------------------------------------------------------------------------- #
# Text-layer words -> cells / blocks
# --------------------------------------------------------------------------- #
def _group_lines(words: list[dict[str, Any]]) -> list[list[dict[str, Any]]]:
    """Cluster words into visual lines by vertical overlap, left-to-right."""

    ordered = sorted(
        words, key=lambda w: ((w["bbox"][1] + w["bbox"][3]) / 2.0, w["bbox"][0])
    )
    lines: list[list[dict[str, Any]]] = []
    for word in ordered:
        y0, y1 = word["bbox"][1], word["bbox"][3]
        if lines:
            line = lines[-1]
            ly0 = min(w["bbox"][1] for w in line)
            ly1 = max(w["bbox"][3] for w in line)
            overlap = min(y1, ly1) - max(y0, ly0)
            if overlap > 0.5 * min(y1 - y0, ly1 - ly0):
                line.append(word)
                continue
        lines.append([word])
    for line in lines:
        line.sort(key=lambda w: w["bbox"][0])
    return lines


def _lines_text(words: list[dict[str, Any]]) -> list[str]:
    return [" ".join(str(w["text"]) for w in line) for line in _group_lines(words)]


def _rect_distance(point: tuple[float, float], rect: Sequence[float]) -> float:
    dx = max(rect[0] - point[0], 0.0, point[0] - rect[2])
    dy = max(rect[1] - point[1], 0.0, point[1] - rect[3])
    return math.hypot(dx, dy)


def _assign_words(
    cells: list[dict[str, Any]], tokens: Sequence[Mapping[str, Any]]
) -> float:
    """Assign each text-layer word to the cell it overlaps most; fill ``cell_text``.

    Returns the fraction of words that landed in a cell (0.0 when none).
    """

    buckets: dict[int, list[dict[str, Any]]] = {}
    assigned = 0
    for token in tokens:
        tb = _box4(token["bbox"])
        token_area = _area(tb)
        best_index = -1
        best_overlap = 0.0
        for index, cell in enumerate(cells):
            overlap = _intersection_area(tb, cell["bbox"])
            if overlap > best_overlap:
                best_index, best_overlap = index, overlap
        if best_index < 0 or token_area <= 0 or best_overlap / token_area < 0.5:
            cx = (tb[0] + tb[2]) / 2.0
            cy = (tb[1] + tb[3]) / 2.0
            best_index = -1
            for index, cell in enumerate(cells):
                bx0, by0, bx1, by1 = cell["bbox"]
                if bx0 <= cx <= bx1 and by0 <= cy <= by1:
                    best_index = index
                    break
            if best_index < 0 and cells:
                # Every crop word belongs to the table: never drop text-layer
                # words, fall back to the nearest cell (rect distance).
                best_index = min(
                    range(len(cells)),
                    key=lambda i: _rect_distance((cx, cy), cells[i]["bbox"]),
                )
        if best_index < 0:
            continue
        assigned += 1
        buckets.setdefault(best_index, []).append(dict(token))
    for index, cell in enumerate(cells):
        cell["cell_text"] = "\n".join(_lines_text(buckets.get(index, [])))
    return assigned / len(tokens) if tokens else 0.0


def _grid_boxes(
    cells: Sequence[Mapping[str, Any]],
) -> tuple[
    list[tuple[float, float, float, float]], list[tuple[float, float, float, float]]
]:
    """Approximate row/column bands from the union of cell boxes.

    SLANet-plus predicts cells only, so ``Table.rows`` / ``Table.cols`` are
    derived from single-span cells (falling back to any cell touching the
    index). ``cells`` / ``spans`` / ``to_html()`` do not depend on this.
    """

    if not cells:
        return [], []
    row_count = max(max(c["row_nums"]) for c in cells) + 1
    col_count = max(max(c["column_nums"]) for c in cells) + 1
    all_x0 = min(c["bbox"][0] for c in cells)
    all_x1 = max(c["bbox"][2] for c in cells)
    all_y0 = min(c["bbox"][1] for c in cells)
    all_y1 = max(c["bbox"][3] for c in cells)
    row_boxes: list[tuple[float, float, float, float]] = []
    for row in range(row_count):
        single = [c for c in cells if list(c["row_nums"]) == [row]]
        source = single or [c for c in cells if row in c["row_nums"]]
        if not source:
            continue
        row_boxes.append(
            (
                all_x0,
                min(c["bbox"][1] for c in source),
                all_x1,
                max(c["bbox"][3] for c in source),
            )
        )
    column_boxes: list[tuple[float, float, float, float]] = []
    for column in range(col_count):
        single = [c for c in cells if list(c["column_nums"]) == [column]]
        source = single or [c for c in cells if column in c["column_nums"]]
        if not source:
            continue
        column_boxes.append(
            (
                min(c["bbox"][0] for c in source),
                all_y0,
                max(c["bbox"][2] for c in source),
                all_y1,
            )
        )
    return row_boxes, column_boxes


class _OnnxTableRecord(_TatrTableRecord):
    """A :class:`_TatrTableRecord` whose ``source`` is ``"onnx"``."""

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self.source = "onnx"


def _table_from_region(
    detection: Mapping[str, Any],
    rendered: _RenderedPage,
    runtime: Any,
    options: OnnxOptions,
) -> _OnnxTableRecord | None:
    crop: _TableCrop | None = _make_crop(
        rendered, {"bbox": detection["bbox"], "label": "table"}, options.crop_padding
    )
    if crop is None:
        return None
    tokens, boxes, scores = runtime.recognize_table(crop.image, options)
    cells = _structure_to_cells(tokens, boxes, scores)
    if not cells:
        return None
    structure_score = sum(scores) / len(scores) if scores else 0.0
    if structure_score < options.table_min_score:
        return None
    _assign_words(cells, crop.tokens)
    for cell in cells:
        cell["bbox"] = _crop_box_to_page(cell["bbox"], crop, rendered)
    row_boxes, column_boxes = _grid_boxes(cells)
    confidence = min(float(detection.get("score", 0.0)), structure_score)
    metadata = dict(getattr(runtime, "metadata", {"backend": "onnx"}))
    metadata.update(
        {
            "detection_bbox": list(
                _image_box_to_page(_box4(detection["bbox"]), rendered)
            ),
            "recognition_crop_bbox": list(
                _image_box_to_page(_box4(crop.bbox), rendered)
            ),
            "geometry_source": "slanet-plus-cells",
            "structure_tokens": len(tokens),
            "page_rotation": rendered.rotation,
        }
    )
    record = _OnnxTableRecord(
        cells,
        row_boxes,
        column_boxes,
        confidence,
        rendered.text_source,
        metadata,
        rotated=bool(rendered.rotation % 180),
    )
    return record if record.spans else None


# --------------------------------------------------------------------------- #
# Public entry points
# --------------------------------------------------------------------------- #
def _prepare(
    page: Any, options: Mapping[str, object] | None, runtime: Any
) -> tuple[OnnxOptions, Any, _RenderedPage]:
    config = OnnxOptions.from_mapping(options)
    resolved = runtime if runtime is not None else _get_runtime(config)
    rendered = _render_page(page, config)
    return config, resolved, rendered


def find_tables(
    page: Any,
    *,
    clip: Any = None,
    options: Mapping[str, object] | None = None,
    _runtime: Any = None,
) -> _TatrTableFinderRecord:
    """DocLayout-YOLO table regions -> SLANet-plus cells -> text-layer words."""

    config, runtime, rendered = _prepare(page, options, _runtime)
    if _area(rendered.page_bbox) <= 0:
        return _TatrTableFinderRecord([])
    tables: list[_TatrTableRecord] = []
    for detection in runtime.detect_layout(rendered.image, config):
        if detection.get("label") != "table":
            continue
        table = _table_from_region(detection, rendered, runtime, config)
        if table is not None and _intersects_clip(table, clip):
            tables.append(table)
    tables.sort(key=lambda table: (table.bbox[1], table.bbox[0]))
    return _TatrTableFinderRecord(tables)


def _reading_order(
    blocks: list[tuple[LayoutBlock, dict[str, Any]]],
    page_bbox: tuple[float, float, float, float],
) -> list[tuple[LayoutBlock, dict[str, Any]]]:
    """Order blocks: full-width blocks split the page into bands; inside a band
    the left column precedes the right column, each top-to-bottom.

    TODO: replace with a recursive XY-cut for pages with three or more
    columns and for column layouts that change mid-page.
    """

    width = page_bbox[2] - page_bbox[0]
    if width <= 0:
        return sorted(blocks, key=lambda item: (item[0].bbox.y0, item[0].bbox.x0))
    mid = page_bbox[0] + width / 2.0

    def spans_page(block: LayoutBlock) -> bool:
        rect = block.bbox
        centre = (rect.x0 + rect.x1) / 2.0
        return (rect.x1 - rect.x0) > 0.6 * width or abs(centre - mid) < 0.08 * width

    def column(block: LayoutBlock) -> int:
        return 0 if (block.bbox.x0 + block.bbox.x1) / 2.0 < mid else 1

    ordered: list[tuple[LayoutBlock, dict[str, Any]]] = []
    band: list[tuple[LayoutBlock, dict[str, Any]]] = []

    def flush() -> None:
        band.sort(key=lambda item: (column(item[0]), item[0].bbox.y0, item[0].bbox.x0))
        ordered.extend(band)
        band.clear()

    for item in sorted(blocks, key=lambda item: (item[0].bbox.y0, item[0].bbox.x0)):
        if spans_page(item[0]):
            flush()
            ordered.append(item)
        else:
            band.append(item)
    flush()
    return ordered


def _layout_blocks(
    rendered: _RenderedPage, runtime: Any, config: OnnxOptions
) -> list[tuple[LayoutBlock, dict[str, Any]]]:
    blocks: list[tuple[LayoutBlock, dict[str, Any]]] = []
    for detection in runtime.detect_layout(rendered.image, config):
        page_box = _image_box_to_page(_box4(detection["bbox"]), rendered)
        if _area(page_box) <= 0:
            continue
        block = LayoutBlock(
            Rect(*page_box), str(detection["label"]), float(detection.get("score", 0.0))
        )
        blocks.append((block, dict(detection)))
    return _reading_order(blocks, rendered.page_bbox)


def find_layout(
    page: Any,
    *,
    options: Mapping[str, object] | None = None,
    _runtime: Any = None,
) -> list[LayoutBlock]:
    """Detect layout regions with DocLayout-YOLO, in reading order (page points)."""

    config, runtime, rendered = _prepare(page, options, _runtime)
    if _area(rendered.page_bbox) <= 0:
        return []
    return [block for block, _ in _layout_blocks(rendered, runtime, config)]


def _page_words(rendered: _RenderedPage) -> list[dict[str, Any]]:
    words: list[dict[str, Any]] = []
    for token in rendered.tokens:
        copied = dict(token)
        copied["bbox"] = list(_image_box_to_page(_box4(token["bbox"]), rendered))
        words.append(copied)
    return words


def _block_words(
    bbox: Rect, words: Sequence[Mapping[str, Any]], used: set[int]
) -> list[dict[str, Any]]:
    selected: list[dict[str, Any]] = []
    for index, word in enumerate(words):
        if index in used:
            continue
        x0, y0, x1, y1 = word["bbox"]
        cx, cy = (x0 + x1) / 2.0, (y0 + y1) / 2.0
        if bbox.x0 <= cx <= bbox.x1 and bbox.y0 <= cy <= bbox.y1:
            used.add(index)
            selected.append(dict(word))
    return selected


def _bbox_attr(rect: Rect) -> str:
    return " ".join(f"{value:.2f}" for value in (rect.x0, rect.y0, rect.x1, rect.y1))


def get_layout_html(
    page: Any,
    *,
    options: Mapping[str, object] | None = None,
    _runtime: Any = None,
) -> str:
    """Semantic HTML for one page; every character comes from the text layer."""

    config, runtime, rendered = _prepare(page, options, _runtime)
    if _area(rendered.page_bbox) <= 0:
        return ""
    words = _page_words(rendered)
    used: set[int] = set()
    ordered = _layout_blocks(rendered, runtime, config)
    # Tables fill their cells geometrically from every word of their recognition
    # crop, so they claim those words first; an overlapping text block never
    # repeats them.
    tables: dict[int, _OnnxTableRecord | None] = {}
    table_words: dict[int, list[dict[str, Any]]] = {}
    for index, (block, detection) in enumerate(ordered):
        if block.label != "table":
            continue
        table = _table_from_region(detection, rendered, runtime, config)
        tables[index] = table
        if table is None:
            table_words[index] = _block_words(block.bbox, words, used)
            continue
        crop_bbox = _box4(table.metadata["recognition_crop_bbox"])
        used.update(
            i for i, word in enumerate(words) if _iob(word["bbox"], crop_bbox) >= 0.5
        )
    parts: list[str] = []
    for index, (block, detection) in enumerate(ordered):
        label = block.label
        if label == "abandon":
            _block_words(block.bbox, words, used)
            continue
        if label == "table":
            table = tables[index]
            if table is not None:
                parts.append(table.to_html())
            elif table_words[index]:
                text = "\n".join(_lines_text(table_words[index]))
                parts.append(f'<pre class="table">{escape(text)}</pre>')
            continue
        if label == "figure":
            _block_words(block.bbox, words, used)
            parts.append(f'<figure data-bbox="{_bbox_attr(block.bbox)}"></figure>')
            continue
        block_words = _block_words(block.bbox, words, used)
        if not block_words:
            continue
        text = escape(" ".join(_lines_text(block_words)))
        if label in _TEXT_LABELS:
            tag = _TEXT_LABELS[label]
            parts.append(f"<{tag}>{text}</{tag}>")
        elif label in _FORMULA_LABELS:
            parts.append(f'<p class="formula">{text}</p>')
        else:
            css = label if label in _CLASSED_LABELS else label.replace(" ", "_")
            parts.append(f'<p class="{escape(css)}">{text}</p>')
    # Text-layer words outside every detected block are never dropped: they
    # are appended verbatim so downstream consumers keep the full page text.
    leftover = [word for index, word in enumerate(words) if index not in used]
    if leftover:
        text = "\n".join(_lines_text(leftover))
        parts.append(f'<pre class="unclaimed">{escape(text)}</pre>')
    return "\n".join(parts) + ("\n" if parts else "")


__all__ = [
    "LAYOUT_LABELS",
    "LAYOUT_MODEL_FILE",
    "LAYOUT_MODEL_URL",
    "MODELS_ENV",
    "SLANET_STRUCTURE_DICT",
    "TABLE_MODEL_FILE",
    "TABLE_MODEL_URL",
    "LayoutBlock",
    "OnnxOptions",
    "clear_model_cache",
    "find_layout",
    "find_tables",
    "get_layout_html",
]
