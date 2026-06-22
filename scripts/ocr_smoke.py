#!/usr/bin/env python3
"""ocr_smoke — release gate that proves the BUILT wheel does real OCR.

Run AFTER installing the freshly built ``pdfspine`` wheel into a clean
environment (so we exercise the wheel-bundled PP-OCRv5 ONNX models at
``site-packages/pdfspine/_models``, exactly what an end user gets from
``pip install pdfspine`` — no source tree, no extra, no network):

    python scripts/ocr_smoke.py crates/pdf-ocr/tests/fixtures/ocr_sample.png

It builds a 1-page image-only "scanned" PDF from the sample raster (no text
layer), runs ``page.get_textpage_ocr(engine="paddle")``, and asserts the three
mixed CJK+Latin lines come back. Exit 0 on success, non-zero (with a clear
message) on any failure — so the CI ``wheels`` job fails and ``publish`` (which
``needs`` it) never runs. Pure stdlib decode of the 8-bit truecolor PNG (the
wheel carries no Pillow/numpy); identical on Linux/macOS/Windows.
"""

from __future__ import annotations

import struct
import sys
import zlib
from pathlib import Path

# Force UTF-8 stdout/stderr: this gate prints the recognized text (mixed
# CJK+Latin) and an em-dash banner, but Windows Python defaults stdout to the
# legacy ANSI codepage (cp1252), which cannot encode '纯' & co. and would raise
# UnicodeEncodeError *after* OCR already succeeded. reconfigure() exists since
# 3.7; guarded so the script stays robust everywhere.
for _stream in (sys.stdout, sys.stderr):
    _reconfigure = getattr(_stream, "reconfigure", None)
    if _reconfigure is not None:
        _reconfigure(encoding="utf-8")

import pdfspine

# The three lines printed in the OCR sample raster (must match
# python/tests/test_ocr_paddle.py — the canonical e2e fixture).
_LINE_LATIN_1 = "pdfspine OCR test 2026"
_LINE_CJK = "纯Rust实现的PDF文字识别"
_LINE_LATIN_2 = "PaddleOCR via tract"


def _png_to_rgb(data: bytes) -> tuple[int, int, bytes]:
    """Decodes an 8-bit color-type-2 (truecolor) non-interlaced PNG to
    ``(width, height, rgb_bytes)`` with only the stdlib."""
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
        pos += 12 + length  # length(4) + type(4) + data + crc(4)

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


def _contains_line(text: str, line: str) -> bool:
    """Whitespace-insensitive containment (CJK has no spaces, Latin may differ)."""
    norm = "".join(text.split())
    return "".join(line.split()) in norm


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {argv[0]} <ocr_sample.png>", file=sys.stderr)
        return 2
    sample = Path(argv[1])
    if not sample.exists():
        print(f"OCR sample raster missing: {sample}", file=sys.stderr)
        return 2

    print(f"pdfspine {pdfspine.__version__} — OCR release smoke on {sample}")

    w, h, rgb = _png_to_rgb(sample.read_bytes())
    doc = pdfspine.open()  # empty document
    page = doc.new_page(width=float(w), height=float(h))
    page.insert_image((0, 0, float(w), float(h)), stream=rgb, width=w, height=h)
    # Sanity: this is an image-only page — no text layer at all.
    if page.get_text("text").strip() != "":
        print("FAIL: synthetic page unexpectedly has a text layer", file=sys.stderr)
        return 1

    text = doc[0].get_textpage_ocr(dpi=150, engine="paddle").extractText()
    print(f"recognized text:\n{text!r}")

    missing = []
    if not _contains_line(text, _LINE_LATIN_1):
        missing.append(_LINE_LATIN_1)
    if _LINE_CJK not in "".join(text.split()):
        missing.append(_LINE_CJK)
    if not _contains_line(text, _LINE_LATIN_2):
        missing.append(_LINE_LATIN_2)

    if missing:
        print(f"FAIL: OCR did not recover lines: {missing!r}", file=sys.stderr)
        return 1

    print("OK: PP-OCRv5 recovered all three lines from the wheel-bundled models.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
