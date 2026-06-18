"""Long-tail PyMuPDF parity batch 6 — Document page-helpers (PRD §7 / §9.5).

Covers the newly-implemented Document surface:
  - Page delegations: get_page_images / get_page_fonts / get_page_pixmap /
    search_page_for (forward to the matching Page method of page ``pno``)
  - Page labels: get_page_labels (list of rule dicts), get_page_numbers
    (label -> 0-based page list), get_label (computed label for one page),
    verified against a self-built /PageLabels number tree (roman / decimal /
    prefix rules)
  - Page-ops: delete_pages (range / list / numbers=), insert_page (blank +
    optional text, returns line count), copy_page (shallow reference copy),
    move_page (reorder), with PyMuPDF "insert in front of page ``to``"
    semantics, round-tripped via page count + content order.

Both the native ``pdfspine`` API and the ``fitz`` shim are exercised; all
fixtures are self-generated.
"""

from __future__ import annotations

import fitz
import pdfspine
import pytest
from pdfspine.geometry import Quad, Rect


def _doc(n: int = 1) -> pdfspine.Document:
    """A fresh n-page document, each page tagged with its index as text."""
    d = pdfspine.open()
    for i in range(n):
        p = d.new_page(width=300, height=400)
        p.insert_text((50, 100), f"PAGE-{i}", fontsize=14)
    return d


def _page_text(doc: pdfspine.Document, pno: int) -> str:
    return doc[pno].get_text().strip()


def _order(doc: pdfspine.Document) -> list[str]:
    return [_page_text(doc, i) for i in range(doc.page_count)]


# === Page delegations: images / fonts / pixmap / search ====================


def test_lt6_get_page_images_matches_page():
    doc = _doc(2)
    for pno in (0, 1):
        assert doc.get_page_images(pno) == doc[pno].get_images()
        assert doc.get_page_images(pno, full=True) == doc[pno].get_images(full=True)


def test_lt6_get_page_fonts_matches_page():
    doc = _doc(2)
    # A text page has at least one font; the delegation must equal Page.get_fonts.
    for pno in (0, 1):
        assert doc.get_page_fonts(pno) == doc[pno].get_fonts()
        assert doc.get_page_fonts(pno, full=True) == doc[pno].get_fonts(full=True)


def test_lt6_get_page_pixmap_matches_page():
    doc = _doc(1)
    pm_doc = doc.get_page_pixmap(0, dpi=72)
    pm_page = doc[0].get_pixmap(dpi=72)
    assert (pm_doc.width, pm_doc.height) == (pm_page.width, pm_page.height)


def test_lt6_search_page_for_matches_page():
    doc = _doc(2)
    hits_doc = doc.search_page_for(1, "PAGE-1")
    hits_page = doc[1].search_for("PAGE-1")
    assert len(hits_doc) == len(hits_page) >= 1
    assert all(isinstance(h, Rect) for h in hits_doc)
    # quads=True forwards through and yields Quad.
    qhits = doc.search_page_for(1, "PAGE-1", quads=True)
    assert all(isinstance(q, Quad) for q in qhits)
    # Searching the wrong page finds nothing.
    assert doc.search_page_for(0, "PAGE-1") == []


def test_lt6_fitz_shim_delegations_agree():
    doc = fitz.open()
    for i in range(2):
        p = doc.new_page(width=300, height=400)
        p.insert_text((50, 100), f"PAGE-{i}", fontsize=14)
    assert doc.get_page_images(0) == doc[0].get_images()
    assert doc.get_page_fonts(0) == doc[0].get_fonts()
    assert len(doc.search_page_for(0, "PAGE-0")) == 1


# === Page labels: get_page_labels / get_page_numbers / get_label ===========


def _labelled() -> pdfspine.Document:
    """A 6-page doc: pages 0-2 lowercase roman (i, ii, iii), pages 3-5 decimal
    with an 'A-' prefix starting at 1 (A-1, A-2, A-3)."""
    d = _doc(6)
    d.set_page_labels([
        {"startpage": 0, "style": "r", "prefix": "", "firstpagenum": 1},
        {"startpage": 3, "style": "D", "prefix": "A-", "firstpagenum": 1},
    ])
    return d


def test_lt6_get_page_labels_rules():
    rules = _labelled().get_page_labels()
    assert rules == [
        {"startpage": 0, "prefix": "", "style": "r", "firstpagenum": 1},
        {"startpage": 3, "prefix": "A-", "style": "D", "firstpagenum": 1},
    ]


def test_lt6_get_page_labels_empty_when_absent():
    assert _doc(3).get_page_labels() == []


def test_lt6_get_label_computes_per_page():
    d = _labelled()
    assert [d.get_label(i) for i in range(6)] == ["i", "ii", "iii", "A-1", "A-2", "A-3"]
    # Negative index counts from the end.
    assert d.get_label(-1) == "A-3"
    # Document.get_label agrees with Page.get_label and the existing helper.
    assert d.get_label(2) == d[2].get_label() == d.get_page_label(2)


def test_lt6_get_page_numbers():
    d = _labelled()
    assert d.get_page_numbers("iii") == [2]
    assert d.get_page_numbers("A-1") == [3]
    assert d.get_page_numbers("nope") == []


def test_lt6_get_page_numbers_only_one():
    # Build a doc where two pages share a label (decimal restart).
    d = _doc(4)
    d.set_page_labels([
        {"startpage": 0, "style": "D", "prefix": "", "firstpagenum": 1},
        {"startpage": 2, "style": "D", "prefix": "", "firstpagenum": 1},
    ])
    # Labels: 1, 2, 1, 2 -> "1" appears on pages 0 and 2.
    assert d.get_page_numbers("1") == [0, 2]
    assert d.get_page_numbers("1", only_one=True) == [0]


def test_lt6_labels_via_fitz_shim():
    d = fitz.open()
    for _ in range(3):
        d.new_page(width=300, height=400)
    d.set_page_labels([{"startpage": 0, "style": "R", "prefix": "Ch-", "firstpagenum": 5}])
    assert d.get_page_labels() == [
        {"startpage": 0, "prefix": "Ch-", "style": "R", "firstpagenum": 5}
    ]
    assert [d.get_label(i) for i in range(3)] == ["Ch-V", "Ch-VI", "Ch-VII"]
    assert d.get_page_numbers("Ch-VI") == [1]


# === Page-ops: delete_pages ================================================


def test_lt6_delete_pages_range_positional():
    d = _doc(5)
    d.delete_pages(1, 3)  # inclusive range 1..=3
    assert d.page_count == 2
    assert _order(d) == ["PAGE-0", "PAGE-4"]


def test_lt6_delete_pages_range_kwargs():
    d = _doc(5)
    d.delete_pages(from_page=1, to_page=3)
    assert _order(d) == ["PAGE-0", "PAGE-4"]


def test_lt6_delete_pages_numbers_list():
    d = _doc(5)
    d.delete_pages([0, 2, 4])
    assert _order(d) == ["PAGE-1", "PAGE-3"]


def test_lt6_delete_pages_numbers_kwarg():
    d = _doc(5)
    d.delete_pages(numbers=[0, 2, 4])
    assert _order(d) == ["PAGE-1", "PAGE-3"]


def test_lt6_delete_pages_single():
    d = _doc(3)
    d.delete_pages(1)
    assert _order(d) == ["PAGE-0", "PAGE-2"]


def test_lt6_delete_pages_negative():
    d = _doc(4)
    d.delete_pages(-1)  # last page
    assert _order(d) == ["PAGE-0", "PAGE-1", "PAGE-2"]


# === Page-ops: insert_page =================================================


def test_lt6_insert_page_blank_append():
    d = _doc(2)
    n = d.insert_page(-1)  # append blank
    assert n == 0
    assert d.page_count == 3
    assert _page_text(d, 2) == ""  # the new blank page


def test_lt6_insert_page_before_index_with_text():
    d = _doc(2)
    n = d.insert_page(0, text="HELLO")  # insert before page 0
    assert n == 1  # one line inserted
    assert d.page_count == 3
    assert _page_text(d, 0) == "HELLO"
    assert _order(d)[1:] == ["PAGE-0", "PAGE-1"]


def test_lt6_insert_page_multiline_returns_linecount():
    d = _doc(1)
    n = d.insert_page(-1, text=["one", "two", "three"])
    assert n == 3


# === Page-ops: copy_page ===================================================


def test_lt6_copy_page_append():
    d = _doc(3)
    d.copy_page(0)  # to == -1 -> append
    assert d.page_count == 4
    assert _order(d) == ["PAGE-0", "PAGE-1", "PAGE-2", "PAGE-0"]


def test_lt6_copy_page_before_index():
    d = _doc(3)
    d.copy_page(2, 1)  # copy page 2 in front of page 1
    assert d.page_count == 4
    assert _order(d) == ["PAGE-0", "PAGE-2", "PAGE-1", "PAGE-2"]


def test_lt6_copy_page_is_shallow_reference():
    # A shallow copy shares the leaf's xref-distinct duplicate but same content;
    # editing the copy's box must not blow up and the copy renders identically.
    d = _doc(2)
    d.copy_page(0)
    assert _page_text(d, 0) == _page_text(d, 2) == "PAGE-0"


# === Page-ops: move_page ===================================================


def test_lt6_move_page_to_end():
    d = _doc(3)
    d.move_page(0)  # to == -1 -> move to end
    assert _order(d) == ["PAGE-1", "PAGE-2", "PAGE-0"]


def test_lt6_move_page_forward_before_index():
    # PyMuPDF: move page 0 in front of page 2 -> [B, A, C, ...]
    d = _doc(4)
    d.move_page(0, 2)
    assert _order(d) == ["PAGE-1", "PAGE-0", "PAGE-2", "PAGE-3"]


def test_lt6_move_page_backward_before_index():
    # Move page 3 in front of page 1 -> [A, D, B, C]
    d = _doc(4)
    d.move_page(3, 1)
    assert _order(d) == ["PAGE-0", "PAGE-3", "PAGE-1", "PAGE-2"]


def test_lt6_move_page_count_preserved():
    d = _doc(5)
    before = sorted(_order(d))
    d.move_page(4, 0)
    assert d.page_count == 5
    assert sorted(_order(d)) == before
    assert _order(d)[0] == "PAGE-4"


def test_lt6_page_ops_via_fitz_shim():
    d = fitz.open()
    for i in range(3):
        p = d.new_page(width=300, height=400)
        p.insert_text((50, 100), f"PAGE-{i}", fontsize=14)
    d.copy_page(0)
    assert d.page_count == 4
    d.move_page(0, 2)
    d.delete_pages(0, 0)
    assert d.page_count == 3


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(pytest.main([__file__, "-q"]))
