"""pdfspine callback replay is an explicit extension, not a native device ABI."""

import threading
from concurrent.futures import ThreadPoolExecutor

import pdfspine as p
import pytest

from python.tests.test_rawdict_serialization import _document


def test_lifecycle_order_immutable_retained_and_source_close():
    doc = _document(b"q 10 20 30 40 re f BT /F1 12 Tf 10 80 Td (A) Tj ET Q")
    dl = doc[0].get_displaylist(annots=False)
    events = []
    device = p.ReplayDevice(events.append)
    doc.close()
    assert dl.run(device, None, None) is None
    assert [e.kind for e in events] == [
        "begin",
        "save",
        "fill",
        "text",
        "restore",
        "end",
    ]
    assert [e.sequence for e in events] == list(range(len(events)))
    fill = events[2]
    with pytest.raises((AttributeError, TypeError)):
        fill.kind = "bad"
    with pytest.raises(TypeError):
        fill.payload["color"] = 0
    assert fill.font_buffer is None and fill.image_bytes is None
    assert events[3].font_buffer is None
    device.close()
    device.close()
    with pytest.raises(RuntimeError, match="closed"):
        dl.run(device, None, None)


def test_exception_identity_no_end_and_device_reusable():
    doc = _document(b"10 20 30 40 re f")
    error = LookupError("callback sentinel")
    seen = []

    def callback(event):
        seen.append(event.kind)
        if event.kind == "fill":
            raise error

    device = p.ReplayDevice(callback)
    for _ in range(2):
        with pytest.raises(LookupError) as caught:
            doc[0].run(device, None)
        assert caught.value is error
    assert seen == ["begin", "fill", "begin", "fill"]


def test_recursive_close_and_concurrent_reuse_rejected():
    doc = _document(b"10 20 30 40 re f")
    dl = doc[0].get_displaylist()
    entered, release = threading.Event(), threading.Event()

    def callback(event):
        if event.kind == "begin":
            with pytest.raises(RuntimeError, match="running"):
                dl.run(device, None, None)
            with pytest.raises(RuntimeError, match="running"):
                device.close()
            entered.set()
            assert release.wait(5)

    device = p.ReplayDevice(callback)
    with ThreadPoolExecutor(1) as pool:
        result = pool.submit(dl.run, device, None, None)
        assert entered.wait(5)
        try:
            with pytest.raises(RuntimeError, match="running"):
                dl.run(device, None, None)
        finally:
            release.set()
        result.result()


@pytest.mark.parametrize("area", [(0, 0, 0, 0), (20, 20, 10, 10)])
def test_empty_area_keeps_state_and_lifecycle_only(area):
    doc = _document(b"q 10 20 30 40 re W n 0 0 100 100 re f Q")
    events = []
    doc[0].get_displaylist().run(p.ReplayDevice(events.append), None, area)
    assert [e.kind for e in events] == ["begin", "save", "clip", "restore", "end"]


def test_three_transform_layers_once_and_area_in_final_space():
    doc = _document(b"q 2 0 0 3 10 20 cm 1 2 4 5 re f Q")
    doc[0].set_rotation(90)
    dl = doc[0].get_displaylist(annots=False)
    events = []
    # source corner (1,2) -> CTM(12,26) -> Rotate90(26,12) -> m(57,43)
    dl.run(p.ReplayDevice(events.append), p.Matrix(2, 0, 0, 3, 5, 7), (58, 44, 59, 45))
    fill = next(e for e in events if e.kind == "fill")
    assert fill.bounds == pytest.approx((57, 43, 87, 67))
    assert fill.matrix == pytest.approx((0, 3, 2, 0, 5, 7))
    assert fill.payload["path"] == (("re", (12.0, 26.0, 20.0, 41.0)),)
    events.clear()
    dl.run(p.ReplayDevice(events.append), p.Matrix(2, 0, 0, 3, 5, 7), (12, 26, 13, 27))
    assert "fill" not in [e.kind for e in events]


@pytest.mark.parametrize(
    "matrix,area",
    [
        ((1, 0, 0, 1, float("nan"), 0), None),
        (None, (0, 0, float("inf"), 1)),
        ((1, 2), None),
    ],
)
def test_invalid_geometry_emits_nothing(matrix, area):
    doc = _document(b"10 20 30 40 re f")
    events = []
    with pytest.raises((ValueError, TypeError)):
        doc[0].get_displaylist().run(p.ReplayDevice(events.append), matrix, area)
    assert events == []


def test_nested_clip_restores_and_unknown_stroke_retained():
    doc = _document(
        b"q 10 30 60 50 re W n 0 0 200 100 re f q 20 40 10 10 re W n 0 0 200 100 re f Q 40 40 10 10 re f Q 120 40 10 10 re f 10 w 20 20 m 50 70 l 55 20 l S"
    )
    events = []
    doc[0].get_displaylist().run(
        p.ReplayDevice(events.append), None, (122, 743, 125, 747)
    )
    paints = [e for e in events if e.kind in ("fill", "stroke")]
    assert [e.kind for e in paints] == ["fill", "stroke"]
    assert paints[0].bounds == pytest.approx((120, 742, 130, 752))
    assert paints[1].bounds is None
    assert [e.payload["depth"] for e in events if e.kind == "restore"] == [1, 0]


def test_lazy_inline_image_bytes_keep_format_and_close_lifetime():
    content = b"q 20 0 0 20 10 10 cm BI /W 1 /H 1 /CS /RGB /BPC 8 ID \xff\x00\x00 EI Q"
    doc = _document(content)
    events = []
    doc[0].run(p.ReplayDevice(events.append), None)
    doc.close()
    image = next(e for e in events if e.kind == "image")
    assert image.image_format == "png"
    assert image.image_bytes.startswith(b"\x89PNG\r\n\x1a\n")
    assert image.font_buffer is None and image.font_format is None
    from PIL import Image
    from io import BytesIO

    assert Image.open(BytesIO(image.image_bytes)).convert("RGB").getpixel((0, 0)) == (
        255,
        0,
        0,
    )


def test_damaged_embedded_font_is_lazy_explicit_error():
    doc = _document(b"BT /F1 12 Tf 10 80 Td (A) Tj ET")
    descriptor = doc.get_new_xref()
    doc.update_object(descriptor, "<< /FontFile2 123456 0 R >>")
    doc.xref_set_key(5, "FontDescriptor", f"{descriptor} 0 R")
    events = []
    doc[0].run(p.ReplayDevice(events.append), None)
    assert events[-1].kind == "end"
    text = next(e for e in events if e.kind == "text")
    with pytest.raises(p.PdfError):
        _ = text.font_buffer


def test_close_source_during_callback_does_not_change_remaining_events():
    doc = _document(b"10 20 30 40 re f BT /F1 12 Tf 10 80 Td (A) Tj ET")
    events = []

    def callback(e):
        events.append(e)
        if e.kind == "begin":
            doc.close()

    doc[0].run(p.ReplayDevice(callback), None)
    assert [e.kind for e in events] == ["begin", "fill", "text", "end"]
    assert events[2].payload["glyphs"][0][0] == "A"


def test_embedded_font_program_survives_source_edit_and_close():
    from python.tests.test_insert_font import SYNTHETIC_TTF

    doc = p.open()
    page = doc.new_page()
    page.insert_font(fontname="Embedded", fontbuffer=SYNTHETIC_TTF)
    page.insert_text((40, 70), "AB", fontname="Embedded")
    dl = page.get_displaylist()
    for xref in range(1, doc.xref_length()):
        if doc.xref_is_stream(xref) and doc.xref_stream(xref) == SYNTHETIC_TTF:
            doc.update_stream(xref, b"broken after recording")
    doc.close()
    events = []
    dl.run(p.ReplayDevice(events.append), None, None)
    text = next(e for e in events if e.kind == "text")
    assert text.font_format == "TrueType"
    assert text.font_buffer == SYNTHETIC_TTF


def test_jpeg_format_matches_real_payload_and_event_lifetime():
    from io import BytesIO
    from PIL import Image

    stream = BytesIO()
    Image.new("RGB", (2, 2), (220, 10, 20)).save(stream, format="JPEG")
    doc = p.open()
    page = doc.new_page()
    page.insert_image(p.Rect(10, 20, 30, 40), stream=stream.getvalue())
    events = []
    page.run(p.ReplayDevice(events.append), None)
    doc.close()
    event = next(e for e in events if e.kind == "image")
    assert event.image_format in ("jpg", "jpeg")
    assert event.image_bytes == stream.getvalue()


def test_singular_matrix_hairline_and_text_unknown_bounds():
    doc = _document(b"0 w 10 10 m 20 10 l S BT /F1 12 Tf 10 80 Td (A) Tj ET")
    events = []
    doc[0].get_displaylist().run(
        p.ReplayDevice(events.append), (0, 0, 0, 0, 1, 1), None
    )
    assert [e.kind for e in events] == ["begin", "stroke", "text", "end"]
    assert all(e.bounds is None for e in events)
