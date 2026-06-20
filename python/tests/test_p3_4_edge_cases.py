"""P3-4 edge-case extraction tests (PRD-NEXT §4 Phase 3).

Locks down three existing-but-never-exercised text-extraction paths against the
behavior of real PyMuPDF 1.24.14 (cross-checked once via ``.venv-oracle``; the
expected values are inlined so the tests stay self-contained — no oracle/PyMuPDF
file dependency, per PRD §10):

* ``VERTICAL-*`` — vertical / ``Identity-V`` CJK writing mode.
* ``NOTOU-*`` — a Type0/CID font with **no** ``/ToUnicode`` (the predefined-CMap
  ``cid_to_unicode`` fallback).
* ``OVERLAP-*`` — overlapping / co-located (double-strike) text.

All fixtures are self-generated raw PDF bytes (the ``_build_pdf`` assembler is
copied from ``test_text.py`` so this file is fully standalone).
"""

from __future__ import annotations

from collections import Counter

import pdfspine
import pytest


# --- self-generated PDF assembler (classic xref) --------------------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int = 1) -> bytes:
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
    out += b"xref\n" + f"0 {size}\n".encode() + b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n" + f"<< /Size {size} /Root {root} 0 R >>\n".encode()
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return bytes(out)


def _stream_obj(num: int, body: bytes) -> tuple[int, bytes]:
    """A simple ``/Length``-prefixed stream object."""
    return (num, b"<< /Length " + str(len(body)).encode() + b" >>\nstream\n" + body + b"\nendstream")


def _equal_width_type1(width: int = 500) -> bytes:
    """A WinAnsi Helvetica with an explicit equal-width /Widths (32..125)."""
    n = 125 - 32 + 1
    widths = b"[" + b" ".join(str(width).encode() for _ in range(n)) + b"]"
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding "
        b"/FirstChar 32 /LastChar 125 /Widths " + widths + b" >>"
    )


def _page(pdf: bytes) -> "pdfspine.Page":
    return pdfspine.open(stream=pdf, filetype="pdf").load_page(0)


# =====================================================================
# VERTICAL-* — vertical / Identity-V CJK writing mode
# =====================================================================
#
# Real PyMuPDF on a single-column Identity-V CJK run extracts the characters in
# logical order ("中文字"). pdfspine matches that text+order for a single column
# (its writing-mode geometry is horizontal — wmode/dir/bbox diverge — but the
# *characters and their order* are correct, which is what get_text() promises).
#
# KNOWN LIMITATION (flagged, not fixed — a medium change, out of scope for this
# cheap-insurance task): vertical writing is not implemented in the glyph
# emission path (crates/pdf-text/src/interp.rs always sets
# WritingDir::Horizontal and advances along +x). For a *single* vertical column
# the extracted text+order still matches PyMuPDF; for *multiple* vertical
# columns the reading order diverges (PyMuPDF reads columns right-to-left,
# pdfspine treats them as left-to-right horizontal runs). See
# test_vertical_002_multicolumn_known_divergence.


def _vertical_cjk_pdf(content: bytes) -> bytes:
    """A 1-page Identity-V Type0 CJK PDF; ToUnicode maps 1..4 -> 中文字書."""
    tounicode = (
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
        b"/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
        b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
        b"4 beginbfchar\n"
        b"<0001> <4E2D>\n"  # 中
        b"<0002> <6587>\n"  # 文
        b"<0003> <5B57>\n"  # 字
        b"<0004> <66F8>\n"  # 書
        b"endbfchar\n"
        b"endcmap CMapName currentdict /CMap defineresource pop end end\n"
    )
    objs = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        _stream_obj(4, content),
        (
            5,
            b"<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light /Encoding /Identity-V "
            b"/DescendantFonts [6 0 R] /ToUnicode 7 0 R >>",
        ),
        (
            6,
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /STSong-Light "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor 8 0 R /CIDToGIDMap /Identity /DW 1000 /DW2 [880 -1000] >>",
        ),
        _stream_obj(7, tounicode),
        (8, b"<< /Type /FontDescriptor /FontName /STSong-Light /Flags 4 /Ascent 880 /Descent -120 >>"),
    ]
    return _build_pdf(objs)


def test_vertical_001_single_column_text_and_order():
    # One vertical column of three glyphs (codes 1,2,3 stacked downward).
    content = b"BT /F0 24 Tf 300 700 Td <000100020003> Tj ET"
    text = _page(_vertical_cjk_pdf(content)).get_text().strip()
    # PyMuPDF 1.24.14: "中文字" — all three chars in logical order.
    assert text == "中文字"


def test_vertical_002_multicolumn_known_divergence():
    # Right column (x=400) 中文, left column (x=300) 字書. PyMuPDF reads vertical
    # columns right-to-left -> "中文\n字書"; pdfspine has no vertical writing mode
    # so it orders the two runs left-to-right as horizontal text. The set of
    # characters is identical (nothing dropped/duplicated); only column order
    # differs. This asserts the *current* (flagged-limitation) behavior so a
    # future vertical-writing implementation has a tripwire.
    content = (
        b"BT /F0 24 Tf 400 700 Td <00010002> Tj ET "
        b"BT /F0 24 Tf 300 700 Td <00030004> Tj ET"
    )
    text = _page(_vertical_cjk_pdf(content)).get_text()
    # No glyph dropped or duplicated vs PyMuPDF (same character multiset).
    assert Counter(text.replace("\n", "").replace(" ", "")) == Counter("中文字書")
    # Current pdfspine order (horizontal L->R columns). PyMuPDF would give
    # "中文\n字書" (R->L vertical columns) — the documented divergence.
    assert text.split() == ["字書", "中文"]


# =====================================================================
# NOTOU-* — Type0/CID font with NO /ToUnicode (predefined-CMap fallback)
# =====================================================================


def _notou_predefined_pdf(content: bytes) -> bytes:
    """A Type0 CIDFontType0 with predefined UniGB-UCS2-H and **no** /ToUnicode.

    The CID→Unicode fallback (predefined CMap) must resolve text without any
    embedded ToUnicode.
    """
    objs = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        _stream_obj(4, content),
        # No /ToUnicode key on the Type0 font:
        (
            5,
            b"<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light "
            b"/Encoding /UniGB-UCS2-H /DescendantFonts [6 0 R] >>",
        ),
        (
            6,
            b"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /STSong-Light "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 4 >> "
            b"/FontDescriptor 7 0 R /DW 1000 >>",
        ),
        (7, b"<< /Type /FontDescriptor /FontName /STSong-Light /Flags 4 /Ascent 880 /Descent -120 >>"),
    ]
    return _build_pdf(objs)


def test_notou_001_predefined_cmap_resolves_without_tounicode():
    # UniGB-UCS2-H maps the 2-byte UCS2 code directly; content shows 中文.
    content = b"BT /F0 24 Tf 72 700 Td <4E2D6587> Tj ET"
    text = _page(_notou_predefined_pdf(content)).get_text().strip()
    # PyMuPDF 1.24.14: "中文" via the predefined CID→Unicode table (no ToUnicode).
    assert text == "中文"


def test_notou_002_predefined_cmap_kangxi_folds_like_pymupdf():
    # On the predefined-CMap (no-ToUnicode) path PyMuPDF folds a Kangxi radical
    # to its canonical CJK ideograph (⼀ U+2F00 -> 一 U+4E00); pdfspine matches.
    content = b"BT /F0 24 Tf 72 700 Td <2F00> Tj ET"
    text = _page(_notou_predefined_pdf(content)).get_text().strip()
    assert text == "一"
    assert ord(text) == 0x4E00


# =====================================================================
# OVERLAP-* — overlapping / co-located text (no dropped/duplicated text)
# =====================================================================


def _overlap_pdf(content: bytes) -> bytes:
    objs = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        ),
        _stream_obj(4, content),
        (5, _equal_width_type1()),
    ]
    return _build_pdf(objs)


def test_overlap_001_double_strike_no_dropped_or_duplicated_text():
    # Fake-bold: the same run "Bold" drawn twice at the exact same origin.
    # PyMuPDF keeps both strikes ("Bold\nBold"); the durable correctness property
    # is that NO glyph is dropped and NONE is duplicated beyond the content — the
    # extracted character multiset equals two copies of "Bold".
    content = (
        b"BT /F1 24 Tf 1 0 0 1 100 700 Tm (Bold) Tj "
        b"1 0 0 1 100 700 Tm (Bold) Tj ET"
    )
    text = _page(_overlap_pdf(content)).get_text()
    letters = [c for c in text if c.isalpha()]
    assert Counter(letters) == Counter("Bold" * 2)
    assert len(letters) == 8  # nothing dropped, nothing duplicated


def test_overlap_002_colocated_runs_keep_all_glyphs():
    # Two heavily-overlapping (2pt-offset) copies of "Hello". Same property: every
    # glyph survives, none is duplicated. (pdfspine interleaves co-located glyphs
    # by x; PyMuPDF keeps each run on its own line — the multiset is identical and
    # that is what "handled like fitz, no dropped/duplicated text" requires.)
    content = (
        b"BT /F1 24 Tf 1 0 0 1 100 700 Tm (Hello) Tj "
        b"1 0 0 1 102 700 Tm (Hello) Tj ET"
    )
    text = _page(_overlap_pdf(content)).get_text()
    letters = [c for c in text if c.isalpha()]
    assert Counter(letters) == Counter("Hello" * 2)
    assert len(letters) == 10
