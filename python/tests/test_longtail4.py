"""Long-tail PyMuPDF parity batch 4 (PRD §7 / §9.5).

Covers the newly-implemented surface:
  - Link (fitz.Link): rect / kind / uri / page / dest / is_external / border /
    colors / flags / xref / next / linkDest; Page.first_link + Page.links()
  - Outline (fitz.Outline): title / page / uri / is_open / is_external / dest /
    next / down; Document.outline
  - TextWriter (fitz.TextWriter): append / appendv / fill_textbox / write_text /
    text_rect / last_point / color, rendered into a page
  - Colorspace (fitz.Colorspace) + csGRAY / csRGB / csCMYK + CS_* constants;
    Pixmap(colorspace, irect) ctor
  - Annot members: get_text / get_textpage / next
  - Text helpers: Page.get_text_blocks / get_text_words / get_textbox /
    get_text_selection; TextPage.extractRAWJSON

Both the native ``oxide_pdf`` API and the ``fitz`` shim are exercised; all
fixtures are self-generated.
"""

from __future__ import annotations

import json

import fitz
import oxide_pdf
import pytest


def _doc_with_text() -> oxide_pdf.Document:
    d = oxide_pdf.open()
    p = d.new_page()
    p.insert_text((72, 100), "Hello world here", fontsize=12)
    return d


def _doc_with_pages(n: int = 3) -> oxide_pdf.Document:
    d = oxide_pdf.open()
    for _ in range(n):
        d.new_page()
    return d


# === Link ================================================================


def test_first_link_uri() -> None:
    d = _doc_with_pages()
    p = d[0]
    p.insert_link({"kind": 2, "from": (10, 10, 100, 30), "uri": "https://x.test"})
    link = p.first_link
    assert link is not None
    assert link.kind == 2
    assert link.uri == "https://x.test"
    assert link.is_external is True
    assert link.page == -1
    assert tuple(link.rect) == (10.0, 10.0, 100.0, 30.0)
    assert isinstance(link.xref, int) and link.xref > 0


def test_first_link_goto_and_dest() -> None:
    d = _doc_with_pages()
    p = d[0]
    p.insert_link({"kind": 1, "from": (10, 40, 100, 60), "page": 2})
    link = p.first_link
    assert link.kind == 1
    assert link.page == 2
    assert link.is_external is False
    dest = link.dest
    assert dest.page == 2
    assert link.linkDest.kind == 1


def test_link_next_chain() -> None:
    d = _doc_with_pages()
    p = d[0]
    p.insert_link({"kind": 2, "from": (0, 0, 10, 10), "uri": "https://a.test"})
    p.insert_link({"kind": 1, "from": (0, 20, 10, 30), "page": 1})
    first = p.first_link
    assert first.next is not None
    assert first.next.kind == 1
    assert first.next.page == 1
    assert first.next.next is None
    # Walk the chain PyMuPDF-style.
    kinds = []
    link = p.first_link
    while link:
        kinds.append(link.kind)
        link = link.next
    assert kinds == [2, 1]


def test_links_iterator() -> None:
    d = _doc_with_pages()
    p = d[0]
    p.insert_link({"kind": 2, "from": (0, 0, 10, 10), "uri": "https://a.test"})
    p.insert_link({"kind": 1, "from": (0, 20, 10, 30), "page": 1})
    assert [l.kind for l in p.links()] == [2, 1]
    assert [l.kind for l in p.links(kinds=[2])] == [2]


def test_link_border_color_flags() -> None:
    d = _doc_with_pages()
    p = d[0]
    p.insert_link({"kind": 2, "from": (0, 0, 10, 10), "uri": "https://a.test"})
    link = p.first_link
    # Default inserted link has a 0-width border, no color, 0 flags.
    assert link.border["width"] == 0.0
    assert link.colors["stroke"] is None
    assert link.flags == 0


def test_no_links() -> None:
    d = _doc_with_pages()
    assert d[0].first_link is None
    assert list(d[0].links()) == []


def test_fitz_link_alias() -> None:
    assert fitz.Link is oxide_pdf.Link
    assert fitz.linkDest is oxide_pdf.linkDest


# === Outline =============================================================


def test_outline_tree() -> None:
    d = _doc_with_pages()
    d.set_toc([[1, "Chapter 1", 0], [2, "Section 1.1", 1], [1, "Chapter 2", 2]])
    ol = d.outline
    assert ol is not None
    assert ol.title == "Chapter 1"
    assert ol.page == 0
    assert ol.is_open is True
    assert ol.is_external is False
    # down → child, next → sibling.
    assert ol.down.title == "Section 1.1"
    assert ol.down.page == 1
    assert ol.next.title == "Chapter 2"
    assert ol.next.page == 2
    assert ol.next.down is None


def test_outline_walk() -> None:
    d = _doc_with_pages()
    d.set_toc([[1, "A", 0], [1, "B", 1], [1, "C", 2]])
    titles = []
    ol = d.outline
    while ol:
        titles.append(ol.title)
        ol = ol.next
    assert titles == ["A", "B", "C"]


def test_outline_none() -> None:
    d = _doc_with_pages()
    assert d.outline is None


def test_outline_dest() -> None:
    d = _doc_with_pages()
    d.set_toc([[1, "A", 0]])
    dest = d.outline.dest
    assert dest.page == 0
    assert dest.kind == 1
    assert d.outline.destination.page == 0


def test_fitz_outline_alias() -> None:
    assert fitz.Outline is oxide_pdf.Outline


# === Colorspace ==========================================================


def test_colorspace_singletons() -> None:
    assert oxide_pdf.csGRAY.n == 1
    assert oxide_pdf.csRGB.n == 3
    assert oxide_pdf.csCMYK.n == 4
    assert oxide_pdf.csRGB.name == "DeviceRGB"
    assert oxide_pdf.csGRAY.name == "DeviceGray"
    assert oxide_pdf.csCMYK.name == "DeviceCMYK"
    assert oxide_pdf.csGRAY.is_gray is True
    assert oxide_pdf.csRGB.is_gray is False


def test_colorspace_ctor() -> None:
    assert oxide_pdf.Colorspace(oxide_pdf.CS_RGB).name == "DeviceRGB"
    assert oxide_pdf.Colorspace(oxide_pdf.CS_GRAY).n == 1
    assert oxide_pdf.Colorspace(oxide_pdf.CS_CMYK).n == 4
    with pytest.raises(ValueError):
        oxide_pdf.Colorspace(99)


def test_colorspace_equality() -> None:
    assert oxide_pdf.Colorspace(oxide_pdf.CS_RGB) == oxide_pdf.csRGB
    assert oxide_pdf.csRGB != oxide_pdf.csGRAY


def test_fitz_colorspace_constants() -> None:
    assert fitz.csRGB.n == 3
    assert fitz.csGRAY.n == 1
    assert fitz.csCMYK.n == 4
    assert fitz.Colorspace(fitz.CS_RGB).name == "DeviceRGB"


def test_pixmap_with_colorspace_object() -> None:
    pm = oxide_pdf.Pixmap(oxide_pdf.csRGB, (0, 0, 4, 4))
    assert pm.width == 4 and pm.height == 4
    assert pm.n == 3
    assert pm.colorspace == "DeviceRGB"
    pmg = oxide_pdf.Pixmap(oxide_pdf.csGRAY, (0, 0, 4, 4))
    assert pmg.n == 1


# === TextWriter ==========================================================


def test_textwriter_append_metrics() -> None:
    tw = oxide_pdf.TextWriter((0, 0, 400, 400))
    tw.append((50, 50), "Hello")
    assert tw.last_point.x > 50
    assert tw.last_point.y == 50
    tr = tw.text_rect
    assert tr.x0 == 50
    assert tr.x1 > tr.x0


def test_textwriter_write_renders_into_page() -> None:
    d = oxide_pdf.open()
    p = d.new_page()
    tw = oxide_pdf.TextWriter(p.rect)
    tw.append((72, 100), "Rendered Text")
    tw.write_text(p)
    assert "Rendered" in p.get_text()


def test_textwriter_fill_textbox_wraps() -> None:
    tw = oxide_pdf.TextWriter((0, 0, 200, 200))
    overflow = tw.fill_textbox(
        (0, 0, 60, 200), "one two three four five six seven", fontsize=10
    )
    # Everything fits vertically into the tall box.
    assert overflow == []
    assert len(tw._segments) > 1  # wrapped into multiple lines


def test_textwriter_fill_textbox_overflow() -> None:
    tw = oxide_pdf.TextWriter((0, 0, 200, 200))
    # A very short box forces overflow lines.
    overflow = tw.fill_textbox(
        (0, 0, 30, 12), "aaa bbb ccc ddd eee fff", fontsize=10
    )
    assert overflow  # some lines did not fit


def test_textwriter_appendv() -> None:
    tw = oxide_pdf.TextWriter((0, 0, 200, 200))
    tw.appendv((10, 10), "abc", fontsize=10)
    assert len(tw._segments) == 3
    assert tw.last_point.y > 10


def test_textwriter_color() -> None:
    tw = oxide_pdf.TextWriter((0, 0, 200, 200), color=(1, 0, 0))
    assert tw.color == (1.0, 0.0, 0.0)


def test_fitz_textwriter() -> None:
    assert fitz.TextWriter is oxide_pdf.TextWriter
    d = fitz.open()
    p = d.new_page()
    tw = fitz.TextWriter(p.rect)
    tw.append((72, 100), "Via fitz")
    tw.write_text(p)
    assert "Via fitz" in p.get_text()


# === Annot members =======================================================


def test_annot_get_text() -> None:
    d = _doc_with_text()
    p = d[0]
    a = p.add_rect_annot((60, 85, 320, 110))
    txt = a.get_text()
    assert "Hello" in txt


def test_annot_get_textpage() -> None:
    d = _doc_with_text()
    p = d[0]
    a = p.add_rect_annot((60, 85, 320, 110))
    tp = a.get_textpage()
    assert isinstance(tp, oxide_pdf.TextPage)


def test_annot_next() -> None:
    d = _doc_with_text()
    p = d[0]
    a1 = p.add_rect_annot((60, 85, 320, 110))
    a2 = p.add_rect_annot((60, 120, 320, 140))
    first = p.first_annot
    assert first.next is not None
    assert first.next.xref == a2.xref
    assert first.next.next is None


# === Text helpers ========================================================


def test_get_text_words() -> None:
    d = _doc_with_text()
    words = d[0].get_text_words()
    assert len(words) == 3
    assert {w[4] for w in words} == {"Hello", "world", "here"}


def test_get_text_words_clipped() -> None:
    d = _doc_with_text()
    p = d[0]
    all_words = p.get_text_words()
    clipped = p.get_text_words(clip=(0, 80, 300, 120))
    assert len(clipped) <= len(all_words)
    assert len(clipped) >= 1


def test_get_text_blocks() -> None:
    d = _doc_with_text()
    blocks = d[0].get_text_blocks()
    assert len(blocks) >= 1
    assert "Hello" in blocks[0][4]


def test_get_textbox() -> None:
    d = _doc_with_text()
    txt = d[0].get_textbox((0, 80, 400, 120))
    assert "Hello world here" == txt.strip()


def test_get_text_selection() -> None:
    d = _doc_with_text()
    txt = d[0].get_text_selection((0, 80), (400, 120))
    assert "Hello" in txt


def test_extract_rawjson() -> None:
    d = _doc_with_text()
    tp = d[0].get_textpage()
    raw = tp.extractRAWJSON()
    parsed = json.loads(raw)
    span = parsed["blocks"][0]["lines"][0]["spans"][0]
    assert "chars" in span
    assert len(span["chars"]) > 0


def test_fitz_text_helpers() -> None:
    d = fitz.open()
    p = d.new_page()
    p.insert_text((72, 100), "fitz words", fontsize=12)
    assert len(p.get_text_words()) == 2
    assert "fitz" in p.get_textbox((0, 80, 400, 120))
