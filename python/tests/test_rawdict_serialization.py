"""Rawdict conversion preserves per-character values and native Python types."""

from __future__ import annotations

import json
import math

import pdfspine
import pytest


def _document(content: bytes):
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length "
        + str(len(content)).encode()
        + b" >>\nstream\n"
        + content
        + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>",
    ]
    data = bytearray(b"%PDF-1.7\n")
    offsets = [0]
    for number, body in enumerate(objects, 1):
        offsets.append(len(data))
        data += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(data)
    data += f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode()
    for offset in offsets[1:]:
        data += f"{offset:010} 00000 n \n".encode()
    data += f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return pdfspine.open(stream=bytes(data))


def _spans(data):
    return [
        span
        for block in data["blocks"]
        if block["type"] == 0
        for line in block["lines"]
        for span in line["spans"]
    ]


@pytest.mark.parametrize("matrix", ["14 0 0 12", "12 0 6 12", "0 0 0 12", "12 0 0 0"])
def test_rawdict_preserves_heterogeneous_and_degenerate_geometry(matrix: str) -> None:
    content = (
        b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (A) Tj "
        + matrix.encode()
        + b" 108 700 Tm (B) Tj ET"
    )
    with _document(content) as doc:
        page = doc[0]
        raw = page.get_text("rawdict")
        chars = [
            char
            for span in _spans(raw)
            for char in span["chars"]
            if not char["synthetic"]
        ]
        by_char = {char["c"]: char for char in chars}
        assert set(by_char) == {"A", "B"}
        assert by_char["A"]["matrix"] == (12.0, 0.0, 0.0, -12.0, 100.0, 92.0)
        a, b, c, d = map(float, matrix.split())
        expected = (a, -b, c, -d, 108.0, 92.0)
        assert by_char["B"]["matrix"] == expected
        assert by_char["B"]["rendered_size"] == math.sqrt(abs(a * d - b * c))
        for char in chars:
            for key, length in (("origin", 2), ("bbox", 4), ("matrix", 6), ("quad", 8)):
                assert type(char[key]) is tuple and len(char[key]) == length
                assert all(type(value) is float for value in char[key])
            assert type(char["rendered_size"]) is float
            assert type(char["seq"]) is int
            assert type(char["synthetic"]) is bool

        # The Rust JSON serializer is independent of the native-object bridge.
        # Its decimal formatting is limited to six places, so normalize floats
        # before checking every nested key and value.
        def rounded(value):
            if isinstance(value, dict):
                return {key: rounded(item) for key, item in value.items()}
            if isinstance(value, (tuple, list)):
                return [rounded(item) for item in value]
            return round(value, 6) if isinstance(value, float) else value

        assert rounded(raw) == rounded(json.loads(page.get_text("rawjson")))
        assert rounded(page.get_text("dict")) == rounded(
            json.loads(page.get_text("json"))
        )
        # Every call returns independent mutable containers.
        chars[0]["c"] = "changed"
        again = [
            ch for span in _spans(page.get_text("rawdict")) for ch in span["chars"]
        ]
        assert all(ch["c"] != "changed" for ch in again)
