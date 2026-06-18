"""Long-tail PyMuPDF parity batch 2 (PRD §7 / §9.5).

Covers the newly-implemented surface:
  - Page.get_image_info / get_image_bbox
  - Pixmap.set_origin / set_dpi / x / y / xres / yres / tint_with /
    gamma_with / color_count / color_topusage / is_monochrome /
    is_unicolor / digest / invert_irect
  - Annot.line_ends / set_line_ends / blendmode / set_blendmode / set_name /
    set_open / is_open / border (dict)
  - Document.set_page_labels / get_char_widths / page_cropbox / page_mediabox /
    fullcopy_page (arbitrary position) / journal_* (enable/undo/redo/can_do)

Both the native ``pdfspine`` API and the ``fitz`` shim are exercised. All
fixtures are self-generated (raw PDF bytes via ``stream=``) — no external files.
"""

from __future__ import annotations

import pdfspine
import fitz
import pytest


# --- self-generated PDF fixtures -----------------------------------------


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
    out += b"xref\n" + f"0 {size}\n".encode() + b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n" + f"<< /Size {size} /Root {root} 0 R >>\n".encode()
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return bytes(out)


def _stream_obj(num: int, dict_body: bytes, data: bytes) -> tuple[int, bytes]:
    body = b"<< " + dict_body + f" /Length {len(data)} >>\nstream\n".encode() + data + b"\nendstream"
    return (num, body)


def image_pdf() -> bytes:
    """A one-page PDF that paints one image XObject ``Im0`` (2x2 RGB)."""
    page_content = b"q 50 0 0 50 100 100 cm /Im0 Do Q\n"
    image_data = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
            b"/CropBox [10 10 190 190] /Contents 4 0 R "
            b"/Resources << /XObject << /Im0 6 0 R >> >> >>",
        ),
        _stream_obj(4, b"", page_content),
        _stream_obj(
            6,
            b"/Type /XObject /Subtype /Image /Width 2 /Height 2 "
            b"/ColorSpace /DeviceRGB /BitsPerComponent 8",
            image_data,
        ),
    ]
    objects.sort(key=lambda o: o[0])
    return _build_pdf(objects, root=1)


def font_pdf() -> bytes:
    """A one-page PDF whose font (object 5) has a /Widths array."""
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
            b"/Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (
            5,
            b"<< /Type /Font /Subtype /TrueType /BaseFont /Arial "
            b"/FirstChar 65 /LastChar 67 /Widths [500 600 700] >>",
        ),
    ]
    objects.sort(key=lambda o: o[0])
    return _build_pdf(objects, root=1)


def multi_page_pdf(n: int) -> bytes:
    objects: list[tuple[int, bytes]] = []
    kids = []
    for i in range(n):
        leaf = 3 + i
        kids.append(f"{leaf} 0 R")
        objects.append(
            (leaf, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>")
        )
    objects.append((1, b"<< /Type /Catalog /Pages 2 0 R >>"))
    objects.append(
        (
            2,
            b"<< /Type /Pages /Count "
            + str(n).encode()
            + b" /Kids ["
            + b" ".join(k.encode() for k in kids)
            + b"] >>",
        )
    )
    objects.sort(key=lambda o: o[0])
    return _build_pdf(objects, root=1)


# === Page.get_image_info / get_image_bbox ================================


def test_lt2_get_image_info():
    doc = pdfspine.open(stream=image_pdf())
    info = doc[0].get_image_info()
    assert len(info) == 1
    e = info[0]
    assert e["name"] == "Im0"
    assert e["xref"] == 6
    assert e["width"] == 2 and e["height"] == 2
    assert e["bpc"] == 8
    assert e["colorspace"] == "DeviceRGB"
    assert e["bbox"].x1 > e["bbox"].x0


def test_lt2_get_image_bbox_by_name_and_xref():
    doc = pdfspine.open(stream=image_pdf())
    by_name = doc[0].get_image_bbox("Im0")
    by_xref = doc[0].get_image_bbox(6)
    assert by_name.x1 > by_name.x0
    assert tuple(by_name) == tuple(by_xref)
    # An unknown name → empty rect.
    missing = doc[0].get_image_bbox("nope")
    assert missing.is_empty


def test_lt2_fitz_get_image_info():
    doc = fitz.open(stream=image_pdf())
    info = doc[0].get_image_info()
    assert len(info) == 1 and info[0]["name"] == "Im0"


# === Pixmap origin / dpi metadata ========================================


def test_lt2_pixmap_origin_dpi():
    pix = pdfspine.Pixmap("rgb", (0, 0, 4, 4))
    assert (pix.x, pix.y) == (0, 0)
    pix.set_origin(5, 7)
    assert (pix.x, pix.y) == (5, 7)
    pix.set_dpi(150, 200)
    assert (pix.xres, pix.yres) == (150, 200)


# === Pixmap.tint_with / gamma_with =======================================


def test_lt2_pixmap_tint_with():
    pix = pdfspine.Pixmap("rgb", (0, 0, 2, 2))
    pix.clear_with(128)
    before = pix.samples
    # black=0 white=0xffffff is the identity tint.
    pix.tint_with(0x000000, 0xFFFFFF)
    assert pix.samples == before
    # Invert via tint (black=white, white=black) flips toward complement.
    pix.tint_with(0xFFFFFF, 0x000000)
    assert pix.samples != before


def test_lt2_pixmap_gamma_with_identity_noop():
    pix = pdfspine.Pixmap("rgb", (0, 0, 2, 2))
    pix.clear_with(100)
    before = pix.samples
    pix.gamma_with(1.0)
    assert pix.samples == before
    pix.gamma_with(0.5)
    assert pix.samples != before


# === Pixmap.color_count / color_topusage / is_* ==========================


def test_lt2_pixmap_color_count_and_topusage():
    pix = pdfspine.Pixmap("rgb", (0, 0, 2, 2))
    pix.clear_with(0)  # all black → one color
    assert pix.color_count() == 1
    ratio, pixel = pix.color_topusage()
    assert ratio == pytest.approx(1.0)
    assert pixel == bytes([0, 0, 0])
    # Paint one pixel white → two colors.
    pix.set_pixel(0, 0, [255, 255, 255])
    assert pix.color_count() == 2
    ratio2, pixel2 = pix.color_topusage()
    assert ratio2 == pytest.approx(0.75)
    assert pixel2 == bytes([0, 0, 0])


def test_lt2_pixmap_is_unicolor_monochrome():
    pix = pdfspine.Pixmap("rgb", (0, 0, 2, 2))
    pix.clear_with(0)
    assert pix.is_unicolor
    assert pix.is_monochrome  # all black
    pix.set_pixel(0, 0, [255, 255, 255])
    assert not pix.is_unicolor
    assert pix.is_monochrome  # black + white only
    pix.set_pixel(1, 1, [10, 20, 30])
    assert not pix.is_monochrome


def test_lt2_pixmap_digest_determinism():
    a = pdfspine.Pixmap("rgb", (0, 0, 2, 2))
    a.clear_with(50)
    b = pdfspine.Pixmap("rgb", (0, 0, 2, 2))
    b.clear_with(50)
    assert a.digest() == b.digest()
    assert len(a.digest()) == 16
    b.set_pixel(0, 0, [1, 2, 3])
    assert a.digest() != b.digest()


def test_lt2_pixmap_invert_irect():
    pix = pdfspine.Pixmap("rgb", (0, 0, 2, 2))
    pix.clear_with(0)
    pix.invert_irect()
    assert pix.pixel(0, 0) == (255, 255, 255)


# === Annot line ends / blend / name / open / border ======================


def _annot_doc():
    doc = pdfspine.open(stream=multi_page_pdf(1))
    page = doc[0]
    annot = page.add_line_annot((20, 20), (80, 80))
    return doc, page, annot


def test_lt2_annot_line_ends():
    doc, page, annot = _annot_doc()
    annot.set_line_ends(4, 5)  # OpenArrow, ClosedArrow
    assert annot.line_ends == (4, 5)


def test_lt2_annot_blendmode():
    doc, page, annot = _annot_doc()
    assert annot.blendmode is None
    annot.set_blendmode("Multiply")
    assert annot.blendmode == "Multiply"


def test_lt2_annot_open_and_name():
    doc, page, annot = _annot_doc()
    assert annot.is_open is False
    annot.set_open(True)
    assert annot.is_open is True
    annot.set_name("Comment")  # should not raise


def test_lt2_annot_border_dict():
    doc, page, annot = _annot_doc()
    annot.set_border(width=2.5)
    b = annot.border
    assert b["width"] == pytest.approx(2.5)
    assert b["style"] == "S"
    assert b["dashes"] == []


# === Document.get_char_widths ============================================


def test_lt2_get_char_widths():
    doc = pdfspine.open(stream=font_pdf())
    widths = doc.get_char_widths(5)
    assert len(widths) == 3
    assert widths[0] == (65, pytest.approx(0.5))
    assert widths[1] == (66, pytest.approx(0.6))
    assert widths[2] == (67, pytest.approx(0.7))


# === Document.page_cropbox / page_mediabox ===============================


def test_lt2_page_boxes():
    doc = pdfspine.open(stream=image_pdf())
    mb = doc.page_mediabox(0)
    cb = doc.page_cropbox(0)
    assert tuple(mb) == (0.0, 0.0, 200.0, 200.0)
    assert tuple(cb) == (10.0, 10.0, 190.0, 190.0)


# === Document.set_page_labels ============================================


def test_lt2_set_page_labels_roundtrip():
    doc = pdfspine.open(stream=multi_page_pdf(5))
    doc.set_page_labels(
        [
            {"startpage": 0, "style": "r", "prefix": "", "firstpagenum": 1},
            {"startpage": 3, "style": "D", "prefix": "A-", "firstpagenum": 1},
        ]
    )
    assert doc.get_page_label(0) == "i"
    assert doc.get_page_label(2) == "iii"
    assert doc.get_page_label(3) == "A-1"
    assert doc.get_page_label(4) == "A-2"
    # Empty list removes labels.
    doc.set_page_labels([])
    assert doc.get_page_label(0) == ""


def test_lt2_fitz_set_page_labels():
    doc = fitz.open(stream=multi_page_pdf(3))
    doc.set_page_labels([{"startpage": 0, "style": "A", "prefix": "", "firstpagenum": 1}])
    assert doc.get_page_label(0) == "A"
    assert doc.get_page_label(1) == "B"


# === Document.fullcopy_page (arbitrary position) =========================


def test_lt2_fullcopy_page_insert_position():
    doc = pdfspine.open(stream=multi_page_pdf(3))
    assert doc.page_count == 3
    # Copy page 0 and insert it at position 1.
    doc.fullcopy_page(0, to=1)
    assert doc.page_count == 4


def test_lt2_fullcopy_page_append_default():
    doc = pdfspine.open(stream=multi_page_pdf(2))
    doc.fullcopy_page(0)  # default to=-1 appends
    assert doc.page_count == 3


# === Document.journal_* ==================================================


def test_lt2_journal_undo_redo():
    doc = pdfspine.open(stream=multi_page_pdf(2))
    assert doc.journal_is_enabled() is False
    doc.journal_enable()
    assert doc.journal_is_enabled() is True
    assert doc.journal_can_undo() is False

    base = doc.page_count
    doc.fullcopy_page(0)  # mutate
    doc.journal_save_state()
    assert doc.page_count == base + 1
    assert doc.journal_can_undo() is True
    assert doc.journal_can_redo() is False

    assert doc.journal_undo() is True
    assert doc.page_count == base
    assert doc.journal_can_redo() is True

    assert doc.journal_redo() is True
    assert doc.page_count == base + 1


def test_lt2_journal_can_do_dict():
    doc = pdfspine.open(stream=multi_page_pdf(2))
    doc.journal_enable()
    cando = doc.journal_can_do()
    assert cando == {"undo": False, "redo": False}
    doc.fullcopy_page(0)
    doc.journal_save_state()
    assert doc.journal_can_do()["undo"] is True
