"""``pymupdf`` alias package for oxipdf (PRD §9.5).

PyMuPDF can also be imported as ``import pymupdf``; this mirrors the :mod:`fitz`
shim by re-exporting it wholesale, so ``pymupdf.open`` / ``pymupdf.Rect`` / the
exception aliases all resolve to the same objects.
"""

from __future__ import annotations

from fitz import *  # noqa: F401,F403
from fitz import (  # noqa: F401
    Document,
    Matrix,
    Page,
    PdfError,
    PdfPasswordError,
    PdfUnsupportedError,
    Rect,
    __getattr__,
    __version__,
    open,
)
