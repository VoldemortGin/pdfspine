"""Owned text snapshots from display lists (PyMuPDF 1.28.2 behavior probes)."""

from __future__ import annotations

import json

import pdfspine
import pytest

from python.tests.test_rawdict_serialization import _document, _spans


def test_displaylist_textpage_survives_source_edit_and_close() -> None:
    doc = _document(b"BT /F1 12 Tf 40 700 Td (Alpha) Tj ET")
    page = doc[0]
    display = page.get_displaylist()
    before_pixels = display.get_pixmap().samples
    page.insert_text((40, 120), "Beta")
    assert "Beta" in page.get_text()
    doc.close()
    textpage = display.get_textpage()
    assert isinstance(textpage, pdfspine.TextPage)
    assert textpage.extractText() == "Alpha\n"
    assert textpage.extractWORDS()[0][4] == "Alpha"
    assert _spans(textpage.extractDICT())[0]["font"] == "Helvetica"
    assert _spans(json.loads(textpage.extractJSON()))[0]["text"] == "Alpha"
    assert textpage.search("Alpha")
    assert not textpage.search("Beta")
    assert display.get_pixmap().samples == before_pixels


@pytest.mark.parametrize("mode", [0, 3, 7])
def test_displaylist_textpage_records_invisible_and_clip_text(mode: int) -> None:
    doc = _document(
        f"BT /F1 12 Tf {mode} Tr 40 700 Td (Visible semantic text) Tj ET".encode()
    )
    display = doc[0].get_displaylist()
    expected = doc[0].get_text()
    doc.close()
    assert display.get_textpage().extractText() == expected == "Visible semantic text\n"


def test_displaylist_textpage_preserves_creation_spacing_flags() -> None:
    doc = _document(b"BT /F1 12 Tf 40 700 Td (Alpha) Tj 35 0 Td (Beta) Tj ET")
    display = doc[0].get_displaylist()
    regular = display.get_textpage(flags=3)
    inhibited = display.get_textpage(flags=3 | 8)
    assert regular.extractText() == "Alpha Beta\n"
    assert inhibited.extractText() == "AlphaBeta\n"
    assert "Alpha Beta" in json.dumps(regular.extractDICT())
    assert "AlphaBeta" in json.dumps(inhibited.extractDICT())
    doc.close()


def _resource_pdf() -> bytes:
    """Two forms reuse local names for distinct fonts and image streams."""

    def stream(dictionary: bytes, content: bytes) -> bytes:
        return (
            b"<< "
            + dictionary
            + b" /Length "
            + str(len(content)).encode()
            + b" >>\nstream\n"
            + content
            + b"\nendstream"
        )

    form = b"/Type /XObject /Subtype /Form /BBox [0 0 100 100] "
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] "
        b"/Resources << /XObject << /A 5 0 R /B 6 0 R >> >> /Contents 4 0 R >>",
        stream(b"", b"q 1 0 0 1 10 50 cm /A Do Q q 1 0 0 1 160 50 cm /B Do Q"),
        stream(
            form + b"/Resources << /Font << /F1 7 0 R >> /XObject << /Im1 9 0 R >> >>",
            b"BT /F1 12 Tf 0 80 Td (Alpha) Tj ET q 30 0 0 30 0 0 cm /Im1 Do Q",
        ),
        stream(
            form + b"/Resources << /Font << /F1 8 0 R >> /XObject << /Im1 10 0 R >> >>",
            b"BT /F1 12 Tf 0 80 Td (Beta) Tj ET q 30 0 0 30 0 0 cm /Im1 Do Q",
        ),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
        stream(
            b"/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            b"\xff\x00\x00",
        ),
        stream(
            b"/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            b"\x00\x00\xff",
        ),
    ]
    data = bytearray(b"%PDF-1.7\n")
    offsets = [0]
    for number, body in enumerate(objects, 1):
        offsets.append(len(data))
        data += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(data)
    data += f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode()
    for offset in offsets[1:]:
        data += f"{offset:010} 00000 n \n".encode()
    data += f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(data)


def test_displaylist_textpage_owns_form_resources_and_image_flags() -> None:
    import io

    from PIL import Image

    doc = pdfspine.open(stream=_resource_pdf())
    display = doc[0].get_displaylist()
    # Destroy both image payloads and the font name in the live store.
    doc.update_stream(9, b"\x00\xff\x00")
    doc.update_stream(10, b"\x00\xff\x00")
    doc.xref_set_key(7, "BaseFont", "/Times-Roman")
    doc.close()
    default = display.get_textpage()
    images = display.get_textpage(flags=7)
    assert {span["font"] for span in _spans(default.extractDICT())} == {
        "Helvetica",
        "Courier",
    }
    assert all(block["type"] == 0 for block in default.extractDICT()["blocks"])
    assert all(block["type"] == 0 for block in default.extractRAWDICT()["blocks"])
    assert default.extractIMGINFO() == []
    blocks = [block for block in images.extractDICT()["blocks"] if block["type"] == 1]
    assert len(blocks) == 2
    assert [
        Image.open(io.BytesIO(block["image"])).convert("RGB").getpixel((0, 0))
        for block in blocks
    ] == [(255, 0, 0), (0, 0, 255)]
    assert [tuple(block["bbox"]) for block in blocks] == [
        (10, 120, 40, 150),
        (160, 120, 190, 150),
    ]
    assert len(images.extractIMGINFO()) == 2
    assert len([b for b in images.extractRAWDICT()["blocks"] if b["type"] == 1]) == 2
    assert (
        len([b for b in json.loads(images.extractJSON())["blocks"] if b["type"] == 1])
        == 2
    )


def test_displaylist_textpage_owns_inline_image() -> None:
    import io

    from PIL import Image

    doc = _document(
        b"q 20 0 0 30 40 700 cm BI /W 1 /H 1 /CS /RGB /BPC 8 ID \xff\x00\x00 EI Q"
    )
    display = doc[0].get_displaylist()
    doc.close()
    blocks = display.get_textpage(flags=7).extractDICT()["blocks"]
    assert len(blocks) == 1 and blocks[0]["type"] == 1
    assert Image.open(io.BytesIO(blocks[0]["image"])).convert("RGB").getpixel(
        (0, 0)
    ) == (255, 0, 0)


@pytest.mark.parametrize("rotation", [0, 90, 180, 270])
def test_displaylist_textpage_crop_and_rotation(rotation: int) -> None:
    doc = pdfspine.open(stream=_resource_pdf())
    page = doc[0]
    page.set_cropbox(pdfspine.Rect(5, 10, 295, 190))
    page.set_rotation(rotation)
    display = page.get_displaylist()
    expected = page.get_textpage(flags=3).extractDICT()
    doc.close()
    actual = display.get_textpage().extractDICT()
    assert (actual["width"], actual["height"]) == (
        expected["width"],
        expected["height"],
    )
    assert [(s["text"], s["bbox"]) for s in _spans(actual)] == [
        (s["text"], s["bbox"]) for s in _spans(expected)
    ]


def test_displaylist_textpage_owns_inline_gray_image() -> None:
    import io

    from PIL import Image

    doc = _document(b"q 20 0 0 30 40 700 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x7f EI Q")
    display = doc[0].get_displaylist()
    doc.close()
    block = display.get_textpage(flags=7).extractDICT()["blocks"][0]
    assert Image.open(io.BytesIO(block["image"])).convert("RGB").getpixel((0, 0)) == (
        127,
        127,
        127,
    )


def _annotation_document(flags: int = 0):
    doc = pdfspine.open(stream=_resource_pdf())
    doc.update_stream(4, b"")
    free = doc.get_new_xref()
    widget = doc.get_new_xref()
    doc.update_object(
        free,
        f"<< /Type /Annot /Subtype /FreeText /Rect [10 50 110 150] /F {flags} /AP << /N 5 0 R >> >>",
    )
    doc.update_object(
        widget,
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (SnapshotWidget) /Rect [160 50 260 150] /F 0 /AS /On /AP << /N << /Off 5 0 R /On 6 0 R >> >> >>",
    )
    doc.xref_set_key(3, "Annots", f"[{free} 0 R {widget} 0 R]")
    doc.xref_set_key(1, "AcroForm", f"<< /Fields [{widget} 0 R] >>")
    return doc


@pytest.mark.parametrize(
    "flags, visible",
    [(0, True), (1, False), (2, False), (4, True), (32, False), (64, True)],
)
def test_displaylist_records_visible_annotation_and_widget_appearances(
    flags: int, visible: bool
) -> None:
    doc = _annotation_document(flags)
    page = doc[0]
    default = page.get_displaylist()
    included = page.get_displaylist(annots=1)
    excluded = page.get_displaylist(annots=0)
    assert excluded.get_textpage().extractText() == ""
    assert set(excluded.get_pixmap().samples) == {255}
    assert default.get_pixmap().samples == included.get_pixmap().samples
    assert default.get_pixmap().samples != excluded.get_pixmap().samples
    # Semantic and encoded-image resources must survive edits to each AP source.
    doc.update_stream(5, b"")
    doc.update_stream(6, b"")
    doc.update_stream(9, b"\x00\xff\x00")
    doc.update_stream(10, b"\x00\xff\x00")
    doc.close()
    expected = "Alpha\nBeta\n" if visible else "Beta\n"
    assert default.get_textpage().extractText() == expected
    assert included.get_textpage().extractText() == expected
    assert len(included.get_textpage(flags=7).extractIMGINFO()) == (2 if visible else 1)
    import io
    from PIL import Image

    image_blocks = [
        b
        for b in included.get_textpage(flags=7).extractDICT()["blocks"]
        if b["type"] == 1
    ]
    pixels = [
        Image.open(io.BytesIO(b["image"])).convert("RGB").getpixel((0, 0))
        for b in image_blocks
    ]
    assert pixels == ([(255, 0, 0), (0, 0, 255)] if visible else [(0, 0, 255)])


def test_displaylist_annotations_replay_matches_equivalent_page_forms() -> None:
    source = pdfspine.open(stream=_resource_pdf())
    annotated = _annotation_document()
    assert (
        source[0].get_displaylist(annots=0).get_pixmap().samples
        == annotated[0].get_displaylist().get_pixmap().samples
    )
    source.close()
    annotated.close()


def test_displaylist_annotation_optional_content_snapshot() -> None:
    doc = _annotation_document()
    hidden = doc.add_ocg("hidden appearance", on=False)
    doc.xref_set_key(11, "OC", f"{hidden} 0 R")
    display = doc[0].get_displaylist()
    doc.xref_set_key(11, "OC", "null")
    assert "Alpha" in doc[0].get_displaylist().get_textpage().extractText()
    doc.close()
    assert display.get_textpage().extractText() == "Beta\n"


def test_displaylist_annotation_matrix_and_nonzero_bbox() -> None:
    doc = _annotation_document()
    doc.xref_set_key(5, "BBox", "[-10 -20 110 110]")
    doc.xref_set_key(5, "Matrix", "[0 2 -3 0 40 -20]")
    display = doc[0].get_displaylist()
    doc.close()
    textpage = display.get_textpage(flags=7)
    spans = {s["text"]: s for s in _spans(textpage.extractDICT())}
    assert set(spans) == {"Alpha", "Beta"}
    assert spans["Alpha"]["origin"] == pytest.approx(
        (33.0769230769, 141.6666666667), abs=1e-4
    )
    images = [b for b in textpage.extractDICT()["blocks"] if b["type"] == 1]
    assert images[0]["bbox"] == pytest.approx(
        (71.5384615385, 116.6666666667, 94.6153846154, 141.6666666667), abs=1e-4
    )


@pytest.mark.parametrize(
    "kind", ["forms", "annotations", "transformed", "hidden", "named-inline"]
)
def test_displaylist_textpage_live_oracle(kind: str, tmp_path) -> None:
    from python.tests.test_ocg_layers import _real_pymupdf_available, _run_child

    if not _real_pymupdf_available():
        pytest.skip("real PyMuPDF oracle unavailable")
    doc = (
        pdfspine.open(stream=_resource_pdf())
        if kind == "forms"
        else _annotation_document(2 if kind == "hidden" else 0)
    )
    if kind in {"named", "named-inline"}:
        doc.close()
        doc = _named_colorspace_document(kind == "named-inline")
    if kind == "transformed":
        doc.xref_set_key(5, "BBox", "[-10 -20 110 110]")
        doc.xref_set_key(5, "Matrix", "[0 2 -3 0 40 -20]")
    doc[0].set_cropbox(pdfspine.Rect(5, 10, 295, 190))
    doc[0].set_rotation(90)
    path = tmp_path / "recording.pdf"
    doc.save(path)
    code = """import json, pymupdf
D = pymupdf.open(__PATH__)
recordings = [D[0].get_displaylist(annots=a) for a in (0,1)]
D.close()
out = []
for dl in recordings:
 for flags in (3,7):
  tp = pymupdf.TextPage(dl.get_textpage(flags))
  data = tp.extractDICT()
  out.append({'size':[data['width'],data['height']],
    'text':sorted((s['text'],s['font'],s['origin']) for b in data['blocks'] if b['type']==0 for l in b['lines'] for s in l['spans']),
    'images':[{'bbox':b['bbox'],'rgb':pymupdf.Pixmap(pymupdf.csRGB, pymupdf.Pixmap(b['image'])).pixel(0,0)} for b in data['blocks'] if b['type']==1]})
print(json.dumps(out))
""".replace("__PATH__", repr(str(path)))
    expected = _run_child(code)
    recordings = [doc[0].get_displaylist(annots=a) for a in (0, 1)]
    doc.close()
    import io
    from PIL import Image

    for index, (display, flags) in enumerate(
        (dl, f) for dl in recordings for f in (3, 7)
    ):
        actual = display.get_textpage(flags).extractDICT()
        oracle = expected[index]
        assert [actual["width"], actual["height"]] == oracle["size"]
        spans = sorted((s["text"], s["font"], s["origin"]) for s in _spans(actual))
        assert [(s[0], s[1]) for s in spans] == [(s[0], s[1]) for s in oracle["text"]]
        for span, other in zip(spans, oracle["text"], strict=True):
            assert span[2] == pytest.approx(other[2], abs=1e-4)
        images = [b for b in actual["blocks"] if b["type"] == 1]
        assert len(images) == len(oracle["images"])
        for image, other in zip(images, oracle["images"], strict=True):
            assert image["bbox"] == pytest.approx(other["bbox"], abs=1e-4)
            assert Image.open(io.BytesIO(image["image"])).convert("RGB").getpixel(
                (0, 0)
            ) == tuple(other["rgb"])


def _named_colorspace_document(inline: bool = False):
    doc = pdfspine.open(stream=_resource_pdf())
    doc.xref_set_key(
        5,
        "Resources",
        "<< /Font << /F1 7 0 R >> /XObject << /Im1 9 0 R >> /ColorSpace << /CS1 /DeviceRGB >> >>",
    )
    doc.xref_set_key(
        6,
        "Resources",
        "<< /Font << /F1 8 0 R >> /XObject << /Im1 10 0 R >> /ColorSpace << /CS1 /DeviceGray >> >>",
    )
    if inline:
        doc.update_stream(
            5, b"q 30 0 0 30 0 0 cm BI /W 1 /H 1 /CS /CS1 /BPC 8 ID \xff\x00\x00 EI Q"
        )
        doc.update_stream(
            6, b"q 30 0 0 30 0 0 cm BI /W 1 /H 1 /CS /CS1 /BPC 8 ID \x7f EI Q"
        )
    else:
        doc.xref_set_key(9, "ColorSpace", "/CS1")
        doc.xref_set_key(10, "ColorSpace", "/CS1")
        doc.update_stream(10, b"\x7f")
    return doc


@pytest.mark.parametrize("inline", [False, True])
def test_displaylist_resolves_named_colorspaces_per_form(inline: bool) -> None:
    import io
    from PIL import Image

    doc = _named_colorspace_document(inline)
    display = doc[0].get_displaylist()
    doc.xref_set_key(5, "Resources", "<< /ColorSpace << /CS1 /DeviceCMYK >> >>")
    doc.xref_set_key(6, "Resources", "<< /ColorSpace << /CS1 /DeviceCMYK >> >>")
    doc.close()
    blocks = [
        b
        for b in display.get_textpage(flags=7).extractDICT()["blocks"]
        if b["type"] == 1
    ]
    assert [b["colorspace"] for b in blocks] == [3, 1]
    assert [
        Image.open(io.BytesIO(b["image"])).convert("RGB").getpixel((0, 0))
        for b in blocks
    ] == [(255, 0, 0), (127, 127, 127)]


def test_displaylist_named_inline_colorspace_and_flate_filter() -> None:
    import io
    import zlib
    from PIL import Image

    doc = _named_colorspace_document(True)
    data = zlib.compress(b"\xff\x00\x00")
    doc.update_stream(
        5,
        b"q 30 0 0 30 0 0 cm BI /W 1 /H 1 /CS /CS1 /BPC 8 /F /Fl /DP << /Predictor 1 >> ID "
        + data
        + b" EI Q",
    )
    display = doc[0].get_displaylist()
    doc.close()
    block = display.get_textpage(flags=7).extractDICT()["blocks"][0]
    assert Image.open(io.BytesIO(block["image"])).convert("RGB").getpixel((0, 0)) == (
        255,
        0,
        0,
    )
