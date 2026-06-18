"""Long-tail PyMuPDF parity batch 8 — Widget appearance (PRD §C batch-3).

Covers the newly-implemented ``fitz.Widget`` appearance surface read from the
form-field annotation dict:
  - ``/MK``: border_color (BC), fill_color (BG), button_caption (CA).
  - ``/DA``: text_font, text_fontsize, text_color (parsed operators).
  - ``/BS``: border_style (S → full name), border_width (W, 0→1), border_dashes (D).
  - flags: field_display (/F → MuPDF display code), is_signed (signature signed?).
  - ``/Q`` text_format, ``/MaxLen`` text_maxlen, ``/AS`` on_state, /Parent rb_parent.
  - reset (revert /V to /DV) and next (page widget linked-list).

Every expected value below is the GROUND TRUTH captured from REAL PyMuPDF 1.27
(``.venv-oracle``); the in-repo ``.venv`` ``fitz`` is the pdfspine shim, so the
oracle values are embedded as literals here. The native ``pdfspine`` API reads
the identical fixtures and must reproduce these values.

DEVIATION: ``Widget.text_format`` — PyMuPDF 1.27's getter ALWAYS returns 0 (its
``pdf_text_widget_format`` never reads ``/Q`` from the dict, and its setter never
writes ``/Q``). pdfspine reads the spec-correct ``/Q`` value instead (so pdfspine is
strictly more correct here); the test asserts pdfspine's spec-correct read.
"""

from __future__ import annotations

import base64

import pdfspine
import pytest


# === fixtures (raw PDFs; values verified against real PyMuPDF 1.27) ==========

# A form built with real fitz: text field (DA + MK BC/BG + BS dashed + MaxLen +
# Q=1), checkbox (on-state Yes), pushbutton (MK CA caption). /F = 4 (Print) on
# all annotations, which MuPDF maps to field_display 0.
_FORM_PDF = b"""%PDF-1.7
1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R 5 0 R 6 0 R]>>>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 300]/Annots[4 0 R 5 0 R 6 0 R]>>endobj
4 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(txt1)/Rect[50 50 250 80]/F 4/V(hello)\
/DA(1 0 0 rg /Helv 12 Tf)/Q 1/MaxLen 30\
/MK<</BC[0 0 1]/BG[0.9 0.9 0.9]>>/BS<</S/D/W 2/D[3 2]>>>>endobj
5 0 obj<</Type/Annot/Subtype/Widget/FT/Btn/T(cb1)/Rect[50 100 70 120]/F 4/V/Yes\
/AS/Yes/AP<</N<</Yes 7 0 R/Off 7 0 R>>>>>>endobj
6 0 obj<</Type/Annot/Subtype/Widget/FT/Btn/Ff 65536/T(pb1)/Rect[50 130 150 160]/F 4\
/MK<</CA(Submit)>>>>endobj
7 0 obj<</Type/XObject/Subtype/Form/BBox[0 0 20 20]/Length 0>>stream
endstream endobj
trailer<</Root 1 0 R>>
%%EOF"""

# A radio group: parent field 4, two kid widgets (xref 5 on-state opt1, xref 6
# on-state opt2). rb_parent = 4 for both; on_state = the kid's non-Off /AP /N key.
_RADIO_PDF = b"""%PDF-1.7
1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 300]/Annots[5 0 R 6 0 R]>>endobj
4 0 obj<</FT/Btn/Ff 32768/T(radio)/V/opt1/Kids[5 0 R 6 0 R]>>endobj
5 0 obj<</Type/Annot/Subtype/Widget/Parent 4 0 R/Rect[50 50 70 70]/AS/opt1\
/AP<</N<</opt1 7 0 R/Off 7 0 R>>>>>>endobj
6 0 obj<</Type/Annot/Subtype/Widget/Parent 4 0 R/Rect[50 80 70 100]/AS/Off\
/AP<</N<</opt2 7 0 R/Off 7 0 R>>>>>>endobj
7 0 obj<</Type/XObject/Subtype/Form/BBox[0 0 20 20]/Length 0>>stream
endstream endobj
trailer<</Root 1 0 R>>
%%EOF"""

# Unsigned signature field: is_signed = False (signature field, but no /V).
_SIG_PDF = b"""%PDF-1.7
1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 200]/Annots[4 0 R]>>endobj
4 0 obj<</Type/Annot/Subtype/Widget/FT/Sig/T(sig1)/Rect[50 50 250 100]>>endobj
trailer<</Root 1 0 R>>
%%EOF"""

# Text field with /V (current) and /DV (default): reset reverts /V to /DV.
_RESET_PDF = b"""%PDF-1.7
1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 200]/Annots[4 0 R]>>endobj
4 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(t)/Rect[50 50 250 80]/V(current)\
/DV(default)/DA(/Helv 0 Tf 0 g)>>endobj
trailer<</Root 1 0 R>>
%%EOF"""

# Border-style codes → full names (the /BS /S mapping fitz uses).
_BS_PDF = b"""%PDF-1.7
1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R 5 0 R 6 0 R 8 0 R 9 0 R]>>>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 400]/Annots[4 0 R 5 0 R 6 0 R 8 0 R 9 0 R]>>endobj
4 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(s)/Rect[50 50 250 70]/BS<</S/S>>>>endobj
5 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(d)/Rect[50 80 250 100]/BS<</S/D>>>>endobj
6 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(b)/Rect[50 110 250 130]/BS<</S/B>>>>endobj
8 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(i)/Rect[50 140 250 160]/BS<</S/I>>>>endobj
9 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(u)/Rect[50 170 250 190]/BS<</S/U>>>>endobj
trailer<</Root 1 0 R>>
%%EOF"""

# Display-flag mapping: /F → field_display (MuPDF pdf_field_display).
_DISPLAY_PDF = b"""%PDF-1.7
1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R 5 0 R 6 0 R 8 0 R]>>>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 400]/Annots[4 0 R 5 0 R 6 0 R 8 0 R]>>endobj
4 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(f0)/Rect[50 50 250 70]/F 0>>endobj
5 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(f4)/Rect[50 80 250 100]/F 4>>endobj
6 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(f2)/Rect[50 110 250 130]/F 2>>endobj
8 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(f32)/Rect[50 140 250 160]/F 32>>endobj
trailer<</Root 1 0 R>>
%%EOF"""


def _write(tmp_path, name: str, data: bytes) -> str:
    p = tmp_path / name
    p.write_bytes(data)
    return str(p)


def _widgets_by_name(path: str) -> dict[str, object]:
    doc = pdfspine.open(path)
    return {w.field_name: w for w in doc[0].widgets()}


def _approx(a, b, tol: float = 1e-4) -> bool:
    return len(a) == len(b) and all(abs(x - y) < tol for x, y in zip(a, b))


# === MK colors + captions ====================================================

def test_border_and_fill_color(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert _approx(w["txt1"].border_color, [0.0, 0.0, 1.0])
    assert _approx(w["txt1"].fill_color, [0.9, 0.9, 0.9])
    # Absent on the checkbox / pushbutton.
    assert w["cb1"].border_color is None
    assert w["cb1"].fill_color is None


def test_button_caption(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["pb1"].button_caption == "Submit"
    assert w["txt1"].button_caption is None
    assert w["cb1"].button_caption is None


# === DA: font / size / color =================================================

def test_text_da_properties(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["txt1"].text_font == "Helv"
    assert w["txt1"].text_fontsize == 12.0
    assert _approx(w["txt1"].text_color, [1.0, 0.0, 0.0])


def test_text_da_defaults_when_no_da(tmp_path):
    # The checkbox has no /DA → fitz defaults: Helv, 0, black.
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["cb1"].text_font == "Helv"
    assert w["cb1"].text_fontsize == 0.0
    assert _approx(w["cb1"].text_color, [0.0, 0.0, 0.0])


# === BS: style / width / dashes ==============================================

def test_border_style_width_dashes(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["txt1"].border_style == "Dashed"
    assert w["txt1"].border_width == 2.0
    assert w["txt1"].border_dashes == [3, 2]


def test_border_style_full_names(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "bs.pdf", _BS_PDF))
    assert w["s"].border_style == "Solid"
    assert w["d"].border_style == "Dashed"
    assert w["b"].border_style == "Beveled"
    assert w["i"].border_style == "Inset"
    assert w["u"].border_style == "Underline"


def test_border_width_defaults_to_one(tmp_path):
    # No /BS → width defaults to 1 (matches fitz).
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["cb1"].border_width == 1.0
    assert w["cb1"].border_style == "Solid"
    assert w["cb1"].border_dashes is None


# === Q / MaxLen ==============================================================

def test_text_maxlen(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["txt1"].text_maxlen == 30
    assert w["cb1"].text_maxlen == 0


def test_text_format_reads_quadding(tmp_path):
    # DEVIATION: fitz 1.27 always returns 0; pdfspine reads the spec-correct /Q.
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["txt1"].text_format == 1
    assert w["cb1"].text_format == 0


# === field_display (/F mapping) ==============================================

def test_field_display_mapping(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "fd.pdf", _DISPLAY_PDF))
    assert w["f0"].field_display == 2   # Print clear → no-print
    assert w["f4"].field_display == 0   # Print set, visible → normal
    assert w["f2"].field_display == 1   # Hidden
    assert w["f32"].field_display == 1  # NoView


def test_field_display_print_form(tmp_path):
    # The /F 4 fixture form: all visible+printing → 0.
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["txt1"].field_display == 0
    assert w["cb1"].field_display == 0


# === is_signed ===============================================================

def test_is_signed_unsigned_signature(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "sig.pdf", _SIG_PDF))
    sig = w["sig1"]
    assert sig.field_type_string == "Signature"
    assert sig.is_signed is False


def test_is_signed_none_for_non_signature(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["txt1"].is_signed is None
    assert w["cb1"].is_signed is None


# === on_state ================================================================

def test_on_state_checkbox(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["cb1"].on_state() == "Yes"
    # Non-button fields: None.
    assert w["txt1"].on_state() is None


def test_on_state_and_rb_parent_radio(tmp_path):
    doc = pdfspine.open(_write(tmp_path, "radio.pdf", _RADIO_PDF))
    ws = doc[0].widgets()
    assert [wi.field_type_string for wi in ws] == ["RadioButton", "RadioButton"]
    assert ws[0].on_state() == "opt1"
    assert ws[1].on_state() == "opt2"
    # Both kids share the same /Parent field xref.
    assert ws[0].rb_parent == ws[1].rb_parent
    assert ws[0].rb_parent == 4


def test_rb_parent_none_for_non_radio(tmp_path):
    w = _widgets_by_name(_write(tmp_path, "f.pdf", _FORM_PDF))
    assert w["txt1"].rb_parent is None
    assert w["cb1"].rb_parent is None


# === reset ===================================================================

def test_reset_reverts_to_default(tmp_path):
    path = _write(tmp_path, "reset.pdf", _RESET_PDF)
    doc = pdfspine.open(path)
    wi = doc[0].widgets()[0]
    assert wi.field_value == "current"
    wi.reset()
    assert doc[0].widgets()[0].field_value == "default"


# === next (page widget linked list) ==========================================

def test_next_links_page_widgets(tmp_path):
    doc = pdfspine.open(_write(tmp_path, "f.pdf", _FORM_PDF))
    page = doc[0]
    ws = page.widgets()
    assert [wi.field_name for wi in ws] == ["txt1", "cb1", "pb1"]
    assert ws[0].next.field_name == "cb1"
    assert ws[1].next.field_name == "pb1"
    assert ws[2].next is None


def test_first_widget_next_chain(tmp_path):
    doc = pdfspine.open(_write(tmp_path, "f.pdf", _FORM_PDF))
    first = doc[0].first_widget
    assert first is not None
    assert first.field_name == "txt1"
    assert first.next.field_name == "cb1"
    assert first.next.next.field_name == "pb1"
    assert first.next.next.next is None


# === Annot members (PRD §C batch-3) ==========================================
#
# Fixtures BELOW were built with REAL PyMuPDF 1.27 (.venv-oracle) and embedded as
# base64. The native pdfspine reads the identical bytes and must reproduce the
# oracle values captured alongside each fixture.
#
# DEVIATION: PyMuPDF's ``Annot.language`` getter leaks the *system locale*
# (e.g. ``"zh-Hans"``) for annotations that carry NO ``/Lang`` key; pdfspine returns
# the spec-correct empty string. The tests assert pdfspine's value where /Lang is
# absent and the real oracle value (``"en"``) where /Lang is present.

# fixture1: a Text note (xref 5) with /Lang(en) + /Popup, a Square (xref 8) with
# /RD[1 1 1 1], and a reply Text (xref 10) whose /IRT -> xref 5.
_ANNOT_PDF_B64 = (
    "JVBERi0xLjcKJcK1wrYKJSBXcml0dGVuIGJ5IE11UERGIDEuMjcuMgoKMSAwIG9iago8PC9UeXBl"
    "L0NhdGFsb2cvUGFnZXMgMiAwIFIvSW5mbzw8L1Byb2R1Y2VyKE11UERGIDEuMjcuMik+Pj4+CmVu"
    "ZG9iagoKMiAwIG9iago8PC9UeXBlL1BhZ2VzL0NvdW50IDEvS2lkc1s0IDAgUl0+PgplbmRvYmoK"
    "CjMgMCBvYmoKPDw+PgplbmRvYmoKCjQgMCBvYmoKPDwvVHlwZS9QYWdlL01lZGlhQm94WzAgMCAz"
    "MDAgNDAwXS9Sb3RhdGUgMC9SZXNvdXJjZXMgMyAwIFIvUGFyZW50IDIgMCBSL0Fubm90c1s1IDAg"
    "UiA2IDAgUiA4IDAgUiAxMCAwIFIgMTEgMCBSXT4+CmVuZG9iagoKNSAwIG9iago8PC9UeXBlL0Fu"
    "bm90L1N1YnR5cGUvVGV4dC9SZWN0WzIwIDM2NCAzNiAzODBdL0NbMSAxIDBdL1BvcHVwIDYgMCBS"
    "L1AgNCAwIFIvRiAyOC9Db250ZW50cyhwYXJlbnQgbm90ZSkvTmFtZS9Ob3RlL0FQPDwvTiA3IDAg"
    "Uj4+L05NKGZpdHotQTApL0xhbmcoZW4pPj4KZW5kb2JqCgo2IDAgb2JqCjw8L1R5cGUvQW5ub3Qv"
    "U3VidHlwZS9Qb3B1cC9QYXJlbnQgNSAwIFIvUmVjdFs0MCAyMjAgMTgwIDM2MF0+PgplbmRvYmoK"
    "CjcgMCBvYmoKPDwvVHlwZS9YT2JqZWN0L1N1YnR5cGUvRm9ybS9CQm94WzAgMCAxNiAxNl0vTWF0"
    "cml4WzEgMCAwIDEgMCAwXS9MZW5ndGggOTk+PgpzdHJlYW0KMSAxIDAgcmcKMSB3CjAuNSAwLjUg"
    "MTUgMTUgcmUKYgoxIDAgMCAtMSA0IDEyIGNtCjAgZwowIDAgOCAxIHJlCjAgMiA4IDEgcmUKMCA0"
    "IDggMSByZQowIDYgOCAxIHJlCmYKCmVuZHN0cmVhbQplbmRvYmoKCjggMCBvYmoKPDwvVHlwZS9B"
    "bm5vdC9TdWJ0eXBlL1NxdWFyZS9SZWN0Wzk5IDE5OSAyMDEgMzAxXS9SRFsxIDEgMSAxXS9CUzw8"
    "L1R5cGUvQm9yZGVyL1cgMT4+L0NbMSAwIDBdL1AgNCAwIFIvRiA0L0FQPDwvTiA5IDAgUj4+L05N"
    "KGZpdHotQTEpPj4KZW5kb2JqCgo5IDAgb2JqCjw8L1R5cGUvWE9iamVjdC9TdWJ0eXBlL0Zvcm0v"
    "QkJveFs5OSAxOTkgMjAxIDMwMV0vTWF0cml4WzEgMCAwIDEgMCAwXS9MZW5ndGggMzQ+PgpzdHJl"
    "YW0KMSB3CjEgMCAwIFJHCjEwMCAyMDAgMTAwIDEwMCByZQpTCgplbmRzdHJlYW0KZW5kb2JqCgox"
    "MCAwIG9iago8PC9UeXBlL0Fubm90L1N1YnR5cGUvVGV4dC9SZWN0WzYwIDM2NCA3NiAzODBdL0Nb"
    "MSAxIDBdL1BvcHVwIDExIDAgUi9QIDQgMCBSL0YgMjgvQ29udGVudHMoYSByZXBseSkvTmFtZS9O"
    "b3RlL0FQPDwvTiAxMiAwIFI+Pi9OTShmaXR6LUEyKS9JUlQgNSAwIFI+PgplbmRvYmoKCjExIDAg"
    "b2JqCjw8L1R5cGUvQW5ub3QvU3VidHlwZS9Qb3B1cC9QYXJlbnQgMTAgMCBSL1JlY3RbMzIgMjg4"
    "IDIzMiAzODhdPj4KZW5kb2JqCgoxMiAwIG9iago8PC9UeXBlL1hPYmplY3QvU3VidHlwZS9Gb3Jt"
    "L0JCb3hbMCAwIDE2IDE2XS9NYXRyaXhbMSAwIDAgMSAwIDBdL0xlbmd0aCA5OT4+CnN0cmVhbQox"
    "IDEgMCByZwoxIHcKMC41IDAuNSAxNSAxNSByZQpiCjEgMCAwIC0xIDQgMTIgY20KMCBnCjAgMCA4"
    "IDEgcmUKMCAyIDggMSByZQowIDQgOCAxIHJlCjAgNiA4IDEgcmUKZgoKZW5kc3RyZWFtCmVuZG9i"
    "agoKeHJlZgowIDEzCjAwMDAwMDAwMDAgNjU1MzUgZiAKMDAwMDAwMDA0MiAwMDAwMCBuIAowMDAw"
    "MDAwMTIwIDAwMDAwIG4gCjAwMDAwMDAxNzIgMDAwMDAgbiAKMDAwMDAwMDE5MyAwMDAwMCBuIAow"
    "MDAwMDAwMzI0IDAwMDAwIG4gCjAwMDAwMDA0OTEgMDAwMDAgbiAKMDAwMDAwMDU3MSAwMDAwMCBu"
    "IAowMDAwMDAwNzgxIDAwMDAwIG4gCjAwMDAwMDA5MzIgMDAwMDAgbiAKMDAwMDAwMTA4MiAwMDAw"
    "MCBuIAowMDAwMDAxMjQ5IDAwMDAwIG4gCjAwMDAwMDEzMzEgMDAwMDAgbiAKCnRyYWlsZXIKPDwv"
    "U2l6ZSAxMy9Sb290IDEgMCBSL0lEWzwzMDVBQzI5Q0MyQkJDMkIyMkMxQjc4QzJCQ0MyOEUxQT48"
    "QzU5QkRBQkZCNDczNUJDN0JDRDdBQ0JFMDUyM0ZEQTg+XT4+CnN0YXJ0eHJlZgoxNTQyCiUlRU9G"
    "Cg=="
)

# fixture2: a FileAttachment (xref 5) embedding b"hello file content" as data.txt
# with /Desc(my file).
_FILE_PDF_B64 = (
    "JVBERi0xLjcKJcK1wrYKJSBXcml0dGVuIGJ5IE11UERGIDEuMjcuMgoKMSAwIG9iago8PC9UeXBl"
    "L0NhdGFsb2cvUGFnZXMgMiAwIFIvSW5mbzw8L1Byb2R1Y2VyKE11UERGIDEuMjcuMik+Pj4+CmVu"
    "ZG9iagoKMiAwIG9iago8PC9UeXBlL1BhZ2VzL0NvdW50IDEvS2lkc1s0IDAgUl0+PgplbmRvYmoK"
    "CjMgMCBvYmoKPDw+PgplbmRvYmoKCjQgMCBvYmoKPDwvVHlwZS9QYWdlL01lZGlhQm94WzAgMCA1"
    "OTUgODQyXS9Sb3RhdGUgMC9SZXNvdXJjZXMgMyAwIFIvUGFyZW50IDIgMCBSL0Fubm90c1s1IDAg"
    "UiA2IDAgUl0+PgplbmRvYmoKCjUgMCBvYmoKPDwvVHlwZS9Bbm5vdC9TdWJ0eXBlL0ZpbGVBdHRh"
    "Y2htZW50L1JlY3RbMTAwIDEyNiAxMTYgMTQyXS9DWzEgMSAwXS9Qb3B1cCA2IDAgUi9QIDQgMCBS"
    "L0YgNC9GUzw8L1R5cGUvRmlsZXNwZWMvQ0k8PD4+L0VGPDwvRiA3IDAgUj4+L0YoZGF0YS50eHQp"
    "L1VGKGRhdGEudHh0KS9EZXNjKG15IGZpbGUpPj4vQ29udGVudHMoZGF0YS50eHQpL0FQPDwvTiA4"
    "IDAgUj4+L05NKGZpdHotQTApPj4KZW5kb2JqCgo2IDAgb2JqCjw8L1R5cGUvQW5ub3QvU3VidHlw"
    "ZS9Qb3B1cC9QYXJlbnQgNSAwIFIvUmVjdFszMiA3MzAgMjMyIDgzMF0+PgplbmRvYmoKCjcgMCBv"
    "YmoKPDwvTGVuZ3RoIDE4L0RMIDE4L1BhcmFtczw8L1NpemUgMTg+Pj4+CnN0cmVhbQpoZWxsbyBm"
    "aWxlIGNvbnRlbnQKZW5kc3RyZWFtCmVuZG9iagoKOCAwIG9iago8PC9UeXBlL1hPYmplY3QvU3Vi"
    "dHlwZS9Gb3JtL0JCb3hbMCAwIDE2IDE2XS9NYXRyaXhbMSAwIDAgMSAwIDBdL0xlbmd0aCA0ODk+"
    "PgpzdHJlYW0KMSAxIDAgcmcKMSB3CjAuNSAwLjUgMTUgMTUgcmUKYgoxIDAgMCAtMSA0IDEyIGNt"
    "CjAgZwoxLjM0IDAgbQouOTIgLjA0IC43NiAuNjQgMS4xIC44OSBjCjEuMzQgMS4wOCAxLjY1IC45"
    "NyAxLjkzIDEgYwoyLjA4IC45OCAxLjk2IDEuMjIgMiAxLjMyIGMKMiAxLjg4IDIgMi40NCAyIDMg"
    "YwoxLjYgMy4wMSAxLjIgMi45OCAuOCAzLjAyIGMKLjM1IDMuMTEgLS4wMSAzLjU0IDAgNCBjCjEg"
    "NCAyIDQgMyA0IGMKMyA1IDMgNiAzIDcgYwozLjE0NiA3LjMzIDMuMjkgNy42NyAzLjQ0IDggYwoz"
    "LjYyIDcuNjYgMy44MyA3LjMyIDQgNi45OCBjCjQgNS45OSA0IDQuOTkgNCA0IGMKNSA0IDYgNCA3"
    "IDQgYwo3LjAyIDMuNDIgNi40NiAyLjk0IDUuODkgMyBjCjUuNiAzIDUuMyAzIDUgMyBjCjUgMi4z"
    "MyA1IDEuNjcgNSAxIGMKNS4zMCAuOTggNS42NyAxLjA5IDUuODkgLjgxIGMKNi4xNiAuNSA1Ljg5"
    "IC0uMDM4IDUuNDggMCBjCjQuMTUgMCAyLjgzIDAgMS41IDAgYwpoCmYKCmVuZHN0cmVhbQplbmRv"
    "YmoKCnhyZWYKMCA5CjAwMDAwMDAwMDAgNjU1MzUgZiAKMDAwMDAwMDA0MiAwMDAwMCBuIAowMDAw"
    "MDAwMTIwIDAwMDAwIG4gCjAwMDAwMDAxNzIgMDAwMDAgbiAKMDAwMDAwMDE5MyAwMDAwMCBuIAow"
    "MDAwMDAwMzA0IDAwMDAwIG4gCjAwMDAwMDA1NDIgMDAwMDAgbiAKMDAwMDAwMDYyMiAwMDAwMCBu"
    "IAowMDAwMDAwNzE0IDAwMDAwIG4gCgp0cmFpbGVyCjw8L1NpemUgOS9Sb290IDEgMCBSL0lEWzwz"
    "OEMzODJDM0E0NjMzQTE0QzNBM0MyQTBDMzg0QzNCNj48Q0QwMzIyNjczQzVCMjNFRkM0M0ZGNDY2"
    "OEQwQkM1RDA+XT4+CnN0YXJ0eHJlZgoxMzE1CiUlRU9GCg=="
)


def _annot_doc():
    return pdfspine.open(stream=base64.b64decode(_ANNOT_PDF_B64), filetype="pdf")


def _by_xref(page) -> dict[int, object]:
    return {a.xref: a for a in page.annots()}


def test_annot_rect_delta():
    # Square xref 8 has /RD[1 1 1 1] -> (1,1,-1,-1); the Text notes have no /RD.
    page = _annot_doc()[0]
    a = _by_xref(page)
    assert a[8].rect_delta == (1.0, 1.0, -1.0, -1.0)
    assert a[5].rect_delta is None
    assert a[10].rect_delta is None


def test_annot_popup():
    page = _annot_doc()[0]
    a = _by_xref(page)
    # Text note xref 5 has a popup; Square xref 8 does not.
    assert a[5].has_popup is True
    assert a[5].popup_xref == 6
    assert _approx(tuple(a[5].popup_rect), (40.0, 40.0, 180.0, 180.0))
    assert a[8].has_popup is False
    assert a[8].popup_xref == 0
    # Absent popup -> infinite rect (PyMuPDF semantics).
    pr = tuple(a[8].popup_rect)
    assert pr[0] == pr[1] == -2147483648.0


def test_annot_apn_bbox_matrix():
    page = _annot_doc()[0]
    a = _by_xref(page)
    # BBox [0 0 16 16] on a 400-tall page -> page space (0,384,16,400).
    assert _approx(tuple(a[5].apn_bbox()), (0.0, 384.0, 16.0, 400.0))
    # Square's AP BBox is already [99 199 201 301] -> page space (99,99,201,201).
    assert _approx(tuple(a[8].apn_bbox()), (99.0, 99.0, 201.0, 201.0))
    assert _approx(tuple(a[5].apn_matrix()), (1.0, 0.0, 0.0, 1.0, 0.0, 0.0))


def test_annot_language():
    page = _annot_doc()[0]
    a = _by_xref(page)
    # xref 5 carries /Lang(en); the others carry none (pdfspine: spec-correct "").
    assert a[5].language == "en"
    assert a[8].language == ""
    assert a[10].language == ""


def test_annot_irt_xref():
    page = _annot_doc()[0]
    a = _by_xref(page)
    assert a[10].irt_xref == 5
    assert a[5].irt_xref == 0


def test_annot_set_rotation_roundtrip():
    page = _annot_doc()[0]
    note = _by_xref(page)[5]
    note.set_rotation(90)
    assert page.parent.xref_get_key(5, "Rotate") == "90"
    # fitz normalizes via modulo into [0, 360): -1 -> 359 (NOT key removal).
    note.set_rotation(-1)
    assert page.parent.xref_get_key(5, "Rotate") == "359"
    note.set_rotation(450)
    assert page.parent.xref_get_key(5, "Rotate") == "90"


def test_annot_set_popup_roundtrip():
    page = _annot_doc()[0]
    sq = _by_xref(page)[8]
    assert sq.has_popup is False
    sq.set_popup(pdfspine.Rect(50, 60, 150, 160))
    # Re-read.
    sq2 = _by_xref(page)[8]
    assert sq2.has_popup is True
    assert _approx(tuple(sq2.popup_rect), (50.0, 60.0, 150.0, 160.0))


def test_annot_set_language_roundtrip():
    page = _annot_doc()[0]
    sq = _by_xref(page)[8]
    sq.set_language("de")
    assert _by_xref(page)[8].language == "de"
    sq.set_language("")
    assert _by_xref(page)[8].language == ""


def test_annot_set_irt_xref_roundtrip():
    page = _annot_doc()[0]
    a = _by_xref(page)
    a[8].set_irt_xref(5)
    assert _by_xref(page)[8].irt_xref == 5


def test_annot_set_apn_bbox_matrix_roundtrip():
    page = _annot_doc()[0]
    sq = _by_xref(page)[8]
    # set_apn_bbox takes page space; apn_bbox reads it back in page space, so
    # the round-trip is involutive.
    sq.set_apn_bbox(pdfspine.Rect(0, 0, 50, 50))
    sq.set_apn_matrix(pdfspine.Matrix(2, 0, 0, 2, 0, 0))
    sq2 = _by_xref(page)[8]
    assert _approx(tuple(sq2.apn_bbox()), (0.0, 0.0, 50.0, 50.0))
    assert _approx(tuple(sq2.apn_matrix()), (2.0, 0.0, 0.0, 2.0, 0.0, 0.0))


def test_annot_delete_responses():
    page = _annot_doc()[0]
    before = sum(1 for _ in page.annots())
    note = _by_xref(page)[5]
    note.delete_responses()  # removes reply xref 10 (+ its popup xref 11)
    xrefs = {a.xref for a in page.annots()}
    assert 10 not in xrefs
    assert 5 in xrefs
    assert sum(1 for _ in page.annots()) < before


def test_annot_clean_contents():
    page = _annot_doc()[0]
    sq = _by_xref(page)[8]
    sq.clean_contents()  # must not raise; AP/N stays renderable
    assert sq.has_appearance is True


def test_annot_get_textbox():
    # Annot.get_textbox is deferred: fitz reads the annot's OWN appearance
    # textpage (requires a rect arg), semantically unlike Page.get_textbox; we
    # do not ship the page-delegating approximation. Page.get_textbox is the
    # supported surface.
    page = _annot_doc()[0]
    note = _by_xref(page)[5]
    with pytest.raises(pdfspine.PdfUnsupportedError):
        note.get_textbox((0, 0, 100, 100))


def test_annot_file_get_info():
    doc = pdfspine.open(stream=base64.b64decode(_FILE_PDF_B64), filetype="pdf")
    fa = [a for a in doc[0].annots() if a.type[1] == "FileAttachment"][0]
    assert fa.get_file() == b"hello file content"
    info = fa.file_info()
    # fitz's exact key set: filename / description / length / size.
    assert info["filename"] == "data.txt"
    assert info["description"] == "my file"
    assert info["length"] == 18
    assert info["size"] == 18


def test_annot_update_file():
    doc = pdfspine.open(stream=base64.b64decode(_FILE_PDF_B64), filetype="pdf")
    fa = [a for a in doc[0].annots() if a.type[1] == "FileAttachment"][0]
    # fitz's first param is `buffer_` (not `buffer`).
    fa.update_file(buffer_=b"replaced bytes!", filename="new.txt", desc="updated")
    assert fa.get_file() == b"replaced bytes!"
    info = fa.file_info()
    assert info["filename"] == "new.txt"
    assert info["description"] == "updated"
    assert info["length"] == len(b"replaced bytes!")
