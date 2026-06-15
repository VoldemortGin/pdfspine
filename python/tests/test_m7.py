"""M7 Python surface tests (PRD §7 / §9.5).

``PYTABLE-*`` exercises ``page.find_tables`` / ``Table`` (extract / markdown /
html, including merged-header colspan); ``PYOCG-*`` the optional-content
(layers) round-trip; ``PYSVG-*`` ``page.get_svg_image``; ``PYFITZ-M7-*`` the
``fitz`` / ``pymupdf`` shim parity (camelCase aliases).

All fixtures are self-generated in-test (raw PDF bytes via ``stream=``) — no
external/PyMuPDF files (PRD §10).
"""

from __future__ import annotations

import oxide_pdf
import pytest


# --- self-generated PDF assembler (classic xref) --------------------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int, extra_trailer: bytes = b"") -> bytes:
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
    out += b"trailer\n"
    out += f"<< /Size {size} /Root {root} 0 R {extra_trailer.decode()} >>\n".encode()
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return bytes(out)


def _font() -> bytes:
    widths = b"[" + b" ".join(b"500" for _ in range(32, 126)) + b"]"
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 125 /Widths "
        + widths
        + b" >>"
    )


def _page_pdf(content: bytes, mediabox: str = "[0 0 612 792]") -> bytes:
    stream = f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream"
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>"),
            (3, _font()),
            (
                4,
                b"<< /Type /Page /Parent 2 0 R /MediaBox "
                + mediabox.encode()
                + b" /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>",
            ),
            (5, stream),
        ],
        1,
    )


def _ruled_table_pdf() -> bytes:
    """A 2-row × 3-col ruled grid with a label in each cell (user space, y-up)."""
    c = "1 w\n"
    for y in (700, 670, 640):
        c += f"100 {y} m 400 {y} l S\n"
    for x in (100, 200, 300, 400):
        c += f"{x} 640 m {x} 700 l S\n"
    c += "BT /F1 10 Tf\n"
    for x, y, t in [
        (110, 685, "A1"),
        (210, 685, "B1"),
        (310, 685, "C1"),
        (110, 655, "A2"),
        (210, 655, "B2"),
        (310, 655, "C2"),
    ]:
        c += f"1 0 0 1 {x} {y} Tm ({t}) Tj\n"
    c += "ET\n"
    return _page_pdf(c.encode())


def _merged_header_pdf() -> bytes:
    """A table whose top row spans both columns (a colspan=2 merged header).

    Grid lines: outer box + a middle horizontal split + a vertical split that
    only exists in the lower (body) band, so the top row is one merged cell.
    """
    c = "1 w\n"
    # horizontal rulings: top (700), middle (670), bottom (640)
    for y in (700, 670, 640):
        c += f"100 {y} m 300 {y} l S\n"
    # vertical rulings: left + right span the full height; the middle split only
    # exists in the lower band (640..670), so the header row (670..700) is merged.
    c += "100 640 m 100 700 l S\n"
    c += "300 640 m 300 700 l S\n"
    c += "200 640 m 200 670 l S\n"
    c += "BT /F1 10 Tf\n"
    for x, y, t in [
        (150, 685, "HEAD"),
        (110, 655, "L"),
        (210, 655, "R"),
    ]:
        c += f"1 0 0 1 {x} {y} Tm ({t}) Tj\n"
    c += "ET\n"
    return _page_pdf(c.encode())


_BLANK_PDF = (
    b"%PDF-1.7\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
    b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n"
    b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n"
    b"trailer<</Root 1 0 R>>\n%%EOF"
)


# === PYTABLE — find_tables / Table ========================================


def test_pytable_001_find_tables_detects_grid():
    doc = oxide_pdf.open(stream=_ruled_table_pdf())
    page = doc.load_page(0)
    finder = page.find_tables()
    assert len(finder) == 1
    assert len(finder.tables) == 1
    table = finder.tables[0]
    assert table.row_count == 2
    assert table.col_count == 3
    assert isinstance(table.bbox, oxide_pdf.Rect)


def test_pytable_002_extract_returns_cell_strings():
    doc = oxide_pdf.open(stream=_ruled_table_pdf())
    grid = doc.load_page(0).find_tables().tables[0].extract()
    assert grid == [["A1", "B1", "C1"], ["A2", "B2", "C2"]]


def test_pytable_003_to_markdown_shape():
    doc = oxide_pdf.open(stream=_ruled_table_pdf())
    md = doc.load_page(0).find_tables().tables[0].to_markdown()
    assert "A1" in md and "C2" in md
    assert "|" in md  # pipe-delimited GFM


def test_pytable_004_to_html_shape():
    doc = oxide_pdf.open(stream=_ruled_table_pdf())
    html = doc.load_page(0).find_tables().tables[0].to_html()
    assert "<table" in html
    assert "<td" in html or "<th" in html
    assert "A1" in html


def test_pytable_005_merged_header_colspan():
    doc = oxide_pdf.open(stream=_merged_header_pdf())
    finder = doc.load_page(0).find_tables()
    assert len(finder) == 1
    table = finder.tables[0]
    html = table.to_html()
    assert "colspan" in html, f"merged header should emit colspan: {html}"


def test_pytable_006_finder_iterable_and_indexable():
    doc = oxide_pdf.open(stream=_ruled_table_pdf())
    finder = doc.load_page(0).find_tables()
    tables_iter = list(finder)
    assert len(tables_iter) == 1
    assert finder[0].row_count == 2
    assert finder[0].col_count == 3


def test_pytable_007_text_strategy():
    doc = oxide_pdf.open(stream=_ruled_table_pdf())
    finder = doc.load_page(0).find_tables(strategy="text")
    # text strategy may or may not match the same grid, but must never raise and
    # must return a TableFinder.
    assert isinstance(finder, oxide_pdf.TableFinder)


# === PYOCG — optional content / layers ====================================


def test_pyocg_001_add_save_reopen_roundtrip(tmp_path):
    doc = oxide_pdf.open(stream=_BLANK_PDF)
    assert doc.get_ocgs() == {}
    xref = doc.add_ocg("Layer1")
    assert isinstance(xref, int)

    out = tmp_path / "ocg.pdf"
    doc.save(out, garbage=1)

    re = oxide_pdf.open(out)
    ocgs = re.get_ocgs()
    assert len(ocgs) == 1
    assert ocgs[xref]["name"] == "Layer1"
    assert ocgs[xref]["on"] is True

    configs = re.layer_ui_configs()
    assert any(c["text"] == "Layer1" for c in configs)


def test_pyocg_002_set_layer_off():
    doc = oxide_pdf.open(stream=_BLANK_PDF)
    xref = doc.add_ocg("Layer1")
    assert doc.ocg_state(xref) is True
    doc.set_layer(off=[xref])
    assert doc.ocg_state(xref) is False
    # get_layer reflects the off state
    state = doc.get_layer()
    assert xref in state["off"]
    assert xref not in state["on"]


def test_pyocg_003_add_off_initial():
    doc = oxide_pdf.open(stream=_BLANK_PDF)
    xref = doc.add_ocg("Hidden", on=False)
    assert doc.ocg_state(xref) is False


# === PYSVG — get_svg_image ================================================


def test_pysvg_001_wellformed():
    doc = oxide_pdf.open(stream=_BLANK_PDF)
    svg = doc.load_page(0).get_svg_image()
    assert svg.startswith("<?xml") or svg.startswith("<svg")
    assert "<svg" in svg
    assert "</svg>" in svg


def test_pysvg_002_matrix_arg():
    doc = oxide_pdf.open(stream=_ruled_table_pdf())
    svg = doc.load_page(0).get_svg_image(matrix=oxide_pdf.Matrix(2, 0, 0, 2, 0, 0))
    assert "<svg" in svg


# === PYFITZ-M7 — fitz / pymupdf shim parity ===============================


def test_pyfitz_m7_001_find_tables_alias():
    import fitz

    doc = fitz.open(stream=_ruled_table_pdf())
    page = doc.load_page(0)
    # snake_case + camelCase parity
    f1 = page.find_tables()
    f2 = page.findTables()
    assert len(f1) == len(f2) == 1
    assert isinstance(f1, fitz.TableFinder)
    assert isinstance(f1.tables[0], fitz.Table)
    assert f1.tables[0].extract() == [["A1", "B1", "C1"], ["A2", "B2", "C2"]]


def test_pyfitz_m7_002_svg_alias():
    import fitz

    doc = fitz.open(stream=_BLANK_PDF)
    page = doc.load_page(0)
    s1 = page.get_svg_image()
    s2 = page.getSVGimage()
    assert "<svg" in s1
    assert "<svg" in s2


def test_pyfitz_m7_003_ocg_aliases(tmp_path):
    import fitz

    doc = fitz.open(stream=_BLANK_PDF)
    xref = doc.addOCG("LayerA")
    out = tmp_path / "f.pdf"
    doc.save(out)
    re = fitz.open(out)
    assert xref in re.getOCGs()
    assert any(c["text"] == "LayerA" for c in re.layerUIConfigs())
    re.setLayer(off=[xref])
    assert re.ocg_state(xref) is False


def test_pyfitz_m7_004_pymupdf_table_objects():
    import pymupdf

    doc = pymupdf.open(stream=_ruled_table_pdf())
    md = doc.load_page(0).find_tables().tables[0].to_markdown()
    assert "A1" in md
