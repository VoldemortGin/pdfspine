"""``DOCPY-*`` — long-tail branch coverage for the ``pdfspine.document`` wrappers.

Document-level surface: open/save/tobytes options, xref plumbing, page ops,
TOC ``set_toc_item``/``del_toc_item``, metadata/markinfo/need_appearances,
embedded files, forms, OCG/layers, journal, scrub, ``to_html``, the module
helpers (``_is_content_wrapped`` / ``_font_name`` / ``_text_width`` /
``_ensure_ocr_models_env``) and the ``Colorspace`` / ``linkDest`` / ``Outline`` /
``Link`` / ``TextWriter`` / ``TextPage`` value classes.

All fixtures are generated in-code (raw PDF bytes or a blank ``pdfspine.open()``)
— no external files, no network (PRD §10). Every document is closed so the
``-W error`` gate never trips a ``ResourceWarning``.
"""

from __future__ import annotations

import sys
import types

import pdfspine
import pytest

from pdfspine import document as _doc
from pdfspine.document import (
    _font_name,
    _is_content_wrapped,
    _text_width,
)


# --- self-generated PDF assembler (classic xref) --------------------------
# Copied verbatim from test_text.py so this file is self-contained.
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


def _xobject_pdf() -> bytes:
    """A 1-page PDF whose page resources carry a font, a Form XObject and an
    image XObject — exercises the xref-classification predicates."""
    form = (
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 8 >>\n"
        b"stream\n1 0 0 RG\nendstream"
    )
    image = (
        b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 "
        b"/BitsPerComponent 8 /ColorSpace /DeviceRGB /Length 3 >>\n"
        b"stream\n\xff\x00\x00\nendstream"
    )
    content = b"BT /F1 12 Tf 20 20 Td (Hi) Tj ET /Fm Do"
    return _build_pdf(
        [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
                b"/Resources << /Font << /F1 5 0 R >> "
                b"/XObject << /Fm 6 0 R /Im 7 0 R >> >> /Contents 4 0 R >>",
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
            (6, form),
            (7, image),
        ],
        1,
    )


# =====================================================================
# DOCPY-001 — _is_content_wrapped (the core of Page.is_wrapped)
# =====================================================================
@pytest.mark.parametrize(
    "content,expected",
    [
        (b"", True),  # empty -> trivially balanced
        (b"q BT (a) Tj ET Q", True),  # literal string inside q..Q
        (b"BT (a) Tj ET", False),  # first token not q -> not wrapped
        (b"% comment\nq Q", True),  # leading comment skipped
        (b"q <deadbeef> Q", True),  # hex string inside q
        (b"<dead> BT Q", False),  # hex at depth 0 -> content outside
        (b"q ((x)) Tj Q", True),  # nested balanced parens
        (b"q (a\\)b) Tj Q", True),  # escaped paren inside literal
        (b"q /F1 [1 2] Tf Q", True),  # name + array delimiters skipped
        (b"1 0 0 1 0 0 cm q Q", False),  # operator before the first q
        (b"q Q % trailing", True),  # trailing comment without newline
        (b"q <dead", False),  # unterminated hex string
        (b"q q Q", False),  # unbalanced (depth ends nonzero)
        (b"q Q Q", False),  # over-closed (depth ends negative)
    ],
)
def test_docpy_001_is_content_wrapped(content: bytes, expected: bool) -> None:
    assert _is_content_wrapped(content) is expected


def test_docpy_001_page_is_wrapped_roundtrip() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        assert page.is_wrapped is True  # empty content
        page.insert_text((50, 100), "Hi")
        assert page.is_wrapped is True  # inserted content is q..Q balanced
        page.wrap_contents()
        assert page.is_wrapped is True


# =====================================================================
# DOCPY-002 — _ensure_ocr_models_env exception handlers
# =====================================================================
@pytest.fixture()
def _restore_models_env():
    sentinel = object()
    keys = ("PDFSPINE_OCR_MODELS", "OCRSPINE_MODELS")
    saved = {k: _doc.os.environ.get(k, sentinel) for k in keys}
    try:
        yield
    finally:
        for k, v in saved.items():
            if v is sentinel:
                _doc.os.environ.pop(k, None)
            else:
                _doc.os.environ[k] = v


def test_docpy_002_ensure_ocr_models_env_survives_broken_packages(
    monkeypatch, _restore_models_env
) -> None:
    """A data package whose ``models_dir()`` raises must not crash the helper nor
    leak env — both tiers' ``except Exception: pass`` handlers run and the env is
    left for the Rust engine to report on."""
    monkeypatch.delenv("PDFSPINE_OCR_MODELS", raising=False)
    monkeypatch.delenv("OCRSPINE_MODELS", raising=False)

    def _boom() -> str:
        raise RuntimeError("partial install")

    shared = types.ModuleType("ocrspine_models")
    shared.models_dir = _boom  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "ocrspine_models", shared)
    legacy = types.ModuleType("pdfspine_ocr_models")
    legacy.models_dir = _boom  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "pdfspine_ocr_models", legacy)

    _doc._ensure_ocr_models_env()  # must not raise

    assert "PDFSPINE_OCR_MODELS" not in _doc.os.environ
    assert "OCRSPINE_MODELS" not in _doc.os.environ


# =====================================================================
# DOCPY-003 — _font_name / _text_width helpers
# =====================================================================
def test_docpy_003_font_name_variants() -> None:
    assert _font_name(None) == "helv"
    assert _font_name("Times-Roman") == "Times-Roman"
    assert _font_name(pdfspine.Font("helv")) == "Helvetica"
    # An object without a str .name falls back to helv.
    assert _font_name(object()) == "helv"


def test_docpy_003_text_width_metrics_and_fallback(monkeypatch) -> None:
    good = _text_width("abc", "helv", 12.0)
    assert good > 0

    class _Boom:
        def __init__(self, *_a, **_k) -> None:
            raise RuntimeError("no font")

    monkeypatch.setattr(_doc._core, "Font", _Boom)
    # The except path estimates len*fontsize*0.5.
    assert _text_width("abcd", "helv", 10.0) == pytest.approx(4 * 10.0 * 0.5)


# =====================================================================
# DOCPY-004 — Colorspace repr / hash / eq
# =====================================================================
def test_docpy_004_colorspace_repr_hash_eq() -> None:
    assert repr(pdfspine.csRGB) == "Colorspace(DeviceRGB)"
    assert repr(pdfspine.csGRAY) == "Colorspace(DeviceGray)"
    assert hash(pdfspine.csRGB) == hash(pdfspine.csRGB)
    assert pdfspine.csRGB == pdfspine.csRGB
    assert pdfspine.csRGB != pdfspine.csCMYK
    assert pdfspine.csGRAY.is_gray is True
    assert {pdfspine.csRGB, pdfspine.csRGB} == {pdfspine.csRGB}
    with pytest.raises(ValueError):
        _doc.Colorspace(999)


# =====================================================================
# DOCPY-005 — open / tobytes / save options + convert_to_pdf
# =====================================================================
def test_docpy_005_tobytes_roundtrip_with_options() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        raw = doc.tobytes(garbage=3, deflate=True)
    assert raw.startswith(b"%PDF")
    with pdfspine.open(stream=raw) as reopened:
        assert reopened.page_count == 1
    # write is an alias of tobytes.
    assert _doc.Document.write is _doc.Document.tobytes


def test_docpy_005_convert_to_pdf_reparses() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=80, height=80)
        pdf_bytes = doc.convert_to_pdf(from_page=0, to_page=-1, rotate=0)
    with pdfspine.open(stream=pdf_bytes) as reparsed:
        assert reparsed.is_pdf is True
        assert reparsed.page_count == 1


def test_docpy_005_save_and_ez_save(tmp_path) -> None:
    out = tmp_path / "out.pdf"
    ez = tmp_path / "ez.pdf"
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=120)
        doc.save(out, garbage=1)
        doc.ez_save(ez)
    assert out.read_bytes().startswith(b"%PDF")
    assert ez.read_bytes().startswith(b"%PDF")
    with pdfspine.open() as doc:
        with pytest.raises(ValueError):
            doc.saveIncr(None)


# =====================================================================
# DOCPY-006 — document facts + __repr__
# =====================================================================
def test_docpy_006_document_facts() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=100, height=100)
        assert doc.is_closed is False
        assert doc.is_reflowable is False
        assert isinstance(doc.is_fast_webaccess, bool)
        assert isinstance(doc.can_save_incrementally(), bool)
        assert isinstance(doc.permissions, int)
        page.insert_text((10, 10), "x")
        assert doc.is_dirty is True
        assert repr(doc) == "<pdfspine.Document page_count=1>"
    assert doc.is_closed is True


# =====================================================================
# DOCPY-007 — low-level xref plumbing
# =====================================================================
def test_docpy_007_xref_classification_and_streams() -> None:
    with pdfspine.open(stream=_xobject_pdf()) as doc:
        assert doc.is_stream(4) is True
        assert doc.xref_stream(4) == b"BT /F1 12 Tf 20 20 Td (Hi) Tj ET /Fm Do"
        assert isinstance(doc.xref_stream_raw(4), bytes)
        assert doc.xref_is_font(5) is True
        assert doc.xref_is_xobject(6) is True  # Form
        assert doc.xref_is_xobject(7) is False  # Image is not a Form
        assert doc.xref_is_image(7) is True
        keys = doc.xref_get_keys(6)
        assert "Subtype" in keys and "BBox" in keys
        assert doc.pdf_trailer().startswith("<<")
        assert doc.get_page_text(0, "text").startswith("Hi")
        # negative pno resolves from the end.
        assert doc.get_page_xobjects(-1)


def test_docpy_007_update_stream_new_and_clear() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        xref = doc.get_new_xref()
        doc.update_stream(xref, b"hello world", new=True)
        assert doc.is_stream(xref) is True
        assert doc.xref_stream(xref) == b"hello world"
        # stream=None clears the body to empty bytes.
        doc.update_stream(xref)
        assert doc.xref_stream(xref) == b""


# =====================================================================
# DOCPY-008 — page ops (negative indices, delete_pages forms, insert_page)
# =====================================================================
def test_docpy_008_page_ops_negative_and_ranges() -> None:
    with pdfspine.open() as doc:
        for i in range(5):
            doc.new_page(width=100, height=100).insert_text((10, 10), str(i))
        assert doc.load_page(-1).number == 4
        assert doc[-1].number == 4
        assert doc.page_cropbox(-1).width == 100
        assert doc.page_mediabox(-1).height == 100

        doc.copy_page(0, -1)  # append a shallow copy of page 0
        assert doc.page_count == 6
        doc.move_page(0, -1)  # move first to the end
        assert doc.page_count == 6
        doc.delete_page(-1)  # delete last
        assert doc.page_count == 5

        doc.delete_pages(0, 1)  # inclusive range form
        assert doc.page_count == 3
        doc.delete_pages([0])  # list form
        assert doc.page_count == 2
        doc.delete_pages(numbers=[-1])  # numbers= + negative
        assert doc.page_count == 1
        with pytest.raises(ValueError):
            doc.delete_pages(1, 2, 3)  # too many positional args


def test_docpy_008_insert_page_with_text() -> None:
    with pdfspine.open() as doc:
        n = doc.insert_page(-1, text=["line one", "line two"], fontsize=10)
        assert n == 2
        assert doc.page_count == 1
        assert doc.insert_page(-1) == 0  # no text -> 0 lines


def test_docpy_008_copy_move_negative_pno() -> None:
    with pdfspine.open() as doc:
        for i in range(3):
            doc.new_page(width=100, height=100).insert_text((10, 10), str(i))
        doc.copy_page(-1)  # negative pno resolves from the end, appends a copy
        assert doc.page_count == 4
        doc.move_page(-1)  # move last page to the end (negative pno path)
        assert doc.page_count == 4


def test_docpy_008_insert_file_from_document_and_path(tmp_path) -> None:
    src_path = tmp_path / "src.pdf"
    with pdfspine.open() as src:
        src.new_page(width=100, height=100).insert_text((10, 10), "src")
        src.save(src_path)
        with pdfspine.open() as dst:
            dst.new_page(width=100, height=100)
            dst.insert_file(src)  # Document source
            assert dst.page_count == 2
            dst.insert_file(str(src_path))  # path source -> open(path)
            assert dst.page_count == 3


# =====================================================================
# DOCPY-009 — TOC set_toc_item / del_toc_item / _dest_action
# =====================================================================
def _toc_doc() -> pdfspine.Document:
    doc = pdfspine.open()
    for _ in range(3):
        doc.new_page(width=200, height=300)
    doc.set_toc([[1, "One", 1], [2, "Sub", 2], [1, "Two", 3]])
    return doc


def test_docpy_009_set_toc_item_uri_goto_and_title() -> None:
    with _toc_doc() as doc:
        doc.set_toc_item(0, kind=pdfspine.linkDest.LINK_URI, uri="https://ex.org", title="URI")
        doc.set_toc_item(1, kind=pdfspine.linkDest.LINK_GOTO, pno=2, to=pdfspine.Point(72, 100))
        doc.set_toc_item(2, title="Renamed")  # title-only
        doc.set_toc_item(2, kind=None, title=None)  # no-op path
        toc = doc.get_toc()
        assert toc[0][1] == "URI"
        assert toc[2][1] == "Renamed"


def test_docpy_009_set_toc_item_dest_dict_with_style() -> None:
    with _toc_doc() as doc:
        doc.set_toc_item(
            0,
            dest_dict={
                "kind": pdfspine.linkDest.LINK_GOTO,
                "page": 0,
                "to": pdfspine.Point(10, 20),
                "color": (1, 0, 0),
                "bold": True,
                "italic": True,
            },
            title="Styled",
        )
        assert doc.get_toc()[0][1] == "Styled"


def test_docpy_009_set_toc_item_errors_and_delete() -> None:
    with _toc_doc() as doc:
        with pytest.raises(ValueError, match="bad page number"):
            doc.set_toc_item(0, kind=pdfspine.linkDest.LINK_GOTO, pno=None)
        with pytest.raises(ValueError, match="bad bookmark dest"):
            doc.set_toc_item(0, kind=99)  # unsupported kind
        with pytest.raises(ValueError, match="bad bookmark dest"):
            doc.set_toc_item(0, dest_dict={"kind": 0})
        with pytest.raises(ValueError, match="bad color value"):
            doc.set_toc_item(
                0, dest_dict={"kind": 2, "uri": "u", "color": (2, 2, 2)}
            )
        # kind == LINK_NONE deletes (dims) the item.
        doc.set_toc_item(2, kind=pdfspine.linkDest.LINK_NONE)
        assert doc.get_toc()[2][2] == -1  # page reset on a neutralized item


def test_docpy_009_dest_action_uri() -> None:
    action = _doc.Document._dest_action(0, {"kind": 2, "uri": "https://x"})
    assert action == "/A<</S/URI/URI(https://x)>>"
    assert _doc.Document._dest_action(0, {"kind": 0}) == ""


# =====================================================================
# DOCPY-010 — metadata / markinfo / need_appearances
# =====================================================================
def test_docpy_010_metadata_and_markinfo() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        doc.set_metadata({"title": "T", "author": None})
        assert (doc.metadata or {}).get("title") == "T"
        assert doc.set_markinfo({"Marked": True}) is True
        mi = doc.markinfo
        assert mi["Marked"] is True and mi["Suspects"] is False


def test_docpy_010_need_appearances() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        assert doc.need_appearances() is None  # no AcroForm yet
        w = pdfspine.Widget()
        w.field_name = "f"
        w.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
        w.rect = pdfspine.Rect(10, 10, 100, 40)
        w.field_value = "v"
        page.add_widget(w)
        assert doc.need_appearances(True) is True
        assert doc.need_appearances() is True


# =====================================================================
# DOCPY-011 — embedded files
# =====================================================================
def test_docpy_011_embedded_files_lifecycle() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        doc.embfile_add("a", b"payload-a", filename="a.bin", desc="first")
        doc.embfile_add("b", b"payload-b")
        assert doc.embfile_count() == 2
        assert set(doc.embfile_names()) == {"a", "b"}
        assert doc.embfile_get("a") == b"payload-a"
        assert doc.embfile_info("a")["filename"] == "a.bin"
        doc.embfile_upd("a", buffer=b"new-a", desc="updated")
        assert doc.embfile_get("a") == b"new-a"
        with pytest.raises(ValueError):
            doc.embfile_upd(5)  # index out of range
        with pytest.raises(ValueError):
            doc.embfile_upd("missing")
        doc.embfile_del("b")
        assert doc.embfile_count() == 1


# =====================================================================
# DOCPY-012 — AcroForm forms
# =====================================================================
def test_docpy_012_form_helpers() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        w = pdfspine.Widget()
        w.field_name = "name"
        w.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
        w.rect = pdfspine.Rect(10, 10, 120, 40)
        w.field_value = "start"
        page.add_widget(w)
        assert doc.is_form_pdf is True
        assert "name" in doc.form_field_names()
        doc.form_fill("name", "filled")
        doc.form_flatten()
        doc.bake(annots=True, widgets=True)


# =====================================================================
# DOCPY-013 — OCG / optional-content layers
# =====================================================================
def test_docpy_013_ocg_layers() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        xref = doc.add_ocg("Layer 1", on=True, intent="View")
        assert xref in doc.get_ocgs()
        assert doc.ocg_state(xref) is True
        layer = doc.get_layer()
        assert xref in layer["on"]
        off_xref = doc.add_ocg("Layer 2", on=False)
        doc.set_layer(on=[off_xref], off=[xref])
        assert isinstance(doc.layer_ui_configs(), list)
        # Bind an object to a layer via /OC (snake + camelCase alias).
        content_xref = doc.get_new_xref()
        doc.update_stream(content_xref, b"q Q", new=True)
        doc.set_oc(content_xref, xref)
        other_xref = doc.get_new_xref()
        doc.update_stream(other_xref, b"q Q", new=True)
        doc.setOC(other_xref, xref)
        assert doc.getLayer() == doc.get_layer()
        assert doc.addOCG("Layer 3") in doc.getOCGs()


# =====================================================================
# DOCPY-014 — undo/redo journal
# =====================================================================
def test_docpy_014_journal() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        doc.journal_enable()
        assert doc.journal_is_enabled() is True
        doc.journal_save_state()
        can = doc.journal_can_do()
        assert set(can) == {"undo", "redo"}
        assert isinstance(doc.journal_can_undo(), bool)
        assert isinstance(doc.journal_can_redo(), bool)


# =====================================================================
# DOCPY-015 — scrub
# =====================================================================
def test_docpy_015_scrub() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=100, height=100)
        page.insert_link({"kind": 2, "from": (5, 5, 40, 20), "uri": "https://x"})
        doc.set_metadata({"title": "secret"})
        doc.scrub(remove_links=True, metadata=True)
        assert page.get_links() == []


# =====================================================================
# DOCPY-016 — to_html title falls back to file basename
# =====================================================================
def test_docpy_016_to_html_title_from_basename(tmp_path) -> None:
    path = tmp_path / "report.pdf"
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100).insert_text((10, 10), "body")
        doc.save(path)
    with pdfspine.open(path) as doc:
        html = doc.to_html()
        assert "<title>report.pdf</title>" in html
        assert html.startswith("<!doctype html>")


# =====================================================================
# DOCPY-017 — extract_font named-dict form
# =====================================================================
def test_docpy_017_extract_font_named_dict() -> None:
    with pdfspine.open(stream=_xobject_pdf()) as doc:
        as_tuple = doc.extract_font(5)
        assert isinstance(as_tuple, tuple) and len(as_tuple) == 4
        as_dict = doc.extract_font(5, named=True)
        assert set(as_dict) == {"name", "ext", "type", "content"}
        assert as_dict["name"] == as_tuple[0]
        # camelCase alias forwards to extract_font.
        assert doc.extractFont(5) == as_tuple


# =====================================================================
# DOCPY-018 — Document camelCase aliases behave like the snake methods
# =====================================================================
def test_docpy_018_document_camelcase_aliases() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        doc.set_toc([[1, "A", 1]])
        assert doc.getToC() == doc.get_toc()
        assert doc.getOCGs() == doc.get_ocgs()
        assert doc.getLayer() == doc.get_layer()
        assert doc.isFormPDF == doc.is_form_pdf
        assert doc.embfileNames() == doc.embfile_names()
        assert doc.embfileCount() == doc.embfile_count()
        assert list(doc.FormFonts) == list(doc.FormFonts)
        # setter-style aliases drive the same core path.
        doc.setMetadata({"title": "Z"})
        assert (doc.metadata or {}).get("title") == "Z"
        doc.setToC([[1, "B", 1]])
        assert doc.get_toc()[0][1] == "B"
        doc.setTocItem(0, title="C")  # camelCase alias -> set_toc_item
        assert doc.get_toc()[0][1] == "C"


def test_docpy_018_embfile_camelcase_aliases() -> None:
    with pdfspine.open() as doc:
        doc.new_page(width=100, height=100)
        doc.embfileAdd("z", b"zz")
        assert doc.embfileGet("z") == b"zz"
        assert "z" in doc.embfileNames()
        assert doc.embfileCount() == 1
        assert doc.embfileInfo("z")["filename"]
        doc.embfileDel("z")
        assert doc.embfileCount() == 0


# =====================================================================
# DOCPY-019 — TextWriter branches
# =====================================================================
def test_docpy_019_textwriter_render_and_helpers() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=200)
        tw = pdfspine.TextWriter(page.rect)
        assert tw.text_rect == pdfspine.Rect(0, 0, 0, 0)  # empty
        tw.append((30, 100), "Hello", fontsize=12)
        assert tw.text_rect.width > 0
        tw.writeText(page)  # camelCase alias -> renders onto the page
        assert "Hello" in page.get_text("text")
        assert tw.clean_rtl("abc") == "abc"
        assert repr(tw).startswith("<pdfspine.TextWriter segments=")


def test_docpy_019_textwriter_appendv_and_fill_overflow() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        tw = pdfspine.TextWriter(page.rect)
        tw.appendv((20, 20), "AB", fontsize=10)
        # A tiny box forces overflow lines to be returned.
        overflow = tw.fill_textbox(
            pdfspine.Rect(0, 0, 20, 6), "one two three four five", fontsize=10
        )
        assert overflow  # some lines did not fit
        tw.write_text(page)


# =====================================================================
# DOCPY-020 — linkDest / Outline / Link value-class reprs
# =====================================================================
def test_docpy_020_outline_link_reprs() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=200, height=200)
        doc.set_toc([[1, "Chapter", 1]])
        outline = doc.outline
        assert outline is not None
        assert outline.uri is None
        assert outline.x == 0.0 and outline.y == 0.0
        assert repr(outline).startswith("<pdfspine.Outline title=")
        dest = outline.dest
        assert repr(dest).startswith("_OutlineDest(")

        page.insert_link({"kind": 2, "from": (5, 5, 40, 20), "uri": "https://x"})
        link = page.first_link
        assert link is not None
        assert repr(link).startswith("<pdfspine.Link kind=")
        assert repr(link.dest).startswith("linkDest(")
        assert link.linkDest.is_uri is True
        # Compatibility setters are accepted no-ops.
        assert link.set_border(width=2) is None
        assert link.set_colors(stroke=(1, 0, 0)) is None
        assert link.set_flags(4) is None


# =====================================================================
# DOCPY-021 — TextPage extract aliases
# =====================================================================
def test_docpy_021_textpage_extract_aliases() -> None:
    with pdfspine.open() as doc:
        page = doc.new_page(width=300, height=200)
        page.insert_text((30, 100), "alpha beta")
        tp = page.get_textpage()
        assert isinstance(tp.extractWORDS(), list)
        assert isinstance(tp.extractBLOCKS(), list)
        assert isinstance(tp.extractDICT(), dict)
        assert isinstance(tp.extractRAWDICT(), dict)
        assert isinstance(tp.extractJSON(), str)
        assert tp.rect == pdfspine.Rect(0, 0, page.rect.width, page.rect.height)
        assert repr(tp)  # non-empty
