"""oxide_pdf — an Apache-2.0-licensed, pure-Rust reimplementation of PyMuPDF (``fitz``).

This is the native, idiomatic-Python package backed by the Rust ``_core``
extension module. M1f exposes the read surface (PRD §7 / §9.2): :func:`open`
returns a :class:`Document` with ``page_count``/indexing/``load_page``/
``metadata`` and per-page geometry (``rect``/``rotation``/``bound``/boxes).

Geometry is returned to Python as PyMuPDF-compatible value types
(:class:`Rect`, :class:`Matrix`, …) defined in :mod:`oxide_pdf.geometry`.
"""

from __future__ import annotations

from . import _core
from ._core import (
    PdfDecodeError,
    PdfError,
    PdfLimitError,
    PdfPasswordError,
    PdfRedactionError,
    PdfSyntaxError,
    PdfUnsupportedError,
    identity_matrix,
    version,
)
from .document import (
    PDF_ENCRYPT_AES_128,
    PDF_ENCRYPT_AES_256,
    PDF_ENCRYPT_NONE,
    PDF_ENCRYPT_RC4_128,
    TOOLS,
    Annot,
    DisplayList,
    Document,
    Font,
    Page,
    Pixmap,
    Shape,
    Table,
    TableFinder,
    TextPage,
    Tools,
    Widget,
    open,
)
from .geometry import IRect, Matrix, Point, Quad, Rect

__version__: str = _core.__version__

__all__ = [
    "__version__",
    "version",
    "identity_matrix",
    "open",
    "Document",
    "Page",
    "Pixmap",
    "DisplayList",
    "TextPage",
    "Annot",
    "Widget",
    "Shape",
    "Table",
    "TableFinder",
    "Font",
    "Tools",
    "TOOLS",
    "Rect",
    "IRect",
    "Point",
    "Matrix",
    "Quad",
    "PDF_ENCRYPT_NONE",
    "PDF_ENCRYPT_RC4_128",
    "PDF_ENCRYPT_AES_128",
    "PDF_ENCRYPT_AES_256",
    "PdfError",
    "PdfSyntaxError",
    "PdfPasswordError",
    "PdfUnsupportedError",
    "PdfDecodeError",
    "PdfLimitError",
    "PdfRedactionError",
]
