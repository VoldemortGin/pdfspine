"""``Page.get_text("layout")`` / ``Page.get_text_layout`` — layout-preserving
text with a y tolerance (pdfspine-original extension)."""

from __future__ import annotations

import pytest

import pdfspine
from pdfspine._layout import layout_text


def _page(items, width=612, height=792) -> pdfspine.Page:
    doc = pdfspine.open()
    page = doc.new_page(width=width, height=height)
    for x, y, text, size in items:
        page.insert_text((x, y), text, fontsize=size)
    return page


def test_jittered_words_form_one_visual_line():
    page = _page(
        [(72, 100, "Label", 12), (300, 100.4, "Value", 12), (450, 99.7, "End", 12)]
    )
    text = page.get_text("layout")
    assert text.count("\n") == 1
    assert text.split() == ["Label", "Value", "End"]
    assert text.index("Label") < text.index("Value") < text.index("End")


def test_tolerance_decides_line_splitting():
    page = _page([(72, 100, "Alpha", 12), (300, 105, "Beta", 12)])
    lines = page.get_text_layout().rstrip("\n").split("\n")
    assert [ln.split() for ln in lines] == [["Alpha"], ["Beta"]]
    wide = page.get_text_layout(y_tolerance=6).rstrip("\n").split("\n")
    assert [ln.split() for ln in wide] == [["Alpha", "Beta"]]
    tight = page.get_text_layout(y_tolerance=0).rstrip("\n").split("\n")
    assert len(tight) == 2


def test_columns_stay_aligned_across_lines():
    page = _page(
        [
            (72, 100, "Item", 12),
            (300, 100, "Price", 12),
            (72, 115, "Tea", 12),
            (300, 115.3, "2.50", 12),
        ]
    )
    first, second = page.get_text("layout").rstrip("\n").split("\n")
    assert first.startswith("Item") and second.startswith("Tea")
    assert first.index("Price") == second.index("2.50")
    assert first.index("Price") > 20


def test_char_width_scales_the_grid():
    page = _page([(72, 100, "A", 12), (300, 100, "B", 12)])
    narrow = page.get_text_layout(char_width=2.0)
    wide = page.get_text_layout(char_width=8.0)
    assert narrow.index("B") > wide.index("B") >= 2


def test_large_vertical_gap_becomes_a_blank_line():
    page = _page([(72, 100, "Top", 12), (72, 114, "Next", 12), (72, 146, "Far", 12)])
    assert page.get_text("layout") == "Top\nNext\n\nFar\n"


def test_words_are_left_aligned_to_the_leftmost_word():
    page = _page([(150, 100, "Indented", 12), (150, 114, "Same", 12)])
    assert page.get_text("layout") == "Indented\nSame\n"


def test_clip_filters_words():
    page = _page([(72, 100, "Keep", 12), (72, 300, "Drop", 12)])
    assert page.get_text_layout(clip=pdfspine.Rect(0, 0, 612, 200)) == "Keep\n"
    assert page.get_text("layout", clip=(0, 0, 612, 200)) == "Keep\n"


def test_empty_page_and_option_equivalence():
    empty = _page([])
    assert empty.get_text("layout") == ""
    page = _page([(72, 100, "Hello world", 12)])
    assert page.get_text("layout") == page.get_text_layout() == "Hello world\n"
    tp = page.get_textpage()
    assert page.get_text("layout", textpage=tp) == "Hello world\n"


def test_sort_is_ignored_for_layout():
    page = _page([(72, 100, "One", 12), (72, 114, "Two", 12)])
    assert page.get_text("layout", sort=True) == page.get_text("layout")


@pytest.mark.parametrize("kwargs", [{"y_tolerance": -1.0}, {"char_width": 0.0}])
def test_invalid_arguments_raise(kwargs):
    with pytest.raises(ValueError):
        _page([(72, 100, "x", 12)]).get_text_layout(**kwargs)


def test_layout_text_groups_by_anchor_not_by_neighbour():
    # Centers 100, 102, 104, 106: with tolerance 3 the anchor rule yields two
    # lines ({100, 102}, {104, 106}); chaining on the previous word would
    # wrongly merge all four.
    words = [
        (10, 95, 30, 105, "a", 0, 0, 0),
        (40, 97, 60, 107, "b", 0, 0, 1),
        (70, 99, 90, 109, "c", 0, 0, 2),
        (100, 101, 120, 111, "d", 0, 0, 3),
    ]
    assert layout_text(words, y_tolerance=3.0, char_width=10.0) == "a  b\n      c  d\n"


def test_layout_text_orders_words_by_x_within_a_line():
    words = [
        (200, 100, 240, 110, "right", 0, 0, 0),
        (10, 100.2, 50, 110.2, "left", 0, 0, 1),
    ]
    assert layout_text(words, char_width=10.0) == "left               right\n"
