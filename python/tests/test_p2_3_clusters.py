"""P2-3 — small binding clusters (Pixmap / Tools / Page / Document).

Covers the newly-implemented PyMuPDF baseline symbols:

* ``Pixmap.samples_ptr`` + ``Pixmap.__array_interface__`` — the raw-address /
  numpy-array-interface views over the buffer-protocol samples;
* ``Tools.image_profile`` + module-level ``image_profile`` — the raster header
  profile dict (fitz shape);
* ``Page.language`` / ``Page.set_language`` — inheritable ``/Lang`` get/set;
* ``Page.set_contents`` — point ``/Contents`` at a stream xref;
* ``Document.get_outline_xrefs`` — the outline-item xref walk;
* ``Document.embfile_upd`` — in-place embedded-file update.
"""

from __future__ import annotations

import ctypes

import pdfspine
import pytest

from pdfspine._core import PdfError


# ---------------------------------------------------------------------------
# Pixmap.samples_ptr + __array_interface__
# ---------------------------------------------------------------------------
def test_pixmap_samples_ptr_addresses_buffer() -> None:
    pix = pdfspine.Pixmap(pdfspine.csRGB, (0, 0, 4, 3), False)
    pix.clear_with(123)
    ptr = pix.samples_ptr
    assert isinstance(ptr, int) and ptr != 0
    # The address really points at the sample bytes.
    buf = (ctypes.c_ubyte * len(pix.samples)).from_address(ptr)
    assert bytes(buf) == bytes(pix.samples)


def test_pixmap_array_interface_shape_and_data() -> None:
    pix = pdfspine.Pixmap(pdfspine.csRGB, (0, 0, 4, 3), False)
    ai = pix.__array_interface__
    assert ai["shape"] == (pix.height, pix.width, pix.n) == (3, 4, 3)
    assert ai["typestr"] == "|u1"
    assert ai["version"] == 3
    addr, readonly = ai["data"]
    assert addr == pix.samples_ptr
    assert readonly is True


def test_pixmap_array_interface_gray_alpha_n() -> None:
    pix = pdfspine.Pixmap(pdfspine.csGRAY, (0, 0, 2, 2), True)
    # gray + alpha => n == 2.
    assert pix.__array_interface__["shape"] == (2, 2, 2)


# ---------------------------------------------------------------------------
# Tools.image_profile + module-level image_profile
# ---------------------------------------------------------------------------
def _png_bytes(w: int = 4, h: int = 3, value: int = 200) -> bytes:
    pix = pdfspine.Pixmap(pdfspine.csRGB, (0, 0, w, h), False)
    pix.clear_with(value)
    return pix.tobytes("png")


# A minimal baseline grayscale 1x1 JPEG (SOI + APP0 + DQT + SOF0 + EOI).
_GRAY_JPEG = bytes.fromhex(
    "ffd8ffe000104a46494600010100000100010000"
    "ffdb004300" + "08" * 64 + "ffc0000b080001000101011100" + "ffd9"
)


def test_module_image_profile_png_shape() -> None:
    prof = pdfspine.image_profile(_png_bytes())
    assert prof is not None
    assert set(prof) == {
        "width",
        "height",
        "orientation",
        "transform",
        "xres",
        "yres",
        "colorspace",
        "bpc",
        "ext",
        "cs-name",
    }
    assert (prof["width"], prof["height"]) == (4, 3)
    assert prof["ext"] == "png"
    assert prof["colorspace"] == 3
    assert prof["cs-name"] == "DeviceRGB"
    assert prof["bpc"] == 8
    assert prof["transform"] == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


def test_tools_image_profile_matches_module() -> None:
    png = _png_bytes()
    assert pdfspine.TOOLS.image_profile(png) == pdfspine.image_profile(png)


def test_image_profile_jpeg_grayscale() -> None:
    prof = pdfspine.image_profile(_GRAY_JPEG)
    assert prof is not None
    assert prof["ext"] == "jpeg"
    assert prof["colorspace"] == 1
    assert prof["cs-name"] == "DeviceGray"
    assert (prof["width"], prof["height"]) == (1, 1)


@pytest.mark.parametrize("data", [b"", b"abc", b"not an image at all 1234"])
def test_image_profile_unrecognized_returns_none(data: bytes) -> None:
    assert pdfspine.image_profile(data) is None
    assert pdfspine.TOOLS.image_profile(data) is None


# ---------------------------------------------------------------------------
# Page.language / Page.set_language
# ---------------------------------------------------------------------------
def test_page_language_absent_is_none() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    assert page.language is None
    doc.close()


def test_page_set_language_normalizes() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    page.set_language("en-US")
    assert page.language == "en"  # MuPDF compact ISO-639 form
    page.set_language("zh-CN")
    assert page.language == "zh-Hans"
    doc.close()


def test_page_set_language_clear_and_roundtrip() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    page.set_language("de")
    page.set_language(None)
    assert page.language is None
    page.set_language("fr")
    reopened = pdfspine.open(stream=doc.tobytes())
    assert reopened[0].language == "fr"
    doc.close()
    reopened.close()


# ---------------------------------------------------------------------------
# Page.set_contents
# ---------------------------------------------------------------------------
def test_page_set_contents_points_at_stream() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    xref = doc.get_new_xref()
    doc.update_stream(xref, b"BT /F1 12 Tf 72 700 Td (Hi) Tj ET", new=True)
    page.set_contents(xref)
    assert page.get_contents() == [xref]
    doc.close()


def test_page_set_contents_rejects_bad_xref() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    with pytest.raises(PdfError):
        page.set_contents(0)
    # A non-stream object is rejected too.
    nx = doc.get_new_xref()
    doc.update_object(nx, "<< /Type /Foo >>")
    with pytest.raises(PdfError):
        page.set_contents(nx)
    doc.close()


# ---------------------------------------------------------------------------
# Document.get_outline_xrefs
# ---------------------------------------------------------------------------
def test_get_outline_xrefs_empty_when_no_outline() -> None:
    doc = pdfspine.open()
    doc.new_page()
    assert doc.get_outline_xrefs() == []
    doc.close()


def test_get_outline_xrefs_one_per_item() -> None:
    doc = pdfspine.open()
    for _ in range(3):
        doc.new_page()
    doc.set_toc([[1, "Chapter 1", 1], [2, "Section 1.1", 2], [1, "Chapter 2", 3]])
    xrefs = doc.get_outline_xrefs()
    assert len(xrefs) == 3
    assert all(isinstance(x, int) and x > 0 for x in xrefs)
    assert len(set(xrefs)) == 3  # distinct objects
    doc.close()


# ---------------------------------------------------------------------------
# Document.embfile_upd
# ---------------------------------------------------------------------------
def test_embfile_upd_replaces_content_and_desc() -> None:
    doc = pdfspine.open()
    doc.new_page()
    doc.embfile_add("f1", b"original", filename="orig.txt", desc="first")
    doc.embfile_upd("f1", b"new longer content", desc="updated")
    assert doc.embfile_get("f1") == b"new longer content"
    info = doc.embfile_info("f1")
    assert info["desc"] == "updated"
    assert info["filename"] == "orig.txt"  # untouched
    assert info["size"] == len(b"new longer content")
    doc.close()


def test_embfile_upd_partial_keeps_content() -> None:
    doc = pdfspine.open()
    doc.new_page()
    doc.embfile_add("f1", b"keepme")
    doc.embfile_upd("f1", filename="renamed.txt")
    assert doc.embfile_get("f1") == b"keepme"  # content unchanged
    assert doc.embfile_info("f1")["filename"] == "renamed.txt"
    doc.close()


def test_embfile_upd_by_index() -> None:
    doc = pdfspine.open()
    doc.new_page()
    doc.embfile_add("f1", b"data")
    doc.embfile_upd(0, b"newdata")
    assert doc.embfile_get("f1") == b"newdata"
    doc.close()


def test_embfile_upd_bad_item_raises() -> None:
    doc = pdfspine.open()
    doc.new_page()
    doc.embfile_add("f1", b"data")
    with pytest.raises(ValueError):
        doc.embfile_upd("missing", b"x")
    with pytest.raises(ValueError):
        doc.embfile_upd(99, b"x")
    doc.close()


def test_embfile_upd_survives_save_reopen() -> None:
    doc = pdfspine.open()
    doc.new_page()
    doc.embfile_add("f1", b"old")
    doc.embfile_upd("f1", b"updated bytes")
    reopened = pdfspine.open(stream=doc.tobytes())
    assert reopened.embfile_get("f1") == b"updated bytes"
    doc.close()
    reopened.close()
