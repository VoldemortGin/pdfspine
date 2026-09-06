"""Markdown export: ``Page.to_markdown`` / ``Document.to_markdown`` /
``Document.save_markdown`` (pdfspine-original extension)."""

from __future__ import annotations

from pathlib import Path

import pytest

import pdfspine

_FONTS = {
    "helv": b"/Helvetica",
    "hebo": b"/Helvetica-Bold",
    "heit": b"/Helvetica-Oblique",
    "cour": b"/Courier",
}
_FONT_KEYS = {name: f"/F{i + 1}".encode() for i, name in enumerate(_FONTS)}
_CORPUS = Path(__file__).resolve().parents[2] / "fixtures" / "corpus"


def _encode(text: str) -> bytes:
    out = bytearray()
    for ch in text:
        if ch in "()\\":
            out += b"\\" + ch.encode()
        elif ord(ch) < 128:
            out += ch.encode()
        else:
            out += b"\\%03o" % ch.encode("cp1252")[0]
    return bytes(out)


def _content(page: dict, height: float) -> bytes:
    parts: list[bytes] = []
    for x, y_top, text, size, font in page["items"]:
        parts.append(
            b"BT "
            + _FONT_KEYS[font]
            + b" %g Tf 1 0 0 1 %g %g Tm (" % (size, x, height - y_top)
            + _encode(text)
            + b") Tj ET"
        )
    for x0, y0, x1, y1 in page.get("rules", ()):
        parts.append(b"%g %g m %g %g l S" % (x0, height - y0, x1, height - y1))
    if page.get("image"):
        parts.append(b"q 100 0 0 50 72 %g cm /Im1 Do Q" % (height - 300))
    return b"\n".join(parts)


def make_pdf(pages: list[dict], *, width: float = 612, height: float = 792) -> bytes:
    """A classic-xref PDF; each page spec has ``items`` (x, y_top, text, size,
    font key), optional ``rules`` (x0, y0, x1, y1 line segments, top-down
    coordinates) and an optional 2x2 gray ``image``."""
    objects: list[tuple[int, bytes]] = [
        (2, b"<< /Type /Pages /Kids [%s] /Count %d >>"),
    ]
    font_objects = []
    next_num = 3
    for name, base in _FONTS.items():
        font_objects.append(
            (
                next_num,
                b"<< /Type /Font /Subtype /Type1 /BaseFont "
                + base
                + b" /Encoding /WinAnsiEncoding >>",
            )
        )
        next_num += 1
    font_dict = b" ".join(
        _FONT_KEYS[name] + b" %d 0 R" % num
        for name, (num, _) in zip(_FONTS, font_objects)
    )
    image_num = next_num
    next_num += 1
    samples = bytes([0, 255, 255, 0])
    objects.append(
        (
            image_num,
            b"<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace "
            b"/DeviceGray /BitsPerComponent 8 /Length 4 >>\nstream\n"
            + samples
            + b"\nendstream",
        )
    )
    objects.extend(font_objects)
    kids: list[bytes] = []
    for page in pages:
        page_num, content_num = next_num, next_num + 1
        next_num += 2
        content = _content(page, height)
        objects.append(
            (
                page_num,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %g %g] /Contents %d 0 R "
                b"/Resources << /Font << %s >> /XObject << /Im1 %d 0 R >> >> >>"
                % (width, height, content_num, font_dict, image_num),
            )
        )
        objects.append(
            (
                content_num,
                b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
            )
        )
        kids.append(b"%d 0 R" % page_num)
    objects[0] = (
        2,
        b"<< /Type /Pages /Kids [%s] /Count %d >>" % (b" ".join(kids), len(pages)),
    )
    objects.insert(0, (1, b"<< /Type /Catalog /Pages 2 0 R >>"))

    out = bytearray(b"%PDF-1.7\n")
    offsets: dict[int, int] = {}
    for num, body in objects:
        offsets[num] = len(out)
        out += b"%d 0 obj\n" % num + body + b"\nendobj\n"
    size = max(offsets) + 1
    startxref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % size
    for num in range(1, size):
        out += b"%010d 00000 n \n" % offsets[num]
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        size,
        startxref,
    )
    return bytes(out)


def _page(items, **extra) -> pdfspine.Page:
    return pdfspine.open(stream=make_pdf([{"items": items, **extra}]), filetype="pdf")[
        0
    ]


def _grid_rules(x0, y0, col_widths, row_height, rows):
    """Ruling lines of a table whose top-left corner is ``(x0, y0)``."""
    x1 = x0 + sum(col_widths)
    y1 = y0 + row_height * rows
    rules = [
        (x0, y0 + row_height * r, x1, y0 + row_height * r) for r in range(rows + 1)
    ]
    x = x0
    for width in [0, *col_widths]:
        x += width
        rules.append((x, y0, x, y1))
    return rules


BODY = [
    (
        72,
        300,
        "Body text with enough characters to be the dominant size on",
        12,
        "helv",
    ),
    (72, 315, "this page, so that headings are judged relative to it.", 12, "helv"),
]


# ---------------------------------------------------------------- headings


def test_heading_levels_follow_font_size_clusters():
    page = _page(
        [
            (72, 80, "Document Title", 24, "helv"),
            (72, 130, "Section One", 16, "helv"),
            (72, 170, "Sub Section", 14, "helv"),
            *BODY,
        ]
    )
    assert page.to_markdown() == (
        "# Document Title\n\n## Section One\n\n### Sub Section\n\n"
        "Body text with enough characters to be the dominant size on this page, "
        "so that headings are judged relative to it.\n"
    )


def test_heading_levels_cap_shares_the_last_level():
    page = _page(
        [
            (72, 80, "Document Title", 24, "helv"),
            (72, 130, "Section One", 16, "helv"),
            (72, 170, "Sub Section", 14, "helv"),
            *BODY,
        ]
    )
    md = page.to_markdown(heading_levels=2)
    assert md.startswith("# Document Title\n\n## Section One\n\n## Sub Section\n\n")


def test_heading_ratio_controls_the_threshold():
    page = _page([(72, 80, "Slightly bigger", 13, "helv"), *BODY])
    assert page.to_markdown().startswith("Slightly bigger\n\n")
    assert page.to_markdown(heading_ratio=1.05).startswith("# Slightly bigger\n\n")


def test_bold_body_size_line_becomes_next_deeper_heading():
    page = _page(
        [
            (72, 80, "Document Title", 24, "helv"),
            (72, 130, "Bold Lead", 12, "hebo"),
            *BODY,
        ]
    )
    assert page.to_markdown().startswith("# Document Title\n\n## Bold Lead\n\n")
    assert page.to_markdown(bold_headings=False).startswith(
        "# Document Title\n\n**Bold Lead**\n\n"
    )


def test_bold_sentence_is_not_a_heading():
    page = _page([(72, 130, "This bold line is a sentence.", 12, "hebo"), *BODY])
    assert page.to_markdown().startswith("**This bold line is a sentence.**\n\n")


def test_heading_needs_a_few_characters():
    page = _page([(500, 40, "7", 24, "helv"), *BODY])
    assert "#" not in page.to_markdown()


# --------------------------------------------------------------- paragraphs


def test_paragraph_lines_join_and_soft_hyphenation_is_mended():
    page = _page(
        [
            (72, 100, "The quick brown fox jumps over the con-", 12, "helv"),
            (72, 115, "tinuous line and then over the dog -", 12, "helv"),
            (72, 130, "Mark the end.", 12, "helv"),
        ]
    )
    assert page.to_markdown() == (
        "The quick brown fox jumps over the continuous line and then over the dog - "
        "Mark the end.\n"
    )


def test_blocks_become_separate_paragraphs():
    page = _page(
        [
            (72, 100, "First paragraph.", 12, "helv"),
            (72, 200, "Second paragraph.", 12, "helv"),
        ]
    )
    assert page.to_markdown() == "First paragraph.\n\nSecond paragraph.\n"


def test_block_markers_at_paragraph_start_are_escaped():
    page = _page([(72, 100, "# not a heading", 12, "helv"), *BODY])
    assert page.to_markdown().startswith("\\# not a heading\n\n")


def test_empty_page_yields_empty_string():
    assert _page([]).to_markdown() == ""


# -------------------------------------------------------------------- lists


def test_bullet_number_and_label_markers():
    page = _page(
        [
            (72, 100, "• apples", 12, "helv"),
            (72, 115, "• second item that", 12, "helv"),
            (84, 130, "wraps onto a line", 12, "helv"),
            (90, 145, "• nested one", 12, "helv"),
            (72, 160, "- dash item", 12, "helv"),
            (72, 200, "1. first step", 12, "helv"),
            (72, 215, "2) second step", 12, "helv"),
            (72, 255, "(a) lettered", 12, "helv"),
            (72, 270, "ii. roman", 12, "helv"),
        ]
    )
    assert page.to_markdown() == (
        "- apples\n"
        "- second item that wraps onto a line\n"
        "    - nested one\n"
        "- dash item\n"
        "\n"
        "1. first step\n"
        "2. second step\n"
        "\n"
        "- (a) lettered\n"
        "- ii. roman\n"
    )


def test_items_split_across_blocks_form_one_list():
    page = _page(
        [
            (72, 100, "• first", 12, "helv"),
            (72, 140, "• second", 12, "helv"),
            (72, 180, "Trailing paragraph.", 12, "helv"),
        ]
    )
    assert page.to_markdown() == "- first\n- second\n\nTrailing paragraph.\n"


def test_bullet_only_line_is_glued_to_the_next_line():
    page = _page(
        [
            (72, 100, "•", 12, "helv"),
            (72, 115, "the item text", 12, "helv"),
        ]
    )
    assert page.to_markdown() == "- the item text\n"


# ------------------------------------------------------------------- tables


def _table_page(*, tables_before_after=True, **extra):
    items = [
        (76, 214, "Name", 12, "hebo"),
        (176, 214, "Qty", 12, "hebo"),
        (76, 244, "Apple", 12, "helv"),
        (176, 244, "3", 12, "helv"),
        (76, 274, "Pear", 12, "helv"),
        (176, 274, "5", 12, "helv"),
    ]
    if tables_before_after:
        items = [(72, 150, "Before the table.", 12, "helv"), *items]
        items.append((72, 340, "After the table.", 12, "helv"))
    return _page(items, rules=_grid_rules(72, 200, [100, 100], 30, 3), **extra)


def test_ruled_table_is_rendered_as_gfm_in_reading_order():
    md = _table_page().to_markdown()
    assert md == (
        "Before the table.\n\n"
        "| Name | Qty |\n| --- | --- |\n| Apple | 3 |\n| Pear | 5 |\n\n"
        "After the table.\n"
    )


def test_tables_can_be_disabled():
    md = _table_page().to_markdown(tables=False)
    assert "|" not in md
    assert "Apple" in md and "Name" in md


def test_table_without_surrounding_text_is_emitted():
    md = _table_page(tables_before_after=False).to_markdown()
    assert md.startswith("| Name | Qty |\n")


# ---------------------------------------------------------- inline styles


def _styled_page(emphasis=True):
    doc = pdfspine.open()
    page = doc.new_page(width=500, height=300)
    x = 40.0
    for text, font in [
        ("Mixed ", "helv"),
        ("bold", "hebo"),
        (" and ", "helv"),
        ("italic", "heit"),
        (" and ", "helv"),
        ("code", "cour"),
        (" end.", "helv"),
    ]:
        page.insert_text((x, 60), text, fontname=font, fontsize=12)
        x += pdfspine.get_text_length(text, fontname=font, fontsize=12)
    page.insert_text((40, 120), "def main():", fontname="cour", fontsize=12)
    page.insert_text((40 + 4 * 7.2, 135), "return 1", fontname="cour", fontsize=12)
    return page


def test_inline_emphasis_and_fenced_code_block():
    md = _styled_page().to_markdown()
    assert md == (
        "Mixed **bold** and _italic_ and `code` end.\n\n"
        "```\ndef main():\n    return 1\n```\n"
    )


def test_emphasis_can_be_disabled():
    md = _styled_page().to_markdown(emphasis=False)
    assert md.startswith("Mixed bold and italic and code end.\n\n")


# ------------------------------------------------------------------- images


def test_image_placeholders_are_opt_in():
    page = _page([(72, 100, "Caption text here.", 12, "helv")], image=True)
    assert page.to_markdown() == "Caption text here.\n"
    md = page.to_markdown(images=True)
    assert "Caption text here." in md
    assert "![image](page-1-image-1." in md


# ------------------------------------------------------------------- clip


def test_clip_cuts_running_headers():
    page = _page([(72, 30, "Running header", 9, "helv"), *BODY])
    assert "Running header" in page.to_markdown()
    md = page.to_markdown(clip=pdfspine.Rect(0, 60, 612, 792))
    assert "Running header" not in md
    assert md.startswith("Body text")


# ----------------------------------------------------------------- options


@pytest.mark.parametrize(
    "kwargs", [{"heading_levels": 0}, {"heading_levels": 7}, {"heading_ratio": 1.0}]
)
def test_invalid_options_raise(kwargs):
    with pytest.raises(ValueError):
        _page(BODY).to_markdown(**kwargs)


# --------------------------------------------------------------- document


def _multi_page_doc() -> pdfspine.Document:
    pages = [
        {
            "items": [
                (72, 80, "Document Title", 24, "helv"),
                (72, 130, "Section One", 16, "helv"),
                *BODY,
            ]
        },
        {"items": []},
        {
            "items": [
                (72, 80, "Section Two", 16, "helv"),
                (72, 130, "Page two body text, short.", 12, "helv"),
            ]
        },
    ]
    return pdfspine.open(stream=make_pdf(pages), filetype="pdf")


def test_document_markdown_shares_one_heading_scale():
    doc = _multi_page_doc()
    assert doc[2].to_markdown().startswith("# Section Two\n")
    md = doc.to_markdown()
    assert md == (
        "# Document Title\n\n## Section One\n\n"
        "Body text with enough characters to be the dominant size on this page, "
        "so that headings are judged relative to it."
        "\n\n-----\n\n"
        "## Section Two\n\nPage two body text, short.\n"
    )


def test_document_markdown_page_selection_and_separator():
    doc = _multi_page_doc()
    assert doc.to_markdown(pages=[2]) == "# Section Two\n\nPage two body text, short.\n"
    md = doc.to_markdown(pages=[2, 0], page_separator="\n\n<!-- page -->\n\n")
    assert md.startswith(
        "## Section Two\n\nPage two body text, short.\n\n<!-- page -->\n\n"
    )
    assert md.count("<!-- page -->") == 1
    assert doc.to_markdown(pages=[1]) == ""


def test_save_markdown_writes_utf8(tmp_path):
    page_spec = {"items": [(72, 100, "• Café item", 12, "helv"), *BODY]}
    doc = pdfspine.open(stream=make_pdf([page_spec]), filetype="pdf")
    target = tmp_path / "out.md"
    assert doc.save_markdown(target) is None
    assert target.read_text(encoding="utf-8") == doc.to_markdown()
    assert "- Café item" in target.read_bytes().decode("utf-8")


def test_document_markdown_forwards_options():
    doc = _multi_page_doc()
    md = doc.to_markdown(heading_levels=1)
    assert "# Document Title" in md and "# Section One" in md and "##" not in md


# ------------------------------------------------------------------ corpus


@pytest.mark.skipif(not _CORPUS.is_dir(), reason="fixtures/corpus not fetched")
def test_corpus_smoke():
    pdfs = sorted(_CORPUS.glob("*.pdf"))[:3]
    if not pdfs:
        pytest.skip("no corpus PDFs")
    for path in pdfs:
        doc = pdfspine.open(path)
        md = doc.to_markdown(pages=range(min(3, len(doc))))
        assert md.strip()
        headings = [ln for ln in md.splitlines() if ln.startswith("#")]
        assert len(headings) < 60


# ------------------------------------------------------- review-driven rules


def test_wrapped_heading_lines_across_blocks_merge():
    page = _page(
        [
            (72, 80, "Basic Mechanics of Laminated", 24, "helv"),
            (72, 120, "Composite Plates", 24, "helv"),
            (72, 190, "Chapter One:", 16, "helv"),
            (72, 230, "Second Heading", 16, "helv"),
            *BODY,
        ]
    )
    md = page.to_markdown()
    assert md.startswith(
        "# Basic Mechanics of Laminated Composite Plates\n\n"
        "## Chapter One:\n\n## Second Heading\n\n"
    )


def test_distinct_heading_sizes_never_merge_even_when_levels_collapse():
    page = _page(
        [
            (72, 80, "Document Title", 24, "helv"),
            (72, 130, "Section One", 16, "helv"),
            *BODY,
        ]
    )
    assert page.to_markdown(heading_levels=1).startswith(
        "# Document Title\n\n# Section One\n\n"
    )


def test_dotted_leader_lines_are_not_headings():
    page = _page(
        [
            (72, 80, "TABLE OF CONTENTS", 16, "helv"),
            (72, 120, "I. INTRODUCTION ........................ 1", 16, "helv"),
            (72, 140, "II. METHODS . . . . . . . . . . . . . . 12", 16, "helv"),
            *BODY,
        ]
    )
    md = page.to_markdown()
    assert md.startswith("# TABLE OF CONTENTS\n\n")
    assert md.count("#") == 1
    assert "I. INTRODUCTION ........................ 1" in md


def test_numeric_parenthesis_marker_and_letter_dot_label():
    page = _page(
        [
            (72, 100, "(1) first assumption", 12, "helv"),
            (72, 115, "(2) second assumption", 12, "helv"),
            (72, 160, "A. Intent and Scope", 12, "helv"),
            (72, 200, "a. lowercase label", 12, "helv"),
        ]
    )
    assert page.to_markdown() == (
        "1. first assumption\n2. second assumption\n\n"
        "A. Intent and Scope\n\n"
        "- a. lowercase label\n"
    )


def test_bold_lines_of_a_caption_share_one_marker_pair():
    page = _page(
        [
            (72, 100, "Oblique aerial view of the remains of the", 12, "hebo"),
            (72, 115, "town of Armero, Colombia, devastated by", 12, "hebo"),
            (72, 130, "a lahar. Photograph by R.J. Janda.", 12, "hebo"),
            *BODY,
        ]
    )
    assert page.to_markdown().startswith(
        "**Oblique aerial view of the remains of the town of Armero, Colombia, "
        "devastated by a lahar. Photograph by R.J. Janda.**\n\n"
    )


def test_compound_hyphen_survives_a_line_break():
    page = _page(
        [
            (72, 100, "A flow of 180-million-", 12, "helv"),
            (72, 115, "cubic-yard volume.", 12, "helv"),
        ]
    )
    assert page.to_markdown() == "A flow of 180-million-cubic-yard volume.\n"


def test_single_characters_are_not_emphasized():
    doc = pdfspine.open()
    page = doc.new_page(width=400, height=200)
    page.insert_text((40, 60), "See ", fontsize=12)
    page.insert_text(
        (40 + pdfspine.get_text_length("See ", fontsize=12), 60),
        "B",
        fontname="hebo",
        fontsize=12,
    )
    page.insert_text(
        (40 + pdfspine.get_text_length("See B", fontsize=12), 60),
        " above.",
        fontsize=12,
    )
    assert page.to_markdown() == "See B above.\n"


def test_phantom_full_page_grid_is_not_a_table():
    items = [(72, 150, "Body text that must survive.", 12, "helv"), *BODY]
    rules = _grid_rules(10, 10, [296, 296], 386, 2)
    page = _page(items, rules=rules)
    assert len(page.find_tables()) == 1
    md = page.to_markdown()
    assert "|" not in md
    assert md.startswith("Body text that must survive.\n\n")


def test_grid_with_a_prose_cell_is_not_a_table():
    prose = "Long running prose inside a ruled figure frame. " * 12
    items = [(76, 214, prose[:90], 8, "helv"), (76, 226, prose[90:180], 8, "helv")]
    for i in range(6):
        items.append((76, 238 + 12 * i, prose[180 + 90 * i : 270 + 90 * i], 8, "helv"))
    items.append((376, 214, "label", 8, "helv"))
    items.append((72, 400, "Paragraph after the frame.", 12, "helv"))
    rules = _grid_rules(72, 200, [300, 100], 130, 1)
    page = _page(items, rules=rules)
    assert len(page.find_tables()) == 1
    assert "|" not in page.to_markdown()


def test_table_is_plausible_rules():
    from pdfspine._markdown import table_is_plausible

    assert table_is_plausible((0, 0, 100, 50), [["a", "b"], ["c", None]], 10000.0)
    assert not table_is_plausible((0, 0, 100, 50), [[None, ""], ["", None]], 10000.0)
    assert not table_is_plausible((0, 0, 100, 50), [["only one"]], 10000.0)
    assert not table_is_plausible((0, 0, 95, 100), [["a", "b"]], 10000.0)
    assert not table_is_plausible((0, 0, 100, 50), [["a", "x" * 501]], 10000.0)


def test_paragraph_like_size_groups_stay_body_text():
    page = _page(
        [
            (72, 80, "Lead paragraph set in a larger face. It runs on for", 14, "helv"),
            (
                72,
                98,
                "several lines and ends like a sentence, so it is not",
                14,
                "helv",
            ),
            (72, 116, "a heading at all.", 14, "helv"),
            (500, 150, "I", 16, "helv"),
            (72, 190, "Real Heading", 16, "helv"),
            *BODY,
            (
                72,
                330,
                "More body text keeps the twelve point face dominant.",
                12,
                "helv",
            ),
        ]
    )
    md = page.to_markdown()
    assert md.startswith("Lead paragraph set in a larger face.")
    assert "# Real Heading\n" in md
    assert md.count("#") == 1
    assert "\nI\n" in md
