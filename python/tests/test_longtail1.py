"""Long-tail PyMuPDF parity batch 1 (PRD §7 / §9.5).

Covers the newly-implemented Document/Page/Pixmap getters and ops:
  - Document.pages / reload_page / page_xref / get_page_xobjects
  - Document.resolve_link / fullcopy_page / chapter_count /
    chapter_page_count / last_location
  - Page.get_xobjects / get_image_rects / get_contents / read_contents /
    show_pdf_page
  - Pixmap.copy / set_rect / shrink / pil_tobytes / pil_save

Both the native ``oxide_pdf`` API and the ``fitz`` shim are exercised. All
fixtures are self-generated (raw PDF bytes via ``stream=``) — no external files.
"""

from __future__ import annotations

import oxide_pdf
import fitz
import pytest


# --- self-generated PDF fixtures -----------------------------------------


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


def _stream_obj(num: int, dict_body: bytes, data: bytes) -> tuple[int, bytes]:
    body = b"<< " + dict_body + f" /Length {len(data)} >>\nstream\n".encode() + data + b"\nendstream"
    return (num, body)


def xobject_pdf() -> bytes:
    """A one-page PDF whose page paints a Form XObject and an Image XObject.

    Object map: 1 catalog, 2 pages, 3 page leaf, 4 page content, 5 Form XObject,
    6 Image XObject, 7 font (shared by the form).
    """
    page_content = (
        b"BT /F1 12 Tf 20 150 Td (Top) Tj ET\n"
        b"q 80 0 0 60 20 20 cm /Fm0 Do Q\n"
        b"q 50 0 0 50 100 100 cm /Im0 Do Q\n"
    )
    form_content = b"BT /F1 10 Tf 2 2 Td (form) Tj ET"
    # 2x2 RGB image (12 bytes) so /Width 2 /Height 2 decodes cleanly.
    image_data = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
            b"/Contents 4 0 R /Resources << /Font << /F1 7 0 R >> "
            b"/XObject << /Fm0 5 0 R /Im0 6 0 R >> >> >>",
        ),
        _stream_obj(4, b"", page_content),
        _stream_obj(
            5,
            b"/Type /XObject /Subtype /Form /BBox [0 0 100 100] "
            b"/Resources << /Font << /F1 7 0 R >> >>",
            form_content,
        ),
        _stream_obj(
            6,
            b"/Type /XObject /Subtype /Image /Width 2 /Height 2 "
            b"/ColorSpace /DeviceRGB /BitsPerComponent 8",
            image_data,
        ),
        (7, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ]
    objects.sort(key=lambda o: o[0])
    return _build_pdf(objects, root=1)


def named_dest_pdf() -> bytes:
    """A two-page PDF with a catalog ``/Dests`` named destination ``Chapter2``
    that points at page 2 (object 4)."""
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R /Dests 5 0 R >>"),
        (2, b"<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 200 200] >>"),
        (3, b"<< /Type /Page /Parent 2 0 R >>"),
        (4, b"<< /Type /Page /Parent 2 0 R >>"),
        (5, b"<< /Chapter2 [4 0 R /XYZ 0 200 0] >>"),
    ]
    return _build_pdf(objects, root=1)


def multi_page_pdf(markers: list[str]) -> bytes:
    objects: list[tuple[int, bytes]] = [
        (3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ]
    kids = []
    for i, marker in enumerate(markers):
        leaf = 4 + i * 2
        content = leaf + 1
        kids.append(f"{leaf} 0 R")
        body = f"BT /F1 12 Tf 20 100 Td ({marker}) Tj ET".encode()
        objects.append(
            (
                leaf,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
                + f"/Contents {content} 0 R ".encode()
                + b"/Resources << /Font << /F1 3 0 R >> >> >>",
            )
        )
        objects.append(_stream_obj(content, b"", body))
    objects.append((1, b"<< /Type /Catalog /Pages 2 0 R >>"))
    objects.append(
        (
            2,
            b"<< /Type /Pages /Count "
            + str(len(markers)).encode()
            + b" /Kids ["
            + b" ".join(k.encode() for k in kids)
            + b"] >>",
        )
    )
    objects.sort(key=lambda o: o[0])
    return _build_pdf(objects, root=1)


# --- Page.get_xobjects ----------------------------------------------------


def test_lt1_page_get_xobjects():
    doc = oxide_pdf.open(stream=xobject_pdf())
    xobjs = doc[0].get_xobjects()
    by_name = {x[1]: x for x in xobjs}
    assert set(by_name) == {"Fm0", "Im0"}
    xref, name, kind, bbox, matrix, ref = by_name["Fm0"]
    assert kind == "Form"
    assert tuple(bbox) == (0.0, 0.0, 100.0, 100.0)
    assert xref == 5
    assert ref == 3  # the page object number
    assert by_name["Im0"][2] == "Image"
    # Matrix on a Form without /Matrix is identity.
    assert tuple(by_name["Fm0"][4]) == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


def test_lt1_document_get_page_xobjects_matches_page():
    doc = oxide_pdf.open(stream=xobject_pdf())
    doc_view = {x[1] for x in doc.get_page_xobjects(0)}
    page_view = {x[1] for x in doc[0].get_xobjects()}
    assert doc_view == page_view == {"Fm0", "Im0"}


def test_lt1_fitz_get_xobjects():
    doc = fitz.open(stream=xobject_pdf())
    names = {x[1] for x in doc[0].get_xobjects()}
    assert names == {"Fm0", "Im0"}


# --- Page.get_image_rects -------------------------------------------------


def test_lt1_page_get_image_rects():
    doc = oxide_pdf.open(stream=xobject_pdf())
    rects = doc[0].get_image_rects()
    assert len(rects) >= 1
    # Each rect is a non-degenerate Rect.
    r = rects[0]
    assert r.x1 > r.x0 and r.y1 > r.y0


# --- Page.get_contents / read_contents ------------------------------------


def test_lt1_page_get_contents_xref():
    doc = oxide_pdf.open(stream=xobject_pdf())
    contents = doc[0].get_contents()
    assert contents == [4]


def test_lt1_page_read_contents_bytes():
    doc = oxide_pdf.open(stream=xobject_pdf())
    raw = doc[0].read_contents()
    assert isinstance(raw, bytes)
    assert b"/Fm0 Do" in raw
    assert b"/Im0 Do" in raw


def test_lt1_fitz_read_contents():
    doc = fitz.open(stream=xobject_pdf())
    assert b"Do" in doc[0].read_contents()


# --- Page.show_pdf_page ---------------------------------------------------


def test_lt1_show_pdf_page_places_form():
    dst = oxide_pdf.open(stream=multi_page_pdf(["DST"]))
    src = oxide_pdf.open(stream=multi_page_pdf(["SRC"]))
    name = dst[0].show_pdf_page((10, 10, 110, 110), src, 0)
    assert name.startswith("Fm")
    # The destination page now references a Form XObject.
    xobjs = {x[1]: x[2] for x in dst[0].get_xobjects()}
    assert name in xobjs and xobjs[name] == "Form"
    # The placement Do is in the content.
    assert f"/{name} Do".encode() in dst[0].read_contents()


def test_lt1_show_pdf_page_roundtrips():
    dst = oxide_pdf.open(stream=multi_page_pdf(["DST"]))
    src = oxide_pdf.open(stream=multi_page_pdf(["SRC"]))
    dst[0].show_pdf_page((10, 10, 110, 110), src, 0)
    re = oxide_pdf.open(stream=dst.tobytes())
    assert re.page_count == 1
    # The grafted form is intact after a full save/reopen.
    assert any(x[2] == "Form" for x in re[0].get_xobjects())


def test_lt1_show_pdf_page_out_of_range():
    dst = oxide_pdf.open(stream=multi_page_pdf(["DST"]))
    src = oxide_pdf.open(stream=multi_page_pdf(["SRC"]))
    with pytest.raises(Exception):
        dst[0].show_pdf_page((0, 0, 50, 50), src, 9)


# --- Document.pages / reload_page / page_xref -----------------------------


def test_lt1_document_pages_iterator():
    doc = oxide_pdf.open(stream=multi_page_pdf(["A", "B", "C"]))
    nums = [p.number for p in doc.pages()]
    assert nums == [0, 1, 2]


def test_lt1_document_reload_page():
    doc = oxide_pdf.open(stream=multi_page_pdf(["A", "B"]))
    page = doc[0]
    reloaded = doc.reload_page(page)
    assert reloaded.number == 0
    # Also accepts an int index.
    assert doc.reload_page(1).number == 1


def test_lt1_document_page_xref():
    doc = oxide_pdf.open(stream=multi_page_pdf(["A", "B"]))
    assert doc.page_xref(0) == 4
    assert doc.page_xref(1) == 6
    assert doc.page_xref(-1) == 6


def test_lt1_fitz_pages_and_page_xref():
    doc = fitz.open(stream=multi_page_pdf(["A", "B"]))
    assert [p.number for p in doc.pages()] == [0, 1]
    assert doc.page_xref(0) == 4


# --- Document.resolve_link ------------------------------------------------


def test_lt1_resolve_link_named_dest():
    doc = oxide_pdf.open(stream=named_dest_pdf())
    assert doc.resolve_link("Chapter2") == 1
    assert doc.resolve_link("Missing") is None


def test_lt1_resolve_link_page_fragment():
    doc = oxide_pdf.open(stream=multi_page_pdf(["A", "B", "C"]))
    assert doc.resolve_link("file.pdf#page=2") == 1
    assert doc.resolve_link("#3") == 2
    assert doc.resolve_link("#page=99") is None


# --- Document.fullcopy_page -----------------------------------------------


def test_lt1_fullcopy_page_appends_independent():
    doc = oxide_pdf.open(stream=multi_page_pdf(["AAA", "BBB"]))
    assert doc.page_count == 2
    doc.fullcopy_page(0)
    assert doc.page_count == 3
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.page_count == 3
    assert "AAA" in re[2].get_text()


def test_lt1_fullcopy_page_to_position():
    # fullcopy_page now supports an explicit insert position (long-tail batch 2).
    doc = oxide_pdf.open(stream=multi_page_pdf(["A", "B"]))
    doc.fullcopy_page(0, to=1)
    assert doc.page_count == 3


# --- Document chapter / location model ------------------------------------


def test_lt1_chapter_location_model():
    doc = oxide_pdf.open(stream=multi_page_pdf(["A", "B", "C"]))
    assert doc.chapter_count == 1
    assert doc.chapter_page_count(0) == 3
    assert doc.chapter_page_count(1) == 0
    assert doc.last_location == (0, 2)


def test_lt1_fitz_chapter_location():
    doc = fitz.open(stream=multi_page_pdf(["A", "B"]))
    assert doc.chapter_count == 1
    assert doc.last_location == (0, 1)


# --- Pixmap accessors -----------------------------------------------------


def _blank_pixmap(w: int = 8, h: int = 8):
    return oxide_pdf.Pixmap(3, (0, 0, w, h))


def test_lt1_pixmap_copy_independent():
    pix = _blank_pixmap()
    pix.clear_with(0)
    cp = pix.copy()
    cp.set_pixel(0, 0, [255, 0, 0])
    # The original is untouched (copy-on-write).
    assert pix.pixel(0, 0) == (0, 0, 0)
    assert cp.pixel(0, 0) == (255, 0, 0)


def test_lt1_pixmap_set_rect():
    pix = _blank_pixmap(8, 8)
    pix.clear_with(0)
    wrote = pix.set_rect((1, 1, 4, 4), [10, 20, 30])
    assert wrote is True
    assert pix.pixel(2, 2) == (10, 20, 30)
    assert pix.pixel(0, 0) == (0, 0, 0)  # outside the rect


def test_lt1_pixmap_shrink():
    pix = _blank_pixmap(8, 8)
    pix.shrink(1)
    assert pix.width == 4 and pix.height == 4
    pix.shrink(2)
    assert pix.width == 1 and pix.height == 1


def test_lt1_pixmap_pil_tobytes_and_save(tmp_path):
    pix = _blank_pixmap(4, 4)
    pix.clear_with(128)
    data = pix.pil_tobytes("PNG")
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    out = tmp_path / "pix.png"
    pix.pil_save(str(out))
    assert out.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"


def test_lt1_fitz_pixmap_pil_tobytes():
    pix = fitz.Pixmap(3, (0, 0, 4, 4))
    pix.clear_with(200)
    assert pix.pil_tobytes("PNG")[:8] == b"\x89PNG\r\n\x1a\n"
