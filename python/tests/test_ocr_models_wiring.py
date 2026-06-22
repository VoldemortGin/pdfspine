"""OCR-MODELS-WIRING-* — the Python side that points the Rust PaddleOCR engine at
its model weights.

The published ``pdfspine`` wheel has the OCR code compiled in AND ships the
~15 MB PP-OCRv4 ONNX weights inside the package (at ``pdfspine/_models``), so a
plain ``pip install pdfspine`` is full-OCR-capable offline.
``document._ensure_ocr_models_env`` exports a model directory as
``PDFSPINE_OCR_MODELS`` for the Rust ``models_dir()`` to read, resolving (in
order): an explicit env override → the wheel-bundled ``pdfspine/_models`` → the
legacy ``pdfspine_ocr_models`` companion data package.

These tests exercise the pure-Python wiring (no real OCR run, no real companion
install needed) plus the clear error on an install with no models at all. They
neutralize the wheel-bundled tier with ``monkeypatch`` where they target the
companion / no-op fallbacks, so the order stays observable in isolation.
"""

from __future__ import annotations

import sys
import types

import pdfspine
import pytest

from pdfspine import document as _doc

_ENV = "PDFSPINE_OCR_MODELS"


@pytest.fixture(autouse=True)
def _restore_models_env():
    """Snapshot/restore ``PDFSPINE_OCR_MODELS`` around each test.

    ``_ensure_ocr_models_env`` writes ``os.environ`` directly (not via
    monkeypatch), so monkeypatch's teardown cannot undo that write — this fixture
    guarantees the env is restored so the helper's mutation never leaks into other
    test modules (e.g. the paddle e2e tests that rely on the in-repo dev fallback)."""
    sentinel = object()
    saved = _doc.os.environ.get(_ENV, sentinel)
    try:
        yield
    finally:
        if saved is sentinel:
            _doc.os.environ.pop(_ENV, None)
        else:
            _doc.os.environ[_ENV] = saved


# --- OCR-MODELS-WIRING-000: the wheel-bundled _models tier wins (the default) -


def test_ensure_ocr_models_env_sets_from_bundled(monkeypatch, tmp_path):
    """With no env preset, the helper exports the wheel-bundled ``pdfspine/_models``
    directory — even if a companion is also importable (bundled wins)."""
    monkeypatch.delenv(_ENV, raising=False)
    monkeypatch.setattr(_doc, "_bundled_models_dir", lambda: str(tmp_path))

    # A fake companion that, were it consulted, would win — it must NOT be.
    fake = types.ModuleType("pdfspine_ocr_models")
    fake.models_dir = lambda: "/companion/dir"  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "pdfspine_ocr_models", fake)

    _doc._ensure_ocr_models_env()

    assert _doc.os.environ.get(_ENV) == str(tmp_path)


# --- OCR-MODELS-WIRING-001: helper sets env from an importable companion -----


def test_ensure_ocr_models_env_sets_from_companion(monkeypatch, tmp_path):
    """With no env preset, no wheel-bundled models, and the companion importable,
    the helper exports the companion's ``models_dir()`` as ``PDFSPINE_OCR_MODELS``."""
    monkeypatch.delenv(_ENV, raising=False)
    monkeypatch.setattr(_doc, "_bundled_models_dir", lambda: None)  # no bundled tier

    # A fake `pdfspine_ocr_models` so the test needs no real 15 MB install.
    fake = types.ModuleType("pdfspine_ocr_models")
    fake.models_dir = lambda: str(tmp_path)  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "pdfspine_ocr_models", fake)

    _doc._ensure_ocr_models_env()

    assert _doc.os.environ.get(_ENV) == str(tmp_path)


# --- OCR-MODELS-WIRING-002: a preset env is an override, never clobbered -----


def test_ensure_ocr_models_env_respects_preset(monkeypatch, tmp_path):
    """If the user already set ``PDFSPINE_OCR_MODELS``, the helper leaves it as-is
    even when bundled models / a companion exist (explicit override wins)."""
    monkeypatch.setenv(_ENV, "/user/override/dir")
    monkeypatch.setattr(_doc, "_bundled_models_dir", lambda: str(tmp_path))

    fake = types.ModuleType("pdfspine_ocr_models")
    fake.models_dir = lambda: str(tmp_path)  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "pdfspine_ocr_models", fake)

    _doc._ensure_ocr_models_env()

    assert _doc.os.environ.get(_ENV) == "/user/override/dir"


# --- OCR-MODELS-WIRING-003: no bundled + no companion + no env -> no-op -------


def test_ensure_ocr_models_env_noop_without_companion(monkeypatch):
    """With no preset env, no wheel-bundled models, and no importable companion,
    the helper does nothing (leaving the Rust dev fallback / clear error to take
    over)."""
    monkeypatch.delenv(_ENV, raising=False)
    monkeypatch.setattr(_doc, "_bundled_models_dir", lambda: None)  # no bundled tier
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
