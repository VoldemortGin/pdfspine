"""P2-4 medium parity items — validated against PyMuPDF 1.24.14 semantics.

Covers ``Document.set_toc_item`` / ``del_toc_item`` / ``version_count`` /
``extract_font`` / ``subset`` and ``Page.add_caret_annot`` / ``add_widget``.
Corpus-dependent cases (font extraction, version count on real updated /
linearized PDFs) skip cleanly when the regenerable corpus is absent.
"""

from __future__ import annotations

from pathlib import Path

import pdfspine
import pytest

_CORPUS = Path(__file__).resolve().parents[2] / "fixtures" / "corpus"


def _five_page_doc_with_toc() -> pdfspine.Document:
    doc = pdfspine.open()
    for _ in range(5):
        doc.new_page()
    doc.set_toc([[1, "A", 1], [2, "A.1", 2], [2, "A.2", 3], [1, "B", 4]])
    return doc


# --- version_count --------------------------------------------------------


def test_version_count_fresh_doc_is_one() -> None:
    doc = pdfspine.open()
    doc.new_page()
    assert doc.version_count == 1


@pytest.mark.skipif(not _CORPUS.exists(), reason="regenerable corpus absent")
@pytest.mark.parametrize(
    "name, want",
    [
        ("irs-fw4.pdf", 2),  # linearized + one incremental update
        ("cdc-mmwr-7301a1.pdf", 1),  # linearized only
        ("govinfo-hr1.pdf", 2),  # non-linearized, two sections
    ],
)
def test_version_count_matches_fitz(name: str, want: int) -> None:
    path = _CORPUS / name
    if not path.exists():
        pytest.skip(f"{name} not in corpus")
    assert pdfspine.open(str(path)).version_count == want


# --- extract_font ---------------------------------------------------------


@pytest.mark.skipif(not _CORPUS.exists(), reason="regenerable corpus absent")
def test_extract_font_embedded_program_and_metadata() -> None:
    path = _CORPUS / "govinfo-hr1.pdf"
    if not path.exists():
        pytest.skip("govinfo-hr1.pdf not in corpus")
    doc = pdfspine.open(str(path))
    # xref 4 is the DeVinne Type1 (CFF) font in this fixture.
    basefont, ext, ftype, buffer = doc.extract_font(4)
    assert basefont == "DeVinne"
    assert ext == "cff"
    assert ftype == "Type1"
    assert buffer[:4] == b"\x01\x00\x04\x02"  # CFF program magic
    assert len(buffer) > 1000


@pytest.mark.skipif(not _CORPUS.exists(), reason="regenerable corpus absent")
def test_extract_font_info_only_drops_buffer() -> None:
    path = _CORPUS / "govinfo-hr1.pdf"
    if not path.exists():
        pytest.skip("govinfo-hr1.pdf not in corpus")
    doc = pdfspine.open(str(path))
    basefont, ext, ftype, buffer = doc.extract_font(4, info_only=1)
    assert (basefont, ext, ftype) == ("DeVinne", "cff", "Type1")
    assert buffer == b""


@pytest.mark.skipif(not _CORPUS.exists(), reason="regenerable corpus absent")
def test_extract_font_named_returns_dict() -> None:
    path = _CORPUS / "govinfo-hr1.pdf"
    if not path.exists():
        pytest.skip("govinfo-hr1.pdf not in corpus")
    doc = pdfspine.open(str(path))
    info = doc.extract_font(4, named=1)
    assert info["name"] == "DeVinne"
    assert info["ext"] == "cff"
    assert info["type"] == "Type1"
    assert info["content"][:4] == b"\x01\x00\x04\x02"


def test_extract_font_non_font_xref_is_empty() -> None:
    doc = pdfspine.open()
    doc.new_page()
    # xref 1 is the catalog (not a font): fitz returns ("", "", "", b"").
    assert doc.extract_font(1) == ("", "", "", b"")


# --- subset ---------------------------------------------------------------


def test_subset_returns_none_and_never_corrupts() -> None:
    doc = pdfspine.open()
    doc.new_page()
    before = doc.tobytes()
    assert doc.subset() is None
    # The document is still openable after subsetting (no corruption).
    assert pdfspine.open(stream=doc.tobytes()).page_count == 1
    assert before  # sanity


# --- set_toc_item / del_toc_item ------------------------------------------


def test_set_toc_item_title_only_updates_title() -> None:
    doc = _five_page_doc_with_toc()
    doc.set_toc_item(1, title="RENAMED")
    toc = doc.get_toc()
    assert toc[1][1] == "RENAMED"
    # The other titles are untouched.
    assert [row[1] for row in toc] == ["A", "RENAMED", "A.2", "B"]


def test_set_toc_item_kind_none_deletes_item() -> None:
    doc = _five_page_doc_with_toc()
    doc.set_toc_item(2, kind=0)  # LINK_NONE → del_toc_item
    # The neutralized item keeps its title but loses its destination (page -1).
    toc = doc.get_toc()
    assert toc[2][1] == "A.2"
    assert toc[2][2] == -1


def test_set_toc_item_goto_repoints_destination() -> None:
    doc = _five_page_doc_with_toc()
    doc.set_toc_item(1, kind=1, pno=4, title="MOVED")
    item_xref = doc.get_outline_xrefs()[1]
    obj = doc.xref_object(item_xref)
    # The title changed and a /GoTo /A action now targets page 4's object
    # (page_xref is the authoritative target); the prior /Dest is removed.
    assert "/Title (MOVED)" in obj
    assert "/GoTo" in obj
    assert "/Dest" not in obj
    assert f"{doc.page_xref(3)} 0 R" in obj  # pno=4 → page index 3


def test_del_toc_item_neutralizes_in_place() -> None:
    doc = _five_page_doc_with_toc()
    doc.del_toc_item(0)
    toc = doc.get_toc()
    # Item 0 keeps its title but its destination is removed (page -1).
    assert toc[0][1] == "A"
    assert toc[0][2] == -1
    # The tree length is preserved (fitz does not drop the item).
    assert len(toc) == 4


# --- Page.add_caret_annot -------------------------------------------------


def test_add_caret_annot_type_and_rect() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    annot = page.add_caret_annot((100, 100))
    assert annot.type == (14, "Caret")
    obj = doc.xref_object(annot.xref)
    assert "/Subtype /Caret" in obj
    assert "/AP" in obj  # an appearance stream was generated
    # The stored /Rect matches fitz: [point.x-1, ..., point.x+19, ...].
    assert "/Rect [99" in obj and "119" in obj


# --- Page.add_widget ------------------------------------------------------


def test_add_widget_text_field_registers_acroform() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    widget = pdfspine.Widget()
    widget.field_name = "myfield"
    widget.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
    widget.rect = pdfspine.Rect(50, 50, 200, 80)
    widget.field_value = "hello"
    annot = page.add_widget(widget)
    assert annot.type == (21, "Widget")
    obj = doc.xref_object(annot.xref)
    assert "/FT /Tx" in obj
    assert "/T (myfield)" in obj
    assert "/V (hello)" in obj
    # The document is now a form (the field is in /AcroForm /Fields).
    assert doc.is_form_pdf is True
    assert [w.field_name for w in page.widgets()] == ["myfield"]


def test_add_widget_combobox_carries_options() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    widget = pdfspine.Widget()
    widget.field_name = "combo"
    widget.field_type = pdfspine.PDF_WIDGET_TYPE_COMBOBOX
    widget.rect = pdfspine.Rect(50, 100, 200, 130)
    widget.choice_values = ["a", "b", "c"]
    widget.field_value = "a"
    page.add_widget(widget)
    reopened = pdfspine.open(stream=doc.tobytes())
    widgets = {w.field_name: w for w in reopened[0].widgets()}
    assert "combo" in widgets
    assert widgets["combo"].field_type_string == "ComboBox"
    assert widgets["combo"].choice_values == ["a", "b", "c"]
    assert widgets["combo"].field_value == "a"
