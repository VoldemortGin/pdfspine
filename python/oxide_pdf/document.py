"""Idiomatic-Python ``Document`` / ``Page`` wrappers over the Rust ``_core``
handles (PRD §9.2 / §9.4 / §9.5).

These thin wrappers add PyMuPDF-compatible names and return geometry value types
(:class:`~oxide_pdf.geometry.Rect`) instead of raw tuples. Known-but-unimplemented
PyMuPDF methods raise :class:`~oxide_pdf._core.PdfUnsupportedError` (never
``AttributeError``), per PRD §9.5.
"""

from __future__ import annotations

import os
from typing import Iterator

from . import _core
from ._core import PdfRedactionError, PdfUnsupportedError
from .geometry import Point, Quad, Rect

# PyMuPDF methods/properties that exist on the real API but land in later
# milestones. Accessing them raises a typed, catchable error with a hint, not
# AttributeError (PRD §9.5).
_UNIMPLEMENTED_PAGE = {
    "get_pixmap": "rendering / image pages (M5/M6)",
}

_UNIMPLEMENTED_DOC = {
    "convert_to_pdf": "image documents (M5)",
}

# PyMuPDF encryption-method constants (PRD §8.4). AES-256 is always authored as
# R6 (never R5).
PDF_ENCRYPT_NONE = 0
PDF_ENCRYPT_RC4_128 = 1
PDF_ENCRYPT_AES_128 = 2
PDF_ENCRYPT_AES_256 = 4
# PyMuPDF permission flags (advisory). All-permissions sentinel.
PDF_PERM_ACCESSIBILITY = 1 << 9


def _rect(t: tuple[float, float, float, float]) -> Rect:
    return Rect(*t)


def _as_clip(clip) -> tuple[float, float, float, float] | None:
    """Normalizes a clip argument (``Rect``/sequence/``None``) to a 4-tuple."""
    if clip is None:
        return None
    return (float(clip[0]), float(clip[1]), float(clip[2]), float(clip[3]))


def _pt(p) -> tuple[float, float]:
    """Normalizes a point (``Point``/sequence/2-tuple) to ``(x, y)`` floats."""
    return (float(p[0]), float(p[1]))


def _rt(r) -> tuple[float, float, float, float]:
    """Normalizes a rect (``Rect``/``IRect``/sequence/4-tuple) to a 4-tuple."""
    return (float(r[0]), float(r[1]), float(r[2]), float(r[3]))


def _color(c) -> tuple[float, float, float] | None:
    """Normalizes a color to an ``(r, g, b)`` float tuple or ``None``.

    Accepts ``None`` (→ ``None``), a single number (gray → ``(c, c, c)``), or a
    3-sequence (→ ``(r, g, b)``), matching PyMuPDF's color leniency.
    """
    if c is None:
        return None
    if isinstance(c, (int, float)):
        g = float(c)
        return (g, g, g)
    return (float(c[0]), float(c[1]), float(c[2]))


def _quad(q) -> tuple[float, float, float, float, float, float, float, float]:
    """Normalizes one quad to the corner-coord 8-tuple
    ``(ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y)`` the core expects.

    Accepts a :class:`Quad`, a :class:`Rect`/4-sequence (rect → quad corners),
    or an existing 8-sequence.
    """
    if isinstance(q, Quad):
        return (
            q.ul.x, q.ul.y, q.ur.x, q.ur.y,
            q.ll.x, q.ll.y, q.lr.x, q.lr.y,
        )
    seq = tuple(float(v) for v in q)
    if len(seq) == 8:
        return seq  # type: ignore[return-value]
    if len(seq) == 4:
        x0, y0, x1, y1 = seq
        return (x0, y0, x1, y0, x0, y1, x1, y1)
    raise ValueError("a quad must be a Quad, a 4-rect or an 8-tuple")


def _quads(arg) -> list[tuple[float, ...]]:
    """Normalizes a quad argument to a list of corner-coord 8-tuples.

    PyMuPDF's ``add_highlight_annot`` (and kin) accept a single ``Quad``/``Rect``
    *or* a list of them; this wraps a single value in a list, else maps each.
    """
    if isinstance(arg, Quad):
        return [_quad(arg)]
    # A bare Rect or flat 4-/8-sequence of numbers is a single quad.
    if isinstance(arg, Rect):
        return [_quad(arg)]
    seq = list(arg)
    if seq and all(isinstance(v, (int, float)) for v in seq):
        return [_quad(seq)]
    return [_quad(q) for q in seq]


# Subtype string → PyMuPDF annotation-type int (PyMuPDF ``annot.type[0]``).
_ANNOT_TYPE_INT = {
    "Text": 0,
    "Link": 1,
    "FreeText": 2,
    "Line": 3,
    "Square": 4,
    "Circle": 5,
    "Polygon": 6,
    "PolyLine": 7,
    "Highlight": 8,
    "Underline": 9,
    "Squiggly": 10,
    "StrikeOut": 11,
    "Stamp": 13,
    "Caret": 14,
    "Ink": 15,
    "Popup": 16,
    "FileAttachment": 17,
    "Sound": 18,
    "Movie": 19,
    "Widget": 20,
    "Redact": 25,
}


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


class Annot:
    """A page annotation (PyMuPDF ``fitz.Annot``).

    Wraps a Rust ``_core.Annot``, exposing PyMuPDF's annotation surface and
    returning geometry value types (:class:`Rect`/:class:`Point`).
    """

    __slots__ = ("_annot",)

    def __init__(self, core_annot: "_core.Annot") -> None:
        self._annot = core_annot

    @property
    def rect(self) -> Rect:
        """The annotation rectangle (PyMuPDF ``annot.rect``)."""
        return _rect(self._annot.rect)

    @property
    def type(self) -> tuple[int, str]:
        """``(type_int, type_string)`` (PyMuPDF ``annot.type``)."""
        s = self._annot.type_string
        return (_ANNOT_TYPE_INT.get(s, -1), s)

    @property
    def xref(self) -> int:
        """The annotation object's ``xref`` (PyMuPDF ``annot.xref``)."""
        return self._annot.xref

    @property
    def info(self) -> dict:
        """The annotation info dict (PyMuPDF ``annot.info``).

        Always carries PyMuPDF's keys; ones the core doesn't track are ``""``.
        """
        core = self._annot.info()
        return {
            "content": core.get("content", ""),
            "name": core.get("name", ""),
            "title": core.get("title", ""),
            "creationDate": "",
            "modDate": "",
            "subject": "",
            "id": "",
        }

    @property
    def colors(self) -> dict:
        """``{"stroke": (r,g,b)|None, "fill": (r,g,b)|None}`` (PyMuPDF ``annot.colors``)."""
        return self._annot.colors()

    @property
    def opacity(self) -> float:
        """The annotation opacity ∈ [0, 1] (PyMuPDF ``annot.opacity``)."""
        return self._annot.opacity

    @property
    def flags(self) -> int:
        """The annotation flags bitfield (PyMuPDF ``annot.flags``)."""
        return self._annot.flags

    @property
    def border(self) -> dict:
        """``{"width": w, "dashes": [], "style": "S"}`` (PyMuPDF ``annot.border``)."""
        return {"width": self._annot.border_width, "dashes": [], "style": "S"}

    @property
    def vertices(self) -> list:
        """The annotation vertices as :class:`Point` list (PyMuPDF ``annot.vertices``)."""
        return [Point(*v) for v in self._annot.vertices()]

    @property
    def has_appearance(self) -> bool:
        """Whether the annotation carries an ``/AP`` appearance stream."""
        return self._annot.has_appearance

    def has_ap(self) -> bool:
        """Whether the annotation carries an ``/AP`` appearance (PyMuPDF ``annot.has_ap``)."""
        return self._annot.has_appearance

    # --- setters ---
    def set_rect(self, rect) -> None:
        """Sets the annotation rectangle (PyMuPDF ``annot.set_rect``)."""
        self._annot.set_rect(_rt(rect))

    def set_colors(self, colors=None, *, stroke=None, fill=None, **_ignored) -> None:
        """Sets the stroke/fill colors (PyMuPDF ``annot.set_colors``).

        Accepts a ``colors=`` dict (``{"stroke": ..., "fill": ...}``) or the
        ``stroke=``/``fill=`` keywords.
        """
        if colors is not None:
            stroke = colors.get("stroke", stroke)
            fill = colors.get("fill", fill)
        self._annot.set_colors(stroke=_color(stroke), fill=_color(fill))

    def set_opacity(self, opacity: float) -> None:
        """Sets the annotation opacity (PyMuPDF ``annot.set_opacity``)."""
        self._annot.set_opacity(float(opacity))

    def set_border(self, border=None, *, width=None, **_ignored) -> None:
        """Sets the border width (PyMuPDF ``annot.set_border``)."""
        if border is not None and isinstance(border, dict):
            width = border.get("width", width)
        self._annot.set_border(width=1.0 if width is None else float(width))

    def set_flags(self, flags: int) -> None:
        """Sets the annotation flags (PyMuPDF ``annot.set_flags``)."""
        self._annot.set_flags(int(flags))

    def set_info(self, info=None, *, content=None, title=None, name=None) -> None:
        """Sets the annotation info (PyMuPDF ``annot.set_info``).

        Accepts an ``info=`` dict or the ``content=``/``title=``/``name=`` keywords.
        """
        if info is not None:
            content = info.get("content", content)
            title = info.get("title", title)
            name = info.get("name", name)
        self._annot.set_info(content=content, title=title, name=name)

    def update(self, **_ignored) -> bool:
        """Regenerates the appearance stream (PyMuPDF ``annot.update``).

        Extra PyMuPDF kwargs (``opacity``/``blend_mode``/…) are accepted and
        ignored.
        """
        self._annot.update()
        return True

    # --- PyMuPDF deprecated camelCase aliases ---
    def setColors(self, *args, **kw) -> None:  # noqa: N802
        self.set_colors(*args, **kw)

    def setRect(self, rect) -> None:  # noqa: N802
        self.set_rect(rect)

    def setOpacity(self, opacity: float) -> None:  # noqa: N802
        self.set_opacity(opacity)

    def setBorder(self, *args, **kw) -> None:  # noqa: N802
        self.set_border(*args, **kw)

    def setInfo(self, *args, **kw) -> None:  # noqa: N802
        self.set_info(*args, **kw)

    def setFlags(self, flags: int) -> None:  # noqa: N802
        self.set_flags(flags)

    def __repr__(self) -> str:
        t = self._annot.type_string
        return f"<oxide_pdf.Annot {t!r} xref={self._annot.xref}>"


class Widget:
    """An AcroForm field widget (PyMuPDF ``fitz.Widget``).

    PyMuPDF's ``Widget`` is a mutable struct: set :attr:`field_value` then call
    :meth:`update`. This wrapper buffers the pending value the same way.
    """

    __slots__ = ("_widget", "_pending_value", "_has_pending")

    def __init__(self, core_widget: "_core.Widget") -> None:
        self._widget = core_widget
        self._pending_value = None
        self._has_pending = False

    @property
    def rect(self) -> Rect:
        """The widget rectangle (PyMuPDF ``widget.rect``)."""
        return _rect(self._widget.rect)

    @property
    def xref(self) -> int:
        """The widget object's ``xref`` (PyMuPDF ``widget.xref``)."""
        return self._widget.xref

    @property
    def field_type(self) -> int:
        """The PyMuPDF field-type int (PyMuPDF ``widget.field_type``)."""
        return self._widget.field_type

    @property
    def field_type_string(self) -> str:
        """The field-type name (PyMuPDF ``widget.field_type_string``)."""
        return self._widget.field_type_string

    @property
    def field_name(self) -> str | None:
        """The fully-qualified field name (PyMuPDF ``widget.field_name``)."""
        return self._widget.field_name

    @property
    def field_label(self) -> str | None:
        """The field's user label / ``/TU`` (PyMuPDF ``widget.field_label``)."""
        return self._widget.field_label

    @property
    def field_value(self):
        """The field value (PyMuPDF ``widget.field_value``).

        Returns the pending (set-but-not-yet-updated) value if one was assigned,
        else the value read from the document.
        """
        if self._has_pending:
            return self._pending_value
        return self._widget.field_value

    @field_value.setter
    def field_value(self, value) -> None:
        self._pending_value = value
        self._has_pending = True

    @property
    def field_flags(self) -> int:
        """The field flags bitfield (PyMuPDF ``widget.field_flags``)."""
        return self._widget.field_flags

    @property
    def choice_values(self) -> list[str]:
        """The choice options for combo/list fields (PyMuPDF ``widget.choice_values``)."""
        return self._widget.choice_values

    @property
    def button_states(self) -> list[str]:
        """The on-states for checkbox/radio fields (PyMuPDF ``widget.button_states``)."""
        return self._widget.button_states

    def update(self, value=None) -> None:
        """Writes the field value back to the document (PyMuPDF ``widget.update``)."""
        if value is not None:
            self._pending_value = value
            self._has_pending = True
        if self._has_pending:
            self._widget.update(self._pending_value)
            self._has_pending = False
            self._pending_value = None
        else:
            self._widget.update()

    def __repr__(self) -> str:
        return f"<oxide_pdf.Widget {self._widget.field_name!r} xref={self._widget.xref}>"


class Shape:
    """A reusable drawing canvas for one page (PyMuPDF ``fitz.Shape``).

    Build geometry with the ``draw_*`` methods, style it with :meth:`finish`,
    then write it to the page with :meth:`commit`.
    """

    __slots__ = ("_shape",)

    def __init__(self, core_shape: "_core.Shape") -> None:
        self._shape = core_shape

    def draw_line(self, p1, p2) -> Point:
        """Draws a line segment (PyMuPDF ``shape.draw_line``)."""
        self._shape.draw_line(_pt(p1), _pt(p2))
        return Point(*_pt(p2))

    def draw_rect(self, rect) -> Rect:
        """Draws a rectangle (PyMuPDF ``shape.draw_rect``)."""
        self._shape.draw_rect(_rt(rect))
        return _rect(_rt(rect))

    def draw_circle(self, center, radius) -> Point:
        """Draws a circle (PyMuPDF ``shape.draw_circle``)."""
        self._shape.draw_circle(_pt(center), float(radius))
        return Point(*_pt(center))

    def draw_oval(self, rect) -> Rect:
        """Draws an ellipse inscribed in ``rect`` (PyMuPDF ``shape.draw_oval``)."""
        self._shape.draw_oval(_rt(rect))
        return _rect(_rt(rect))

    def draw_bezier(self, p1, p2, p3, p4) -> Point:
        """Draws a cubic Bézier curve (PyMuPDF ``shape.draw_bezier``)."""
        self._shape.draw_bezier(_pt(p1), _pt(p2), _pt(p3), _pt(p4))
        return Point(*_pt(p4))

    def draw_polyline(self, points) -> Point:
        """Draws a connected polyline (PyMuPDF ``shape.draw_polyline``)."""
        pts = [_pt(p) for p in points]
        self._shape.draw_polyline(pts)
        return Point(*pts[-1]) if pts else Point()

    def draw_curve(self, points) -> Point:
        """Draws a smooth curve through ``points`` (PyMuPDF ``shape.draw_curve``)."""
        pts = [_pt(p) for p in points]
        self._shape.draw_curve(pts)
        return Point(*pts[-1]) if pts else Point()

    def finish(
        self,
        color=None,
        fill=None,
        width: float = 1.0,
        dashes=None,
        even_odd: bool = False,
        closePath: bool = False,  # noqa: N803  (PyMuPDF uses camelCase here)
        **_ignored,
    ) -> None:
        """Styles and closes the current drawing path (PyMuPDF ``shape.finish``)."""
        self._shape.finish(
            color=_color(color),
            fill=_color(fill),
            width=float(width),
            dashes=dashes,
            even_odd=bool(even_odd),
            close_path=bool(closePath),
        )

    def commit(self, overlay: bool = True) -> None:
        """Writes the accumulated drawing to the page (PyMuPDF ``shape.commit``)."""
        self._shape.commit(overlay=bool(overlay))

    def __repr__(self) -> str:
        return "<oxide_pdf.Shape>"


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


# ``Pixmap`` is the Rust ``_core.Pixmap`` directly (PyMuPDF ``fitz.Pixmap``,
# PRD §8.10 / §3.3). Using the native pyclass — rather than a pure-Python wrapper
# — means the zero-copy **buffer protocol** (``memoryview(pix)`` /
# ``numpy.frombuffer(pix)``) works on every supported interpreter (PEP 688's
# pure-Python ``__buffer__`` is 3.12-only; the native ``bf_getbuffer`` slot is
# universal on our ≥3.11 abi3 floor). The enforced copy-on-write lifetime
# contract (PRD §9.4) lives in the Rust class: a live view keeps the bytes alive
# past the Pixmap's GC, and in-place mutators copy-on-write under a live view.
#
# Construct a blank pixmap as ``Pixmap(colorspace, irect, alpha=False)`` where
# ``colorspace`` is a component count (1/3/4) or a name string, or obtain one
# from :meth:`Page.get_pixmap`. ``pix.samples`` is an owning ``bytes`` copy;
# ``memoryview(pix)`` is the zero-copy path.
Pixmap = _core.Pixmap

# ``DisplayList`` is the Rust ``_core.DisplayList`` directly (PyMuPDF
# ``fitz.DisplayList``, PRD §8.11). Obtain one from :meth:`Page.get_displaylist`
# and replay it with ``dl.get_pixmap(...)``.
DisplayList = _core.DisplayList


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

    # --- get_pixmap (PRD §3.3 / §8.10) ---
    def get_pixmap(
        self,
        *,
        matrix=None,
        dpi: int | None = None,
        colorspace=None,
        alpha: bool = False,
        clip=None,
    ) -> Pixmap:
        """Renders the page to a :class:`Pixmap` (PyMuPDF ``page.get_pixmap``).

        Renders **any** page (PRD §8.11): image-only pages take a fast
        native-raster path; vector / text / mixed pages are rasterized full-page
        (text, fills, strokes, images, clips, axial/radial shadings).

        ``matrix`` (a :class:`~oxide_pdf.Matrix` / 6-sequence) or ``dpi`` set the
        output resolution; ``colorspace`` selects Gray/RGB/CMYK; ``alpha`` adds
        an alpha channel; ``clip`` is a device-space sub-rectangle.
        """
        mtx = None
        if matrix is not None:
            m = tuple(float(v) for v in matrix)
            if len(m) != 6:
                raise ValueError("matrix must be a 6-sequence (a, b, c, d, e, f)")
            mtx = m
        cs = colorspace.n if hasattr(colorspace, "n") else colorspace
        return self._page.get_pixmap(
            matrix=mtx,
            dpi=dpi,
            colorspace=cs,
            alpha=alpha,
            clip=_as_clip(clip),
        )

    def get_displaylist(self) -> "DisplayList":
        """Records the page's drawcalls into a :class:`DisplayList` (PyMuPDF
        ``page.get_displaylist``). Replay with ``dl.get_pixmap(...)``."""
        return self._page.get_displaylist()

    @property
    def is_image_only(self) -> bool:
        """Whether the page is image-only (in scope for ``get_pixmap``)."""
        return self._page.is_image_only

    # --- links / labels / rotation (PRD §8.9) ---
    def get_links(self) -> list[dict]:
        """The page's link annotations (PyMuPDF ``page.get_links``).

        Each link is a dict with ``kind`` (0 none / 1 goto / 2 uri), ``from``
        (a :class:`Rect`), and ``uri``/``page`` as applicable, plus ``xref``.
        """
        out = []
        for link in self._page.get_links():
            link = dict(link)
            if "from" in link:
                link["from"] = _rect(link["from"])
            out.append(link)
        return out

    def insert_link(self, link: dict) -> None:
        """Inserts a link annotation (PyMuPDF ``page.insert_link``).

        ``link`` is a dict with ``kind`` (1 goto / 2 uri), ``from`` (a rect or
        4-sequence) and ``uri`` or ``page``.
        """
        spec = dict(link)
        if "from" in spec:
            fr = spec["from"]
            spec["from"] = (float(fr[0]), float(fr[1]), float(fr[2]), float(fr[3]))
        self._page.insert_link(spec)

    def delete_link(self, link: dict) -> None:
        """Deletes a link annotation by its ``xref`` (PyMuPDF ``page.delete_link``)."""
        self._page.delete_link(int(link["xref"]))

    def get_label(self) -> str:
        """The page's label under ``/PageLabels`` (PyMuPDF ``page.get_label``)."""
        return self._page.get_label()

    def set_rotation(self, rotation: int) -> None:
        """Sets the page rotation (PyMuPDF ``page.set_rotation``)."""
        self._page.set_rotation(int(rotation))

    # --- content insertion (PRD §8.8) ---
    def insert_text(
        self,
        point,
        text: str,
        *,
        fontname: str = "helv",
        fontsize: float = 11.0,
        color=(0, 0, 0),
        fontfile=None,
        **_ignored,
    ) -> int:
        """Writes ``text`` at ``point`` (PyMuPDF ``page.insert_text``).

        Returns the number of lines written. Extra PyMuPDF kwargs
        (``rotate``/``render_mode``/``encoding``/…) are accepted and ignored.
        """
        return self._page.insert_text(
            _pt(point),
            text,
            fontname=fontname,
            fontsize=float(fontsize),
            color=_color(color),
            fontfile=fontfile,
        )

    def insert_textbox(
        self,
        rect,
        text: str,
        *,
        fontname: str = "helv",
        fontsize: float = 11.0,
        color=(0, 0, 0),
        align: int = 0,
        fontfile=None,
        **_ignored,
    ) -> float:
        """Fills ``rect`` with wrapped ``text`` (PyMuPDF ``page.insert_textbox``).

        Returns the remaining vertical space (negative if overflowed).
        """
        return self._page.insert_textbox(
            _rt(rect),
            text,
            fontname=fontname,
            fontsize=float(fontsize),
            color=_color(color),
            align=int(align),
            fontfile=fontfile,
        )

    def insert_image(
        self,
        rect,
        *,
        stream=None,
        filename=None,
        pixmap=None,
        width: int = 0,
        height: int = 0,
        **_ignored,
    ) -> None:
        """Places an image in ``rect`` (PyMuPDF ``page.insert_image``).

        Supply the image as ``stream=`` bytes or ``filename=`` (read to bytes).
        A JPEG stream is detected automatically; pass ``width=``/``height=`` for
        raw RGB pixel data. ``pixmap=`` is not yet supported.
        """
        if pixmap is not None:
            raise PdfUnsupportedError(
                "Page.insert_image(pixmap=...) is not implemented yet; "
                "pass stream= bytes or filename=."
            )
        if stream is None and filename is not None:
            with __import__("builtins").open(os.fspath(filename), "rb") as fh:
                stream = fh.read()
        if stream is None:
            raise ValueError("insert_image() requires stream= or filename=")
        if width and height:
            self._page.insert_image(
                _rt(rect), stream=bytes(stream), width=int(width), height=int(height)
            )
        else:
            self._page.insert_image(_rt(rect), stream=bytes(stream))

    # --- vector drawing (PRD §8.8) ---
    def draw_line(self, p1, p2, *, color=(0, 0, 0), width: float = 1.0, **_ignored):
        """Draws a line segment (PyMuPDF ``page.draw_line``)."""
        self._page.draw_line(_pt(p1), _pt(p2), color=_color(color), width=float(width))

    def draw_rect(self, rect, *, color=(0, 0, 0), fill=None, width: float = 1.0, **_ignored):
        """Draws a rectangle (PyMuPDF ``page.draw_rect``)."""
        self._page.draw_rect(
            _rt(rect), color=_color(color), fill=_color(fill), width=float(width)
        )

    def draw_circle(self, center, radius, *, color=(0, 0, 0), fill=None, width: float = 1.0, **_ignored):
        """Draws a circle (PyMuPDF ``page.draw_circle``)."""
        self._page.draw_circle(
            _pt(center), float(radius), color=_color(color), fill=_color(fill), width=float(width)
        )

    def draw_oval(self, rect, *, color=(0, 0, 0), fill=None, width: float = 1.0, **_ignored):
        """Draws an ellipse inscribed in ``rect`` (PyMuPDF ``page.draw_oval``)."""
        self._page.draw_oval(
            _rt(rect), color=_color(color), fill=_color(fill), width=float(width)
        )

    def draw_bezier(self, p1, p2, p3, p4, *, color=(0, 0, 0), width: float = 1.0, **_ignored):
        """Draws a cubic Bézier curve (PyMuPDF ``page.draw_bezier``)."""
        self._page.draw_bezier(
            _pt(p1), _pt(p2), _pt(p3), _pt(p4), color=_color(color), width=float(width)
        )

    def draw_polyline(self, points, *, color=(0, 0, 0), width: float = 1.0, **_ignored):
        """Draws a connected polyline (PyMuPDF ``page.draw_polyline``)."""
        self._page.draw_polyline(
            [_pt(p) for p in points], color=_color(color), width=float(width)
        )

    def new_shape(self) -> Shape:
        """Builds a reusable :class:`Shape` for this page (PyMuPDF ``page.new_shape``)."""
        return Shape(self._page.new_shape())

    # --- annotations (PRD §8.8) ---
    def add_text_annot(self, point, text: str, *, icon: str = "Note", **_ignored) -> Annot:
        """Adds a sticky-note text annotation (PyMuPDF ``page.add_text_annot``)."""
        return Annot(self._page.add_text_annot(_pt(point), text, icon=icon))

    def add_freetext_annot(
        self,
        rect,
        text: str,
        *,
        fontsize: float = 11.0,
        text_color=(0, 0, 0),
        fill_color=None,
        align: int = 0,
        **_ignored,
    ) -> Annot:
        """Adds a free-text annotation (PyMuPDF ``page.add_freetext_annot``)."""
        return Annot(
            self._page.add_freetext_annot(
                _rt(rect),
                text,
                fontsize=float(fontsize),
                text_color=_color(text_color),
                fill_color=_color(fill_color),
                align=int(align),
            )
        )

    def add_highlight_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds a highlight annotation over ``quads`` (PyMuPDF ``page.add_highlight_annot``)."""
        return Annot(self._page.add_highlight_annot(_quads(quads)))

    def add_underline_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds an underline annotation over ``quads`` (PyMuPDF ``page.add_underline_annot``)."""
        return Annot(self._page.add_underline_annot(_quads(quads)))

    def add_strikeout_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds a strike-out annotation over ``quads`` (PyMuPDF ``page.add_strikeout_annot``)."""
        return Annot(self._page.add_strikeout_annot(_quads(quads)))

    def add_squiggly_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds a squiggly-underline annotation over ``quads`` (PyMuPDF ``page.add_squiggly_annot``)."""
        return Annot(self._page.add_squiggly_annot(_quads(quads)))

    def add_rect_annot(self, rect, *, color=(0, 0, 0), fill=None, **_ignored) -> Annot:
        """Adds a rectangle annotation (PyMuPDF ``page.add_rect_annot``)."""
        return Annot(
            self._page.add_rect_annot(_rt(rect), color=_color(color), fill=_color(fill))
        )

    def add_circle_annot(self, rect, *, color=(0, 0, 0), fill=None, **_ignored) -> Annot:
        """Adds a circle/ellipse annotation (PyMuPDF ``page.add_circle_annot``)."""
        return Annot(
            self._page.add_circle_annot(_rt(rect), color=_color(color), fill=_color(fill))
        )

    def add_line_annot(self, p1, p2, *, color=(0, 0, 0), **_ignored) -> Annot:
        """Adds a line annotation from ``p1`` to ``p2`` (PyMuPDF ``page.add_line_annot``)."""
        return Annot(self._page.add_line_annot(_pt(p1), _pt(p2), color=_color(color)))

    def add_polygon_annot(self, points, *, color=(0, 0, 0), fill=None, **_ignored) -> Annot:
        """Adds a polygon annotation through ``points`` (PyMuPDF ``page.add_polygon_annot``)."""
        return Annot(
            self._page.add_polygon_annot(
                [_pt(p) for p in points], color=_color(color), fill=_color(fill)
            )
        )

    def add_polyline_annot(self, points, *, color=(0, 0, 0), **_ignored) -> Annot:
        """Adds a polyline annotation through ``points`` (PyMuPDF ``page.add_polyline_annot``)."""
        return Annot(
            self._page.add_polyline_annot([_pt(p) for p in points], color=_color(color))
        )

    def add_ink_annot(self, handwriting, *, color=(0, 0, 0), **_ignored) -> Annot:
        """Adds a free-hand ink annotation (PyMuPDF ``page.add_ink_annot``).

        ``handwriting`` is a list of strokes; each stroke is a list of points.
        """
        strokes = [[_pt(p) for p in stroke] for stroke in handwriting]
        return Annot(self._page.add_ink_annot(strokes, color=_color(color)))

    def add_stamp_annot(self, rect, *, stamp: str = "Approved", **_ignored) -> Annot:
        """Adds a rubber-stamp annotation (PyMuPDF ``page.add_stamp_annot``)."""
        return Annot(self._page.add_stamp_annot(_rt(rect), stamp=stamp))

    def add_file_annot(self, point, buffer, filename: str, *, ufilename=None, desc=None, icon=None, **_ignored) -> Annot:
        """Adds a file-attachment annotation (PyMuPDF ``page.add_file_annot``)."""
        return Annot(self._page.add_file_annot(_pt(point), bytes(buffer), filename))

    def add_redact_annot(self, quad, *, text=None, fill=None, **_ignored) -> Annot:
        """Adds a redaction annotation over ``quad`` (PyMuPDF ``page.add_redact_annot``)."""
        return Annot(
            self._page.add_redact_annot(_rt(_rect_from_corners(_quad(quad))), fill=_color(fill), text=text)
        )

    def annots(self, types=None) -> Iterator[Annot]:
        """Iterates the page's annotations (PyMuPDF ``page.annots``).

        When ``types`` is given (a sequence of PyMuPDF annotation-type ints), only
        annotations of those types are yielded.
        """
        for core in self._page.annots():
            annot = Annot(core)
            if types is None or annot.type[0] in types:
                yield annot

    @property
    def first_annot(self) -> Annot | None:
        """The first annotation, or ``None`` (PyMuPDF ``page.first_annot``)."""
        core = self._page.first_annot
        return Annot(core) if core is not None else None

    def annot_xrefs(self) -> list[int]:
        """The xrefs of the page's annotations (PyMuPDF ``page.annot_xrefs``)."""
        return self._page.annot_xrefs()

    def annot_names(self) -> list[str]:
        """The ``/NM`` names of the page's annotations (PyMuPDF ``page.annot_names``)."""
        return self._page.annot_names()

    def delete_annot(self, annot) -> None:
        """Deletes an annotation (PyMuPDF ``page.delete_annot``).

        Accepts an :class:`Annot` or a bare ``xref`` int.
        """
        xref = annot.xref if isinstance(annot, Annot) else int(annot)
        self._page.delete_annot(xref)

    def apply_redactions(self, *args, **kwargs) -> int:
        """Applies pending redaction annotations (PyMuPDF ``page.apply_redactions``).

        Returns the number of redactions applied. PyMuPDF's ``images``/``graphics``/
        ``text`` kwargs are accepted and ignored.
        """
        return self._page.apply_redactions(*args, **kwargs)

    # --- vector + widget inventory (PRD §8.6) ---
    def get_drawings(self, **_ignored) -> list[dict]:
        """The page's vector drawings (PyMuPDF ``page.get_drawings``).

        Each drawing is a dict with a :class:`Rect` ``rect`` and an ``items`` list
        of path segments whose points are :class:`Point` (and rects :class:`Rect`),
        matching PyMuPDF's shapes.
        """
        return [self._wrap_drawing(d) for d in self._page.get_drawings()]

    def get_cdrawings(self, **_ignored) -> list[dict]:
        """The page's vector drawings as raw dicts (PyMuPDF ``page.get_cdrawings``).

        Like :meth:`get_drawings` but leaves geometry as plain tuples (faster).
        """
        return self._page.get_cdrawings()

    @staticmethod
    def _wrap_drawing(d: dict) -> dict:
        """Converts a core drawing dict's geometry to PyMuPDF value types."""
        out = dict(d)
        if "rect" in out and out["rect"] is not None:
            out["rect"] = _rect(out["rect"])
        items = out.get("items")
        if items:
            new_items = []
            for it in items:
                op = it[0]
                if op == "l":
                    new_items.append(("l", Point(*it[1]), Point(*it[2])))
                elif op == "c":
                    new_items.append(("c", Point(*it[1]), Point(*it[2]), Point(*it[3]), Point(*it[4])))
                elif op == "re":
                    new_items.append(("re", _rect(it[1])))
                else:
                    new_items.append(it)
            out["items"] = new_items
        return out

    def widgets(self) -> list[Widget]:
        """The page's form-field widgets (PyMuPDF ``page.widgets``)."""
        return [Widget(w) for w in self._page.widgets()]

    @property
    def first_widget(self) -> Widget | None:
        """The first form-field widget, or ``None`` (PyMuPDF ``page.first_widget``)."""
        core = self._page.first_widget
        return Widget(core) if core is not None else None

    # PyMuPDF deprecated camelCase aliases.
    def getPixmap(self, *args, **kw) -> Pixmap:  # noqa: N802
        return self.get_pixmap(*args, **kw)

    def getDisplayList(self) -> "DisplayList":  # noqa: N802
        return self.get_displaylist()

    def getImages(self, full: bool = False) -> list[tuple]:  # noqa: N802
        return self.get_images(full)

    def getLinks(self) -> list[dict]:  # noqa: N802
        return self.get_links()

    def setRotation(self, rotation: int) -> None:  # noqa: N802
        self.set_rotation(rotation)

    def insertText(self, *args, **kw) -> int:  # noqa: N802
        return self.insert_text(*args, **kw)

    def insertTextbox(self, *args, **kw) -> float:  # noqa: N802
        return self.insert_textbox(*args, **kw)

    def insertImage(self, *args, **kw):  # noqa: N802
        return self.insert_image(*args, **kw)

    def drawLine(self, *args, **kw):  # noqa: N802
        return self.draw_line(*args, **kw)

    def drawRect(self, *args, **kw):  # noqa: N802
        return self.draw_rect(*args, **kw)

    def drawCircle(self, *args, **kw):  # noqa: N802
        return self.draw_circle(*args, **kw)

    def drawOval(self, *args, **kw):  # noqa: N802
        return self.draw_oval(*args, **kw)

    def drawBezier(self, *args, **kw):  # noqa: N802
        return self.draw_bezier(*args, **kw)

    def drawPolyline(self, *args, **kw):  # noqa: N802
        return self.draw_polyline(*args, **kw)

    def newShape(self) -> Shape:  # noqa: N802
        return self.new_shape()

    def addTextAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_text_annot(*args, **kw)

    def addFreetextAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_freetext_annot(*args, **kw)

    def addHighlightAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_highlight_annot(*args, **kw)

    def addUnderlineAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_underline_annot(*args, **kw)

    def addStrikeoutAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_strikeout_annot(*args, **kw)

    def addSquigglyAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_squiggly_annot(*args, **kw)

    def addRectAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_rect_annot(*args, **kw)

    def addCircleAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_circle_annot(*args, **kw)

    def addLineAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_line_annot(*args, **kw)

    def addPolygonAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_polygon_annot(*args, **kw)

    def addPolylineAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_polyline_annot(*args, **kw)

    def addInkAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_ink_annot(*args, **kw)

    def addStampAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_stamp_annot(*args, **kw)

    def addFileAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_file_annot(*args, **kw)

    def addRedactAnnot(self, *args, **kw) -> Annot:  # noqa: N802
        return self.add_redact_annot(*args, **kw)

    def applyRedactions(self, *args, **kw) -> int:  # noqa: N802
        return self.apply_redactions(*args, **kw)

    def getDrawings(self, **kw) -> list[dict]:  # noqa: N802
        return self.get_drawings(**kw)

    def getCdrawings(self, **kw) -> list[dict]:  # noqa: N802
        return self.get_cdrawings(**kw)

    def deleteAnnot(self, annot) -> None:  # noqa: N802
        self.delete_annot(annot)

    @property
    def firstAnnot(self) -> Annot | None:  # noqa: N802
        return self.first_annot

    @property
    def firstWidget(self) -> Widget | None:  # noqa: N802
        return self.first_widget

    def __repr__(self) -> str:
        return f"<oxide_pdf.Page number={self.number}>"

    def __getattr__(self, name: str):
        hint = _UNIMPLEMENTED_PAGE.get(name)
        if hint is not None:
            raise PdfUnsupportedError(
                f"Page.{name} is not implemented yet: {hint}. "
                "See the oxide_pdf parity matrix."
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

    # --- extract_image (PRD §8.10) ---
    def extract_image(self, xref: int) -> dict:
        """The image XObject ``xref`` as a PyMuPDF-shaped dict (PyMuPDF
        ``doc.extract_image``): ``ext``, ``colorspace``, ``bpc``, ``width``,
        ``height``, ``n``, ``smask``, ``image`` (bytes)."""
        return self._doc.extract_image(int(xref))

    def get_page_pixmap(self, pno: int, **kw) -> Pixmap:
        """Renders page ``pno`` to a :class:`Pixmap` (PyMuPDF
        ``doc.get_page_pixmap``)."""
        return self.load_page(int(pno)).get_pixmap(**kw)

    def extractImage(self, xref: int) -> dict:  # noqa: N802
        return self.extract_image(xref)

    # --- save (PRD §8.7 / §8.4) ---
    def save(
        self,
        filename: str | os.PathLike[str],
        *,
        garbage: int = 0,
        deflate: bool = False,
        incremental: bool = False,
        encryption: int | None = None,
        owner_pw: str | None = None,
        user_pw: str | None = None,
        permissions: int = -1,
        **_ignored,
    ) -> None:
        """Saves the document (PyMuPDF ``doc.save``).

        ``garbage`` 0–4, ``deflate`` compresses streams, ``incremental`` appends,
        ``encryption`` selects a method (``PDF_ENCRYPT_*``).
        """
        self._doc.save(
            os.fspath(filename),
            garbage=garbage,
            deflate=deflate,
            incremental=incremental,
            encryption=encryption,
            owner_pw=owner_pw,
            user_pw=user_pw,
            permissions=permissions,
        )

    def tobytes(
        self,
        *,
        garbage: int = 0,
        deflate: bool = False,
        incremental: bool = False,
        encryption: int | None = None,
        owner_pw: str | None = None,
        user_pw: str | None = None,
        permissions: int = -1,
        **_ignored,
    ) -> bytes:
        """Serializes the document to bytes (PyMuPDF ``doc.tobytes``/``write``)."""
        return self._doc.tobytes(
            garbage=garbage,
            deflate=deflate,
            incremental=incremental,
            encryption=encryption,
            owner_pw=owner_pw,
            user_pw=user_pw,
            permissions=permissions,
        )

    write = tobytes

    def ez_save(self, filename: str | os.PathLike[str], **kwargs) -> None:
        """PyMuPDF ``ez_save`` — save with garbage collection + deflate defaults."""
        kwargs.setdefault("garbage", 3)
        kwargs.setdefault("deflate", True)
        self.save(filename, **kwargs)

    def saveIncr(self, filename: str | os.PathLike[str] | None = None) -> None:  # noqa: N802
        """PyMuPDF deprecated alias: incremental save."""
        if filename is None:
            raise ValueError("saveIncr() requires the original filename")
        self._doc.saveIncr(os.fspath(filename))

    # --- metadata write (PRD §8.9) ---
    def set_metadata(self, metadata: dict) -> None:
        """Writes the ``/Info`` metadata dict (PyMuPDF ``doc.set_metadata``)."""
        self._doc.set_metadata({k: ("" if v is None else str(v)) for k, v in metadata.items()})

    def setMetadata(self, metadata: dict) -> None:  # noqa: N802
        self.set_metadata(metadata)

    def get_xml_metadata(self) -> str:
        """The catalog XMP metadata string (PyMuPDF ``doc.get_xml_metadata``)."""
        return self._doc.get_xml_metadata()

    def set_xml_metadata(self, xml: str) -> None:
        """Sets the catalog XMP metadata stream (PyMuPDF ``doc.set_xml_metadata``)."""
        self._doc.set_xml_metadata(xml)

    # --- TOC (PRD §8.9) ---
    def get_toc(self, simple: bool = True) -> list[list]:
        """The outline as ``[[level, title, page], …]`` (PyMuPDF ``doc.get_toc``)."""
        return [list(row) for row in self._doc.get_toc(simple)]

    def getToC(self, simple: bool = True) -> list[list]:  # noqa: N802
        return self.get_toc(simple)

    def set_toc(self, toc: list) -> None:
        """Builds the ``/Outlines`` tree (PyMuPDF ``doc.set_toc``). Raises on a
        level jump."""
        self._doc.set_toc([list(row) for row in toc])

    def setToC(self, toc: list) -> None:  # noqa: N802
        self.set_toc(toc)

    # --- page ops + merge (PRD §8.7) ---
    def insert_pdf(
        self,
        docsrc: "Document",
        from_page: int | None = None,
        to_page: int | None = None,
        start_at: int | None = None,
        **_ignored,
    ) -> None:
        """Inserts pages from ``docsrc`` (PyMuPDF ``doc.insert_pdf``)."""
        self._doc.insert_pdf(
            docsrc._doc, from_page=from_page, to_page=to_page, start_at=start_at
        )

    def insertPDF(self, docsrc: "Document", **kwargs) -> None:  # noqa: N802
        self.insert_pdf(docsrc, **kwargs)

    def new_page(self, pno: int = -1, width: float = 595.0, height: float = 842.0) -> Page:
        """Inserts a blank page, returning it (PyMuPDF ``doc.new_page``)."""
        return Page(self._doc.new_page(pno, width, height))

    def newPage(self, pno: int = -1, width: float = 595.0, height: float = 842.0) -> Page:  # noqa: N802
        return self.new_page(pno, width, height)

    def delete_page(self, pno: int = -1) -> None:
        """Deletes the page at ``pno`` (PyMuPDF ``doc.delete_page``)."""
        if pno < 0:
            pno += self._doc.page_count
        self._doc.delete_page(pno)

    def select(self, pages: list[int]) -> None:
        """Keeps only ``pages`` in order (PyMuPDF ``doc.select``)."""
        self._doc.select([int(p) for p in pages])

    def get_page_label(self, pno: int) -> str:
        """The page label of physical page ``pno`` (PyMuPDF helper)."""
        return self._doc.get_page_label(pno)

    # --- AcroForm forms (PRD §8.8) ---
    @property
    def is_form_pdf(self) -> bool:
        """Whether the document has an AcroForm (PyMuPDF ``doc.is_form_pdf``)."""
        return self._doc.is_form_pdf

    def form_field_names(self) -> list[str]:
        """The fully-qualified names of all form fields (PyMuPDF helper)."""
        return self._doc.form_field_names()

    def form_fill(self, name: str, value) -> None:
        """Sets the value of the form field ``name`` (PyMuPDF helper)."""
        self._doc.form_fill(name, value)

    def form_flatten(self) -> None:
        """Flattens (bakes) all form fields into page content (PyMuPDF helper)."""
        self._doc.form_flatten()

    # --- embedded files (PRD §8.9) ---
    def embfile_add(
        self,
        name: str,
        buffer,
        filename: str | None = None,
        ufilename: str | None = None,
        desc: str | None = None,
        **_ignored,
    ) -> None:
        """Embeds a file in the document (PyMuPDF ``doc.embfile_add``)."""
        self._doc.embfile_add(
            name,
            bytes(buffer),
            filename=filename,
            ufilename=ufilename,
            desc=desc,
        )

    def embfile_get(self, name: str) -> bytes:
        """The bytes of the embedded file ``name`` (PyMuPDF ``doc.embfile_get``)."""
        return self._doc.embfile_get(name)

    def embfile_del(self, name: str) -> None:
        """Deletes the embedded file ``name`` (PyMuPDF ``doc.embfile_del``)."""
        self._doc.embfile_del(name)

    def embfile_names(self) -> list[str]:
        """The names of all embedded files (PyMuPDF ``doc.embfile_names``)."""
        return self._doc.embfile_names()

    def embfile_count(self) -> int:
        """The number of embedded files (PyMuPDF ``doc.embfile_count``)."""
        return self._doc.embfile_count()

    def embfile_info(self, name: str) -> dict:
        """The metadata of the embedded file ``name`` (PyMuPDF ``doc.embfile_info``)."""
        return self._doc.embfile_info(name)

    # --- sanitize / bake (PRD §8.8) ---
    def scrub(
        self,
        *,
        attached_files: bool = True,
        clean_pages: bool = True,
        embedded_files: bool = True,
        hidden_text: bool = True,
        javascript: bool = True,
        metadata: bool = True,
        redactions: bool = True,
        redact_images: int = 0,
        remove_links: bool = False,
        reset_fields: bool = True,
        reset_responses: bool = True,
        thumbnails: bool = True,
        xml_metadata: bool = True,
        **_ignored,
    ) -> None:
        """Removes sensitive content (PyMuPDF ``doc.scrub``).

        PyMuPDF's full set of toggles is accepted; the ones the core implements are
        metadata, javascript, attached/embedded files, links and XMP metadata.
        """
        self._doc.scrub(
            metadata=metadata,
            javascript=javascript,
            attached_files=(attached_files or embedded_files),
            remove_links=remove_links,
            xml_metadata=xml_metadata,
        )

    def bake(self, *, annots: bool = True, widgets: bool = True, **_ignored) -> None:
        """Bakes annotations and/or form widgets into page content (PyMuPDF ``doc.bake``)."""
        self._doc.bake(annots=annots, widgets=widgets)

    # --- PyMuPDF deprecated camelCase aliases ---
    @property
    def isFormPDF(self) -> bool:  # noqa: N802
        return self.is_form_pdf

    def embfileAdd(self, *args, **kw) -> None:  # noqa: N802
        return self.embfile_add(*args, **kw)

    def embfileGet(self, name: str) -> bytes:  # noqa: N802
        return self.embfile_get(name)

    def embfileDel(self, name: str) -> None:  # noqa: N802
        return self.embfile_del(name)

    def embfileNames(self) -> list[str]:  # noqa: N802
        return self.embfile_names()

    def embfileCount(self) -> int:  # noqa: N802
        return self.embfile_count()

    def embfileInfo(self, name: str) -> dict:  # noqa: N802
        return self.embfile_info(name)

    def close(self) -> None:
        """Releases the document (drops the underlying Rust handle)."""
        self._doc = None  # type: ignore[assignment]

    def __enter__(self) -> "Document":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def __repr__(self) -> str:
        return f"<oxide_pdf.Document page_count={self.page_count}>"

    def __getattr__(self, name: str):
        hint = _UNIMPLEMENTED_DOC.get(name)
        if hint is not None:
            raise PdfUnsupportedError(
                f"Document.{name} is not implemented yet: {hint}. "
                "See the oxide_pdf parity matrix."
            )
        raise AttributeError(f"'Document' object has no attribute {name!r}")


# A minimal, empty PDF used as the seed for ``open()`` with no arguments
# (PyMuPDF ``fitz.open()`` returns a new, empty PDF).
_BLANK_PDF = (
    b"%PDF-1.7\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
    b"2 0 obj<</Type/Pages/Kids[]/Count 0>>endobj\n"
    b"trailer<</Root 1 0 R>>\n"
    b"%%EOF"
)


def open(
    filename: str | os.PathLike[str] | None = None,
    *,
    stream: bytes | None = None,
    filetype: str | None = None,
) -> Document:
    """Opens a document (PyMuPDF ``fitz.open``).

    Pass a path positionally, or in-memory bytes via ``stream=``. Called with no
    arguments, returns a new, empty PDF (PyMuPDF ``fitz.open()``). The heavy parse
    runs with the GIL released in the Rust core (PRD §9.4).
    """
    if stream is not None:
        return Document(_core.open_bytes(bytes(stream)))
    if filename is None:
        return Document(_core.open_bytes(_BLANK_PDF))
    return Document(_core.open(os.fspath(filename)))
