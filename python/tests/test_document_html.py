from __future__ import annotations

import pdfspine
import pytest


def test_to_html_empty_document_is_complete_html5() -> None:
    doc = pdfspine.open()

    html = doc.to_html()

    assert html.startswith("<!doctype html>\n<html")
    assert '<meta charset="utf-8">' in html
    assert "<title>PDF document</title>" in html
    assert "<body>\n</body>" in html
    assert html.endswith("</html>\n")


def test_to_html_preserves_page_fragments_in_order() -> None:
    doc = pdfspine.open()
    doc.new_page().insert_text((72, 72), "First page")
    doc.new_page().insert_text((72, 72), "Second page")
    fragments = [page.get_text("html") for page in doc]

    html = doc.to_html()

    assert all(html.count(fragment) == 1 for fragment in fragments)
    assert html.index(fragments[0]) < html.index(fragments[1])


def test_to_html_escapes_metadata_title() -> None:
    doc = pdfspine.open()
    doc.set_metadata({"title": """R&D <notes> "draft" 'one'"""})

    html = doc.to_html()

    assert (
        "<title>R&amp;D &lt;notes&gt; &quot;draft&quot; &#x27;one&#x27;</title>" in html
    )


def test_save_html_accepts_pathlike_and_writes_utf8(tmp_path) -> None:
    doc = pdfspine.open()
    doc.set_metadata({"title": "Résumé 文档"})
    output = tmp_path / "document.html"

    result = doc.save_html(output)

    assert result is None
    assert output.read_text(encoding="utf-8") == doc.to_html()


def test_save_html_propagates_write_errors(tmp_path) -> None:
    doc = pdfspine.open()

    with pytest.raises(FileNotFoundError):
        doc.save_html(tmp_path / "missing" / "document.html")
