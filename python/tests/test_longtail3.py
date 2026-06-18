"""Long-tail PyMuPDF parity batch 3 (PRD §7 / §9.5).

Covers the newly-implemented surface:
  - Page.clean_contents / wrap_contents / delete_image / replace_image /
    set_oc / get_oc / get_texttrace / get_bboxlog
  - Document.xref_is_font / xref_is_image / xref_set_key / xref_copy /
    del_xml_metadata / subset_fonts
  - Font (fitz.Font): name / ascender / descender / bbox / glyph_count /
    flags / is_* / glyph_advance / has_glyph / text_length / char_lengths /
    glyph_name_to_unicode / unicode_to_glyph_name
  - Tools / TOOLS: gen_id / mupdf_warnings / reset_mupdf_warnings /
    mupdf_version / store_shrink / store_size / store_maxsize / fitz_config /
    glyph_cache_empty / set_small_glyph_heights / mupdf_display_*

Both the native ``pdfspine`` API and the ``fitz`` shim are exercised. Most
fixtures are self-generated; the image tests use a bundled JPEG asset.
"""

from __future__ import annotations

import pathlib

import pdfspine
import fitz
import pytest

# A real (small) JPEG bundled with the workspace, used for image XObject tests.
_JPEG_PATH = (
    pathlib.Path(__file__).resolve().parents[2]
    / "crates"
    / "pdf-image"
    / "tests"
    / "assets"
    / "gray.jpg"
)


def _doc_with_text() -> pdfspine.Document:
    d = pdfspine.open()
    p = d.new_page()
    p.insert_text((72, 100), "Hello", fontsize=20)
    return d


def _doc_with_image() -> tuple[pdfspine.Document, str, int]:
    """A one-page doc carrying one JPEG image; returns (doc, resname, xref)."""
    jpeg = _JPEG_PATH.read_bytes()
    d = pdfspine.open()
    p = d.new_page()
    p.insert_image(pdfspine.Rect(0, 0, 100, 100), stream=jpeg)
    imgs = p.get_images(full=True)
    return d, imgs[0][7], imgs[0][0]


# === Page content low-level ==============================================


def test_clean_contents_consolidates() -> None:
    d = _doc_with_text()
    p = d[0]
    p.clean_contents()
    # /Contents is now a single stream.
    assert len(p.get_contents()) == 1
    # Text survives the consolidation.
    assert "Hello" in p.get_text()


def test_wrap_contents_brackets_and_roundtrips() -> None:
    d = _doc_with_text()
    p = d[0]
    p.wrap_contents()
    body = p.read_contents()
    assert body.startswith(b"q")
    assert body.rstrip().endswith(b"Q")
    # Still saveable / reopenable.
    d2 = pdfspine.open(stream=d.tobytes())
    assert d2.page_count == 1


def test_page_set_get_oc() -> None:
    d = _doc_with_text()
    p = d[0]
    assert p.get_oc() == 0
    ocg = d.add_ocg("layer1")
    p.set_oc(ocg)
    assert p.get_oc() == ocg
    p.set_oc(0)
    assert p.get_oc() == 0


@pytest.mark.skipif(not _JPEG_PATH.exists(), reason="JPEG asset missing")
def test_delete_image() -> None:
    d, name, xref = _doc_with_image()
    assert d.xref_is_image(xref)
    d[0].delete_image(name)
    # The XObject is now a 1x1 stub.
    assert d.xref_get_key(xref, "Width") == "1"
    # Document still valid.
    assert pdfspine.open(stream=d.tobytes()).page_count == 1


@pytest.mark.skipif(not _JPEG_PATH.exists(), reason="JPEG asset missing")
def test_replace_image_by_name_and_xref() -> None:
    jpeg = _JPEG_PATH.read_bytes()
    d, name, xref = _doc_with_image()
    d[0].replace_image(name, stream=jpeg)
    assert d.xref_is_image(xref)
    # Replace by xref string too.
    d[0].replace_image(str(xref), stream=jpeg)
    assert pdfspine.open(stream=d.tobytes()).page_count == 1


def test_replace_image_requires_stream() -> None:
    d = _doc_with_text()
    with pytest.raises(ValueError):
        d[0].replace_image("Img0")


# === Page text trace =====================================================


def test_get_texttrace_shape() -> None:
    d = _doc_with_text()
    tt = d[0].get_texttrace()
    assert isinstance(tt, list) and len(tt) >= 1
    span = tt[0]
    for key in (
        "dir",
        "font",
        "wmode",
        "flags",
        "ascender",
        "descender",
        "color",
        "colorspace",
        "size",
        "bbox",
        "chars",
        "seqno",
    ):
        assert key in span, f"missing texttrace key {key}"
    assert span["size"] == 20.0
    assert span["dir"] == (1.0, 0.0)
    # chars are (ucs, gid, origin, bbox) tuples.
    ch = span["chars"][0]
    assert len(ch) == 4
    assert ch[0] == ord("H")
    assert len(ch[2]) == 2 and len(ch[3]) == 4


def test_get_bboxlog_shape() -> None:
    d = _doc_with_text()
    bl = d[0].get_bboxlog()
    assert isinstance(bl, list) and len(bl) >= 1
    op, bbox = bl[0]
    assert op == "fill-text"
    assert len(bbox) == 4


# === Document xref predicates + writes ===================================


def test_xref_is_font_and_image() -> None:
    d, _name, xref = _doc_with_image()
    assert d.xref_is_image(xref) is True
    assert d.xref_is_font(xref) is False
    # A non-existent object is not a font/image.
    assert d.xref_is_font(99999) is False
    assert d.xref_is_image(99999) is False


def test_xref_set_key_roundtrip() -> None:
    d = _doc_with_text()
    pxref = d.page_xref(0)
    d.xref_set_key(pxref, "Rotate", "90")
    assert d.xref_get_key(pxref, "Rotate") == "90"
    # "null" removes the key.
    d.xref_set_key(pxref, "Rotate", "null")
    assert d.xref_get_key(pxref, "Rotate") is None


@pytest.mark.skipif(not _JPEG_PATH.exists(), reason="JPEG asset missing")
def test_xref_copy() -> None:
    d, _name, xref = _doc_with_image()
    # Allocate + initialize a fresh slot (the fitz-canonical sequence: a new
    # xref is a null object, so it must be made a dict before keys are set),
    # then copy the image onto it.
    target = d.get_new_xref()
    d.update_object(target, "<< >>")
    d.xref_set_key(target, "Type", "/XObject")
    d.xref_copy(xref, target)
    assert d.xref_is_image(target)
    assert d.xref_get_key(target, "Width") == d.xref_get_key(xref, "Width")


def test_del_xml_metadata() -> None:
    d = _doc_with_text()
    d.set_xml_metadata("<x:xmpmeta>meta</x:xmpmeta>")
    assert "meta" in d.get_xml_metadata()
    d.del_xml_metadata()
    assert d.get_xml_metadata() == ""
    # Idempotent / safe on a doc with no metadata.
    d.del_xml_metadata()


def test_subset_fonts_reports_count() -> None:
    d = _doc_with_text()
    # Base-14 inline font is not embedded → no subsettable fonts.
    assert d.subset_fonts() == 0


# === Font ================================================================


def test_font_metrics() -> None:
    f = pdfspine.Font("helv")
    assert f.name == "Helvetica"
    assert f.ascender > 0.0
    assert f.descender < 0.0
    assert f.glyph_count > 0
    bb = f.bbox
    assert bb[0] < bb[2] and bb[1] < bb[3]
    flags = f.flags
    assert flags["bold"] == 0 and flags["serif"] == 0
    assert f.is_monospaced == 0


def test_font_aliases() -> None:
    assert pdfspine.Font("tiro").name == "Times-Roman"
    assert pdfspine.Font("cour").name == "Courier"
    assert pdfspine.Font("Times-Bold").name == "Times-Bold"
    assert pdfspine.Font("cour").is_monospaced == 1
    # Unknown falls back, never raises.
    assert pdfspine.Font("nonesuch").name == "Helvetica"


def test_font_advances_and_lengths() -> None:
    f = pdfspine.Font("helv")
    # Helvetica 'A' is 667/1000.
    assert abs(f.glyph_advance(ord("A")) - 0.667) < 1e-6
    assert abs(f.text_length("AB", fontsize=10.0) - (0.667 + 0.667) * 10.0) < 1e-4
    assert len(f.char_lengths("AB", 1.0)) == 2
    assert f.has_glyph(ord("A")) == ord("A")
    assert f.has_glyph(0x1F600) == -1


def test_font_glyph_name_mappings() -> None:
    f = pdfspine.Font("helv")
    assert f.glyph_name_to_unicode("A") == ord("A")
    assert f.glyph_name_to_unicode(".notdef") == 0xFFFD
    assert f.unicode_to_glyph_name(ord("A")) == "A"
    assert f.unicode_to_glyph_name(0x00E9) == "eacute"
    assert repr(f) == "Font('Helvetica')"


def test_font_via_fitz_shim() -> None:
    f = fitz.Font("helv")
    assert f.name == "Helvetica"
    assert f.glyph_count > 0


# === Tools / TOOLS =======================================================


def test_tools_singleton() -> None:
    t = pdfspine.TOOLS
    assert isinstance(t, pdfspine.Tools)
    # gen_id is monotonically increasing and positive.
    a = t.gen_id()
    b = t.gen_id()
    assert b > a >= 1


def test_tools_diagnostics() -> None:
    t = pdfspine.TOOLS
    assert t.mupdf_warnings() == ""
    t.reset_mupdf_warnings()
    assert isinstance(t.mupdf_version(), str) and t.mupdf_version()
    assert t.store_shrink(100) == 0
    assert t.store_size == 0
    assert t.store_maxsize > 0
    assert t.glyph_cache_empty() is True
    assert isinstance(t.fitz_config(), dict)
    assert t.set_small_glyph_heights(True) is True
    assert t.set_small_glyph_heights(False) is False
    assert isinstance(t.mupdf_display_errors(), bool)
    assert isinstance(t.mupdf_display_warnings(), bool)


def test_tools_via_fitz_shim() -> None:
    assert fitz.TOOLS.mupdf_version() == pdfspine.TOOLS.mupdf_version()
    assert isinstance(fitz.Tools, type)
