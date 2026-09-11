"""Warp mathematical contract; public MuPDF wrapper failures are not pixel oracles."""

import math

import pdfspine as f
import pytest


def pixmap(colors, cs=3, alpha=False):
    result = f.Pixmap(cs, (0, 0, len(colors[0]), len(colors)), alpha)
    for y, row in enumerate(colors):
        for x, value in enumerate(row):
            result.set_pixel(x, y, value)
    return result


def square(w=2, h=2):
    return f.Quad((0, 0), (w, 0), (0, h), (w, h))


def test_identity_center_and_rotated_orientation():
    p = pixmap([[(255, 0, 0), (0, 255, 0)], [(0, 0, 255), (255, 255, 255)]])
    result = p.warp(square(), 2, 2)
    assert result.samples == bytes(
        [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]
    )
    assert p.warp(square(), 1, 1).pixel(0, 0) == (128, 128, 128, 255)
    rotated = p.warp(f.Quad((0, 2), (0, 0), (2, 2), (2, 0)), 2, 2)
    assert [rotated.pixel(x, y)[:3] for y in range(2) for x in range(2)] == [
        (0, 0, 255),
        (255, 0, 0),
        (255, 255, 255),
        (0, 255, 0),
    ]


@pytest.mark.parametrize(
    "q,expected",
    [
        ([(80, 20), (160, 20), (20, 220), (240, 220)], (125, 120, 0, 255)),
        ([(40, 20), (210, 50), (10, 210), (240, 240)], (125, 130, 0, 255)),
    ],
)
def test_slanted_quad_center_uses_four_corner_average(q, expected):
    p = pixmap([[(x, y, 0) for x in range(256)] for y in range(256)])
    assert p.warp(f.Quad(*q), 1, 1).pixel(0, 0) == expected


def test_premultiplied_alpha_and_memory_independence():
    p = pixmap([[(0, 0, 0, 0), (128, 0, 0, 128)]], alpha=True)
    view = memoryview(p)
    result = p.warp(square(2, 1), 1, 1)
    assert result.pixel(0, 0) == (64, 0, 0, 64)
    before = bytes(view)
    p.set_pixel(1, 0, (0, 0, 0, 128))
    assert bytes(view) == before
    assert result.pixel(0, 0) == (64, 0, 0, 64)
    result.set_pixel(0, 0, (1, 2, 3, 4))
    assert p.pixel(1, 0) == (0, 0, 0, 128)


@pytest.mark.parametrize("cs", [1, 3, 4])
def test_colorspace_alpha_dpi_origin_and_clamp(cs):
    p = pixmap([[tuple([20] * cs), tuple([90] * cs)]], cs=cs)
    p.set_origin(20, 30)
    p.set_dpi(72, 144)
    result = p.warp(square(2, 1), 2, 1)
    assert (result.x, result.y, result.xres, result.yres) == (0, 0, 72, 144)
    assert result.colorspace == p.colorspace
    assert result.n == cs + 1 and result.alpha
    assert result.pixel(0, 0) == tuple([20] * cs + [255])
    for bounds, color in [((-4, -4, -2, -2), 20), ((4, 4, 6, 6), 90)]:
        q = f.Rect(*bounds).quad
        assert p.warp(q, 1, 1).pixel(0, 0) == tuple([color] * cs + [255])


@pytest.mark.parametrize("w,h", [(0, 2), (2, 0), (0, 0)])
def test_zero_dimensions(w, h):
    p = pixmap([[(1,)]], cs=1)
    result = p.warp(square(1, 1), w, h)
    assert (result.width, result.height, result.samples) == (w, h, b"")


@pytest.mark.parametrize(
    "q",
    [
        f.Quad((0, 0), (1, 1), (0, 1), (1, 0)),
        f.Quad((0, 0), (0, 0), (0, 1), (0, 1)),
        f.Quad((0, 0), (1, 0), (0.5, 0.2), (1, 1)),
        f.Quad((math.nan, 0), (1, 0), (0, 1), (1, 1)),
        f.Quad((math.inf, 0), (1, 0), (0, 1), (1, 1)),
    ],
)
def test_invalid_quads(q):
    with pytest.raises(ValueError):
        pixmap([[(1,)]], cs=1).warp(q, 1, 1)


def test_dimension_types_and_allocation_limit():
    p = pixmap([[(1,)]], cs=1)
    for w in [-1, 2**32]:
        with pytest.raises(ValueError):
            p.warp(square(1, 1), w, 1)
    for w in [1.5, "2", None]:
        with pytest.raises(TypeError):
            p.warp(square(1, 1), w, 1)
    with pytest.raises(f.PdfLimitError):
        p.warp(square(1, 1), 2**32 - 1, 2**32 - 1)
    with pytest.raises(TypeError):
        p.warp([(0, 0)] * 4, 1, 1)
    assert p.warp(square(1, 1), True, True).pixel(0, 0) == (1, 255)
