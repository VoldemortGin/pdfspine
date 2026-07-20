"""DICT-IMG-* — ``get_text("dict"/"json"/"rawjson")`` image blocks carry the real
encoded image bytes + raster header (PRD §10.7 / §8.6.2 / §8.10).

A ``type == 1`` image block inlines the encoded image payload (like fitz) and its
raster header (``ext``, ``colorspace`` as a channel count, ``bpc``, ``xres``/
``yres``, ``width``/``height``, ``size``). The payload is byte-for-byte the same
as ``Document.extract_image(xref)`` for the same XObject.

All fixtures are self-generated in-test (raw PDF / PNG bytes) — no external files
(PRD §10).
"""

from __future__ import annotations

import base64
import json
import struct
import zlib

import pdfspine


# --- self-generated fixtures ----------------------------------------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int) -> bytes:
    out = b"%PDF-1.7\n"
    offs: dict[int, int] = {}
    for num, body in objects:
        offs[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
    startxref = len(out)
    size = max(offs) + 1
    out += b"xref\n" + f"0 {size}\n".encode() + b"0000000000 65535 f \n"
    for i in range(1, size):
        out += f"{offs[i]:010d} 00000 n \n".encode()
    out += b"trailer\n" + f"<< /Size {size} /Root {root} 0 R >>\n".encode()
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return out


def _rgb_samples(w: int, h: int) -> bytes:
    s = bytearray()
    for y in range(h):
        for x in range(w):
            s += bytes([(x * 17) & 0xFF, (y * 23) & 0xFF, ((x + y) * 5) & 0xFF])
    return bytes(s)


def _image_only_pdf(w: int, h: int, samples: bytes) -> bytes:
    """A 1-page PDF drawing one Flate-encoded DeviceRGB image XObject (obj 4)."""
    img = zlib.compress(samples)
    img_obj = (
        b"<< /Type /XObject /Subtype /Image "
        + f"/Width {w} /Height {h} ".encode()
        + b"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode "
        + f"/Length {len(img)} ".encode()
        + b">>\nstream\n"
        + img
        + b"\nendstream"
    )
    content = b"q 200 0 0 200 0 0 cm /Im0 Do Q"
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
                b"/Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>",
            ),
            (4, img_obj),
            (
                5,
                f"<< /Length {len(content)} >>\nstream\n".encode()
                + content
                + b"\nendstream",
            ),
        ],
        root=1,
    )


def _png(w: int, h: int, rgb: tuple[int, int, int] = (255, 0, 0)) -> bytes:
    """A minimal 8-bit RGB PNG of size ``w`` x ``h`` filled with ``rgb``."""

    def chunk(typ: bytes, data: bytes) -> bytes:
        body = typ + data
        return (
            struct.pack(">I", len(data))
            + body
            + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)
        )

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    return (
        sig
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def _image_blocks(blocks: list[dict]) -> list[dict]:
    return [b for b in blocks if b["type"] == 1]


# --- DICT-IMG-001: dict image block carries real bytes + full header -------


def test_dictimg_001_dict_block_has_real_bytes_and_header():
    w, h = 8, 6
    doc = pdfspine.open(stream=_image_only_pdf(w, h, _rgb_samples(w, h)))
    page = doc[0]
    imgs = _image_blocks(page.get_text("dict")["blocks"])
    assert len(imgs) == 1
    b = imgs[0]
    # Full PyMuPDF image-block key set is present.
    assert set(b.keys()) == {
        "number",
        "type",
        "bbox",
        "width",
        "height",
        "ext",
        "colorspace",
        "xres",
        "yres",
        "bpc",
        "transform",
        "size",
        "image",
    }
    # Real encoded bytes, not a stub.
    assert isinstance(b["image"], bytes) and len(b["image"]) > 0
    assert b["image"][:8] == b"\x89PNG\r\n\x1a\n"  # Flate raster → re-encoded PNG
    assert b["ext"] == "png"
    assert b["width"] == w and b["height"] == h
    assert b["bpc"] == 8
    assert b["colorspace"] == 3  # DeviceRGB → 3 channels (fitz uses channel count)
    assert b["xres"] == 96 and b["yres"] == 96
    assert b["size"] == len(b["image"])
    assert isinstance(b["transform"], tuple) and len(b["transform"]) == 6


# --- DICT-IMG-002: dict block bytes == Document.extract_image(xref) --------


def test_dictimg_002_bytes_match_extract_image():
    w, h = 8, 6
    doc = pdfspine.open(stream=_image_only_pdf(w, h, _rgb_samples(w, h)))
    page = doc[0]
    b = _image_blocks(page.get_text("dict")["blocks"])[0]

    xref = page.get_images()[0][0]
    ex = doc.extract_image(xref)
    # Same-source parity: identical encoded payload + coherent header.
    assert bytes(b["image"]) == bytes(ex["image"])
    assert b["ext"] == ex["ext"]
    assert b["bpc"] == ex["bpc"]
    assert b["width"] == ex["width"] and b["height"] == ex["height"]
    assert b["colorspace"] == ex["n"]  # dict colorspace == extract_image components


# --- DICT-IMG-003: rawdict image block is populated the same way ----------


def test_dictimg_003_rawdict_block_has_bytes():
    w, h = 8, 6
    doc = pdfspine.open(stream=_image_only_pdf(w, h, _rgb_samples(w, h)))
    page = doc[0]
    b = _image_blocks(page.get_text("rawdict")["blocks"])[0]
    ex = doc.extract_image(page.get_images()[0][0])
    assert bytes(b["image"]) == bytes(ex["image"])
    assert b["ext"] == "png" and b["colorspace"] == 3 and b["bpc"] == 8


# --- DICT-IMG-004: json / rawjson image field is base64 of the bytes ------


def test_dictimg_004_json_and_rawjson_base64():
    w, h = 8, 6
    doc = pdfspine.open(stream=_image_only_pdf(w, h, _rgb_samples(w, h)))
    page = doc[0]
    ex_bytes = bytes(doc.extract_image(page.get_images()[0][0])["image"])

    for opt in ("json", "rawjson"):
        payload = json.loads(page.get_text(opt))
        img = _image_blocks(payload["blocks"])[0]
        assert isinstance(img["image"], str) and img["image"]  # non-empty base64
        assert base64.b64decode(img["image"]) == ex_bytes
        assert img["ext"] == "png"
        assert img["width"] == w and img["height"] == h
        assert img["bpc"] == 8 and img["colorspace"] == 3


# --- DICT-IMG-005: convert_to_pdf (image → PDF) fixture also inlines bytes -


def test_dictimg_005_convert_to_pdf_fixture():
    # pdfspine.open transparently converts a raster image to a 1-page PDF.
    w, h = 10, 7
    doc = pdfspine.open(stream=_png(w, h))
    page = doc[0]
    imgs = _image_blocks(page.get_text("dict")["blocks"])
    assert len(imgs) == 1
    b = imgs[0]
    assert isinstance(b["image"], bytes) and len(b["image"]) > 0
    assert b["ext"] and b["bpc"] > 0 and b["colorspace"] > 0
    assert b["width"] == w and b["height"] == h
    # Byte-for-byte parity with extract_image on the embedded XObject.
    xref = page.get_images()[0][0]
    assert bytes(b["image"]) == bytes(doc.extract_image(xref)["image"])
