"""Typed page-content value objects (pdfspine-original extension).

Frozen dataclasses returned by the native typed ``Page`` API —
:meth:`pdfspine.Page.content_blocks`, :meth:`pdfspine.Page.link_annotations`
and :meth:`pdfspine.Page.filled_rectangles` — so callers get validated value
objects instead of hand-parsing ``get_text("dict")`` / ``get_links()`` /
``get_drawings()`` dicts. These types are NOT part of the PyMuPDF-compatible
surface (they are not tracked in COMPAT.toml).
"""

from __future__ import annotations

from dataclasses import dataclass

from .geometry import Rect


@dataclass(frozen=True)
class TextBlock:
    """A text content block (``get_text("dict")`` ``type == 0``), typed.

    ``text`` is the block's text with span texts concatenated per line and
    lines joined with newlines.

    >>> from pdfspine import Rect, TextBlock
    >>> block = TextBlock(number=0, bbox=Rect(0, 0, 100, 20), text="Hello")
    >>> block.text
    'Hello'
    """

    number: int
    bbox: Rect
    text: str


@dataclass(frozen=True)
class ImageBlock:
    """An image content block (``get_text("dict")`` ``type == 1``), typed.

    ``image`` carries the original encoded image bytes exactly as embedded
    (no OCR, no re-encoding) and ``ext`` its file extension (e.g. ``"png"``,
    ``"jpeg"``); ``image`` is ``None`` when the payload is unavailable.

    >>> from pdfspine import ImageBlock, Rect
    >>> block = ImageBlock(
    ...     number=1, bbox=Rect(0, 0, 8, 6), width=8, height=6,
    ...     ext="png", image=b"\\x89PNG...",
    ... )
    >>> block.ext
    'png'
    """

    number: int
    bbox: Rect
    width: int
    height: int
    ext: str
    image: bytes | None


@dataclass(frozen=True)
class LinkAnnotation:
    """An external-URI link annotation, typed.

    ``rect`` is the link's hot area (the ``get_links()`` dict's ``from``).

    >>> from pdfspine import LinkAnnotation, Rect
    >>> link = LinkAnnotation(uri="https://example.com", rect=Rect(0, 0, 50, 20))
    >>> link.uri
    'https://example.com'
    """

    uri: str
    rect: Rect


@dataclass(frozen=True)
class FilledRectangle:
    """A filled vector rectangle from ``get_drawings()``, typed.

    ``fill`` is the fill color as a tuple of float components in ``0..1``
    (RGB for the common case).

    >>> from pdfspine import FilledRectangle, Rect
    >>> box = FilledRectangle(rect=Rect(0, 0, 10, 10), fill=(1.0, 0.0, 0.0))
    >>> box.fill
    (1.0, 0.0, 0.0)
    """

    rect: Rect
    fill: tuple[float, ...]
