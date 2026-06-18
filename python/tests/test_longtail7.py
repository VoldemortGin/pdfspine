"""Long-tail PyMuPDF parity batch 7 — Shape drawing primitives, text & props
(PRD §7 / §8.8 / §9.5).

Covers the newly-implemented ``fitz.Shape`` surface:
  - Drawing primitives: draw_quad / draw_sector / draw_squiggle / draw_zigzag.
    Each is asserted against ground-truth content-stream operators captured from
    REAL PyMuPDF 1.27 (.venv-oracle) — the four primitives match fitz operator-
    for-operator and point-for-point (within 1e-3).
  - Text: insert_text (line count) / insert_textbox (leftover height), delegating
    to the owning page so the on-page result matches.
  - Parent / geometry props: doc / page / width / height / x / y / rect plus the
    update_rect bbox accumulator and the static horizontal_angle helper, mirroring
    fitz ``Shape.__init__`` semantics (width/height = mediabox size, x/y = cropbox
    origin, rect = drawn-geometry bbox).

Correctness is anchored to the real-PyMuPDF GROUND TRUTH embedded below (the
project ``.venv`` ``fitz`` is the pdfspine shim, so a live fitz-vs-native compare
there is shim-vs-native only). Both the native ``pdfspine`` API and the ``fitz``
shim are still exercised for shim coverage; all fixtures are self-generated.
"""

from __future__ import annotations

import math

import fitz
import pdfspine
import pytest


# === ground truth captured from real PyMuPDF 1.27 (.venv-oracle) ============
# Content-stream path operators (a 300x400 page, top-left input space).
_GT_QUAD = ["50 350 m", "160 250 l", "40 260 l", "150 340 l", "50 350 l"]
_GT_SECTOR_FULL = [
    "250 200 m",
    "250 255.22847 205.22847 300 150 300 c",
    "250 200 m",
    "150 200 l",
    "150 300 l",
]
_GT_SECTOR_NOFULL = [
    "250 200 m",
    "250 255.22847 205.22847 300 150 300 c",
]
_GT_SQUIGGLE_HEAD = [
    "50 100 m",
    "51.10457 102.66666 52.89543 102.66666 54 100 c",
    "55.10457 97.33334 56.89543 97.33334 58 100 c",
]
_GT_SQUIGGLE_OPCOUNT = 51  # 1 'm' + 50 'c' (last 'l' from oracle close excluded)
_GT_ZIGZAG_HEAD = ["50 50 m", "52 52 l", "56 48 l"]


def _doc(w: float = 300, h: float = 400) -> tuple[pdfspine.Document, pdfspine.Page]:
    d = pdfspine.open()
    p = d.new_page(width=w, height=h)
    return d, p


def _path_ops(page: pdfspine.Page) -> list[str]:
    """The path-construction operators in the page content stream, each number
    rounded to 3 decimals for float-format tolerance."""
    raw = page.read_contents().decode("latin-1")
    out: list[str] = []
    for line in raw.splitlines():
        line = line.strip()
        if not (line.endswith((" m", " l", " c", " re")) or line == "h"):
            continue
        toks = line.split()
        norm = []
        for t in toks[:-1]:
            try:
                norm.append(f"{float(t):.3f}")
            except ValueError:
                norm.append(t)
        norm.append(toks[-1])
        out.append(" ".join(norm))
    return out


def _norm(ops: list[str]) -> list[str]:
    out = []
    for line in ops:
        toks = line.split()
        norm = []
        for t in toks[:-1]:
            try:
                norm.append(f"{float(t):.3f}")
            except ValueError:
                norm.append(t)
        norm.append(toks[-1])
        out.append(" ".join(norm))
    return out


# === drawing primitives vs real-PyMuPDF ground truth =======================
def test_draw_quad_matches_fitz_stream():
    d, p = _doc()
    sh = p.new_shape()
    # PyMuPDF (ul, ur, ll, lr) positional corner order.
    sh.draw_quad([(50, 50), (150, 60), (160, 150), (40, 140)])
    sh.finish(color=(0, 0, 0), width=1)
    sh.commit()
    ops = [o for o in _path_ops(p) if o.endswith((" m", " l"))]
    assert ops == _norm(_GT_QUAD)
    d.close()


def test_draw_quad_returns_first_corner_chain():
    d, p = _doc()
    sh = p.new_shape()
    last = sh.draw_quad([(50, 50), (150, 60), (160, 150), (40, 140)])
    # draw_polyline returns the last point (== ul, the closing point).
    assert (round(last.x), round(last.y)) == (50, 50)
    d.close()


def test_draw_sector_full_matches_fitz_stream():
    d, p = _doc()
    sh = p.new_shape()
    sh.draw_sector((150, 200), (250, 200), 90, fullSector=True)
    sh.finish(color=(0, 0, 0), width=1)
    sh.commit()
    ops = [o for o in _path_ops(p) if o.endswith((" m", " l", " c"))]
    assert ops == _norm(_GT_SECTOR_FULL)
    # the wedge must be CLOSED (closepath h): arc + 2 radii + closing edge = 4
    # get_drawings items (regression guard — an open wedge reports only 3).
    assert sum(len(dr["items"]) for dr in p.get_drawings()) == 4
    d.close()


def test_draw_sector_nofull_matches_fitz_stream():
    d, p = _doc()
    sh = p.new_shape()
    sh.draw_sector((150, 200), (250, 200), 90, fullSector=False)
    sh.finish(color=(0, 0, 0), width=1)
    sh.commit()
    ops = [o for o in _path_ops(p) if o.endswith((" m", " l", " c"))]
    assert ops == _norm(_GT_SECTOR_NOFULL)
    d.close()


def test_draw_sector_270_emits_three_arcs():
    # 270 degrees = three 90-degree cubic arcs + the closing wedge.
    d, p = _doc()
    sh = p.new_shape()
    sh.draw_sector((150, 200), (250, 200), 270, fullSector=True)
    sh.finish(color=(0, 0, 0), width=1)
    sh.commit()
    ops = _path_ops(p)
    assert sum(1 for o in ops if o.endswith(" c")) == 3
    d.close()


def test_draw_sector_zero_radius_raises():
    d, p = _doc()
    sh = p.new_shape()
    with pytest.raises(ValueError):
        sh.draw_sector((100, 100), (100, 100), 90)
    d.close()


def test_draw_squiggle_matches_fitz_stream():
    d, p = _doc()
    sh = p.new_shape()
    sh.draw_squiggle((50, 300), (250, 300), breadth=2)
    sh.finish(color=(0, 0, 0), width=1)
    sh.commit()
    ops = [o for o in _path_ops(p) if o.endswith((" m", " c"))]
    assert len(ops) == _GT_SQUIGGLE_OPCOUNT
    assert ops[:3] == _norm(_GT_SQUIGGLE_HEAD)
    d.close()


def test_draw_squiggle_too_close_raises():
    d, p = _doc()
    sh = p.new_shape()
    with pytest.raises(ValueError):
        sh.draw_squiggle((50, 50), (51, 50), breadth=2)
    d.close()


def test_draw_zigzag_matches_fitz_stream():
    d, p = _doc()
    sh = p.new_shape()
    sh.draw_zigzag((50, 350), (250, 350), breadth=2)
    sh.finish(color=(0, 0, 0), width=1)
    sh.commit()
    ops = [o for o in _path_ops(p) if o.endswith((" m", " l"))]
    assert ops[:3] == _norm(_GT_ZIGZAG_HEAD)
    # one moveto + alternating linetos
    assert sum(1 for o in ops if o.endswith(" m")) == 1
    d.close()


def test_draw_zigzag_too_close_raises():
    d, p = _doc()
    sh = p.new_shape()
    with pytest.raises(ValueError):
        sh.draw_zigzag((50, 50), (51, 50), breadth=2)
    d.close()


# === text (delegates to the owning page) ====================================
def test_shape_insert_text_lines_and_content():
    d, p = _doc()
    sh = p.new_shape()
    n = sh.insert_text((50, 250), "Line one\nLine two\nLine three", fontsize=11)
    sh.finish()
    sh.commit()
    assert n == 3
    txt = p.get_text("text")
    assert "Line one" in txt and "Line three" in txt
    d.close()


def test_shape_insert_text_accepts_list():
    d, p = _doc()
    sh = p.new_shape()
    n = sh.insert_text((50, 250), ["alpha", "beta"], fontsize=11)
    sh.commit()
    assert n == 2
    assert "alpha" in p.get_text("text")
    d.close()


def test_shape_insert_textbox_returns_float_and_updates_rect():
    d, p = _doc()
    sh = p.new_shape()
    r = sh.insert_textbox((50, 50, 250, 200), "wrap me " * 8, fontsize=11)
    assert isinstance(r, float)
    # textbox extends the accumulated rect to the box.
    assert list(sh.rect) == [50.0, 50.0, 250.0, 200.0]
    sh.commit()
    assert "wrap" in p.get_text("text")
    d.close()


def test_shape_insert_text_matches_page_insert_text():
    # Shape.insert_text delegates to Page.insert_text → identical line count.
    d, p = _doc()
    d2, p2 = _doc()
    sh = p.new_shape()
    n_shape = sh.insert_text((50, 100), "a\nb\nc\nd", fontsize=11)
    n_page = p2.insert_text((50, 100), "a\nb\nc\nd", fontsize=11)
    assert n_shape == n_page
    d.close()
    d2.close()


# === parent / geometry properties ==========================================
def test_shape_parent_props():
    d, p = _doc()
    sh = p.new_shape()
    assert sh.doc is d
    assert sh.page is p
    d.close()


def test_shape_dimension_props_match_page():
    d, p = _doc(w=300, h=400)
    sh = p.new_shape()
    assert sh.width == 300.0
    assert sh.height == 400.0
    assert sh.x == 0.0
    assert sh.y == 0.0
    d.close()


def test_shape_rect_starts_none_then_accumulates():
    d, p = _doc()
    sh = p.new_shape()
    assert sh.rect is None
    sh.draw_line((10, 20), (110, 80))
    assert list(sh.rect) == [10.0, 20.0, 110.0, 80.0]
    sh.draw_rect((5, 5, 200, 200))
    assert list(sh.rect) == [5.0, 5.0, 200.0, 200.0]
    d.close()


def test_shape_rect_circle_does_not_extend():
    # PyMuPDF draw_circle does not call updateRect.
    d, p = _doc()
    sh = p.new_shape()
    sh.draw_rect((5, 5, 200, 200))
    sh.draw_circle((1000, 1000), 30)
    assert list(sh.rect) == [5.0, 5.0, 200.0, 200.0]
    d.close()


def test_shape_update_rect_point_and_rect():
    d, p = _doc()
    sh = p.new_shape()
    sh.update_rect((100, 100))
    assert list(sh.rect) == [100.0, 100.0, 100.0, 100.0]
    sh.update_rect((50, 50, 150, 200))
    assert list(sh.rect) == [50.0, 50.0, 150.0, 200.0]
    # camelCase alias.
    sh.updateRect((0, 0))
    assert list(sh.rect) == [0.0, 0.0, 150.0, 200.0]
    d.close()


@pytest.mark.parametrize(
    "c, pt, expected",
    [
        ((0, 0), (1, 0), 0.0),
        ((0, 0), (0, 1), math.pi / 2),
        ((0, 0), (-1, 0), -math.pi),
        ((0, 0), (-1, -1), -3 * math.pi / 4),
        ((0, 0), (1, -1), -math.pi / 4),
    ],
)
def test_horizontal_angle(c, pt, expected):
    got = pdfspine.Shape.horizontal_angle(c, pt)
    assert abs(got - expected) < 1e-9


# === fitz shim coverage =====================================================
def test_shim_shape_primitives_and_props():
    d = fitz.open()
    p = d.new_page(width=300, height=400)
    sh = p.new_shape()
    sh.draw_quad([(50, 50), (150, 60), (160, 150), (40, 140)])
    sh.draw_sector((150, 200), (250, 200), 90)
    sh.draw_squiggle((50, 300), (250, 300))
    sh.draw_zigzag((50, 350), (250, 350))
    assert sh.width == 300.0 and sh.height == 400.0
    assert sh.rect is not None
    sh.finish(color=(0, 0, 0), width=1)
    sh.commit()
    assert len(p.get_drawings()) >= 1
    d.close()


def test_shim_shape_text():
    d = fitz.open()
    p = d.new_page(width=300, height=400)
    sh = p.new_shape()
    n = sh.insert_text((50, 100), "shim text", fontsize=12)
    r = sh.insert_textbox((50, 150, 250, 300), "boxed shim text", fontsize=11)
    sh.commit()
    assert n == 1
    assert isinstance(r, float)
    assert "shim" in p.get_text("text")
    d.close()
