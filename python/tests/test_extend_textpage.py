"""Atomic TextPage append, matching the scoped local PyMuPDF oracle contract."""

from __future__ import annotations

import json
import math

import pdfspine
import pytest


def document(text: str):
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=100)
    page.insert_text((20, 30), text)
    return doc, page


def test_append_keeps_identity_old_blocks_rect_and_duplicate_numbers():
    old_doc, old = document("Alpha")
    source_doc, source = document("Beta")
    target = old.get_textpage()
    alias = target
    rect = tuple(target.rect)
    before = target.extractRAWDICT()["blocks"]
    assert source.extend_textpage(target) is None
    assert source.extend_textpage(target) is None
    assert target is alias
    assert tuple(target.rect) == rect
    assert target.extractRAWDICT()["blocks"][: len(before)] == before
    assert target.extractText() == "Alpha\nBeta\nBeta\n"
    assert [w[5] for w in target.extractWORDS()] == [0, 1, 2]
    assert [b["number"] for b in target.extractDICT()["blocks"]] == [0, 1, 2]
    assert len(target.search("Beta")) == 2
    assert len(json.loads(target.extractJSON())["blocks"]) == 3
    old_doc.close()
    source_doc.close()
    assert target.extractText() == "Alpha\nBeta\nBeta\n"


@pytest.mark.parametrize(
    "matrix,expected",
    [
        (pdfspine.Matrix(1, 0, 0, 1, 30, 20), "Beta"),
        (pdfspine.Matrix(2, 2), "Beta"),
        (pdfspine.Matrix(0, 1, -1, 0, 80, 10), "Beta"),
        (pdfspine.Matrix(1, 0, 0, 1, 170, 0), "Be"),
        (pdfspine.Matrix(1, 0, 0, 1, 210, 0), ""),
        (pdfspine.Matrix(0, 0, 0, 0, 0, 0), ""),
    ],
)
def test_transform_then_clip_new_glyph_origins(matrix, expected):
    old_doc, old = document("Alpha")
    source_doc, source = document("Beta")
    target = old.get_textpage()
    before = target.extractRAWDICT()["blocks"]
    source.extend_textpage(target, matrix=matrix)
    assert target.extractRAWDICT()["blocks"][: len(before)] == before
    assert target.extractText() == "Alpha\n" + (expected + "\n" if expected else "")
    assert tuple(target.rect) == (0, 0, 200, 100)
    old_doc.close()
    source_doc.close()


@pytest.mark.parametrize(
    "matrix,error",
    [
        ("bad", TypeError),
        ([1, 0], TypeError),
        (pdfspine.Matrix(math.nan, 0, 0, 1, 0, 0), ValueError),
        (pdfspine.Matrix(math.inf, 0, 0, 1, 0, 0), ValueError),
    ],
)
def test_invalid_matrix_is_atomic(matrix, error):
    doc, page = document("Alpha")
    target = page.get_textpage()
    previous = target._tp
    before = target.extractRAWDICT()
    with pytest.raises(error):
        page.extend_textpage(target, matrix=matrix)
    assert target._tp is previous
    assert target.extractRAWDICT() == before
    doc.close()


def test_closed_source_fails_atomically():
    doc, page = document("Alpha")
    target = page.get_textpage()
    previous = target._tp
    doc.close()
    with pytest.raises(ValueError, match="closed"):
        page.extend_textpage(target)
    assert target._tp is previous


def add_image(page, color):
    page.insert_image(
        pdfspine.Rect(80, 40, 100, 60), stream=bytes(color), width=1, height=1
    )


@pytest.mark.parametrize("initial,added", [(0, 0), (4, 0), (0, 4), (4, 4)])
def test_recorded_flags_and_cross_document_image_resources(initial, added):
    a, old = document("Alpha")
    b, source = document("Beta")
    add_image(old, (255, 0, 0))
    add_image(source, (0, 0, 255))
    target = old.get_displaylist().get_textpage(flags=initial)
    before = target.extractDICT()["blocks"]
    red = [block["image"] for block in before if block["type"] == 1]
    blue = [
        block["image"]
        for block in source.get_displaylist()
        .get_textpage(flags=4)
        .extractDICT()["blocks"]
        if block["type"] == 1
    ]
    source.extend_textpage(target, flags=added)
    source.extend_textpage(target, flags=added)
    a.close()
    b.close()
    blocks = target.extractDICT()["blocks"]
    assert blocks[: len(before)] == before
    assert [block["image"] for block in blocks if block["type"] == 1] == red + (
        blue * 2 if added else []
    )
    assert [block["number"] for block in blocks] == list(range(len(blocks)))
    assert [entry["number"] for entry in target.extractIMGINFO()] == [
        block["number"] for block in blocks if block["type"] == 1
    ]
    assert len(json.loads(target.extractJSON())["blocks"]) == len(blocks)


def test_ordinary_page_target_keeps_legacy_visible_images_on_promotion():
    a, old = document("Alpha")
    b, source = document("Beta")
    add_image(old, (255, 0, 0))
    target = old.get_textpage(flags=0)
    before = target.extractDICT()["blocks"]
    assert any(block["type"] == 1 for block in before)
    source.extend_textpage(target, flags=0)
    a.close()
    b.close()
    assert target.extractDICT()["blocks"][: len(before)] == before


def test_affine_coordinates_apply_only_to_appended_text():
    a, old = document("Alpha")
    b, source = document("Beta")
    matrix = pdfspine.Matrix(1, 0, 0, 1, 30, 20)
    original = source.get_textpage().extractWORDS()[0]
    target = old.get_textpage()
    source.extend_textpage(target, matrix=matrix)
    word = target.extractWORDS()[-1]
    assert word[:4] == pytest.approx(
        (original[0] + 30, original[1] + 20, original[2] + 30, original[3] + 20)
    )
    a.close()
    b.close()


def test_page_get_text_reuses_combined_resources_and_flags():
    a, old = document("Alpha")
    b, source = document("Beta")
    add_image(old, (255, 0, 0))
    add_image(source, (0, 0, 255))
    target = old.get_displaylist().get_textpage(flags=4)
    source.extend_textpage(target, flags=4)
    assert old.get_text("dict", textpage=target, flags=0) == target.extractDICT()
    assert source.get_text("rawdict", textpage=target) == target.extractRAWDICT()
    assert source.get_text("words", textpage=target) == target.extractWORDS()
    assert len(source.search_for("Beta", textpage=target)) == 1
    a.close()
    b.close()


def test_unreadable_visible_target_image_fails_without_partial_append():
    a, old = document("Alpha")
    b, source = document("Beta")
    add_image(old, (255, 0, 0))
    target = old.get_textpage()
    xref = old.get_images()[0][0]
    a.update_stream(xref, b"")
    before = target.extractRAWDICT()
    previous = target._tp
    with pytest.raises(pdfspine.PdfDecodeError, match="promote target image"):
        source.extend_textpage(target)
    assert target._tp is previous
    assert target.extractRAWDICT() == before
    a.close()
    b.close()


def test_closed_target_can_promote_retained_resources_without_relayout():
    a, old = document("Alpha")
    b, source = document("Beta")
    add_image(old, (255, 0, 0))
    target = old.get_textpage()
    before = target.extractDICT()["blocks"]
    a.close()
    source.extend_textpage(target)
    b.close()
    assert target.extractDICT()["blocks"][: len(before)] == before


@pytest.mark.parametrize(
    "matrix,transform,bbox,size,direction,origin",
    [
        (
            pdfspine.Matrix(0, 1, -1, 0, 100, 0),
            (0, 20, -20, 0, 60, 80),
            (40, 80, 60, 100),
            11,
            (0, 1),
            (70, 20),
        ),
        (
            pdfspine.Matrix(1, 0.2, 0.3, 1, 0, 0),
            (20, 4, 6, 20, 92, 56),
            (92, 56, 118, 80),
            10.664895686,
            (0.980580676, 0.196116135),
            (29, 34),
        ),
        (
            pdfspine.Matrix(2, 2),
            (40, 0, 0, 40, 160, 80),
            (160, 80, 200, 120),
            22,
            (1, 0),
            (40, 60),
        ),
    ],
)
def test_new_image_affine_transform_and_text_metrics_match_oracle(
    matrix, transform, bbox, size, direction, origin
):
    a = pdfspine.open()
    blank = a.new_page(width=300, height=200)
    b = pdfspine.open()
    source = b.new_page(width=300, height=200)
    source.insert_text((20, 30), "Beta")
    add_image(source, (255, 0, 0))
    target = blank.get_textpage()
    source.extend_textpage(target, flags=4, matrix=matrix)
    blocks = target.extractDICT()["blocks"]
    image = next(block for block in blocks if block["type"] == 1)
    assert image["transform"] == pytest.approx(transform)
    assert image["bbox"] == pytest.approx(bbox)
    assert json.loads(target.extractJSON())["blocks"][-1]["transform"] == pytest.approx(
        transform
    )
    assert target.extractIMGINFO()[0]["transform"] == pytest.approx(transform)
    line = next(block for block in blocks if block["type"] == 0)["lines"][0]
    assert line["dir"] == pytest.approx(direction)
    assert line["spans"][0]["size"] == pytest.approx(size, abs=1e-5)
    assert line["spans"][0]["origin"] == pytest.approx(origin)
    a.close()
    b.close()


def test_dehyphenation_is_retained_per_segment():
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=100)
    page.insert_text((20, 30), "inter-")
    page.insert_text((20, 42), "national")
    display = page.get_displaylist()
    target = display.get_textpage(flags=16)
    original = target.extractText()
    plain = display.get_textpage(flags=0).extractText()
    assert original != plain
    page.extend_textpage(target, flags=0)
    assert target.extractText() == original + plain
    assert page.get_text(textpage=target) == original + plain
    blocks = target.extractBLOCKS()
    assert "international" in blocks[0][4]
    assert "inter-" in blocks[-1][4]
    doc.close()
