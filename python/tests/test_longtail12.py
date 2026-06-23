"""Long-tail PyMuPDF parity batch 12 — Page draw-convenience + loader/alias
cluster (PRD §C, Task P2-1).

Covers the twelve pure-Python ``Page`` methods this batch implements:
``draw_curve`` / ``draw_quad`` / ``draw_sector`` / ``draw_squiggle`` /
``draw_zigzag`` (thin wrappers over the existing ``Shape`` primitives),
``load_links`` / ``update_link`` (link helpers), ``load_annot`` / ``load_widget``
/ ``delete_widget`` (xref/name loaders), ``cluster_drawings`` (vector-graphic
clustering) and the ``is_wrapped`` predicate.

Every expected value below was captured from real PyMuPDF 1.24.14
(``.venv-oracle``); the assertions double as the regression baseline since the
real package is not importable at CI time. ``fitz`` here is the pdfspine shim.
"""

from __future__ import annotations

from pathlib import Path

import fitz
import pdfspine
import pytest


_CORPUS = Path(__file__).resolve().parents[2] / "fixtures" / "corpus"
_BORN = Path(__file__).resolve().parents[2] / "fixtures" / "born"
_CDC = _CORPUS / "cdc-mmwr-7301a1.pdf"
_FW4 = _CORPUS / "irs-fw4.pdf"
_RENDER = _BORN / "render-fixture.pdf"


def _require(path: Path) -> None:
    """跳过缺失 corpus 的用例(CI 不 checkout gitignored 的 fixtures/corpus/)。"""
    if not path.exists():
        pytest.skip(f"{path.name} missing")


# ---------------------------------------------------------------------------
# draw_* convenience methods — return the same Point as fitz (and actually emit
# content). Expected return points captured from the oracle.
# ---------------------------------------------------------------------------
def test_draw_convenience_return_points_match_fitz() -> None:
    doc = pdfspine.open(_RENDER)
    page = doc[0]
    assert tuple(page.draw_curve((10, 10), (50, 80), (90, 10))) == (90.0, 10.0)
    assert tuple(
        page.draw_quad(pdfspine.Quad((10, 10), (90, 12), (8, 90), (92, 88)))
    ) == (10.0, 10.0)
    assert tuple(
        round(v, 4) for v in page.draw_sector((100, 100), (140, 100), 90)
    ) == (100.0, 60.0)
    assert tuple(page.draw_squiggle((10, 200), (200, 200))) == (200.0, 200.0)
    assert tuple(page.draw_zigzag((10, 250), (200, 250))) == (200.0, 250.0)


def test_draw_quad_accepts_rect_like() -> None:
    """draw_quad accepts a rect / 4-sequence (rect → quad corners), like fitz."""
    doc = pdfspine.open(_RENDER)
    page = doc[0]
    assert tuple(page.draw_quad((10, 10, 90, 90))) == (10.0, 10.0)


def test_draw_convenience_emit_drawings() -> None:
    """Each draw actually commits vector content to the page."""
    doc = pdfspine.open()
    page = doc.new_page()
    before = len(page.get_drawings())
    page.draw_curve((10, 10), (50, 80), (90, 10), color=(1, 0, 0))
    page.draw_squiggle((10, 200), (200, 200))
    page.draw_zigzag((10, 250), (200, 250))
    assert len(page.get_drawings()) > before


def test_draw_convenience_present_on_fitz_shim() -> None:
    for name in ("draw_curve", "draw_quad", "draw_sector", "draw_squiggle", "draw_zigzag"):
        assert callable(getattr(fitz.Page, name))


# ---------------------------------------------------------------------------
# load_links — returns the FIRST Link (like fitz), not a list.
# ---------------------------------------------------------------------------
def test_load_links_returns_first_link() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    link = page.load_links()
    assert link is not None
    assert isinstance(link, pdfspine.Link)
    # Same object semantics as page.first_link.
    assert link.uri == page.first_link.uri
    assert link.rect == page.first_link.rect


def test_load_links_none_when_no_links() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    assert page.load_links() is None


# ---------------------------------------------------------------------------
# update_link — delete + re-insert; the new URI is observable on the page.
# ---------------------------------------------------------------------------
def test_update_link_changes_uri() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    page.insert_link({"kind": 2, "from": (10, 10, 100, 30), "uri": "https://a.com"})
    link = next(lk for lk in page.get_links() if lk.get("uri") == "https://a.com")
    spec = dict(link)
    spec["uri"] = "https://b.com"
    page.update_link(spec)
    uris = {lk.get("uri") for lk in page.get_links()}
    assert "https://b.com" in uris
    assert "https://a.com" not in uris


# ---------------------------------------------------------------------------
# load_annot — by xref (int) or name (str); errors match fitz's ValueError set.
# ---------------------------------------------------------------------------
def test_load_annot_by_xref_and_name() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    annot = page.add_text_annot((50, 50), "hi")
    loaded = page.load_annot(annot.xref)
    assert loaded.xref == annot.xref
    name = annot.info["name"]
    if name:
        assert page.load_annot(name).xref == annot.xref


def test_load_annot_bad_inputs_raise() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    with pytest.raises(ValueError):
        page.load_annot(999999)  # xref not on this page
    with pytest.raises(ValueError):
        page.load_annot(1.5)  # neither str nor int


# ---------------------------------------------------------------------------
# load_widget / delete_widget — xref loader + next-widget return (fitz values
# captured from irs-fw4.pdf page 0).
# ---------------------------------------------------------------------------
def test_load_widget_and_delete_widget() -> None:
    _require(_FW4)
    doc = pdfspine.open(_FW4)
    page = None
    widgets = []
    for pg in doc:
        ws = pg.widgets()
        if len(ws) >= 2:
            page, widgets = pg, ws
            break
    assert page is not None

    first_name = "topmostSubform[0].Page1[0].Step1a[0].f1_01[0]"
    second_name = "topmostSubform[0].Page1[0].Step1a[0].f1_02[0]"
    assert widgets[0].field_name == first_name
    assert widgets[1].field_name == second_name

    loaded = page.load_widget(widgets[0].xref)
    assert loaded.xref == widgets[0].xref
    assert loaded.field_name == first_name

    total = len(widgets)
    nxt = page.delete_widget(widgets[0])
    assert nxt is not None
    assert nxt.field_name == second_name
    assert len(page.widgets()) == total - 1


def test_load_widget_missing_raises() -> None:
    _require(_FW4)
    doc = pdfspine.open(_FW4)
    page = next(iter(doc))
    with pytest.raises(ValueError):
        page.load_widget(99999999)


# ---------------------------------------------------------------------------
# cluster_drawings — joins neighboring vector-graphic rectangles. Expected
# cluster boxes captured from the oracle for cdc-mmwr-7301a1 page 0.
# ---------------------------------------------------------------------------
_CDC_CLUSTERS = [
    (40.73, 37.001, 284.608, 47.605),
    (36.119, 52.296, 292.788, 108.806),
    (318.6, 420.98, 576.0, 642.8),
    (319.1, 649.44, 576.5, 679.5),
    (144.112, 696.551, 249.192, 757.259),
]


def test_cluster_drawings_matches_oracle() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    clusters = page.cluster_drawings()
    got = sorted((r.x0, r.y0, r.x1, r.y1) for r in clusters)
    expected = sorted(_CDC_CLUSTERS)
    assert len(got) == len(expected)
    # Coordinates match the oracle within sub-0.05pt drawing-extraction noise.
    for g, e in zip(got, expected):
        assert g == pytest.approx(e, abs=0.05)


def test_cluster_drawings_reuses_drawings_and_respects_clip() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    drawings = page.get_drawings()
    # Reusing a prior get_drawings() yields the same clusters.
    a = page.cluster_drawings()
    b = page.cluster_drawings(drawings=drawings)
    assert [tuple(r) for r in a] == [tuple(r) for r in b]
    # A tiny clip excludes everything significant.
    assert page.cluster_drawings(clip=(0, 0, 1, 1)) == []


def test_cluster_drawings_tolerance_filters_tiny() -> None:
    """Only clusters wider AND taller than the tolerances are returned."""
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    for r in page.cluster_drawings(x_tolerance=3, y_tolerance=3):
        assert r.width > 3 and r.height > 3


# ---------------------------------------------------------------------------
# is_wrapped — balanced-graphics-state predicate. Oracle: a normal page is NOT
# wrapped; after wrap_contents() it IS; an empty new page IS.
# ---------------------------------------------------------------------------
def test_is_wrapped_normal_page_false() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    assert doc[0].is_wrapped is False


def test_is_wrapped_after_wrap_contents_true() -> None:
    _require(_CDC)
    doc = pdfspine.open(_CDC)
    page = doc[0]
    page.wrap_contents()
    assert page.is_wrapped is True


def test_is_wrapped_empty_new_page_true() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    assert page.is_wrapped is True


# ---------------------------------------------------------------------------
# Surface presence — every symbol resolves (no PdfUnsupportedError) on both the
# pdfspine Page and the fitz shim.
# ---------------------------------------------------------------------------
def test_all_symbols_resolve_on_page() -> None:
    names = [
        "draw_curve", "draw_quad", "draw_sector", "draw_squiggle", "draw_zigzag",
        "load_links", "update_link", "load_annot", "load_widget", "delete_widget",
        "cluster_drawings", "is_wrapped",
    ]
    for n in names:
        assert hasattr(pdfspine.Page, n), f"pdfspine.Page missing {n}"
        assert hasattr(fitz.Page, n), f"fitz.Page missing {n}"
