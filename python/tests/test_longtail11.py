"""Long-tail PyMuPDF parity batch 11 — module-level CONSTANTS (PRD §C, Task 1).

Implements every ``fitz.*`` module-level constant family with the EXACT value
PyMuPDF 1.27 uses. The expected values below were captured from a real PyMuPDF
1.27 install (``import fitz``); they double as the regression baseline because
the real package is not present at CI time.

Each family is asserted both as raw values (parity vs the documented fitz value)
and re-exported through the ``pdfspine`` top level + the ``fitz`` shim.
"""

import pdfspine
import pdfspine.constants as C

import fitz


# (name, expected fitz 1.27 value) — full enumeration of every family.
_EXPECTED: dict[str, object] = {
    # --- TEXT_* extraction flags (bitmask) ---
    "TEXT_PRESERVE_LIGATURES": 1,
    "TEXT_PRESERVE_WHITESPACE": 2,
    "TEXT_PRESERVE_IMAGES": 4,
    "TEXT_INHIBIT_SPACES": 8,
    "TEXT_DEHYPHENATE": 16,
    "TEXT_PRESERVE_SPANS": 32,
    "TEXT_MEDIABOX_CLIP": 64,
    "TEXT_CID_FOR_UNKNOWN_UNICODE": 128,
    "TEXT_COLLECT_STRUCTURE": 256,
    "TEXT_ACCURATE_BBOXES": 512,
    "TEXT_COLLECT_VECTORS": 1024,
    "TEXT_IGNORE_ACTUALTEXT": 2048,
    # --- TEXTFLAGS_* pre-combined bundles ---
    "TEXTFLAGS_TEXT": 195,
    "TEXTFLAGS_WORDS": 195,
    "TEXTFLAGS_BLOCKS": 195,
    "TEXTFLAGS_DICT": 199,
    "TEXTFLAGS_RAWDICT": 199,
    "TEXTFLAGS_HTML": 199,
    "TEXTFLAGS_XHTML": 199,
    "TEXTFLAGS_XML": 195,
    "TEXTFLAGS_SEARCH": 210,
    # --- TEXT_FONT_* span flags ---
    "TEXT_FONT_SUPERSCRIPT": 1,
    "TEXT_FONT_ITALIC": 2,
    "TEXT_FONT_SERIFED": 4,
    "TEXT_FONT_MONOSPACED": 8,
    "TEXT_FONT_BOLD": 16,
    # --- TEXT_ALIGN_* ---
    "TEXT_ALIGN_LEFT": 0,
    "TEXT_ALIGN_CENTER": 1,
    "TEXT_ALIGN_RIGHT": 2,
    "TEXT_ALIGN_JUSTIFY": 3,
    # --- PDF_ANNOT_* types ---
    "PDF_ANNOT_TEXT": 0,
    "PDF_ANNOT_LINK": 1,
    "PDF_ANNOT_FREE_TEXT": 2,
    "PDF_ANNOT_LINE": 3,
    "PDF_ANNOT_SQUARE": 4,
    "PDF_ANNOT_CIRCLE": 5,
    "PDF_ANNOT_POLYGON": 6,
    "PDF_ANNOT_POLY_LINE": 7,
    "PDF_ANNOT_HIGHLIGHT": 8,
    "PDF_ANNOT_UNDERLINE": 9,
    "PDF_ANNOT_SQUIGGLY": 10,
    "PDF_ANNOT_STRIKE_OUT": 11,
    "PDF_ANNOT_REDACT": 12,
    "PDF_ANNOT_STAMP": 13,
    "PDF_ANNOT_CARET": 14,
    "PDF_ANNOT_INK": 15,
    "PDF_ANNOT_POPUP": 16,
    "PDF_ANNOT_FILE_ATTACHMENT": 17,
    "PDF_ANNOT_SOUND": 18,
    "PDF_ANNOT_MOVIE": 19,
    "PDF_ANNOT_RICH_MEDIA": 20,
    "PDF_ANNOT_WIDGET": 21,
    "PDF_ANNOT_SCREEN": 22,
    "PDF_ANNOT_PRINTER_MARK": 23,
    "PDF_ANNOT_TRAP_NET": 24,
    "PDF_ANNOT_WATERMARK": 25,
    "PDF_ANNOT_3D": 26,
    "PDF_ANNOT_PROJECTION": 27,
    "PDF_ANNOT_UNKNOWN": -1,
    # --- PDF_ANNOT_IS_* flags ---
    "PDF_ANNOT_IS_INVISIBLE": 1,
    "PDF_ANNOT_IS_HIDDEN": 2,
    "PDF_ANNOT_IS_PRINT": 4,
    "PDF_ANNOT_IS_NO_ZOOM": 8,
    "PDF_ANNOT_IS_NO_ROTATE": 16,
    "PDF_ANNOT_IS_NO_VIEW": 32,
    "PDF_ANNOT_IS_READ_ONLY": 64,
    "PDF_ANNOT_IS_LOCKED": 128,
    "PDF_ANNOT_IS_TOGGLE_NO_VIEW": 256,
    "PDF_ANNOT_IS_LOCKED_CONTENTS": 512,
    # --- PDF_ANNOT_LE_* line-end styles ---
    "PDF_ANNOT_LE_NONE": 0,
    "PDF_ANNOT_LE_SQUARE": 1,
    "PDF_ANNOT_LE_CIRCLE": 2,
    "PDF_ANNOT_LE_DIAMOND": 3,
    "PDF_ANNOT_LE_OPEN_ARROW": 4,
    "PDF_ANNOT_LE_CLOSED_ARROW": 5,
    "PDF_ANNOT_LE_BUTT": 6,
    "PDF_ANNOT_LE_R_OPEN_ARROW": 7,
    "PDF_ANNOT_LE_R_CLOSED_ARROW": 8,
    "PDF_ANNOT_LE_SLASH": 9,
    # --- PDF_WIDGET_TYPE_* ---
    "PDF_WIDGET_TYPE_UNKNOWN": 0,
    "PDF_WIDGET_TYPE_BUTTON": 1,
    "PDF_WIDGET_TYPE_CHECKBOX": 2,
    "PDF_WIDGET_TYPE_COMBOBOX": 3,
    "PDF_WIDGET_TYPE_LISTBOX": 4,
    "PDF_WIDGET_TYPE_RADIOBUTTON": 5,
    "PDF_WIDGET_TYPE_SIGNATURE": 6,
    "PDF_WIDGET_TYPE_TEXT": 7,
    # --- PDF_WIDGET_TX_FORMAT_* (fitz exposes only these four) ---
    "PDF_WIDGET_TX_FORMAT_NONE": 0,
    "PDF_WIDGET_TX_FORMAT_NUMBER": 1,
    "PDF_WIDGET_TX_FORMAT_DATE": 3,
    "PDF_WIDGET_TX_FORMAT_TIME": 4,
    # --- PDF_FIELD_IS_* (fitz exposes only these three) ---
    "PDF_FIELD_IS_READ_ONLY": 1,
    "PDF_FIELD_IS_REQUIRED": 2,
    "PDF_FIELD_IS_NO_EXPORT": 4,
    # --- PDF_BM_* blend-mode name strings ---
    "PDF_BM_Normal": "Normal",
    "PDF_BM_Multiply": "Multiply",
    "PDF_BM_Screen": "Screen",
    "PDF_BM_Overlay": "Overlay",
    "PDF_BM_Darken": "Darken",
    "PDF_BM_Lighten": "Lighten",
    "PDF_BM_ColorDodge": "ColorDodge",
    "PDF_BM_ColorBurn": "ColorBurn",
    "PDF_BM_HardLight": "HardLight",
    "PDF_BM_SoftLight": "Softlight",
    "PDF_BM_Difference": "Difference",
    "PDF_BM_Exclusion": "Exclusion",
    "PDF_BM_Hue": "Hue",
    "PDF_BM_Saturation": "Saturation",
    "PDF_BM_Color": "Color",
    "PDF_BM_Luminosity": "Luminosity",
    # --- PDF_REDACT_* ---
    "PDF_REDACT_IMAGE_NONE": 0,
    "PDF_REDACT_IMAGE_REMOVE": 1,
    "PDF_REDACT_IMAGE_PIXELS": 2,
    "PDF_REDACT_LINE_ART_NONE": 0,
    "PDF_REDACT_LINE_ART_REMOVE_IF_COVERED": 1,
    "PDF_REDACT_LINE_ART_REMOVE_IF_TOUCHED": 2,
    "PDF_REDACT_TEXT_REMOVE": 0,
    "PDF_REDACT_TEXT_NONE": 1,
    # --- STAMP_* icons ---
    "STAMP_Approved": 0,
    "STAMP_AsIs": 1,
    "STAMP_Confidential": 2,
    "STAMP_Departmental": 3,
    "STAMP_Experimental": 4,
    "STAMP_Expired": 5,
    "STAMP_Final": 6,
    "STAMP_ForComment": 7,
    "STAMP_ForPublicRelease": 8,
    "STAMP_NotApproved": 9,
    "STAMP_NotForPublicRelease": 10,
    "STAMP_Sold": 11,
    "STAMP_TopSecret": 12,
    "STAMP_Draft": 13,
    # --- PDF_BORDER_STYLE_* ---
    "PDF_BORDER_STYLE_SOLID": 0,
    "PDF_BORDER_STYLE_DASHED": 1,
    "PDF_BORDER_STYLE_BEVELED": 2,
    "PDF_BORDER_STYLE_INSET": 3,
    "PDF_BORDER_STYLE_UNDERLINE": 4,
    # --- PDF_PAGE_LABEL_* ---
    "PDF_PAGE_LABEL_NONE": 0,
    "PDF_PAGE_LABEL_DECIMAL": "D",
    "PDF_PAGE_LABEL_ROMAN_UC": "R",
    "PDF_PAGE_LABEL_ROMAN_LC": "r",
    "PDF_PAGE_LABEL_ALPHA_UC": "A",
    "PDF_PAGE_LABEL_ALPHA_LC": "a",
    # --- PDF_ENCRYPT_* (== MuPDF pdf_encrypt_method) ---
    "PDF_ENCRYPT_KEEP": 0,
    "PDF_ENCRYPT_NONE": 1,
    "PDF_ENCRYPT_RC4_40": 2,
    "PDF_ENCRYPT_RC4_128": 3,
    "PDF_ENCRYPT_AES_128": 4,
    "PDF_ENCRYPT_AES_256": 5,
    "PDF_ENCRYPT_UNKNOWN": 6,
    # --- PDF_SIGNATURE_* appearance / error flags + SigFlag_* ---
    "PDF_SIGNATURE_SHOW_LABELS": 1,
    "PDF_SIGNATURE_SHOW_DN": 2,
    "PDF_SIGNATURE_SHOW_DATE": 4,
    "PDF_SIGNATURE_SHOW_TEXT_NAME": 8,
    "PDF_SIGNATURE_SHOW_GRAPHIC_NAME": 16,
    "PDF_SIGNATURE_SHOW_LOGO": 32,
    "PDF_SIGNATURE_DEFAULT_APPEARANCE": 63,
    "PDF_SIGNATURE_ERROR_OKAY": 0,
    "PDF_SIGNATURE_ERROR_NO_SIGNATURES": 1,
    "PDF_SIGNATURE_ERROR_NO_CERTIFICATE": 2,
    "PDF_SIGNATURE_ERROR_DIGEST_FAILURE": 3,
    "PDF_SIGNATURE_ERROR_SELF_SIGNED": 4,
    "PDF_SIGNATURE_ERROR_SELF_SIGNED_IN_CHAIN": 5,
    "PDF_SIGNATURE_ERROR_NOT_TRUSTED": 6,
    "PDF_SIGNATURE_ERROR_NOT_SIGNED": 7,
    "PDF_SIGNATURE_ERROR_UNKNOWN": 8,
    "SigFlag_SignaturesExist": 1,
    "SigFlag_AppendOnly": 2,
    # --- PDF_PERM_* ---
    "PDF_PERM_PRINT": 4,
    "PDF_PERM_MODIFY": 8,
    "PDF_PERM_COPY": 16,
    "PDF_PERM_ANNOTATE": 32,
    "PDF_PERM_FORM": 256,
    "PDF_PERM_ACCESSIBILITY": 512,
    "PDF_PERM_ASSEMBLE": 1024,
    "PDF_PERM_PRINT_HQ": 2048,
    # --- CS_* colorspace types ---
    "CS_RGB": 1,
    "CS_GRAY": 2,
    "CS_CMYK": 3,
    # --- PDF_TOK_* token types (== MuPDF pdf_token) ---
    "PDF_TOK_ERROR": 0,
    "PDF_TOK_EOF": 1,
    "PDF_TOK_OPEN_ARRAY": 2,
    "PDF_TOK_CLOSE_ARRAY": 3,
    "PDF_TOK_OPEN_DICT": 4,
    "PDF_TOK_CLOSE_DICT": 5,
    "PDF_TOK_OPEN_BRACE": 6,
    "PDF_TOK_CLOSE_BRACE": 7,
    "PDF_TOK_NAME": 8,
    "PDF_TOK_INT": 9,
    "PDF_TOK_REAL": 10,
    "PDF_TOK_STRING": 11,
    "PDF_TOK_KEYWORD": 12,
    "PDF_TOK_R": 13,
    "PDF_TOK_TRUE": 14,
    "PDF_TOK_FALSE": 15,
    "PDF_TOK_NULL": 16,
    "PDF_TOK_OBJ": 17,
    "PDF_TOK_ENDOBJ": 18,
    "PDF_TOK_STREAM": 19,
    "PDF_TOK_ENDSTREAM": 20,
    "PDF_TOK_XREF": 21,
    "PDF_TOK_TRAILER": 22,
    "PDF_TOK_STARTXREF": 23,
    "PDF_TOK_NEWOBJ": 24,
}


def test_constants_exact_values_match_fitz() -> None:
    """Every constant equals the documented PyMuPDF 1.27 value."""
    for name, expected in _EXPECTED.items():
        got = getattr(C, name)
        assert got == expected, f"{name}: {got!r} != {expected!r}"
        # And it must NOT silently coerce a str/int mismatch.
        assert type(got) is type(expected), f"{name}: type mismatch"


def test_constants_reexported_at_top_level() -> None:
    """All constants are reachable as ``pdfspine.<NAME>``."""
    for name in _EXPECTED:
        assert getattr(pdfspine, name) == _EXPECTED[name]


def test_constants_reexported_through_fitz_shim() -> None:
    """All constants are reachable as ``fitz.<NAME>`` (shim parity)."""
    for name in _EXPECTED:
        assert getattr(fitz, name) == _EXPECTED[name]


def test_textflags_bundles_are_consistent_with_atoms() -> None:
    """The bundles really are the documented OR of their atom flags."""
    assert C.TEXTFLAGS_TEXT == (
        C.TEXT_PRESERVE_LIGATURES
        | C.TEXT_PRESERVE_WHITESPACE
        | C.TEXT_MEDIABOX_CLIP
        | C.TEXT_CID_FOR_UNKNOWN_UNICODE
    )
    assert C.TEXTFLAGS_DICT == C.TEXTFLAGS_TEXT | C.TEXT_PRESERVE_IMAGES
    assert C.TEXTFLAGS_SEARCH == (
        C.TEXT_PRESERVE_WHITESPACE
        | C.TEXT_DEHYPHENATE
        | C.TEXT_MEDIABOX_CLIP
        | C.TEXT_CID_FOR_UNKNOWN_UNICODE
    )


def test_version_tuple_shape_matches_fitz() -> None:
    """``fitz.version`` is a 3-tuple ``(VersionBind, VersionFitz, timestamp)``."""
    assert isinstance(pdfspine.version, tuple)
    assert len(pdfspine.version) == 3
    assert pdfspine.version == (pdfspine.VersionBind, pdfspine.VersionFitz, None)
    assert pdfspine.VersionBind == pdfspine.__version__
    assert pdfspine.version_info == pdfspine.version
    assert fitz.version == pdfspine.version


def test_colorspace_constructors_use_fitz_values() -> None:
    """The CS_* swap (RGB=1, GRAY=2) keeps the singletons semantically correct."""
    assert pdfspine.csGRAY.n == 1 and pdfspine.csGRAY.name == "DeviceGray"
    assert pdfspine.csRGB.n == 3 and pdfspine.csRGB.name == "DeviceRGB"
    assert pdfspine.csCMYK.n == 4 and pdfspine.csCMYK.name == "DeviceCMYK"


def test_encryption_roundtrip_with_fitz_method_constants() -> None:
    """Saving with the (now fitz-exact) method constants still encrypts."""
    doc = fitz.open()
    doc.new_page()
    data = doc.tobytes(encryption=fitz.PDF_ENCRYPT_AES_256, owner_pw="o", user_pw="u")
    reopened = fitz.open(stream=data, filetype="pdf")
    assert reopened.needs_pass or reopened.is_encrypted


# ---------------------------------------------------------------------------
# Module-level HELPER FUNCTIONS (PRD §C, Task 1) — values captured from real
# PyMuPDF 1.27 (.venv-oracle); also asserted through the ``fitz`` shim.
# ---------------------------------------------------------------------------
import math

import pytest


def _q(q):
    return (
        tuple(round(v, 3) for v in q.ul),
        tuple(round(v, 3) for v in q.ur),
        tuple(round(v, 3) for v in q.ll),
        tuple(round(v, 3) for v in q.lr),
    )


# A span identical in shape to a real get_text("rawdict") span.
_SPAN = {
    "bbox": (72.0, 50.5, 172.04, 77.98),
    "size": 20.0,
    "ascender": 1.075,
    "descender": -0.299,
}


def test_planish_line_maps_line_to_x_axis() -> None:
    """planish_line(p1,p2): p1 -> origin, p2 -> +x at same distance (fitz-exact)."""
    m = pdfspine.planish_line((1, 1), (4, 5))
    assert tuple(round(v, 6) for v in m) == (0.6, -0.8, 0.8, 0.6, -1.4, 0.2)
    p1 = pdfspine.Point(1, 1) * m
    assert (round(p1.x, 6), round(p1.y, 6)) == (0.0, 0.0)
    p2 = pdfspine.Point(4, 5) * m
    assert round(p2.y, 6) == 0.0
    assert round(p2.x, 6) == round(math.hypot(3, 4), 6)  # length preserved


def test_planish_line_degenerate_is_zero_matrix() -> None:
    """A zero-length line yields the all-zero matrix (matches fitz normalisation)."""
    assert tuple(pdfspine.planish_line((2, 3), (2, 3))) == (0, 0, 0, 0, 0, 0)


def test_recover_quad_matches_oracle() -> None:
    expected = ((72.0, 50.5), (172.04, 50.5), (72.0, 77.98), (172.04, 77.98))
    assert _q(pdfspine.recover_quad((1.0, 0.0), _SPAN)) == expected
    assert _q(fitz.recover_quad((1.0, 0.0), _SPAN)) == expected


def test_recover_bbox_quad_rotated_quadrant() -> None:
    """The rotated (quadrant) branch matches the oracle exactly."""
    c, s = math.cos(math.radians(30)), math.sin(math.radians(30))
    expected = ((72.0, 54.182), (158.3, 50.5), (85.74, 77.98), (172.04, 74.298))
    assert _q(pdfspine.recover_bbox_quad((c, -s), _SPAN, _SPAN["bbox"])) == expected


def test_recover_char_quad_dict_and_tuple() -> None:
    char_d = {"bbox": (72.0, 50.5, 86.44, 77.98)}
    expected = ((72.0, 50.5), (86.44, 50.5), (72.0, 77.98), (86.44, 77.98))
    assert _q(pdfspine.recover_char_quad((1.0, 0.0), _SPAN, char_d)) == expected
    char_t = ("H", 0, 0, (72.0, 50.5, 86.44, 77.98))
    assert _q(pdfspine.recover_char_quad((1.0, 0.0), _SPAN, char_t)) == expected


def test_recover_line_and_span_quad() -> None:
    line = {"dir": (1.0, 0.0), "spans": [_SPAN]}
    expected = ((72.0, 50.5), (172.04, 50.5), (72.0, 77.98), (172.04, 77.98))
    assert _q(pdfspine.recover_line_quad(line)) == expected
    assert _q(pdfspine.recover_span_quad((1.0, 0.0), _SPAN)) == expected


def test_recover_span_quad_char_subselection() -> None:
    span = dict(_SPAN)
    span["chars"] = [
        {"bbox": (72.0, 50.5, 86.44, 77.98)},
        {"bbox": (86.44, 50.5, 102.0, 77.98)},
        {"bbox": (102.0, 50.5, 120.0, 77.98)},
    ]
    q = pdfspine.recover_span_quad((1.0, 0.0), span, span["chars"])
    assert _q(q) == ((72.0, 50.5), (120.0, 50.5), (72.0, 77.98), (120.0, 77.98))


def test_recover_quad_bad_args_raise() -> None:
    with pytest.raises(ValueError):
        pdfspine.recover_quad([1, 0], _SPAN)  # not a tuple
    with pytest.raises(ValueError):
        pdfspine.recover_quad((1.0, 0.0), [1, 2])  # not a dict


def test_glyph_name_unicode_roundtrip() -> None:
    assert pdfspine.glyph_name_to_unicode("A") == 65533  # unicodedata has no "A"
    assert pdfspine.glyph_name_to_unicode("nonexistent_xyz") == 65533
    assert pdfspine.unicode_to_glyph_name(65) == "LATIN CAPITAL LETTER A"
    assert pdfspine.unicode_to_glyph_name(0x2019) == "RIGHT SINGLE QUOTATION MARK"
    assert pdfspine.unicode_to_glyph_name(0) == ".notdef"
    assert fitz.unicode_to_glyph_name(0xFB01) == "LATIN SMALL LIGATURE FI"


def test_srgb_conversions_match_fitz() -> None:
    assert pdfspine.sRGB_to_rgb(0xFF8800) == (255, 136, 0)
    assert pdfspine.sRGB_to_rgb(0x10FF8800) == (255, 136, 0)  # high bits masked
    r, g, b = pdfspine.sRGB_to_pdf(0xFF8800)
    assert (round(r, 6), round(g, 6), round(b, 6)) == (1.0, 0.533333, 0.0)
    assert fitz.sRGB_to_rgb(0x000000) == (0, 0, 0)


def test_get_pdf_now_format() -> None:
    now = pdfspine.get_pdf_now()
    assert now.startswith("D:")
    assert len(now) == 16 + 7 or len(now) == 16  # with/without tz suffix
    assert now[2:16].isdigit()
    if len(now) > 16:
        assert now[16] in "+-"
        assert now.endswith("'")


def test_get_pdf_str_escaping() -> None:
    assert pdfspine.get_pdf_str("") == "()"
    assert pdfspine.get_pdf_str("Hello (World)\\") == "(Hello \\(World\\)\\\\)"
    assert pdfspine.get_pdf_str("\xe9") == "(\\351)"  # 8-bit -> octal
    assert pdfspine.get_pdf_str("中文") == "<feff4e2d6587>"  # UTF-16BE BOM hex
    assert pdfspine.get_pdf_str("a\tb\nc") == "(a\\tb\\nc)"
    assert fitz.get_pdf_str("plain") == "(plain)"


def test_get_text_length_matches_fitz() -> None:
    cases = [
        ("Hello World", "helv", 56.837),
        ("Hello World", "tiro", 55.297),
        ("ABCabc123", "cour", 59.4),
        ("The quick brown fox", "tibo", 98.098),
        ("ABC", "symb", 23.221),
        ("ABC", "zadb", 24.926),
    ]
    for text, font, expected in cases:
        assert round(pdfspine.get_text_length(text, font, 11), 3) == expected
        assert round(fitz.get_text_length(text, font, 11), 3) == expected


def test_get_text_length_cjk_and_errors() -> None:
    assert pdfspine.get_text_length("abc", "china-s", 11) == 33
    assert pdfspine.get_text_length("ABCD", "japan", 10) == 40
    with pytest.raises(ValueError):
        pdfspine.get_text_length("a", "no-such-font")


def test_get_text_length_default_args() -> None:
    """Defaults are fontname='helv', fontsize=11 (fitz signature)."""
    assert round(pdfspine.get_text_length("Hello World"), 3) == 56.837


def test_conversion_header_trailer_match_fitz() -> None:
    assert pdfspine.ConversionHeader("text") == ""
    assert pdfspine.ConversionTrailer("text") == ""
    assert "DOCTYPE html" in pdfspine.ConversionHeader("html")
    assert pdfspine.ConversionTrailer("html") == "</body>\n</html>\n"
    assert pdfspine.ConversionTrailer("xhtml") == "</body>\n</html>\n"
    assert pdfspine.ConversionHeader("xml", "f.pdf").endswith('<document name="f.pdf">\n')
    assert pdfspine.ConversionTrailer("xml") == "</document>\n"
    assert pdfspine.ConversionHeader("json", "f.pdf") == '{"document": "f.pdf", "pages": [\n'
    assert pdfspine.ConversionTrailer("json") == "]\n}"
    assert fitz.ConversionHeader("html") == pdfspine.ConversionHeader("html")
    # Default header is the empty (text) wrapper.
    assert pdfspine.ConversionHeader() == ""


def test_message_and_log_shims(capsys, tmp_path) -> None:
    """message/log route to the configured destination; set_* swaps it."""
    import io

    buf = io.StringIO()
    pdfspine.set_messages(stream=buf)
    pdfspine.message("hello")
    assert buf.getvalue() == "hello\n"
    # restore default stdout sink
    pdfspine.set_messages(stream=__import__("sys").stdout)

    logbuf = io.StringIO()
    pdfspine.set_log(stream=logbuf)
    pdfspine.log("diag")
    out = logbuf.getvalue()
    assert out.endswith("diag\n")
    assert "test_message_and_log_shims" in out  # caller frame info prefixed
    pdfspine.set_log(stream=__import__("sys").stdout)


def test_helpers_exported_via_pdfspine_and_shim() -> None:
    names = [
        "recover_quad", "recover_char_quad", "recover_line_quad",
        "recover_span_quad", "recover_bbox_quad", "planish_line",
        "glyph_name_to_unicode", "unicode_to_glyph_name", "sRGB_to_rgb",
        "sRGB_to_pdf", "get_pdf_now", "get_pdf_str", "get_text_length",
        "ConversionHeader", "ConversionTrailer", "set_messages", "message",
        "set_log", "log",
    ]
    for n in names:
        assert hasattr(pdfspine, n), f"pdfspine missing {n}"
        assert hasattr(fitz, n), f"fitz shim missing {n}"
