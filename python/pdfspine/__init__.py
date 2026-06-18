"""pdfspine — an Apache-2.0-licensed, pure-Rust reimplementation of PyMuPDF (``fitz``).

This is the native, idiomatic-Python package backed by the Rust ``_core``
extension module. M1f exposes the read surface (PRD §7 / §9.2): :func:`open`
returns a :class:`Document` with ``page_count``/indexing/``load_page``/
``metadata`` and per-page geometry (``rect``/``rotation``/``bound``/boxes).

Geometry is returned to Python as PyMuPDF-compatible value types
(:class:`Rect`, :class:`Matrix`, …) defined in :mod:`pdfspine.geometry`.
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
)
from . import constants
from .constants import *  # noqa: F403  (re-exported; names enumerated in constants.__all__)
from .document import (
    TOOLS,
    Annot,
    Base14_fontnames,
    Colorspace,
    DisplayList,
    Document,
    Font,
    Link,
    Outline,
    Page,
    Pixmap,
    Shape,
    Table,
    TableFinder,
    TextPage,
    TextWriter,
    Tools,
    Widget,
    csCMYK,
    csGRAY,
    csRGB,
    linkDest,
    open,
)
from .geometry import IRect, Matrix, Point, Quad, Rect
from .helpers import (
    Base14_fontdict,
    ConversionHeader,
    ConversionTrailer,
    get_pdf_now,
    get_pdf_str,
    get_text_length,
    glyph_name_to_unicode,
    log,
    message,
    planish_line,
    recover_bbox_quad,
    recover_char_quad,
    recover_line_quad,
    recover_quad,
    recover_span_quad,
    sRGB_to_pdf,
    sRGB_to_rgb,
    set_log,
    set_messages,
    unicode_to_glyph_name,
)

__version__: str = _core.__version__

__all__ = [
    "__version__",
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
    "Base14_fontnames",
    "Tools",
    "TOOLS",
    "Link",
    "linkDest",
    "Outline",
    "Colorspace",
    "TextWriter",
    "csGRAY",
    "csRGB",
    "csCMYK",
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
    "PdfRedactionError",
    "constants",
    "Base14_fontdict",
    "ConversionHeader",
    "ConversionTrailer",
    "get_pdf_now",
    "get_pdf_str",
    "get_text_length",
    "glyph_name_to_unicode",
    "log",
    "message",
    "planish_line",
    "recover_bbox_quad",
    "recover_char_quad",
    "recover_line_quad",
    "recover_quad",
    "recover_span_quad",
    "sRGB_to_pdf",
    "sRGB_to_rgb",
    "set_log",
    "set_messages",
    "unicode_to_glyph_name",
    *constants.__all__,
]
