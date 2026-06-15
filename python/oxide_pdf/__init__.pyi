"""Type stubs for the :mod:`oxide_pdf` package."""

from . import _core as _core
from ._core import (
    PdfDecodeError as PdfDecodeError,
    PdfError as PdfError,
    PdfLimitError as PdfLimitError,
    PdfPasswordError as PdfPasswordError,
    PdfRedactionError as PdfRedactionError,
    PdfSyntaxError as PdfSyntaxError,
    PdfUnsupportedError as PdfUnsupportedError,
    identity_matrix as identity_matrix,
    version as version,
)
from .document import (
    PDF_ENCRYPT_AES_128 as PDF_ENCRYPT_AES_128,
    PDF_ENCRYPT_AES_256 as PDF_ENCRYPT_AES_256,
    PDF_ENCRYPT_NONE as PDF_ENCRYPT_NONE,
    PDF_ENCRYPT_RC4_128 as PDF_ENCRYPT_RC4_128,
    Annot as Annot,
    DisplayList as DisplayList,
    Document as Document,
    Page as Page,
    Pixmap as Pixmap,
    Shape as Shape,
    Table as Table,
    TableFinder as TableFinder,
    TextPage as TextPage,
    Widget as Widget,
    open as open,
)
from .geometry import (
    IRect as IRect,
    Matrix as Matrix,
    Point as Point,
    Quad as Quad,
    Rect as Rect,
)

__version__: str

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
