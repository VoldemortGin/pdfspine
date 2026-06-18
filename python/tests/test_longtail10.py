"""Long-tail PyMuPDF parity batch 10 — Document low-level COS members (PRD §C
batch-5).

Covers the newly-implemented low-level ``fitz.Document`` cross-reference / object
surface, built on the existing xref read/write + ChangeSet-overlay infra:
  - ``pdf_catalog()``        : the ``/Catalog`` (``/Root``) object number.
  - ``pdf_trailer()``        : the trailer dictionary serialized to a string.
  - ``is_stream(xref)``      : companion of ``xref_is_stream``.
  - ``xref_stream_raw(xref)``: the RAW (still filter-encoded) stream bytes.
  - ``xref_get_keys(xref)``  : the dict keys at ``xref`` (names, no slash).
  - ``xref_is_xobject(xref)``: whether ``xref`` is a Form XObject.
  - ``page_annot_xrefs(pno)``: the page's annotations as ``(xref, type, id)``.
  - ``resolve_names()``      : the ``/Dests`` name-tree as a fitz-shaped dict.
  - ``update_object(xref, text)`` : replace an object's dict from PDF syntax.
  - ``update_stream(xref, data, ...)`` : set an object's stream bytes.
  - ``get_new_xref()``       : allocate a new empty xref slot.

It also covers the Document state/meta surface (PRD §C batch-5 state/meta half):
  - ``pagelayout`` / ``set_pagelayout``  : catalog ``/PageLayout``.
  - ``pagemode``   / ``set_pagemode``    : catalog ``/PageMode``.
  - ``markinfo``   / ``set_markinfo``    : catalog ``/MarkInfo`` dict.
  - ``language``   / ``set_language``    : catalog ``/Lang`` (MuPDF-normalized).
  - ``need_appearances([value])``        : ``/AcroForm /NeedAppearances``.
  - ``get_sigflags()``                   : ``/AcroForm /SigFlags`` (-1 if none).
  - ``xref_xml_metadata()``              : xref of ``/Metadata`` XML (0 if none).
  - ``is_dirty`` / ``is_closed`` / ``is_reflowable`` / ``is_fast_webaccess``.
  - ``name``                             : file path, or ``None`` if stream-opened.
  - ``can_save_incrementally()`` and ``get_page_label(pno)``.

The expected values below are the GROUND TRUTH captured from REAL PyMuPDF 1.27
(``.venv-oracle``) reading the EXACT SAME bytes (the in-repo ``.venv`` ``fitz`` is
the pdfspine shim). For the writer methods the round-trip (write → read back → save →
reopen → confirm persisted) was also cross-checked against real fitz performing
the same operations.

DEVIATIONS from fitz (documented):
  - ``xref_get_keys`` returns keys in the backing dictionary's order, which is
    SORTED (pdfspine stores dicts in a ``BTreeMap``); fitz returns source order.
    The KEY SET is identical — the tests compare sorted key lists. (This is a
    pre-existing pdfspine model trait, shared by ``xref_object`` serialization.)
  - ``xref_is_xobject`` keys on ``/Subtype /Form`` exactly like fitz, so an image
    XObject is False here (use ``xref_is_image`` for those) — matches fitz.
  - ``update_object`` on a stream replaces the DICTIONARY verbatim and preserves
    the underlying stream body bytes; ``/Length`` is recomputed by the writer on
    save (fitz semantics). pdfspine keeps the body's RAW bytes; fitz may re-encode
    them. Either way the dictionary keys and stream identity match.
  - ``update_stream`` with ``new=True`` is slightly MORE lenient than fitz: pdfspine
    can promote a ``null`` slot straight to a stream, whereas fitz requires the
    slot to already be a dict (``update_object`` first). The fitz-canonical
    sequence (``update_object`` then ``update_stream``) is exercised and matches
    fitz byte-for-byte through a save/reopen round-trip.
  - ``is_dirty``: a freshly *created* document (``fitz.open()`` with no source)
    is reported dirty-from-creation by fitz, whereas pdfspine treats opening the
    blank seed bytes as a clean parse (dirty only after a real edit). Both engines
    agree for a *file*-opened document: clean on open, dirty after an edit — which
    is what the test asserts.
  - ``page_annot_xrefs`` returns ``(xref, type-code, id)`` tuples like fitz; the
    type-code uses pdfspine's ``_ANNOT_TYPE_INT`` (the 1.24 baseline). For the
    subtypes whose codes are stable across PyMuPDF 1.24/1.27 (Text=0, Square=4,
    Highlight=8, …) the oracle (1.27) match is exact; only ``Widget`` (1.24=20,
    1.27=21) and ``Redact`` (1.24=25, 1.27=12) were renumbered in 1.27, so those
    are cross-checked against pdfspine's own ``Annot.type`` rather than the oracle.
    Unlike annotation *iteration* (``page.annots()``), this includes ``/Popup``
    annotations — fitz's ``page_annot_xrefs`` dumps the raw ``/Annots`` array.

DEFERRED (does not map cleanly to pdfspine's model — an honest deferral beats a
wrong value):
  - ``version_count`` (incremental-update generations). MuPDF's count is an
    engine-internal artifact of how it lazily layers xref revisions: it is
    neither the ``%%EOF`` count nor the ``/Prev`` chain length. Counter-example
    in the corpus: ``irs-p15.pdf`` has TWO ``%%EOF`` markers AND a trailer
    ``/Prev`` (chain length ≥ 2), yet real fitz reports ``version_count == 1``;
    ``govinfo-hr2.pdf`` reports ``2``. No byte-level or chain-level formula
    reproduces fitz here, so ``version_count`` is left deferred (cf. batch-4's
    deferred ``Font.glyph_bbox``).
"""

from __future__ import annotations

import base64
import tempfile
from pathlib import Path

import fitz
import pdfspine
import pytest


_CORPUS = Path(__file__).resolve().parents[2] / "fixtures" / "corpus"
_GOVINFO = _CORPUS / "govinfo-hr2.pdf"
_IRS_P15 = _CORPUS / "irs-p15.pdf"


# A deterministic 2-page PDF carrying BOTH a catalog ``/Dests`` dict entry
# ("direct") and a ``/Names /Dests`` name-tree entry ("treekey"), each an
# explicit ``/XYZ`` destination. (Built by hand; the same bytes pdfspine AND the
# oracle read.)
_DESTS_PDF = base64.b64decode(
    "JVBERi0xLjcKMSAwIG9iajw8L1R5cGUvQ2F0YWxvZy9QYWdlcyAyIDAgUi9OYW1lczw8L0Rlc3Rz"
    "IDYgMCBSPj4vRGVzdHM8PC9kaXJlY3QgNyAwIFI+Pj4+ZW5kb2JqCjIgMCBvYmo8PC9UeXBlL1Bh"
    "Z2VzL0NvdW50IDIvS2lkc1szIDAgUiA0IDAgUl0+PmVuZG9iagozIDAgb2JqPDwvVHlwZS9QYWdl"
    "L1BhcmVudCAyIDAgUi9NZWRpYUJveFswIDAgMjAwIDMwMF0+PmVuZG9iago0IDAgb2JqPDwvVHlw"
    "ZS9QYWdlL1BhcmVudCAyIDAgUi9NZWRpYUJveFswIDAgMjAwIDMwMF0+PmVuZG9iago2IDAgb2Jq"
    "PDwvTmFtZXNbKHRyZWVrZXkpWzQgMCBSL1hZWiAxMCAyNTAgMF1dPj5lbmRvYmoKNyAwIG9ialsz"
    "IDAgUi9YWVogMjAgMjgwIDBdZW5kb2JqCnRyYWlsZXI8PC9Sb290IDEgMCBSL1NpemUgOD4+CiUl"
    "RU9G"
)

# A fixture with non-/XYZ named dests (the ``dest``-string fallback path).
_FIT_PDF = (
    b"%PDF-1.7\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R/Names<</Dests 6 0 R>>>>endobj\n"
    b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n"
    b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 300]>>endobj\n"
    b"6 0 obj<</Names[(fith)[3 0 R/FitH 222](fit)[3 0 R/Fit]]>>endobj\n"
    b"trailer<</Root 1 0 R/Size 7>>\n%%EOF"
)


# === ground truth from real PyMuPDF 1.27 on the SAME bytes ===================

_GT_GOVINFO_CATALOG = 704
_GT_GOVINFO_XREF_LEN = 744
_GT_GOVINFO_CATKEYS = [
    "AcroForm", "Legal", "Metadata", "Names", "PageLabels", "Pages",
    "Perms", "Type", "Version",
]
_GT_GOVINFO_FORM_XOBJECT = 719
_GT_GOVINFO_FORM_KEYS = ["BBox", "Filter", "Length", "Resources", "Subtype", "Type"]

_GT_IRS_RESOLVE_COUNT = 567
_GT_IRS_SAMPLE_KEY = "-FOInline12489"
_GT_IRS_SAMPLE_VAL = {"page": 6, "to": (0.0, 188.25), "zoom": 0.0}


@pytest.fixture()
def gov():
    doc = fitz.open(str(_GOVINFO))
    yield doc
    doc.close()


# === pdf_catalog / pdf_trailer ==============================================


def test_pdf_catalog(gov):
    assert gov.pdf_catalog() == _GT_GOVINFO_CATALOG


def test_pdf_catalog_is_a_catalog(gov):
    cat = gov.pdf_catalog()
    assert gov.xref_get_key(cat, "Type") == "/Catalog"


def test_pdf_trailer_has_root_and_size(gov):
    trailer = gov.pdf_trailer()
    assert isinstance(trailer, str)
    assert trailer.startswith("<<") and trailer.rstrip().endswith(">>")
    assert "/Root" in trailer
    # The /Root reference in the trailer points at the catalog.
    assert f"{_GT_GOVINFO_CATALOG} 0 R" in trailer


# === is_stream / xref_is_xobject / xref_stream_raw ==========================


def test_is_stream_matches_xref_is_stream(gov):
    for x in range(1, 300):
        assert gov.is_stream(x) == gov.xref_is_stream(x)


def test_xref_stream_raw_vs_decoded(gov):
    sx = next(x for x in range(1, gov.xref_length()) if gov.is_stream(x))
    raw = gov.xref_stream_raw(sx)
    dec = gov.xref_stream(sx)
    assert isinstance(raw, bytes) and isinstance(dec, bytes)
    # The first stream is FlateDecode in this fixture: raw is the compressed
    # payload, decoded is larger.
    assert len(raw) > 0 and len(dec) >= len(raw)


def test_xref_is_xobject_form(gov):
    assert gov.xref_is_xobject(_GT_GOVINFO_FORM_XOBJECT) is True
    assert gov.is_stream(_GT_GOVINFO_FORM_XOBJECT) is True


def test_xref_is_xobject_excludes_image(gov):
    # The first stream (content stream) is not a Form XObject.
    sx = next(x for x in range(1, gov.xref_length()) if gov.is_stream(x))
    assert gov.xref_is_xobject(sx) is False


def test_xref_is_xobject_missing_is_false(gov):
    assert gov.xref_is_xobject(0) is False


# === xref_get_keys ==========================================================


def test_xref_get_keys_catalog(gov):
    keys = gov.xref_get_keys(gov.pdf_catalog())
    assert isinstance(keys, tuple)
    assert sorted(keys) == _GT_GOVINFO_CATKEYS


def test_xref_get_keys_form_xobject(gov):
    keys = gov.xref_get_keys(_GT_GOVINFO_FORM_XOBJECT)
    assert sorted(keys) == _GT_GOVINFO_FORM_KEYS


def test_xref_get_keys_missing_is_empty(gov):
    assert gov.xref_get_keys(0) == ()


# === page_annot_xrefs =======================================================

# A 1-page PDF with three annotations carrying explicit ``/NM`` ids whose
# subtypes have STABLE PyMuPDF type-codes across 1.24/1.27: Highlight=8, Text=0,
# Square=4. (The same bytes pdfspine AND the oracle read.)
_ANNOTS_PDF = base64.b64decode(
    "JVBERi0xLjcKMSAwIG9iajw8L1R5cGUvQ2F0YWxvZy9QYWdlcyAyIDAgUj4+ZW5kb2JqCjIgMCBv"
    "Ymo8PC9UeXBlL1BhZ2VzL0NvdW50IDEvS2lkc1szIDAgUl0+PmVuZG9iagozIDAgb2JqPDwvVHlw"
    "ZS9QYWdlL1BhcmVudCAyIDAgUi9NZWRpYUJveFswIDAgMjAwIDMwMF0vQW5ub3RzWzQgMCBSIDUg"
    "MCBSIDYgMCBSXT4+ZW5kb2JqCjQgMCBvYmo8PC9UeXBlL0Fubm90L1N1YnR5cGUvSGlnaGxpZ2h0"
    "L1JlY3RbMTAgMTAgNTAgMjBdL05NKGgtMSk+PmVuZG9iago1IDAgb2JqPDwvVHlwZS9Bbm5vdC9T"
    "dWJ0eXBlL1RleHQvUmVjdFs2MCA2MCA4MCA4MF0vTk0odC0yKT4+ZW5kb2JqCjYgMCBvYmo8PC9U"
    "eXBlL0Fubm90L1N1YnR5cGUvU3F1YXJlL1JlY3RbOTAgOTAgMTUwIDE1MF0vTk0ocy0zKT4+ZW5k"
    "b2JqCnRyYWlsZXI8PC9Sb290IDEgMCBSL1NpemUgNz4+CiUlRU9G"
)

# Ground truth from real PyMuPDF 1.27 on the bytes above.
_GT_ANNOT_XREFS = [(4, 8, "h-1"), (5, 0, "t-2"), (6, 4, "s-3")]


def test_page_annot_xrefs_tuple_shape_matches_fitz():
    # The FIX: pdfspine returns (xref, type-code, id) tuples like fitz, not bare
    # xref ints. Cross-checked against the real-fitz oracle in
    # test_oracle.py-style harnessing; here the shim must reproduce the GT.
    doc = fitz.open(stream=_ANNOTS_PDF, filetype="pdf")
    assert doc.page_annot_xrefs(0) == _GT_ANNOT_XREFS
    doc.close()


def test_page_annot_xrefs_xref_column_matches_page_method(gov):
    # The xref column still agrees with Page.annot_xrefs() (which yields ints).
    for pno in range(min(gov.page_count, 3)):
        xrefs = [t[0] for t in gov.page_annot_xrefs(pno)]
        assert xrefs == gov.load_page(pno).annot_xrefs()


def test_page_annot_xrefs_empty_page(gov):
    # govinfo pages 1+ have no annotations.
    assert gov.page_annot_xrefs(1) == []


# === resolve_names ==========================================================


def test_resolve_names_dests_dict_and_name_tree():
    doc = pdfspine.open(stream=_DESTS_PDF, filetype="pdf")
    rn = doc.resolve_names()
    assert rn["direct"] == {"page": 0, "to": (20.0, 280.0), "zoom": 0.0}
    assert rn["treekey"] == {"page": 1, "to": (10.0, 250.0), "zoom": 0.0}
    doc.close()


def test_resolve_names_non_xyz_dest_fallback():
    doc = pdfspine.open(stream=_FIT_PDF, filetype="pdf")
    rn = doc.resolve_names()
    assert rn["fith"] == {"page": 0, "dest": "/FitH 222"}
    assert rn["fit"] == {"page": 0, "dest": "/Fit"}
    doc.close()


def test_resolve_names_corpus_count_and_sample():
    doc = fitz.open(str(_IRS_P15))
    rn = doc.resolve_names()
    assert len(rn) == _GT_IRS_RESOLVE_COUNT
    assert rn[_GT_IRS_SAMPLE_KEY] == _GT_IRS_SAMPLE_VAL
    doc.close()


def test_resolve_names_empty_when_no_dests(gov):
    # govinfo-hr2 has a /Names tree but no /Dests entries.
    assert gov.resolve_names() == {}


# === get_new_xref ===========================================================


def test_get_new_xref_allocates_and_bumps_length(gov):
    before = gov.xref_length()
    nx = gov.get_new_xref()
    assert nx == before
    assert gov.xref_length() == before + 1


def test_get_new_xref_is_monotonic(gov):
    a = gov.get_new_xref()
    b = gov.get_new_xref()
    assert b == a + 1


# === update_object ==========================================================


def test_update_object_replaces_dict(gov):
    nx = gov.get_new_xref()
    gov.update_object(nx, "<< /Type /OxideTest /Val 42 >>")
    assert sorted(gov.xref_get_keys(nx)) == ["Type", "Val"]
    assert gov.xref_get_key(nx, "Val") == "42"
    assert gov.xref_get_key(nx, "Type") == "/OxideTest"


def test_update_object_on_stream_preserves_body(gov):
    sx = next(x for x in range(1, gov.xref_length()) if gov.is_stream(x))
    raw = gov.xref_stream_raw(sx)
    gov.update_object(sx, "<< /Type /OxideMod /Foo (bar) >>")
    # Keys are replaced by the new dict; the stream body bytes survive.
    assert sorted(gov.xref_get_keys(sx)) == ["Foo", "Type"]
    assert gov.is_stream(sx) is True
    assert gov.xref_stream_raw(sx) == raw


def test_update_object_rejects_unparseable(gov):
    nx = gov.get_new_xref()
    with pytest.raises(Exception):
        gov.update_object(nx, "<< /Broken")


# === update_stream ==========================================================


def test_update_stream_new_slot(gov):
    nx = gov.get_new_xref()
    gov.update_object(nx, "<< /Type /Demo >>")
    gov.update_stream(nx, b"payload data here", new=True)
    assert gov.is_stream(nx) is True
    assert gov.xref_stream(nx) == b"payload data here"
    assert gov.xref_get_key(nx, "Length") == "17"
    assert sorted(gov.xref_get_keys(nx)) == ["Length", "Type"]


def test_update_stream_accepts_str(gov):
    nx = gov.get_new_xref()
    gov.update_object(nx, "<< /Type /Demo >>")
    gov.update_stream(nx, "stringy", new=True)
    assert gov.xref_stream(nx) == b"stringy"


def test_update_stream_replaces_existing_body(gov):
    sx = next(x for x in range(1, gov.xref_length()) if gov.is_stream(x))
    gov.update_stream(sx, b"brand new body", compress=False)
    assert gov.xref_stream(sx) == b"brand new body"


# === writer round-trip (save → reopen → persisted) ==========================


def test_writer_roundtrip_persists():
    doc = fitz.open(str(_GOVINFO))
    nx = doc.get_new_xref()
    doc.update_object(nx, "<< /Type /Demo /N 7 >>")
    doc.update_stream(nx, b"payload data here", new=True)
    with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as fh:
        out = fh.name
    try:
        doc.save(out)
        doc.close()
        reopened = fitz.open(out)
        assert reopened.is_stream(nx) is True
        assert reopened.xref_stream(nx) == b"payload data here"
        assert reopened.xref_get_key(nx, "N") == "7"
        assert reopened.xref_get_key(nx, "Type") == "/Demo"
        reopened.close()
    finally:
        Path(out).unlink(missing_ok=True)


def test_update_object_roundtrip_persists():
    doc = fitz.open(str(_GOVINFO))
    nx = doc.get_new_xref()
    doc.update_object(nx, "<< /Type /Marker /Tag (pdfspine) >>")
    with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as fh:
        out = fh.name
    try:
        doc.save(out)
        doc.close()
        reopened = fitz.open(out)
        assert sorted(reopened.xref_get_keys(nx)) == ["Tag", "Type"]
        assert reopened.xref_get_key(nx, "Type") == "/Marker"
        reopened.close()
    finally:
        Path(out).unlink(missing_ok=True)

# === state / meta (PRD §C batch-5 state/meta half) ==========================
#
# Ground truth captured from real PyMuPDF 1.27 (.venv-oracle) on the SAME bytes
# / the SAME operation sequence; constants annotated with their oracle source.


def _blank() -> "fitz.Document":
    doc = fitz.open()
    doc.new_page()
    return doc


# --- pagelayout / set_pagelayout -------------------------------------------


def test_pagelayout_default_is_singlepage():
    # oracle: blank doc -> 'SinglePage' (PDF default, returned even when /PageLayout absent)
    doc = _blank()
    assert doc.pagelayout == "SinglePage"
    doc.close()


def test_set_pagelayout_roundtrip():
    # oracle: set_pagelayout('TwoColumnLeft') -> pagelayout == 'TwoColumnLeft',
    # /PageLayout stored as /TwoColumnLeft
    doc = _blank()
    doc.set_pagelayout("TwoColumnLeft")
    assert doc.pagelayout == "TwoColumnLeft"
    assert doc.xref_get_key(doc.pdf_catalog(), "PageLayout") == "/TwoColumnLeft"
    doc.close()


def test_pagelayout_corpus(gov):
    # oracle: govinfo-hr2 -> 'SinglePage'
    assert gov.pagelayout == "SinglePage"


# --- pagemode / set_pagemode ------------------------------------------------


def test_pagemode_default_is_usenone():
    # oracle: blank doc -> 'UseNone'
    doc = _blank()
    assert doc.pagemode == "UseNone"
    doc.close()


def test_set_pagemode_roundtrip():
    # oracle: set_pagemode('UseOutlines') -> 'UseOutlines', /PageMode /UseOutlines
    doc = _blank()
    doc.set_pagemode("UseOutlines")
    assert doc.pagemode == "UseOutlines"
    assert doc.xref_get_key(doc.pdf_catalog(), "PageMode") == "/UseOutlines"
    doc.close()


# --- language / set_language (MuPDF fz_text_language normalization) ----------


def test_language_default_is_none():
    # oracle: blank doc -> None
    doc = _blank()
    assert doc.language is None
    doc.close()


@pytest.mark.parametrize(
    ("tag", "expected"),
    [
        # Every (input -> output) verified against real PyMuPDF 1.27 set_language.
        ("en-US", "en"),
        ("EN", "en"),
        ("fr", "fr"),
        ("de-DE-1996", "de"),
        ("zh-Hant", "zh-Hant"),
        ("zh-Hans", "zh-Hans"),
        ("zh-TW", "zh-Hant"),
        ("zh-CN", "zh-Hans"),
        ("sr-Latn", "sr"),
        ("art-lojban", "art"),
        ("und", "und"),
        ("xy", "xy"),
        ("xyza", "xyz"),
        ("aaa", "aaa"),
        ("en-us-x-foo", "en"),
    ],
)
def test_set_language_normalization(tag, expected):
    doc = _blank()
    doc.set_language(tag)
    assert doc.language == expected
    doc.close()


@pytest.mark.parametrize("tag", ["x", "a-b", "i-klingon", "123", ""])
def test_set_language_invalid_removes_lang(tag):
    # oracle: invalid / too-short tags -> /Lang removed, language is None
    doc = _blank()
    doc.set_language("en")  # seed a value first
    doc.set_language(tag)
    assert doc.language is None
    doc.close()


def test_set_language_none_removes_lang():
    # oracle: set_language(None) -> language None
    doc = _blank()
    doc.set_language("en")
    doc.set_language(None)
    assert doc.language is None
    doc.close()


# --- markinfo / set_markinfo ------------------------------------------------


def test_markinfo_default_is_empty():
    # oracle: blank doc -> {} (no /MarkInfo)
    doc = _blank()
    assert doc.markinfo == {}
    doc.close()


def test_set_markinfo_fills_all_three_keys():
    # oracle: set_markinfo({'Marked': True, 'UserProperties': False}) ->
    # {'Marked': True, 'UserProperties': False, 'Suspects': False}
    doc = _blank()
    doc.set_markinfo({"Marked": True, "UserProperties": False})
    assert doc.markinfo == {
        "Marked": True,
        "UserProperties": False,
        "Suspects": False,
    }
    doc.close()


def test_set_markinfo_partial_defaults_false():
    # oracle: partial dict -> missing keys default False
    doc = _blank()
    doc.set_markinfo({"Marked": True})
    assert doc.markinfo == {
        "Marked": True,
        "UserProperties": False,
        "Suspects": False,
    }
    doc.close()


def test_markinfo_roundtrip_through_save():
    doc = _blank()
    doc.set_markinfo({"Marked": True, "Suspects": True})
    with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as fh:
        out = fh.name
    try:
        doc.save(out)
        doc.close()
        reopened = fitz.open(out)
        assert reopened.markinfo == {
            "Marked": True,
            "UserProperties": False,
            "Suspects": True,
        }
        reopened.close()
    finally:
        Path(out).unlink(missing_ok=True)


# --- need_appearances / get_sigflags ----------------------------------------


def test_need_appearances_none_without_form():
    # oracle: blank doc (no /AcroForm) -> None
    doc = _blank()
    assert doc.need_appearances() is None
    doc.close()


def test_get_sigflags_minus_one_without_form():
    # oracle: blank doc -> -1
    doc = _blank()
    assert doc.get_sigflags() == -1
    doc.close()


def test_get_sigflags_corpus(gov):
    # oracle: govinfo-hr2 -> 3
    assert gov.get_sigflags() == 3


def test_need_appearances_corpus(gov):
    # oracle: govinfo-hr2 -> False (it has an /AcroForm but no /NeedAppearances)
    assert gov.need_appearances() is False


def test_need_appearances_set_on_form_doc():
    # A doc whose catalog carries an inline /AcroForm with one field.
    pdf = (
        b"%PDF-1.7\n"
        b"1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>endobj\n"
        b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n"
        b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 300]/Annots[4 0 R]>>endobj\n"
        b"4 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(t)/Rect[10 10 100 30]>>endobj\n"
        b"trailer<</Root 1 0 R/Size 5>>\n%%EOF"
    )
    doc = fitz.open(stream=pdf, filetype="pdf")
    assert doc.need_appearances() is False  # absent -> False (form present)
    assert doc.need_appearances(True) is True
    assert doc.need_appearances() is True
    doc.close()


# --- xref_xml_metadata ------------------------------------------------------


def test_xref_xml_metadata_present(gov):
    # oracle: govinfo-hr2 -> 705 (catalog /Metadata 705 0 R)
    assert gov.xref_xml_metadata() == 705
    assert gov.xref_get_key(gov.pdf_catalog(), "Metadata") == "705 0 R"


def test_xref_xml_metadata_absent_is_zero():
    # oracle: blank doc -> 0 (fitz returns 0, NOT -1, for a missing /Metadata)
    doc = _blank()
    assert doc.xref_xml_metadata() == 0
    doc.close()


def test_xref_xml_metadata_after_del(gov):
    # oracle: after del_xml_metadata -> 0
    gov.del_xml_metadata()
    assert gov.xref_xml_metadata() == 0


# --- is_dirty / is_closed / is_reflowable -----------------------------------


def test_is_dirty_clean_on_open(gov):
    # oracle: freshly-opened doc -> is_dirty False
    assert gov.is_dirty is False


def test_is_dirty_after_edit():
    # A clean file parse starts not-dirty; a catalog edit flips it to dirty.
    # (A freshly *created* doc is dirty from the start in fitz — exercised here
    # against an opened file, where both engines agree on clean-on-open.)
    doc = fitz.open(str(_GOVINFO))
    assert doc.is_dirty is False
    doc.set_pagelayout("TwoPageLeft")
    assert doc.is_dirty is True
    doc.close()


def test_is_closed_lifecycle():
    # oracle: open -> False, after close -> True
    doc = fitz.open(str(_GOVINFO))
    assert doc.is_closed is False
    doc.close()
    assert doc.is_closed is True


def test_is_reflowable_false_for_pdf(gov):
    # oracle: PDF -> False
    assert gov.is_reflowable is False


# --- is_fast_webaccess (linearization) --------------------------------------


def test_is_fast_webaccess_false_non_linearized(gov):
    # oracle: govinfo-hr2 is not linearized -> falsy
    assert not gov.is_fast_webaccess


def test_is_fast_webaccess_true_linearized():
    # oracle: irs-f1040 is linearized -> truthy
    p = _CORPUS / "irs-f1040.pdf"
    if not p.exists():
        pytest.skip("irs-f1040.pdf missing")
    doc = fitz.open(str(p))
    assert bool(doc.is_fast_webaccess) is True
    doc.close()


# --- name -------------------------------------------------------------------


def test_name_is_path_for_path_opened():
    # oracle: path-opened doc -> the file path
    doc = fitz.open(str(_GOVINFO))
    assert doc.name == str(_GOVINFO)
    doc.close()


def test_name_is_none_for_stream_opened():
    # oracle: stream/new doc -> None
    doc = _blank()
    assert doc.name is None
    doc.close()


# --- can_save_incrementally / get_page_label --------------------------------


def test_can_save_incrementally_clean_doc(gov):
    # oracle: govinfo-hr2 -> True (clean parse)
    assert gov.can_save_incrementally() is True


def test_get_page_label_delegates(gov):
    # oracle: govinfo-hr2 page 0 label -> '1'
    assert gov.get_page_label(0) == "1"
    # Document.get_page_label matches the Page-level getter.
    assert gov.get_page_label(0) == gov.load_page(0).get_label()


# --- xref_get_keys ORDER deviation (PRD §C batch-5 §2, documented, benign) ---


def test_xref_get_keys_order_is_sorted_same_set_as_fitz(gov):
    # pdfspine returns catalog keys SORTED (Dict = BTreeMap); fitz preserves the
    # PDF's stored order. The KEY SET is identical — assert the SET, not order.
    keys = gov.xref_get_keys(gov.pdf_catalog())
    assert set(keys) == set(_GT_GOVINFO_CATKEYS)
    # pdfspine's own output is sorted (the documented model trait).
    assert list(keys) == sorted(keys)
