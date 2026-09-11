"""Annotation appearance text, independent of underlying page text."""

from __future__ import annotations

import pdfspine
import pytest


def annotation_page():
    doc = pdfspine.open()
    page = doc.new_page(width=300, height=200)
    page.insert_text((45, 65), "PARENT PAGE")
    annot = page.add_rect_annot(pdfspine.Rect(40, 40, 180, 90))
    font = doc.get_new_xref()
    doc.update_object(font, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    ap = doc.get_new_xref()
    doc.update_object(
        ap,
        f"<< /Type /XObject /Subtype /Form /BBox [0 0 140 50] /Resources << /Font << /F9 {font} 0 R >> >> >>",
    )
    doc.update_stream(ap, b"BT /F9 12 Tf 10 25 Td (OWN AP) Tj ET")
    doc.xref_set_key(annot.xref, "AP", f"<< /N {ap} 0 R >>")
    return doc, page, annot, ap


def test_annotation_textbox_reads_own_appearance_not_parent_region():
    doc, page, annot, _ = annotation_page()
    assert "PARENT PAGE" in page.get_text()
    assert annot.get_textbox(page.rect) == "OWN AP"
    assert annot.get_textbox(pdfspine.Rect(0, 0, 10, 10)) == ""
    with pytest.raises(TypeError):
        annot.get_textbox()
    doc.close()


@pytest.mark.parametrize("flags", [1, 2, 32, 3])
def test_hidden_annotation_has_no_appearance_text(flags):
    doc, page, annot, _ = annotation_page()
    doc.xref_set_key(annot.xref, "F", str(flags))
    assert annot.get_textbox(page.rect) == ""
    doc.close()


def test_missing_appearance_does_not_fall_back_to_contents():
    doc, page, annot, _ = annotation_page()
    doc.xref_set_key(annot.xref, "AP", "null")
    doc.xref_set_key(annot.xref, "Contents", "(COMMENT ONLY)")
    assert annot.get_textbox(page.rect) == ""
    doc.close()


def test_state_dictionary_selects_only_matching_appearance():
    doc, page, annot, ap = annotation_page()
    doc.xref_set_key(annot.xref, "AP", f"<< /N << /On {ap} 0 R >> >>")
    doc.xref_set_key(annot.xref, "AS", "/On")
    assert annot.get_textbox(page.rect) == "OWN AP"
    doc.xref_set_key(annot.xref, "AS", "/Missing")
    assert annot.get_textbox(page.rect) == ""
    doc.close()


def test_prebuilt_page_textpage_is_not_annotation_owned():
    doc, page, annot, _ = annotation_page()
    for target in [page.get_textpage(), annot.get_textpage()]:
        with pytest.raises(ValueError):
            annot.get_textbox(page.rect, textpage=target)
    doc.close()


def test_appearance_bbox_does_not_clip_semantic_text():
    doc, page, annot, ap = annotation_page()
    doc.update_stream(ap, b"BT /F9 12 Tf 10 25 Td (INSIDE) Tj 150 0 Td (OUTSIDE) Tj ET")
    assert annot.get_textbox(page.rect) == "INSIDE\nOUTSIDE"
    doc.close()


@pytest.mark.parametrize("rotation", [0, 90, 180, 270])
def test_query_uses_rotated_page_coordinates(rotation):
    doc, page, annot, _ = annotation_page()
    page.set_rotation(rotation)
    # Original AP text starts at (50, 65); rotate that page-space point.
    bounds = pdfspine.Rect(49, 50, 110, 75) * page.rotation_matrix
    assert annot.get_textbox(bounds) == "OWN AP"
    assert annot.get_textbox(pdfspine.Rect(0, 0, 5, 5)) == ""
    doc.close()


def test_other_annotation_appearance_is_not_included():
    doc, page, annot, ap = annotation_page()
    other = page.add_rect_annot(pdfspine.Rect(40, 40, 180, 90))
    second_ap = doc.get_new_xref()
    doc.update_object(second_ap, doc.xref_object(ap))
    doc.update_stream(second_ap, b"BT /F9 12 Tf 10 25 Td (OTHER) Tj ET")
    doc.xref_set_key(other.xref, "AP", f"<< /N {second_ap} 0 R >>")
    assert annot.get_textbox(page.rect) == "OWN AP"
    assert other.get_textbox(page.rect) == "OTHER"
    doc.close()


def test_appearance_matrix_and_nonzero_bbox_use_local_resources():
    doc, page, annot, ap = annotation_page()
    doc.xref_set_key(ap, "BBox", "[10 20 110 70]")
    doc.xref_set_key(ap, "Matrix", "[0 1 -1 0 80 -10]")
    doc.update_stream(ap, b"BT /F9 12 Tf 20 35 Td (CUSTOM) Tj ET")
    # PyMuPDF 1.28.2: bbox (101.88, 59.002, 148.0464, 85).
    assert annot.get_textbox(pdfspine.Rect(100, 58, 150, 86)) == "CUSTOM"
    assert annot.get_textbox(pdfspine.Rect(40, 40, 80, 90)) == ""
    doc.close()


def test_cropbox_offset_and_partial_character_overlap():
    doc, page, annot, _ = annotation_page()
    doc.xref_set_key(page.xref, "CropBox", "[20 10 280 190]")
    assert annot.get_textbox(pdfspine.Rect(29, 40, 80, 60)) == "OWN AP"
    # A thin slice inside the first O's bbox retains that character.
    assert annot.get_textbox(pdfspine.Rect(30.1, 45, 30.2, 46)) == "O"
    assert annot.get_textbox(pdfspine.Rect(0, 0, 20, 20)) == ""
    doc.close()


def test_own_appearance_text_ignores_explicit_clip():
    doc, page, annot, ap = annotation_page()
    doc.update_stream(
        ap,
        b"0 0 70 50 re W n BT /F9 12 Tf 10 25 Td (INSIDE) Tj 150 0 Td (OUTSIDE) Tj ET",
    )
    assert annot.get_textbox(page.rect) == "INSIDE\nOUTSIDE"
    doc.xref_set_key(page.xref, "Contents", "null")
    replay_text = page.get_displaylist().get_textpage().extractText()
    assert "INSIDE" in replay_text
    assert "OUTSIDE" not in replay_text
    doc.close()


def test_own_appearance_text_ignores_nested_form_bbox():
    doc, page, annot, ap = annotation_page()
    nested = doc.get_new_xref()
    doc.update_object(nested, doc.xref_object(ap))
    doc.update_stream(
        nested, b"BT /F9 12 Tf 10 25 Td (INSIDE) Tj 150 0 Td (OUTSIDE) Tj ET"
    )
    doc.xref_set_key(ap, "Resources", f"<< /XObject << /Nested {nested} 0 R >> >>")
    doc.update_stream(ap, b"/Nested Do")
    assert annot.get_textbox(page.rect) == "INSIDE\nOUTSIDE"
    doc.xref_set_key(page.xref, "Contents", "null")
    replay_text = page.get_displaylist().get_textpage().extractText()
    assert "INSIDE" in replay_text
    assert "OUTSIDE" not in replay_text
    doc.close()


@pytest.mark.parametrize("local_resources", [False, True])
def test_inherited_page_resources_and_local_appearance_precedence(local_resources):
    doc, page, annot, ap = annotation_page()
    parent = int(doc.xref_get_key(page.xref, "Parent").split()[0])
    inherited_font = doc.get_new_xref()
    doc.update_object(
        inherited_font,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding /Differences [79 /Z] >> >>",
    )
    doc.xref_set_key(parent, "Resources", f"<< /Font << /F9 {inherited_font} 0 R >> >>")
    doc.xref_set_key(page.xref, "Resources", "null")
    if not local_resources:
        doc.xref_set_key(ap, "Resources", "null")
    assert annot.get_textbox(page.rect) == ("OWN AP" if local_resources else "ZWN AP")
    doc.close()
