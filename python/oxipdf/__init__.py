"""oxipdf — an MIT-licensed, pure-Rust reimplementation of PyMuPDF (``fitz``).

This is the native, idiomatic-Python package backed by the Rust ``_core``
extension module. M1f exposes the read surface (PRD §7 / §9.2): :func:`open`
returns a :class:`Document` with ``page_count``/indexing/``load_page``/
``metadata`` and per-page geometry (``rect``/``rotation``/``bound``/boxes).

Geometry is returned to Python as PyMuPDF-compatible value types
(:class:`Rect`, :class:`Matrix`, …) defined in :mod:`oxipdf.geometry`.
"""

from __future__ import annotations

from . import _core
from ._core import (
    PdfDecodeError,
    PdfError,
    PdfLimitError,
    PdfPasswordError,
    PdfSyntaxError,
    PdfUnsupportedError,
    identity_matrix,
    version,
)
from .document import Document, Page, TextPage, open
from .geometry import IRect, Matrix, Point, Quad, Rect

__version__: str = _core.__version__

__all__ = [
    "__version__",
    "version",
    "identity_matrix",
    "open",
    "Document",
    "Page",
    "TextPage",
    "Rect",
    "IRect",
    "Point",
    "Matrix",
    "Quad",
    "PdfError",
    "PdfSyntaxError",
    "PdfPasswordError",
    "PdfUnsupportedError",
    "PdfDecodeError",
    "PdfLimitError",
]
