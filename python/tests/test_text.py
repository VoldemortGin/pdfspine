"""M2e Python text-surface tests (PRD §8.6 / §9.4 / §9.5 / §12).

``PYTEXT-*`` / ``PYSEARCH-*`` / ``PYINV-*`` exercise the native ``pdfspine`` text
extraction, search and inventory surface; ``PYFITZ-TEXT-*`` the ``fitz`` shim;
``ACCURACY-GT-*`` the M2 accuracy exit gate (normalized-Levenshtein similarity of
``get_text("text")`` against a known ground truth).

All fixtures are self-generated in-test (raw PDF bytes via ``stream=``) — no
external/PyMuPDF files (PRD §10). Fonts that we assert geometry on carry an
explicit ``/Widths`` array (Core-14 AFM widths are not embedded yet, so a
width-less font collapses glyph boxes to zero width at the correct x).
"""

from __future__ import annotations

import json

import pdfspine
import pytest


# --- self-generated PDF assembler (classic xref) --------------------------
# Copied from test_document.py so this file is fully self-contained.


def _build_pdf(objects: list[tuple[int, bytes]], root: int, extra_trailer: bytes = b"") -> bytes:
    """Assembles a classic-xref PDF from ``(num, body)`` object pairs."""
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    max_num = 0
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
        max_num = max(max_num, num)

    size = max_num + 1
    startxref = len(out)
    out += b"xref\n"
    out += f"0 {size}\n".encode()
    out += b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n"
    out += f"<< /Size {size} /Root {root} 0 R {extra_trailer.decode()} >>\n".encode()
    out += b"startxref\n"
    out += f"{startxref}\n".encode()
    out += b"%%EOF\n"
    return bytes(out)


# --- small fixture builders ------------------------------------------------


def _escape(s: str) -> bytes:
    """Escapes ``(``/``)``/``\\`` for a PDF literal string (Latin-1 bytes)."""
    b = s.encode("latin-1")
    return b.replace(b"\\", b"\\\\").replace(b"(", b"\\(").replace(b")", b"\\)")


def _helvetica_font(first: int = 32, last: int = 125, width: int = 500) -> bytes:
    """A Type1 Helvetica/WinAnsi font with an explicit equal-width /Widths."""
    n = last - first + 1
    widths = b"[" + b" ".join(str(width).encode() for _ in range(n)) + b"]"
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding "
        + f"/FirstChar {first} /LastChar {last} ".encode()
        + b"/Widths " + widths + b" >>"
    )


def text_pdf(lines: list[str], font_widths: bool = True, ystart: int = 700, leading: int = 20) -> bytes:
    """A 1-page PDF (MediaBox [0 0 612 792]) drawing ``lines`` with /F1.

    ``BT /F1 12 Tf 72 <ystart> Td (line0) Tj 0 -<leading> Td (line1) Tj ... ET``.
    When ``font_widths`` the font carries explicit width-500 glyphs (needed for
    non-zero word/char bboxes and search rects).
    """
    parts = [f"BT /F1 12 Tf 72 {ystart} Td".encode()]
    for i, line in enumerate(lines):
        if i:
            parts.append(f"0 -{leading} Td".encode())
        parts.append(b"(" + _escape(line) + b") Tj")
    parts.append(b"ET")
    content = b" ".join(parts)

    if font_widths:
        font = _helvetica_font()
    else:
        font = (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        )
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream"),
            (5, font),
        ],
        root=1,
    )


def _raw_content_pdf(content: bytes, font: bytes) -> bytes:
    """A 1-page PDF whose content stream is supplied verbatim (raw bytes)."""
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream"),
            (5, font),
        ],
        root=1,
    )


def winansi_specials_pdf() -> tuple[bytes, str]:
    """A page with WinAnsi high-range bytes; returns (pdf, ground_truth).

    WinAnsi maps: 0xE9->'é', 0xFC->'ü', 0xF1->'ñ', 0xA9->'©'. These all
    round-trip WinAnsi->Unicode, so the extracted line is ``"Café über ñ ©"``.
    """
    lit = b"Caf\xe9 \xfcber \xf1 \xa9"  # Café über ñ ©
    content = b"BT /F1 12 Tf 72 700 Td (" + lit + b") Tj ET"
    font = _helvetica_font(first=32, last=255)  # cover the high-range codes
    return _raw_content_pdf(content, font), "Café über ñ ©"


def cid_identity_h_pdf() -> tuple[bytes, str]:
    """A Type0/Identity-H CID font with a /ToUnicode CMap; returns (pdf, gt).

    Content uses 2-byte Identity-H codes ``<00010002000300030004>``; the
    ToUnicode CMap maps 0x0001..0x0004 -> H E L L O so the extracted text is
    the ASCII string ``"HELLO"``.
    """
    content = b"BT /F1 24 Tf 72 700 Td <00010002000300030004> Tj ET"
    tounicode = (
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
        b"/CMapName /Adobe-Identity-UCS def\n"
        b"/CMapType 2 def\n"
        b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
        b"4 beginbfchar\n"
        b"<0001> <0048>\n"  # H
        b"<0002> <0045>\n"  # E
        b"<0003> <004C>\n"  # L
        b"<0004> <004F>\n"  # O
        b"endbfchar\n"
        b"endcmap CMapName currentdict /CMap defineresource pop end end\n"
    )
    objs = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        ),
        (4, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream"),
        (
            5,
            b"<< /Type /Font /Subtype /Type0 /BaseFont /F0 /Encoding /Identity-H "
            b"/DescendantFonts [6 0 R] /ToUnicode 7 0 R >>",
        ),
        (
            6,
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /F0 "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor 8 0 R /CIDToGIDMap /Identity /DW 1000 >>",
        ),
        (7, b"<< /Length " + str(len(tounicode)).encode() + b" >>\nstream\n" + tounicode + b"\nendstream"),
        (8, b"<< /Type /FontDescriptor /FontName /F0 /Flags 4 /Ascent 800 /Descent -200 >>"),
    ]
    return _build_pdf(objs, root=1), "HELLO"


def image_pdf() -> bytes:
    """A 1-page PDF with a single 1x1 DeviceRGB image XObject painted as /Im0."""
    import zlib

    pix = zlib.compress(b"\x00\x00\x00")  # 1x1 RGB; pixel content irrelevant (M5)
    content = b"q 1 0 0 1 100 100 cm /Im0 Do Q"
    objs = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        (4, b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream"),
        (
            5,
            b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 "
            b"/BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode "
            b"/Length " + str(len(pix)).encode() + b" >>\nstream\n" + pix + b"\nendstream",
        ),
    ]
    return _build_pdf(objs, root=1)


# --- Levenshtein similarity helper ----------------------------------------


def _levenshtein(a: str, b: str) -> int:
    """Standard edit-distance DP."""
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cost = 0 if ca == cb else 1
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost))
        prev = cur
    return prev[-1]


def _similarity(a: str, b: str) -> float:
    if not a and not b:
        return 1.0
    d = _levenshtein(a, b)
    return 1.0 - d / max(len(a), len(b))


def _normalize(s: str) -> str:
    """Collapses whitespace runs (incl. block/line separators) to single
    spaces and strips ends, so layout separators don't unfairly tank the
    similarity score — without dropping any real glyph content."""
    return " ".join(s.split())


# ==========================================================================
# PYTEXT-* — native text extraction
# ==========================================================================


def _page(pdf: bytes) -> "pdfspine.Page":
    return pdfspine.open(stream=pdf)[0]


def test_pytext_001_text_content():
    # PYTEXT-001: get_text("text") returns the known text content.
    page = _page(text_pdf(["Hello World", "Second Line"]))
    assert page.get_text("text") == "Hello World\nSecond Line\n"


def test_pytext_002_words_arity8_with_bbox():
    # PYTEXT-002: get_text("words") → arity-8 tuples; a known word has bbox width > 0.
    page = _page(text_pdf(["Hello World"]))
    words = page.get_text("words")
    assert words and all(len(w) == 8 for w in words)
    by_text = {w[4]: w for w in words}
    assert "World" in by_text
    x0, y0, x1, y1, word, block_no, line_no, word_no = by_text["World"]
    assert word == "World"
    assert (x1 - x0) > 0  # explicit /Widths → non-zero glyph box
    assert (y1 - y0) > 0
    assert isinstance(block_no, int) and isinstance(line_no, int) and isinstance(word_no, int)


def test_pytext_003_dict_keys_and_types():
    # PYTEXT-003: get_text("dict") key set + types.
    page = _page(text_pdf(["Hello World"]))
    d = page.get_text("dict")
    assert set(d.keys()) == {"width", "height", "blocks"}
    assert isinstance(d["width"], float) and isinstance(d["height"], float)

    block = d["blocks"][0]
    assert set(block.keys()) == {"number", "type", "bbox", "lines"}
    assert block["type"] == 0
    assert isinstance(block["bbox"], tuple) and len(block["bbox"]) == 4

    line = block["lines"][0]
    assert set(line.keys()) == {"spans", "wmode", "dir", "bbox"}
    assert isinstance(line["bbox"], tuple)

    span = line["spans"][0]
    assert set(span.keys()) == {
        "size", "flags", "font", "color", "ascender",
        "descender", "origin", "bbox", "text",
    }
    assert isinstance(span["color"], int)
    assert isinstance(span["bbox"], tuple) and len(span["bbox"]) == 4
    assert isinstance(span["origin"], tuple) and len(span["origin"]) == 2
    assert "Hello" in span["text"]


def test_pytext_004_blocks_arity7():
    # PYTEXT-004: get_text("blocks") → arity-7 tuples; type==0 for text blocks.
    page = _page(text_pdf(["Hello World", "Second Line"]))
    blocks = page.get_text("blocks")
    assert blocks and all(len(b) == 7 for b in blocks)
    for x0, y0, x1, y1, text, block_no, btype in blocks:
        assert btype == 0  # text block
        assert isinstance(text, str)
    joined = "".join(b[4] for b in blocks)
    assert "Hello World" in joined and "Second Line" in joined


def test_pytext_005_json_parses():
    # PYTEXT-005: get_text("json") → json.loads parses to a dict with 'blocks'.
    page = _page(text_pdf(["Hello World"]))
    s = page.get_text("json")
    assert isinstance(s, str)
    parsed = json.loads(s)
    assert isinstance(parsed, dict)
    assert "blocks" in parsed


def test_pytext_006_rawdict_chars():
    # PYTEXT-006: get_text("rawdict") span carries 'chars' (origin/bbox/c), no 'text'.
    page = _page(text_pdf(["Hi"]))
    d = page.get_text("rawdict")
    span = d["blocks"][0]["lines"][0]["spans"][0]
    assert "chars" in span
    assert "text" not in span
    char = span["chars"][0]
    assert set(char.keys()) == {"origin", "bbox", "c"}
    assert isinstance(char["c"], str)
    assert isinstance(char["bbox"], tuple) and len(char["bbox"]) == 4
    assert isinstance(char["origin"], tuple) and len(char["origin"]) == 2


def test_pytext_007_markup_formats_are_str():
    # PYTEXT-007: html/xhtml/xml + json/rawjson all return non-empty str.
    page = _page(text_pdf(["Hello World"]))
    for opt in ("html", "xhtml", "xml", "json", "rawjson"):
        v = page.get_text(opt)
        assert isinstance(v, str), opt
        assert v, opt
    # json/rawjson must be valid JSON strings.
    assert isinstance(json.loads(page.get_text("json")), dict)
    assert isinstance(json.loads(page.get_text("rawjson")), dict)


def test_pytext_008_textpage_reuse():
    # PYTEXT-008: get_textpage() handle; extractText() == get_text("text");
    # reuse via textpage= gives identical text; search via textpage= works.
    page = _page(text_pdf(["Hello World"]))
    tp = page.get_textpage()
    expected = page.get_text("text")
    assert tp.extractText() == expected
    assert page.get_text("text", textpage=tp) == expected
    hits = page.search_for("World", textpage=tp)
    fresh = page.search_for("World")
    assert [tuple(h) for h in hits] == [tuple(h) for h in fresh]
    assert len(hits) == 1


def test_pytext_009_sort_orders_blocks_by_y():
    # PYTEXT-009: sort=True orders blocks by (y, x). Two vertically-separated
    # blocks emitted bottom-first in the content stream; sorted blocks' y0 are
    # non-decreasing and the content is preserved.
    content = (
        b"BT /F1 12 Tf 72 100 Td (Bottom block) Tj ET "
        b"BT /F1 12 Tf 72 700 Td (Top block) Tj ET"
    )
    page = _page(_raw_content_pdf(content, _helvetica_font()))
    blocks = page.get_text("blocks", sort=True)
    assert len(blocks) >= 2
    y0s = [b[1] for b in blocks]
    assert y0s == sorted(y0s)  # non-decreasing in device y (y-down)
    joined = "".join(b[4] for b in blocks)
    assert "Top block" in joined and "Bottom block" in joined


# ==========================================================================
# PYSEARCH-* — search_for
# ==========================================================================


def test_pysearch_001_rect_overlaps_location():
    # PYSEARCH-001: search_for returns a Rect overlapping the known location.
    page = _page(text_pdf(["Hello World"]))
    rects = page.search_for("World")
    assert len(rects) == 1
    r = rects[0]
    assert r.width > 0 and r.height > 0
    # "World" is the 2nd word: "Hello " = 6 chars * 12pt * 0.5 width = 36 → x≈108.
    assert abs(r.x0 - 108.0) < 1.0
    # The hit rect overlaps the line region (the word's own bbox from words()).
    wb = {w[4]: w for w in page.get_text("words")}["World"]
    assert r.x0 < wb[2] and r.x1 > wb[0]  # x ranges overlap
    assert r.y0 < wb[3] and r.y1 > wb[1]  # y ranges overlap


def test_pysearch_002_quads():
    # PYSEARCH-002: quads=True returns Quad objects at the same location.
    page = _page(text_pdf(["Hello World"]))
    quads = page.search_for("World", quads=True)
    assert len(quads) == 1
    q = quads[0]
    assert hasattr(q, "ul") and hasattr(q, "ur") and hasattr(q, "ll") and hasattr(q, "lr")
    assert hasattr(q.ul, "x") and hasattr(q.ul, "y")
    # The quad's enclosing rect matches the default (rect) search.
    r = page.search_for("World")[0]
    assert abs(q.rect.x0 - r.x0) < 1e-6
    assert abs(q.rect.x1 - r.x1) < 1e-6


def test_pysearch_003_hit_max_and_not_found():
    # PYSEARCH-003: hit_max caps results; not-found needle → [].
    content = (
        b"BT /F1 12 Tf 72 700 Td (cat one) Tj "
        b"0 -20 Td (cat two) Tj "
        b"0 -20 Td (cat three) Tj ET"
    )
    page = _page(_raw_content_pdf(content, _helvetica_font()))
    assert len(page.search_for("cat")) == 3  # all three single-line hits
    assert len(page.search_for("cat", hit_max=2)) <= 2
    assert page.search_for("nonexistent") == []


# ==========================================================================
# PYINV-* — get_fonts / get_images
# ==========================================================================


def test_pyinv_001_get_fonts():
    # PYINV-001: get_fonts returns the expected Helvetica 7-tuple.
    page = _page(text_pdf(["Hello"]))
    fonts = page.get_fonts()
    assert len(fonts) == 1
    f = fonts[0]
    assert len(f) == 7
    xref, ext, ftype, basefont, name, encoding, referencer = f
    assert (xref, ext, ftype, basefont, name, encoding, referencer) == (
        5, "n/a", "Type1", "Helvetica", "F1", "WinAnsiEncoding", 3,
    )


def test_pyinv_002_get_images():
    # PYINV-002: get_images returns the expected 10-tuple; empty page → [].
    page = _page(image_pdf())
    images = page.get_images()
    assert len(images) == 1
    img = images[0]
    assert len(img) == 10
    xref, smask, width, height, bpc, cs, alt_cs, name, filt, referencer = img
    assert width == 1
    assert height == 1
    assert bpc == 8
    assert cs == "DeviceRGB"
    assert name == "Im0"

    # A page without images → [].
    no_img = _page(text_pdf(["Hello"]))
    assert no_img.get_images() == []


# ==========================================================================
# PYFITZ-TEXT-* — fitz shim parity
# ==========================================================================


def test_pyfitz_text_001_dict_parity():
    # PYFITZ-TEXT-001: fitz dict has the same key set as the native dict.
    import fitz

    pdf = text_pdf(["Hello World"])
    native = pdfspine.open(stream=pdf)[0].get_text("dict")
    shimmed = fitz.open(stream=pdf).load_page(0).get_text("dict")
    assert set(shimmed.keys()) == set(native.keys())
    assert set(shimmed["blocks"][0].keys()) == set(native["blocks"][0].keys())
    nspan = native["blocks"][0]["lines"][0]["spans"][0]
    sspan = shimmed["blocks"][0]["lines"][0]["spans"][0]
    assert set(sspan.keys()) == set(nspan.keys())


def test_pyfitz_text_002_search_value_types():
    # PYFITZ-TEXT-002: fitz search returns fitz.Rect / fitz.Quad value types.
    import fitz

    page = fitz.open(stream=text_pdf(["Hello World"])).load_page(0)
    rects = page.search_for("World")
    assert rects and all(isinstance(r, fitz.Rect) for r in rects)
    quads = page.search_for("World", quads=True)
    assert quads and all(isinstance(q, fitz.Quad) for q in quads)


# ==========================================================================
# ACCURACY-GT-* — M2 accuracy exit gate
# ==========================================================================


def test_accuracy_gt_001_ascii(capsys):
    # ACCURACY-GT-001: ASCII multi-line PDF → similarity ≥ 0.98.
    lines = [
        "The quick brown fox",
        "jumps over the lazy dog",
        "while pangrams pack letters",
        "into short sentences cleanly",
    ]
    ground_truth = _normalize("\n".join(lines))
    extracted = _normalize(_page(text_pdf(lines)).get_text("text"))
    sim = _similarity(extracted, ground_truth)
    with capsys.disabled():
        print(f"\nACCURACY-GT-001 similarity={sim:.4f}")
    assert sim >= 0.98, (extracted, ground_truth, sim)


def test_accuracy_gt_002_winansi(capsys):
    # ACCURACY-GT-002: WinAnsi specials PDF → similarity ≥ 0.98.
    pdf, gt = winansi_specials_pdf()
    ground_truth = _normalize(gt)
    extracted = _normalize(_page(pdf).get_text("text"))
    sim = _similarity(extracted, ground_truth)
    with capsys.disabled():
        print(f"ACCURACY-GT-002 similarity={sim:.4f} extracted={extracted!r}")
    assert sim >= 0.98, (extracted, ground_truth, sim)


def test_accuracy_gt_003_cid_identity_h(capsys):
    # ACCURACY-GT-003: Type0/Identity-H CID + ToUnicode PDF → similarity ≥ 0.95.
    pdf, gt = cid_identity_h_pdf()
    ground_truth = _normalize(gt)
    extracted = _normalize(_page(pdf).get_text("text"))
    sim = _similarity(extracted, ground_truth)
    with capsys.disabled():
        print(f"ACCURACY-GT-003 similarity={sim:.4f} extracted={extracted!r} gt={ground_truth!r}")
    assert sim >= 0.95, (extracted, ground_truth, sim)
