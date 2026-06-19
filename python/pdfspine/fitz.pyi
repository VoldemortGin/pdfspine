"""Type stubs for the ``pdfspine.fitz`` compatibility shim (re-exports pdfspine)."""

from typing import Any

from pdfspine import (
    CS_CMYK as CS_CMYK,
    CS_GRAY as CS_GRAY,
    CS_RGB as CS_RGB,
    PDF_ENCRYPT_AES_128 as PDF_ENCRYPT_AES_128,
    PDF_ENCRYPT_AES_256 as PDF_ENCRYPT_AES_256,
    PDF_ENCRYPT_NONE as PDF_ENCRYPT_NONE,
    PDF_ENCRYPT_RC4_128 as PDF_ENCRYPT_RC4_128,
    Annot as Annot,
    Colorspace as Colorspace,
    DisplayList as DisplayList,
    Document as Document,
    IRect as IRect,
    Link as Link,
    Matrix as Matrix,
    Outline as Outline,
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
    TextWriter as TextWriter,
    Widget as Widget,
    __version__ as __version__,
    csCMYK as csCMYK,
    csGRAY as csGRAY,
    csRGB as csRGB,
    linkDest as linkDest,
    open as open,
)

pymupdf_version: str

# PyMuPDF exception-name aliases mapped onto pdfspine's typed hierarchy.
FileDataError = PdfSyntaxError
EmptyFileError = PdfSyntaxError
mupdf_display_errors = PdfError

def __getattr__(name: str) -> Any: ...
