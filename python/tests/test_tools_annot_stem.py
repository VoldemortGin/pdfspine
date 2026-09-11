"""Real annotation IDs for Tools.set_annot_stem, with explicit Unicode policy."""

from __future__ import annotations

import pdfspine
import pytest


@pytest.fixture(autouse=True)
def restore_stem():
    previous = pdfspine.TOOLS.set_annot_stem()
    pdfspine.TOOLS.set_annot_stem("fitz")
    try:
        yield
    finally:
        pdfspine.TOOLS.set_annot_stem(previous)


def _new_document(pages: int = 1):
    doc = pdfspine.open()
    for _ in range(pages):
        doc.new_page(width=200, height=200)
    return doc


def test_stem_default_query_and_explicit_reset() -> None:
    assert pdfspine.TOOLS.set_annot_stem() == "fitz"
    assert pdfspine.TOOLS.set_annot_stem("Custom") == "Custom"
    assert pdfspine.TOOLS.set_annot_stem(None) == "Custom"
    assert pdfspine.TOOLS.set_annot_stem("fitz") == "fitz"


@pytest.mark.parametrize("stem", ["Tag", "", "x" * 51, "字" * 55, "🙂" * 55])
def test_stem_unicode_limit_and_persistent_id(stem: str) -> None:
    expected = stem[:50]
    assert pdfspine.TOOLS.set_annot_stem(stem) == expected
    doc = _new_document()
    annot = doc[0].add_text_annot((20, 20), "note")
    name = expected + "-A0"
    assert annot.info["id"] == name
    assert name in doc[0].annot_names()
    data = doc.tobytes()
    doc.close()
    with pdfspine.open(stream=data) as reopened:
        page = reopened[0]
        assert page.first_annot.info["id"] == name
        assert page.add_text_annot((20, 50), "next").info["id"] == expected + "-A1"


@pytest.mark.parametrize("value", [b"bytes", ["list"], {"dict": 1}, 3, 1.5, True])
def test_stem_rejects_non_string_without_changing_state(value: object) -> None:
    pdfspine.TOOLS.set_annot_stem("safe")
    with pytest.raises(TypeError):
        pdfspine.TOOLS.set_annot_stem(value)
    assert pdfspine.TOOLS.set_annot_stem() == "safe"


def test_stem_smallest_gap_pages_delete_and_markers() -> None:
    pdfspine.TOOLS.set_annot_stem("Scope")
    with _new_document(2) as doc:
        page = doc[0]
        a = page.add_text_annot((20, 20), "one")
        b = page.add_rect_annot((20, 40, 40, 60))
        assert [a.info["id"], b.info["id"]] == ["Scope-A0", "Scope-A1"]
        doc.xref_set_key(a.xref, "NM", "(Scope-A2)")
        doc.xref_set_key(b.xref, "NM", "(Scope-A0)")
        c = page.add_text_annot((20, 70), "gap")
        assert c.info["id"] == "Scope-A1"
        page.delete_annot(a)
        assert page.add_text_annot((20, 90), "reuse").info["id"] == "Scope-A2"
        assert doc[1].add_text_annot((20, 20), "other page").info["id"] == "Scope-A0"
        widget = pdfspine.Widget()
        widget.field_name = "field"
        widget.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
        widget.rect = pdfspine.Rect(20, 110, 100, 140)
        created = page.add_widget(widget)
        assert (
            next(nm for xref, _, nm in doc.page_annot_xrefs(0) if xref == created.xref)
            == "Scope-W0"
        )
        doc.xref_set_key(created.xref, "NM", "(Scope-A3)")
        assert (
            page.add_text_annot((130, 20), "cross-marker collision").info["id"]
            == "Scope-A4"
        )
        doc.xref_set_key(c.xref, "NM", "(Scope-A01)")
        assert page.add_text_annot((130, 50), "exact names").info["id"] == "Scope-A1"
        pdfspine.TOOLS.set_annot_stem("Changed")
        assert b.info["id"] == "Scope-A0"
        assert page.add_text_annot((130, 80), "new stem").info["id"] == "Changed-A0"


def test_annotation_icon_and_legacy_id_alias_are_independent() -> None:
    with _new_document() as doc:
        page = doc[0]
        annot = page.add_text_annot((20, 20), "note")
        assert annot.info["name"] == "Note"
        assert annot.info["id"] == "fitz-A0"
        annot.set_name("Help")
        assert annot.info["name"] == "Help"
        assert annot.info["id"] == "fitz-A0"
        annot.set_info(name="legacy-id")
        assert annot.info["name"] == "Help"
        assert annot.info["id"] == "legacy-id"
        assert page.load_annot("legacy-id").xref == annot.xref
        data = doc.tobytes()
    with pdfspine.open(stream=data) as reopened:
        assert reopened[0].first_annot.info["name"] == "Help"
        assert reopened[0].first_annot.info["id"] == "legacy-id"


def test_stem_is_shared_by_tools_instances() -> None:
    other = pdfspine.Tools()
    assert other.set_annot_stem("shared") == "shared"
    assert pdfspine.TOOLS.set_annot_stem() == "shared"
    with _new_document() as doc:
        assert doc[0].add_text_annot((20, 20), "note").info["id"] == "shared-A0"


def _exercise_ascii_stem(f):
    result = []
    for stem in ("Scope", "", "x" * 51):
        selected = f.TOOLS.set_annot_stem(stem)
        doc = f.open()
        doc.new_page(width=200, height=200)
        doc.new_page(width=200, height=200)
        page = doc[0]
        a = page.add_text_annot((20, 20), "a")
        b = page.add_rect_annot((20, 40, 40, 60))
        ids = [a.info["id"], b.info["id"]]
        doc.xref_set_key(a.xref, "NM", "(" + selected + "-A2)")
        doc.xref_set_key(b.xref, "NM", "(" + selected + "-A0)")
        gap = page.add_text_annot((20, 70), "gap").info["id"]
        page.delete_annot(a)
        reuse = page.add_text_annot((20, 90), "reuse").info["id"]
        other = doc[1].add_text_annot((20, 20), "other page").info["id"]
        widget = f.Widget()
        widget.field_name = "field"
        widget.field_type = f.PDF_WIDGET_TYPE_TEXT
        widget.rect = f.Rect(20, 110, 100, 140)
        created = page.add_widget(widget)
        widget_id = next(
            nm for xref, _, nm in doc.page_annot_xrefs(0) if xref == created.xref
        )
        data = doc.tobytes()
        doc.close()
        reopened = f.open(stream=data)
        persisted = reopened[0].add_text_annot((130, 20), "persisted").info["id"]
        reopened.close()
        result.append([selected, ids, gap, reuse, other, widget_id, persisted])
    return result


def test_stem_ascii_live_oracle() -> None:
    import inspect
    from python.tests.test_ocg_layers import _real_pymupdf_available, _run_child

    if not _real_pymupdf_available():
        pytest.skip("real PyMuPDF oracle unavailable")
    expected = _run_child(
        "import json, pymupdf\n"
        + inspect.getsource(_exercise_ascii_stem)
        + "\nprint(json.dumps(_exercise_ascii_stem(pymupdf)))\n"
    )
    assert _exercise_ascii_stem(pdfspine) == expected
