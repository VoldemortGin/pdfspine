"""Type stubs for :mod:`pdfspine._onnx` (ONNX layout / table backend)."""

from dataclasses import dataclass
from typing import Any, Mapping, Sequence

from .geometry import Rect

MODELS_ENV: str
DEFAULT_LAYOUT_VARIANT: str
LAYOUT_VARIANTS: tuple[str, ...]
LAYOUT_MODEL_FILE: str
LAYOUT_MODEL_FILES: dict[str, str]
TABLE_MODEL_FILE: str
LAYOUT_MODEL_URL: str
LAYOUT_MODEL_URLS: dict[str, str]
LAYOUT_INPUT_SIZES: dict[str, int]
TABLE_MODEL_URL: str
LAYOUT_LABELS: tuple[str, ...]
PP_DOCLAYOUT_L_LABELS: tuple[str, ...]
PP_DOCLAYOUTV3_LABELS: tuple[str, ...]
LAYOUT_LABELS_BY_VARIANT: dict[str, tuple[str, ...]]
LAYOUT_LABEL_MAP: dict[str, str]
LAYOUT_LABEL_MAP_V3: dict[str, str]
LAYOUT_LABEL_MAPS: dict[str, dict[str, str]]
SLANET_STRUCTURE_DICT: tuple[str, ...]

@dataclass(frozen=True)
class OnnxOptions:
    dpi: int = ...
    layout_model: str | None = ...
    table_model: str | None = ...
    providers: str | tuple[str, ...] = ...
    layout_variant: str = ...
    layout_size: int | None = ...
    layout_threshold: float = ...
    layout_nms_iou: float | None = ...
    table_size: int = ...
    table_min_score: float = ...
    channel_order: str = ...
    crop_padding: int = ...
    ocr_if_no_text: bool = ...
    ocr_engine: str = ...
    ocr_language: str = ...
    @classmethod
    def from_mapping(cls, value: Mapping[str, object] | None) -> OnnxOptions: ...

@dataclass(frozen=True)
class LayoutBlock:
    bbox: Rect
    label: str
    score: float
    raw_label: str = ...

def clear_model_cache() -> None: ...
def find_tables(
    page: Any,
    *,
    clip: Any = ...,
    options: Mapping[str, object] | None = ...,
    _runtime: Any = ...,
) -> Any: ...
def find_layout(
    page: Any,
    *,
    options: Mapping[str, object] | None = ...,
    _runtime: Any = ...,
) -> list[LayoutBlock]: ...
def get_layout_html(
    page: Any,
    *,
    options: Mapping[str, object] | None = ...,
    _runtime: Any = ...,
) -> str: ...

__all__: Sequence[str]
