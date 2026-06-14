"""Idiomatic-Python ``Document`` / ``Page`` wrappers over the Rust ``_core``
handles (PRD §9.2 / §9.4 / §9.5).

These thin wrappers add PyMuPDF-compatible names and return geometry value types
(:class:`~oxipdf.geometry.Rect`) instead of raw tuples. Known-but-unimplemented
PyMuPDF methods raise :class:`~oxipdf._core.PdfUnsupportedError` (never
``AttributeError``), per PRD §9.5.
"""

from __future__ import annotations

import os
from typing import Iterator

from . import _core
from ._core import PdfUnsupportedError
from .geometry import Point, Quad, Rect

# PyMuPDF methods/properties that exist on the real API but land in later
# milestones. Accessing them raises a typed, catchable error with a hint, not
# AttributeError (PRD §9.5).
_UNIMPLEMENTED_PAGE = {
    "get_pixmap": "rendering / image pages (M5/M6)",
    "get_drawings": "vector drawings (M4)",
    "get_links": "links (M3)",
    "annots": "annotations (M4)",
    "insert_text": "content emission (M4)",
    "draw_line": "content emission (M4)",
}

_UNIMPLEMENTED_DOC = {
    "get_toc": "table of contents (M3)",
    "set_metadata": "metadata write (M3)",
    "save": "save / incremental (M3)",
    "insert_pdf": "merge (M3)",
    "get_xml_metadata": "XMP metadata (M3)",
    "select": "page selection (M3)",
    "convert_to_pdf": "image documents (M5)",
}


def _rect(t: tuple[float, float, float, float]) -> Rect:
    return Rect(*t)


def _as_clip(clip) -> tuple[float, float, float, float] | None:
    """Normalizes a clip argument (``Rect``/sequence/``None``) to a 4-tuple."""
    if clip is None:
        return None
    return (float(clip[0]), float(clip[1]), float(clip[2]), float(clip[3]))


def _quad_from_corners(t: tuple[float, ...]) -> Quad:
    """Builds a :class:`Quad` from the corner-coord 8-tuple
    ``(ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y)`` the core returns."""
    return Quad(
        Point(t[0], t[1]),
        Point(t[2], t[3]),
        Point(t[4], t[5]),
        Point(t[6], t[7]),
    )


def _rect_from_corners(t: tuple[float, ...]) -> Rect:
    """The enclosing :class:`Rect` of the corner-coord 8-tuple."""
    xs = (t[0], t[2], t[4], t[6])
    ys = (t[1], t[3], t[5], t[7])
    return Rect(min(xs), min(ys), max(xs), max(ys))


class TextPage:
    """A reusable text-extraction handle (PyMuPDF ``fitz.TextPage``).

    Built by :meth:`Page.get_textpage`; pass it back to
    :meth:`Page.get_text` / :meth:`Page.search_for` via ``textpage=`` to avoid
    re-parsing the page (PRD §9.4).
    """

    __slots__ = ("_tp",)

    def __init__(self, core_tp: "_core.TextPage") -> None:
        self._tp = core_tp

    def extractText(self) -> str:
        return self._tp.extractText()

    def extractWORDS(self) -> list[tuple]:
        return self._tp.extractWORDS()

    def extractBLOCKS(self) -> list[tuple]:
        return self._tp.extractBLOCKS()

    def extractDICT(self) -> dict:
        return self._tp.extractDICT()

    def extractRAWDICT(self) -> dict:
        return self._tp.extractRAWDICT()

    def extractJSON(self) -> str:
        return self._tp.extractJSON()

    @property
    def rect(self) -> Rect:
        return Rect(0.0, 0.0, self._tp.width, self._tp.height)

    def __repr__(self) -> str:
        return repr(self._tp)


class Page:
    """One page of a :class:`Document` (PyMuPDF ``fitz.Page``)."""

    __slots__ = ("_page",)

    def __init__(self, core_page: "_core.Page") -> None:
        self._page = core_page

    @property
    def number(self) -> int:
        """The zero-based page index (PyMuPDF ``page.number``)."""
        return self._page.number

    @property
    def rect(self) -> Rect:
        """The page bound ``CropBox ∩ MediaBox`` (PyMuPDF ``page.rect``)."""
        return _rect(self._page.rect())

    def bound(self) -> Rect:
        """Alias for :attr:`rect` (PyMuPDF ``page.bound()``)."""
        return _rect(self._page.bound())

    @property
    def mediabox(self) -> Rect:
        """The effective ``/MediaBox`` (inherited)."""
        return _rect(self._page.mediabox())

    @property
    def cropbox(self) -> Rect:
        """The effective ``/CropBox`` (inherited, clipped to media box)."""
        return _rect(self._page.cropbox())

    @property
    def rotation(self) -> int:
        """The normalized rotation ∈ {0, 90, 180, 270} (PyMuPDF ``page.rotation``)."""
        return self._page.rotation()

    # --- text extraction (PRD §8.6 / §9.4) ---
    def get_textpage(self, flags: int | None = None, clip=None) -> TextPage:
        """Builds a reusable :class:`TextPage` (PyMuPDF ``page.get_textpage``)."""
        return TextPage(self._page.get_textpage(flags, _as_clip(clip)))

    def get_text(
        self,
        option: str = "text",
        *,
        clip=None,
        flags: int | None = None,
        textpage: TextPage | None = None,
        sort: bool = False,
    ):
        """Extracts text (PyMuPDF ``page.get_text``).

        Returns the native object per ``option``: ``str`` for
        ``text``/``html``/``xhtml``/``xml``/``json``/``rawjson``;
        ``list[tuple]`` for ``blocks``/``words``; ``dict`` for
        ``dict``/``rawdict``. Reuses ``textpage`` when given; ``sort`` orders
        blocks by ``(y, x)``.
        """
        tp = textpage._tp if textpage is not None else None
        return self._page.get_text(
            option, clip=_as_clip(clip), flags=flags, textpage=tp, sort=sort
        )

    def search_for(
        self,
        needle: str,
        *,
        hit_max: int = 0,
        quads: bool = False,
        clip=None,
        flags: int | None = None,
        textpage: TextPage | None = None,
    ) -> list:
        """Searches for ``needle`` (PyMuPDF ``page.search_for``).

        Returns a list of :class:`Quad` (``quads=True``) or :class:`Rect`
        (default), each overlapping a hit.
        """
        tp = textpage._tp if textpage is not None else None
        hits = self._page.search_for(
            needle,
            hit_max=hit_max,
            quads=quads,
            clip=_as_clip(clip),
            flags=flags,
            textpage=tp,
        )
        if quads:
            return [_quad_from_corners(h) for h in hits]
        return [_rect_from_corners(h) for h in hits]

    # --- inventory (PRD §8.6) ---
    def get_fonts(self, full: bool = False) -> list[tuple]:
        """The page's fonts as PyMuPDF tuples (PyMuPDF ``page.get_fonts``)."""
        return self._page.get_fonts(full)

    def get_images(self, full: bool = False) -> list[tuple]:
        """The page's images as PyMuPDF tuples (PyMuPDF ``page.get_images``)."""
        return self._page.get_images(full)

    def __repr__(self) -> str:
        return f"<oxipdf.Page number={self.number}>"

    def __getattr__(self, name: str):
        hint = _UNIMPLEMENTED_PAGE.get(name)
        if hint is not None:
            raise PdfUnsupportedError(
                f"Page.{name} is not implemented yet: {hint}. "
                "See the oxipdf parity matrix."
            )
        raise AttributeError(f"'Page' object has no attribute {name!r}")


class Document:
    """A parsed document (PyMuPDF ``fitz.Document``)."""

    __slots__ = ("_doc",)

    def __init__(self, core_doc: "_core.Document") -> None:
        self._doc = core_doc

    # --- pages ---
    @property
    def page_count(self) -> int:
        """The number of pages (PyMuPDF ``doc.page_count``)."""
        return self._doc.page_count

    def __len__(self) -> int:
        return self._doc.page_count

    def load_page(self, index: int = 0) -> Page:
        """Loads the page at zero-based ``index`` (PyMuPDF ``load_page``)."""
        if index < 0:
            index += self._doc.page_count
        return Page(self._doc.load_page(index))

    def __getitem__(self, index: int) -> Page:
        return Page(self._doc[index])

    def __iter__(self) -> Iterator[Page]:
        for i in range(self._doc.page_count):
            yield Page(self._doc.load_page(i))

    # --- document facts ---
    @property
    def is_pdf(self) -> bool:
        return self._doc.is_pdf

    @property
    def is_repaired(self) -> bool:
        return self._doc.is_repaired

    @property
    def is_encrypted(self) -> bool:
        return self._doc.is_encrypted

    @property
    def needs_pass(self) -> bool:
        return self._doc.needs_pass

    @property
    def permissions(self) -> int:
        return self._doc.permissions

    def authenticate(self, password) -> bool:
        """Authenticates ``password`` (str or bytes). Returns True on success."""
        return self._doc.authenticate(password)

    # --- text convenience ---
    def get_page_text(
        self,
        pno: int,
        option: str = "text",
        *,
        flags: int | None = None,
        sort: bool = False,
    ):
        """Extracts text from page ``pno`` (PyMuPDF ``Document.get_page_text``)."""
        return self._doc.get_page_text(pno, option, flags=flags, sort=sort)

    @property
    def metadata(self) -> dict[str, str]:
        """The document metadata dict with PyMuPDF keys (PRD §9.5)."""
        return self._doc.metadata()

    # --- low-level xref read API ---
    def xref_length(self) -> int:
        return self._doc.xref_length()

    def xref_object(self, xref: int) -> str:
        return self._doc.xref_object(xref)

    def xref_get_key(self, xref: int, key: str):
        return self._doc.xref_get_key(xref, key)

    def xref_is_stream(self, xref: int) -> bool:
        return self._doc.xref_is_stream(xref)

    def xref_stream(self, xref: int) -> bytes:
        return self._doc.xref_stream(xref)

    def close(self) -> None:
        """Releases the document (drops the underlying Rust handle)."""
        self._doc = None  # type: ignore[assignment]

    def __enter__(self) -> "Document":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def __repr__(self) -> str:
        return f"<oxipdf.Document page_count={self.page_count}>"

    def __getattr__(self, name: str):
        hint = _UNIMPLEMENTED_DOC.get(name)
        if hint is not None:
            raise PdfUnsupportedError(
                f"Document.{name} is not implemented yet: {hint}. "
                "See the oxipdf parity matrix."
            )
        raise AttributeError(f"'Document' object has no attribute {name!r}")


def open(
    filename: str | os.PathLike[str] | None = None,
    *,
    stream: bytes | None = None,
    filetype: str | None = None,
) -> Document:
    """Opens a document (PyMuPDF ``fitz.open``).

    Pass a path positionally, or in-memory bytes via ``stream=``. The heavy
    parse runs with the GIL released in the Rust core (PRD §9.4).
    """
    if stream is not None:
        return Document(_core.open_bytes(bytes(stream)))
    if filename is None:
        raise ValueError("open() requires a filename or stream=")
    return Document(_core.open(os.fspath(filename)))
