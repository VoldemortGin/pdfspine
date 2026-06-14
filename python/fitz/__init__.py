"""``fitz`` compatibility shim for oxipdf (PRD §9.5).

PyMuPDF is imported as ``import fitz``; this package maps PyMuPDF's exact names
onto oxipdf so existing code runs unchanged. It re-exports :func:`oxipdf.open`,
the :class:`~oxipdf.Document`/:class:`~oxipdf.Page` classes and the geometry value
types, and aliases PyMuPDF's exception names onto oxipdf's typed hierarchy.

The full PyMuPDF surface (text/image/edit) is built out in later milestones; the
read surface (open, page_count, indexing, metadata, geometry, encryption) is
M1f-complete.
"""

from __future__ import annotations

import oxipdf
from oxipdf import (
    Document,
    IRect,
    Matrix,
    Page,
    PdfDecodeError,
    PdfError,
    PdfLimitError,
    PdfPasswordError,
    PdfSyntaxError,
    PdfUnsupportedError,
    Point,
    Quad,
    Rect,
    __version__,
    open,
)

# The PyMuPDF baseline this shim targets (PRD §1129).
pymupdf_version = "1.24.x (oxipdf shim)"

# --- PyMuPDF exception-name aliases (PRD §9.3) ---
# PyMuPDF raises these names; map them onto oxipdf's typed hierarchy so
# `except fitz.FileDataError` keeps working.
FileDataError = PdfSyntaxError
FileNotFoundError = FileNotFoundError  # built-in; PyMuPDF re-exports it
EmptyFileError = PdfSyntaxError
mupdf_display_errors = PdfError

__all__ = [
    "__version__",
    "pymupdf_version",
    "open",
    "Document",
    "Page",
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
    "FileDataError",
    "EmptyFileError",
]


def __getattr__(name: str):
    """Surface PyMuPDF's huge namespace lazily — anything not yet implemented
    raises :class:`PdfUnsupportedError` with a hint, never ``AttributeError``
    (PRD §9.5)."""
    # Defer to oxipdf for anything it defines.
    if hasattr(oxipdf, name):
        return getattr(oxipdf, name)
    raise PdfUnsupportedError(
        f"fitz.{name} is not implemented in the oxipdf shim yet. "
        "See the oxipdf parity matrix."
    )
