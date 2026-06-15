"""Type stubs for the ``pymupdf`` alias package (re-exports the fitz shim)."""

from typing import Any

from fitz import (
    PDF_ENCRYPT_AES_128 as PDF_ENCRYPT_AES_128,
    PDF_ENCRYPT_AES_256 as PDF_ENCRYPT_AES_256,
    PDF_ENCRYPT_NONE as PDF_ENCRYPT_NONE,
    PDF_ENCRYPT_RC4_128 as PDF_ENCRYPT_RC4_128,
    Annot as Annot,
    DisplayList as DisplayList,
    Document as Document,
    EmptyFileError as EmptyFileError,
    FileDataError as FileDataError,
    IRect as IRect,
    Matrix as Matrix,
    Page as Page,
    PdfDecodeError as PdfDecodeError,
    PdfError as PdfError,
    PdfLimitError as PdfLimitError,
    PdfPasswordError as PdfPasswordError,
    PdfRedactionError as PdfRedactionError,
    PdfSyntaxError as PdfSyntaxError,
    PdfUnsupportedError as PdfUnsupportedError,
    Pixmap as Pixmap,
    Point as Point,
    Quad as Quad,
    Rect as Rect,
    Shape as Shape,
    Table as Table,
    TableFinder as TableFinder,
    TextPage as TextPage,
    Widget as Widget,
    __version__ as __version__,
    open as open,
    pymupdf_version as pymupdf_version,
)

def __getattr__(name: str) -> Any: ...
