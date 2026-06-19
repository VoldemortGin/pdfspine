"""``pymupdf`` alias module for pdfspine (PRD §9.5).

PyMuPDF can also be imported as ``import pymupdf``; this mirrors the
:mod:`pdfspine.fitz` shim by re-exporting it wholesale, so ``pymupdf.open`` /
``pymupdf.Rect`` / the exception aliases all resolve to the same objects.

Like :mod:`pdfspine.fitz`, this is opt-in: a default install does not claim the
global ``pymupdf`` name. Use ``from pdfspine import pymupdf`` directly, or call
:func:`pdfspine.install_fitz_shim` to make the literal ``import pymupdf`` resolve
here.
"""

from __future__ import annotations

from pdfspine.fitz import *  # noqa: F401,F403
from pdfspine.fitz import (  # noqa: F401
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
