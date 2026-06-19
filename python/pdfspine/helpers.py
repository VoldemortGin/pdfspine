"""Module-level helper functions mirroring PyMuPDF (``fitz``).

These are the pure-Python utilities PyMuPDF exposes at module scope: text-quad
reconstruction from ``get_text("dict"/"rawdict")`` pieces, the line-planishing
matrix, Adobe-glyph/unicode name lookups, sRGB colour conversion, PDF date/string
formatting, built-in-font text measurement, the HTML/XML/XHTML conversion
wrappers, and the message/log output shims. Every signature, return shape and
value matches real PyMuPDF 1.27 (cross-checked in ``test_longtail11.py``).
"""

from __future__ import annotations

import inspect
import io
import os
import sys
import time
import unicodedata
from typing import Any

from . import _core
from .document import TOOLS
from .geometry import Matrix, Point, Quad, Rect

# Built-in (base-14) font-name dictionary, identical to fitz's ``Base14_fontdict``:
# every accepted alias maps to its canonical PostScript name.
Base14_fontdict: dict[str, str] = {
    "courier": "Courier",
    "courier-oblique": "Courier-Oblique",
    "courier-bold": "Courier-Bold",
    "courier-boldoblique": "Courier-BoldOblique",
    "helvetica": "Helvetica",
    "helvetica-oblique": "Helvetica-Oblique",
    "helvetica-bold": "Helvetica-Bold",
    "helvetica-boldoblique": "Helvetica-BoldOblique",
    "times-roman": "Times-Roman",
    "times-italic": "Times-Italic",
    "times-bold": "Times-Bold",
    "times-bolditalic": "Times-BoldItalic",
    "symbol": "Symbol",
    "zapfdingbats": "ZapfDingbats",
    "helv": "Helvetica",
    "heit": "Helvetica-Oblique",
    "hebo": "Helvetica-Bold",
    "hebi": "Helvetica-BoldOblique",
    "cour": "Courier",
    "coit": "Courier-Oblique",
    "cobo": "Courier-Bold",
    "cobi": "Courier-BoldOblique",
    "tiro": "Times-Roman",
    "tibo": "Times-Bold",
    "tiit": "Times-Italic",
    "tibi": "Times-BoldItalic",
    "symb": "Symbol",
    "zadb": "ZapfDingbats",
}

# Per-glyph advance widths (units of 1/1000 em) for the two non-Latin built-in
# fonts; indexed by byte code, identical to fitz's ``symbol_glyphs``/``zapf_glyphs``.
_SYMBOL_WIDTHS: tuple[float, ...] = (
    0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46,
    0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46,
    0.25, 0.333, 0.713, 0.5, 0.549, 0.833, 0.778, 0.439, 0.333, 0.333, 0.5, 0.549, 0.25, 0.549, 0.25, 0.278,
    0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.278, 0.278, 0.549, 0.549, 0.549, 0.444,
    0.549, 0.722, 0.667, 0.722, 0.612, 0.611, 0.763, 0.603, 0.722, 0.333, 0.631, 0.722, 0.686, 0.889, 0.722, 0.722,
    0.768, 0.741, 0.556, 0.592, 0.611, 0.69, 0.439, 0.768, 0.645, 0.795, 0.611, 0.333, 0.863, 0.333, 0.658, 0.5,
    0.5, 0.631, 0.549, 0.549, 0.494, 0.439, 0.521, 0.411, 0.603, 0.329, 0.603, 0.549, 0.549, 0.576, 0.521, 0.549,
    0.549, 0.521, 0.549, 0.603, 0.439, 0.576, 0.713, 0.686, 0.493, 0.686, 0.494, 0.48, 0.2, 0.48, 0.549, 0.46,
    0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46,
    0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46, 0.46,
    0.25, 0.62, 0.247, 0.549, 0.167, 0.713, 0.5, 0.753, 0.753, 0.753, 0.753, 1.042, 0.713, 0.603, 0.987, 0.603,
    0.4, 0.549, 0.411, 0.549, 0.549, 0.576, 0.494, 0.46, 0.549, 0.549, 0.549, 0.549, 1.0, 0.603, 1.0, 0.658,
    0.823, 0.686, 0.795, 0.987, 0.768, 0.768, 0.823, 0.768, 0.768, 0.713, 0.713, 0.713, 0.713, 0.713, 0.713, 0.713,
    0.768, 0.713, 0.79, 0.79, 0.89, 0.823, 0.549, 0.549, 0.713, 0.603, 0.603, 1.042, 0.987, 0.603, 0.987, 0.603,
    0.494, 0.329, 0.79, 0.79, 0.786, 0.713, 0.384, 0.384, 0.384, 0.384, 0.384, 0.384, 0.494, 0.494, 0.494, 0.494,
    0.46, 0.329, 0.274, 0.686, 0.686, 0.686, 0.384, 0.549, 0.384, 0.384, 0.384, 0.384, 0.494, 0.494, 0.494, 0.46,
)
_ZAPF_WIDTHS: tuple[float, ...] = (
    0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788,
    0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788,
    0.278, 0.974, 0.961, 0.974, 0.98, 0.719, 0.789, 0.79, 0.791, 0.69, 0.96, 0.939, 0.549, 0.855, 0.911, 0.933,
    0.911, 0.945, 0.974, 0.755, 0.846, 0.762, 0.761, 0.571, 0.677, 0.763, 0.76, 0.759, 0.754, 0.494, 0.552, 0.537,
    0.577, 0.692, 0.786, 0.788, 0.788, 0.79, 0.793, 0.794, 0.816, 0.823, 0.789, 0.841, 0.823, 0.833, 0.816, 0.831,
    0.923, 0.744, 0.723, 0.749, 0.79, 0.792, 0.695, 0.776, 0.768, 0.792, 0.759, 0.707, 0.708, 0.682, 0.701, 0.826,
    0.815, 0.789, 0.789, 0.707, 0.687, 0.696, 0.689, 0.786, 0.787, 0.713, 0.791, 0.785, 0.791, 0.873, 0.761, 0.762,
    0.762, 0.759, 0.759, 0.892, 0.892, 0.788, 0.784, 0.438, 0.138, 0.277, 0.415, 0.392, 0.392, 0.668, 0.668, 0.788,
    0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788,
    0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788,
    0.732, 0.544, 0.544, 0.91, 0.667, 0.76, 0.76, 0.776, 0.595, 0.694, 0.626, 0.788, 0.788, 0.788, 0.788, 0.788,
    0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788,
    0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788, 0.788,
    0.894, 0.838, 1.016, 0.458, 0.748, 0.924, 0.748, 0.918, 0.927, 0.928, 0.928, 0.834, 0.873, 0.828, 0.924, 0.924,
    0.917, 0.93, 0.931, 0.463, 0.883, 0.836, 0.836, 0.867, 0.867, 0.696, 0.696, 0.874, 0.788, 0.874, 0.76, 0.946,
    0.771, 0.865, 0.771, 0.888, 0.967, 0.888, 0.831, 0.873, 0.927, 0.97, 0.788, 0.788,
)

# CJK built-in fonts measured as one em per character (fitz parity).
_CJK_FONTS = frozenset(
    {"china-t", "china-s", "china-ts", "china-ss", "japan", "japan-s", "korea", "korea-s"}
)


# ---------------------------------------------------------------------------
# Geometry helpers
# ---------------------------------------------------------------------------
def planish_line(p1, p2) -> Matrix:
    """The matrix mapping the line ``p1`` -> ``p2`` onto the x-axis.

    ``p1`` is mapped to ``(0, 0)`` and ``p2`` to a point on the positive x-axis
    at the same distance (PyMuPDF ``fitz.planish_line``).
    """
    p1 = Point(p1)
    p2 = Point(p2)
    dx = p2.x - p1.x
    dy = p2.y - p1.y
    length = (dx * dx + dy * dy) ** 0.5
    if length == 0:  # degenerate: fitz normalises (0,0) to (0,0) -> all-zero matrix
        return Matrix(0, 0, 0, 0, 0, 0)
    s_x = dx / length
    s_y = dy / length
    # m1 = translate(-p1), m2 = rotate by -angle; result = m1 * m2.
    m1 = Matrix(1, 0, 0, 1, -p1.x, -p1.y)
    m2 = Matrix(s_x, -s_y, s_y, s_x, 0, 0)
    return m1 * m2


def recover_bbox_quad(line_dir, span: dict, bbox) -> Quad:
    """The quad enclosing ``bbox`` inside ``span`` (PyMuPDF ``recover_bbox_quad``)."""
    if line_dir is None:
        line_dir = span["dir"]
    cos, sin = line_dir
    bbox = Rect(bbox)
    if TOOLS.set_small_glyph_heights():
        d = 1
    else:
        d = span["ascender"] - span["descender"]

    height = d * span["size"]
    hs = height * sin
    hc = height * cos
    if hc >= 0 and hs <= 0:  # quadrant 1
        ul = bbox.bl - (0, hc)
        ur = bbox.tr + (hs, 0)
        ll = bbox.bl - (hs, 0)
        lr = bbox.tr + (0, hc)
    elif hc <= 0 and hs <= 0:  # quadrant 2
        ul = bbox.br + (hs, 0)
        ur = bbox.tl - (0, hc)
        ll = bbox.br + (0, hc)
        lr = bbox.tl - (hs, 0)
    elif hc <= 0 and hs >= 0:  # quadrant 3
        ul = bbox.tr - (0, hc)
        ur = bbox.bl + (hs, 0)
        ll = bbox.tr - (hs, 0)
        lr = bbox.bl + (0, hc)
    else:  # quadrant 4
        ul = bbox.tl + (hs, 0)
        ur = bbox.br - (0, hc)
        ll = bbox.tl + (0, hc)
        lr = bbox.br - (hs, 0)
    return Quad(ul, ur, ll, lr)


def recover_quad(line_dir, span: dict) -> Quad:
    """The quad enveloping a text span (PyMuPDF ``recover_quad``)."""
    if type(line_dir) is not tuple or len(line_dir) != 2:
        raise ValueError("bad line dir argument")
    if type(span) is not dict:
        raise ValueError("bad span argument")
    return recover_bbox_quad(line_dir, span, span["bbox"])


def recover_char_quad(line_dir, span: dict, char) -> Quad:
    """The quad enveloping a single character (PyMuPDF ``recover_char_quad``)."""
    if line_dir is None:
        line_dir = span["dir"]
    if type(line_dir) is not tuple or len(line_dir) != 2:
        raise ValueError("bad line dir argument")
    if type(span) is not dict:
        raise ValueError("bad span argument")
    if type(char) is dict:
        bbox = Rect(char["bbox"])
    elif type(char) is tuple:
        bbox = Rect(char[3])
    else:
        raise ValueError("bad span argument")
    return recover_bbox_quad(line_dir, span, bbox)


def recover_line_quad(line: dict, spans: list | None = None) -> Quad:
    """The quad covering selected spans of a line (PyMuPDF ``recover_line_quad``)."""
    if spans is None:
        spans = line["spans"]
    if len(spans) == 0:
        raise ValueError("bad span list")
    line_dir = line["dir"]
    q0 = recover_quad(line_dir, spans[0])
    if len(spans) > 1:
        q1 = recover_quad(line_dir, spans[-1])
    else:
        q1 = q0

    line_ll = q0.ll
    line_lr = q1.lr
    mat0 = planish_line(line_ll, line_lr)
    x_lr = line_lr * mat0

    small = TOOLS.set_small_glyph_heights()
    h = max(
        s["size"] * (1 if small else (s["ascender"] - s["descender"])) for s in spans
    )
    line_rect = Rect(0, -h, x_lr.x, 0)
    line_quad = line_rect.quad
    line_quad *= ~mat0
    return line_quad


def recover_span_quad(line_dir, span: dict, chars: list | None = None) -> Quad:
    """The quad covering selected characters of a span (PyMuPDF ``recover_span_quad``)."""
    if line_dir is None:
        line_dir = span["dir"]
    if chars is None:
        return recover_quad(line_dir, span)
    if "chars" not in span.keys():
        raise ValueError("need 'rawdict' option to sub-select chars")

    q0 = recover_char_quad(line_dir, span, chars[0])
    if len(chars) > 1:
        q1 = recover_char_quad(line_dir, span, chars[-1])
    else:
        q1 = q0

    span_ll = q0.ll
    span_lr = q1.lr
    mat0 = planish_line(span_ll, span_lr)
    x_lr = span_lr * mat0

    small = TOOLS.set_small_glyph_heights()
    h = span["size"] * (1 if small else (span["ascender"] - span["descender"]))
    span_rect = Rect(0, -h, x_lr.x, 0)
    span_quad = span_rect.quad
    span_quad *= ~mat0
    return span_quad


# ---------------------------------------------------------------------------
# Glyph-name / unicode lookups
# ---------------------------------------------------------------------------
def glyph_name_to_unicode(name: str) -> int:
    """The unicode code point for a glyph name (PyMuPDF ``glyph_name_to_unicode``).

    Uses :mod:`unicodedata`; returns ``0xFFFD`` (65533) when the name is unknown.
    """
    try:
        return ord(unicodedata.lookup(name))
    except Exception:
        return 65533


def unicode_to_glyph_name(ch: int) -> str:
    """The glyph name for a unicode code point (PyMuPDF ``unicode_to_glyph_name``).

    Uses :mod:`unicodedata`; returns ``".notdef"`` when unnamed.
    """
    try:
        return unicodedata.name(chr(ch))
    except ValueError:
        return ".notdef"


# ---------------------------------------------------------------------------
# Colour conversion
# ---------------------------------------------------------------------------
def sRGB_to_rgb(srgb: int) -> tuple[int, int, int]:  # noqa: N802 (fitz name)
    """Convert an sRGB integer ``0xRRGGBB`` to an ``(r, g, b)`` int triple."""
    srgb &= 0xFFFFFF
    r = srgb >> 16
    g = (srgb - (r << 16)) >> 8
    b = srgb - (r << 16) - (g << 8)
    return (r, g, b)


def sRGB_to_pdf(srgb: int) -> tuple[float, float, float]:  # noqa: N802 (fitz name)
    """Convert an sRGB integer to an ``(r, g, b)`` float triple in ``[0, 1]``."""
    r, g, b = sRGB_to_rgb(srgb)
    return (r / 255.0, g / 255.0, b / 255.0)


# ---------------------------------------------------------------------------
# PDF date / string formatting
# ---------------------------------------------------------------------------
def get_pdf_now() -> str:
    """The current local time as a PDF date string ``D:YYYYMMDDHHmmSS±HH'mm'``."""
    a = str(abs(time.altzone // 3600)).rjust(2, "0")
    b = str(abs(time.altzone // 60) % 60).rjust(2, "0")
    tz = f"{a}'{b}'"
    tstamp = time.strftime("D:%Y%m%d%H%M%S", time.localtime())
    if time.altzone > 0:
        tstamp += "-" + tz
    elif time.altzone < 0:
        tstamp += "+" + tz
    return tstamp


def get_pdf_str(s: str) -> str:
    """Escape ``s`` as a PDF literal ``(...)`` / hex ``<...>`` string (fitz parity)."""
    if not bool(s):
        return "()"

    def make_utf16be(s: str) -> str:
        r = bytearray([254, 255]) + bytearray(s, "UTF-16BE")
        return "<" + r.hex() + ">"

    r = ""
    for c in s:
        oc = ord(c)
        if oc > 255:
            return make_utf16be(s)
        if 31 < oc < 127:
            if c in ("(", ")", "\\"):
                r += "\\"
            r += c
            continue
        if oc > 127:
            r += f"\\{oc:03o}"
            continue
        if oc == 8:
            r += "\\b"
        elif oc == 9:
            r += "\\t"
        elif oc == 10:
            r += "\\n"
        elif oc == 12:
            r += "\\f"
        elif oc == 13:
            r += "\\r"
        else:
            r += "\\267"
    return "(" + r + ")"


def image_profile(stream, keep_image: int = 0) -> dict | None:
    """Basic header properties of a raster image (PyMuPDF ``image_profile``).

    Returns a dict with ``width``, ``height``, ``orientation``, ``transform``,
    ``xres``, ``yres``, ``colorspace`` (component count), ``bpc``, ``ext`` and
    ``cs-name``, or ``None`` for empty / unrecognized input. ``keep_image`` is
    accepted for signature parity (pdfspine never returns the decoded image).
    """
    return _core.image_profile(bytes(stream), keep_image)


# ---------------------------------------------------------------------------
# Text measurement
# ---------------------------------------------------------------------------
def get_text_length(text: str, fontname: str = "helv", fontsize: float = 11, encoding: int = 0) -> float:
    """The advance width of ``text`` for a built-in font (PyMuPDF ``get_text_length``).

    ``encoding`` 0=Latin (default), 1=Greek, 2=Cyrillic. Raises ``ValueError`` for
    unsupported fonts. CJK built-in fonts measure one em per character.
    """
    fontname = fontname.lower()
    basename = Base14_fontdict.get(fontname, None)

    glyphs = None
    if basename == "Symbol":
        glyphs = _SYMBOL_WIDTHS
    elif basename == "ZapfDingbats":
        glyphs = _ZAPF_WIDTHS
    if glyphs is not None:
        w = sum(glyphs[ord(c)] if ord(c) < 256 else glyphs[183] for c in text)
        return w * fontsize

    if fontname in Base14_fontdict:
        return _core.Font(basename).text_length(text, fontsize, encoding)

    if fontname in _CJK_FONTS:
        return len(text) * fontsize

    raise ValueError(f"Font '{fontname}' is unsupported")


# ---------------------------------------------------------------------------
# Conversion header / trailer (text-page serialisation wrappers)
# ---------------------------------------------------------------------------
_HTML_HEADER = (
    "\n<!DOCTYPE html>\n<html>\n<head>\n<style>\n"
    "body{background-color:gray}\n"
    "div{position:relative;background-color:white;margin:1em auto}\n"
    "p{position:absolute;margin:0}\n"
    "img{position:absolute}\n"
    "</style>\n</head>\n<body>\n"
)
_XHTML_HEADER = (
    '\n<?xml version="1.0"?>\n'
    '<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" '
    '"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">\n'
    '<html xmlns="http://www.w3.org/1999/xhtml">\n<head>\n<style>\n'
    "body{background-color:gray}\n"
    "div{background-color:white;margin:1em;padding:1em}\n"
    "p{white-space:pre-wrap}\n"
    "</style>\n</head>\n<body>\n"
)


def ConversionHeader(output: str = "text", filename: str = "unknown") -> str:  # noqa: N802 (fitz name)
    """The header string fitz emits for a serialised text page (PyMuPDF ``ConversionHeader``)."""
    t = output.lower()
    if t == "html":
        return _HTML_HEADER
    if t == "json":
        return f'{{"document": "{filename}", "pages": [\n'
    if t == "xml":
        return f'\n<?xml version="1.0"?>\n<document name="{filename}">\n'
    if t == "xhtml":
        return _XHTML_HEADER
    return ""


def ConversionTrailer(output: str) -> str:  # noqa: N802 (fitz name)
    """The trailer string fitz emits for a serialised text page (PyMuPDF ``ConversionTrailer``)."""
    t = output.lower()
    if t == "html":
        return "</body>\n</html>\n"
    if t == "json":
        return "]\n}"
    if t == "xml":
        return "</document>\n"
    if t == "xhtml":
        return "</body>\n</html>\n"
    return ""


# ---------------------------------------------------------------------------
# Message / log output shims
# ---------------------------------------------------------------------------
_g_out_message: Any = sys.stdout
_g_out_log: Any = sys.stdout


def _make_output(
    *,
    text: str | None = None,
    fd: int | None = None,
    stream: Any = None,
    path: str | None = None,
    path_append: str | None = None,
    pylogging: Any = None,
    pylogging_logger: Any = None,
    pylogging_level: Any = None,
    pylogging_name: Any = None,
    default: Any = None,
) -> Any:
    """Resolve a message/log destination (PyMuPDF ``_make_output``)."""
    if text is not None:
        if text.startswith("fd:"):
            fd = int(text[3:])
        elif text.startswith("path:"):
            path = text[5:]
        elif text.startswith("path+"):
            path_append = text[5:]
        elif text.startswith("logging:"):
            pylogging = True
            items_d: dict[str, str] = {}
            for item in text[8:].split(","):
                if not item:
                    continue
                n, v = item.split("=", 1)
                items_d[n] = v
            lvl = items_d.get("level")
            pylogging_level = int(lvl) if lvl is not None else None
            pylogging_name = items_d.get("name", "pymupdf")
        else:
            raise AssertionError(
                f"Expected prefix `fd:`, `path:`, `path+:` or `logging:` in {text=}."
            )

    if fd is not None:
        return io.open(fd, mode="w", closefd=False)
    if stream is not None:
        return stream
    if path is not None:
        return io.open(path, "w")
    if path_append is not None:
        return io.open(path_append, "a")
    if (
        pylogging is not None
        or pylogging_logger is not None
        or pylogging_level is not None
        or pylogging_name is not None
    ):
        import logging

        if pylogging_logger is None:
            pylogging_logger = logging.getLogger(pylogging_name or "pymupdf")
        if pylogging_level is None:
            pylogging_level = pylogging_logger.getEffectiveLevel()

        class _Out:
            def write(self, s: str) -> None:
                s = s.rstrip("\n")
                if s:
                    pylogging_logger.log(pylogging_level, s)

            def flush(self) -> None:
                pass

        return _Out()
    return default


def set_messages(
    *,
    text: str | None = None,
    fd: int | None = None,
    stream: Any = None,
    path: str | None = None,
    path_append: str | None = None,
    pylogging: Any = None,
    pylogging_logger: Any = None,
    pylogging_level: Any = None,
    pylogging_name: Any = None,
) -> None:
    """Set the destination of user messages (PyMuPDF ``set_messages``)."""
    global _g_out_message
    _g_out_message = _make_output(
        text=text,
        fd=fd,
        stream=stream,
        path=path,
        path_append=path_append,
        pylogging=pylogging,
        pylogging_logger=pylogging_logger,
        pylogging_level=pylogging_level,
        pylogging_name=pylogging_name,
        default=_g_out_message,
    )


def message(text: str = "") -> None:
    """Emit a user message to the configured destination (PyMuPDF ``message``)."""
    if _g_out_message:
        print(text, file=_g_out_message, flush=True)


def set_log(
    *,
    text: str | None = None,
    fd: int | None = None,
    stream: Any = None,
    path: str | None = None,
    path_append: str | None = None,
    pylogging: Any = None,
    pylogging_logger: Any = None,
    pylogging_level: Any = None,
    pylogging_name: Any = None,
) -> None:
    """Set the destination of development/debug logging (PyMuPDF ``set_log``)."""
    global _g_out_log
    _g_out_log = _make_output(
        text=text,
        fd=fd,
        stream=stream,
        path=path,
        path_append=path_append,
        pylogging=pylogging,
        pylogging_logger=pylogging_logger,
        pylogging_level=pylogging_level,
        pylogging_name=pylogging_name,
        default=_g_out_log,
    )


def log(text: str = "", caller: int = 1) -> None:
    """Emit a development/debug diagnostic, prefixed with caller info (PyMuPDF ``log``)."""
    try:
        stack = inspect.stack(context=0)
    except StopIteration:
        pass
    else:
        frame_record = stack[caller]
        try:
            filename = os.path.relpath(frame_record.filename)
        except Exception:
            filename = frame_record.filename
        text = f"{filename}:{frame_record.lineno}:{frame_record.function}(): {text}"
        del stack
    if _g_out_log:
        print(text, file=_g_out_log, flush=True)


__all__ = [
    "Base14_fontdict",
    "planish_line",
    "recover_quad",
    "recover_char_quad",
    "recover_line_quad",
    "recover_span_quad",
    "recover_bbox_quad",
    "glyph_name_to_unicode",
    "unicode_to_glyph_name",
    "sRGB_to_rgb",
    "sRGB_to_pdf",
    "get_pdf_now",
    "get_pdf_str",
    "get_text_length",
    "image_profile",
    "ConversionHeader",
    "ConversionTrailer",
    "set_messages",
    "message",
    "set_log",
    "log",
]
