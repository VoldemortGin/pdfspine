"""CONTENT-BLOCKS-* / LINK-ANNOT-* / TEXT-IN-RECT-* / FILLED-RECT-* — the typed
page-content API (pdfspine-original extension, not part of the fitz surface).

``Page.content_blocks`` converts ``get_text("dict")`` blocks into frozen
``TextBlock``/``ImageBlock`` value objects; ``Page.link_annotations`` types the
external-URI subset of ``get_links``; ``Page.text_in_rect`` rebuilds visually
ordered text from spans whose bbox center falls inside a rectangle;
``Page.filled_rectangles`` types the rectangular fill paths of ``get_drawings``.

All fixtures are self-generated in-test (raw PDF bytes via ``stream=``) — no
external/PyMuPDF files (PRD §10). The text font carries an explicit ``/Widths``
array so span bboxes (and their centers) are exact.
"""

from __future__ import annotations

import dataclasses
import zlib

import pytest

import pdfspine
from pdfspine import FilledRectangle, ImageBlock, LinkAnnotation, Rect, TextBlock


# --- self-generated PDF assembler (classic xref) ---------------------------
# Copied from test_text.py so this file is fully self-contained.


def _build_pdf(objects: list[tuple[int, bytes]], root: int) -> bytes:
    """Assembles a classic-xref PDF from ``(num, body)`` object pairs."""
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
    out += f"<< /Size {size} /Root {root} 0 R >>\n".encode()
    out += b"startxref\n"
    out += f"{startxref}\n".encode()
    out += b"%%EOF\n"
    return bytes(out)


def _helvetica_font(first: int = 32, last: int = 125, width: int = 500) -> bytes:
    """A Type1 Helvetica/WinAnsi font with an explicit equal-width /Widths."""
    n = last - first + 1
    widths = b"[" + b" ".join(str(width).encode() for _ in range(n)) + b"]"
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding "
        + f"/FirstChar {first} /LastChar {last} ".encode()
        + b"/Widths "
        + widths
        + b" >>"
    )


# --- small fixture builders -------------------------------------------------


def _text_page(content: bytes, annots: list[bytes] | None = None) -> pdfspine.Page:
    """A 1-page PDF (MediaBox [0 0 612 792]) with /F1 text and optional annots."""
    annot_nums = list(range(6, 6 + len(annots or [])))
    annots_entry = (
        b" /Annots [" + b" ".join(f"{n} 0 R".encode() for n in annot_nums) + b"]"
        if annot_nums
        else b""
    )
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R"
            + annots_entry
            + b" >>",
        ),
        (
            4,
            b"<< /Length "
            + str(len(content)).encode()
            + b" >>\nstream\n"
            + content
            + b"\nendstream",
        ),
        (5, _helvetica_font()),
    ]
    for num, body in zip(annot_nums, annots or []):
        objects.append((num, body))
    return pdfspine.open(stream=_build_pdf(objects, root=1))[0]


def _mixed_page() -> pdfspine.Page:
    """A 1-page PDF with one text line and one Flate DeviceRGB image XObject."""
    samples = bytes(bytearray((i * 3) % 256 for i in range(8 * 6 * 3)))
    img = zlib.compress(samples)
    img_obj = (
        b"<< /Type /XObject /Subtype /Image /Width 8 /Height 6 "
        b"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode "
        + f"/Length {len(img)} ".encode()
        + b">>\nstream\n"
        + img
        + b"\nendstream"
    )
    content = b"BT /F1 12 Tf 72 700 Td (Hello) Tj ET q 100 0 0 80 200 100 cm /Im0 Do Q"
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> "
            b"/Contents 4 0 R >>",
        ),
        (
            4,
            b"<< /Length "
            + str(len(content)).encode()
            + b" >>\nstream\n"
            + content
            + b"\nendstream",
        ),
        (5, _helvetica_font()),
        (6, img_obj),
    ]
    return pdfspine.open(stream=_build_pdf(objects, root=1))[0]


def _uri_annot(uri: bytes, rect: bytes = b"[72 690 150 710]") -> bytes:
    return (
        b"<< /Type /Annot /Subtype /Link /Rect "
        + rect
        + b" /A << /S /URI /URI ("
        + uri
        + b") >> >>"
    )


_GOTO_ANNOT = (
    b"<< /Type /Annot /Subtype /Link /Rect [72 660 150 680] "
    b"/A << /S /GoTo /D [3 0 R /Fit] >> >>"
)

_HELLO = b"BT /F1 12 Tf 72 700 Td (Hello) Tj ET"


# --- CONTENT-BLOCKS-* : Page.content_blocks --------------------------------


def test_contentblocks_001_mixed_page_types_and_order():
    """CONTENT-BLOCKS-001: text+image page → (TextBlock, ImageBlock) matching
    the ``get_text("dict", sort=True)`` block sequence."""
    page = _mixed_page()
    blocks = page.content_blocks()
    assert isinstance(blocks, tuple)
    raw = page.get_text("dict", sort=True)["blocks"]
    assert len(blocks) == len(raw) == 2
    for typed, rawb in zip(blocks, raw):
        expected = TextBlock if rawb["type"] == 0 else ImageBlock
        assert type(typed) is expected
        assert typed.number == rawb["number"]
        assert typed.bbox == Rect(rawb["bbox"])
        assert isinstance(typed.bbox, Rect)


def test_contentblocks_002_text_block_fields():
    """CONTENT-BLOCKS-002: TextBlock carries the block text — span texts
    concatenated per line, lines joined with newlines."""
    page = _text_page(
        b"BT /F1 12 Tf 72 700 Td (Hello world) Tj 0 -20 Td (Second line) Tj ET"
    )
    blocks = page.content_blocks()
    assert len(blocks) == 1
    (block,) = blocks
    assert isinstance(block, TextBlock)
    assert block.text == "Hello world\nSecond line"
    assert block.number == 0


def test_contentblocks_003_image_block_keeps_raw_bytes_and_ext():
    """CONTENT-BLOCKS-003: ImageBlock keeps the original encoded bytes and
    extension, byte-for-byte equal to ``Document.extract_image``."""
    page = _mixed_page()
    images = [b for b in page.content_blocks() if isinstance(b, ImageBlock)]
    assert len(images) == 1
    (image,) = images
    xref = page.get_images()[0][0]
    extracted = page.parent.extract_image(xref)
    assert image.image == bytes(extracted["image"])
    assert image.ext == extracted["ext"] == "png"
    assert image.width == 8 and image.height == 6


def test_contentblocks_004_sort_false_matches_unsorted_dict():
    """CONTENT-BLOCKS-004: ``sort=False`` mirrors ``get_text("dict",
    sort=False)`` block order."""
    page = _mixed_page()
    raw = page.get_text("dict", sort=False)["blocks"]
    blocks = page.content_blocks(sort=False)
    assert [b.number for b in blocks] == [b["number"] for b in raw]
    assert [type(b) is ImageBlock for b in blocks] == [b["type"] == 1 for b in raw]


def test_contentblocks_005_blocks_are_frozen():
    """CONTENT-BLOCKS-005: the returned value objects are frozen dataclasses."""
    page = _mixed_page()
    for block in page.content_blocks():
        with pytest.raises(dataclasses.FrozenInstanceError):
            block.number = 99  # type: ignore[misc]


# --- LINK-ANNOT-* : Page.link_annotations -----------------------------------


def test_linkannot_001_uri_link_typed():
    """LINK-ANNOT-001: an external URI link → LinkAnnotation with the dict's
    ``uri`` and ``from`` rect."""
    page = _text_page(_HELLO, annots=[_uri_annot(b"https://example.com")])
    links = page.link_annotations()
    assert links == (
        LinkAnnotation(uri="https://example.com", rect=Rect(72, 690, 150, 710)),
    )
    assert isinstance(links[0].rect, Rect)


def test_linkannot_002_goto_link_skipped():
    """LINK-ANNOT-002: internal GoTo links are not returned (external URIs
    only)."""
    page = _text_page(_HELLO, annots=[_GOTO_ANNOT, _uri_annot(b"https://example.org")])
    links = page.link_annotations()
    assert [link.uri for link in links] == ["https://example.org"]


def test_linkannot_003_empty_uri_skipped():
    """LINK-ANNOT-003: a URI link with an empty URI is skipped (tolerant-parse
    convention — no error)."""
    page = _text_page(
        _HELLO,
        annots=[_uri_annot(b""), _uri_annot(b"https://ok.example")],
    )
    assert [link.uri for link in page.link_annotations()] == ["https://ok.example"]


def test_linkannot_004_no_links_empty_tuple():
    """LINK-ANNOT-004: a page without link annotations → empty tuple."""
    assert _text_page(_HELLO).link_annotations() == ()


def test_linkannot_005_pymupdf_links_iterator_unchanged():
    """LINK-ANNOT-005: the PyMuPDF-compatible ``Page.links()`` iterator keeps
    its fitz semantics alongside the typed API."""
    page = _text_page(_HELLO, annots=[_uri_annot(b"https://example.com")])
    fitz_links = list(page.links())
    assert len(fitz_links) == 1
    assert isinstance(fitz_links[0], pdfspine.Link)
    assert fitz_links[0].uri == "https://example.com"


# --- TEXT-IN-RECT-* : Page.text_in_rect --------------------------------------

# Two visual lines built from spans the extractor keeps separate (font-size
# changes split spans): line 1 = "BB"(12pt, x 72-84) + "CC"(10pt, x 84-94,
# no gap) + "EE"(10pt, x 200-210, clear gap); line 2 = "DD" at y 660.
_SPANS_CONTENT = (
    b"BT /F1 12 Tf 72 700 Td (BB) Tj ET "
    b"BT /F1 10 Tf 84 700 Td (CC) Tj ET "
    b"BT /F1 10 Tf 200 700 Td (EE) Tj ET "
    b"BT /F1 12 Tf 72 660 Td (DD) Tj ET"
)


def test_textinrect_001_visual_line_rebuild():
    """TEXT-IN-RECT-001: spans on one visual line are ordered by x0; a clear
    horizontal gap inserts a single space, a touching span does not."""
    page = _text_page(_SPANS_CONTENT)
    assert page.text_in_rect(Rect(0, 70, 612, 100)) == "BBCC EE"


def test_textinrect_002_center_point_selection():
    """TEXT-IN-RECT-002: a span is selected iff its bbox center lies in the
    rect — an intersecting span whose center is outside is excluded."""
    page = _text_page(_SPANS_CONTENT)
    # "BB" spans x 72-84 (center 78): rect ending at 80 intersects it but
    # keeps the center; "CC" (center 89) and "EE" (center 205) fall out.
    assert page.text_in_rect(Rect(0, 70, 80, 100)) == "BB"
    # Rect ending left of the center excludes "BB" too.
    assert page.text_in_rect(Rect(0, 70, 77, 100)) == ""


def test_textinrect_003_multiline_order_and_join():
    """TEXT-IN-RECT-003: multiple visual lines are ordered by (y0, x0) and
    joined with newlines."""
    page = _text_page(_SPANS_CONTENT)
    assert page.text_in_rect(Rect(0, 0, 612, 792)) == "BBCC EE\nDD"
    assert page.text_in_rect((0, 0, 612, 792)) == "BBCC EE\nDD"  # rect_like ok


def test_textinrect_004_whitespace_compressed():
    """TEXT-IN-RECT-004: runs of whitespace inside the selected text collapse
    to single spaces and edges are stripped."""
    page = _text_page(b"BT /F1 12 Tf 72 700 Td ( A  B ) Tj ET")
    assert page.text_in_rect(Rect(0, 0, 612, 792)) == "A B"


def test_textinrect_005_empty_selection():
    """TEXT-IN-RECT-005: a rect selecting nothing → empty string."""
    page = _text_page(_SPANS_CONTENT)
    assert page.text_in_rect(Rect(400, 400, 500, 500)) == ""


def test_textinrect_006_unknown_sort_mode_raises():
    """TEXT-IN-RECT-006: only ``sort="visual"`` is supported; other values
    raise ValueError."""
    page = _text_page(_SPANS_CONTENT)
    with pytest.raises(ValueError):
        page.text_in_rect(Rect(0, 0, 612, 792), sort="reading")


# --- FILLED-RECT-* : Page.filled_rectangles ----------------------------------

# One gray fill, one white fill, one blue fill+stroke, one stroke-only, and one
# non-rectangular (triangle) fill.
_DRAW_CONTENT = (
    b"0.5 0.5 0.5 rg 10 10 100 50 re f "
    b"1 1 1 rg 200 10 50 50 re f "
    b"1 0 0 RG 0 0 1 rg 300 10 60 40 re B "
    b"0 1 0 RG 400 10 30 30 re S "
    b"0 0 0 rg 500 10 m 550 10 l 525 50 l h f"
)


def test_filledrect_001_fill_types_only():
    """FILLED-RECT-001: fill ("f") and fill+stroke ("fs") rectangles are
    returned with rect + fill color; stroke-only and non-rectangular fills
    are not."""
    page = _text_page(_DRAW_CONTENT)
    rects = page.filled_rectangles()
    assert isinstance(rects, tuple)
    assert rects == (
        FilledRectangle(
            rect=Rect(10, 732, 110, 782),
            fill=(0.5019607843137255, 0.5019607843137255, 0.5019607843137255),
        ),
        FilledRectangle(rect=Rect(300, 742, 360, 782), fill=(0.0, 0.0, 1.0)),
    )


def test_filledrect_002_white_filtered_by_default():
    """FILLED-RECT-002: white fills are dropped by default and kept with
    ``include_white=True``."""
    page = _text_page(_DRAW_CONTENT)
    default_rects = {r.rect for r in page.filled_rectangles()}
    assert Rect(200, 732, 250, 782) not in default_rects
    with_white = page.filled_rectangles(include_white=True)
    assert FilledRectangle(rect=Rect(200, 732, 250, 782), fill=(1.0, 1.0, 1.0)) in (
        with_white
    )
    assert len(with_white) == len(default_rects) + 1


def test_filledrect_003_no_drawings_empty_tuple():
    """FILLED-RECT-003: a page without fill paths → empty tuple."""
    assert _text_page(_HELLO).filled_rectangles() == ()


def test_filledrect_004_results_are_frozen():
    """FILLED-RECT-004: the returned value objects are frozen dataclasses."""
    page = _text_page(_DRAW_CONTENT)
    rect = page.filled_rectangles()[0]
    with pytest.raises(dataclasses.FrozenInstanceError):
        rect.fill = (0.0, 0.0, 0.0)  # type: ignore[misc]
