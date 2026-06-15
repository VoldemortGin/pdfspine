"""Type stubs for the ``fitz`` compatibility shim (re-exports oxide_pdf)."""

from typing import Any

from oxide_pdf import (
    PDF_ENCRYPT_AES_128 as PDF_ENCRYPT_AES_128,
    PDF_ENCRYPT_AES_256 as PDF_ENCRYPT_AES_256,
    PDF_ENCRYPT_NONE as PDF_ENCRYPT_NONE,
    PDF_ENCRYPT_RC4_128 as PDF_ENCRYPT_RC4_128,
    Annot as Annot,
    DisplayList as DisplayList,
    Document as Document,
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
)

pymupdf_version: str

# PyMuPDF exception-name aliases mapped onto oxide_pdf's typed hierarchy.
FileDataError = PdfSyntaxError
EmptyFileError = PdfSyntaxError
mupdf_display_errors = PdfError

def __getattr__(name: str) -> Any: ...
