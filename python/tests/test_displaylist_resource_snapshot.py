"""A display list keeps the resource revision captured when it was recorded."""

import pdfspine
import pytest

from python.tests.test_displaylist_textpage import _resource_pdf


@pytest.mark.parametrize("resource", ["image", "font"])
def test_displaylist_raster_keeps_recorded_resources(resource: str) -> None:
    doc = pdfspine.open(stream=_resource_pdf())
    page = doc[0]
    display = page.get_displaylist()
    before = display.get_pixmap().samples
    assert page.get_pixmap().samples == before
    if resource == "image":
        doc.update_stream(9, b"\x00\xff\x00")
    else:
        doc.xref_set_key(7, "BaseFont", "/Times-Roman")
    assert page.get_pixmap().samples != before
    assert display.get_pixmap().samples == before
    doc.close()
    assert display.get_pixmap().samples == before


def test_displaylist_includes_overlay_before_recording() -> None:
    doc = pdfspine.open(stream=_resource_pdf())
    doc.update_stream(9, b"\x00\xff\x00")
    before_pdf = doc.tobytes()
    display = doc[0].get_displaylist()
    expected = doc[0].get_pixmap().samples
    assert display.get_pixmap().samples == expected
    assert doc.tobytes() == before_pdf
    doc.update_stream(9, b"\xff\xff\x00")
    assert display.get_pixmap().samples == expected


def test_displaylist_keeps_indirect_soft_mask_revision() -> None:
    doc = pdfspine.open(stream=_resource_pdf())
    mask = doc.get_new_xref()
    doc.update_object(
        mask,
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray >>",
    )
    doc.update_stream(mask, b"\xff")
    doc.xref_set_key(9, "SMask", f"{mask} 0 R")
    display = doc[0].get_displaylist()
    before = display.get_pixmap().samples
    doc.update_stream(mask, b"\x00")
    assert doc[0].get_pixmap().samples != before
    assert display.get_pixmap().samples == before


def test_displaylist_keeps_indirect_palette_revision() -> None:
    doc = pdfspine.open(stream=_resource_pdf())
    palette = doc.get_new_xref()
    doc.update_object(palette, "<< >>")
    doc.update_stream(palette, b"\xff\x00\x00")
    doc.xref_set_key(9, "ColorSpace", f"[/Indexed /DeviceRGB 0 {palette} 0 R]")
    doc.update_stream(9, b"\x00")
    display = doc[0].get_displaylist()
    before = display.get_pixmap().samples
    doc.update_stream(palette, b"\x00\xff\x00")
    assert doc[0].get_pixmap().samples != before
    assert display.get_pixmap().samples == before


@pytest.mark.parametrize("resource", ["icc", "form"])
def test_displaylist_keeps_icc_alternate_and_form_revision(resource: str) -> None:
    # Current ICC rendering uses the declared device alternate, not a full CMS.
    doc = pdfspine.open(stream=_resource_pdf())
    profile = doc.get_new_xref()
    doc.update_object(profile, "<< /N 3 /Alternate /DeviceRGB >>")
    doc.update_stream(profile, b"profile placeholder")
    doc.xref_set_key(9, "ColorSpace", f"[/ICCBased {profile} 0 R]")
    display = doc[0].get_displaylist()
    before = display.get_pixmap().samples
    if resource == "icc":
        doc.xref_set_key(profile, "Alternate", "/DeviceGray")
        doc.xref_set_key(profile, "N", "1")
    else:
        doc.update_stream(5, b"0 1 0 rg 0 0 100 100 re f")
    assert doc[0].get_pixmap().samples != before
    assert display.get_pixmap().samples == before


def test_displaylist_keeps_layer_visibility_after_source_change_and_close() -> None:
    doc = pdfspine.open()
    page = doc.new_page(width=150, height=100)
    layer = doc.add_ocg("Visible at capture")
    page.insert_text((20, 40), "Layer text", oc=layer)
    display = page.get_displaylist()
    before = display.get_pixmap().samples
    doc.set_layer(-1, off=[layer])
    assert page.get_pixmap().samples != before
    doc.close()
    assert display.get_pixmap().samples == before
    assert "Layer text" in display.get_textpage().extractText()


def test_snapshot_preserves_raw_font_view_after_edit_and_close() -> None:
    from python.tests.test_rawdict_serialization import _spans

    previous = pdfspine.TOOLS.set_subset_fontnames()
    try:
        pdfspine.TOOLS.set_subset_fontnames(False)
        doc = pdfspine.open(stream=_resource_pdf())
        doc.xref_set_key(7, "BaseFont", "/ABCDEF+Helvetica")
        mask = doc.get_new_xref()
        doc.update_object(
            mask,
            "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray >>",
        )
        doc.update_stream(mask, b"\xff")
        doc.xref_set_key(9, "SMask", f"{mask} 0 R")
        display = doc[0].get_displaylist()
        before = display.get_pixmap().samples
        doc.xref_set_key(7, "BaseFont", "/CHANGED+Times-Roman")
        doc.update_stream(mask, b"\x00")
        doc.close()
        pdfspine.TOOLS.set_subset_fontnames(True)
        textpage = display.get_textpage()
        assert _spans(textpage.extractDICT())[0]["font"] == "ABCDEF+Helvetica"
        pdfspine.TOOLS.set_subset_fontnames(False)
        assert _spans(textpage.extractDICT())[0]["font"] == "Helvetica"
        assert display.get_pixmap().samples == before
    finally:
        pdfspine.TOOLS.set_subset_fontnames(previous)
