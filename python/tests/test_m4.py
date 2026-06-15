"""M4e Python gates: the full M4 edit surface through ``oxide_pdf`` + the ``fitz``
deprecated-alias shim (PRD §8.8 / §9.4 / §9.5 / §12 M4).

Covers content insert / draw / Shape, the annotation family + ``/AP``
portability, the redaction Python gate (gone-after-reopen), forms + ``Widget``,
embedded files, ``scrub``, and ``fitz`` camelCase parity. All fixtures are
self-generated in-test (PRD §10). Catalog IDs ``PYM4-*``.
"""

from __future__ import annotations

import oxide_pdf
import pytest


# --- fixtures (self-built raw PDF bytes; no external files) ----------------


def _build_pdf(objects: list[tuple[int, bytes]], root: int) -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    objects = sorted(objects, key=lambda o: o[0])
    for num, body in objects:
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
    size = max(offsets) + 1
    startxref = len(out)
    out += b"xref\n" + f"0 {size}\n".encode() + b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in offsets:
            out += f"{offsets[num]:010} 00000 n \n".encode()
        else:
            out += b"0000000000 65535 f \n"
    out += b"trailer\n" + f"<< /Size {size} /Root {root} 0 R >>\n".encode()
    out += b"startxref\n" + f"{startxref}\n".encode() + b"%%EOF\n"
    return bytes(out)


def _widths_font() -> bytes:
    """Helvetica with explicit ``/Widths`` so the interpreter can measure glyph
    advances — required for the redaction glyph-overlap test (PyMuPDF too)."""
    widths = b" ".join(b"600" for _ in range(32, 127))
    return (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths ["
        + widths
        + b"] >>"
    )


def blank_doc(media: tuple[int, int, int, int] = (0, 0, 612, 792)) -> "oxide_pdf.Document":
    """A one-page doc with a shared ``/Widths`` Helvetica under ``/F1`` and no
    content."""
    mb = " ".join(str(v) for v in media).encode()
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [" + mb + b"] "
            b"/Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (4, b"<< /Length 0 >>\nstream\n\nendstream"),
        (5, _widths_font()),
    ]
    return oxide_pdf.open(stream=_build_pdf(objects, root=1))


def secret_doc(lead: str, secret: str) -> tuple[bytes, tuple[float, float, float, float]]:
    """A page showing ``lead`` then ``secret`` on one line; returns the bytes and
    the top-left rect covering only ``secret`` (mirrors the Rust harness)."""
    char_w = 12.0 * 0.6
    x_lead = 72.0
    x_secret = x_lead + len(lead) * char_w
    x_end = x_secret + len(secret) * char_w
    body = (
        f"BT /F1 12 Tf 1 0 0 1 {x_lead:g} 700 Tm ({lead}) Tj "
        f"1 0 0 1 {x_secret:g} 700 Tm ({secret}) Tj ET"
    ).encode()
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (4, b"<< /Length " + str(len(body)).encode() + b" >>\nstream\n" + body + b"\nendstream"),
        (5, _widths_font()),
    ]
    # Top-left rect: user y 698..710 → top-left y (792-710)..(792-698) = 82..94.
    rect = (x_secret - 1.0, 82.0, x_end + 1.0, 96.0)
    return _build_pdf(objects, root=1), rect


def acroform_doc() -> bytes:
    """A single text-field AcroForm (merged field+widget), value ``init``."""
    objects = [
        (1, b"<< /Type /Catalog /Pages 2 0 R /AcroForm 10 0 R >>"),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Contents 4 0 R /Annots [11 0 R] "
            b"/Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (4, b"<< /Length 0 >>\nstream\n\nendstream"),
        (5, _widths_font()),
        (
            11,
            b"<< /Type /Annot /Subtype /Widget /P 3 0 R "
            b"/FT /Tx /T (tx1) /TU (Text One) /Rect [72 700 272 720] "
            b"/V (init) /DA (0 0 1 rg /F1 12 Tf) /Q 0 >>",
        ),
        (
            10,
            b"<< /Fields [11 0 R] /NeedAppearances false "
            b"/DA (0 0 1 rg /F1 12 Tf) /DR << /Font << /F1 5 0 R >> >> >>",
        ),
    ]
    return _build_pdf(objects, root=1)


# --- PYM4-INSERT-* : content insert / draw / Shape -------------------------


def test_pym4_insert_001_insert_text(tmp_path):
    doc = blank_doc()
    doc[0].insert_text((72, 100), "INSERTED", fontname="helv", fontsize=12)
    re = oxide_pdf.open(stream=doc.tobytes())
    assert "INSERTED" in re[0].get_text()


def test_pym4_insert_002_insert_textbox():
    doc = blank_doc()
    rv = doc[0].insert_textbox(oxide_pdf.Rect(72, 72, 400, 200), "BOXED TEXT")
    assert isinstance(rv, float)
    re = oxide_pdf.open(stream=doc.tobytes())
    assert "BOXED" in re[0].get_text()


def test_pym4_draw_001_draw_rect_line_get_drawings():
    doc = blank_doc()
    page = doc[0]
    page.draw_rect((50, 50, 150, 150), color=(0, 0, 1), width=2)
    page.draw_line((10, 10), (200, 200), color=(1, 0, 0))
    drawings = page.get_drawings()
    assert len(drawings) >= 2
    # reopen stays valid
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.page_count == 1


def test_pym4_shape_001_new_shape_commit():
    doc = blank_doc()
    page = doc[0]
    shape = page.new_shape()
    shape.draw_rect((40, 40, 120, 120))
    shape.finish(color=(0, 0, 0), fill=(0.5, 0.5, 0.5), width=1)
    shape.commit()
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.page_count == 1
    assert len(re[0].get_drawings()) >= 1


# --- PYM4-ANNOT-* : annotations + /AP portability --------------------------

_HL_QUAD = (60, 100, 200, 100, 60, 120, 200, 120)


def test_pym4_annot_001_add_and_list():
    doc = blank_doc()
    page = doc[0]
    hl = page.add_highlight_annot([_HL_QUAD])
    ft = page.add_freetext_annot(oxide_pdf.Rect(60, 200, 260, 240), "Free text")
    assert hl.type[1] == "Highlight"
    assert ft.type[1] == "FreeText"
    listed = {a.type[1] for a in page.annots()}
    assert {"Highlight", "FreeText"} <= listed
    # rect is a fitz Rect value type
    assert isinstance(ft.rect, oxide_pdf.Rect)


def test_pym4_annot_002_set_colors_update_persists():
    doc = blank_doc()
    page = doc[0]
    a = page.add_rect_annot((60, 60, 200, 120), color=(0, 0, 0))
    a.set_colors(stroke=(1, 0, 0))
    a.update()
    assert a.colors["stroke"] == (1.0, 0.0, 0.0)
    re = oxide_pdf.open(stream=doc.tobytes())
    ra = list(re[0].annots())
    assert len(ra) == 1
    assert ra[0].type[1] == "Square"
    assert ra[0].has_appearance  # /AP /N regenerated and persisted


def test_pym4_annot_003_delete():
    doc = blank_doc()
    page = doc[0]
    a = page.add_rect_annot((60, 60, 200, 120))
    assert len(page.annot_xrefs()) == 1
    page.delete_annot(a)
    assert page.annot_xrefs() == []
    re = oxide_pdf.open(stream=doc.tobytes())
    assert list(re[0].annots()) == []


def test_pym4_annot_004_ap_portability():
    """Every added subtype reopens with an /AP /N appearance stream."""
    doc = blank_doc()
    page = doc[0]
    page.add_highlight_annot([_HL_QUAD])
    page.add_freetext_annot(oxide_pdf.Rect(60, 200, 260, 240), "FT")
    page.add_rect_annot((60, 300, 200, 360), color=(0, 0, 0))
    page.add_circle_annot((60, 400, 200, 460), color=(0, 0, 0))
    page.add_line_annot((10, 10), (100, 100), color=(0, 0, 0))
    re = oxide_pdf.open(stream=doc.tobytes())
    annots = list(re[0].annots())
    assert len(annots) == 5
    for a in annots:
        assert a.has_appearance, f"{a.type[1]} missing /AP /N after reopen"


# --- PYM4-REDACT-* : redaction Python gate ---------------------------------


def test_pym4_redact_001_secret_gone_after_reopen(tmp_path):
    data, rect = secret_doc("PUBLIC ", "TOPSECRET")
    doc = oxide_pdf.open(stream=data)
    page = doc[0]
    assert "TOPSECRET" in page.get_text()
    page.add_redact_annot(rect)
    applied = page.apply_redactions()
    assert applied == 1
    out = tmp_path / "redacted.pdf"
    doc.save(str(out))
    re = oxide_pdf.open(str(out))
    text = re[0].get_text()
    assert "TOPSECRET" not in text  # gone after reopen (the M4 exit gate)
    assert "PUBLIC" in text  # neighbouring text intact


def test_pym4_redact_002_no_annots_noop():
    doc = blank_doc()
    assert doc[0].apply_redactions() == 0


# --- PYM4-WIDGET-* : forms + Widget ----------------------------------------


def test_pym4_widget_001_list_fields():
    doc = oxide_pdf.open(stream=acroform_doc())
    assert doc.is_form_pdf
    page = doc[0]
    widgets = page.widgets()
    assert len(widgets) == 1
    w = widgets[0]
    assert w.field_name == "tx1"
    assert w.field_type_string == "Text"
    assert w.field_value == "init"
    assert isinstance(page.first_widget, oxide_pdf.Widget)


def test_pym4_widget_002_update_value_persists():
    doc = oxide_pdf.open(stream=acroform_doc())
    w = doc[0].widgets()[0]
    w.update("changed")
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.is_form_pdf
    assert re[0].widgets()[0].field_value == "changed"


# --- PYM4-EMBFILE-* / PYM4-SCRUB-* -----------------------------------------


def test_pym4_embfile_001_roundtrip():
    doc = blank_doc()
    doc.embfile_add("data.bin", b"\x00\x01payload\xff", filename="data.bin", desc="a blob")
    assert doc.embfile_names() == ["data.bin"]
    assert doc.embfile_count() == 1
    assert doc.embfile_get("data.bin") == b"\x00\x01payload\xff"
    info = doc.embfile_info("data.bin")
    assert info["filename"] == "data.bin"
    assert info["size"] == len(b"\x00\x01payload\xff")
    # persists across save/reopen
    re = oxide_pdf.open(stream=doc.tobytes())
    assert re.embfile_names() == ["data.bin"]
    assert re.embfile_get("data.bin") == b"\x00\x01payload\xff"


def test_pym4_scrub_001_removes_metadata():
    doc = blank_doc()
    doc.set_metadata({"title": "Confidential", "author": "Spy"})
    assert doc.metadata.get("title") == "Confidential"
    doc.scrub(metadata=True)
    md = doc.metadata
    assert not md.get("title")
    assert not md.get("author")


# --- PYM4-FITZ-* : deprecated-alias parity ---------------------------------


def test_pym4_fitz_001_camelcase_aliases(tmp_path):
    import fitz

    # annotations / drawings / insert / shape via camelCase
    doc = fitz.open(stream=blank_doc().tobytes())
    page = doc[0]
    page.insertText((72, 100), "ALIASED")
    a = page.addHighlightAnnot([_HL_QUAD])
    assert a.type[1] == "Highlight"
    assert page.firstAnnot is not None
    shape = page.newShape()
    shape.draw_rect((10, 10, 50, 50))
    shape.finish(color=(0, 0, 0))
    shape.commit()
    assert isinstance(page.getDrawings(), list)
    re = fitz.open(stream=doc.tobytes())
    assert "ALIASED" in re[0].get_text()

    # redaction via applyRedactions
    data, rect = secret_doc("KEEP ", "HIDDEN")
    rdoc = fitz.open(stream=data)
    rdoc[0].addRedactAnnot(rect)
    assert rdoc[0].applyRedactions() == 1
    reopened = fitz.open(stream=rdoc.tobytes())
    assert "HIDDEN" not in reopened[0].get_text()


def test_pym4_fitz_002_classes_exposed():
    import fitz

    assert fitz.Annot is oxide_pdf.Annot
    assert fitz.Widget is oxide_pdf.Widget
    assert fitz.Shape is oxide_pdf.Shape
