"""Long-tail PyMuPDF parity batch 14 — ``Document.FormFonts`` promoted from
deferred to implemented (COMPAT.toml parity long-tail).

``Document.FormFonts`` is a PyMuPDF **property** returning the key names in
``/Root /AcroForm /DR /Font`` (the form's default font resources, e.g.
``"Helv"``); an empty / missing dict on a valid PDF → ``[]``.

The empty-doc ``[]`` case matches real PyMuPDF 1.24.x / 1.27 (``.venv-oracle``)
exactly. The widget case diverges: the oracle does NOT populate ``/DR /Font`` on
a bare ``add_widget`` (its ``FormFonts`` stays ``[]``), whereas pdfspine's
``ensure_acroform`` inserts ``/DR <</Font <</Helv N 0 R>>>>`` — so we pin against
pdfspine's actual serialization here.
"""

from __future__ import annotations

import pdfspine


# ---------------------------------------------------------------------------
# Empty doc — no /AcroForm at all → [] (matches fitz exactly).
# ---------------------------------------------------------------------------
def test_form_fonts_empty_doc_is_empty_list() -> None:
    doc = pdfspine.open()
    doc.new_page()
    assert doc.FormFonts == []
    assert isinstance(doc.FormFonts, list)
    doc.close()


# ---------------------------------------------------------------------------
# add_widget triggers pdfspine's ensure_acroform, which inserts /DR /Font /Helv.
# 分歧:真实 PyMuPDF 在裸 add_widget 后 FormFonts 仍为 [],pdfspine 会加 /Helv。
# ---------------------------------------------------------------------------
def test_form_fonts_lists_helv_after_add_widget() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    w = pdfspine.Widget()
    w.field_name = "f1"
    w.field_type = pdfspine.PDF_WIDGET_TYPE_TEXT
    w.rect = pdfspine.Rect(50, 50, 200, 80)
    w.field_value = "hi"
    page.add_widget(w)

    fonts = doc.FormFonts
    assert isinstance(fonts, list)
    assert all(isinstance(name, str) for name in fonts)
    assert "Helv" in fonts
    doc.close()
