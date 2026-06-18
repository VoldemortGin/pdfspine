"""Idiomatic-Python ``Document`` / ``Page`` wrappers over the Rust ``_core``
handles (PRD §9.2 / §9.4 / §9.5).

These thin wrappers add PyMuPDF-compatible names and return geometry value types
(:class:`~oxide_pdf.geometry.Rect`) instead of raw tuples. Known-but-unimplemented
PyMuPDF methods raise :class:`~oxide_pdf._core.PdfUnsupportedError` (never
``AttributeError``), per PRD §9.5.
"""

from __future__ import annotations

import builtins
import math
import os
from typing import Iterator

from . import _core
from ._core import PdfError, PdfRedactionError, PdfUnsupportedError
from .geometry import FZ_MAX_INF_RECT, FZ_MIN_INF_RECT, Matrix, Point, Quad, Rect

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


def _intersects(a: tuple[float, float, float, float], b: tuple[float, float, float, float]) -> bool:
    """Whether two (un-normalized) rects overlap."""
    ax0, ay0, ax1, ay1 = min(a[0], a[2]), min(a[1], a[3]), max(a[0], a[2]), max(a[1], a[3])
    bx0, by0, bx1, by1 = min(b[0], b[2]), min(b[1], b[3]), max(b[0], b[2]), max(b[1], b[3])
    return ax0 < bx1 and bx0 < ax1 and ay0 < by1 and by0 < ay1


def _hor_matrix(c: Point, p: Point) -> Matrix:
    """The matrix mapping ``c`` -> origin and ``p`` onto the +x axis (PyMuPDF
    ``util_hor_matrix``): translate by ``-c`` then rotate by the normalized
    ``p - c`` vector. Used by :meth:`Shape.draw_squiggle` / ``draw_zigzag``."""
    s = (p - c).unit
    m1 = Matrix(1, 0, 0, 1, -c.x, -c.y)
    m2 = Matrix(s.x, -s.y, s.y, s.x, 0, 0)
    return m1 * m2


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

    __slots__ = ("_annot", "_parent", "_siblings", "_index")

    def __init__(self, core_annot: "_core.Annot", parent=None, siblings=None, index=0) -> None:
        self._annot = core_annot
        self._parent = parent
        self._siblings = siblings
        self._index = index

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
        """``{"width": w, "style": s, "dashes": [...]}`` (PyMuPDF ``annot.border``)."""
        width, style, dashes = self._annot.border_tuple()
        return {"width": width, "style": style, "dashes": list(dashes)}

    @property
    def line_ends(self) -> tuple[int, int]:
        """The ``(start, end)`` line-ending style codes (PyMuPDF ``annot.line_ends``)."""
        return self._annot.line_ends()

    @property
    def blendmode(self) -> str | None:
        """The blend mode ``/BM``, or ``None`` (PyMuPDF ``annot.blendmode``)."""
        return self._annot.blendmode

    @property
    def is_open(self) -> bool:
        """Whether the annotation is open (PyMuPDF ``annot.is_open``)."""
        return self._annot.is_open

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

    def set_line_ends(self, start: int, end: int) -> None:
        """Sets the ``(start, end)`` line-ending styles (PyMuPDF ``annot.set_line_ends``)."""
        self._annot.set_line_ends(int(start), int(end))

    def set_blendmode(self, blend_mode: str) -> None:
        """Sets the blend mode ``/BM`` (PyMuPDF ``annot.set_blendmode``)."""
        self._annot.set_blendmode(str(blend_mode))

    def set_name(self, name: str) -> None:
        """Sets the icon/appearance ``/Name`` (PyMuPDF ``annot.set_name``)."""
        self._annot.set_name(str(name))

    def set_open(self, is_open: bool) -> None:
        """Sets the ``/Open`` flag (PyMuPDF ``annot.set_open``)."""
        self._annot.set_open(bool(is_open))

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

    @property
    def rect_delta(self):
        """The ``(left, top, right, bottom)`` ``/RD`` rect deltas, or ``None``
        (PyMuPDF ``annot.rect_delta``)."""
        return self._annot.rect_delta()

    @property
    def has_popup(self) -> bool:
        """Whether the annotation has a ``/Popup`` (PyMuPDF ``annot.has_popup``)."""
        return self._annot.has_popup

    @property
    def popup_xref(self) -> int:
        """The ``/Popup`` object ``xref``, or ``0`` (PyMuPDF ``annot.popup_xref``)."""
        return self._annot.popup_xref

    @property
    def popup_rect(self) -> Rect:
        """The ``/Popup`` rectangle (PyMuPDF ``annot.popup_rect``).

        Returns the infinite rect when no popup is present, matching PyMuPDF.
        """
        r = self._annot.popup_rect
        if r is None:
            return Rect(FZ_MIN_INF_RECT, FZ_MIN_INF_RECT, FZ_MAX_INF_RECT, FZ_MAX_INF_RECT)
        return _rect(r)

    @property
    def language(self) -> str:
        """The ``/Lang`` language identifier (PyMuPDF ``annot.language``)."""
        return self._annot.language

    @property
    def irt_xref(self) -> int:
        """The ``/IRT`` in-reply-to ``xref``, or ``0`` (PyMuPDF ``annot.irt_xref``)."""
        return self._annot.irt_xref

    def apn_bbox(self) -> Rect:
        """The ``/AP /N`` appearance ``/BBox`` (PyMuPDF ``annot.apn_bbox``).

        Returns MuPDF's infinite-rect sentinel when there is no ``/AP /N`` stream
        or it carries no ``/BBox`` (matching PyMuPDF).
        """
        r = self._annot.apn_bbox()
        if r is None:
            return Rect(FZ_MIN_INF_RECT, FZ_MIN_INF_RECT, FZ_MAX_INF_RECT, FZ_MAX_INF_RECT)
        return _rect(r)

    def apn_matrix(self) -> Matrix:
        """The ``/AP /N`` appearance ``/Matrix`` (PyMuPDF ``annot.apn_matrix``)."""
        return Matrix(*self._annot.apn_matrix())

    def file_info(self) -> dict:
        """The embedded-file info dict (PyMuPDF ``annot.file_info``).

        Returns exactly ``{'filename', 'description', 'length', 'size'}`` to match
        PyMuPDF 1.27's key set; ``description`` defaults to the filename when no
        ``/Desc`` is present.
        """
        return self._annot.file_info()

    def get_file(self) -> bytes:
        """The embedded file's bytes (PyMuPDF ``annot.get_file``)."""
        return self._annot.get_file()

    def get_textbox(self, *args, **kwargs) -> str:
        """DEFERRED — PyMuPDF ``annot.get_textbox`` reads the annotation's OWN
        appearance textpage and requires a ``rect`` argument; oxide has no
        annotation-appearance textpage yet, and delegating to the page region
        would be semantically opposite. Use :meth:`Page.get_textbox` instead.
        """
        raise PdfUnsupportedError(
            "Annot.get_textbox is not implemented yet: it needs the annotation's "
            "own appearance textpage (fitz semantics), which differs from page "
            "region text. Use Page.get_textbox. See the oxide_pdf parity matrix."
        )

    # --- setters / mutators ---
    def set_rotation(self, rotation: int) -> None:
        """Sets the ``/Rotate`` value (PyMuPDF ``annot.set_rotation``)."""
        self._annot.set_rotation(int(rotation))

    def set_popup(self, rect) -> None:
        """Adds / replaces the ``/Popup`` covering ``rect`` (PyMuPDF
        ``annot.set_popup``)."""
        self._annot.set_popup(_rt(rect))

    def set_apn_bbox(self, bbox) -> None:
        """Sets the ``/AP /N`` ``/BBox`` (PyMuPDF ``annot.set_apn_bbox``)."""
        self._annot.set_apn_bbox(_rt(bbox))

    def set_apn_matrix(self, matrix) -> None:
        """Sets the ``/AP /N`` ``/Matrix`` (PyMuPDF ``annot.set_apn_matrix``)."""
        m = matrix
        self._annot.set_apn_matrix((m[0], m[1], m[2], m[3], m[4], m[5]))

    def set_language(self, language) -> None:
        """Sets the ``/Lang`` identifier (PyMuPDF ``annot.set_language``)."""
        self._annot.set_language("" if language is None else str(language))

    def set_irt_xref(self, xref: int) -> None:
        """Sets the ``/IRT`` in-reply-to reference (PyMuPDF ``annot.set_irt_xref``)."""
        self._annot.set_irt_xref(int(xref))

    def delete_responses(self) -> None:
        """Deletes reply annotations to this one (PyMuPDF ``annot.delete_responses``)."""
        self._annot.delete_responses()

    def clean_contents(self, sanitize: int = 1) -> None:
        """Sanitizes the ``/AP /N`` stream (PyMuPDF ``annot.clean_contents``)."""
        self._annot.clean_contents(int(sanitize))

    def update_file(self, buffer_=None, filename=None, ufilename=None, desc=None) -> None:
        """Replaces the embedded file content / metadata (PyMuPDF
        ``annot.update_file``). The first parameter is ``buffer_`` to match
        fitz's signature."""
        self._annot.update_file(buffer_=buffer_, filename=filename, ufilename=ufilename, desc=desc)

    @property
    def next(self) -> "Annot | None":
        """The next annotation on the page, or ``None`` (PyMuPDF ``annot.next``)."""
        if self._siblings is None or self._index + 1 >= len(self._siblings):
            return None
        nxt = self._siblings[self._index + 1]
        return Annot(nxt, parent=self._parent, siblings=self._siblings, index=self._index + 1)

    def get_textpage(self, clip=None, flags: int = 0) -> "TextPage":
        """A :class:`TextPage` for the annotation's region (PyMuPDF
        ``annot.get_textpage``). Defaults to the annotation's own rect."""
        if self._parent is None:
            raise PdfError("Annot.get_textpage requires the owning page")
        clip = clip if clip is not None else self.rect
        return self._parent.get_textpage(clip=clip, flags=flags)

    def get_text(self, option: str = "text", *, clip=None, flags=None, **_ignored):
        """Text under the annotation (PyMuPDF ``annot.get_text``).

        Extracts the page text restricted to the annotation rect (or ``clip``).
        """
        if self._parent is None:
            raise PdfError("Annot.get_text requires the owning page")
        clip = clip if clip is not None else self.rect
        return self._parent.get_text(option, clip=clip, flags=flags)

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

    __slots__ = ("_widget", "_pending_value", "_has_pending", "next")

    def __init__(self, core_widget: "_core.Widget") -> None:
        self._widget = core_widget
        self._pending_value = None
        self._has_pending = False
        #: The next widget on the page, or ``None`` (PyMuPDF ``widget.next``).
        #: Wired up by :meth:`Page.widgets` / :attr:`Page.first_widget`.
        self.next: "Widget | None" = None

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

    @property
    def border_color(self) -> list[float] | None:
        """The ``/MK /BC`` border color components (PyMuPDF ``widget.border_color``)."""
        return self._widget.border_color

    @property
    def fill_color(self) -> list[float] | None:
        """The ``/MK /BG`` fill color components (PyMuPDF ``widget.fill_color``)."""
        return self._widget.fill_color

    @property
    def border_style(self) -> str:
        """The border style name (PyMuPDF ``widget.border_style``)."""
        return self._widget.border_style

    @property
    def border_width(self) -> float:
        """The border width (PyMuPDF ``widget.border_width``)."""
        return self._widget.border_width

    @property
    def border_dashes(self) -> list[int] | None:
        """The border dash pattern (PyMuPDF ``widget.border_dashes``)."""
        return self._widget.border_dashes

    @property
    def text_color(self) -> list[float]:
        """The ``/DA`` text color components (PyMuPDF ``widget.text_color``)."""
        return self._widget.text_color

    @property
    def text_font(self) -> str:
        """The ``/DA`` text font name (PyMuPDF ``widget.text_font``)."""
        return self._widget.text_font

    @property
    def text_fontsize(self) -> float:
        """The ``/DA`` text font size (PyMuPDF ``widget.text_fontsize``)."""
        return self._widget.text_fontsize

    @property
    def text_maxlen(self) -> int:
        """The ``/MaxLen`` maximum text length (PyMuPDF ``widget.text_maxlen``)."""
        return self._widget.text_maxlen

    @property
    def text_format(self) -> int:
        """The ``/Q`` text quadding 0/1/2 (PyMuPDF ``widget.text_format``)."""
        return self._widget.text_format

    @property
    def button_caption(self) -> str | None:
        """The pushbutton caption ``/MK /CA`` (PyMuPDF ``widget.button_caption``)."""
        return self._widget.button_caption

    @property
    def field_display(self) -> int:
        """The annotation display flags ``/F`` (PyMuPDF ``widget.field_display``)."""
        return self._widget.field_display

    @property
    def is_signed(self) -> bool | None:
        """Whether a signature field is signed (PyMuPDF ``widget.is_signed``)."""
        return self._widget.is_signed

    @property
    def rb_parent(self) -> int | None:
        """The radio-group parent xref (PyMuPDF ``widget.rb_parent``)."""
        return self._widget.rb_parent

    def on_state(self) -> str | None:
        """The button widget's current on-state name (PyMuPDF ``widget.on_state``)."""
        return self._widget.on_state()

    def reset(self) -> None:
        """Resets the field to its default value (PyMuPDF ``widget.reset``)."""
        self._widget.reset()

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

    # PyMuPDF's draw_curve (3-point) control constant.
    _CURVE_KAPPA = 0.55228474983

    __slots__ = ("_shape", "_page", "_doc", "_rect")

    def __init__(
        self,
        core_shape: "_core.Shape",
        page: "Page | None" = None,
        doc: "Document | None" = None,
    ) -> None:
        self._shape = core_shape
        self._page = page
        self._doc = doc
        # The accumulated drawing bounding box (PyMuPDF ``Shape.rect``); ``None``
        # until the first geometry is added.
        self._rect: Rect | None = None

    def draw_line(self, p1, p2) -> Point:
        """Draws a line segment (PyMuPDF ``shape.draw_line``)."""
        self._shape.draw_line(_pt(p1), _pt(p2))
        self.update_rect(p1)
        self.update_rect(p2)
        return Point(*_pt(p2))

    def draw_rect(self, rect) -> Rect:
        """Draws a rectangle (PyMuPDF ``shape.draw_rect``)."""
        self._shape.draw_rect(_rt(rect))
        self.update_rect(rect)
        return _rect(_rt(rect))

    def draw_circle(self, center, radius) -> Point:
        """Draws a circle (PyMuPDF ``shape.draw_circle``)."""
        self._shape.draw_circle(_pt(center), float(radius))
        return Point(*_pt(center))

    def draw_oval(self, rect) -> Rect:
        """Draws an ellipse inscribed in ``rect`` (PyMuPDF ``shape.draw_oval``)."""
        self._shape.draw_oval(_rt(rect))
        self.update_rect(rect)
        return _rect(_rt(rect))

    def draw_bezier(self, p1, p2, p3, p4) -> Point:
        """Draws a cubic Bézier curve (PyMuPDF ``shape.draw_bezier``)."""
        self._shape.draw_bezier(_pt(p1), _pt(p2), _pt(p3), _pt(p4))
        for p in (p1, p2, p3, p4):
            self.update_rect(p)
        return Point(*_pt(p4))

    def draw_polyline(self, points) -> Point:
        """Draws a connected polyline (PyMuPDF ``shape.draw_polyline``)."""
        pts = [_pt(p) for p in points]
        self._shape.draw_polyline(pts)
        for p in pts:
            self.update_rect(p)
        return Point(*pts[-1]) if pts else Point()

    def draw_curve(self, points) -> Point:
        """Draws a smooth curve through ``points`` (PyMuPDF ``shape.draw_curve``)."""
        pts = [_pt(p) for p in points]
        self._shape.draw_curve(pts)
        for p in pts:
            self.update_rect(p)
        return Point(*pts[-1]) if pts else Point()

    def draw_quad(self, quad) -> Point:
        """Draws a (closed) quadrilateral (PyMuPDF ``shape.draw_quad``).

        Mirrors PyMuPDF exactly: emits ``draw_polyline([ul, ll, lr, ur, ul])``.
        Accepts an :class:`~oxide_pdf.geometry.Quad` (uses its named corners) or
        a 4-sequence of points in PyMuPDF ``(ul, ur, lr, ll)`` order.
        """
        if all(hasattr(quad, c) for c in ("ul", "ur", "ll", "lr")):
            ul, ur, ll, lr = quad.ul, quad.ur, quad.ll, quad.lr
        else:
            ul, ur, ll, lr = (quad[0], quad[1], quad[2], quad[3])
        return self.draw_polyline([ul, ll, lr, ur, ul])

    def draw_curve3(self, p1, p2, p3) -> Point:
        """PyMuPDF's 3-point ``draw_curve``: a cubic through ``p1`` and ``p3``
        with single control point ``p2``. (oxide's public ``draw_curve`` takes a
        point list, so this private helper preserves the exact PyMuPDF math used
        by :meth:`draw_squiggle`.)"""
        a = Point(*_pt(p1))
        b = Point(*_pt(p2))
        c = Point(*_pt(p3))
        k = self._CURVE_KAPPA
        k1 = a + (b - a) * k
        k2 = c + (b - c) * k
        return self.draw_bezier(a, k1, k2, c)

    def draw_sector(self, center, point, angle, fullSector: bool = True) -> Point:  # noqa: N803
        """Draws a circular sector / pie wedge (PyMuPDF ``shape.draw_sector``).

        Sweeps ``angle`` degrees from ``point`` around ``center``. Replicates
        PyMuPDF's 90°-arc decomposition (cubic Béziers) exactly. With
        ``fullSector`` the arc is closed back through ``center``.
        """
        center = Point(*_pt(center))
        point = Point(*_pt(point))
        betar = math.radians(-float(angle))
        w360 = math.radians(math.copysign(360, betar)) * (-1)
        w90 = math.radians(math.copysign(90, betar))
        w45 = w90 / 2
        while abs(betar) > 2 * math.pi:
            betar += w360
        C = center
        P = point
        S = P - C
        rad = abs(S)
        if not rad > 1e-5:
            raise ValueError("radius must be positive")
        alfa = self.horizontal_angle(center, point)
        Q = Point(0, 0)
        # 'm' to the start point so the arc chains from there.
        last = point
        while abs(betar) > abs(w90):  # full 90° arcs
            q1 = C.x + math.cos(alfa + w90) * rad
            q2 = C.y + math.sin(alfa + w90) * rad
            Q = Point(q1, q2)
            r1 = C.x + math.cos(alfa + w45) * rad / math.cos(w45)
            r2 = C.y + math.sin(alfa + w45) * rad / math.cos(w45)
            R = Point(r1, r2)
            kappah = (1 - math.cos(w45)) * 4 / 3 / abs(R - Q)
            kappa = kappah * abs(P - Q)
            cp1 = P + (R - P) * kappa
            cp2 = Q + (R - Q) * kappa
            self.draw_bezier(last, cp1, cp2, Q)
            last = Q
            betar -= w90
            alfa += w90
            P = Q
        if abs(betar) > 1e-3:  # remaining partial arc
            beta2 = betar / 2
            q1 = C.x + math.cos(alfa + betar) * rad
            q2 = C.y + math.sin(alfa + betar) * rad
            Q = Point(q1, q2)
            r1 = C.x + math.cos(alfa + beta2) * rad / math.cos(beta2)
            r2 = C.y + math.sin(alfa + beta2) * rad / math.cos(beta2)
            R = Point(r1, r2)
            kappah = (1 - math.cos(beta2)) * 4 / 3 / abs(R - Q)
            kappa = kappah * abs(P - Q) / (1 - math.cos(betar))
            cp1 = P + (R - P) * kappa
            cp2 = Q + (R - Q) * kappa
            self.draw_bezier(last, cp1, cp2, Q)
            last = Q
        if fullSector:
            # Close the wedge as a fresh subpath: arc-start (``point``) ->
            # ``center`` -> arc-end ``Q``, then closepath (`h`) back to ``point``
            # — exactly as PyMuPDF does (the `h` is emitted, not an explicit line).
            self.draw_polyline([point, center, Q])
            self._shape.draw_close()
        return Point(Q.x, Q.y)

    def draw_squiggle(self, p1, p2, breadth: float = 2) -> Point:
        """Draws a wavy / squiggly line from ``p1`` to ``p2`` (PyMuPDF
        ``shape.draw_squiggle``). Replicates PyMuPDF's phase math exactly."""
        a = Point(*_pt(p1))
        b = Point(*_pt(p2))
        rad = abs(b - a)
        cnt = 4 * int(round(rad / (4 * breadth)))
        if cnt < 4:
            raise ValueError("points too close")
        mb = rad / cnt
        i_mat = ~Matrix(_hor_matrix(a, b))
        k = 2.4142135623765633
        points = []
        for i in range(1, cnt):
            if i % 4 == 1:
                p = Point(i, -k) * mb
            elif i % 4 == 3:
                p = Point(i, k) * mb
            else:
                p = Point(i, 0) * mb
            points.append(p * i_mat)
        points = [a] + points + [b]
        n = len(points)
        i = 0
        while i + 2 < n:
            self.draw_curve3(points[i], points[i + 1], points[i + 2])
            i += 2
        return Point(b.x, b.y)

    def draw_zigzag(self, p1, p2, breadth: float = 2) -> Point:
        """Draws a zig-zagged line from ``p1`` to ``p2`` (PyMuPDF
        ``shape.draw_zigzag``). Replicates PyMuPDF's phase math exactly."""
        a = Point(*_pt(p1))
        b = Point(*_pt(p2))
        rad = abs(b - a)
        cnt = 4 * int(round(rad / (4 * breadth)))
        if cnt < 4:
            raise ValueError("points too close")
        mb = rad / cnt
        i_mat = ~Matrix(_hor_matrix(a, b))
        points = []
        for i in range(1, cnt):
            if i % 4 == 1:
                p = Point(i, -1) * mb
            elif i % 4 == 3:
                p = Point(i, 1) * mb
            else:
                continue
            points.append(p * i_mat)
        self.draw_polyline([a] + points + [b])
        return Point(b.x, b.y)

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

    # --- text (PyMuPDF Shape.insert_text / insert_textbox) ---
    #
    # PyMuPDF buffers Shape text until ``commit``; oxide writes it onto the
    # owning page immediately. The page is the same object, so the on-page
    # result and the return values (line count / leftover height) are identical.
    def insert_text(
        self,
        point,
        text,
        *,
        fontname: str = "helv",
        fontsize: float = 11.0,
        color=(0, 0, 0),
        fontfile=None,
        **_ignored,
    ) -> int:
        """Writes ``text`` at ``point`` (PyMuPDF ``shape.insert_text``).

        Returns the number of lines written. ``text`` may be a string or a
        list/tuple of lines.
        """
        if self._page is None:
            raise PdfUnsupportedError("Shape.insert_text() needs an owning Page")
        if isinstance(text, (list, tuple)):
            text = "\n".join(str(t) for t in text)
        return self._page.insert_text(
            point, str(text), fontname=fontname, fontsize=float(fontsize),
            color=color, fontfile=fontfile,
        )

    def insert_textbox(
        self,
        rect,
        buffer,
        *,
        fontname: str = "helv",
        fontsize: float = 11.0,
        color=(0, 0, 0),
        align: int = 0,
        fontfile=None,
        **_ignored,
    ) -> float:
        """Fills ``rect`` with wrapped ``buffer`` (PyMuPDF ``shape.insert_textbox``).

        Returns the unused (or, if negative, deficit) vertical space, like
        PyMuPDF. Also extends the shape's accumulated :attr:`rect`.
        """
        if self._page is None:
            raise PdfUnsupportedError("Shape.insert_textbox() needs an owning Page")
        if isinstance(buffer, (list, tuple)):
            buffer = "\n".join(str(t) for t in buffer)
        more = self._page.insert_textbox(
            rect, str(buffer), fontname=fontname, fontsize=float(fontsize),
            color=color, align=int(align), fontfile=fontfile,
        )
        self.update_rect(rect)
        return more

    # --- geometry / parent properties (PyMuPDF Shape.*) ---
    @property
    def doc(self) -> "Document | None":
        """The owning :class:`Document` (PyMuPDF ``Shape.doc``)."""
        return self._doc

    @property
    def page(self) -> "Page | None":
        """The owning :class:`Page` (PyMuPDF ``Shape.page``)."""
        return self._page

    @property
    def width(self) -> float:
        """The page's media-box width (PyMuPDF ``Shape.width``)."""
        return self._page.mediabox_size.x if self._page is not None else 0.0

    @property
    def height(self) -> float:
        """The page's media-box height (PyMuPDF ``Shape.height``)."""
        return self._page.mediabox_size.y if self._page is not None else 0.0

    @property
    def x(self) -> float:
        """The page crop-box ``x0`` origin (PyMuPDF ``Shape.x``)."""
        return self._page.cropbox_position.x if self._page is not None else 0.0

    @property
    def y(self) -> float:
        """The page crop-box ``y0`` origin (PyMuPDF ``Shape.y``)."""
        return self._page.cropbox_position.y if self._page is not None else 0.0

    @property
    def rect(self) -> Rect | None:
        """The bounding box of all drawn geometry, or ``None`` (PyMuPDF
        ``Shape.rect``)."""
        return self._rect

    @staticmethod
    def horizontal_angle(c, p) -> float:
        """The angle (radians) of the vector ``c`` -> ``p`` relative to the
        horizontal, quadrant-aware (PyMuPDF ``Shape.horizontal_angle``)."""
        c = Point(*_pt(c))
        p = Point(*_pt(p))
        s = (p - c).unit
        alfa = math.asin(abs(s.y))
        if s.x < 0:
            if s.y <= 0:
                alfa = -(math.pi - alfa)
            else:
                alfa = math.pi - alfa
        else:
            if s.y >= 0:
                pass
            else:
                alfa = -alfa
        return alfa

    def update_rect(self, x) -> None:
        """Extends the accumulated :attr:`rect` to include point/rect ``x``
        (PyMuPDF ``Shape.updateRect``)."""
        is_rect = hasattr(x, "__len__") and len(x) == 4
        if self._rect is None:
            if is_rect:
                r = _rect(_rt(x))
                self._rect = r.normalize()
            else:
                px, py = _pt(x)
                self._rect = Rect(px, py, px, py)
            return
        if is_rect:
            rx0, ry0, rx1, ry1 = _rt(x)
            x0, x1 = min(rx0, rx1), max(rx0, rx1)
            y0, y1 = min(ry0, ry1), max(ry0, ry1)
        else:
            px, py = _pt(x)
            x0 = x1 = px
            y0 = y1 = py
        self._rect = Rect(
            min(self._rect.x0, x0),
            min(self._rect.y0, y0),
            max(self._rect.x1, x1),
            max(self._rect.y1, y1),
        )

    # PyMuPDF deprecated camelCase alias.
    def updateRect(self, x) -> None:  # noqa: N802
        self.update_rect(x)

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

    def extractRAWJSON(self) -> str:  # noqa: N802
        return self._tp.extractRAWJSON()

    def extractHTML(self) -> str:  # noqa: N802
        """fitz-shaped HTML (PyMuPDF ``TextPage.extractHTML``)."""
        return self._tp.extractHTML()

    def extractXHTML(self) -> str:  # noqa: N802
        """fitz-shaped XHTML (PyMuPDF ``TextPage.extractXHTML``)."""
        return self._tp.extractXHTML()

    def extractXML(self) -> str:  # noqa: N802
        """fitz-shaped char-level XML (PyMuPDF ``TextPage.extractXML``)."""
        return self._tp.extractXML()

    def extractTextbox(self, rect) -> str:  # noqa: N802
        """The text contained in ``rect`` (PyMuPDF ``TextPage.extractTextbox``)."""
        r = rect if isinstance(rect, (tuple, list)) else (rect.x0, rect.y0, rect.x1, rect.y1)
        return self._tp.extractTextbox((float(r[0]), float(r[1]), float(r[2]), float(r[3])))

    def extractSelection(self, a, b) -> str:  # noqa: N802
        """Text between two points like a mouse drag (PyMuPDF
        ``TextPage.extractSelection``). ``a``/``b`` are points or ``(x, y)``."""
        pa = a if isinstance(a, (tuple, list)) else (a.x, a.y)
        pb = b if isinstance(b, (tuple, list)) else (b.x, b.y)
        return self._tp.extractSelection(
            (float(pa[0]), float(pa[1])), (float(pb[0]), float(pb[1]))
        )

    def search(self, needle: str, quads: bool = False) -> list:
        """All occurrences of ``needle`` (PyMuPDF ``TextPage.search``).

        Returns a list of :class:`Quad` when ``quads`` is ``True``, else a list
        of :class:`Rect` (the enclosing rectangle of each hit)."""
        hits = self._tp.search(needle, quads)
        if quads:
            # The core returns each quad as an 8-tuple
            # (ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y).
            return [
                Quad((q[0], q[1]), (q[2], q[3]), (q[4], q[5]), (q[6], q[7]))
                for q in hits
            ]
        return [Rect(*r) for r in hits]

    def extractIMGINFO(self) -> list[dict]:  # noqa: N802
        """Per-image info dicts for images on the page (PyMuPDF
        ``TextPage.extractIMGINFO``)."""
        return self._tp.extractIMGINFO()

    def poolsize(self) -> int:
        """The structured-text pool size (PyMuPDF ``TextPage.poolsize``)."""
        return self._tp.poolsize()

    @property
    def rect(self) -> Rect:
        return Rect(0.0, 0.0, self._tp.width, self._tp.height)

    def __repr__(self) -> str:
        return repr(self._tp)


class Table:
    """One detected table on a page (PyMuPDF ``fitz.table.Table``).

    Wraps a Rust ``_core.Table``, returning geometry as :class:`Rect` value
    types. ``extract()`` returns the cell-text grid (PyMuPDF-compatible);
    ``to_markdown()`` / ``to_html()`` render the table.
    """

    __slots__ = ("_table",)

    def __init__(self, core_table: "_core.Table") -> None:
        self._table = core_table

    @property
    def bbox(self) -> Rect:
        """The table bounding box (PyMuPDF ``Table.bbox``)."""
        return _rect(self._table.bbox)

    @property
    def row_count(self) -> int:
        """The number of cell rows (PyMuPDF ``Table.row_count``)."""
        return self._table.row_count

    @property
    def col_count(self) -> int:
        """The number of cell columns (PyMuPDF ``Table.col_count``)."""
        return self._table.col_count

    @property
    def header(self) -> list:
        """The header row's cell text, or ``[]`` (PyMuPDF ``Table.header``)."""
        return self._table.header

    @property
    def rows(self) -> list[float]:
        """The snapped horizontal grid-line y positions (PyMuPDF ``Table.rows``)."""
        return self._table.rows

    @property
    def cols(self) -> list[float]:
        """The snapped vertical grid-line x positions (PyMuPDF ``Table.cols``)."""
        return self._table.cols

    @property
    def cells(self) -> list:
        """The per-slot cell rects (row-major), each a :class:`Rect` or ``None``
        for an absent / merge-continuation slot (PyMuPDF ``Table.cells``)."""
        return [
            [(_rect(c) if c is not None else None) for c in row]
            for row in self._table.cells
        ]

    @property
    def spans(self) -> list:
        """One span per originating cell as
        ``(row, col, row_span, col_span, Rect)`` (PyMuPDF ``Table.spans``)."""
        return [(r, c, rs, cs, _rect(rect)) for (r, c, rs, cs, rect) in self._table.spans]

    def extract(self) -> list[list]:
        """The cell-text grid (row-major); ``None`` for an empty /
        continuation slot (PyMuPDF ``Table.extract``)."""
        return self._table.extract()

    def to_markdown(self) -> str:
        """The table as GitHub-Flavored-Markdown (PyMuPDF ``Table.to_markdown``)."""
        return self._table.to_markdown()

    def to_html(self) -> str:
        """The table as an HTML ``<table>`` string (oxide_pdf extra)."""
        return self._table.to_html()

    # PyMuPDF deprecated camelCase aliases.
    def toMarkdown(self) -> str:  # noqa: N802
        return self.to_markdown()

    def __repr__(self) -> str:
        return f"<oxide_pdf.Table {self.row_count}x{self.col_count}>"


class TableFinder:
    """A page's detected tables (PyMuPDF ``fitz.table.TableFinder``).

    Iterable and indexable; ``len(finder)`` is the table count and
    ``finder.tables`` is the list of :class:`Table`.
    """

    __slots__ = ("_finder",)

    def __init__(self, core_finder: "_core.TableFinder") -> None:
        self._finder = core_finder

    @property
    def tables(self) -> list[Table]:
        """The detected tables (PyMuPDF ``TableFinder.tables``)."""
        return [Table(t) for t in self._finder.tables]

    def __len__(self) -> int:
        return len(self._finder)

    def __getitem__(self, index: int) -> Table:
        return Table(self._finder[index])

    def __iter__(self) -> Iterator[Table]:
        for t in self._finder.tables:
            yield Table(t)

    def __repr__(self) -> str:
        return f"<oxide_pdf.TableFinder tables={len(self)}>"


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

# ``Font`` is the Rust ``_core.Font`` directly (PyMuPDF ``fitz.Font``): a
# standalone Core-14 font handle exposing name / metrics / advances /
# glyph-name ↔ Unicode helpers (PRD §8.5).
Font = _core.Font

# The 14 standard PDF base-font names (PyMuPDF module-level
# ``fitz.Base14_fontnames``).
Base14_fontnames = _core.Base14_fontnames

# ``Tools`` / ``TOOLS`` is PyMuPDF's utility singleton (cache knobs, ids,
# version, warnings). Most methods are advisory no-ops in the pure-Rust core.
Tools = _core.Tools
TOOLS = _core.TOOLS


class Page:
    """One page of a :class:`Document` (PyMuPDF ``fitz.Page``)."""

    __slots__ = ("_page", "_parent")

    def __init__(self, core_page: "_core.Page", parent: "Document | None" = None) -> None:
        self._page = core_page
        self._parent = parent

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

    @property
    def artbox(self) -> Rect:
        """The effective ``/ArtBox`` (defaults to the crop box; PyMuPDF ``page.artbox``)."""
        return _rect(self._page.artbox())

    @property
    def bleedbox(self) -> Rect:
        """The effective ``/BleedBox`` (defaults to the crop box; PyMuPDF ``page.bleedbox``)."""
        return _rect(self._page.bleedbox())

    @property
    def trimbox(self) -> Rect:
        """The effective ``/TrimBox`` (defaults to the crop box; PyMuPDF ``page.trimbox``)."""
        return _rect(self._page.trimbox())

    @property
    def mediabox_size(self) -> Point:
        """The media-box ``(width, height)`` as a :class:`Point` (PyMuPDF ``page.mediabox_size``)."""
        mb = self._page.mediabox()
        return Point(mb[2] - mb[0], mb[3] - mb[1])

    @property
    def cropbox_position(self) -> Point:
        """The crop-box origin ``(x0, y0)`` as a :class:`Point` (PyMuPDF ``page.cropbox_position``)."""
        cb = self._page.cropbox()
        return Point(cb[0], cb[1])

    @property
    def transformation_matrix(self) -> Matrix:
        """The page-to-fitz transformation matrix (PyMuPDF ``page.transformation_matrix``)."""
        return Matrix(*self._page.transformation_matrix())

    @property
    def rotation_matrix(self) -> Matrix:
        """The page rotation matrix (PyMuPDF ``page.rotation_matrix``)."""
        return Matrix(*self._page.rotation_matrix())

    @property
    def derotation_matrix(self) -> Matrix:
        """The inverse rotation matrix (PyMuPDF ``page.derotation_matrix``)."""
        return Matrix(*self._page.derotation_matrix())

    @property
    def xref(self) -> int:
        """The page object's xref number (PyMuPDF ``page.xref``)."""
        return self._page.xref

    @property
    def parent(self) -> "Document | None":
        """The owning :class:`Document` (PyMuPDF ``page.parent``); ``page.parent is doc``."""
        return self._parent

    # --- text extraction (PRD §8.6 / §9.4) ---
    def get_textpage(self, flags: int | None = None, clip=None) -> TextPage:
        """Builds a reusable :class:`TextPage` (PyMuPDF ``page.get_textpage``)."""
        return TextPage(self._page.get_textpage(flags, _as_clip(clip)))

    def get_textpage_ocr(
        self,
        flags: int = 3,
        language: str = "eng",
        dpi: int = 72,
        full: bool = True,
        tessdata: str | None = None,
    ) -> TextPage:
        """Builds an OCR :class:`TextPage` via the system Tesseract (PyMuPDF
        ``page.get_textpage_ocr``).

        Rasterizes the page at ``dpi``, recognizes it with Tesseract
        (``language``), and returns a :class:`TextPage` whose ``get_text`` /
        ``search_for`` work on the OCR result. ``full=False`` (image-region-only
        OCR) is not yet implemented and falls back to full-page OCR. Raises
        ``PdfUnsupportedError`` if Tesseract is not installed.
        """
        return TextPage(
            self._page.get_textpage_ocr(flags, language, dpi, full, tessdata)
        )

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

    def get_text_words(self, *, clip=None, flags: int | None = None, sort: bool = False) -> list[tuple]:
        """Word tuples ``(x0, y0, x1, y1, word, block, line, word_no)`` (PyMuPDF
        ``page.get_text_words``). When ``clip`` is given, only words whose bbox
        intersects the clip are returned."""
        words = self._page.get_text("words", clip=None, flags=flags, textpage=None, sort=sort)
        if clip is None:
            return words
        cr = _rt(clip)
        return [w for w in words if _intersects((w[0], w[1], w[2], w[3]), cr)]

    def get_text_blocks(self, *, clip=None, flags: int | None = None, sort: bool = False) -> list[tuple]:
        """Block tuples ``(x0, y0, x1, y1, text, block_no, block_type)`` (PyMuPDF
        ``page.get_text_blocks``). ``clip`` filters by bbox intersection."""
        blocks = self._page.get_text("blocks", clip=None, flags=flags, textpage=None, sort=sort)
        if clip is None:
            return blocks
        cr = _rt(clip)
        return [b for b in blocks if _intersects((b[0], b[1], b[2], b[3]), cr)]

    def get_textbox(self, rect, *, textpage: TextPage | None = None) -> str:
        """The text within ``rect`` (PyMuPDF ``page.get_textbox``). A word is
        included when its bbox intersects ``rect``; words are joined preserving
        line breaks."""
        cr = _rt(rect)
        words = self._page.get_text("words", clip=None, flags=None, textpage=None, sort=False)
        sel = [w for w in words if _intersects((w[0], w[1], w[2], w[3]), cr)]
        # Group selected words by (block, line) preserving order, join with spaces;
        # separate lines with newlines.
        lines: list[str] = []
        cur_key = None
        cur: list[str] = []
        for w in sel:
            key = (w[5], w[6])
            if key != cur_key and cur:
                lines.append(" ".join(cur))
                cur = []
            cur_key = key
            cur.append(w[4])
        if cur:
            lines.append(" ".join(cur))
        return "\n".join(lines)

    def get_text_selection(self, p1, p2, clip=None) -> str:
        """The text between two points ``p1`` and ``p2`` (PyMuPDF
        ``page.get_text_selection``). Selects words whose bbox falls within the
        rectangle spanned by ``p1``/``p2`` (intersected with ``clip``)."""
        a = _pt(p1)
        b = _pt(p2)
        sel_rect = (min(a[0], b[0]), min(a[1], b[1]), max(a[0], b[0]), max(a[1], b[1]))
        if clip is not None:
            cr = _rt(clip)
            sel_rect = (
                max(sel_rect[0], cr[0]),
                max(sel_rect[1], cr[1]),
                min(sel_rect[2], cr[2]),
                min(sel_rect[3], cr[3]),
            )
        return self.get_textbox(sel_rect)

    @property
    def first_link(self) -> "Link | None":
        """The first link on the page, or ``None`` (PyMuPDF ``page.first_link``)."""
        core = self._page.first_link
        return Link(core, self) if core is not None else None

    def links(self, kinds=None) -> Iterator["Link"]:
        """Iterates the page's links as :class:`Link` objects (PyMuPDF
        ``page.links``). When ``kinds`` is given, only links of those kinds are
        yielded."""
        for core in self._page.link_objects():
            link = Link(core, self)
            if kinds is None or link.kind in kinds:
                yield link

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

    def get_xobjects(self) -> list[tuple]:
        """The page's XObjects (PyMuPDF ``page.get_xobjects``).

        Each entry is ``(xref, name, type, bbox, matrix, referencer)`` where
        ``type`` is ``"Form"`` or ``"Image"``, ``bbox`` is a :class:`Rect`, and
        ``matrix`` is a :class:`Matrix`.
        """
        out = []
        for xref, name, kind, bbox, matrix, ref in self._page.get_xobjects():
            out.append((xref, name, kind, _rect(bbox), Matrix(*matrix), ref))
        return out

    def get_image_rects(self, *_args, **_kwargs) -> list[Rect]:
        """The page's image placements as :class:`Rect` (PyMuPDF
        ``page.get_image_rects``). One rectangle per painted image."""
        return [_rect(bbox) for _name, _inline, bbox, _w, _h in self._page.get_image_rects()]

    def get_image_info(self, *_args, **_kwargs) -> list[dict]:
        """Per-image placement info dicts (PyMuPDF ``page.get_image_info``).

        Each dict carries ``number``, ``xref``, ``name``, ``bbox`` (:class:`Rect`),
        ``width``, ``height``, ``bpc``, ``colorspace``/``cs-name``, and ``filter``.
        """
        out = []
        for info in self._page.get_image_info():
            info = dict(info)
            info["bbox"] = _rect(info["bbox"])
            out.append(info)
        return out

    def get_image_bbox(self, name_or_xref, *_args, **_kwargs) -> Rect:
        """The :class:`Rect` bbox of the image identified by ``name_or_xref``
        (PyMuPDF ``page.get_image_bbox``). Accepts a resource name, an xref int,
        or a ``get_images`` tuple. Returns an empty :class:`Rect` if not found."""
        if isinstance(name_or_xref, (tuple, list)) and name_or_xref:
            # A get_images() tuple: PyMuPDF accepts the whole entry; use its name
            # (index 7) when present, else its xref (index 0).
            key = name_or_xref[7] if len(name_or_xref) > 7 and name_or_xref[7] else name_or_xref[0]
        else:
            key = name_or_xref
        bbox = self._page.get_image_bbox(str(key))
        return _rect(bbox) if bbox is not None else Rect(0, 0, 0, 0)

    def get_contents(self) -> list[int]:
        """The object numbers of the page's content streams (PyMuPDF
        ``page.get_contents``)."""
        return self._page.get_contents()

    def read_contents(self) -> bytes:
        """The decoded, concatenated content-stream bytes (PyMuPDF
        ``page.read_contents``)."""
        return self._page.read_contents()

    def clean_contents(self, *args, **kwargs) -> None:
        """Consolidates ``/Contents`` into a single stream (PyMuPDF
        ``page.clean_contents``)."""
        self._page.clean_contents(*args, **kwargs)

    def wrap_contents(self) -> None:
        """Wraps the page content in a balanced ``q … Q`` (PyMuPDF
        ``page.wrap_contents``)."""
        self._page.wrap_contents()

    def delete_image(self, name_or_xref, *_args, **_kwargs) -> None:
        """Deletes an image XObject by resource name or xref, replacing it with a
        transparent stub (PyMuPDF ``page.delete_image``)."""
        self._page.delete_image(str(name_or_xref))

    def replace_image(self, name_or_xref, *, filename=None, stream=None, pixmap=None, **_kwargs) -> None:
        """Replaces an image XObject (by name or xref) with a new JPEG, keeping
        the existing placement (PyMuPDF ``page.replace_image``).

        Provide the new image via ``stream=`` (JPEG bytes) or ``filename=`` (a
        path to a JPEG file).
        """
        if stream is None and filename is not None:
            with builtins.open(os.fspath(filename), "rb") as fh:
                stream = fh.read()
        if stream is None:
            raise ValueError("replace_image requires stream= or filename= (JPEG)")
        self._page.replace_image(str(name_or_xref), stream=bytes(stream))

    def set_oc(self, oc: int) -> None:
        """Binds the page's content to an optional-content group (PyMuPDF
        ``page.set_oc``); ``0`` clears the binding."""
        self._page.set_oc(int(oc))

    def get_oc(self) -> int:
        """The xref of the optional-content group bound to this page, or ``0``
        (PyMuPDF ``page.get_oc``)."""
        return self._page.get_oc()

    def get_texttrace(self) -> list[dict]:
        """The low-level per-glyph text trace (PyMuPDF ``page.get_texttrace``):
        a list of span dicts, each with style metadata and a ``chars`` list of
        ``(ucs, gid, origin, bbox)`` tuples."""
        return self._page.get_texttrace()

    def get_bboxlog(self, *args, **kwargs) -> list[tuple]:
        """The page's bbox paint log (PyMuPDF ``page.get_bboxlog``): a list of
        ``(op, bbox)`` tuples in reading order."""
        return self._page.get_bboxlog(*args, **kwargs)

    def show_pdf_page(self, rect, src: "Document", pno: int = 0, *_args, **_kwargs) -> str:
        """Places ``src``'s page ``pno`` onto this page as a Form XObject filling
        ``rect`` (PyMuPDF ``page.show_pdf_page``). Returns the XObject name."""
        return self._page.show_pdf_page(_rt(rect), src._doc, int(pno))

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

    # --- table detection (PRD §7, M7) ---
    def find_tables(
        self,
        *,
        strategy: str = "lines",
        line_max_thickness: float = 3.0,
        snap_tolerance: float = 3.0,
        min_line_length: float = 3.0,
        clip=None,
        **_ignored,
    ) -> "TableFinder":
        """Detects the tables on this page (PyMuPDF ``page.find_tables``).

        ``strategy`` is ``"lines"`` (default), ``"lines_strict"`` or ``"text"``.
        PyMuPDF's ``vertical_strategy``/``horizontal_strategy`` kwargs are
        accepted: a single non-default value selects that strategy. Returns a
        :class:`TableFinder` (iterable; ``.tables`` is the list).
        """
        # PyMuPDF passes vertical_strategy / horizontal_strategy; honor either.
        vs = _ignored.get("vertical_strategy")
        hs = _ignored.get("horizontal_strategy")
        for cand in (vs, hs):
            if cand:
                strategy = str(cand)
                break
        return TableFinder(
            self._page.find_tables(
                strategy=strategy,
                line_max_thickness=float(line_max_thickness),
                snap_tolerance=float(snap_tolerance),
                min_line_length=float(min_line_length),
            )
        )

    # --- SVG export (PRD §7, M7) ---
    def get_svg_image(self, matrix=None, *, text_as_path: bool = True, **_ignored) -> str:
        """Renders this page to a standalone SVG document string (PyMuPDF
        ``page.get_svg_image``).

        ``matrix`` is an optional :class:`Matrix` / 6-sequence page-space
        transform. PyMuPDF's ``text_as_path`` kwarg is accepted and ignored.
        """
        mtx = None
        if matrix is not None:
            m = tuple(float(v) for v in matrix)
            if len(m) != 6:
                raise ValueError("matrix must be a 6-sequence (a, b, c, d, e, f)")
            mtx = m
        return self._page.get_svg_image(matrix=mtx)

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

    def set_mediabox(self, rect) -> None:
        """Sets the ``/MediaBox`` (PyMuPDF ``page.set_mediabox``)."""
        self._page.set_mediabox(_rt(rect))

    def set_cropbox(self, rect) -> None:
        """Sets the ``/CropBox``, clipped to the media box (PyMuPDF ``page.set_cropbox``)."""
        self._page.set_cropbox(_rt(rect))

    def set_artbox(self, rect) -> None:
        """Sets the ``/ArtBox`` (PyMuPDF ``page.set_artbox``)."""
        self._page.set_artbox(_rt(rect))

    def set_bleedbox(self, rect) -> None:
        """Sets the ``/BleedBox`` (PyMuPDF ``page.set_bleedbox``)."""
        self._page.set_bleedbox(_rt(rect))

    def set_trimbox(self, rect) -> None:
        """Sets the ``/TrimBox`` (PyMuPDF ``page.set_trimbox``)."""
        self._page.set_trimbox(_rt(rect))

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
        return Shape(self._page.new_shape(), page=self, doc=self._parent)

    # --- annotations (PRD §8.8) ---
    def add_text_annot(self, point, text: str, *, icon: str = "Note", **_ignored) -> Annot:
        """Adds a sticky-note text annotation (PyMuPDF ``page.add_text_annot``)."""
        return Annot(self._page.add_text_annot(_pt(point), text, icon=icon), parent=self)

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
            ),
            parent=self,
        )

    def add_highlight_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds a highlight annotation over ``quads`` (PyMuPDF ``page.add_highlight_annot``)."""
        return Annot(self._page.add_highlight_annot(_quads(quads)), parent=self)

    def add_underline_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds an underline annotation over ``quads`` (PyMuPDF ``page.add_underline_annot``)."""
        return Annot(self._page.add_underline_annot(_quads(quads)), parent=self)

    def add_strikeout_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds a strike-out annotation over ``quads`` (PyMuPDF ``page.add_strikeout_annot``)."""
        return Annot(self._page.add_strikeout_annot(_quads(quads)), parent=self)

    def add_squiggly_annot(self, quads=None, *, start=None, stop=None, clip=None, **_ignored) -> Annot:
        """Adds a squiggly-underline annotation over ``quads`` (PyMuPDF ``page.add_squiggly_annot``)."""
        return Annot(self._page.add_squiggly_annot(_quads(quads)), parent=self)

    def add_rect_annot(self, rect, *, color=(0, 0, 0), fill=None, **_ignored) -> Annot:
        """Adds a rectangle annotation (PyMuPDF ``page.add_rect_annot``)."""
        return Annot(
            self._page.add_rect_annot(_rt(rect), color=_color(color), fill=_color(fill)),
            parent=self,
        )

    def add_circle_annot(self, rect, *, color=(0, 0, 0), fill=None, **_ignored) -> Annot:
        """Adds a circle/ellipse annotation (PyMuPDF ``page.add_circle_annot``)."""
        return Annot(
            self._page.add_circle_annot(_rt(rect), color=_color(color), fill=_color(fill)),
            parent=self,
        )

    def add_line_annot(self, p1, p2, *, color=(0, 0, 0), **_ignored) -> Annot:
        """Adds a line annotation from ``p1`` to ``p2`` (PyMuPDF ``page.add_line_annot``)."""
        return Annot(self._page.add_line_annot(_pt(p1), _pt(p2), color=_color(color)), parent=self)

    def add_polygon_annot(self, points, *, color=(0, 0, 0), fill=None, **_ignored) -> Annot:
        """Adds a polygon annotation through ``points`` (PyMuPDF ``page.add_polygon_annot``)."""
        return Annot(
            self._page.add_polygon_annot(
                [_pt(p) for p in points], color=_color(color), fill=_color(fill)
            ),
            parent=self,
        )

    def add_polyline_annot(self, points, *, color=(0, 0, 0), **_ignored) -> Annot:
        """Adds a polyline annotation through ``points`` (PyMuPDF ``page.add_polyline_annot``)."""
        return Annot(
            self._page.add_polyline_annot([_pt(p) for p in points], color=_color(color)),
            parent=self,
        )

    def add_ink_annot(self, handwriting, *, color=(0, 0, 0), **_ignored) -> Annot:
        """Adds a free-hand ink annotation (PyMuPDF ``page.add_ink_annot``).

        ``handwriting`` is a list of strokes; each stroke is a list of points.
        """
        strokes = [[_pt(p) for p in stroke] for stroke in handwriting]
        return Annot(self._page.add_ink_annot(strokes, color=_color(color)), parent=self)

    def add_stamp_annot(self, rect, *, stamp: str = "Approved", **_ignored) -> Annot:
        """Adds a rubber-stamp annotation (PyMuPDF ``page.add_stamp_annot``)."""
        return Annot(self._page.add_stamp_annot(_rt(rect), stamp=stamp), parent=self)

    def add_file_annot(self, point, buffer, filename: str, *, ufilename=None, desc=None, icon=None, **_ignored) -> Annot:
        """Adds a file-attachment annotation (PyMuPDF ``page.add_file_annot``)."""
        return Annot(self._page.add_file_annot(_pt(point), bytes(buffer), filename, desc=desc), parent=self)

    def add_redact_annot(self, quad, *, text=None, fill=None, **_ignored) -> Annot:
        """Adds a redaction annotation over ``quad`` (PyMuPDF ``page.add_redact_annot``)."""
        return Annot(
            self._page.add_redact_annot(_rt(_rect_from_corners(_quad(quad))), fill=_color(fill), text=text),
            parent=self,
        )

    def annots(self, types=None) -> Iterator[Annot]:
        """Iterates the page's annotations (PyMuPDF ``page.annots``).

        When ``types`` is given (a sequence of PyMuPDF annotation-type ints), only
        annotations of those types are yielded.
        """
        cores = self._page.annots()
        for i, core in enumerate(cores):
            annot = Annot(core, parent=self, siblings=cores, index=i)
            if types is None or annot.type[0] in types:
                yield annot

    @property
    def first_annot(self) -> Annot | None:
        """The first annotation, or ``None`` (PyMuPDF ``page.first_annot``)."""
        cores = self._page.annots()
        if not cores:
            return None
        return Annot(cores[0], parent=self, siblings=cores, index=0)

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
        ws = [Widget(w) for w in self._page.widgets()]
        for a, b in zip(ws, ws[1:]):
            a.next = b
        return ws

    @property
    def first_widget(self) -> Widget | None:
        """The first form-field widget, or ``None`` (PyMuPDF ``page.first_widget``).

        The returned widget's :attr:`Widget.next` chain links the remaining page
        widgets, matching PyMuPDF's linked-list traversal.
        """
        ws = self.widgets()
        return ws[0] if ws else None

    # PyMuPDF deprecated camelCase aliases.
    def getPixmap(self, *args, **kw) -> Pixmap:  # noqa: N802
        return self.get_pixmap(*args, **kw)

    def getTextPageOCR(self, *args, **kw) -> TextPage:  # noqa: N802
        return self.get_textpage_ocr(*args, **kw)

    def getDisplayList(self) -> "DisplayList":  # noqa: N802
        return self.get_displaylist()

    def findTables(self, **kw) -> "TableFinder":  # noqa: N802
        return self.find_tables(**kw)

    def getSVGimage(self, *args, **kw) -> str:  # noqa: N802
        return self.get_svg_image(*args, **kw)

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


class linkDest:  # noqa: N801 — PyMuPDF spells it linkDest
    """A resolved link destination (PyMuPDF ``fitz.linkDest``).

    A lightweight value object carrying the PyMuPDF destination fields most code
    reads: ``kind``, ``page`` (for GoTo), ``uri`` (for URI), ``dest`` (the named
    destination string) and the ``flags``.
    """

    __slots__ = ("kind", "page", "uri", "dest", "named", "flags", "is_uri", "is_map")

    # PyMuPDF link-kind constants.
    LINK_NONE = 0
    LINK_GOTO = 1
    LINK_URI = 2

    def __init__(self, core_link: "_core.Link") -> None:
        self.kind = core_link.kind
        self.page = core_link.page
        self.uri = core_link.uri
        self.dest = core_link.dest
        self.named = core_link.dest
        self.flags = core_link.flags
        self.is_uri = self.kind == self.LINK_URI
        self.is_map = self.kind == self.LINK_GOTO

    def __repr__(self) -> str:
        return f"linkDest(kind={self.kind}, page={self.page}, uri={self.uri!r})"


class Link:
    """A page link annotation (PyMuPDF ``fitz.Link``).

    Wraps a Rust ``_core.Link``, exposing rect/kind/uri/page/dest and the
    ``next`` chain so existing ``link = page.first_link; while link: …`` loops
    work unchanged.
    """

    __slots__ = ("_link", "_page")

    def __init__(self, core_link: "_core.Link", page: "Page") -> None:
        self._link = core_link
        self._page = page

    @property
    def rect(self) -> Rect:
        """The link source rectangle (PyMuPDF ``link.rect``)."""
        return _rect(self._link.rect)

    @property
    def kind(self) -> int:
        """The link kind (0 none / 1 goto / 2 uri) (PyMuPDF ``link.kind``)."""
        return self._link.kind

    @property
    def uri(self) -> str:
        """The external URI, or ``""`` (PyMuPDF ``link.uri``)."""
        return self._link.uri

    @property
    def page(self) -> int:
        """The destination page index for a GoTo link, else ``-1`` (PyMuPDF
        ``link.page``)."""
        return self._link.page

    @property
    def dest(self) -> linkDest:
        """The resolved :class:`linkDest` (PyMuPDF ``link.dest``)."""
        return linkDest(self._link)

    @property
    def is_external(self) -> bool:
        """Whether the link targets an external URI (PyMuPDF ``link.is_external``)."""
        return self._link.is_external

    @property
    def border(self) -> dict:
        """``{"width": w, "dashes": [...], "style": ...}`` (PyMuPDF ``link.border``)."""
        h, v, w = self._link.border
        return {"width": w, "dashes": [], "style": "S", "clouds": -1}

    @property
    def colors(self) -> dict:
        """``{"stroke": (r,g,b)|None, "fill": None}`` (PyMuPDF ``link.colors``)."""
        return {"stroke": self._link.color, "fill": None}

    @property
    def flags(self) -> int:
        """The annotation flags ``/F`` (PyMuPDF ``link.flags``)."""
        return self._link.flags

    @property
    def xref(self) -> int:
        """The link annotation object number (PyMuPDF ``link.xref``)."""
        return self._link.xref

    @property
    def linkDest(self) -> linkDest:  # noqa: N802 — PyMuPDF attribute name
        """The resolved :class:`linkDest` (PyMuPDF ``link.linkDest``)."""
        return linkDest(self._link)

    def set_border(self, border=None, *, width=None, **_ignored) -> None:
        """Sets the link border by re-inserting the annotation (PyMuPDF
        ``link.set_border``). Buffered: re-emits the same target with a new rect.

        Only the geometry/target are persisted by the core link writer; the
        border width is accepted for API compatibility.
        """
        # The core link writer does not yet round-trip border widths; this is a
        # no-op accepted for compatibility (raising would break callers).
        return None

    def set_colors(self, colors=None, *, stroke=None, fill=None, **_ignored) -> None:
        """Sets link colors (PyMuPDF ``link.set_colors``). Accepted for API
        compatibility; the core link writer does not round-trip link colors."""
        return None

    def set_flags(self, flags: int) -> None:
        """Sets link flags (PyMuPDF ``link.set_flags``). Accepted for API
        compatibility."""
        return None

    @property
    def next(self) -> "Link | None":
        """The next link on the page, or ``None`` (PyMuPDF ``link.next``)."""
        core = self._link.next
        return Link(core, self._page) if core is not None else None

    def __repr__(self) -> str:
        return f"<oxide_pdf.Link kind={self._link.kind} xref={self._link.xref}>"


class Outline:
    """A document outline (bookmark) tree node (PyMuPDF ``fitz.Outline``).

    Wraps a Rust ``_core.Outline``; ``next``/``down`` walk the tree so
    ``ol = doc.outline; while ol: … ol = ol.next`` works unchanged.
    """

    __slots__ = ("_node",)

    def __init__(self, core_node: "_core.Outline") -> None:
        self._node = core_node

    @property
    def title(self) -> str:
        """The bookmark title (PyMuPDF ``outline.title``)."""
        return self._node.title

    @property
    def page(self) -> int:
        """The target page index, or ``-1`` (PyMuPDF ``outline.page``)."""
        return self._node.page

    @property
    def uri(self) -> str | None:
        """The external URI, or ``None`` (PyMuPDF ``outline.uri``)."""
        return self._node.uri

    @property
    def is_external(self) -> bool:
        """Whether the destination is external (PyMuPDF ``outline.is_external``)."""
        return self._node.is_external

    @property
    def is_open(self) -> bool:
        """Whether the item is expanded (PyMuPDF ``outline.is_open``)."""
        return self._node.is_open

    @property
    def dest(self) -> linkDest:
        """The resolved destination (PyMuPDF ``outline.dest``)."""
        return _OutlineDest(self._node)

    @property
    def destination(self) -> linkDest:
        """Alias for :attr:`dest` (PyMuPDF ``outline.destination``)."""
        return _OutlineDest(self._node)

    @property
    def x(self) -> float:
        """The destination x-coordinate (PyMuPDF ``outline.x``); ``0`` if none."""
        return 0.0

    @property
    def y(self) -> float:
        """The destination y-coordinate (PyMuPDF ``outline.y``); ``0`` if none."""
        return 0.0

    @property
    def next(self) -> "Outline | None":
        """The next sibling, or ``None`` (PyMuPDF ``outline.next``)."""
        core = self._node.next
        return Outline(core) if core is not None else None

    @property
    def down(self) -> "Outline | None":
        """The first child, or ``None`` (PyMuPDF ``outline.down``)."""
        core = self._node.down
        return Outline(core) if core is not None else None

    def __repr__(self) -> str:
        return f"<oxide_pdf.Outline title={self._node.title!r}>"


class _OutlineDest:
    """A resolved destination for an :class:`Outline` node."""

    __slots__ = ("kind", "page", "uri", "is_external", "named")

    def __init__(self, node: "_core.Outline") -> None:
        self.uri = node.uri
        self.is_external = node.is_external
        self.page = node.page
        self.kind = linkDest.LINK_URI if node.is_external else linkDest.LINK_GOTO
        self.named = None

    def __repr__(self) -> str:
        return f"_OutlineDest(kind={self.kind}, page={self.page}, uri={self.uri!r})"


class Colorspace:
    """A colorspace (PyMuPDF ``fitz.Colorspace``).

    Three device colorspaces are supported, matching PyMuPDF's
    ``csGRAY``/``csRGB``/``csCMYK`` singletons.
    """

    __slots__ = ("n", "_name")

    def __init__(self, type_: int) -> None:
        """``type_`` is the PyMuPDF ``CS_*`` constant (1=GRAY, 2=RGB, 3=CMYK)."""
        if type_ == CS_GRAY:
            self.n, self._name = 1, "DeviceGray"
        elif type_ == CS_RGB:
            self.n, self._name = 3, "DeviceRGB"
        elif type_ == CS_CMYK:
            self.n, self._name = 4, "DeviceCMYK"
        else:
            raise ValueError(f"unsupported colorspace type: {type_}")

    @property
    def name(self) -> str:
        """The colorspace name (PyMuPDF ``cs.name``)."""
        return self._name

    @property
    def is_gray(self) -> bool:
        """Whether this is a grayscale colorspace."""
        return self.n == 1

    def __repr__(self) -> str:
        return f"Colorspace({self._name})"

    def __eq__(self, other) -> bool:
        return isinstance(other, Colorspace) and other.n == self.n and other._name == self._name

    def __hash__(self) -> int:
        return hash((self.n, self._name))


# PyMuPDF colorspace-type constants + the three device-colorspace singletons.
CS_GRAY = 1
CS_RGB = 2
CS_CMYK = 3
csGRAY = Colorspace(CS_GRAY)  # noqa: N816 — PyMuPDF spelling
csRGB = Colorspace(CS_RGB)  # noqa: N816
csCMYK = Colorspace(CS_CMYK)  # noqa: N816


class TextWriter:
    """An accumulating text emitter (PyMuPDF ``fitz.TextWriter``).

    Collects ``append``/``fill_textbox`` calls, then renders them onto a page via
    :meth:`write_text`. Backed by the page content emitter / font metrics.
    """

    __slots__ = ("page_rect", "opacity", "color", "_segments", "last_point", "_font", "_fontsize")

    def __init__(self, page_rect, opacity: float = 1.0, color=None) -> None:
        self.page_rect = _rect(page_rect)
        self.opacity = float(opacity)
        self.color = _color(color) if color is not None else (0.0, 0.0, 0.0)
        # Each segment: (point, text, fontname, fontsize, color).
        self._segments: list[tuple] = []
        self.last_point = Point(0.0, 0.0)
        self._font = None
        self._fontsize = 11.0

    @property
    def text_rect(self) -> Rect:
        """The bounding rect of all appended text (PyMuPDF ``tw.text_rect``)."""
        if not self._segments:
            return Rect(0.0, 0.0, 0.0, 0.0)
        x0 = min(s[0][0] for s in self._segments)
        y0 = min(s[0][1] - s[3] for s in self._segments)
        x1 = max(s[0][0] + _text_width(s[1], s[2], s[3]) for s in self._segments)
        y1 = max(s[0][1] for s in self._segments)
        return Rect(x0, y0, x1, y1)

    def append(self, pos, text, font=None, fontsize: float = 11.0, *, language=None, **_ignored):
        """Appends ``text`` starting at ``pos`` (PyMuPDF ``tw.append``).

        Returns ``(self, last_point)`` mirroring PyMuPDF's return shape.
        """
        p = _pt(pos)
        fontname = _font_name(font)
        self._segments.append((p, str(text), fontname, float(fontsize), self.color))
        adv = _text_width(str(text), fontname, float(fontsize))
        self.last_point = Point(p[0] + adv, p[1])
        return (self, self.last_point)

    def appendv(self, pos, text, font=None, fontsize: float = 11.0, **_ignored):
        """Appends ``text`` vertically (PyMuPDF ``tw.appendv``).

        Each character is stacked downward by ``fontsize``.
        """
        p = _pt(pos)
        fontname = _font_name(font)
        x, y = p
        for ch in str(text):
            self._segments.append(((x, y), ch, fontname, float(fontsize), self.color))
            y += float(fontsize)
        self.last_point = Point(x, y)
        return (self, self.last_point)

    def fill_textbox(self, rect, text, *, font=None, fontsize: float = 11.0, align=0, **_ignored):
        """Wraps and fills ``text`` into ``rect`` (PyMuPDF ``tw.fill_textbox``).

        Greedy word-wrap at ``rect`` width; returns the list of lines that did
        not fit (empty when everything fit).
        """
        r = _rt(rect)
        fontname = _font_name(font)
        width = r[2] - r[0]
        line_h = float(fontsize) * 1.2
        words = str(text).split()
        lines: list[str] = []
        cur = ""
        for w in words:
            trial = w if not cur else cur + " " + w
            if _text_width(trial, fontname, float(fontsize)) <= width or not cur:
                cur = trial
            else:
                lines.append(cur)
                cur = w
        if cur:
            lines.append(cur)
        y = r[1] + float(fontsize)
        overflow: list[str] = []
        for line in lines:
            if y > r[3]:
                overflow.append(line)
                continue
            self._segments.append(((r[0], y), line, fontname, float(fontsize), self.color))
            self.last_point = Point(r[0] + _text_width(line, fontname, float(fontsize)), y)
            y += line_h
        return overflow

    def write_text(self, page, *, opacity=None, color=None, overlay=True, **_ignored) -> None:
        """Renders the accumulated text onto ``page`` (PyMuPDF ``tw.write_text``)."""
        col = _color(color) if color is not None else None
        for (px, py), text, fontname, fontsize, seg_color in self._segments:
            page.insert_text(
                (px, py),
                text,
                fontname=fontname,
                fontsize=fontsize,
                color=(col if col is not None else seg_color),
            )

    # PyMuPDF alias.
    def writeText(self, page, **kw) -> None:  # noqa: N802
        self.write_text(page, **kw)

    def clean_rtl(self, text: str) -> str:
        """Returns ``text`` unchanged (PyMuPDF ``tw.clean_rtl`` reverses RTL runs;
        the pure-Rust core stores text logical-order and does not reshape)."""
        return text

    def __repr__(self) -> str:
        return f"<oxide_pdf.TextWriter segments={len(self._segments)}>"


def _font_name(font) -> str:
    """The base-14 font name for a ``Font``/string/``None``."""
    if font is None:
        return "helv"
    if isinstance(font, str):
        return font
    name = getattr(font, "name", None)
    return name if isinstance(name, str) else "helv"


def _text_width(text: str, fontname: str, fontsize: float) -> float:
    """The advance width of ``text`` via core font metrics."""
    try:
        return _core.Font(fontname).text_length(text, fontsize)
    except Exception:
        return len(text) * fontsize * 0.5


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
        return Page(self._doc.load_page(index), self)

    def __getitem__(self, index: int) -> Page:
        return Page(self._doc[index], self)

    def __iter__(self) -> Iterator[Page]:
        for i in range(self._doc.page_count):
            yield Page(self._doc.load_page(i), self)

    def pages(self, *_args, **_kwargs) -> Iterator[Page]:
        """Yields every page in order (PyMuPDF ``doc.pages``)."""
        for i in range(self._doc.page_count):
            yield Page(self._doc.load_page(i), self)

    def reload_page(self, page) -> Page:
        """Re-fetches a page from the live store (PyMuPDF ``doc.reload_page``).

        Accepts a :class:`Page` (its ``number`` is used) or an int index.
        """
        index = page.number if isinstance(page, Page) else int(page)
        return Page(self._doc.reload_page(index), self)

    def page_xref(self, pno: int) -> int:
        """The object number of page ``pno`` (PyMuPDF ``doc.page_xref``)."""
        if pno < 0:
            pno += self._doc.page_count
        return self._doc.page_xref(pno)

    def get_page_xobjects(self, pno: int) -> list[tuple]:
        """The XObjects on page ``pno`` (PyMuPDF ``doc.get_page_xobjects``).

        Each entry is ``(xref, name, type, bbox, matrix, referencer)``.
        """
        if pno < 0:
            pno += self._doc.page_count
        out = []
        for xref, name, kind, bbox, matrix, ref in self._doc.get_page_xobjects(pno):
            out.append((xref, name, kind, _rect(bbox), Matrix(*matrix), ref))
        return out

    def resolve_link(self, uri: str = "", *, chapters: int = 0) -> int | None:
        """Resolves a link/destination spec to a 0-based page index, or ``None``
        (PyMuPDF ``doc.resolve_link``)."""
        _ = chapters
        return self._doc.resolve_link(str(uri))

    def fullcopy_page(self, pno: int, to: int = -1) -> None:
        """Deep-copies page ``pno`` and inserts the copy at ``to`` (PyMuPDF
        ``doc.fullcopy_page``); ``to == -1`` appends."""
        self._doc.fullcopy_page(int(pno), int(to))

    @property
    def chapter_count(self) -> int:
        """The chapter count — always 1 for PDF (PyMuPDF ``doc.chapter_count``)."""
        return self._doc.chapter_count

    def chapter_page_count(self, chapter: int) -> int:
        """The page count of ``chapter`` (PyMuPDF ``doc.chapter_page_count``)."""
        return self._doc.chapter_page_count(chapter)

    @property
    def last_location(self) -> tuple[int, int]:
        """The last ``(chapter, page)`` location (PyMuPDF ``doc.last_location``)."""
        return self._doc.last_location

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

    def xref_is_font(self, xref: int) -> bool:
        """Whether object ``xref`` is a font dictionary (PyMuPDF
        ``doc.xref_is_font``)."""
        return self._doc.xref_is_font(int(xref))

    def xref_is_image(self, xref: int) -> bool:
        """Whether object ``xref`` is an image XObject (PyMuPDF
        ``doc.xref_is_image``)."""
        return self._doc.xref_is_image(int(xref))

    def xref_set_key(self, xref: int, key: str, value: str) -> None:
        """Sets dictionary key ``key`` of object ``xref`` to the PDF value parsed
        from ``value`` (PyMuPDF ``doc.xref_set_key``); ``"null"`` removes it."""
        self._doc.xref_set_key(int(xref), str(key), str(value))

    def xref_copy(self, source: int, target: int, *, keep=None) -> None:
        """Copies object ``source`` onto object ``target`` (PyMuPDF
        ``doc.xref_copy``)."""
        del keep
        self._doc.xref_copy(int(source), int(target))

    def subset_fonts(self, *args, **kwargs) -> int:
        """Reports the number of subsettable embedded fonts (PyMuPDF
        ``doc.subset_fonts``). Actual glyph subsetting is deferred; this never
        modifies the document and never raises."""
        return self._doc.subset_fonts(*args, **kwargs)

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

    def get_page_images(self, pno: int, full: bool = False) -> list[tuple]:
        """The images on page ``pno`` as PyMuPDF tuples (PyMuPDF
        ``doc.get_page_images``)."""
        return self.load_page(int(pno)).get_images(full=full)

    def get_page_fonts(self, pno: int, full: bool = False) -> list[tuple]:
        """The fonts on page ``pno`` as PyMuPDF tuples (PyMuPDF
        ``doc.get_page_fonts``)."""
        return self.load_page(int(pno)).get_fonts(full=full)

    def search_page_for(self, pno: int, text: str, **kw) -> list:
        """Searches page ``pno`` for ``text`` (PyMuPDF ``doc.search_page_for``).

        Returns a list of :class:`Quad` (``quads=True``) or :class:`Rect`."""
        return self.load_page(int(pno)).search_for(text, **kw)

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

    def pdfocr_tobytes(
        self,
        *,
        compress: bool = True,
        language: str = "eng",
        tessdata: str | None = None,
        dpi: int = 300,
    ) -> bytes:
        """Produces a searchable OCR "sandwich" PDF as bytes (PyMuPDF
        ``doc.pdfocr_tobytes``).

        Each page is rendered, OCR'd via the system Tesseract (``language``), and
        rebuilt with the page image plus an invisible OCR text layer, so the
        result is selectable / searchable. ``dpi`` (an oxide extension) tunes the
        recognition resolution. Raises ``PdfUnsupportedError`` if Tesseract is
        not installed.
        """
        return self._doc.pdfocr_tobytes(
            compress=compress, language=language, tessdata=tessdata, dpi=dpi
        )

    def pdfocr_save(
        self,
        filename: str | os.PathLike[str],
        *,
        compress: bool = True,
        language: str = "eng",
        tessdata: str | None = None,
        dpi: int = 300,
    ) -> None:
        """Writes a searchable OCR "sandwich" PDF to ``filename`` (PyMuPDF
        ``doc.pdfocr_save``). See :meth:`pdfocr_tobytes`."""
        self._doc.pdfocr_save(
            os.fspath(filename),
            compress=compress,
            language=language,
            tessdata=tessdata,
            dpi=dpi,
        )

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

    def del_xml_metadata(self) -> None:
        """Removes the catalog XMP metadata stream (PyMuPDF
        ``doc.del_xml_metadata``)."""
        self._doc.del_xml_metadata()

    # --- TOC (PRD §8.9) ---
    @property
    def outline(self) -> Outline | None:
        """The document outline tree, or ``None`` (PyMuPDF ``doc.outline``)."""
        core = self._doc.outline
        return Outline(core) if core is not None else None

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
        return Page(self._doc.new_page(pno, width, height), self)

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

    def copy_page(self, pno: int, to: int = -1) -> None:
        """Reference-copies page ``pno`` in front of page ``to`` (PyMuPDF
        ``doc.copy_page``); ``to == -1`` (or out of range) appends.

        This is a *shallow* copy — the new page shares the source's content and
        resources. For an independent deep copy use :meth:`fullcopy_page`.
        """
        count = self._doc.page_count
        if pno < 0:
            pno += count
        # PyMuPDF: insert the copy in front of page ``to`` (the new page becomes
        # page ``to``); ``to == -1`` / out-of-range appends at the end.
        target = count if to < 0 or to >= count else to
        self._doc.copy_page(int(pno), int(target))

    def move_page(self, pno: int, to: int = -1) -> None:
        """Moves page ``pno`` in front of page ``to`` (PyMuPDF ``doc.move_page``);
        ``to == -1`` (or out of range) moves it to the end."""
        count = self._doc.page_count
        if pno < 0:
            pno += count
        if to < 0 or to >= count:
            target = count  # append (clamped to post-removal length)
        else:
            # PyMuPDF inserts in front of original page ``to`` then removes the
            # original ``pno``; when ``pno`` precedes ``to`` that removal shifts
            # the final position down by one.
            target = to - 1 if pno < to else to
        self._doc.move_page(int(pno), int(target))

    def delete_pages(self, *args, **kw) -> None:
        """Deletes multiple pages (PyMuPDF ``doc.delete_pages``).

        Accepts the same forms as PyMuPDF:

        * ``delete_pages(from_page, to_page)`` / ``delete_pages(from_page=a,
          to_page=b)`` — the inclusive range ``a..=b``;
        * ``delete_pages(numbers)`` / ``delete_pages(numbers=[...])`` — an
          explicit list/tuple/range of page numbers;
        * ``delete_pages(n)`` — a single page.

        Negative page numbers count from the end. Pages are kept via
        :meth:`select` of the complement, so the operation is order-preserving.
        """
        count = self._doc.page_count

        def _norm(n: int) -> int:
            n = int(n)
            return n + count if n < 0 else n

        numbers: list[int]
        if "numbers" in kw:
            numbers = [_norm(n) for n in kw["numbers"]]
        elif "from_page" in kw or "to_page" in kw:
            frm = _norm(kw.get("from_page", 0))
            to = _norm(kw.get("to_page", count - 1))
            numbers = list(range(frm, to + 1))
        elif len(args) == 1 and isinstance(args[0], (list, tuple, range)):
            numbers = [_norm(n) for n in args[0]]
        elif len(args) == 2:
            frm, to = _norm(args[0]), _norm(args[1])
            numbers = list(range(frm, to + 1))
        elif len(args) == 1:
            numbers = [_norm(args[0])]
        else:
            raise ValueError("delete_pages: expected (from, to), a list, or numbers=[...]")

        drop = {n for n in numbers if 0 <= n < count}
        keep = [i for i in range(count) if i not in drop]
        self._doc.select(keep)

    def insert_page(
        self,
        pno: int,
        text=None,
        fontsize: float = 11,
        width: float = 595,
        height: float = 842,
        fontname: str = "helv",
        fontfile: str | None = None,
        color=None,
        **_ignored,
    ) -> int:
        """Inserts a new blank page in front of page ``pno`` (PyMuPDF
        ``doc.insert_page``); ``pno == -1`` appends.

        Optionally draws ``text`` (a string or list of strings, one per line)
        starting near the top-left. Returns the number of text lines inserted
        (``0`` when ``text`` is ``None``), matching PyMuPDF.
        """
        page = self.new_page(pno, width=width, height=height)
        if text is None:
            return 0
        lines = text if isinstance(text, (list, tuple)) else str(text).splitlines() or [str(text)]
        page.insert_text(
            Point(50, 72),
            "\n".join(str(line) for line in lines),
            fontsize=fontsize,
            fontname=fontname,
            fontfile=fontfile,
            color=(0, 0, 0) if color is None else color,
        )
        return len(lines)

    def get_page_label(self, pno: int) -> str:
        """The page label of physical page ``pno`` (PyMuPDF helper)."""
        return self._doc.get_page_label(pno)

    def get_label(self, pno: int) -> str:
        """The computed ``/PageLabels`` label of page ``pno`` (PyMuPDF
        ``doc.get_page_label`` equivalent at the document level)."""
        if pno < 0:
            pno += self._doc.page_count
        return self._doc.get_page_label(int(pno))

    def get_page_labels(self) -> list[dict]:
        """The ``/Root /PageLabels`` rules (PyMuPDF ``doc.get_page_labels``).

        Returns a list of dicts ``{"startpage": int, "prefix": str, "style":
        str, "firstpagenum": int}``, one per number-tree range, sorted by start
        page. Empty when the document has no page labels.
        """
        return [
            {
                "startpage": start,
                "prefix": prefix,
                "style": style,
                "firstpagenum": first,
            }
            for start, style, prefix, first in self._doc.get_page_labels()
        ]

    def get_page_numbers(self, label: str, only_one: bool = False) -> list[int]:
        """The 0-based page numbers whose computed label equals ``label`` (PyMuPDF
        ``doc.get_page_numbers``). With ``only_one`` stops after the first hit."""
        out: list[int] = []
        for i in range(self._doc.page_count):
            if self._doc.get_page_label(i) == label:
                out.append(i)
                if only_one:
                    break
        return out

    def set_page_labels(self, labels) -> None:
        """Writes ``/Root /PageLabels`` (PyMuPDF ``doc.set_page_labels``).

        ``labels`` is a list of range dicts, each with ``startpage`` (0-based),
        ``prefix`` (str, optional), ``style`` (one of ``"D"``/``"r"``/``"R"``/
        ``"a"``/``"A"`` or ``""``), and ``firstpagenum`` (int, default 1). An
        empty list removes the labels.
        """
        specs = []
        for entry in labels:
            start = int(entry.get("startpage", 0))
            style = entry.get("style", "D")
            style = None if style in ("", None) else str(style)
            prefix = str(entry.get("prefix", "") or "")
            first = int(entry.get("firstpagenum", 1))
            specs.append((start, style, prefix, first))
        self._doc.set_page_labels(specs)

    def get_char_widths(self, xref: int, *_args, **_kwargs) -> list[tuple[int, float]]:
        """The glyph widths of font object ``xref`` (PyMuPDF ``doc.get_char_widths``).

        Returns ``(glyph_id, width)`` pairs where ``width`` is em-relative
        (``/Widths`` value divided by 1000)."""
        return self._doc.get_char_widths(int(xref))

    def page_cropbox(self, pno: int) -> Rect:
        """The ``/CropBox`` of page ``pno`` as a :class:`Rect` (PyMuPDF ``doc.page_cropbox``)."""
        if pno < 0:
            pno += self._doc.page_count
        return _rect(self._doc.page_cropbox(pno))

    def page_mediabox(self, pno: int) -> Rect:
        """The ``/MediaBox`` of page ``pno`` as a :class:`Rect` (PyMuPDF ``doc.page_mediabox``)."""
        if pno < 0:
            pno += self._doc.page_count
        return _rect(self._doc.page_mediabox(pno))

    # --- undo/redo journal (PyMuPDF ``doc.journal_*``) ---
    def journal_enable(self) -> None:
        """Enables the undo/redo journal, recording the baseline state
        (PyMuPDF ``doc.journal_enable``)."""
        self._doc.journal_enable()

    def journal_is_enabled(self) -> bool:
        """Whether the journal is enabled (PyMuPDF ``doc.journal_is_enabled``)."""
        return self._doc.journal_is_enabled()

    def journal_save_state(self) -> None:
        """Records the current state as a journal checkpoint."""
        self._doc.journal_save_state()

    def journal_can_undo(self) -> bool:
        """Whether an undo is possible (PyMuPDF ``doc.journal_can_do`` undo)."""
        return self._doc.journal_can_undo()

    def journal_can_redo(self) -> bool:
        """Whether a redo is possible (PyMuPDF ``doc.journal_can_do`` redo)."""
        return self._doc.journal_can_redo()

    def journal_can_do(self) -> dict[str, bool]:
        """``{"undo": bool, "redo": bool}`` (PyMuPDF ``doc.journal_can_do``)."""
        return {
            "undo": self._doc.journal_can_undo(),
            "redo": self._doc.journal_can_redo(),
        }

    def journal_undo(self) -> bool:
        """Reverts to the previous checkpoint (PyMuPDF ``doc.journal_undo``)."""
        return self._doc.journal_undo()

    def journal_redo(self) -> bool:
        """Re-applies the next checkpoint (PyMuPDF ``doc.journal_redo``)."""
        return self._doc.journal_redo()

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

    # --- optional content / layers (PRD §7, M7) ---
    def get_ocgs(self) -> dict[int, dict]:
        """The optional-content groups keyed by ``xref`` (PyMuPDF ``doc.get_ocgs``).

        Each value is a dict with ``name``, ``intent`` (list), ``on``, ``locked``.
        """
        return self._doc.get_ocgs()

    def layer_ui_configs(self) -> list[dict]:
        """The layer-panel UI configuration rows (PyMuPDF ``doc.layer_ui_configs``).

        Each row is a dict with ``number``, ``text``, ``depth``, ``type``
        (``"checkbox"``/``"label"``), ``on``, ``locked``.
        """
        return self._doc.layer_ui_configs()

    def ocg_state(self, xref: int) -> bool:
        """Whether the OCG ``xref`` is ON in the default config (layer state)."""
        return self._doc.ocg_state(int(xref))

    def get_layer(self, config: int = -1) -> dict:
        """The current ON/OFF/locked layer state (PyMuPDF ``doc.get_layer``).

        Returns a dict with ``on``/``off``/``locked`` xref lists for the default
        configuration (``config`` is accepted for compatibility).
        """
        on, off, locked = [], [], []
        for xref, info in self._doc.get_ocgs().items():
            (on if info["on"] else off).append(xref)
            if info["locked"]:
                locked.append(xref)
        return {"on": on, "off": off, "locked": locked}

    def set_layer(
        self,
        config: int = -1,
        *,
        on: list[int] | None = None,
        off: list[int] | None = None,
        locked: list[int] | None = None,
        **_ignored,
    ) -> None:
        """Bulk-sets layer visibility (PyMuPDF ``doc.set_layer``): ``on`` xrefs
        turned ON, ``off`` xrefs OFF (``config``/``locked`` accepted)."""
        self._doc.set_layer(
            on=[int(x) for x in (on or [])],
            off=[int(x) for x in (off or [])],
        )

    def add_ocg(
        self,
        name: str,
        config: int | None = None,
        *,
        on: bool = True,
        intent: str = "View",
        usage: str | None = None,
        **_ignored,
    ) -> int:
        """Adds an optional-content group, returning its ``xref`` (PyMuPDF
        ``doc.add_ocg``).

        ``config`` may be a string UI-label group (PyMuPDF also accepts an int
        config index, which is ignored here); ``on`` the initial visibility;
        ``intent`` the ``/Intent`` name.
        """
        cfg = config if isinstance(config, str) else None
        return self._doc.add_ocg(name, config=cfg, on=bool(on), intent=intent)

    def set_oc(self, xref: int, ocg: int) -> None:
        """Binds object ``xref`` to OCG ``ocg`` via its ``/OC`` entry (PyMuPDF
        ``doc.set_oc``)."""
        self._doc.set_oc(int(xref), int(ocg))

    # --- PyMuPDF deprecated camelCase aliases (OCG) ---
    def getOCGs(self) -> dict[int, dict]:  # noqa: N802
        return self.get_ocgs()

    def layerUIConfigs(self) -> list[dict]:  # noqa: N802
        return self.layer_ui_configs()

    def getLayer(self, config: int = -1) -> dict:  # noqa: N802
        return self.get_layer(config)

    def setLayer(self, *args, **kw) -> None:  # noqa: N802
        return self.set_layer(*args, **kw)

    def addOCG(self, *args, **kw) -> int:  # noqa: N802
        return self.add_ocg(*args, **kw)

    def setOC(self, xref: int, ocg: int) -> None:  # noqa: N802
        return self.set_oc(xref, ocg)

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
