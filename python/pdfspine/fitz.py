"""``fitz`` compatibility shim for pdfspine (PRD §9.5).

PyMuPDF is imported as ``import fitz``; this module maps PyMuPDF's exact names
onto pdfspine so existing code runs unchanged. It re-exports :func:`pdfspine.open`,
the :class:`~pdfspine.Document`/:class:`~pdfspine.Page` classes and the geometry value
types, and aliases PyMuPDF's exception names onto pdfspine's typed hierarchy.

This shim is *opt-in*: a default ``pip install pdfspine`` does NOT claim the
global top-level ``fitz`` / ``pymupdf`` import names (so it never collides with a
real PyMuPDF in the same environment). It is always available one step away as
``import pdfspine.fitz as fitz``; to make the literal ``import fitz`` resolve to
this shim, call :func:`pdfspine.install_fitz_shim` first.

The full PyMuPDF surface (text/image/edit) is built out in later milestones; the
read surface (open, page_count, indexing, metadata, geometry, encryption) is
M1f-complete.
"""

from __future__ import annotations

import pdfspine
from pdfspine import (
    CS_CMYK,
    CS_GRAY,
    CS_RGB,
    PDF_ENCRYPT_AES_128,
    PDF_ENCRYPT_AES_256,
    PDF_ENCRYPT_NONE,
    PDF_ENCRYPT_RC4_128,
    TOOLS,
    Annot,
    Base14_fontnames,
    Colorspace,
    DisplayList,
    Document,
    Font,
    IRect,
    Link,
    Matrix,
    Outline,
    Page,
    PdfDecodeError,
    PdfError,
    PdfLimitError,
    PdfPasswordError,
    PdfRedactionError,
    PdfSyntaxError,
    PdfUnsupportedError,
    Pixmap,
    Point,
    Quad,
    Rect,
    Shape,
    Table,
    TableFinder,
    TextPage,
    TextWriter,
    Tools,
    Widget,
    __version__,
    csCMYK,
    csGRAY,
    csRGB,
    image_profile,
    linkDest,
    open,
)

# The PyMuPDF baseline this shim targets (PRD §1129).
pymupdf_version = "1.24.x (pdfspine shim)"

# --- PyMuPDF exception-name aliases (PRD §9.3) ---
# PyMuPDF raises these names; map them onto pdfspine's typed hierarchy so
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
    "image_profile",
    "csGRAY",
    "csRGB",
    "csCMYK",
    "CS_GRAY",
    "CS_RGB",
    "CS_CMYK",
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
    "FileDataError",
    "EmptyFileError",
]


def __getattr__(name: str):
    """Surface PyMuPDF's huge namespace lazily — anything not yet implemented
    raises :class:`PdfUnsupportedError` with a hint, never ``AttributeError``
    (PRD §9.5)."""
    # Defer to pdfspine for anything it defines.
    if hasattr(pdfspine, name):
        return getattr(pdfspine, name)
    # 工具/解释器探测的特殊属性（dunder 协议属性、pytest 的模块级 ``pytest_plugins`` /
    # ``pytestmark`` 等 ``pytest*`` 收集探针）不属于 PyMuPDF API 面——必须按“无此属性”
    # 语义抛 ``AttributeError``，否则会绊倒 pytest collection / inspect /
    # ``--doctest-modules``（它们靠 ``AttributeError`` 判定属性缺失）。PyMuPDF 没有
    # ``pytest*`` 或 dunder 形态的公开 API，故此例外不会遮蔽真实 API。PRD §9.5 的
    # “never AttributeError”只约束 known-but-unimplemented 的 PyMuPDF *方法*。
    if name.startswith("pytest") or (name.startswith("__") and name.endswith("__")):
        raise AttributeError(name)
    raise PdfUnsupportedError(
        f"fitz.{name} is not implemented in the pdfspine shim yet. "
        "See the pdfspine parity matrix."
    )
