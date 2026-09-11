"""The subset-name policy changes presentation of already owned text."""

import json

import pdfspine
import pytest

from python.tests.test_rawdict_serialization import _document, _spans


@pytest.fixture(autouse=True)
def reset_policy():
    pdfspine.TOOLS.set_subset_fontnames(False)
    yield
    pdfspine.TOOLS.set_subset_fontnames(False)


def subset_document(content=b"BT /F1 12 Tf 40 700 Td (A) Tj /F2 12 Tf (B) Tj ET"):
    doc = _document(content)
    doc.xref_set_key(5, "BaseFont", "/ABCDEF+Helvetica")
    font = doc.get_new_xref()
    doc.update_object(
        font,
        "<< /Type /Font /Subtype /Type1 /BaseFont /UVWXYZ+Helvetica /Encoding /WinAnsiEncoding >>",
    )
    doc.xref_set_key(3, "Resources", f"<< /Font << /F1 5 0 R /F2 {font} 0 R >> >>")
    return doc


def span_text(span):
    return span.get("text", "".join(char["c"] for char in span.get("chars", [])))


def test_truthiness_query_and_failed_conversion_are_atomic():
    assert pdfspine.TOOLS.set_subset_fontnames() is False
    assert pdfspine.TOOLS.set_subset_fontnames(on=[1]) is True
    assert pdfspine.TOOLS.set_subset_fontnames(None) is True

    class Broken:
        def __bool__(self):
            raise RuntimeError("truth conversion failed")

    with pytest.raises(RuntimeError, match="truth conversion failed"):
        pdfspine.TOOLS.set_subset_fontnames(Broken())
    assert pdfspine.TOOLS.set_subset_fontnames() is True
    assert pdfspine.TOOLS.set_subset_fontnames([]) is False


@pytest.mark.parametrize("source", ["page", "displaylist", "extended"])
@pytest.mark.parametrize("creation", [False, True])
def test_existing_closed_textpage_switches_without_relayout(source, creation):
    doc = subset_document()
    page = doc[0]
    pdfspine.TOOLS.set_subset_fontnames(creation)
    tp = (
        page.get_displaylist().get_textpage()
        if source == "displaylist"
        else page.get_textpage()
    )
    if source == "extended":
        page.extend_textpage(tp)
    # The textpage owns its recorded names; neither xref edits nor close may
    # make later policy changes reread live resources.
    doc.xref_set_key(5, "BaseFont", "/CHANGED+Courier")
    doc.close()
    pdfspine.TOOLS.set_subset_fontnames(False)
    before = tp.extractRAWDICT()
    unchanged = [
        tp.extractHTML(),
        tp.extractXHTML(),
        tp.extractXML(),
        tp.extractText(),
        tp.extractWORDS(),
    ]
    pdfspine.TOOLS.set_subset_fontnames(True)
    expected = [("ABCDEF+Helvetica", "A"), ("UVWXYZ+Helvetica", "B")]
    if source == "extended":
        expected *= 2
    for method in ("extractDICT", "extractRAWDICT", "extractJSON", "extractRAWJSON"):
        data = getattr(tp, method)()
        if isinstance(data, str):
            data = json.loads(data)
        spans = _spans(data)
        assert [(span["font"], span_text(span)) for span in spans] == expected
        for first, second in zip(spans[::2], spans[1::2]):
            assert first["text_matrix"] is not None
            assert second["text_matrix"] is None and second["ctm"] is None
            assert first["bbox"][2] <= second["bbox"][0] + 1e-5
    assert [
        tp.extractHTML(),
        tp.extractXHTML(),
        tp.extractXML(),
        tp.extractText(),
        tp.extractWORDS(),
    ] == unchanged
    pdfspine.TOOLS.set_subset_fontnames(False)
    assert tp.extractRAWDICT() == before


def test_synthetic_space_inherits_following_font_and_trace_uses_policy():
    doc = subset_document(b"BT /F1 12 Tf 40 700 Td (A) Tj /F2 12 Tf 12 0 Td (B) Tj ET")
    page = doc[0]
    before = page.get_texttrace()
    pdfspine.TOOLS.set_subset_fontnames(True)
    spans = _spans(page.get_text("rawdict"))
    assert [(span["font"], span_text(span)) for span in spans] == [
        ("ABCDEF+Helvetica", "A"),
        ("UVWXYZ+Helvetica", " B"),
    ]
    assert spans[1]["chars"][0]["synthetic"] is True
    assert [span["font"] for span in page.get_texttrace()] == [
        "ABCDEF+Helvetica",
        "UVWXYZ+Helvetica",
    ]
    pdfspine.TOOLS.set_subset_fontnames(False)
    assert page.get_texttrace() == before


def test_form_local_font_aliases_keep_their_own_original_names():
    doc = subset_document(b"/X1 Do /X2 Do")
    fonts = [5, next(item[0] for item in doc[0].get_fonts() if item[4] == "F2")]
    forms = []
    for index, (font, text, x) in enumerate(zip(fonts, ["A", "B"], [40, 48.004]), 1):
        form = doc.get_new_xref()
        doc.update_object(
            form,
            f"<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Matrix [1 0 0 1 {x} 650] /Resources << /Font << /F1 {font} 0 R >> >> >>",
        )
        doc.update_stream(form, f"BT /F1 12 Tf 0 50 Td ({text}) Tj ET".encode())
        forms.append(f"/X{index} {form} 0 R")
    doc.xref_set_key(3, "Resources", "<< /XObject << " + " ".join(forms) + " >> >>")
    tp = doc[0].get_displaylist().get_textpage()
    doc.close()
    pdfspine.TOOLS.set_subset_fontnames(True)
    assert [(span["font"], span_text(span)) for span in _spans(tp.extractDICT())] == [
        ("ABCDEF+Helvetica", "A"),
        ("UVWXYZ+Helvetica", "B"),
    ]


def test_equal_raw_names_do_not_split_by_resource_or_arc_identity():
    doc = subset_document()
    second = next(item[0] for item in doc[0].get_fonts() if item[4] == "F2")
    doc.xref_set_key(second, "BaseFont", "/ABCDEF+Helvetica")
    tp = doc[0].get_textpage()
    pdfspine.TOOLS.set_subset_fontnames(True)
    assert [(span["font"], span_text(span)) for span in _spans(tp.extractDICT())] == [
        ("ABCDEF+Helvetica", "AB")
    ]


@pytest.mark.parametrize(
    "name", ["abcdef+Helvetica", "123456+Helvetica", "ABCDE+Helvetica"]
)
def test_noncanonical_prefixes_remain_unchanged_in_both_modes(name):
    doc = _document(b"BT /F1 12 Tf 40 700 Td (A) Tj ET")
    doc.xref_set_key(5, "BaseFont", "/" + name)
    tp = doc[0].get_textpage()
    before = tp.extractRAWDICT()
    pdfspine.TOOLS.set_subset_fontnames(True)
    assert tp.extractRAWDICT() == before
    assert _spans(before)[0]["font"] == name


def test_each_json_output_uses_one_policy_snapshot_during_concurrent_toggle():
    import threading

    doc = subset_document()
    tp = doc[0].get_textpage()
    for _ in range(12):
        doc[0].extend_textpage(tp)
    expected = {tp.extractRAWJSON()}
    pdfspine.TOOLS.set_subset_fontnames(True)
    expected.add(tp.extractRAWJSON())
    assert len(expected) == 2
    stop = threading.Event()

    def toggle():
        while not stop.is_set():
            pdfspine.TOOLS.set_subset_fontnames(False)
            pdfspine.TOOLS.set_subset_fontnames(True)

    thread = threading.Thread(target=toggle)
    thread.start()
    try:
        for _ in range(40):
            assert tp.extractRAWJSON() in expected
    finally:
        stop.set()
        thread.join()
