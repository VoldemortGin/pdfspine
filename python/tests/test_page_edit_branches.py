"""``DOCPY-*`` (page family) — long-tail branch coverage for the ``Page`` /
``Annot`` / ``Widget`` / ``Shape`` / ``Table`` wrappers in ``pdfspine.document``.

Content-edit surface: annotation add/setter/alias methods, form widgets
(authoring + existing), ``Shape`` drawing (curve/oval/sector/squiggle/zigzag),
``cluster_drawings``, ``remove_rotation`` (link path), ``write_text`` (composed
path), ``text_in_rect`` / ``content_blocks`` / ``filled_rectangles``, link
objects, native table detection and ``insert_image`` / pixmap / matrix argument
branches.

All fixtures are generated in-code; every document is closed for the ``-W error``
gate.
"""

from __future__ import annotations

import math

import pdfspine
import pytest

from pdfspine.document import Annot, Page, Shape, _color, _quad, _quads


# --- self-generated PDF assembler (classic xref) --------------------------
def _build_pdf(
    objects: list[tuple[int, bytes]], root: int, extra_trailer: bytes = b""
) -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    max_num = 0
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
        max_num = max(max_num, num)
    size = max_num + 1
    startxref = len(out)
    out += b"xref\n"
    out += f"0 {size}\n".encode()
    out += b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n"
    out += f"<< /Size {size} /Root {root} 0 R {extra_trailer.decode()} >>\n".encode()
    out += b"startxref\n"
    out += f"{startxref}\n".encode()
    out += b"%%EOF\n"
    return bytes(out)


def _table_pdf() -> bytes:
    """A 2x2 ruled table with text in the first row — native ``find_tables``."""
    content = (
        b"1 w 0 G "
        b"50 50 m 250 50 l S 50 100 m 250 100 l S 50 150 m 250 150 l S "
        b"50 50 m 50 150 l S 150 50 m 150 150 l S 250 50 m 250 150 l S "
        b"BT /F1 10 Tf 60 120 Td (A) Tj 100 0 Td (B) Tj ET"
    )
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] "
                b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (
                4,
                b"<< /Length "
                + str(len(content)).encode()
                + b" >>\nstream\n"
                + content
                + b"\nendstream",
            ),
            (5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ],
        1,
    )


def _text_widget(doc: pdfspine.Document, page: pdfspine.Page, name: str = "f1"):
    w = pdfspine.Widget()
    w.field_name = name
    w.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
    w.rect = pdfspine.Rect(20, 40, 120, 70)
    w.field_value = "v"
    w.field_flags = 4096
    w.text_color = (1, 0, 0)
    w.text_font = "Helv"
    w.text_fontsize = 10.0
    return page.add_widget(w)


# =====================================================================
# DOCPY-030 — Annot getters / setters / camelCase aliases
# =====================================================================
def test_docpy_030_annot_setters_getters_aliases() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=200)
        page.insert_text((50, 100), "Hello world")
        annot = page.add_text_annot((20, 20), "note")

        assert annot.info["content"] == "note"
        annot.set_info(info={"content": "c", "title": "t", "name": "n"})
        assert annot.info["title"] == "t"
        annot.set_colors(colors={"stroke": (1, 0, 0), "fill": None})
        annot.set_border(border={"width": 2})
        annot.set_opacity(0.5)
        annot.set_flags(4)
        assert annot.flags == 4
        assert 0.0 <= annot.opacity <= 1.0
        assert isinstance(annot.vertices, list)
        assert isinstance(annot.has_ap(), bool)
        assert isinstance(annot.apn_bbox(), pdfspine.Rect)
        assert annot.get_text().startswith("Hello")
        assert isinstance(annot.get_textpage(), pdfspine.TextPage)
        assert repr(annot).startswith("<pdfspine.Annot ")

        # Deprecated camelCase aliases forward to the snake_case methods.
        annot.setRect(pdfspine.Rect(10, 10, 40, 40))
        annot.setColors(stroke=(0, 0, 1))
        annot.setOpacity(0.9)
        annot.setBorder(width=1)
        annot.setInfo(content="c2")
        annot.setFlags(2)
        assert annot.flags == 2


def test_docpy_030_annot_next_chain() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        page.add_rect_annot(pdfspine.Rect(10, 10, 40, 40))
        page.add_rect_annot(pdfspine.Rect(50, 50, 80, 80))
        first = page.first_annot
        assert first is not None
        assert first.next is not None
        assert first.next.next is None


# =====================================================================
# DOCPY-031 — Widget authoring (new) + existing (reopened) branches
# =====================================================================
def test_docpy_031_new_widget_buffers_all_attributes() -> None:
    w = pdfspine.Widget()
    w.rect = pdfspine.Rect(10, 10, 100, 40)
    w.field_name = "author"
    w.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
    w.field_value = "hello"
    w.field_flags = 2
    w.choice_values = ["x", "y"]
    w.text_color = (0.1, 0.2, 0.3)
    w.text_font = "Cour"
    w.text_fontsize = 9.0
    assert w.rect == pdfspine.Rect(10, 10, 100, 40)
    assert w.field_name == "author"
    assert w.field_value == "hello"
    assert w.field_flags == 2
    assert w.choice_values == ["x", "y"]
    assert w.text_color == [0.1, 0.2, 0.3]
    assert w.text_font == "Cour"
    assert w.text_fontsize == 9.0


def test_docpy_031_existing_widget_getters_and_readonly() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        _text_widget(doc, page)
        data = doc.tobytes()
    with pdfspine.open(stream=data) as reopened:
        w = reopened[0].widgets()[0]
        # getters that read from the core widget
        assert isinstance(w.xref, int)
        assert w.field_label is None or isinstance(w.field_label, str)
        assert isinstance(w.button_states, list)
        assert w.field_flags == 4096
        assert w.text_color == [1.0, 0.0, 0.0]
        assert w.text_font == "Helv"
        assert w.text_fontsize == 10.0
        assert w.field_type_string == "Text"

        # every structural attribute is read-only on an existing widget
        for attr, value in [
            ("rect", pdfspine.Rect(0, 0, 1, 1)),
            ("field_type", 1),
            ("field_name", "x"),
            ("field_flags", 1),
            ("choice_values", ["a"]),
            ("text_color", (0, 0, 0)),
            ("text_font", "Cour"),
            ("text_fontsize", 5.0),
        ]:
            with pytest.raises(pdfspine.PdfUnsupportedError):
                setattr(w, attr, value)

        # update() with no pending value regenerates the appearance in place...
        w.update()
        # ...while field_value is buffered (not read-only) and written on update.
        w.field_value = "typed"
        assert w.field_value == "typed"
        w.update()
        assert repr(w).startswith("<pdfspine.Widget ")


# =====================================================================
# DOCPY-032 — add_widget value coercion + rect requirement
# =====================================================================
def test_docpy_032_add_widget_value_coercion() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=300)

        cb = pdfspine.Widget()
        cb.field_name = "agree"
        cb.field_type = pdfspine.PDF_WIDGET_TYPE_CHECKBOX
        cb.rect = pdfspine.Rect(10, 10, 30, 30)
        cb.field_value = True  # -> "Yes"
        assert page.add_widget(cb).type == (21, "Widget")

        empty = pdfspine.Widget()
        empty.field_name = "blank"
        empty.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
        empty.rect = pdfspine.Rect(10, 50, 120, 70)
        empty.field_value = None  # -> ""
        assert page.add_widget(empty).type == (21, "Widget")

        norect = pdfspine.Widget()
        norect.field_name = "bad"
        norect.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
        with pytest.raises(ValueError, match="rect"):
            page.add_widget(norect)


# =====================================================================
# DOCPY-033 — load_annot / load_widget / delete_widget
# =====================================================================
def test_docpy_033_load_annot_by_name_and_xref() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        annot = page.add_text_annot((20, 20), "hi")
        annot.set_info(name="TAG1")
        xref = annot.xref

        assert page.load_annot("TAG1").xref == xref
        assert page.load_annot(xref).info["name"] == "TAG1"
        with pytest.raises(ValueError, match="not an annot"):
            page.load_annot("nope")
        with pytest.raises(ValueError, match="not an annot"):
            page.load_annot(999999)
        with pytest.raises(ValueError, match="string or integer"):
            page.load_annot(1.5)


def test_docpy_033_load_and_delete_widget() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=300)
        a1 = _text_widget(doc, page, "w1")
        w2 = pdfspine.Widget()
        w2.field_name = "w2"
        w2.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
        w2.rect = pdfspine.Rect(20, 100, 120, 130)
        w2.field_value = "v2"
        page.add_widget(w2)

        loaded = page.load_widget(a1.xref)
        assert loaded.field_name == "w1"
        with pytest.raises(ValueError, match="not a widget"):
            page.load_widget(999999)

        widgets = page.widgets()
        nxt = page.delete_widget(widgets[0])
        assert nxt is not None  # the next widget is returned
        assert page.first_widget is not None
        assert page.firstWidget is page.first_widget or page.firstWidget is not None


# =====================================================================
# DOCPY-034 — Shape drawing helpers
# =====================================================================
def test_docpy_034_shape_draw_helpers() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=300)
        shape = page.new_shape()
        shape.draw_oval(pdfspine.Rect(10, 10, 90, 60))
        shape.draw_curve([(10, 100), (40, 130), (80, 90), (120, 120)])
        q = shape.draw_curve3((10, 200), (40, 240), (80, 200))
        assert isinstance(q, pdfspine.Point)
        shape.finish(color=(0, 0, 0), fill=(0.5, 0.5, 0.5), width=1.5)
        shape.commit(overlay=True)
        assert shape.rect is not None
        shape.updateRect((5, 5))  # camelCase alias
        assert repr(shape) == "<pdfspine.Shape>"


def test_docpy_034_shape_sector_squiggle_zigzag() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=400, height=400)
        shape = page.new_shape()
        # angle > 360 exercises the 2*pi normalization loop.
        end = shape.draw_sector((200, 200), (260, 200), 720)
        assert isinstance(end, pdfspine.Point)
        shape.draw_squiggle((20, 300), (180, 300), breadth=3)
        shape.draw_zigzag((20, 340), (180, 340), breadth=3)
        shape.finish(color=(0, 0, 0))
        shape.commit()

        with pytest.raises(ValueError, match="radius"):
            page.new_shape().draw_sector((10, 10), (10, 10), 90)
        with pytest.raises(ValueError, match="too close"):
            page.new_shape().draw_squiggle((0, 0), (1, 0))


def test_docpy_034_shape_insert_text_needs_page() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        # A page-backed shape writes through to the page.
        shape = page.new_shape()
        assert shape.insert_text((20, 20), ["a", "b"]) == 2
        assert isinstance(
            shape.insert_textbox(pdfspine.Rect(0, 0, 100, 80), ["x"]), float
        )

        # A page-less shape raises for text emission.
        orphan = Shape(page._page.new_shape())
        with pytest.raises(pdfspine.PdfUnsupportedError):
            orphan.insert_text((0, 0), "x")
        with pytest.raises(pdfspine.PdfUnsupportedError):
            orphan.insert_textbox(pdfspine.Rect(0, 0, 10, 10), "x")


@pytest.mark.parametrize(
    "c,p,expected",
    [
        ((0, 0), (1, 0), 0.0),  # +x axis
        ((0, 0), (0, 1), math.pi / 2),  # +y (s.x >= 0, s.y > 0)
        ((0, 0), (-1, 0), -math.pi),  # -x, s.y == 0 (s.y <= 0 branch)
        ((0, 0), (-1, 1), math.pi * 3 / 4),  # s.x < 0, s.y > 0
        ((0, 0), (-1, -1), -math.pi * 3 / 4),  # s.x < 0, s.y < 0
        ((0, 0), (1, -1), -math.pi / 4),  # s.x >= 0, s.y < 0
    ],
)
def test_docpy_034_horizontal_angle_quadrants(c, p, expected) -> None:
    assert Shape.horizontal_angle(c, p) == pytest.approx(expected)


# =====================================================================
# DOCPY-035 — Page draw + annotation camelCase aliases
# =====================================================================
def test_docpy_035_page_draw_and_annot_aliases() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=400, height=400)
        # snake draw primitives (each is a thin one-liner)
        page.draw_circle((50, 50), 20, fill=(1, 0, 0))
        page.draw_oval(pdfspine.Rect(80, 80, 140, 120))
        page.draw_bezier((10, 200), (30, 220), (60, 220), (80, 200))
        page.draw_polyline([(10, 10), (40, 40), (70, 10)])

        # camelCase draw aliases
        page.drawLine((0, 0), (10, 10))
        page.drawRect(pdfspine.Rect(0, 0, 10, 10))
        page.drawCircle((100, 100), 5)
        page.drawOval(pdfspine.Rect(120, 120, 160, 150))
        page.drawBezier((0, 0), (1, 1), (2, 2), (3, 3))
        page.drawPolyline([(0, 0), (5, 5)])
        page.setRotation(0)
        page.insertText((20, 20), "x")
        page.insertTextbox(pdfspine.Rect(0, 300, 200, 380), "wrapped text here")
        assert isinstance(page.newShape(), pdfspine.Shape)

        # camelCase annotation aliases (each forwards to add_*_annot)
        page.addTextAnnot((10, 10), "n")
        page.addFreetextAnnot(pdfspine.Rect(10, 30, 120, 60), "ft")
        page.addHighlightAnnot(pdfspine.Rect(10, 70, 60, 90))
        page.addUnderlineAnnot(pdfspine.Rect(10, 100, 60, 120))
        page.addStrikeoutAnnot(pdfspine.Rect(10, 130, 60, 150))
        page.addSquigglyAnnot(pdfspine.Rect(10, 160, 60, 180))
        page.addRectAnnot(pdfspine.Rect(10, 190, 60, 210))
        page.addCircleAnnot(pdfspine.Rect(10, 220, 60, 240))
        page.addLineAnnot((10, 250), (60, 250))
        page.addPolygonAnnot([(10, 260), (30, 280), (60, 260)])
        page.addPolylineAnnot([(10, 290), (30, 300), (60, 290)])
        page.addInkAnnot([[(10, 310), (30, 320), (60, 310)]])
        page.addStampAnnot(pdfspine.Rect(10, 330, 120, 360))
        page.addFileAnnot((10, 370), b"data", "a.bin")

        assert page.annot_names() is not None
        assert isinstance(page.get_cdrawings(), list)
        assert isinstance(page.getCdrawings(), list)
        assert isinstance(page.getDrawings(), list)
        assert isinstance(page.getImages(), list)
        assert page.getLinks() == page.get_links()
        assert isinstance(page.getDisplayList(), pdfspine.DisplayList)
        assert isinstance(page.getPixmap(), pdfspine.Pixmap)

        # delete via the camelCase alias
        first = page.first_annot
        assert first is not None
        page.deleteAnnot(first)


# =====================================================================
# DOCPY-036 — cluster_drawings
# =====================================================================
def test_docpy_036_cluster_drawings() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=400, height=400)
        # Two rectangles close together (one cluster) + one far away.
        page.draw_rect(pdfspine.Rect(20, 20, 80, 60), fill=(0, 0, 0))
        page.draw_rect(pdfspine.Rect(82, 22, 140, 60), fill=(0, 0, 0))
        page.draw_rect(pdfspine.Rect(300, 300, 360, 360), fill=(0, 0, 0))

        clusters = page.cluster_drawings()
        assert len(clusters) == 2
        # The first cluster merges the two neighboring rects.
        assert clusters[0].width > 100

        # clip= restricts the considered area; drawings= reuses a prior result.
        subset = page.cluster_drawings(
            clip=pdfspine.Rect(0, 0, 200, 200), drawings=page.get_drawings()
        )
        assert len(subset) == 1


# =====================================================================
# DOCPY-037 — remove_rotation rewrites links (bug: widgets unsupported)
# =====================================================================
@pytest.mark.parametrize("rot", [90, 180, 270])
def test_docpy_037_remove_rotation_rewrites_links(rot: int) -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=300)
        page.insert_text((20, 20), "x")
        page.insert_link({"kind": 2, "from": (10, 10, 60, 30), "uri": "https://a.b"})
        page.set_rotation(rot)
        inv = page.remove_rotation()
        assert isinstance(inv, pdfspine.Matrix)
        assert page.rotation == 0
        links = page.get_links()
        assert len(links) == 1
        assert links[0]["uri"] == "https://a.b"


def test_docpy_037_remove_rotation_zero_is_identity() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=300)
        inv = page.remove_rotation()
        assert tuple(inv) == (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


# =====================================================================
# DOCPY-038 — write_text composed (multi-writer / rotate) path
# =====================================================================
def test_docpy_038_write_text_multiple_writers() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=300)
        tw1 = pdfspine.TextWriter(page.rect)
        tw1.append((30, 100), "first")
        tw2 = pdfspine.TextWriter(page.rect)
        tw2.append((30, 140), "second")
        page.write_text(writers=[tw1, tw2])
        text = page.get_text("text")
        assert "first" in text and "second" in text


def test_docpy_038_write_text_single_writer_rotated() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=300)
        tw = pdfspine.TextWriter(page.rect)
        tw.append((30, 100), "spun")
        page.write_text(writers=tw, rotate=90)  # composed scratch-page path
        with pytest.raises(ValueError, match="TextWriter"):
            page.write_text(writers=None)


# =====================================================================
# DOCPY-039 — text_in_rect / content_blocks / filled_rectangles
# =====================================================================
def test_docpy_039_text_in_rect_reversed_rect() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=200)
        page.insert_text((50, 100), "Hello")
        # Un-normalized rect (x0>x1, y0>y1) is swapped internally.
        assert page.text_in_rect((300, 120, 0, 80)) == "Hello"
        assert page.text_in_rect((0, 0, 300, 50)) == ""
        with pytest.raises(ValueError, match="sort"):
            page.text_in_rect((0, 0, 10, 10), sort="raw")


def test_docpy_039_content_blocks_and_filled_rectangles() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=200)
        page.insert_text((50, 100), "Hello")
        blocks = page.content_blocks()
        assert [type(b).__name__ for b in blocks] == ["TextBlock"]
        assert blocks[0].text == "Hello"

        page.draw_rect(pdfspine.Rect(10, 10, 50, 40), fill=(1, 0, 0))
        page.draw_rect(pdfspine.Rect(60, 10, 100, 40), fill=(1, 1, 1))  # white
        rects = page.filled_rectangles()
        assert any(r.fill == (1.0, 0.0, 0.0) for r in rects)
        assert all(r.fill != (1.0, 1.0, 1.0) for r in rects)  # white dropped
        with_white = page.filled_rectangles(include_white=True)
        assert len(with_white) >= len(rects)


def test_docpy_039_link_annotations_typed() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=200)
        page.insert_link({"kind": 2, "from": (10, 10, 60, 30), "uri": "https://ex"})
        page.insert_link({"kind": 1, "from": (10, 40, 60, 60), "page": 0})  # goto
        typed = page.link_annotations()
        assert len(typed) == 1  # only the external URI link
        assert typed[0].uri == "https://ex"


# =====================================================================
# DOCPY-040 — get_text_blocks / get_textbox / get_text_selection clipping
# =====================================================================
def test_docpy_040_get_text_clip_variants() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=300)
        page.insert_text((50, 100), "alpha beta")
        page.insert_text((50, 140), "gamma delta")

        blocks = page.get_text_blocks(clip=pdfspine.Rect(0, 90, 300, 115))
        assert blocks  # at least the first line's block intersects
        assert all("gamma" not in b[4] for b in blocks)

        box = page.get_textbox(pdfspine.Rect(0, 90, 300, 115))
        assert "alpha" in box

        sel = page.get_text_selection((0, 90), (300, 115), clip=(0, 0, 300, 300))
        assert "alpha" in sel


# =====================================================================
# DOCPY-041 — links: get/insert/delete/update + Link value class
# =====================================================================
def test_docpy_041_link_object_and_edits() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=200)
        page.insert_link({"kind": 2, "from": (10, 10, 60, 30), "uri": "https://a"})

        link = page.first_link
        assert link is not None
        assert link.kind == 2
        assert link.uri == "https://a"
        assert link.page == -1
        assert link.is_external is True
        assert isinstance(link.rect, pdfspine.Rect)
        assert set(link.border) >= {"width", "dashes", "style"}
        assert link.colors["fill"] is None
        assert isinstance(link.flags, int)
        assert isinstance(link.xref, int)
        assert link.dest.uri == "https://a"

        # load_links returns the same first link
        assert page.load_links().uri == "https://a"

        # update_link (delete + reinsert) then delete_link
        links = page.get_links()
        links[0]["uri"] = "https://b"
        page.update_link(links[0])
        assert page.first_link.uri == "https://b"
        page.delete_link(page.get_links()[0])
        assert page.get_links() == []


# =====================================================================
# DOCPY-042 — native table detection + Table / TableFinder surface
# =====================================================================
def test_docpy_042_find_tables_surface() -> None:
    with pdfspine.open(stream=_table_pdf()) as doc:
        finder = doc[0].find_tables(strategy="lines")
        assert len(finder) == 1
        assert repr(finder) == "<pdfspine.TableFinder tables=1>"
        assert finder.tables[0].row_count == 2
        assert finder[0].col_count == 2

        table = next(iter(finder))
        assert repr(table) == "<pdfspine.Table 2x2>"
        assert table.header[0] == "A"
        assert table.rows == [50.0, 100.0, 150.0]
        assert table.cols == [50.0, 150.0, 250.0]
        assert len(table.spans) == 4
        assert table.confidence is None
        assert table.text_source == "pdfspine-native"
        assert "A" in table.to_markdown()
        assert table.toMarkdown() == table.to_markdown()
        assert table.cells and table.extract()


def test_docpy_042_find_tables_strategy_kwargs_and_errors() -> None:
    with pdfspine.open(stream=_table_pdf()) as doc:
        page = doc[0]
        # PyMuPDF's vertical_/horizontal_strategy kwargs select the strategy.
        assert len(page.find_tables(vertical_strategy="lines")) == 1
        assert isinstance(page.findTables(strategy="text"), pdfspine.TableFinder)
        with pytest.raises(TypeError, match="vision_options"):
            page.find_tables(vision_options={"a": 1})
        with pytest.raises(pdfspine.PdfUnsupportedError, match="backend"):
            page.find_tables(backend="nope")


# =====================================================================
# DOCPY-043 — insert_image argument branches
# =====================================================================
def test_docpy_043_insert_image_branches(tmp_path) -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=100, height=100)
        with pytest.raises(pdfspine.PdfUnsupportedError, match="pixmap"):
            page.insert_image(pdfspine.Rect(0, 0, 40, 40), pixmap=page.get_pixmap())
        with pytest.raises(ValueError, match="stream="):
            page.insert_image(pdfspine.Rect(0, 0, 40, 40))

        raw = tmp_path / "raw.rgb"
        raw.write_bytes(b"\xff\x00\x00" * (4 * 4))
        page.insert_image(
            pdfspine.Rect(0, 0, 40, 40), filename=str(raw), width=4, height=4
        )
        assert len(page.get_images()) == 1


# =====================================================================
# DOCPY-044 — get_pixmap / get_svg_image / get_image_bbox argument branches
# =====================================================================
def test_docpy_044_matrix_and_bbox_branches() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=100, height=100)
        with pytest.raises(ValueError, match="6-sequence"):
            page.get_pixmap(matrix=(1, 2, 3))
        with pytest.raises(ValueError, match="6-sequence"):
            page.get_svg_image(matrix=(1, 2, 3))

        raw = b"\xff\x00\x00" * (4 * 4)
        page.insert_image(pdfspine.Rect(0, 0, 40, 40), stream=raw, width=4, height=4)
        images = page.get_images(full=True)
        assert images
        # Passing the whole get_images() tuple resolves via its name/xref.
        bbox = page.get_image_bbox(images[0])
        assert isinstance(bbox, pdfspine.Rect)
        # An unknown name yields an empty rect rather than raising.
        assert page.get_image_bbox("NoSuchImage") == pdfspine.Rect(0, 0, 0, 0)


# =====================================================================
# DOCPY-045 — parentless Page: refresh no-op + first_annot None
# =====================================================================
def test_docpy_045_parentless_page_and_empty_annots() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=100, height=100)
        assert page.first_annot is None  # no annotations yet
        orphan = Page(page._page)  # parent is None
        assert orphan.parent is None
        assert orphan.refresh() is None  # early return, no crash
        assert repr(page).startswith("<pdfspine.Page number=")


# =====================================================================
# DOCPY-046 — color / quad converters + parentless Annot guards
# =====================================================================
def test_docpy_046_color_and_quad_converters() -> None:
    assert _color(0.5) == (0.5, 0.5, 0.5)  # scalar gray
    assert _color(None) is None
    assert _color((1, 0, 0)) == (1.0, 0.0, 0.0)
    with pytest.raises(ValueError, match="quad"):
        _quad((1, 2, 3))  # not a 4- or 8-sequence
    # A bare Quad and a flat numeric sequence each normalize to one quad.
    assert len(_quads(pdfspine.Quad((0, 0), (1, 0), (0, 1), (1, 1)))) == 1
    assert len(_quads((10, 10, 60, 30))) == 1
    assert len(_quads([pdfspine.Rect(0, 0, 1, 1), pdfspine.Rect(2, 2, 3, 3)])) == 2


def test_docpy_046_add_highlight_quad_and_list() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        page.insert_text((20, 100), "highlighted")
        page.add_highlight_annot(
            pdfspine.Quad((10, 90), (120, 90), (10, 110), (120, 110))
        )
        page.add_highlight_annot([pdfspine.Rect(10, 90, 60, 110)])
        assert len(list(page.annots())) == 2


def test_docpy_046_parentless_annot_text_guards() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        page.add_text_annot((20, 20), "n")
        core = page._page.annots()[0]
        orphan = Annot(core)  # no owning page
        with pytest.raises(pdfspine.PdfError):
            orphan.get_text()
        with pytest.raises(pdfspine.PdfError):
            orphan.get_textpage()


def test_docpy_046_text_in_rect_skips_image_block() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        page.insert_text((30, 100), "label")
        raw = b"\xff\x00\x00" * (4 * 4)
        page.insert_image(pdfspine.Rect(0, 120, 40, 160), stream=raw, width=4, height=4)
        # The image block is skipped; only the text span is returned.
        assert page.text_in_rect((0, 0, 200, 200)) == "label"


def test_docpy_046_get_textbox_multiline() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=300)
        page.insert_text((50, 100), "alpha beta")
        page.insert_text((50, 140), "gamma delta")
        both = page.get_textbox(pdfspine.Rect(0, 90, 300, 160))
        assert "alpha" in both and "gamma" in both
        assert "\n" in both  # the two lines are separated by a newline


def test_docpy_046_insert_image_alias() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=100, height=100)
        raw = b"\xff\x00\x00" * (4 * 4)
        page.insertImage(pdfspine.Rect(0, 0, 40, 40), stream=raw, width=4, height=4)
        assert len(page.get_images()) == 1
