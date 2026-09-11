"""External slice-3 red expectations; copy into its tree only after slice2 review."""

import pdfspine as p
import pytest
from python.tests.test_rawdict_serialization import _document


def source(content):
    doc = _document(content)
    doc.xref_set_key(3, "MediaBox", "[0 0 8 8]")
    return doc


def patterned(alpha=False):
    pix = p.Pixmap(3, (0, 0, 8, 8), alpha)
    for y in range(8):
        for x in range(8):
            value = (17 + x, 43 + y, 91 + x + y)
            if alpha:
                value += ((0, 1, 64, 128)[(x + y) % 4],)
            pix.set_pixel(x, y, value)
    return pix


def test_paints_existing_rgb_and_preserves_old_export():
    doc = source(b"1 0 0 rg 2 2 4 4 re f")
    target = patterned()
    before = target.samples
    view = memoryview(target)
    target.set_dpi(111, 222)
    device = p.ReplayDevice.for_pixmap(target)
    assert doc[0].run(device, None) is None
    assert target.pixel(3, 3) == (255, 0, 0)
    for y in range(8):
        for x in range(8):
            if x < 2 or x >= 6 or y < 2 or y >= 6:
                off = (y * 8 + x) * 3
                assert target.pixel(x, y) == tuple(before[off : off + 3])
    assert bytes(view) == before
    assert (target.xres, target.yres) == (111, 222)


def test_rgba_hole_is_byte_identical_despite_full_outer_bbox():
    doc = source(b"1 0 0 rg 0 0 8 8 re 2 2 4 4 re f*")
    target = patterned(alpha=True)
    before = target.samples
    doc[0].run(p.ReplayDevice.for_pixmap(target), None)
    for y in range(2, 6):
        for x in range(2, 6):
            off = (y * 8 + x) * 4
            assert target.pixel(x, y) == tuple(before[off : off + 4])
    assert target.pixel(0, 0) == (255, 0, 0, 255)


def test_area_is_selection_not_an_added_pixel_clip():
    doc = source(b"1 0 0 rg 2 2 4 4 re f")
    target = patterned()
    doc[0].get_displaylist().run(p.ReplayDevice.for_pixmap(target), None, (3, 3, 4, 4))
    assert target.pixel(5, 5) == (255, 0, 0)


@pytest.mark.parametrize("area", [(0, 0, 0, 0), (4, 4, 2, 2)])
def test_empty_selection_is_byte_and_pointer_stable(area):
    doc = source(b"1 0 0 rg 0 0 8 8 re f")
    target = patterned(alpha=True)
    before = target.samples
    ptr = target.samples_ptr
    doc[0].get_displaylist().run(p.ReplayDevice.for_pixmap(target), None, area)
    assert target.samples == before and target.samples_ptr == ptr


def test_zero_dimension_target_is_valid_noop():
    doc = source(b"1 0 0 rg 0 0 8 8 re f")
    seed = p.Pixmap(3, (0, 0, 1, 1), True)
    target = seed.warp(p.Rect(0, 0, 1, 1).quad, 0, 8)
    assert doc[0].run(p.ReplayDevice.for_pixmap(target), None) is None
    assert target.samples == b"" and (target.width, target.height) == (0, 8)


@pytest.mark.parametrize("cs", [1, 4])
def test_unsupported_target_does_not_mutate(cs):
    target = p.Pixmap(cs, (0, 0, 8, 8), False)
    before = target.samples
    with pytest.raises((TypeError, ValueError, p.PdfUnsupportedError)):
        p.ReplayDevice.for_pixmap(target)
    assert target.samples == before


def test_origin_after_rotate_and_caller_transform_and_dpi_are_preserved():
    doc = source(b"q 2 0 0 1 0 0 cm 1 0 0 rg 1 1 2 3 re f Q")
    doc[0].set_rotation(90)
    target = p.Pixmap(3, (0, 0, 6, 8), False)
    target.set_origin(7, 11)
    target.set_dpi(333, 444)
    doc[0].get_displaylist().run(
        p.ReplayDevice.for_pixmap(target), p.Matrix(2, 0, 0, 2, 5, 7), (8, 12, 9, 13)
    )
    assert target.samples == bytes([255, 0, 0]) * 48
    assert (target.x, target.y, target.xres, target.yres) == (7, 11, 333, 444)


@pytest.mark.parametrize("alpha", [False, True])
def test_no_area_matches_normal_displaylist_render_on_matching_background(alpha):
    doc = source(
        b"q 0 0 8 8 re W n 1 0 0 rg 2 2 4 4 re f Q 0 0 1 RG 0.5 w 0 0 m 8 8 l S"
    )
    record = doc[0].get_displaylist()
    expected = record.get_pixmap(alpha=alpha)
    target = p.Pixmap(3, (0, 0, 8, 8), alpha)
    target.clear_with(0 if alpha else 255)
    doc.close()
    record.run(p.ReplayDevice.for_pixmap(target), None, None)
    assert target.samples == expected.samples


def test_invalid_geometry_rolls_back_and_releases_target_lease():
    doc = source(b"1 0 0 rg 2 2 4 4 re f")
    target = patterned(alpha=True)
    before = target.samples
    ptr = target.samples_ptr
    device = p.ReplayDevice.for_pixmap(target)
    with pytest.raises(ValueError):
        doc[0].run(device, p.Matrix(1, 0, 0, 1, float("nan"), 0))
    assert target.samples == before and target.samples_ptr == ptr
    doc[0].run(device, None)
    assert target.pixel(3, 3) == (255, 0, 0, 255)


def test_closing_device_and_source_does_not_modify_target():
    doc = source(b"1 0 0 rg 2 2 4 4 re f")
    page = doc[0]
    target = patterned()
    before = target.samples
    device = p.ReplayDevice.for_pixmap(target)
    device.close()
    with pytest.raises(RuntimeError, match="closed"):
        page.run(device, None)
    assert target.samples == before
    device = p.ReplayDevice.for_pixmap(target)
    doc.close()
    with pytest.raises(ValueError, match="closed"):
        page.run(device, None)
    assert target.samples == before


def test_nonopaque_rgba_source_over_matches_straight_channel_math():
    doc = source(b"/GS gs 1 0 0 rg 0 0 8 8 re f")
    doc.xref_set_key(3, "Resources", "<< /ExtGState << /GS << /ca 0.5 >> >> >>")
    target = p.Pixmap(3, (0, 0, 8, 8), True)
    for y in range(8):
        for x in range(8):
            target.set_pixel(x, y, (31, 77, 99, 64))
    doc[0].run(p.ReplayDevice.for_pixmap(target), None)
    sa, da = 128 / 255, 64 / 255
    alpha = sa + da * (1 - sa)
    expected = [
        (255 * sa + 31 * da * (1 - sa)) / alpha,
        77 * da * (1 - sa) / alpha,
        99 * da * (1 - sa) / alpha,
        255 * alpha,
    ]
    assert target.pixel(3, 3) == pytest.approx(expected, abs=2)
    assert target.pixel(3, 3)[3] < 255


def test_frozen_embedded_font_and_image_survive_source_stream_edits_and_close():
    from io import BytesIO
    from PIL import Image
    from python.tests.test_insert_font import SYNTHETIC_TTF

    doc = p.open()
    page = doc.new_page(width=80, height=80)
    page.insert_font(fontname="Embedded", fontbuffer=SYNTHETIC_TTF)
    page.insert_text((5, 20), "AB", fontname="Embedded")
    png = BytesIO()
    Image.new("RGB", (2, 2), (200, 30, 40)).save(png, format="JPEG")
    page.insert_image(p.Rect(20, 30, 50, 60), stream=png.getvalue())
    record = page.get_displaylist()
    expected = record.get_pixmap(alpha=True).samples
    for xref in range(1, doc.xref_length()):
        if doc.xref_is_stream(xref):
            doc.update_stream(xref, b"broken after recording")
    doc.close()
    target = p.Pixmap(3, (0, 0, 80, 80), True)
    target.clear_with(0)
    record.run(p.ReplayDevice.for_pixmap(target), None, None)
    assert target.samples == expected
    assert target.pixel(30, 40)[3] == 255
    assert target.pixel(30, 40)[0] > 150
