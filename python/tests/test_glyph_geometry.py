"""``PYGEO-*`` — the glyph-geometry keys published through the Python surface.

``get_text("dict"/"rawdict"/"json"/"rawjson")`` carries, on top of the PyMuPDF
key set, the full rendering geometry of every span and char: the declared *and*
rendered font size, the device-space render matrix, the raw user-space ``Tm`` and
CTM it was composed from, the true (possibly sheared) glyph quad, the baseline
direction, and the painting-order / reading-order keys. These tests pin the
shape, the types and the three geometry invariants (PRD §8.6.1).

Fixtures are self-generated raw PDF bytes — no external files (PRD §10).
"""

from __future__ import annotations

import json
import math

import pdfspine


# --- self-generated PDF assembler (classic xref) --------------------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int) -> bytes:
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
    out += f"<< /Size {size} /Root {root} 0 R >>\n".encode()
    out += b"startxref\n"
    out += f"{startxref}\n".encode()
    out += b"%%EOF\n"
    return bytes(out)


def _helvetica_font() -> bytes:
    """Type1 Helvetica/WinAnsi, every code 500/1000 wide (deterministic cells)."""
    widths = b"[" + b" ".join(b"500" for _ in range(94)) + b"]"
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 125 /Widths "
        + widths
        + b" >>"
    )


def _page(content: bytes):
    """A 612x792 page (unrotated) whose content stream is ``content``."""
    pdf = _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (
                4,
                b"<< /Length "
                + str(len(content)).encode()
                + b" >>\nstream\n"
                + content
                + b"\nendstream",
            ),
            (5, _helvetica_font()),
        ],
        root=1,
    )
    return pdfspine.open(stream=pdf)[0]


# The scale lives entirely in `Tm`, so declared (`Tf 1`) and rendered (12)
# font sizes disagree — the case downstream used to reverse-engineer from bbox.
TM_SCALED = b"BT /F1 1 Tf 12 0 0 12 100 700 Tm (Hi) Tj ET"
# `Tm 12 0 6 12` shears the cell into a real parallelogram.
SHEARED = b"BT /F1 1 Tf 12 0 6 12 100 700 Tm (A) Tj ET"


def _first_span(page, kind: str) -> dict:
    d = page.get_text(kind)
    return d["blocks"][0]["lines"][0]["spans"][0]


# === PYGEO-001: the dict span key set ====================================


def test_pygeo_001_dict_span_geometry_keys() -> None:
    span = _first_span(_page(TM_SCALED), "dict")
    assert set(span.keys()) == {
        # the PyMuPDF key set, unchanged
        "size",
        "flags",
        "font",
        "color",
        "ascender",
        "descender",
        "origin",
        "bbox",
        "text",
        # the pdfspine geometry extension
        "declared_size",
        "rendered_size",
        "matrix",
        "text_matrix",
        "ctm",
        "dir",
        "quad",
        "seq",
    }
    assert isinstance(span["declared_size"], float)
    assert isinstance(span["rendered_size"], float)
    assert isinstance(span["seq"], int)
    for key, arity in (("matrix", 6), ("text_matrix", 6), ("ctm", 6), ("quad", 8)):
        assert isinstance(span[key], tuple) and len(span[key]) == arity, key
        assert all(isinstance(v, float) for v in span[key]), key
    assert isinstance(span["dir"], tuple) and len(span["dir"]) == 2


# === PYGEO-002: the rawdict char key set =================================


def test_pygeo_002_rawdict_char_geometry_keys() -> None:
    span = _first_span(_page(TM_SCALED), "rawdict")
    assert "text" not in span
    char = span["chars"][0]
    assert set(char.keys()) == {
        "origin",
        "bbox",
        "c",
        "matrix",
        "quad",
        "rendered_size",
        "seq",
        "synthetic",
    }
    assert isinstance(char["matrix"], tuple) and len(char["matrix"]) == 6
    assert isinstance(char["quad"], tuple) and len(char["quad"]) == 8
    assert isinstance(char["rendered_size"], float)
    assert isinstance(char["seq"], int)
    assert char["synthetic"] is False


# === PYGEO-003: declared vs rendered font size ===========================


def test_pygeo_003_rendered_size_beats_declared_size() -> None:
    span = _first_span(_page(TM_SCALED), "rawdict")
    # Structured output reports the rendered size, matching fitz; the original
    # `Tf` operand remains available under its explicit name.
    assert span["size"] == 12.0
    assert span["declared_size"] == 1.0
    # `rendered_size` is sqrt(|det|) of the render matrix — the painted size.
    assert math.isclose(span["rendered_size"], 12.0, abs_tol=1e-9)
    a, b, c, d = span["matrix"][:4]
    assert math.isclose(
        span["rendered_size"], math.sqrt(abs(a * d - b * c)), abs_tol=1e-9
    )
    assert all(
        math.isclose(ch["rendered_size"], 12.0, abs_tol=1e-9) for ch in span["chars"]
    )


# === PYGEO-004: the three geometry invariants ============================


def test_pygeo_004_geometry_invariants() -> None:
    for content in (TM_SCALED, SHEARED):
        page = _page(content)
        span = _first_span(page, "rawdict")

        # 1. (0,0) * matrix == origin, for the span and for every char.
        for item in [span, *span["chars"]]:
            m = pdfspine.Matrix(*item["matrix"])
            p = pdfspine.Point(0, 0) * m
            assert math.isclose(p.x, item["origin"][0], abs_tol=1e-9)
            assert math.isclose(p.y, item["origin"][1], abs_tol=1e-9)

            # 2. the quad's bounding rect is the bbox.
            xs = item["quad"][0::2]
            ys = item["quad"][1::2]
            for got, want in zip(
                (min(xs), min(ys), max(xs), max(ys)), item["bbox"], strict=True
            ):
                assert math.isclose(got, want, abs_tol=1e-9)

        # 3. matrix == params * text_matrix * ctm * page_transform, with
        #    params = [Tfs*Th, 0, 0, Tfs, 0, Trise] (Th = 1, Trise = 0 here) and
        #    the unrotated 612x792 page transform [1, 0, 0, -1, 0, 792].
        fs = span["declared_size"]
        composed = (
            pdfspine.Matrix(fs, 0, 0, fs, 0, 0)
            * pdfspine.Matrix(*span["text_matrix"])
            * pdfspine.Matrix(*span["ctm"])
            * pdfspine.Matrix(1, 0, 0, -1, 0, 792)
        )
        flat = (composed.a, composed.b, composed.c, composed.d, composed.e, composed.f)
        for got, want in zip(flat, span["matrix"], strict=True):
            assert math.isclose(got, want, abs_tol=1e-9)


# === PYGEO-005: the sheared quad is a real parallelogram =================


def test_pygeo_005_sheared_char_quad_is_not_axis_aligned() -> None:
    char = _first_span(_page(SHEARED), "rawdict")["chars"][0]
    ul, ur, ll, lr = (
        char["quad"][0:2],
        char["quad"][2:4],
        char["quad"][4:6],
        char["quad"][6:8],
    )
    # Opposite edges are equal vectors.
    assert math.isclose(ur[0] - ul[0], lr[0] - ll[0], abs_tol=1e-9)
    assert math.isclose(ll[1] - ul[1], lr[1] - ur[1], abs_tol=1e-9)
    # ...and the left edge leans: the quad is NOT the axis-aligned bbox.
    assert abs(ll[0] - ul[0]) > 1.0
    # `ul` is the upper-left corner in device space (y grows downwards).
    assert ul[1] < ll[1]
    # The xml dump publishes the same four corners.
    xml = _page(SHEARED).get_text("xml")
    quad = xml.split('quad="', 1)[1].split('"', 1)[0].split()
    for got, want in zip((float(v) for v in quad), char["quad"], strict=True):
        assert math.isclose(got, want, abs_tol=1e-6)


# === PYGEO-006: line/block ordering keys =================================


def test_pygeo_006_line_number_and_seq() -> None:
    page = _page(
        b"BT /F1 12 Tf 100 700 Td (Alpha) Tj 0 -20 Td (Beta) Tj 0 -20 Td (Gamma) Tj ET"
    )
    d = page.get_text("dict")
    lines = [ln for b in d["blocks"] for ln in b["lines"]]
    for b in d["blocks"]:
        assert isinstance(b["seq"], int) and isinstance(b["number"], int)
    assert sorted(ln["number"] for ln in lines) == list(range(len(lines)))
    for ln in lines:
        assert isinstance(ln["seq"], int)

    # Ordering the lines by `number` reproduces get_text("text") line for line.
    lines.sort(key=lambda ln: ln["number"])
    got = ["".join(s["text"] for s in ln["spans"]) for ln in lines]
    want = [ln for ln in page.get_text("text").splitlines() if ln]
    assert got == want


# === PYGEO-007: json / rawjson publish the same geometry =================


def test_pygeo_007_json_matches_dict() -> None:
    page = _page(TM_SCALED)
    jspan = json.loads(page.get_text("json"))["blocks"][0]["lines"][0]["spans"][0]
    dspan = _first_span(page, "dict")
    for key in (
        "declared_size",
        "rendered_size",
        "matrix",
        "text_matrix",
        "ctm",
        "dir",
        "quad",
        "seq",
    ):
        assert key in jspan, key
        want = dspan[key]
        if isinstance(want, tuple):
            assert all(
                math.isclose(g, w, abs_tol=1e-6)
                for g, w in zip(jspan[key], want, strict=True)
            ), key
        else:
            assert math.isclose(jspan[key], want, abs_tol=1e-6), key

    jchar = json.loads(page.get_text("rawjson"))["blocks"][0]["lines"][0]["spans"][0][
        "chars"
    ][0]
    dchar = _first_span(page, "rawdict")["chars"][0]
    assert jchar["synthetic"] is False
    assert jchar["seq"] == dchar["seq"]
    assert all(
        math.isclose(g, w, abs_tol=1e-6)
        for g, w in zip(jchar["quad"], dchar["quad"], strict=True)
    )


# === PYGEO-008: a rotated run keeps its own frame ========================


def test_pygeo_008_rotated_run_dir_and_matrix() -> None:
    # `Tm 0 12 -12 0` is a 90deg rotation at scale 12.
    span = _first_span(_page(b"BT /F1 1 Tf 0 12 -12 0 100 700 Tm (Hi) Tj ET"), "dict")
    assert math.isclose(span["rendered_size"], 12.0, abs_tol=1e-9)
    # The baseline turned with the text (device space: y down flips the sign).
    assert math.isclose(abs(span["dir"][1]), 1.0, abs_tol=1e-9)
    assert math.isclose(span["dir"][0], 0.0, abs_tol=1e-9)
    # `text_matrix` stays the raw user-space operand, untouched by the page flip.
    for got, want in zip(
        span["text_matrix"], (0.0, 12.0, -12.0, 0.0, 100.0, 700.0), strict=True
    ):
        assert math.isclose(got, want, abs_tol=1e-9)
    # ...while `matrix` is device space: the y-flip negates the second column.
    for got, want in zip(span["matrix"][:4], (0.0, -12.0, -12.0, 0.0), strict=True):
        assert math.isclose(got, want, abs_tol=1e-9)


# === PYGEO-009: rotated quad keeps glyph-frame corner names =============


def test_pygeo_009_rotated_quad_keeps_glyph_frame_corner_names() -> None:
    char = _first_span(
        _page(b"BT /F1 1 Tf 0 12 -12 0 100 700 Tm (A) Tj ET"), "rawdict"
    )["chars"][0]
    ul, ur, ll, lr = (
        char["quad"][0:2],
        char["quad"][2:4],
        char["quad"][4:6],
        char["quad"][6:8],
    )

    # PyMuPDF 1.28.2's XML oracle labels the glyph cell in its y-up text-space
    # frame before transforming it.  After a 90-degree run rotation, `ul` is
    # therefore not the visually topmost point: ul->ur follows the baseline
    # upward, while ul->ll points right.  Keep that topology even though our
    # Helvetica fallback metrics produce different absolute coordinates.
    assert math.isclose(ul[0], ur[0], abs_tol=1e-9)
    assert ur[1] < ul[1]
    assert ll[0] > ul[0]
    assert math.isclose(ll[1], ul[1], abs_tol=1e-9)
    assert math.isclose(lr[0], ll[0], abs_tol=1e-9)
    assert math.isclose(lr[1], ur[1], abs_tol=1e-9)
