"""Runtime contract for deferred PyMuPDF baseline symbols (PRD §7 / §9.5).

The §7 fitz-migration contract promises that a *deferred* baseline symbol raises
:class:`~pdfspine._core.PdfUnsupportedError` (with a helpful hint), never a bare
``AttributeError``. The set of deferred symbols is the single source of truth in
``COMPAT.toml`` (regenerated into ``pdfspine._compat_deferred``); this test
exercises every one of them on a live object and asserts the strongest guarantee
each owner can give:

* ``Page`` / ``Document`` — routed through a Python ``__getattr__``: *instance
  attribute access* must raise ``PdfUnsupportedError``.
* ``Annot`` / ``Font`` — the member exists as a descriptor that raises on
  *call* (handled in the Rust core); accessing it does not raise, calling it
  does.
* ``Pixmap`` / ``DisplayList`` / ``Tools`` — non-subclassable ``_core`` aliases
  whose deferred members are simply absent; they raise ``AttributeError`` and
  can only be made to raise ``PdfUnsupportedError`` in the Rust core (tracked in
  COMPAT.toml). Asserted via ``xfail`` so the gap is visible, not hidden.
* ``image_profile`` (module level) — covered by the ``fitz`` shim's module
  ``__getattr__``, which raises ``PdfUnsupportedError``.
"""

from __future__ import annotations

import tomllib
from collections import defaultdict
from pathlib import Path

import pytest

import pdfspine
from pdfspine._compat_deferred import DEFERRED
from pdfspine._core import PdfUnsupportedError

REPO_ROOT = Path(__file__).resolve().parents[2]
COMPAT_TOML = REPO_ROOT / "COMPAT.toml"


def _deferred_from_compat() -> set[str]:
    data = tomllib.loads(COMPAT_TOML.read_text(encoding="utf-8"))
    return {s["name"] for s in data["symbol"] if s["disposition"] == "deferred"}


def _by_group() -> dict[str, list[str]]:
    out: dict[str, list[str]] = defaultdict(list)
    for sym in DEFERRED:
        group, _, member = sym.partition(".")
        if member:
            out[group].append(member)
    return out


def test_generated_deferred_set_matches_compat() -> None:
    """``pdfspine._compat_deferred.DEFERRED`` mirrors COMPAT.toml exactly."""
    assert set(DEFERRED) == _deferred_from_compat()


# ---------------------------------------------------------------------------
# Page / Document — the symbols routed through Python __getattr__ (the P0-2 fix)
# ---------------------------------------------------------------------------
_PAGE_DEFERRED = sorted(_by_group()["Page"])
_DOC_DEFERRED = sorted(_by_group()["Document"])


@pytest.fixture()
def doc_and_page():
    doc = pdfspine.open()
    page = doc.new_page()
    yield doc, page
    doc.close()


@pytest.mark.parametrize("member", _PAGE_DEFERRED)
def test_page_deferred_symbol_raises_unsupported(member, doc_and_page) -> None:
    _doc, page = doc_and_page
    with pytest.raises(PdfUnsupportedError) as exc:
        getattr(page, member)
    msg = str(exc.value)
    assert member in msg and "deferred" in msg


@pytest.mark.parametrize("member", _DOC_DEFERRED)
def test_document_deferred_symbol_raises_unsupported(member, doc_and_page) -> None:
    doc, _page = doc_and_page
    with pytest.raises(PdfUnsupportedError) as exc:
        getattr(doc, member)
    msg = str(exc.value)
    assert member in msg and "deferred" in msg


def test_page_deferred_symbols_never_raise_attributeerror(doc_and_page) -> None:
    """Regression guard: no deferred Page symbol leaks a bare AttributeError."""
    _doc, page = doc_and_page
    for member in _PAGE_DEFERRED:
        try:
            getattr(page, member)
        except PdfUnsupportedError:
            continue
        except AttributeError:  # pragma: no cover - this is the failure we forbid
            pytest.fail(f"Page.{member} raised AttributeError, want PdfUnsupportedError")


def test_document_deferred_symbols_never_raise_attributeerror(doc_and_page) -> None:
    doc, _page = doc_and_page
    for member in _DOC_DEFERRED:
        try:
            getattr(doc, member)
        except PdfUnsupportedError:
            continue
        except AttributeError:  # pragma: no cover - this is the failure we forbid
            pytest.fail(f"Document.{member} raised AttributeError, want PdfUnsupportedError")


# An attribute that is NOT deferred and NOT real must still be a plain
# AttributeError — __getattr__ only intercepts the deferred set.
def test_unknown_attribute_is_plain_attributeerror(doc_and_page) -> None:
    doc, page = doc_and_page
    with pytest.raises(AttributeError):
        page.definitely_not_a_real_symbol
    with pytest.raises(AttributeError):
        doc.definitely_not_a_real_symbol


# ---------------------------------------------------------------------------
# Annot / Font — exist as descriptors; calling raises PdfUnsupportedError
# ---------------------------------------------------------------------------
def test_annot_get_textbox_raises_on_call() -> None:
    doc = pdfspine.open()
    page = doc.new_page()
    annot = page.add_redact_annot(pdfspine.Rect(0, 0, 10, 10))
    with pytest.raises(PdfUnsupportedError):
        annot.get_textbox(pdfspine.Rect(0, 0, 10, 10))
    doc.close()


@pytest.mark.parametrize("member", sorted(_by_group()["Font"]))
def test_font_deferred_member_raises_on_use(member) -> None:
    font = pdfspine.Font("helv")
    with pytest.raises(PdfUnsupportedError):
        value = getattr(font, member)
        if callable(value):
            value(65)


# ---------------------------------------------------------------------------
# image_profile (module level) — now implemented; resolves via the fitz shim
# ---------------------------------------------------------------------------
def test_module_image_profile_resolves_via_fitz_shim() -> None:
    import pdfspine.fitz as fitz

    assert callable(fitz.image_profile)
    # Unrecognized input returns None (fitz parity), never raises.
    assert fitz.image_profile(b"not an image") is None


# ---------------------------------------------------------------------------
# _core aliases — absent members, fixable only in Rust. xfail keeps it visible.
# ---------------------------------------------------------------------------
_CORE_ALIAS_DEFERRED = sorted(
    sym
    for sym in DEFERRED
    for grp in (sym.split(".", 1)[0],)
    if grp in {"Pixmap", "DisplayList", "Tools"} and "." in sym
)


@pytest.mark.xfail(
    reason="_core aliases (Pixmap/DisplayList/Tools) are non-subclassable PyO3 "
    "types; their deferred members are absent and raise AttributeError. "
    "PdfUnsupportedError for these requires a Rust-core change (tracked in COMPAT.toml).",
    strict=False,
)
@pytest.mark.parametrize("sym", _CORE_ALIAS_DEFERRED)
def test_core_alias_deferred_symbol_raises_unsupported(sym) -> None:
    group, _, member = sym.partition(".")
    cls = getattr(pdfspine, group)
    with pytest.raises(PdfUnsupportedError):
        getattr(cls, member)
