"""OCR-PY-* — OCR text extraction, searchable-sandwich export, and the
absent-engine contract from Python (``page.get_textpage_ocr``,
``doc.pdfocr_tobytes`` / ``doc.pdfocr_save``).

All fixtures are self-generated in-test (raw classic-xref PDF bytes via
``stream=``) and embed the repo's DejaVu glyph-subset TrueType so the page
renders readable text — no external / PyMuPDF files (PRD §10).

Tests that need real recognition are skipped when ``tesseract`` is absent; the
absent-engine test runs unconditionally (it forces an absent binary).
"""

from __future__ import annotations

import shutil
from pathlib import Path

import pdfspine
import pytest


_HAS_TESS = shutil.which("tesseract") is not None

_REPO_ROOT = Path(__file__).resolve().parents[2]
_FONT_PATH = _REPO_ROOT / "crates" / "pdf-ocr" / "tests" / "fixtures" / "ocrtest.ttf"


# --- self-generated text PDF fixture (embedded DejaVu subset) --------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int = 1) -> bytes:
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


def _stream(dict_str: str, data: bytes) -> bytes:
    return dict_str.encode() + b"\nstream\n" + data + b"\nendstream"


def _text_pdf() -> bytes:
    """A 1-page PDF showing "HELLO OCR WORLD" via the embedded DejaVu subset."""
    ttf = _FONT_PATH.read_bytes()
    content = b"BT /F1 50 Tf 1 0 0 1 40 90 Tm (HELLO OCR WORLD) Tj ET"
    widths = "[ " + " ".join(["600"] * 95) + " ]"
    font = (
        b"<< /Type /Font /Subtype /TrueType /BaseFont /DejaVuSans "
        b"/FirstChar 32 /LastChar 126 /Widths "
        + widths.encode()
        + b" /FontDescriptor 6 0 R /Encoding /WinAnsiEncoding >>"
    )
    descriptor = (
        b"<< /Type /FontDescriptor /FontName /DejaVuSans /Flags 32 "
        b"/FontBBox [-1021 -463 1793 1232] /ItalicAngle 0 /Ascent 928 "
        b"/Descent -236 /CapHeight 928 /StemV 80 /FontFile2 7 0 R >>"
    )
    fontfile = _stream(f"<< /Length1 {len(ttf)} /Length {len(ttf)} >>", ttf)
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 200] "
                b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, _stream(f"<< /Length {len(content)} >>", content)),
            (5, font),
            (6, descriptor),
            (7, fontfile),
        ],
        root=1,
    )


# --- guard: skip render-dependent tests if the font fixture is missing -----

if not _FONT_PATH.exists():  # pragma: no cover
    pytest.skip(
        f"DejaVu subset font missing: {_FONT_PATH}",
        allow_module_level=True,
    )


# --- OCR-PY-001: page.get_textpage_ocr → readable text --------------------


@pytest.mark.skipif(not _HAS_TESS, reason="tesseract not installed")
def test_ocr_py_get_textpage_ocr():
    doc = pdfspine.open(stream=_text_pdf())
    tp = doc[0].get_textpage_ocr(dpi=150)
    t = tp.extractText().lower()
    assert "hello" in t
    assert "world" in t


# --- OCR-PY-002: camelCase alias getTextPageOCR ---------------------------


@pytest.mark.skipif(not _HAS_TESS, reason="tesseract not installed")
def test_ocr_py_camelcase_alias():
    doc = pdfspine.open(stream=_text_pdf())
    t = doc[0].getTextPageOCR(dpi=150).extractText().lower()
    assert "hello" in t


# --- OCR-PY-003: pdfocr_tobytes → searchable sandwich PDF -----------------


@pytest.mark.skipif(not _HAS_TESS, reason="tesseract not installed")
def test_ocr_py_pdfocr_tobytes_searchable():
    doc = pdfspine.open(stream=_text_pdf())
    sb = doc.pdfocr_tobytes(dpi=150)
    assert len(sb) > 1000
    assert sb[:5] == b"%PDF-"
    d2 = pdfspine.open(stream=sb)
    assert len(d2) == 1
    txt = d2[0].get_text("text").lower()
    flat = txt.replace(" ", "").replace("\n", "")
    assert "hello" in flat
    assert "world" in flat


# --- OCR-PY-004: pdfocr_save writes a searchable file ---------------------


@pytest.mark.skipif(not _HAS_TESS, reason="tesseract not installed")
def test_ocr_py_pdfocr_save_file(tmp_path):
    doc = pdfspine.open(stream=_text_pdf())
    out = tmp_path / "out.pdf"
    doc.pdfocr_save(str(out), dpi=150)
    assert out.exists()
    data = out.read_bytes()
    assert data[:5] == b"%PDF-"
    d2 = pdfspine.open(stream=data)
    assert len(d2) == 1
    flat = d2[0].get_text("text").lower().replace(" ", "").replace("\n", "")
    assert "hello" in flat


# --- OCR-PY-005: the OCR'd word is searchable in the sandwich -------------


@pytest.mark.skipif(not _HAS_TESS, reason="tesseract not installed")
def test_ocr_py_search_in_sandwich():
    doc = pdfspine.open(stream=_text_pdf())
    sb = doc.pdfocr_tobytes(dpi=150)
    d2 = pdfspine.open(stream=sb)
    assert len(d2[0].search_for("HELLO")) >= 1


# --- OCR-PY-006: absent OCR engine raises (runs without tesseract) --------


def test_ocr_py_absent_engine_raises(monkeypatch):
    monkeypatch.setenv("OXIDE_TESSERACT", "/nonexistent/tesseract-xyz")
    doc = pdfspine.open(stream=_text_pdf())
    with pytest.raises(pdfspine.PdfUnsupportedError):
        doc[0].get_textpage_ocr(dpi=72)
    doc2 = pdfspine.open(stream=_text_pdf())
    with pytest.raises(pdfspine.PdfUnsupportedError):
        doc2.pdfocr_tobytes(dpi=72)


# --- OCR-PY-007: fitz shim parity -----------------------------------------


@pytest.mark.skipif(not _HAS_TESS, reason="tesseract not installed")
def test_ocr_py_fitz_shim():
    import fitz

    fd = fitz.open(stream=_text_pdf())
    assert "hello" in fd[0].get_textpage_ocr(dpi=150).extractText().lower()
