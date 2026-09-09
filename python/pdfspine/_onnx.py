"""Optional ONNX vision backend: PP-DocLayout layout + SLANet-plus tables.

The two networks only predict *where* things are (layout regions, table cells).
Every character of text comes from the PDF text layer through pdfspine's native
word coordinates; the models never regenerate text and no OCR is applied.

Both models are Apache-2.0 (PaddleX/PaddleOCR upstream, ONNX exports published
by RapidAI). The layout detector is a PP-DocLayout RT-DETR: ``PP-DocLayoutV3``
(25 classes, 800x800, a per-box reading-order key and a dedicated
``vision_footnote`` class) by default, with the faster ``PP-DocLayout-L``
(23 classes, 640x640) available through ``OnnxOptions.layout_variant``. The
YOLO-based detector used before 2026-09-08
was dropped because its upstream repository, PyPI package and ONNX metadata all
declare AGPL-3.0; the evidence is in ``docs/table-structure-models-survey.md``
section 4.1.

Runtime dependencies (``onnxruntime``, ``numpy``, ``Pillow``) are imported
lazily, so importing :mod:`pdfspine` has no ML dependency and never loads a
model. Model weights are not shipped in the wheel: they are resolved from
``PDFSPINE_ONNX_MODELS`` or explicit ``OnnxOptions`` paths at call time.

Pre/post-processing follows RapidAI's ``rapid_layout`` (PP-DocLayout) and
``rapid_table`` (SLANet-plus) reference implementations: the page is resized to
the model's square input, scaled to ``[0, 1]`` RGB (no mean/std), and fed as
``image`` / ``im_shape`` / ``scale_factor``; the RT-DETR head returns
``(class_id, score, x0, y0, x1, y1)`` rows already in original-image pixels.
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

# The two PP-DocLayout variants (PaddleX RT-DETR heads, Apache-2.0). ``auto``
# picks the variant from the model file name, defaulting to PP-DocLayoutV3.
LAYOUT_VARIANTS: tuple[str, ...] = ("pp_doclayout_l", "pp_doclayoutv3")
LAYOUT_MODEL_FILES: dict[str, str] = {
    "pp_doclayout_l": "pp_doclayout_l.onnx",
    "pp_doclayoutv3": "pp_doc_layoutv3.onnx",
}
LAYOUT_MODEL_URLS: dict[str, str] = {
    "pp_doclayout_l": (
        "https://www.modelscope.cn/models/RapidAI/RapidDoc/resolve/v1.0.0/"
        "layout/PP-DocLayout-L/pp_doclayout_l.onnx"
    ),
    "pp_doclayoutv3": (
        "https://www.modelscope.cn/models/RapidAI/RapidLayout/resolve/v1.2.0/"
        "onnx/pp_doc_layout/pp_doc_layoutv3.onnx"
    ),
}
# Square input edge each variant was exported with. The real value is read back
# from the session's static ``image`` input shape; this is the fallback used
# before the session exists and for exports with a dynamic spatial dimension.
LAYOUT_INPUT_SIZES: dict[str, int] = {"pp_doclayout_l": 640, "pp_doclayoutv3": 800}
DEFAULT_LAYOUT_VARIANT = "pp_doclayoutv3"
LAYOUT_MODEL_FILE = LAYOUT_MODEL_FILES[DEFAULT_LAYOUT_VARIANT]
LAYOUT_MODEL_URL = LAYOUT_MODEL_URLS[DEFAULT_LAYOUT_VARIANT]

TABLE_MODEL_FILE = "slanet-plus.onnx"
TABLE_MODEL_URL = "https://www.modelscope.cn/models/RapidAI/RapidTable/resolve/v2.0.0/slanet-plus.onnx"

# PP-DocLayout-L classes, in class-id order. The RapidAI export also embeds this
# list in the ONNX metadata (``character``, one name per line); when present,
# the embedded list wins over this fallback.
PP_DOCLAYOUT_L_LABELS: tuple[str, ...] = (
    "paragraph_title",
    "image",
    "text",
    "number",
    "abstract",
    "content",
    "figure_title",
    "formula",
    "table",
    "table_title",
    "reference",
    "doc_title",
    "footnote",
    "header",
    "algorithm",
    "footer",
    "seal",
    "chart_title",
    "chart",
    "formula_number",
    "header_image",
    "footer_image",
    "aside_text",
)

# PP-DocLayoutV3 classes, in class-id order (alphabetical upstream). Adds
# ``vision_footnote`` (a note attached to a table/figure) and splits formulas
# into display/inline; it has no ``table_title`` — table and figure captions
# share ``figure_title``.
PP_DOCLAYOUTV3_LABELS: tuple[str, ...] = (
    "abstract",
    "algorithm",
    "aside_text",
    "chart",
    "content",
    "display_formula",
    "doc_title",
    "figure_title",
    "footer",
    "footer_image",
    "footnote",
    "formula_number",
    "header",
    "header_image",
    "image",
    "inline_formula",
    "number",
    "paragraph_title",
    "reference",
    "reference_content",
    "seal",
    "table",
    "text",
    "vertical_text",
    "vision_footnote",
)

# Back-compatible alias: the default variant's class list.
LAYOUT_LABELS: tuple[str, ...] = PP_DOCLAYOUTV3_LABELS

LAYOUT_LABELS_BY_VARIANT: dict[str, tuple[str, ...]] = {
    "pp_doclayout_l": PP_DOCLAYOUT_L_LABELS,
    "pp_doclayoutv3": PP_DOCLAYOUTV3_LABELS,
}

# PP-DocLayout class -> pdfspine layout label. The downstream vocabulary
# (`LayoutBlock.label`, `get_layout_html()`) is unchanged, so the HTML mapping
# and every consumer keep working; ``LayoutBlock.raw_label`` keeps the model's
# own class name. Anything not listed passes through unchanged.
LAYOUT_LABEL_MAP: dict[str, str] = {
    # body text and text-like regions
    "text": "plain text",
    "abstract": "plain text",
    "content": "plain text",
    "reference": "plain text",
    "reference_content": "plain text",
    "aside_text": "plain text",
    "algorithm": "plain text",
    "vertical_text": "plain text",
    # headings
    "paragraph_title": "title",
    "doc_title": "title",
    # graphics
    "image": "figure",
    "chart": "figure",
    "seal": "figure",
    "figure_title": "figure_caption",
    "chart_title": "figure_caption",
    # tables
    "table": "table",
    "table_title": "table_caption",
    # PP-DocLayout-L has no separate table footnote class; ``footnote`` is the
    # closest approximation (overridden for PP-DocLayoutV3 below).
    "footnote": "table_footnote",
    "vision_footnote": "table_footnote",
    # furniture that never carries body text
    "header": "abandon",
    "footer": "abandon",
    "number": "abandon",
    "header_image": "abandon",
    "footer_image": "abandon",
    # formulas
    "formula": "isolate_formula",
    "display_formula": "isolate_formula",
    "inline_formula": "isolate_formula",
    "formula_number": "formula_caption",
}

# PP-DocLayoutV3 has a dedicated ``vision_footnote`` for notes attached to a
# table or figure, so its ``footnote`` really is a page footnote: keep it as
# body text (rendered with a ``footnote`` CSS class).
LAYOUT_LABEL_MAP_V3: dict[str, str] = {**LAYOUT_LABEL_MAP, "footnote": "plain text"}

LAYOUT_LABEL_MAPS: dict[str, dict[str, str]] = {
    "pp_doclayout_l": LAYOUT_LABEL_MAP,
    "pp_doclayoutv3": LAYOUT_LABEL_MAP_V3,
}

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
# Raw model classes that keep their own CSS class even though they normalise to
# a generic label (PP-DocLayoutV3 page footnotes become ``plain text``).
_RAW_CSS_LABELS = {"footnote"}


@dataclass(frozen=True)
class OnnxOptions:
    """Validated options for the opt-in ONNX layout/table backend."""

    dpi: int = 144
    layout_model: str | None = None
    table_model: str | None = None
    providers: str | tuple[str, ...] = "auto"
    layout_variant: str = "auto"
    layout_size: int | None = None
    layout_threshold: float = 0.5
    layout_nms_iou: float | None = 0.6
    table_size: int = 488
    table_min_score: float = 0.0
    channel_order: str = "bgr"
    crop_padding: int = 10
    ocr_if_no_text: bool = True
    ocr_engine: str = "paddle"
    ocr_language: str = "eng"

    def __post_init__(self) -> None:
        for name in ("dpi", "table_size", "crop_padding"):
            if type(getattr(self, name)) is not int:
                raise TypeError(f"ONNX {name} must be an int")
        if self.layout_size is not None and type(self.layout_size) is not int:
            raise TypeError("ONNX layout_size must be an int or None")
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
        for name in ("channel_order", "layout_variant", "ocr_engine", "ocr_language"):
            if not isinstance(getattr(self, name), str):
                raise TypeError(f"ONNX {name} must be a string")
        object.__setattr__(self, "channel_order", self.channel_order.casefold().strip())
        object.__setattr__(
            self, "layout_variant", self.layout_variant.casefold().strip()
        )
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
        if self.layout_size is not None and (
            self.layout_size < 32 or self.layout_size % 32
        ):
            raise ValueError("ONNX layout_size must be a positive multiple of 32")
        if self.layout_variant not in ("auto", *LAYOUT_VARIANTS):
            raise ValueError(
                "ONNX layout_variant must be 'auto' or one of "
                + ", ".join(repr(name) for name in LAYOUT_VARIANTS)
            )
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

    ``label`` is pdfspine's normalised class: ``title``, ``plain text``,
    ``abandon``, ``figure``, ``figure_caption``, ``table``, ``table_caption``,
    ``table_footnote``, ``isolate_formula`` or ``formula_caption``.
    ``raw_label`` is the PP-DocLayout class the model actually predicted (for
    example ``paragraph_title``, ``aside_text``, ``vision_footnote``); see
    :data:`LAYOUT_LABEL_MAP` for the mapping. It equals ``label`` when the model
    class needs no translation.
    """

    bbox: Rect
    label: str
    score: float
    raw_label: str = ""

    def __post_init__(self) -> None:
        if not self.raw_label:
            object.__setattr__(self, "raw_label", self.label)


# --------------------------------------------------------------------------- #
# Model file resolution and runtime cache
# --------------------------------------------------------------------------- #
def _variant_from_name(name: str) -> str:
    """Guess the PP-DocLayout variant from a model file name."""

    stem = os.path.basename(name).casefold()
    if "v3" in stem:
        return "pp_doclayoutv3"
    if "doclayout_l" in stem or "doclayout-l" in stem:
        return "pp_doclayout_l"
    return DEFAULT_LAYOUT_VARIANT


def _layout_variant(options: OnnxOptions) -> str:
    """Resolve ``layout_variant``: explicit value, else the file name, else V3."""

    if options.layout_variant != "auto":
        return options.layout_variant
    if options.layout_model:
        return _variant_from_name(options.layout_model)
    return DEFAULT_LAYOUT_VARIANT


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
        resolve(options.layout_model, LAYOUT_MODEL_FILES[_layout_variant(options)]),
        resolve(options.table_model, TABLE_MODEL_FILE),
    )


def _missing_model(
    kind: str, path: Path, variant: str = DEFAULT_LAYOUT_VARIANT
) -> PdfUnsupportedError:
    filename, url = (
        (LAYOUT_MODEL_FILES[variant], LAYOUT_MODEL_URLS[variant])
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
        layout_variant: str = DEFAULT_LAYOUT_VARIANT,
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
        self.layout_variant = layout_variant
        self._layout_labels: tuple[str, ...] = LAYOUT_LABELS_BY_VARIANT.get(
            layout_variant, PP_DOCLAYOUTV3_LABELS
        )
        self._label_map: dict[str, str] = LAYOUT_LABEL_MAPS.get(
            layout_variant, LAYOUT_LABEL_MAP_V3
        )
        self._layout_size = LAYOUT_INPUT_SIZES.get(layout_variant, 800)
        self._structure_dict: tuple[str, ...] = SLANET_STRUCTURE_DICT
        self.metadata: dict[str, Any] = {
            "backend": "onnx",
            "layout_model": os.fspath(layout_path),
            "layout_variant": layout_variant,
            "table_model": os.fspath(table_path),
            "providers": list(self.providers),
            "preprocessing": (
                f"pp-doclayout-{self._layout_size}-rgb/slanet-plus-488-imagenet"
            ),
        }

    def _session(self, kind: str) -> Any:
        with self._lock:
            session = self._sessions.get(kind)
            if session is not None:
                return session
            path = self._paths[kind]
            if not path.is_file():
                raise _missing_model(kind, path, self.layout_variant)
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
            if kind == "layout":
                size = _input_edge(session)
                if size is not None:
                    self._layout_size = size
                    self.metadata["preprocessing"] = (
                        f"pp-doclayout-{size}-rgb/slanet-plus-488-imagenet"
                    )
            self._sessions[kind] = session
            return session

    @property
    def layout_labels(self) -> tuple[str, ...]:
        return self._layout_labels

    @property
    def layout_label_map(self) -> dict[str, str]:
        return self._label_map

    @property
    def structure_dict(self) -> tuple[str, ...]:
        return self._structure_dict

    def detect_layout(self, image: Any, options: OnnxOptions) -> list[dict[str, Any]]:
        session = self._session("layout")
        np = self._np
        size = options.layout_size or self._layout_size
        array, scale_factor = _layout_input(image, size, np)
        names = {spec.name for spec in session.get_inputs()}
        if "image" not in names:
            raise PdfUnsupportedError(
                "Unexpected PP-DocLayout inputs; expected an 'image' input, got "
                + (", ".join(sorted(names)) or "none")
            )
        feeds: dict[str, Any] = {"image": array}
        if "im_shape" in names:
            feeds["im_shape"] = np.asarray(
                [[float(size), float(size)]], dtype=np.float32
            )
        if "scale_factor" in names:
            feeds["scale_factor"] = np.asarray([scale_factor], dtype=np.float32)
        outputs = session.run(None, feeds)
        output = outputs[0]
        if output.ndim == 3:
            output = output[0]
        count = None
        if len(outputs) > 1:
            flat = outputs[1].reshape(-1)
            if flat.size:
                count = int(flat[0])
        return _decode_layout(
            output.tolist(),
            image.size,
            options.layout_threshold,
            self._layout_labels,
            self._label_map,
            options.layout_nms_iou,
            count,
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
    layout_variant: str = DEFAULT_LAYOUT_VARIANT


_MODEL_CACHE: dict[_RuntimeSpec, _OnnxRuntime] = {}
_MODEL_CACHE_LOCK = threading.Lock()


def _cached_runtime(spec: _RuntimeSpec) -> _OnnxRuntime:
    with _MODEL_CACHE_LOCK:
        cached = _MODEL_CACHE.get(spec)
        if cached is not None:
            return cached
        runtime = _OnnxRuntime(
            Path(spec.layout_path),
            Path(spec.table_path),
            spec.providers,
            spec.layout_variant,
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
        _RuntimeSpec(
            os.fspath(layout_path),
            os.fspath(table_path),
            options.providers,
            _layout_variant(options),
        )
    )


# --------------------------------------------------------------------------- #
# PP-DocLayout pre/post-processing
# --------------------------------------------------------------------------- #
def _input_edge(session: Any) -> int | None:
    """Read the static square edge of the session's ``image`` input, if any."""

    for spec in session.get_inputs():
        if spec.name != "image":
            continue
        shape = list(getattr(spec, "shape", ()) or ())
        if len(shape) != 4:
            return None
        height, width = shape[2], shape[3]
        if isinstance(height, int) and isinstance(width, int) and height == width > 0:
            return int(height)
        return None
    return None


def _layout_input(image: Any, size: int, np: Any) -> tuple[Any, tuple[float, float]]:
    """Resize to ``size``x``size`` RGB in ``[0, 1]``, NCHW float32.

    PP-DocLayout is a PaddleDetection RT-DETR export: it only rescales to
    ``[0, 1]`` (no ImageNet mean/std, no letterbox) and recovers the original
    image size inside the head as ``im_shape / scale_factor``. The returned
    ``scale_factor`` is PaddleDetection's ``(resize_h / h, resize_w / w)``
    order, so the predicted boxes come back in original-image pixels.
    """

    from PIL import Image

    width, height = image.size
    if width <= 0 or height <= 0:
        raise PdfUnsupportedError("Cannot run layout detection on an empty page image")
    resized = image.convert("RGB").resize((size, size), Image.BILINEAR)
    array = np.asarray(resized, dtype=np.float32) / 255.0
    array = np.ascontiguousarray(array.transpose(2, 0, 1))[None]
    return array, (size / float(height), size / float(width))


def _nms(
    boxes: list[dict[str, Any]], iou_threshold: float, per_class: bool = True
) -> list[dict[str, Any]]:
    """Greedy NMS, by default only between boxes of the same model class.

    RT-DETR has no NMS of its own and emits the top-k (query, class) pairs, so
    a same-class pass removes the duplicate boxes it produces.
    """

    kept: list[dict[str, Any]] = []
    for candidate in sorted(boxes, key=lambda b: -float(b["score"])):
        suppressed = False
        for existing in kept:
            if per_class and existing.get("raw_label") != candidate.get("raw_label"):
                continue
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
    image_size: tuple[int, int],
    threshold: float,
    labels: Sequence[str],
    label_map: Mapping[str, str] | None = None,
    nms_iou: float | None = None,
    count: int | None = None,
) -> list[dict[str, Any]]:
    """Decode PP-DocLayout RT-DETR rows ``[cls, score, x0, y0, x1, y1(, order)]``.

    Coordinates are already in original-image pixels. Rows with a negative
    class id are the head's padding and are skipped. PP-DocLayoutV3 adds a
    seventh column: a monotonically increasing reading-order key, kept as
    ``read_order``.
    """

    width, height = image_size
    if count is not None and 0 <= count < len(rows):
        rows = rows[:count]
    mapping = label_map if label_map is not None else LAYOUT_LABEL_MAP
    results: list[dict[str, Any]] = []
    for row in rows:
        if len(row) < 6:
            continue
        class_id = int(row[0])
        if class_id < 0:
            continue
        score = float(row[1])
        if not math.isfinite(score) or score < threshold:
            continue
        raw_label = labels[class_id] if class_id < len(labels) else str(class_id)
        bbox = (
            max(0.0, min(float(width), float(row[2]))),
            max(0.0, min(float(height), float(row[3]))),
            max(0.0, min(float(width), float(row[4]))),
            max(0.0, min(float(height), float(row[5]))),
        )
        if _area(bbox) <= 0:
            continue
        item: dict[str, Any] = {
            "label": mapping.get(raw_label, raw_label),
            "raw_label": raw_label,
            "score": score,
            "bbox": bbox,
        }
        if len(row) >= 7:
            order = float(row[6])
            if math.isfinite(order):
                item["read_order"] = order
        results.append(item)
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
    """PP-DocLayout table regions -> SLANet-plus cells -> text-layer words."""

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
        label = str(detection["label"])
        block = LayoutBlock(
            Rect(*page_box),
            label,
            float(detection.get("score", 0.0)),
            str(detection.get("raw_label") or label),
        )
        blocks.append((block, dict(detection)))
    # PP-DocLayoutV3 predicts a reading-order key per box; when every block has
    # one it beats the geometric band rule (it handles column changes mid-page).
    if blocks and all("read_order" in detection for _, detection in blocks):
        return sorted(
            blocks,
            key=lambda item: (
                float(item[1]["read_order"]),
                item[0].bbox.y0,
                item[0].bbox.x0,
            ),
        )
    return _reading_order(blocks, rendered.page_bbox)


def find_layout(
    page: Any,
    *,
    options: Mapping[str, object] | None = None,
    _runtime: Any = None,
) -> list[LayoutBlock]:
    """Detect layout regions with PP-DocLayout, in reading order (page points)."""

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
        if block.raw_label in _RAW_CSS_LABELS:
            parts.append(f'<p class="{escape(block.raw_label)}">{text}</p>')
        elif label in _TEXT_LABELS:
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
    "DEFAULT_LAYOUT_VARIANT",
    "LAYOUT_INPUT_SIZES",
    "LAYOUT_LABELS",
    "LAYOUT_LABELS_BY_VARIANT",
    "LAYOUT_LABEL_MAP",
    "LAYOUT_LABEL_MAPS",
    "LAYOUT_LABEL_MAP_V3",
    "LAYOUT_MODEL_FILE",
    "LAYOUT_MODEL_FILES",
    "LAYOUT_MODEL_URL",
    "LAYOUT_MODEL_URLS",
    "LAYOUT_VARIANTS",
    "MODELS_ENV",
    "PP_DOCLAYOUTV3_LABELS",
    "PP_DOCLAYOUT_L_LABELS",
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
