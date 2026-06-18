"""PyMuPDF-compatible module-level constants (``fitz.*``).

Every integer/string value here matches **PyMuPDF 1.27** exactly so that code
written against ``fitz`` keeps working verbatim against ``pdfspine`` (PRD §7).
The values were cross-checked against a real PyMuPDF 1.27 install; see
``python/tests/test_longtail11.py`` for the parity assertions.

This module is *data only* — pure-Python named constants, no behaviour.
"""

from __future__ import annotations

from . import _core

# ---------------------------------------------------------------------------
# Text-extraction flags (fitz.TEXT_*) — bitmask, OR-combinable.
# ---------------------------------------------------------------------------
TEXT_PRESERVE_LIGATURES = 1
TEXT_PRESERVE_WHITESPACE = 2
TEXT_PRESERVE_IMAGES = 4
TEXT_INHIBIT_SPACES = 8
TEXT_DEHYPHENATE = 16
TEXT_PRESERVE_SPANS = 32
TEXT_MEDIABOX_CLIP = 64
TEXT_CID_FOR_UNKNOWN_UNICODE = 128
TEXT_COLLECT_STRUCTURE = 256
TEXT_ACCURATE_BBOXES = 512
TEXT_COLLECT_VECTORS = 1024
TEXT_IGNORE_ACTUALTEXT = 2048

# Pre-combined flag bundles fitz uses per extraction method (exact 1.27 values).
TEXTFLAGS_TEXT = (
    TEXT_PRESERVE_LIGATURES
    | TEXT_PRESERVE_WHITESPACE
    | TEXT_MEDIABOX_CLIP
    | TEXT_CID_FOR_UNKNOWN_UNICODE
)  # 195
TEXTFLAGS_WORDS = TEXTFLAGS_TEXT  # 195
TEXTFLAGS_BLOCKS = TEXTFLAGS_TEXT  # 195
TEXTFLAGS_DICT = TEXTFLAGS_TEXT | TEXT_PRESERVE_IMAGES  # 199
TEXTFLAGS_RAWDICT = TEXTFLAGS_DICT  # 199
TEXTFLAGS_HTML = TEXTFLAGS_DICT  # 199
TEXTFLAGS_XHTML = TEXTFLAGS_DICT  # 199
TEXTFLAGS_XML = TEXTFLAGS_TEXT  # 195
TEXTFLAGS_SEARCH = (
    TEXT_PRESERVE_WHITESPACE
    | TEXT_DEHYPHENATE
    | TEXT_MEDIABOX_CLIP
    | TEXT_CID_FOR_UNKNOWN_UNICODE
)  # 210

# ---------------------------------------------------------------------------
# Span font property flags (fitz.TEXT_FONT_*).
# ---------------------------------------------------------------------------
TEXT_FONT_SUPERSCRIPT = 1
TEXT_FONT_ITALIC = 2
TEXT_FONT_SERIFED = 4
TEXT_FONT_MONOSPACED = 8
TEXT_FONT_BOLD = 16

# ---------------------------------------------------------------------------
# Text alignment (fitz.TEXT_ALIGN_*).
# ---------------------------------------------------------------------------
TEXT_ALIGN_LEFT = 0
TEXT_ALIGN_CENTER = 1
TEXT_ALIGN_RIGHT = 2
TEXT_ALIGN_JUSTIFY = 3

# ---------------------------------------------------------------------------
# Annotation types (fitz.PDF_ANNOT_*). Values are the MuPDF enum_pdf_annot_type.
# ---------------------------------------------------------------------------
PDF_ANNOT_TEXT = 0
PDF_ANNOT_LINK = 1
PDF_ANNOT_FREE_TEXT = 2
PDF_ANNOT_LINE = 3
PDF_ANNOT_SQUARE = 4
PDF_ANNOT_CIRCLE = 5
PDF_ANNOT_POLYGON = 6
PDF_ANNOT_POLY_LINE = 7
PDF_ANNOT_HIGHLIGHT = 8
PDF_ANNOT_UNDERLINE = 9
PDF_ANNOT_SQUIGGLY = 10
PDF_ANNOT_STRIKE_OUT = 11
PDF_ANNOT_REDACT = 12
PDF_ANNOT_STAMP = 13
PDF_ANNOT_CARET = 14
PDF_ANNOT_INK = 15
PDF_ANNOT_POPUP = 16
PDF_ANNOT_FILE_ATTACHMENT = 17
PDF_ANNOT_SOUND = 18
PDF_ANNOT_MOVIE = 19
PDF_ANNOT_RICH_MEDIA = 20
PDF_ANNOT_WIDGET = 21
PDF_ANNOT_SCREEN = 22
PDF_ANNOT_PRINTER_MARK = 23
PDF_ANNOT_TRAP_NET = 24
PDF_ANNOT_WATERMARK = 25
PDF_ANNOT_3D = 26
PDF_ANNOT_PROJECTION = 27
PDF_ANNOT_UNKNOWN = -1

# ---------------------------------------------------------------------------
# Annotation flags (fitz.PDF_ANNOT_IS_*). /F entry bits.
# ---------------------------------------------------------------------------
PDF_ANNOT_IS_INVISIBLE = 1
PDF_ANNOT_IS_HIDDEN = 2
PDF_ANNOT_IS_PRINT = 4
PDF_ANNOT_IS_NO_ZOOM = 8
PDF_ANNOT_IS_NO_ROTATE = 16
PDF_ANNOT_IS_NO_VIEW = 32
PDF_ANNOT_IS_READ_ONLY = 64
PDF_ANNOT_IS_LOCKED = 128
PDF_ANNOT_IS_TOGGLE_NO_VIEW = 256
PDF_ANNOT_IS_LOCKED_CONTENTS = 512

# ---------------------------------------------------------------------------
# Line-end styles (fitz.PDF_ANNOT_LE_*).
# ---------------------------------------------------------------------------
PDF_ANNOT_LE_NONE = 0
PDF_ANNOT_LE_SQUARE = 1
PDF_ANNOT_LE_CIRCLE = 2
PDF_ANNOT_LE_DIAMOND = 3
PDF_ANNOT_LE_OPEN_ARROW = 4
PDF_ANNOT_LE_CLOSED_ARROW = 5
PDF_ANNOT_LE_BUTT = 6
PDF_ANNOT_LE_R_OPEN_ARROW = 7
PDF_ANNOT_LE_R_CLOSED_ARROW = 8
PDF_ANNOT_LE_SLASH = 9

# ---------------------------------------------------------------------------
# Widget (form field) types (fitz.PDF_WIDGET_TYPE_*).
# ---------------------------------------------------------------------------
PDF_WIDGET_TYPE_UNKNOWN = 0
PDF_WIDGET_TYPE_BUTTON = 1
PDF_WIDGET_TYPE_CHECKBOX = 2
PDF_WIDGET_TYPE_COMBOBOX = 3
PDF_WIDGET_TYPE_LISTBOX = 4
PDF_WIDGET_TYPE_RADIOBUTTON = 5
PDF_WIDGET_TYPE_SIGNATURE = 6
PDF_WIDGET_TYPE_TEXT = 7

# ---------------------------------------------------------------------------
# Text-field formatting (fitz.PDF_WIDGET_TX_FORMAT_*).
# fitz 1.27 exposes only NONE/NUMBER/DATE/TIME (no MONETARY/PERCENT names).
# ---------------------------------------------------------------------------
PDF_WIDGET_TX_FORMAT_NONE = 0
PDF_WIDGET_TX_FORMAT_NUMBER = 1
PDF_WIDGET_TX_FORMAT_DATE = 3
PDF_WIDGET_TX_FORMAT_TIME = 4

# ---------------------------------------------------------------------------
# Field flags (fitz.PDF_FIELD_IS_*). fitz 1.27 exposes only these three.
# ---------------------------------------------------------------------------
PDF_FIELD_IS_READ_ONLY = 1
PDF_FIELD_IS_REQUIRED = 2
PDF_FIELD_IS_NO_EXPORT = 4

# ---------------------------------------------------------------------------
# Blend modes (fitz.PDF_BM_*). These are the /BM name strings.
# ---------------------------------------------------------------------------
PDF_BM_Normal = "Normal"
PDF_BM_Multiply = "Multiply"
PDF_BM_Screen = "Screen"
PDF_BM_Overlay = "Overlay"
PDF_BM_Darken = "Darken"
PDF_BM_Lighten = "Lighten"
PDF_BM_ColorDodge = "ColorDodge"
PDF_BM_ColorBurn = "ColorBurn"
PDF_BM_HardLight = "HardLight"
PDF_BM_SoftLight = "Softlight"  # fitz spelling (lower-l)
PDF_BM_Difference = "Difference"
PDF_BM_Exclusion = "Exclusion"
PDF_BM_Hue = "Hue"
PDF_BM_Saturation = "Saturation"
PDF_BM_Color = "Color"
PDF_BM_Luminosity = "Luminosity"

# ---------------------------------------------------------------------------
# Redaction options (fitz.PDF_REDACT_*).
# ---------------------------------------------------------------------------
PDF_REDACT_IMAGE_NONE = 0
PDF_REDACT_IMAGE_REMOVE = 1
PDF_REDACT_IMAGE_PIXELS = 2
PDF_REDACT_LINE_ART_NONE = 0
PDF_REDACT_LINE_ART_REMOVE_IF_COVERED = 1
PDF_REDACT_LINE_ART_REMOVE_IF_TOUCHED = 2
PDF_REDACT_TEXT_REMOVE = 0
PDF_REDACT_TEXT_NONE = 1

# ---------------------------------------------------------------------------
# Standard stamp icons (fitz.STAMP_*).
# ---------------------------------------------------------------------------
STAMP_Approved = 0
STAMP_AsIs = 1
STAMP_Confidential = 2
STAMP_Departmental = 3
STAMP_Experimental = 4
STAMP_Expired = 5
STAMP_Final = 6
STAMP_ForComment = 7
STAMP_ForPublicRelease = 8
STAMP_NotApproved = 9
STAMP_NotForPublicRelease = 10
STAMP_Sold = 11
STAMP_TopSecret = 12
STAMP_Draft = 13

# ---------------------------------------------------------------------------
# Border styles (fitz.PDF_BORDER_STYLE_*).
# ---------------------------------------------------------------------------
PDF_BORDER_STYLE_SOLID = 0
PDF_BORDER_STYLE_DASHED = 1
PDF_BORDER_STYLE_BEVELED = 2
PDF_BORDER_STYLE_INSET = 3
PDF_BORDER_STYLE_UNDERLINE = 4

# ---------------------------------------------------------------------------
# Page-label numbering styles (fitz.PDF_PAGE_LABEL_*).
# ---------------------------------------------------------------------------
PDF_PAGE_LABEL_NONE = 0
PDF_PAGE_LABEL_DECIMAL = "D"
PDF_PAGE_LABEL_ROMAN_UC = "R"
PDF_PAGE_LABEL_ROMAN_LC = "r"
PDF_PAGE_LABEL_ALPHA_UC = "A"
PDF_PAGE_LABEL_ALPHA_LC = "a"

# ---------------------------------------------------------------------------
# Encryption methods (fitz.PDF_ENCRYPT_*) == MuPDF pdf_encrypt_method.
# ---------------------------------------------------------------------------
PDF_ENCRYPT_KEEP = 0
PDF_ENCRYPT_NONE = 1
PDF_ENCRYPT_RC4_40 = 2
PDF_ENCRYPT_RC4_128 = 3
PDF_ENCRYPT_AES_128 = 4
PDF_ENCRYPT_AES_256 = 5
PDF_ENCRYPT_UNKNOWN = 6

# ---------------------------------------------------------------------------
# Signature appearance flags (fitz.PDF_SIGNATURE_SHOW_* + DEFAULT_APPEARANCE),
# verification error codes (fitz.PDF_SIGNATURE_ERROR_*), and the /SigFlags bits
# (fitz.SigFlag_*). Values match PyMuPDF 1.27 exactly.
# ---------------------------------------------------------------------------
PDF_SIGNATURE_SHOW_LABELS = 1
PDF_SIGNATURE_SHOW_DN = 2
PDF_SIGNATURE_SHOW_DATE = 4
PDF_SIGNATURE_SHOW_TEXT_NAME = 8
PDF_SIGNATURE_SHOW_GRAPHIC_NAME = 16
PDF_SIGNATURE_SHOW_LOGO = 32
PDF_SIGNATURE_DEFAULT_APPEARANCE = 63

PDF_SIGNATURE_ERROR_OKAY = 0
PDF_SIGNATURE_ERROR_NO_SIGNATURES = 1
PDF_SIGNATURE_ERROR_NO_CERTIFICATE = 2
PDF_SIGNATURE_ERROR_DIGEST_FAILURE = 3
PDF_SIGNATURE_ERROR_SELF_SIGNED = 4
PDF_SIGNATURE_ERROR_SELF_SIGNED_IN_CHAIN = 5
PDF_SIGNATURE_ERROR_NOT_TRUSTED = 6
PDF_SIGNATURE_ERROR_NOT_SIGNED = 7
PDF_SIGNATURE_ERROR_UNKNOWN = 8

SigFlag_SignaturesExist = 1  # noqa: N816 — PyMuPDF spelling
SigFlag_AppendOnly = 2  # noqa: N816

# ---------------------------------------------------------------------------
# Permission flags (fitz.PDF_PERM_*). /P entry bits.
# ---------------------------------------------------------------------------
PDF_PERM_PRINT = 4
PDF_PERM_MODIFY = 8
PDF_PERM_COPY = 16
PDF_PERM_ANNOTATE = 32
PDF_PERM_FORM = 256
PDF_PERM_ACCESSIBILITY = 512
PDF_PERM_ASSEMBLE = 1024
PDF_PERM_PRINT_HQ = 2048

# ---------------------------------------------------------------------------
# Colorspace types (fitz.CS_*).
# ---------------------------------------------------------------------------
CS_RGB = 1
CS_GRAY = 2
CS_CMYK = 3

# ---------------------------------------------------------------------------
# PDF tokenizer token types (fitz.PDF_TOK_*) == MuPDF pdf_token enum.
# Named integer constants only (pdfspine exposes no public tokenizer); kept for
# verbatim source compatibility with code that references the names.
# ---------------------------------------------------------------------------
PDF_TOK_ERROR = 0
PDF_TOK_EOF = 1
PDF_TOK_OPEN_ARRAY = 2
PDF_TOK_CLOSE_ARRAY = 3
PDF_TOK_OPEN_DICT = 4
PDF_TOK_CLOSE_DICT = 5
PDF_TOK_OPEN_BRACE = 6
PDF_TOK_CLOSE_BRACE = 7
PDF_TOK_NAME = 8
PDF_TOK_INT = 9
PDF_TOK_REAL = 10
PDF_TOK_STRING = 11
PDF_TOK_KEYWORD = 12
PDF_TOK_R = 13
PDF_TOK_TRUE = 14
PDF_TOK_FALSE = 15
PDF_TOK_NULL = 16
PDF_TOK_OBJ = 17
PDF_TOK_ENDOBJ = 18
PDF_TOK_STREAM = 19
PDF_TOK_ENDSTREAM = 20
PDF_TOK_XREF = 21
PDF_TOK_TRAILER = 22
PDF_TOK_STARTXREF = 23
PDF_TOK_NEWOBJ = 24

# ---------------------------------------------------------------------------
# Version info (fitz.version / VersionBind / VersionFitz). Matches fitz's SHAPE:
# version == (VersionBind, VersionFitz, timestamp). pdfspine has no separate
# MuPDF layer, so both strings carry pdfspine's own version; timestamp is None
# (as fitz itself reports for un-dated builds).
# ---------------------------------------------------------------------------
VersionBind: str = _core.__version__
VersionFitz: str = _core.__version__
VersionDate = None
version: tuple[str, str, None] = (VersionBind, VersionFitz, VersionDate)
version_info: tuple[str, str, None] = version

# --- Additional names exposed by real PyMuPDF 1.27 (full-family parity) -------
# Extra TEXT_* stext flags / encodings / output selectors (some are aliases of
# names above, e.g. TEXT_CLIP == TEXT_MEDIABOX_CLIP == 64).
TEXT_CLIP = 64
TEXT_COLLECT_STYLES = 32768
TEXT_CLIP_RECT = 131072
TEXT_ACCURATE_ASCENDERS = 262144
TEXT_ACCURATE_SIDE_BEARINGS = 524288
TEXT_LAZY_VECTORS = 1048576
TEXT_FUZZY_VECTORS = 2097152
TEXT_SEGMENT = 4096
TEXT_STEXT_SEGMENT = 4096
TEXT_PARAGRAPH_BREAK = 8192
TEXT_TABLE_HUNT = 16384
TEXT_USE_CID_FOR_UNKNOWN_UNICODE = 128
TEXT_USE_GID_FOR_UNKNOWN_UNICODE = 65536
TEXT_ENCODING_LATIN = 0
TEXT_ENCODING_GREEK = 1
TEXT_ENCODING_CYRILLIC = 2
TEXT_OUTPUT_TEXT = 0
TEXT_OUTPUT_HTML = 1
TEXT_OUTPUT_JSON = 2
TEXT_OUTPUT_XML = 3
TEXT_OUTPUT_XHTML = 4

# Annotation intent (IT) + quadding (Q) constants.
PDF_ANNOT_IT_DEFAULT = 0
PDF_ANNOT_IT_FREETEXT_CALLOUT = 1
PDF_ANNOT_IT_FREETEXT_TYPEWRITER = 2
PDF_ANNOT_IT_LINE_ARROW = 3
PDF_ANNOT_IT_LINE_DIMENSION = 4
PDF_ANNOT_IT_POLYLINE_DIMENSION = 5
PDF_ANNOT_IT_POLYGON_CLOUD = 6
PDF_ANNOT_IT_POLYGON_DIMENSION = 7
PDF_ANNOT_IT_STAMP_IMAGE = 8
PDF_ANNOT_IT_STAMP_SNAPSHOT = 9
PDF_ANNOT_IT_UNKNOWN = 255
PDF_ANNOT_Q_LEFT = 0
PDF_ANNOT_Q_CENTER = 1
PDF_ANNOT_Q_RIGHT = 2

# Widget text-format SPECIAL + extra redaction options present in fitz 1.27.
PDF_WIDGET_TX_FORMAT_SPECIAL = 2
PDF_REDACT_TEXT_REMOVE_INVISIBLE = 2
PDF_REDACT_IMAGE_REMOVE_UNLESS_INVISIBLE = 3

# Public surface: every constant name defined above (data-only module).
__all__ = [
    _n
    for _n in dir()
    if not _n.startswith("_") and _n not in ("annotations", "_core")
]
