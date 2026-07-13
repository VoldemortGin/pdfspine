"""Long-tail PyMuPDF parity batch 13 — deferred-symbol clean-up, pure-Python
cluster (COMPAT.toml parity long-tail).

Covers four ``Page`` / ``Document`` methods promoted from ``deferred`` to
``implemented`` in this batch, all expressible over existing pdfspine infra:

* ``Page.remove_rotation`` — bakes ``/Rotate`` into the content stream (as a
  ``cm`` prefix), swaps the media box for 90/270, resets rotation to 0 and
  rewrites annotation / link / widget rects; returns the inverse derotation
  matrix.
* ``Page.refresh`` — re-syncs the page handle in place (no-op for a parentless
  page).
* ``Page.write_text`` — renders one or more ``TextWriter`` objects onto a page.
* ``Document.insert_file`` — inserts an image / PDF source (Pixmap / Document /
  bytes / path) via the ``image_to_pdf`` + ``insert_pdf`` pipeline; rejects
  genuinely non-image, non-PDF input.

Every expected value below was captured from real PyMuPDF 1.24.x / 1.27
(``.venv-oracle``); the assertions double as the CI regression baseline since
the real package is not importable there.
"""

from __future__ import annotations

import pytest

import pdfspine
from pdfspine._core import PdfUnsupportedError


# ---------------------------------------------------------------------------
# Page.remove_rotation — oracle-captured derotation matrices + geometry.
# For a 200×300 page the inverse derotation matrix and the resulting page rect
# match PyMuPDF exactly; text drawn before derotation stays extractable.
# ---------------------------------------------------------------------------
_REMOVE_ROTATION_ORACLE = {
    0: ((1.0, 0.0, 0.0, 1.0, 0.0, 0.0), (0.0, 0.0, 200.0, 300.0)),
    90: ((0.0, 1.0, -1.0, 0.0, 200.0, 0.0), (0.0, 0.0, 300.0, 200.0)),
    180: ((-1.0, 0.0, 0.0, -1.0, 200.0, 300.0), (0.0, 0.0, 200.0, 300.0)),
    270: ((0.0, -1.0, 1.0, 0.0, 0.0, 300.0), (0.0, 0.0, 300.0, 200.0)),
}


@pytest.mark.parametrize("rot", [0, 90, 180, 270])
def test_remove_rotation_matches_oracle(rot: int) -> None:
    want_mat, want_rect = _REMOVE_ROTATION_ORACLE[rot]
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=300)
    page.insert_text((20, 40), "Hi", fontsize=12)
    page.set_rotation(rot)

    inv = page.remove_rotation()

    assert tuple(inv) == pytest.approx(want_mat, abs=1e-6)
    assert page.rotation == 0
    assert tuple(page.rect) == pytest.approx(want_rect, abs=1e-6)
    # 去旋后正文仍可抽取(视觉/文本内容保持不变)。
    assert "Hi" in page.get_text()
    doc.close()


def test_remove_rotation_rewrites_annot_rect() -> None:
    """A 90° derotation moves the annotation rect by the inverse matrix."""
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=300)
    annot = page.add_rect_annot(pdfspine.Rect(10, 10, 50, 50))
    before = pdfspine.Rect(*annot.rect)
    page.set_rotation(90)

    inv = page.remove_rotation()

    moved = before * inv
    # annot.rect 现在应落在逆矩阵变换后的位置。
    live = next(page.annots())
    assert tuple(live.rect) == pytest.approx(tuple(moved), abs=1e-3)
    doc.close()


def test_remove_rotation_zero_is_identity_noop() -> None:
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=300)
    contents_before = page.read_contents()
    inv = page.remove_rotation()
    assert tuple(inv) == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
    assert page.read_contents() == contents_before  # 未旋转 → 不改内容
    doc.close()


# ---------------------------------------------------------------------------
# Page.refresh — re-syncs the handle, returns None, page stays usable.
# ---------------------------------------------------------------------------
def test_refresh_returns_none_and_keeps_page_usable() -> None:
    doc = pdfspine.open()
    page = doc.new_page(width=200, height=300)
    page.insert_text((20, 40), "keep", fontsize=12)
    assert page.refresh() is None
    assert page.number == 0
    assert "keep" in page.get_text()
    doc.close()


# ---------------------------------------------------------------------------
# Page.write_text — renders TextWriter content (oracle: text round-trips).
# ---------------------------------------------------------------------------
def test_write_text_single_writer_round_trips() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    tw = pdfspine.TextWriter(page.rect)
    tw.append((72, 72), "Hello parity", fontsize=14)
    page.write_text(writers=tw)
    assert "Hello parity" in page.get_text()
    doc.close()


def test_write_text_requires_a_writer() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    with pytest.raises(ValueError):
        page.write_text(writers=None)
    doc.close()


# ---------------------------------------------------------------------------
# Document.insert_file — image / PDF sources append pages; junk is rejected.
# ---------------------------------------------------------------------------
def test_insert_file_pixmap_appends_page_preserving_aspect() -> None:
    doc = pdfspine.open()
    doc.new_page()
    pm = pdfspine.Pixmap(pdfspine.csRGB, (0, 0, 60, 40), False)
    doc.insert_file(pm)
    assert doc.page_count == 2
    rect = doc[-1].rect
    # 60:40 图片纵横比在转 PDF 后保持(与默认 DPI 无关)。
    assert rect.width / rect.height == pytest.approx(1.5, abs=1e-3)
    doc.close()


def test_insert_file_pdf_document_source() -> None:
    src = pdfspine.open()
    src.new_page()
    src.new_page()
    dst = pdfspine.open()
    dst.new_page()
    dst.insert_file(src)
    assert dst.page_count == 3
    src.close()
    dst.close()


def test_insert_file_rejects_non_image_bytes() -> None:
    doc = pdfspine.open()
    with pytest.raises(PdfUnsupportedError):
        doc.insert_file(b"this is not an image or a pdf")
    doc.close()
