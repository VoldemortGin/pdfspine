"""Type stubs for :mod:`pdfspine.models` (typed page-content value objects)."""

from dataclasses import dataclass
from typing import Literal

from .geometry import Rect

@dataclass(frozen=True)
class TextBlock:
    number: int
    bbox: Rect
    text: str

@dataclass(frozen=True)
class ImageBlock:
    number: int
    bbox: Rect
    width: int
    height: int
    ext: str
    image: bytes | None

@dataclass(frozen=True)
class LinkAnnotation:
    uri: str
    rect: Rect

@dataclass(frozen=True)
class FilledRectangle:
    rect: Rect
    fill: tuple[float, ...]

@dataclass(frozen=True)
class TableCell:
    row: int
    col: int
    row_span: int
    col_span: int
    bbox: Rect
    state: Literal["present", "blank", "unavailable"]
    text: str | None

@dataclass(frozen=True)
class TableSlot:
    row: int
    col: int
    state: Literal["present", "blank", "unavailable", "continuation"]
    cell: TableCell | None
    @property
    def origin(self) -> tuple[int, int] | None: ...
