"""OCR-PADDLE-* — end-to-end proof of pdfspine's pure-Rust PaddleOCR engine
(``engine="paddle"``) through the full Python pipeline.

The pipeline under test, with NO external binary and NO network (the PP-OCRv4
models load from disk in the OCR build):

1. Build an image-only "scanned" page in memory: a new doc / page whose entire
   content is the OCR sample raster (three mixed CJK+Latin lines) inserted via
   ``Page.insert_image`` — there is no text layer at all.
2. ``page.get_textpage_ocr(engine="paddle")`` recovers the three lines.
3. ``doc.pdfocr_tobytes(engine="paddle")`` produces a searchable "sandwich"
   PDF; reopening it and calling ``page.get_text()`` finds the same three lines
   in the now-present invisible text layer.

The PaddleOCR engine is an OPT-IN build (P0-5): the lean base wheel compiles it
out, so these paddle assertions are SKIPPED unless the wheel was built with the
``ocr`` feature (``maturin develop --features ocr``). The default
``engine="tesseract"`` path is exercised regardless, but its recognition
assertions are guarded with a skip when Tesseract is not installed (it is an
external dependency).
"""

from __future__ import annotations

import shutil
import struct
import zlib
from pathlib import Path

import pdfspine
import pytest


_HAS_TESS = shutil.which("tesseract") is not None

_REPO_ROOT = Path(__file__).resolve().parents[2]
_SAMPLE_PNG = _REPO_ROOT / "crates" / "pdf-ocr" / "tests" / "fixtures" / "ocr_sample.png"

# The three lines printed in the OCR sample raster (CJK must match exactly;
# Latin lines allow minor whitespace differences).
_LINE_LATIN_1 = "pdfspine OCR test 2026"
_LINE_CJK = "纯Rust实现的PDF文字识别"
_LINE_LATIN_2 = "PaddleOCR via tract"


# --- guard: the sample raster must exist (it is committed in-repo) ----------

if not _SAMPLE_PNG.exists():  # pragma: no cover
    pytest.skip(
        f"OCR sample raster missing: {_SAMPLE_PNG}",
        allow_module_level=True,
    )


# --- pure-stdlib PNG -> raw RGB decoder ------------------------------------
#
# The wheel has no Pillow/numpy, and ``Page.insert_image(stream=...)`` only
# passes JPEG through; for a PNG we decode it here to raw width*height*3 RGB
# bytes and use the ``width=``/``height=`` raw-RGB path. The sample is an 8-bit,
# non-interlaced, color-type-2 (truecolor, no alpha) PNG — the simple case.


def _png_to_rgb(data: bytes) -> tuple[int, int, bytes]:
    """Decodes an 8-bit color-type-2 PNG to ``(width, height, rgb_bytes)``."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    width = height = 0
    bit_depth = color_type = 0
    idat = bytearray()
    pos = 8
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        ctype = data[pos + 4 : pos + 8]
        chunk = data[pos + 8 : pos + 8 + length]
        if ctype == b"IHDR":
            width, height, bit_depth, color_type = struct.unpack(">IIBB", chunk[:10])
        elif ctype == b"IDAT":
            idat += chunk
        elif ctype == b"IEND":
            break
        pos += 12 + length  # length + type(4) + data + crc(4)

    assert bit_depth == 8 and color_type == 2, (
        f"expected 8-bit truecolor PNG, got bit_depth={bit_depth} color_type={color_type}"
    )

    raw = zlib.decompress(bytes(idat))
    stride = width * 3
    out = bytearray(width * height * 3)
    prev = bytearray(stride)
    src = 0
    for row in range(height):
        filt = raw[src]
        src += 1
        line = bytearray(raw[src : src + stride])
        src += stride
        # PNG per-scanline filters (bpp = 3 for RGB8).
        if filt == 1:  # Sub
            for i in range(3, stride):
                line[i] = (line[i] + line[i - 3]) & 0xFF
        elif filt == 2:  # Up
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif filt == 3:  # Average
            for i in range(stride):
                a = line[i - 3] if i >= 3 else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif filt == 4:  # Paeth
            for i in range(stride):
                a = line[i - 3] if i >= 3 else 0
                b = prev[i]
                c = prev[i - 3] if i >= 3 else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        out[row * stride : (row + 1) * stride] = line
        prev = line
    return width, height, bytes(out)


def _scanned_pdf() -> pdfspine.Document:
    """A 1-page image-only "scanned" doc: the OCR sample raster fills the page,
    with no text layer at all."""
    w, h, rgb = _png_to_rgb(_SAMPLE_PNG.read_bytes())
    doc = pdfspine.open()  # empty document
    page = doc.new_page(width=float(w), height=float(h))
    page.insert_image((0, 0, float(w), float(h)), stream=rgb, width=w, height=h)
    # Sanity: this is an image-only page — no text layer yet.
    assert page.get_text("text").strip() == ""
    return doc


def _contains_line(text: str, line: str) -> bool:
    """Whitespace-insensitive containment (CJK has no spaces, Latin may differ)."""
    norm = "".join(text.split())
    return "".join(line.split()) in norm


def _paddle_available() -> bool:
    """Whether this wheel was built with the opt-in ``ocr`` feature (PaddleOCR).

    Probes by running ``engine="paddle"`` on a tiny image-only page. On a lean
    base build the engine is compiled out, so routing raises
    ``PdfUnsupportedError`` *before* any model work — we treat that as "not
    available" and skip. On an OCR build it recognizes (returning, possibly
    empty) text without raising.
    """
    doc = pdfspine.open()
    page = doc.new_page(width=8.0, height=8.0)
    page.insert_image((0, 0, 8.0, 8.0), stream=b"\xff" * (8 * 8 * 3), width=8, height=8)
    try:
        doc[0].get_textpage_ocr(dpi=72, engine="paddle")
    except pdfspine.PdfUnsupportedError:
        return False
    return True


_HAS_PADDLE = _paddle_available()

_requires_paddle = pytest.mark.skipif(
    not _HAS_PADDLE,
    reason="PaddleOCR engine not compiled in (lean build); install pdfspine[ocr]",
)


# --- OCR-PADDLE-001: get_textpage_ocr(engine="paddle") -> the 3 lines -------


@_requires_paddle
def test_paddle_get_textpage_ocr():
    doc = _scanned_pdf()
    tp = doc[0].get_textpage_ocr(dpi=150, engine="paddle")
    text = tp.extractText()
    assert _contains_line(text, _LINE_LATIN_1), f"missing line 1 in:\n{text!r}"
    assert _LINE_CJK in "".join(text.split()), f"missing CJK line in:\n{text!r}"
    assert _contains_line(text, _LINE_LATIN_2), f"missing line 3 in:\n{text!r}"


# --- OCR-PADDLE-002: pdfocr_tobytes(engine="paddle") -> searchable sandwich -


@_requires_paddle
def test_paddle_pdfocr_tobytes_searchable():
    doc = _scanned_pdf()
    sandwich = doc.pdfocr_tobytes(dpi=150, engine="paddle")
    assert sandwich[:5] == b"%PDF-"
    assert len(sandwich) > 1000

    reopened = pdfspine.open(stream=sandwich)
    assert len(reopened) == 1
    text = reopened[0].get_text("text")
    # The three lines are now SEARCHABLE in the invisible OCR text layer.
    assert _contains_line(text, _LINE_LATIN_1), f"missing line 1 in:\n{text!r}"
    assert _LINE_CJK in "".join(text.split()), f"missing CJK line in:\n{text!r}"
    assert _contains_line(text, _LINE_LATIN_2), f"missing line 3 in:\n{text!r}"


# --- OCR-PADDLE-003: the default tesseract path still works (untouched) -----


def test_default_engine_unchanged():
    """The default engine is still Tesseract and the signature is unchanged for
    positional / omitted callers. Tesseract is an external dependency, so its
    recognition assertions are skipped when it is absent — but the call path
    (default engine, no ``engine=`` kwarg) must still resolve to Tesseract."""
    doc = _scanned_pdf()
    if not _HAS_TESS:
        # No tesseract binary: the DEFAULT path must raise the unified
        # unsupported error (proving the default is still Tesseract, not paddle).
        with pytest.raises(pdfspine.PdfUnsupportedError):
            doc[0].get_textpage_ocr(dpi=150)
        pytest.skip("tesseract not installed; default-path recognition not asserted")

    # Tesseract present: the default path recognizes the Latin content (Tesseract
    # has no CJK pack by default, and may garble the made-up word "pdfspine" — it
    # reads it as "odfspine"). The point of this test is that the default path
    # ROUTES to Tesseract and returns recognized text, so assert on the tokens
    # Tesseract reads reliably, not the fragile coined word.
    text = doc[0].get_textpage_ocr(dpi=150).extractText().lower()
    assert "ocr" in text and "2026" in text


# --- OCR-PADDLE-004: an unknown engine raises the unified unsupported error -


def test_unknown_engine_raises():
    doc = _scanned_pdf()
    with pytest.raises(pdfspine.PdfUnsupportedError):
        doc[0].get_textpage_ocr(dpi=72, engine="nope")
