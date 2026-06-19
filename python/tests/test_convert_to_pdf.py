"""CONVERT-TO-PDF-* — image-input ``fitz.open`` and ``Document.convert_to_pdf``
(PRD §8.10).

``pdfspine.open`` transparently converts a raster image (bytes or path) to a
single-page PDF :class:`~pdfspine.Document`; ``Document.convert_to_pdf`` returns
those PDF bytes. A genuinely non-image input raises ``PdfUnsupportedError``.

All fixtures are self-generated in-test (raw PNG bytes) — no external files
(PRD §10).
"""

from __future__ import annotations

import struct
import zlib

import pdfspine
import pytest


def _png(w: int, h: int, rgb: tuple[int, int, int] = (255, 0, 0)) -> bytes:
    """A minimal 8-bit RGB PNG of size ``w`` x ``h`` filled with ``rgb``."""

    def chunk(typ: bytes, data: bytes) -> bytes:
        body = typ + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    return sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


# --- CONVERT-TO-PDF-001: open image bytes → 1-page PDF Document ------------


def test_convert_to_pdf_001_open_image_bytes():
    doc = pdfspine.open(stream=_png(8, 6))
    assert doc.page_count == 1
    assert doc.is_pdf
    # The page geometry is non-empty (one frame → one page).
    assert doc[0].rect.width > 0 and doc[0].rect.height > 0


# --- CONVERT-TO-PDF-002: explicit filetype= image hint --------------------


def test_convert_to_pdf_002_open_image_bytes_filetype():
    doc = pdfspine.open(stream=_png(4, 4), filetype="png")
    assert doc.page_count == 1
    assert doc.is_pdf


# --- CONVERT-TO-PDF-003: open image by path -------------------------------


def test_convert_to_pdf_003_open_image_path(tmp_path):
    p = tmp_path / "frame.png"
    p.write_bytes(_png(5, 3))
    doc = pdfspine.open(str(p))
    assert doc.page_count == 1
    assert doc.is_pdf
    assert doc._name == str(p)


# --- CONVERT-TO-PDF-004: convert_to_pdf bytes reparse cleanly --------------


def test_convert_to_pdf_004_roundtrip():
    doc = pdfspine.open(stream=_png(8, 6))
    pdf = doc.convert_to_pdf()
    assert isinstance(pdf, bytes)
    assert pdf[:5] == b"%PDF-"
    reparsed = pdfspine.open(stream=pdf)
    assert reparsed.page_count == 1
    assert reparsed.is_pdf


# --- CONVERT-TO-PDF-005: convert_to_pdf accepts fitz page/rotate kwargs ----


def test_convert_to_pdf_005_compat_kwargs():
    doc = pdfspine.open(stream=_png(4, 4))
    pdf = doc.convert_to_pdf(from_page=0, to_page=-1, rotate=0)
    assert pdf[:5] == b"%PDF-"


# --- CONVERT-TO-PDF-006: non-image input raises PdfUnsupportedError --------


def test_convert_to_pdf_006_non_image_raises():
    with pytest.raises(pdfspine.PdfUnsupportedError):
        pdfspine.open(stream=b"this is plainly not an image, nor a PDF")


def test_convert_to_pdf_007_non_image_with_image_filetype_raises():
    with pytest.raises(pdfspine.PdfUnsupportedError):
        pdfspine.open(stream=b"\x00\x01\x02 garbage", filetype="png")


# --- CONVERT-TO-PDF-008: a real PDF still opens / converts normally --------


def test_convert_to_pdf_008_pdf_passthrough():
    doc = pdfspine.open()  # blank PDF
    pdf = doc.convert_to_pdf()
    assert pdf[:5] == b"%PDF-"
    assert pdfspine.open(stream=pdf).is_pdf
