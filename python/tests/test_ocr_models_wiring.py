"""OCR-MODELS-WIRING-* — the Python side that points the Rust PaddleOCR engine at
its model weights.

The published ``pdfspine`` wheel has the OCR code compiled in but ships no models;
the ~28 MB PP-OCRv5 ONNX weights come from the shared ``ocrspine-models`` data
package (a hard dependency), so a plain ``pip install pdfspine`` is
full-OCR-capable offline. ``document._ensure_ocr_models_env`` exports a model
directory as both ``PDFSPINE_OCR_MODELS`` and the engine's ``OCRSPINE_MODELS``,
resolving (in order): an explicit env override → the shared ``ocrspine_models``
data package → the legacy ``pdfspine_ocr_models`` companion data package.

These tests exercise the pure-Python wiring (no real OCR run, no real package
install needed) plus the clear error on an install with no models at all. They
inject fake modules into ``sys.modules`` with ``monkeypatch`` so each tier's
priority stays observable in isolation.
"""

from __future__ import annotations

import sys
import types

import pdfspine
import pytest

from pdfspine import document as _doc

_ENV = "PDFSPINE_OCR_MODELS"
# The engine (``ocrspine``) reads this var; the helper now mirrors the resolved
# directory into it too, so the fixture must snapshot/restore it as well.
_ENGINE_ENV = "OCRSPINE_MODELS"


@pytest.fixture(autouse=True)
def _restore_models_env():
    """Snapshot/restore ``PDFSPINE_OCR_MODELS`` and ``OCRSPINE_MODELS`` around each
    test.

    ``_ensure_ocr_models_env`` writes ``os.environ`` directly (not via
    monkeypatch), so monkeypatch's teardown cannot undo that write — this fixture
    guarantees both env vars are restored so the helper's mutation never leaks into
    other test modules (e.g. the paddle e2e tests that rely on the in-crate dev
    fallback)."""
    sentinel = object()
    saved = {k: _doc.os.environ.get(k, sentinel) for k in (_ENV, _ENGINE_ENV)}
    try:
        yield
    finally:
        for k, v in saved.items():
            if v is sentinel:
                _doc.os.environ.pop(k, None)
            else:
                _doc.os.environ[k] = v


# --- OCR-MODELS-WIRING-000: the shared ocrspine-models tier wins (the default) -


def test_ensure_ocr_models_env_sets_from_data_package(monkeypatch, tmp_path):
    """With no env preset, the helper exports the shared ``ocrspine_models`` data
    package's ``models_dir()`` — even if the legacy companion is also importable
    (the shared package wins)."""
    monkeypatch.delenv(_ENV, raising=False)

    shared = types.ModuleType("ocrspine_models")
    shared.models_dir = lambda: str(tmp_path)  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "ocrspine_models", shared)

    # A legacy companion that, were it consulted, would win — it must NOT be.
    legacy = types.ModuleType("pdfspine_ocr_models")
    legacy.models_dir = lambda: "/legacy/dir"  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "pdfspine_ocr_models", legacy)

    _doc._ensure_ocr_models_env()

    assert _doc.os.environ.get(_ENV) == str(tmp_path)
    assert _doc.os.environ.get(_ENGINE_ENV) == str(tmp_path)


# --- OCR-MODELS-WIRING-001: legacy companion is the back-compat fallback ------


def test_ensure_ocr_models_env_sets_from_legacy_companion(monkeypatch, tmp_path):
    """With no env preset and no shared ``ocrspine_models``, the helper falls back
    to the legacy ``pdfspine_ocr_models`` companion's ``models_dir()``."""
    monkeypatch.delenv(_ENV, raising=False)
    monkeypatch.setitem(sys.modules, "ocrspine_models", None)  # force ImportError

    # A fake legacy companion so the test needs no real 28 MB install.
    legacy = types.ModuleType("pdfspine_ocr_models")
    legacy.models_dir = lambda: str(tmp_path)  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "pdfspine_ocr_models", legacy)

    _doc._ensure_ocr_models_env()

    assert _doc.os.environ.get(_ENV) == str(tmp_path)


# --- OCR-MODELS-WIRING-002: a preset env is an override, never clobbered -----


def test_ensure_ocr_models_env_respects_preset(monkeypatch, tmp_path):
    """If the user already set ``PDFSPINE_OCR_MODELS``, the helper leaves it as-is
    even when the shared package exists (explicit override wins)."""
    monkeypatch.setenv(_ENV, "/user/override/dir")

    shared = types.ModuleType("ocrspine_models")
    shared.models_dir = lambda: str(tmp_path)  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "ocrspine_models", shared)

    _doc._ensure_ocr_models_env()

    assert _doc.os.environ.get(_ENV) == "/user/override/dir"
    assert _doc.os.environ.get(_ENGINE_ENV) == "/user/override/dir"


# --- OCR-MODELS-WIRING-003: nothing importable + no env -> no-op --------------


def test_ensure_ocr_models_env_noop_without_packages(monkeypatch):
    """With no preset env and neither the shared package nor the legacy companion
    importable, the helper does nothing (leaving the Rust dev fallback / clear
    error to take over)."""
    monkeypatch.delenv(_ENV, raising=False)
    monkeypatch.setitem(sys.modules, "ocrspine_models", None)  # force ImportError
    monkeypatch.setitem(sys.modules, "pdfspine_ocr_models", None)  # force ImportError

    _doc._ensure_ocr_models_env()

    assert _ENV not in _doc.os.environ


# --- OCR-MODELS-WIRING-004: missing models -> clear PdfUnsupportedError ------


def test_missing_models_raises_clear_error(monkeypatch, tmp_path):
    """A true base install (OCR compiled in, but no models at all) raises a clear
    ``PdfUnsupportedError`` pointing at ``pip install pdfspine[ocr]``.

    Simulated by pointing ``PDFSPINE_OCR_MODELS`` at an empty directory: the Rust
    engine cannot read the ONNX and maps that to the documented error. Skipped on
    a lean build (paddle compiled out) since the error text there is the same
    'install pdfspine[ocr]' message but raised before any model lookup.
    """
    monkeypatch.setenv(_ENV, str(tmp_path))  # empty dir: no ONNX present

    doc = pdfspine.open()
    page = doc.new_page(width=8.0, height=8.0)
    page.insert_image((0, 0, 8.0, 8.0), stream=b"\xff" * (8 * 8 * 3), width=8, height=8)

    with pytest.raises(pdfspine.PdfUnsupportedError) as excinfo:
        doc[0].get_textpage_ocr(dpi=72, engine="paddle")
    assert "pdfspine[ocr]" in str(excinfo.value)
