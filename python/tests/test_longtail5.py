"""Long-tail PyMuPDF parity batch 5 — Page geometry / boxes (PRD §7 / §9.5).

Covers the newly-implemented Page surface:
  - Box getters: artbox / bleedbox / trimbox (default to the crop box when
    absent), mediabox_size (Point), cropbox_position (Point)
  - Matrices: transformation_matrix / rotation_matrix / derotation_matrix,
    verified against the exact values PyMuPDF 1.24.x returns for /Rotate ∈
    {0, 90, 180, 270}
  - Identity: xref (page object number), parent (owning Document, by identity)
  - Box setters: set_mediabox / set_cropbox / set_artbox / set_bleedbox /
    set_trimbox

Both the native ``pdfspine`` API and the ``fitz`` shim are exercised; all
fixtures are self-generated.
"""

from __future__ import annotations

import fitz
import pdfspine
import pytest
from pdfspine.geometry import Matrix, Point


def _letter() -> pdfspine.Document:
    d = pdfspine.open()
    d.new_page(width=612, height=792)
    return d


# === box getters: artbox / bleedbox / trimbox default to crop box ==========


def test_lt5_boxes_default_to_cropbox():
    p = _letter()[0]
    cb = tuple(p.cropbox)
    assert tuple(p.artbox) == cb
    assert tuple(p.bleedbox) == cb
    assert tuple(p.trimbox) == cb


def test_lt5_boxes_explicit():
    doc = _letter()
    p = doc[0]
    p.set_artbox((10, 20, 300, 400))
    p.set_bleedbox((1, 2, 600, 780))
    p.set_trimbox((5, 5, 500, 700))
    p = doc.reload_page(0)
    assert tuple(p.artbox) == (10.0, 20.0, 300.0, 400.0)
    assert tuple(p.bleedbox) == (1.0, 2.0, 600.0, 780.0)
    assert tuple(p.trimbox) == (5.0, 5.0, 500.0, 700.0)


# === mediabox_size / cropbox_position (Point) ==============================


def test_lt5_mediabox_size():
    p = _letter()[0]
    sz = p.mediabox_size
    assert isinstance(sz, Point)
    assert (sz.x, sz.y) == (612.0, 792.0)


def test_lt5_cropbox_position():
    doc = _letter()
    p = doc[0]
    p.set_cropbox((30, 40, 500, 700))
    p = doc.reload_page(0)
    pos = p.cropbox_position
    assert isinstance(pos, Point)
    assert (pos.x, pos.y) == (30.0, 40.0)


# === xref / parent =========================================================


def test_lt5_xref_is_int():
    p = _letter()[0]
    assert isinstance(p.xref, int)
    assert p.xref > 0


def test_lt5_parent_identity():
    doc = _letter()
    p = doc[0]
    assert p.parent is doc
    # load_page / iteration also carry the parent.
    assert doc.load_page(0).parent is doc
    assert next(iter(doc)).parent is doc


# === matrices — exact PyMuPDF 1.24.x values ================================
#
# For a Letter page (612×792, crop box == media box) the values below are the
# ones real PyMuPDF returns at each /Rotate (captured against pymupdf 1.24.14).

_TM = {
    0: (1.0, 0.0, 0.0, -1.0, 0.0, 792.0),
    90: (1.0, 0.0, 0.0, -1.0, 0.0, 792.0),
    180: (1.0, 0.0, 0.0, -1.0, 0.0, 792.0),
    270: (1.0, 0.0, 0.0, -1.0, 0.0, 792.0),
}
_RM = {
    0: (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
    90: (0.0, 1.0, -1.0, 0.0, 792.0, 0.0),
    180: (-1.0, 0.0, 0.0, -1.0, 612.0, 792.0),
    270: (0.0, -1.0, 1.0, 0.0, 0.0, 612.0),
}
_DM = {
    0: (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
    90: (0.0, -1.0, 1.0, 0.0, 0.0, 792.0),
    180: (-1.0, 0.0, 0.0, -1.0, 612.0, 792.0),
    270: (0.0, 1.0, -1.0, 0.0, 612.0, 0.0),
}


def _approx6(actual, expected):
    a = tuple(actual)
    assert len(a) == 6
    # +0.0 / -0.0 compare equal under approx.
    assert a == pytest.approx(expected, abs=1e-6)


@pytest.mark.parametrize("rot", [0, 90, 180, 270])
def test_lt5_matrices_letter(rot):
    doc = _letter()
    p = doc[0]
    p.set_rotation(rot)
    p = doc.reload_page(0)
    assert isinstance(p.transformation_matrix, Matrix)
    assert isinstance(p.rotation_matrix, Matrix)
    assert isinstance(p.derotation_matrix, Matrix)
    _approx6(p.transformation_matrix, _TM[rot])
    _approx6(p.rotation_matrix, _RM[rot])
    _approx6(p.derotation_matrix, _DM[rot])


def test_lt5_derotation_inverts_rotation():
    # rotation_matrix * derotation_matrix == identity (round-trip).
    for rot in (0, 90, 180, 270):
        doc = _letter()
        p = doc[0]
        p.set_rotation(rot)
        p = doc.reload_page(0)
        prod = Matrix(*p.rotation_matrix) * Matrix(*p.derotation_matrix)
        _approx6(prod, (1.0, 0.0, 0.0, 1.0, 0.0, 0.0))


def test_lt5_matrices_offset_cropbox():
    # Crop box not at the media-box origin: transformation_matrix carries the
    # crop-box offset only at /Rotate 0 (the rotation matrix carries it
    # otherwise), exactly like PyMuPDF.
    doc = _letter()
    p = doc[0]
    p.set_cropbox((50, 60, 500, 700))  # cw=450, ch=640
    p = doc.reload_page(0)
    _approx6(p.transformation_matrix, (1.0, 0.0, 0.0, -1.0, -50.0, 700.0))
    p.set_rotation(90)
    p = doc.reload_page(0)
    _approx6(p.transformation_matrix, (1.0, 0.0, 0.0, -1.0, 0.0, 640.0))
    _approx6(p.rotation_matrix, (0.0, 1.0, -1.0, 0.0, 640.0, 0.0))


# === setters: round-trip ===================================================


def test_lt5_set_mediabox():
    doc = _letter()
    p = doc[0]
    p.set_mediabox((0, 0, 400, 500))
    p = doc.reload_page(0)
    assert tuple(p.mediabox) == (0.0, 0.0, 400.0, 500.0)
    assert (p.mediabox_size.x, p.mediabox_size.y) == (400.0, 500.0)


def test_lt5_set_cropbox_clipped_to_mediabox():
    doc = _letter()
    p = doc[0]
    # A crop box larger than the media box is clipped (PyMuPDF semantics).
    p.set_cropbox((-10, -10, 1000, 1000))
    p = doc.reload_page(0)
    assert tuple(p.cropbox) == (0.0, 0.0, 612.0, 792.0)


def test_lt5_set_boxes_accept_rect():
    from pdfspine.geometry import Rect

    doc = _letter()
    p = doc[0]
    p.set_artbox(Rect(11, 22, 333, 444))
    p = doc.reload_page(0)
    assert tuple(p.artbox) == (11.0, 22.0, 333.0, 444.0)


# === fitz-shim parity ======================================================


def test_lt5_fitz_shim_geometry():
    doc = fitz.open()
    p = doc.new_page(width=300, height=400)
    assert (p.mediabox_size.x, p.mediabox_size.y) == (300.0, 400.0)
    assert tuple(p.artbox) == tuple(p.cropbox)
    assert p.parent is doc
    assert isinstance(p.xref, int)
    assert isinstance(p.transformation_matrix, fitz.Matrix)
    _approx6(p.transformation_matrix, (1.0, 0.0, 0.0, -1.0, 0.0, 400.0))


def test_lt5_fitz_shim_setters():
    doc = fitz.open()
    p = doc.new_page(width=300, height=400)
    p.set_trimbox((5, 5, 200, 300))
    p = doc.reload_page(p.number)
    assert tuple(p.trimbox) == (5.0, 5.0, 200.0, 300.0)
