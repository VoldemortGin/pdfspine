"""Type stubs for :mod:`pdfspine.models` (typed page-content value objects)."""

from dataclasses import dataclass

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
