"""M1f Python read-surface tests (PRD §7 / §9.2 / §9.4 / §9.5).

`PYDOC-*` exercise the native ``oxipdf`` package; `PYFITZ-*` the ``fitz`` shim.
All fixtures are self-generated in-test (raw PDF bytes written to a tmp file or
passed via ``stream=``) — no external/PyMuPDF files (PRD §10).
"""

from __future__ import annotations

import oxipdf
import pytest


# --- self-generated PDF fixtures (raw bytes) ------------------------------


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


def two_page_pdf() -> bytes:
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                b"<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] "
                b"/MediaBox [0 0 200 300] >>",
            ),
            (3, b"<< /Type /Page /Parent 2 0 R /Rotate 90 >>"),
            (4, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] >>"),
            (
                5,
                b"<< /Title (Hello Title) /Author (Jane Doe) "
                b"/Producer (oxipdf-test) /CreationDate (D:20240101000000Z) >>",
            ),
        ],
        root=1,
        extra_trailer=b"/Info 5 0 R",
    )


@pytest.fixture()
def two_page_path(tmp_path):
    p = tmp_path / "two_page.pdf"
    p.write_bytes(two_page_pdf())
    return str(p)


# --- PYDOC-* (native oxipdf) ----------------------------------------------


def test_pydoc_001_open_and_pages(two_page_path):
    # PYDOC-001
    doc = oxipdf.open(two_page_path)
    assert doc.page_count == 2
    assert len(doc) == 2
    page = doc.load_page(0)
    assert page.number == 0
    assert doc[1].number == 1
    assert doc[-1].number == 1  # negative index
    pages = list(doc)
    assert [p.number for p in pages] == [0, 1]


def test_pydoc_001_open_stream():
    # PYDOC-001 (stream= variant)
    doc = oxipdf.open(stream=two_page_pdf())
    assert doc.page_count == 2


def test_pydoc_002_page_geometry(two_page_path):
    # PYDOC-002
    doc = oxipdf.open(two_page_path)
    p0 = doc[0]
    assert tuple(p0.rect) == (0.0, 0.0, 200.0, 300.0)
    assert tuple(p0.bound()) == (0.0, 0.0, 200.0, 300.0)
    assert tuple(p0.mediabox) == (0.0, 0.0, 200.0, 300.0)
    assert tuple(p0.cropbox) == (0.0, 0.0, 200.0, 300.0)
    assert p0.rotation == 90
    assert p0.rect.width == 200.0
    assert p0.rect.height == 300.0

    p1 = doc[1]
    assert tuple(p1.rect) == (0.0, 0.0, 400.0, 400.0)
    assert p1.rotation == 0


def test_pydoc_003_metadata_keys(two_page_path):
    # PYDOC-003
    doc = oxipdf.open(two_page_path)
    md = doc.metadata
    for key in (
        "format",
        "title",
        "author",
        "subject",
        "keywords",
        "creator",
        "producer",
        "creationDate",
        "modDate",
        "trapped",
        "encryption",
    ):
        assert key in md, key
    assert md["format"] == "PDF 1.7"
    assert md["title"] == "Hello Title"
    assert md["author"] == "Jane Doe"
    assert md["producer"] == "oxipdf-test"
    assert md["subject"] == ""  # absent → empty (PyMuPDF)
    assert md["encryption"] == ""


def test_pydoc_004_unimplemented_raises(two_page_path):
    # PYDOC-004: a known-but-unimplemented method raises PdfUnsupportedError.
    doc = oxipdf.open(two_page_path)
    page = doc[0]
    with pytest.raises(oxipdf.PdfUnsupportedError):
        page.get_pixmap()
    with pytest.raises(oxipdf.PdfUnsupportedError):
        doc.convert_to_pdf()
    # get_toc is now implemented (M3d): a doc with no /Outlines returns [].
    assert doc.get_toc() == []
    # An attribute that does not exist at all is still AttributeError.
    with pytest.raises(AttributeError):
        page.totally_made_up_attribute


def test_pydoc_xref_api(two_page_path):
    doc = oxipdf.open(two_page_path)
    assert doc.xref_length() == 6  # max obj 5 + 1
    assert "/Catalog" in doc.xref_object(1)
    assert doc.xref_get_key(3, "Type") == "/Page"
    assert doc.xref_is_stream(3) is False


def test_pydoc_repaired(tmp_path):
    # is_repaired flips when the file needs reconstruction.
    bytes_ = two_page_pdf()
    bytes_ = bytes_[: bytes_.rfind(b"startxref")]
    p = tmp_path / "broken.pdf"
    p.write_bytes(bytes_)
    doc = oxipdf.open(str(p))
    assert doc.is_repaired is True
    assert doc.page_count == 2


# --- PYFITZ-* (fitz shim) --------------------------------------------------


def test_pyfitz_001_open_and_metadata(two_page_path):
    # PYFITZ-001
    import fitz

    doc = fitz.open(two_page_path)
    assert doc.page_count == 2
    assert doc[0].number == 0
    md = doc.metadata
    assert md["format"] == "PDF 1.7"
    assert md["title"] == "Hello Title"
    assert tuple(doc[1].rect) == (0.0, 0.0, 400.0, 400.0)
    assert doc[0].rotation == 90


def test_pyfitz_003_geometry_value_types():
    # PYFITZ-003: fitz.Rect / fitz.Matrix match PyMuPDF arithmetic.
    import fitz

    r = fitz.Rect(0, 0, 10, 20)
    assert r.width == 10.0
    assert r.height == 20.0
    assert tuple(r) == (0.0, 0.0, 10.0, 20.0)
    assert r[2] == 10.0

    assert tuple(fitz.Matrix()) == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
    assert tuple(fitz.Matrix(90)) == (0.0, 1.0, -1.0, 0.0, 0.0, 0.0)
    assert tuple(fitz.Matrix(180)) == (-1.0, 0.0, 0.0, -1.0, 0.0, 0.0)
    # concat
    m = fitz.Matrix(1, 0, 0, 1, 5, 5) * fitz.Matrix(2, 0, 0, 2, 0, 0)
    assert tuple(m) == (2.0, 0.0, 0.0, 2.0, 10.0, 10.0)


def test_pymupdf_alias(two_page_path):
    import pymupdf

    doc = pymupdf.open(two_page_path)
    assert doc.page_count == 2
    assert pymupdf.__version__ == oxipdf.__version__
