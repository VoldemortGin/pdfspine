"""Long-tail PyMuPDF parity batch 9 — TextPage members (PRD §C batch-4).

Covers the newly-implemented ``fitz.TextPage`` surface, all built on the existing
structured-text model (blocks → lines → spans, with bbox / font / size / color):
  - ``extractHTML`` / ``extractXHTML`` / ``extractXML``: fitz-shaped serializers.
  - ``extractTextbox(rect)``: the text whose lines intersect ``rect``.
  - ``extractSelection(a, b)``: text between two points, like a mouse drag.
  - ``search(needle, quads)``: hit rects (or quads).
  - ``extractIMGINFO``: per-image info dicts.
  - ``poolsize``: a structured-text byte-footprint stat.

The text content, hit-rect X coordinates, textbox, and selection results below are
the GROUND TRUTH captured from REAL PyMuPDF 1.27 (``.venv-oracle``) reading the
EXACT SAME fixture bytes (the in-repo ``.venv`` ``fitz`` is the pdfspine shim). The
oracle values are embedded here as literals.

DEVIATIONS from fitz (documented):
  - Hit-rect / char-bbox Y coordinates: pdfspine uses a tighter glyph box
    (ascender/descender) than MuPDF's line box, so hit-rect Y differs by a few
    points. X coordinates and all text are an exact match; the tests assert X
    tightly (~1px) and Y loosely (~5px). This is a pre-existing text-model trait,
    not introduced by this batch.
  - ``search`` defaults to ``quads=False`` per the batch-4 spec; PyMuPDF's native
    ``TextPage.search`` defaults to ``quads=1``. Both modes are exercised.
  - ``search`` whitespace handling matches PyMuPDF: a line break reads as one
    space, whitespace runs collapse to a single space (in BOTH page and needle),
    and a spaced needle bridges a line break (one hit rect per line fragment,
    X exact); a needle WITHOUT whitespace cannot bridge a break.
  - ``extractTextbox`` clips per character: a char is kept iff its bbox overlaps
    the clip in both X and Y (strict overlap). So a clip narrower than a line
    trims that line's head/tail rather than including/dropping the whole line.
    Verified char-for-char against the oracle.
  - ``extractSelection`` resolves each endpoint to a line by fitz's
    baseline-relative line box ``[baseline - 0.875*size, baseline + 0.125*size]``
    and within that line by char position; an end point at/below the last line
    keeps it in full. Text matches the oracle exactly (Y of the box is derived
    from the shared baseline, which is identical between pdfspine and fitz).
  - HTML/XHTML/XML are structurally fitz-shaped (tag nesting, key attributes,
    coordinates) and valid/parseable, but not byte-exact: the CSS ``font-family``
    is the raw PDF font name (fitz substitutes ``Arial,sans-serif``); ``<p>`` is
    per-line (fitz may promote headings to ``<h3>``); image ``<img>`` carries no
    data-URI ``src`` (pixel bytes deferred to M5). XML char ``flags`` is the
    span flag bitfield.
  - ``poolsize`` returns a deterministic model-footprint proxy (MuPDF's value is
    its internal ``fz_pool`` allocator size, not portable outside MuPDF).

FONT deviations / deferrals (documented):
  - ``Font.glyph_bbox`` and ``Font.buffer`` are DEFERRED (raise
    ``PdfUnsupportedError``): pdfspine's ``Font`` is a Core-14 metrics-only handle
    (built from a font name; no embedded ``/FontFile*`` program, no per-glyph
    outlines). A constant font-level bbox / empty buffer would be misleading, so
    both honestly raise until embedded-font programs are wired up.
  - ``Font.valid_codepoints`` reflects the font's built-in PDF encoding coverage
    for a non-embedded (Core-14) handle (WinAnsi for text fonts; the font's own
    encoding for Symbol/ZapfDingbats) — an honest STRICT SUBSET of PyMuPDF's
    bundled-cmap set (no false positives; verified below). For an embedded font
    the real cmap would be preferred.
  - ``Base14_fontnames`` and ``is_writable`` match fitz exactly.
"""

from __future__ import annotations

import base64
import xml.dom.minidom as minidom

import fitz
import pdfspine
import pytest


# === fixture (deterministic raw PDF; same bytes pdfspine AND the oracle read) ====

# Three text lines on a 400x300 page, Helvetica:
#   "Hello World"          @ 14pt, baseline y=220 (PDF) → top-origin top≈68.8
#   "Second Line here"     @ 12pt, baseline y=180
#   "Third paragraph text" @ 12pt, baseline y=140
_PDF_B64 = (
    "JVBERi0xLjcKMSAwIG9iajw8L1R5cGUvQ2F0YWxvZy9QYWdlcyAyIDAgUj4+ZW5kb2JqCjIgMCBv"
    "Ymo8PC9UeXBlL1BhZ2VzL0NvdW50IDEvS2lkc1szIDAgUl0+PmVuZG9iagozIDAgb2JqPDwvVHlw"
    "ZS9QYWdlL1BhcmVudCAyIDAgUi9NZWRpYUJveFswIDAgNDAwIDMwMF0vUmVzb3VyY2VzPDwvRm9u"
    "dDw8L0YxIDUgMCBSPj4+Pi9Db250ZW50cyA0IDAgUj4+ZW5kb2JqCjQgMCBvYmo8PC9MZW5ndGgg"
    "MTgwPj5zdHJlYW0KQlQgL0YxIDE0IFRmIDUwIDIyMCBUZCAoSGVsbG8gV29ybGQpIFRqIEVUCkJU"
    "IC9GMSAxMiBUZiA1MCAxODAgVGQgKFNlY29uZCBMaW5lIGhlcmUpIFRqIEVUCkJUIC9GMSAxMiBU"
    "ZiA1MCAxNDAgVGQgKFRoaXJkIHBhcmFncmFwaCB0ZXh0KSBUaiBFVAplbmRzdHJlYW0gZW5kb2Jq"
    "CjUgMCBvYmo8PC9UeXBlL0ZvbnQvU3VidHlwZS9UeXBlMS9CYXNlRm9udC9IZWx2ZXRpY2EvRW5j"
    "b2RpbmcvV2luQW5zaUVuY29kaW5nPj5lbmRvYmoKdHJhaWxlcjw8L1Jvb3QgMSAwIFIvU2l6ZSA2"
    "Pj4KJSVFT0Y="
)


@pytest.fixture()
def tp():
    doc = pdfspine.open(stream=base64.b64decode(_PDF_B64), filetype="pdf")
    return doc[0].get_textpage()


# === ground truth from real PyMuPDF 1.27 on the SAME bytes ===================

_GT_TEXT = "Hello World\nSecond Line here\nThird paragraph text\n"
_GT_SEARCH_HELLO = (50.0, 64.95, 81.89, 84.19)
_GT_SEARCH_LINE = (94.03, 107.1, 116.71, 123.59)
_GT_TEXTBOX_LINE2 = "Second Line here"
_GT_TEXTBOX_ALL = "Hello World\nSecond Line here\nThird paragraph text"
_GT_SEL_CROSS = "ello World\nSecond Lin"
_GT_SEL_MID = "ll"
_GT_SEL_ALL = "Hello World\nSecond Line here\nThird paragraph text"

# X within ~1px of fitz; Y looser (tighter pdfspine glyph box — see module docstring).
_X_TOL = 1.0
_Y_TOL = 5.0


def _close_x(a: float, b: float) -> bool:
    return abs(a - b) <= _X_TOL


def _close_y(a: float, b: float) -> bool:
    return abs(a - b) <= _Y_TOL


def _assert_rect_matches(got, gt) -> None:
    assert _close_x(got.x0, gt[0]), (got, gt)
    assert _close_y(got.y0, gt[1]), (got, gt)
    assert _close_x(got.x1, gt[2]), (got, gt)
    assert _close_y(got.y1, gt[3]), (got, gt)


# === search ==================================================================

def test_search_hello_rect(tp):
    hits = tp.search("Hello", quads=False)
    assert len(hits) == 1
    _assert_rect_matches(hits[0], _GT_SEARCH_HELLO)


def test_search_line_rect(tp):
    hits = tp.search("Line", quads=False)
    assert len(hits) == 1
    _assert_rect_matches(hits[0], _GT_SEARCH_LINE)


def test_search_default_is_rect(tp):
    # Batch-4 spec: default quads=False → list of Rect.
    hits = tp.search("Line")
    assert len(hits) == 1
    assert isinstance(hits[0], pdfspine.Rect)


def test_search_quads(tp):
    quads = tp.search("Line", quads=True)
    assert len(quads) == 1
    q = quads[0]
    assert isinstance(q, pdfspine.Quad)
    # The quad's enclosing rect equals the rect-mode hit.
    _assert_rect_matches(q.rect, _GT_SEARCH_LINE)
    # A horizontal hit: ul/ur share y, ul/ll share x.
    assert _close_y(q.ul.y, q.ur.y)
    assert _close_x(q.ul.x, q.ll.x)


def test_search_miss(tp):
    assert tp.search("nonexistent") == []


# === search across a line break (FIX: cross-line matching) ===================
#
# A separate two-line fixture ("apple banana apple" / "cherry apple grape" on a
# 400x200 page, Helvetica 12pt) — the SAME bytes the oracle reads. fitz reads
# the line break as one space, so the spaced needle "apple cherry" matches the
# tail "apple" of line 1 joined to "cherry" of line 2, returning ONE hit rect
# per line fragment. The X coordinates below are GROUND TRUTH from real PyMuPDF
# 1.27 (.venv-oracle); Y is the documented tighter-glyph-box deviation.
_XLINE_PDF_B64 = (
    "JVBERi0xLjcKMSAwIG9iajw8L1R5cGUvQ2F0YWxvZy9QYWdlcyAyIDAgUj4+ZW5kb2JqCjIgMCBv"
    "Ymo8PC9UeXBlL1BhZ2VzL0NvdW50IDEvS2lkc1szIDAgUl0+PmVuZG9iagozIDAgb2JqPDwvVHlw"
    "ZS9QYWdlL1BhcmVudCAyIDAgUi9NZWRpYUJveFswIDAgNDAwIDIwMF0vUmVzb3VyY2VzPDwvRm9u"
    "dDw8L0YxIDUgMCBSPj4+Pi9Db250ZW50cyA0IDAgUj4+ZW5kb2JqCjQgMCBvYmo8PC9MZW5ndGgg"
    "MTAwPj5zdHJlYW0KQlQgL0YxIDEyIFRmIDQ1IDE2MCBUZCAoYXBwbGUgYmFuYW5hIGFwcGxlKSBU"
    "aiBFVApCVCAvRjEgMTIgVGYgNDUgMTEwIFRkIChjaGVycnkgYXBwbGUgZ3JhcGUpIFRqIEVUCmVu"
    "ZHN0cmVhbSBlbmRvYmoKNSAwIG9iajw8L1R5cGUvRm9udC9TdWJ0eXBlL1R5cGUxL0Jhc2VGb250"
    "L0hlbHZldGljYS9FbmNvZGluZy9XaW5BbnNpRW5jb2Rpbmc+PmVuZG9iagp0cmFpbGVyPDwvUm9v"
    "dCAxIDAgUi9TaXplIDY+PgolJUVPRg=="
)

# oracle X-coords (real PyMuPDF 1.27): the tail "apple" on line 1, then "cherry"
# on line 2.
_GT_XLINE_FRAG1_X = (121.06, 150.41)
_GT_XLINE_FRAG2_X = (45.0, 78.34)


@pytest.fixture()
def xtp():
    doc = pdfspine.open(stream=base64.b64decode(_XLINE_PDF_B64), filetype="pdf")
    return doc[0].get_textpage()


def test_search_cross_line_apple_cherry(xtp):
    hits = xtp.search("apple cherry", quads=False)
    assert len(hits) == 2, hits
    # One rect per line fragment, in reading order; X matches fitz (~1px).
    assert _close_x(hits[0].x0, _GT_XLINE_FRAG1_X[0])
    assert _close_x(hits[0].x1, _GT_XLINE_FRAG1_X[1])
    assert _close_x(hits[1].x0, _GT_XLINE_FRAG2_X[0])
    assert _close_x(hits[1].x1, _GT_XLINE_FRAG2_X[1])
    # The first fragment sits above the second (reading order).
    assert hits[0].y0 < hits[1].y0


def test_search_cross_line_whitespace_collapses(xtp):
    # Multiple spaces / a newline in the needle all match the single line break.
    for needle in ("apple  cherry", "apple\ncherry", "apple\tcherry"):
        hits = xtp.search(needle, quads=False)
        assert len(hits) == 2, (needle, hits)
        assert _close_x(hits[0].x0, _GT_XLINE_FRAG1_X[0])
        assert _close_x(hits[1].x1, _GT_XLINE_FRAG2_X[1])


def test_search_no_space_does_not_bridge_break(xtp):
    # A needle without whitespace cannot bridge the line break (matches fitz:
    # "applecherry" finds nothing across the break).
    assert xtp.search("applecherry", quads=False) == []


# === extractTextbox =========================================================

def test_textbox_single_line(tp):
    assert tp.extractTextbox((40, 108, 400, 124)) == _GT_TEXTBOX_LINE2


def test_textbox_all(tp):
    assert tp.extractTextbox((0, 0, 400, 300)) == _GT_TEXTBOX_ALL


def test_textbox_accepts_rect(tp):
    assert tp.extractTextbox(pdfspine.Rect(40, 108, 400, 124)) == _GT_TEXTBOX_LINE2


def test_textbox_empty_region(tp):
    # A clip below all text → empty string.
    assert tp.extractTextbox((0, 250, 400, 300)) == ""


# === extractSelection =======================================================

def test_selection_cross_lines(tp):
    assert tp.extractSelection((60, 80), (110, 120)) == _GT_SEL_CROSS


def test_selection_mid_word(tp):
    assert tp.extractSelection((65, 80), (75, 80)) == _GT_SEL_MID


def test_selection_whole_page(tp):
    assert tp.extractSelection((0, 0), (400, 300)) == _GT_SEL_ALL


def test_selection_accepts_points(tp):
    a = pdfspine.Point(65, 80)
    b = pdfspine.Point(75, 80)
    assert tp.extractSelection(a, b) == _GT_SEL_MID


# === extractHTML / extractXHTML / extractXML ================================

def test_html_structure(tp):
    html = tp.extractHTML()
    # fitz-shaped: a single page div with per-line <p> of styled <span>s.
    assert '<div id="page0"' in html
    assert "width:400pt;height:300pt" in html
    assert html.count("<p ") == 3
    assert "Hello World" in html
    assert "Second Line here" in html
    assert "Third paragraph text" in html
    # Each <p> carries fitz's top/left/line-height keys.
    assert "top:68.8pt;left:50pt;line-height:14pt" in html
    # Span style carries font-family + size + color.
    assert "font-family:Helvetica;font-size:14pt;color:#000000" in html


def test_xhtml_is_well_formed_and_has_text(tp):
    xhtml = tp.extractXHTML()
    assert '<div id="page0">' in xhtml
    assert xhtml.count("<p>") == 3
    assert "Hello World" in xhtml
    # Parseable as XML (well-formed).
    minidom.parseString(xhtml)


def test_xml_structure_and_chars(tp):
    xml = tp.extractXML()
    # Valid XML.
    dom = minidom.parseString(xml)
    page = dom.documentElement
    assert page.tagName == "page"
    assert page.getAttribute("id") == "page0"
    assert page.getAttribute("width") == "400"
    blocks = page.getElementsByTagName("block")
    assert len(blocks) == 3
    lines = page.getElementsByTagName("line")
    assert len(lines) == 3
    # The first line carries fitz's text= attribute + flags.
    assert lines[0].getAttribute("text") == "Hello World"
    assert lines[0].getAttribute("wmode") == "0"
    # Char nodes carry quad / x / y / color / c, fitz-shaped.
    chars = page.getElementsByTagName("char")
    assert len(chars) == len("Hello World" + "Second Line here" + "Third paragraph text")
    first = chars[0]
    assert first.getAttribute("c") == "H"
    assert first.getAttribute("color") == "#000000"
    assert first.getAttribute("alpha") == "#ff"
    assert len(first.getAttribute("quad").split()) == 8


def test_xml_escapes_special_chars():
    # A literal '<' in the text must be escaped in attribute + char data.
    pdf = (
        b"%PDF-1.7\n"
        b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
        b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n"
        b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]"
        b"/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>endobj\n"
        b"4 0 obj<</Length 40>>stream\n"
        b"BT /F1 12 Tf 50 100 Td (a<b) Tj ET\n"
        b"endstream endobj\n"
        b"5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n"
        b"trailer<</Root 1 0 R/Size 6>>\n%%EOF"
    )
    doc = pdfspine.open(stream=pdf, filetype="pdf")
    xml = doc[0].get_textpage().extractXML()
    # Well-formed despite the literal '<' in content.
    minidom.parseString(xml)
    assert 'c="&lt;"' in xml
    assert 'text="a&lt;b"' in xml


# === extractIMGINFO =========================================================

def test_imginfo_no_images(tp):
    # The text-only fixture has no images → empty list (matches fitz).
    assert tp.extractIMGINFO() == []


def test_imginfo_with_image():
    # An inline-referenced image XObject placed via a CTM.
    pdf = (
        b"%PDF-1.7\n"
        b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
        b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n"
        b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]"
        b"/Resources<</XObject<</Im0 5 0 R>>>>/Contents 4 0 R>>endobj\n"
        b"4 0 obj<</Length 40>>stream\n"
        b"q 60 0 0 50 100 100 cm /Im0 Do Q\n"
        b"endstream endobj\n"
        b"5 0 obj<</Type/XObject/Subtype/Image/Width 20/Height 20"
        b"/ColorSpace/DeviceRGB/BitsPerComponent 8/Filter/DCTDecode/Length 4>>stream\n"
        b"\xff\xd8\xff\xd9\n"
        b"endstream endobj\n"
        b"trailer<</Root 1 0 R/Size 6>>\n%%EOF"
    )
    doc = pdfspine.open(stream=pdf, filetype="pdf")
    info = doc[0].get_textpage().extractIMGINFO()
    assert len(info) == 1
    d = info[0]
    # fitz key set.
    assert set(d) >= {
        "number", "bbox", "transform", "width", "height",
        "colorspace", "cs-name", "xres", "yres", "bpc", "size", "has-mask",
    }
    assert d["width"] == 20
    assert d["height"] == 20
    assert d["bpc"] == 8
    assert d["colorspace"] == 3  # DeviceRGB → 3 components
    assert d["has-mask"] is False
    # Placement bbox from the 60x50 CTM at (100, 100). Y is top-origin device.
    x0, y0, x1, y1 = d["bbox"]
    assert _close_x(x0, 100.0)
    assert _close_x(x1, 160.0)
    assert abs((x1 - x0) - 60.0) <= _X_TOL
    assert abs((y1 - y0) - 50.0) <= _Y_TOL


# === poolsize ===============================================================

def test_poolsize_positive_and_scales(tp):
    # A populated page → positive footprint.
    size = tp.poolsize()
    assert isinstance(size, int)
    assert size > 0
    # An empty page has a strictly smaller footprint.
    empty_pdf = (
        b"%PDF-1.7\n"
        b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
        b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n"
        b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 400 300]>>endobj\n"
        b"trailer<</Root 1 0 R/Size 4>>\n%%EOF"
    )
    empty = pdfspine.open(stream=empty_pdf, filetype="pdf")[0].get_textpage()
    assert empty.poolsize() < size


# === sanity: the model the serializers build on ============================

def test_text_matches_oracle(tp):
    assert tp.extractText() == _GT_TEXT


# === Font members (PRD §C batch-4) ========================================
#
# These exercise the ``fitz.Font`` surface on the Core-14 metrics handle:
# ``Base14_fontnames``, ``is_writable`` (match fitz exactly) and
# ``valid_codepoints`` (honest encoding-derived subset). ``buffer`` and
# ``glyph_bbox`` are DEFERRED. Ground-truth facts below were captured from REAL
# PyMuPDF 1.27 (``.venv-oracle``); the in-repo ``fitz`` is the pdfspine shim.
#
# DEVIATIONS / DEFERRALS from fitz (documented, inherent to a metrics-only
# Core-14 handle — built from a font NAME, with no embedded /FontFile* program
# and no per-glyph outlines):
#   - ``buffer``: DEFERRED → raises ``PdfUnsupportedError``. PyMuPDF substitutes
#     a bundled NimbusSans/Type1 TTF and returns its bytes; the pdfspine handle has
#     no program to expose, and empty bytes would be a misleading
#     non-implementation, so it honestly raises.
#   - ``glyph_bbox``: DEFERRED → raises ``PdfUnsupportedError``. The handle has
#     no per-glyph outlines; returning the same font-level bbox for every glyph
#     would be a misleading constant (a wrong per-glyph value is worse than an
#     honest error), so it raises until per-glyph outlines are available.
#   - ``valid_codepoints``: KEPT. PyMuPDF reads the bundled font's full cmap (653
#     for the text fonts, 195/203 for Symbol/ZapfDingbats); the pdfspine handle
#     reports the codepoints of its built-in PDF encoding (WinAnsi for text
#     fonts; the font's own encoding for the two pictographic families). The
#     pdfspine set is a STRICT SUBSET of fitz's (verified below): every codepoint it
#     reports is genuinely covered by fitz — no false positives. For an embedded
#     font the real cmap would be preferred.

# The 14 standard PDF base-font names, captured verbatim from real PyMuPDF
# (``fitz.Base14_fontnames``).
_GT_BASE14 = (
    "Courier",
    "Courier-Oblique",
    "Courier-Bold",
    "Courier-BoldOblique",
    "Helvetica",
    "Helvetica-Oblique",
    "Helvetica-Bold",
    "Helvetica-BoldOblique",
    "Times-Roman",
    "Times-Italic",
    "Times-Bold",
    "Times-BoldItalic",
    "Symbol",
    "ZapfDingbats",
)

# Per-font ``len(valid_codepoints())`` from REAL PyMuPDF 1.27 (the bundled-cmap
# count) and the pdfspine encoding-derived count (a documented subset).
_GT_FITZ_VC_LEN = {"helv": 653, "tiro": 653, "cour": 653, "symb": 195, "zadb": 203}
_OXIDE_VC_LEN = {"helv": 216, "tiro": 216, "cour": 216, "symb": 98, "zadb": 95}


def test_font_base14_fontnames_matches_fitz():
    # Module-level constant, exact tuple, in PyMuPDF's order.
    assert pdfspine.Base14_fontnames == _GT_BASE14
    # Reachable through the fitz shim too.
    assert tuple(fitz.Base14_fontnames) == _GT_BASE14
    assert len(pdfspine.Base14_fontnames) == 14


def test_font_is_writable_matches_fitz():
    # Real PyMuPDF reports True for every standard font; so does pdfspine.
    for nm in ("helv", "tiro", "cour", "symb", "zadb"):
        assert pdfspine.Font(nm).is_writable is True


def test_font_buffer_deferred():
    # DEFERRED: the metrics-only Core-14 handle carries no embedded /FontFile*
    # program, so buffer honestly raises rather than returning misleading bytes.
    f = pdfspine.Font("helv")
    with pytest.raises(pdfspine.PdfUnsupportedError):
        _ = f.buffer


def test_font_valid_codepoints_type_and_contents():
    f = pdfspine.Font("helv")
    vc = f.valid_codepoints()
    assert isinstance(vc, list)
    assert all(isinstance(c, int) for c in vc)
    # Sorted ascending and de-duplicated (matches fitz's ordering).
    assert vc == sorted(set(vc))
    # The WinAnsi printable run is covered.
    assert ord(" ") in vc
    assert ord("A") in vc
    assert ord("z") in vc
    assert 0x00E9 in vc  # é (eacute)


def test_font_valid_codepoints_len():
    for nm, n in _OXIDE_VC_LEN.items():
        assert len(pdfspine.Font(nm).valid_codepoints()) == n


def test_font_valid_codepoints_subset_of_fitz():
    # Cross-check vs REAL PyMuPDF (.venv-oracle) embedded as literals: pdfspine's
    # set must be a strict subset of fitz's (no false positives), and smaller.
    import subprocess
    import sys
    import os

    oracle = os.path.join(
        os.path.dirname(__file__), "..", "..", ".venv-oracle", "bin", "python"
    )
    if not os.path.exists(oracle):
        pytest.skip("real-PyMuPDF oracle not available")
    code = (
        "import fitz, json;"
        "print(json.dumps({n: sorted(fitz.Font(n).valid_codepoints()) "
        "for n in ['helv','tiro','cour','symb','zadb']}))"
    )
    out = subprocess.run(
        [oracle, "-c", code], capture_output=True, text=True, check=True
    ).stdout
    import json

    fitz_vc = json.loads(out)
    for nm in ("helv", "tiro", "cour", "symb", "zadb"):
        ovc = set(pdfspine.Font(nm).valid_codepoints())
        fvc = set(fitz_vc[nm])
        assert len(fvc) == _GT_FITZ_VC_LEN[nm]
        assert ovc.issubset(fvc), f"{nm}: pdfspine reports codepoints fitz lacks"
        assert len(ovc) < len(fvc)


def test_font_glyph_bbox_deferred():
    # DEFERRED: the metrics-only Core-14 handle has no per-glyph outlines, so it
    # cannot report each glyph's individual ink box. Returning the same
    # font-level bbox for every glyph would be a misleading constant, so
    # glyph_bbox honestly raises (a wrong per-glyph value is worse than an
    # honest PdfUnsupportedError).
    f = pdfspine.Font("helv")
    for cp in (ord("A"), ord("g"), ord("i"), ord("z"), 0x1F600):
        with pytest.raises(pdfspine.PdfUnsupportedError):
            f.glyph_bbox(cp)


def test_font_glyph_bbox_deferred_via_fitz_shim():
    # The deferral is consistent through the fitz shim.
    f = fitz.Font("helv")
    with pytest.raises(pdfspine.PdfUnsupportedError):
        f.glyph_bbox(ord("A"))
