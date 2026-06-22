"""``pdfspine-ocr-models`` — the PP-OCRv5 ONNX weights for pdfspine's PaddleOCR.

This is a pure-data companion distribution (the `pdfspine[ocr]` extra depends on
it). The `pdfspine` wheel has the OCR *code* compiled in but ships no models;
this package supplies the 3 ONNX weights and exposes :func:`models_dir`, which
pdfspine reads at runtime to set ``PDFSPINE_OCR_MODELS`` for the Rust engine.

Models: PP-OCRv5 detection/recognition + PP-LCNet text-line-orientation classifier,
redistributed from PaddleOCR under Apache-2.0. See ``PROVENANCE.md`` / ``NOTICE``
in this package.
"""

from __future__ import annotations

import os
from importlib import resources

__all__ = ["models_dir", "__version__"]

try:
    from importlib.metadata import version as _pkg_version

    __version__ = _pkg_version("pdfspine-ocr-models")
except Exception:  # pragma: no cover - source tree without dist metadata
    __version__ = "0.0.0"

# The 3 ONNX weights that pdfspine's PaddleOCR engine loads at runtime.
_ONNX_FILES = ("ppocrv5_det.onnx", "ppocrv5_rec.onnx", "ppocrv5_cls.onnx")


def models_dir() -> str:
    """Return the absolute path of the directory holding the 3 PP-OCRv5 ONNX models.

    The directory is guaranteed to contain ``ppocrv5_det.onnx``,
    ``ppocrv5_rec.onnx`` and ``ppocrv5_cls.onnx``. pdfspine passes this to the
    Rust engine via the ``PDFSPINE_OCR_MODELS`` environment variable.
    """
    # `resources.files(__package__)` is the installed package directory; the ONNX
    # live directly inside it (placed there by the build hook). Materialize a real
    # filesystem path (no zip import here — wheels install unpacked).
    pkg_dir = resources.files(__package__)
    with resources.as_file(pkg_dir) as path:
        directory = os.fspath(path)
    missing = [f for f in _ONNX_FILES if not os.path.isfile(os.path.join(directory, f))]
    if missing:  # pragma: no cover - corrupt/partial install
        raise FileNotFoundError(
            f"pdfspine-ocr-models: ONNX model(s) missing from {directory}: "
            f"{', '.join(missing)}. Reinstall with `pip install --force-reinstall "
            "pdfspine-ocr-models`."
        )
    return directory
